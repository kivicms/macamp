//! A small real-time spectrum analyzer fed by the audio tap.

use std::collections::VecDeque;
use std::f32::consts::PI;
use std::sync::Arc;
use std::time::{Duration, Instant};

use realfft::num_complex::Complex32;
use realfft::{RealFftPlanner, RealToComplex};

pub const BANDS: usize = 30;
const FFT_LEN: usize = 1024;
const HISTORY: usize = 4096;

pub struct Spectrum {
    samples: VecDeque<f32>,
    levels: [f32; BANDS],
    fft: Arc<dyn RealToComplex<f32>>,
    input: Vec<f32>,
    output: Vec<Complex32>,
    last_analysis: Instant,
    sample_rate: f32,
}

impl Spectrum {
    pub fn new() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_LEN);
        let input = fft.make_input_vec();
        let output = fft.make_output_vec();
        Self {
            samples: VecDeque::with_capacity(HISTORY),
            levels: [0.0; BANDS],
            fft,
            input,
            output,
            last_analysis: Instant::now() - Duration::from_secs(1),
            sample_rate: 44_100.0,
        }
    }

    pub fn push(&mut self, samples: &[f32], sample_rate: Option<u32>) {
        if let Some(rate) = sample_rate {
            if rate > 0 {
                self.sample_rate = rate as f32;
            }
        }
        self.samples.extend(samples.iter().copied());
        if self.samples.len() > HISTORY {
            let excess = self.samples.len() - HISTORY;
            self.samples.drain(..excess);
        }
    }

    pub fn analyze(&mut self, now: Instant) {
        if now.duration_since(self.last_analysis) < Duration::from_millis(33) {
            return;
        }
        self.last_analysis = now;
        if self.samples.len() < FFT_LEN / 4 {
            self.decay();
            return;
        }

        let pad = FFT_LEN.saturating_sub(self.samples.len());
        for i in 0..FFT_LEN {
            let idx = i as isize - pad as isize;
            let sample = if idx >= 0 {
                self.samples.get(idx as usize).copied().unwrap_or(0.0)
            } else {
                0.0
            };
            let window = 0.5 - 0.5 * (2.0 * PI * i as f32 / (FFT_LEN - 1) as f32).cos();
            self.input[i] = sample * window;
        }
        if self.fft.process(&mut self.input, &mut self.output).is_err() {
            self.decay();
            return;
        }

        let nyquist = self.sample_rate * 0.5;
        let f_min = 40.0_f32;
        let f_max = 16_000.0_f32.min(nyquist * 0.94);
        let ratio = (f_max / f_min).powf(1.0 / BANDS as f32);
        let bin_hz = self.sample_rate / FFT_LEN as f32;

        for band in 0..BANDS {
            let low = f_min * ratio.powi(band as i32);
            let high = f_min * ratio.powi(band as i32 + 1);
            let mut bin_low = (low / bin_hz).floor() as usize;
            let mut bin_high = (high / bin_hz).ceil() as usize;
            bin_low = bin_low.clamp(1, self.output.len().saturating_sub(1));
            bin_high = bin_high.clamp(bin_low + 1, self.output.len());
            let mut peak = 0.0_f32;
            for bin in self.output.iter().take(bin_high).skip(bin_low) {
                peak = peak.max(bin.norm());
            }
            let normalized = peak / (FFT_LEN as f32 * 0.25);
            let db = 20.0 * (normalized + 1e-6).log10();
            let target = (((db + 54.0) / 54.0).clamp(0.0, 1.0)).powf(0.72);
            let current = self.levels[band];
            self.levels[band] = if target > current {
                target
            } else {
                (current * 0.82).max(target)
            };
        }
    }

    pub fn decay(&mut self) {
        for level in &mut self.levels {
            *level *= 0.82;
            if *level < 0.002 {
                *level = 0.0;
            }
        }
    }

    pub fn reset(&mut self) {
        self.samples.clear();
        self.levels = [0.0; BANDS];
    }

    pub fn levels(&self) -> &[f32; BANDS] {
        &self.levels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: f32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| (2.0 * PI * freq * i as f32 / rate).sin() * 0.8)
            .collect()
    }

    #[test]
    fn a_strong_sine_lights_a_band() {
        let mut spectrum = Spectrum::new();
        let rate = 44_100.0;
        spectrum.push(&sine(1_000.0, rate, 4096), Some(rate as u32));
        // Force analysis by moving time forward.
        spectrum.last_analysis = Instant::now() - Duration::from_secs(1);
        spectrum.analyze(Instant::now());
        let max = spectrum
            .levels()
            .iter()
            .copied()
            .fold(0.0_f32, |a, b| a.max(b));
        assert!(max > 0.5, "levels: {:?}", spectrum.levels());
    }

    #[test]
    fn decays_toward_zero() {
        let mut spectrum = Spectrum::new();
        spectrum.levels = [1.0; BANDS];
        for _ in 0..80 {
            spectrum.decay();
        }
        assert!(spectrum.levels.iter().all(|l| *l == 0.0));
    }
}
