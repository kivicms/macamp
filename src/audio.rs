//! The playback engine.
//!
//! One OS output stream lives for the whole app. Tracks are decoded lazily and
//! queued on a single rodio `Player`, which gives gapless transitions and keeps
//! device setup out of the hot path. A small DSP wrapper handles balance and a
//! downmixed tap used by the spectrum display.

use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::{
    AtomicBool, AtomicI16, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use biquad::{Biquad, Coefficients, DirectForm2Transposed, ToHertz, Type};
use rodio::source::SeekError;
use rodio::{
    ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate, Source,
};

use crate::metadata::Track;

const TAP_CHUNK: usize = 512;
const TAP_KEEP: usize = 8192;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("не удалось открыть аудиоустройство: {0}")]
    Device(String),
    #[error("{0}")]
    Track(String),
}

#[derive(Debug)]
pub struct PlaybackShared {
    pub order_pos: AtomicUsize,
    pub serial: AtomicU64,
}

impl PlaybackShared {
    fn new() -> Self {
        Self {
            order_pos: AtomicUsize::new(usize::MAX),
            serial: AtomicU64::new(0),
        }
    }

    pub fn current(&self) -> Option<(usize, u64)> {
        let order_pos = self.order_pos.load(Ordering::Acquire);
        if order_pos == usize::MAX {
            None
        } else {
            Some((order_pos, self.serial.load(Ordering::Acquire)))
        }
    }

    fn clear(&self) {
        self.order_pos.store(usize::MAX, Ordering::Release);
    }
}

pub struct DspSettings {
    /// -100..=100, negative is left, positive is right.
    balance: AtomicI32,
    eq_enabled: AtomicBool,
    eq_gains: [AtomicI16; EQ_BANDS],
    eq_preamp: AtomicI16,
    eq_generation: AtomicU32,
}

pub const EQ_BANDS: usize = 10;
pub const EQ_FREQUENCIES: [f32; EQ_BANDS] = [
    60.0, 170.0, 310.0, 600.0, 1_000.0, 3_000.0, 6_000.0, 12_000.0, 14_000.0, 16_000.0,
];

impl DspSettings {
    pub fn new() -> Self {
        Self {
            balance: AtomicI32::new(0),
            eq_enabled: AtomicBool::new(false),
            eq_gains: std::array::from_fn(|_| AtomicI16::new(0)),
            eq_preamp: AtomicI16::new(0),
            eq_generation: AtomicU32::new(1),
        }
    }

    pub fn set_eq(&self, enabled: bool, gains: [f32; EQ_BANDS], preamp: f32) {
        self.eq_enabled.store(enabled, Ordering::Release);
        for (slot, gain) in self.eq_gains.iter().zip(gains) {
            slot.store(
                (gain.clamp(-12.0, 12.0) * 10.0).round() as i16,
                Ordering::Relaxed,
            );
        }
        self.eq_preamp.store(
            (preamp.clamp(-12.0, 12.0) * 10.0).round() as i16,
            Ordering::Relaxed,
        );
        self.eq_generation.fetch_add(1, Ordering::Release);
    }

    fn eq_generation(&self) -> u32 {
        self.eq_generation.load(Ordering::Acquire)
    }

    fn eq_snapshot(&self) -> (bool, [f32; EQ_BANDS], f32, u32) {
        let mut gains = [0.0_f32; EQ_BANDS];
        for (gain, slot) in gains.iter_mut().zip(&self.eq_gains) {
            *gain = slot.load(Ordering::Relaxed) as f32 / 10.0;
        }
        let preamp = self.eq_preamp.load(Ordering::Relaxed) as f32 / 10.0;
        (
            self.eq_enabled.load(Ordering::Acquire),
            gains,
            preamp,
            self.eq_generation.load(Ordering::Acquire),
        )
    }

    pub fn balance(&self) -> f32 {
        self.balance.load(Ordering::Relaxed) as f32 / 100.0
    }

    pub fn set_balance(&self, balance: f32) {
        self.balance.store(
            (balance.clamp(-1.0, 1.0) * 100.0).round() as i32,
            Ordering::Relaxed,
        );
    }

    fn channel_gain(&self, channel: usize) -> f32 {
        let b = self.balance();
        match channel {
            0 if b > 0.0 => 1.0 - b,
            1 if b < 0.0 => 1.0 + b,
            _ => 1.0,
        }
    }
}

pub struct TapBuffer {
    samples: Mutex<Vec<f32>>,
}

impl TapBuffer {
    fn new() -> Self {
        Self {
            samples: Mutex::new(Vec::with_capacity(TAP_KEEP)),
        }
    }

    fn push(&self, chunk: &[f32]) {
        if chunk.is_empty() {
            return;
        }
        if let Ok(mut samples) = self.samples.lock() {
            if samples.len() + chunk.len() > TAP_KEEP {
                let drop = samples.len() + chunk.len() - TAP_KEEP;
                let drop = drop.min(samples.len());
                samples.drain(..drop);
            }
            samples.extend_from_slice(chunk);
        }
    }

    /// Takes everything the tap has captured since the last call.
    pub fn take(&self) -> Vec<f32> {
        self.samples
            .lock()
            .map(|mut samples| std::mem::take(&mut *samples))
            .unwrap_or_default()
    }
}

struct EqChain {
    filters: Vec<Vec<DirectForm2Transposed<f32>>>,
    enabled: bool,
    preamp_gain: f32,
    generation: u32,
    sample_rate: f32,
    channels: usize,
}

fn identity_coefficients() -> Coefficients<f32> {
    Coefficients {
        a1: 0.0,
        a2: 0.0,
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
    }
}

impl EqChain {
    fn new(sample_rate: f32, channels: usize, dsp: &DspSettings) -> Self {
        let filters = (0..EQ_BANDS)
            .map(|_| {
                (0..channels)
                    .map(|_| DirectForm2Transposed::new(identity_coefficients()))
                    .collect()
            })
            .collect();
        let mut chain = Self {
            filters,
            enabled: false,
            preamp_gain: 1.0,
            generation: 0,
            sample_rate,
            channels,
        };
        chain.refresh(dsp);
        chain
    }

    fn refresh(&mut self, dsp: &DspSettings) {
        let (enabled, gains, preamp, generation) = dsp.eq_snapshot();
        self.enabled = enabled;
        self.generation = generation;
        self.preamp_gain = 10.0_f32.powf(preamp / 20.0);
        for band in 0..EQ_BANDS {
            let frequency = EQ_FREQUENCIES[band].min(self.sample_rate * 0.45);
            let coefficients = Coefficients::<f32>::from_params(
                Type::PeakingEQ(gains[band]),
                self.sample_rate.hz(),
                frequency.hz(),
                1.2,
            )
            .unwrap_or_else(|_| identity_coefficients());
            for channel in 0..self.channels {
                self.filters[band][channel].update_coefficients(coefficients);
            }
        }
    }

    fn process(&mut self, sample: f32, channel: usize) -> f32 {
        if !self.enabled {
            return sample;
        }
        let mut output = sample;
        let channel = channel.min(self.channels.saturating_sub(1));
        for band in &mut self.filters {
            output = band[channel].run(output);
        }
        output * self.preamp_gain
    }

    fn reset(&mut self) {
        for band in &mut self.filters {
            for filter in band {
                filter.reset_state();
            }
        }
    }
}

pub struct Engine {
    player: Player,
    _device: MixerDeviceSink,
    shared: Arc<PlaybackShared>,
    tap: Arc<TapBuffer>,
    dsp: Arc<DspSettings>,
}

impl Engine {
    pub fn new(dsp: Arc<DspSettings>) -> Result<Self, AudioError> {
        let builder = DeviceSinkBuilder::from_default_device().map_err(|error| {
            tracing::error!("default audio device error: {error:?}");
            AudioError::Device(error.to_string())
        })?;
        let mut device = builder.open_sink_or_fallback().map_err(|error| {
            tracing::error!("audio stream fallback error: {error:?}");
            AudioError::Device(error.to_string())
        })?;
        device.log_on_drop(false);
        tracing::info!("audio output: {:?}", device.config());
        let player = Player::connect_new(device.mixer());
        Ok(Self {
            player,
            _device: device,
            shared: Arc::new(PlaybackShared::new()),
            tap: Arc::new(TapBuffer::new()),
            dsp,
        })
    }

    pub fn tap(&self) -> &Arc<TapBuffer> {
        &self.tap
    }

    pub fn start(&self, track: &Track, order_pos: usize, serial: u64) -> Result<(), AudioError> {
        self.player.clear();
        self.shared.clear();
        let source = TrackSource::open(
            track,
            order_pos,
            serial,
            self.shared.clone(),
            self.tap.clone(),
            self.dsp.clone(),
        )?;
        self.player.append(source);
        self.player.play();
        Ok(())
    }

    pub fn append(&self, track: &Track, order_pos: usize, serial: u64) -> Result<(), AudioError> {
        let source = TrackSource::open(
            track,
            order_pos,
            serial,
            self.shared.clone(),
            self.tap.clone(),
            self.dsp.clone(),
        )?;
        self.player.append(source);
        Ok(())
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn resume(&self) {
        self.player.play();
    }

    pub fn stop(&self) {
        self.player.stop();
        self.player.pause();
        self.shared.clear();
    }

    pub fn clear(&self) {
        self.player.clear();
        self.shared.clear();
    }

    pub fn skip_one(&self) {
        self.player.skip_one();
    }

    pub fn seek(&self, position: Duration) -> Result<(), SeekError> {
        self.player.try_seek(position)
    }

    pub fn position(&self) -> Duration {
        self.player.get_pos()
    }

    pub fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume.clamp(0.0, 1.0));
    }

    pub fn empty(&self) -> bool {
        self.player.empty()
    }

    pub fn current(&self) -> Option<(usize, u64)> {
        self.shared.current()
    }
}

struct TrackSource {
    inner: Decoder<BufReader<File>>,
    order_pos: usize,
    serial: u64,
    shared: Arc<PlaybackShared>,
    tap: Arc<TapBuffer>,
    dsp: Arc<DspSettings>,
    channels: usize,
    channel_index: usize,
    mono_sum: f32,
    tap_chunk: Vec<f32>,
    started: bool,
    eq: EqChain,
    dsp_counter: u32,
}

impl TrackSource {
    fn open(
        track: &Track,
        order_pos: usize,
        serial: u64,
        shared: Arc<PlaybackShared>,
        tap: Arc<TapBuffer>,
        dsp: Arc<DspSettings>,
    ) -> Result<Self, AudioError> {
        let file = File::open(&track.path)
            .map_err(|e| AudioError::Track(format!("{}: {e}", track.path.display())))?;
        let inner = Decoder::new(BufReader::new(file))
            .map_err(|e| AudioError::Track(format!("{}: {e}", track.path.display())))?;
        let channels = inner.channels().get() as usize;
        let sample_rate = inner.sample_rate().get() as f32;
        let eq = EqChain::new(sample_rate, channels, &dsp);
        Ok(Self {
            inner,
            order_pos,
            serial,
            shared,
            tap,
            dsp,
            channels,
            channel_index: 0,
            mono_sum: 0.0,
            tap_chunk: Vec::with_capacity(TAP_CHUNK),
            started: false,
            eq,
            dsp_counter: 0,
        })
    }

    fn flush_tap(&mut self) {
        if !self.tap_chunk.is_empty() {
            self.tap.push(&self.tap_chunk);
            self.tap_chunk.clear();
        }
    }
}

impl Iterator for TrackSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;
        if !self.started {
            self.started = true;
            self.shared
                .order_pos
                .store(self.order_pos, Ordering::Release);
            self.shared.serial.store(self.serial, Ordering::Release);
        }

        self.dsp_counter = self.dsp_counter.wrapping_add(1);
        if self.dsp_counter >= 512 {
            self.dsp_counter = 0;
            if self.eq.generation != self.dsp.eq_generation() {
                self.eq.refresh(&self.dsp);
            }
        }
        let channel = self.channel_index;
        let is_first = channel == 0;
        let is_last = channel + 1 == self.channels;
        let gain = if is_first || is_last {
            self.dsp.channel_gain(channel)
        } else {
            1.0
        };
        let output = self.eq.process(sample, channel) * gain;

        self.mono_sum += output;
        self.channel_index += 1;
        if self.channel_index == self.channels {
            self.tap_chunk.push(self.mono_sum / self.channels as f32);
            self.mono_sum = 0.0;
            self.channel_index = 0;
            if self.tap_chunk.len() >= TAP_CHUNK {
                self.flush_tap();
            }
        }
        Some(output)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl Source for TrackSource {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(pos)?;
        self.channel_index = 0;
        self.mono_sum = 0.0;
        self.eq.reset();
        self.flush_tap();
        Ok(())
    }
}

impl Drop for TrackSource {
    fn drop(&mut self) {
        self.flush_tap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    fn run_sine(dsp: &DspSettings, frequency: f32, gain_db: f32) -> f32 {
        dsp.set_eq(
            true,
            [0.0, 0.0, 0.0, 0.0, gain_db, 0.0, 0.0, 0.0, 0.0, 0.0],
            0.0,
        );
        let mut chain = EqChain::new(44_100.0, 1, dsp);
        let mut output = Vec::new();
        for i in 0..8_000 {
            let phase = 2.0 * std::f32::consts::PI * frequency * i as f32 / 44_100.0;
            let sample = phase.sin() * 0.2;
            let filtered = chain.process(sample, 0);
            if i > 4_000 {
                output.push(filtered);
            }
        }
        rms(&output)
    }

    #[test]
    fn a_peaking_band_boosts_its_frequency() {
        let dsp = DspSettings::new();
        let baseline = run_sine(&dsp, 1_000.0, 0.0);
        let boosted = run_sine(&dsp, 1_000.0, 12.0);
        assert!(
            boosted > baseline * 1.5,
            "baseline {baseline}, boosted {boosted}"
        );
    }

    #[test]
    fn disabled_eq_is_transparent() {
        let dsp = DspSettings::new();
        let mut chain = EqChain::new(44_100.0, 1, &dsp);
        for i in 0..100 {
            let sample = (i as f32 / 100.0).sin() * 0.1;
            assert!((chain.process(sample, 0) - sample).abs() < 1e-6);
        }
    }
}
