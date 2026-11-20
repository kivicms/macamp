//! macOS Now Playing and media-key bridge.

use std::sync::mpsc::{self, Receiver};

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};

use crate::metadata::Track;
use egui::Context;

pub struct MediaBridge {
    controls: MediaControls,
    events: Receiver<MediaControlEvent>,
}

impl MediaBridge {
    pub fn new(repaint: Context) -> Option<Self> {
        let mut controls = match MediaControls::new(PlatformConfig {
            display_name: "MacAmp",
            dbus_name: "macamp",
            hwnd: None,
        }) {
            Ok(controls) => controls,
            Err(error) => {
                tracing::warn!("could not create media controls: {error}");
                return None;
            }
        };
        let (sender, events) = mpsc::channel();
        if let Err(error) = controls.attach(move |event| {
            let _ = sender.send(event);
            repaint.request_repaint();
        }) {
            tracing::warn!("could not attach media controls: {error}");
            return None;
        }
        tracing::info!("media controls ready (Now Playing + media keys)");
        Some(Self { controls, events })
    }

    pub fn poll(&self) -> Vec<MediaControlEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            events.push(event);
        }
        events
    }

    pub fn update(&mut self, track: Option<&Track>, playback: MediaPlayback) {
        let metadata = if let Some(track) = track {
            MediaMetadata {
                title: Some(track.title.as_str()),
                album: (!track.album.is_empty()).then_some(track.album.as_str()),
                artist: (!track.artist.is_empty()).then_some(track.artist.as_str()),
                cover_url: None,
                duration: track.duration(),
            }
        } else {
            MediaMetadata::default()
        };
        if let Err(error) = self.controls.set_metadata(metadata) {
            tracing::debug!("could not update now-playing metadata: {error}");
        }
        if let Err(error) = self.controls.set_playback(playback) {
            tracing::debug!("could not update now-playing state: {error}");
        }
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        #[cfg(target_os = "macos")]
        {
            if let Err(error) = self.controls.set_shuffle(Some(shuffle)) {
                tracing::debug!("could not update now-playing shuffle: {error}");
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = shuffle;
        }
    }

    pub fn set_progress(&mut self, position: std::time::Duration, playing: bool) {
        let playback = if playing {
            MediaPlayback::Playing {
                progress: Some(MediaPosition(position)),
            }
        } else {
            MediaPlayback::Paused {
                progress: Some(MediaPosition(position)),
            }
        };
        if let Err(error) = self.controls.set_playback(playback) {
            tracing::debug!("could not update now-playing progress: {error}");
        }
    }
}
