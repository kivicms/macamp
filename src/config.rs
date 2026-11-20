//! Durable session state and app paths.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::metadata::Track;
use crate::theme::{
    Skin, PLAYLIST_H, PLAYLIST_MAX_H, PLAYLIST_MAX_W, PLAYLIST_MIN_H, PLAYLIST_MIN_W, PLAYLIST_W,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn is_on(self) -> bool {
        !matches!(self, Self::Off)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "выкл",
            Self::All => "всё",
            Self::One => "один трек",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub tracks: Vec<Track>,
    pub current: Option<usize>,
    pub position_ms: u64,
    pub volume: f32,
    pub balance: f32,
    pub skin: Skin,
    pub show_playlist: bool,
    pub playlist_pos: Option<[f32; 2]>,
    pub playlist_size: [f32; 2],
    pub show_drive: bool,
    pub drive_pos: Option<[f32; 2]>,
    pub main_pos: Option<[f32; 2]>,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub always_on_top: bool,
    pub shaded: bool,
    pub double_size: bool,
    pub show_eq: bool,
    pub eq_enabled: bool,
    pub eq_gains: [f32; 10],
    pub eq_preset: Option<usize>,
    pub eq_pos: Option<[f32; 2]>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            tracks: Vec::new(),
            current: None,
            position_ms: 0,
            volume: 0.82,
            balance: 0.0,
            skin: Skin::Graphite,
            show_playlist: true,
            playlist_pos: None,
            playlist_size: [PLAYLIST_W, PLAYLIST_H],
            show_drive: false,
            drive_pos: None,
            main_pos: None,
            repeat: RepeatMode::Off,
            shuffle: false,
            always_on_top: false,
            shaded: false,
            double_size: false,
            show_eq: false,
            eq_enabled: false,
            eq_gains: [7.0, 6.0, 5.0, 3.0, 0.0, -1.0, 0.0, 2.0, 5.0, 7.0],
            eq_preset: Some(1),
            eq_pos: None,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Self>(&text) {
                Ok(mut config) => {
                    config.normalize();
                    config
                }
                Err(error) => {
                    tracing::warn!("could not parse {}: {error}", path.display());
                    let _ = fs::rename(&path, path.with_extension("json.bad"));
                    Self::default()
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                tracing::warn!("could not read {}: {error}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let Some(path) = config_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    pub(crate) fn normalize(&mut self) {
        self.volume = self.volume.clamp(0.0, 1.0);
        self.balance = self.balance.clamp(-1.0, 1.0);
        if let Some(current) = self.current {
            if current >= self.tracks.len() {
                self.current = if self.tracks.is_empty() {
                    None
                } else {
                    Some(self.tracks.len() - 1)
                };
            }
        }
        for gain in &mut self.eq_gains {
            *gain = gain.clamp(-12.0, 12.0);
        }
        if !self.playlist_size[0].is_finite() || !self.playlist_size[1].is_finite() {
            self.playlist_size = [PLAYLIST_W, PLAYLIST_H];
        }
        self.playlist_size[0] = self.playlist_size[0].clamp(PLAYLIST_MIN_W, PLAYLIST_MAX_W);
        self.playlist_size[1] = self.playlist_size[1].clamp(PLAYLIST_MIN_H, PLAYLIST_MAX_H);
        if self.eq_preset.is_some_and(|preset| preset >= 6) {
            self.eq_preset = None;
        }
    }
}

pub fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("com", "macamp", "MacAmp")
}

pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("MACAMP_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    project_dirs().map(|dirs| dirs.data_dir().to_path_buf())
}

pub fn config_path() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("config.json"))
}

pub fn log_dir() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("logs"))
}

pub fn ensure_dirs() -> io::Result<()> {
    if let Some(dir) = data_dir() {
        fs::create_dir_all(dir.join("logs"))?;
    }
    Ok(())
}

pub fn init_logging() -> tracing_appender::non_blocking::WorkerGuard {
    let (writer, guard) = if let Some(dir) = log_dir() {
        let _ = fs::create_dir_all(&dir);
        let file = tracing_appender::rolling::never(dir, "macamp.log");
        tracing_appender::non_blocking(file)
    } else {
        tracing_appender::non_blocking(io::sink())
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("macamp=info,warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false)
        .init();
    guard
}

pub fn config_dir_display() -> String {
    config_path()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "Application Support".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::Track;
    use std::path::PathBuf;

    #[test]
    fn repeat_mode_cycles_and_wraps() {
        assert_eq!(RepeatMode::Off.cycle(), RepeatMode::All);
        assert_eq!(RepeatMode::All.cycle(), RepeatMode::One);
        assert_eq!(RepeatMode::One.cycle(), RepeatMode::Off);
        assert!(!RepeatMode::Off.is_on());
        assert!(RepeatMode::All.is_on());
    }

    #[test]
    fn config_roundtrip() {
        let track = Track {
            path: PathBuf::from("/tmp/test.flac"),
            title: "Test".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            year: Some("1999".into()),
            duration_ms: Some(123_000),
            sample_rate: Some(44_100),
            channels: Some(2),
            bitrate_kbps: Some(900),
            codec: "FLAC".into(),
        };
        let config = Config {
            tracks: vec![track],
            current: Some(0),
            position_ms: 42_000,
            skin: Skin::Midnight,
            repeat: RepeatMode::All,
            shuffle: true,
            shaded: true,
            double_size: true,
            playlist_size: [640.0, 520.0],
            show_drive: true,
            drive_pos: Some([12.0, 34.0]),
            ..Default::default()
        };
        let text = serde_json::to_string(&config).unwrap();
        let parsed: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.tracks.len(), 1);
        assert_eq!(parsed.current, Some(0));
        assert_eq!(parsed.skin, Skin::Midnight);
        assert_eq!(parsed.repeat, RepeatMode::All);
        assert!(parsed.shuffle);
        assert!(parsed.shaded);
        assert!(parsed.double_size);
        assert_eq!(parsed.playlist_size, [640.0, 520.0]);
        assert!(parsed.show_drive);
        assert_eq!(parsed.drive_pos, Some([12.0, 34.0]));
    }

    #[test]
    fn normalize_fixes_bad_values() {
        let mut config = Config {
            volume: 2.0,
            balance: -4.0,
            current: Some(9),
            playlist_size: [10.0, f32::NAN],
            ..Default::default()
        };
        config.normalize();
        assert_eq!(config.volume, 1.0);
        assert_eq!(config.balance, -1.0);
        assert_eq!(config.current, None);
        assert_eq!(config.playlist_size, [PLAYLIST_W, PLAYLIST_H]);

        config.playlist_size = [10_000.0, 10.0];
        config.normalize();
        assert_eq!(config.playlist_size, [PLAYLIST_MAX_W, PLAYLIST_MIN_H]);
    }
}
