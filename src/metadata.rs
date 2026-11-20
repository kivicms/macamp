//! Track metadata and the background tag reader.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use egui::Context;
use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::tag::Accessor;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

pub const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "flac", "wav", "m4a", "aac", "ogg"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: Option<String>,
    pub duration_ms: Option<u64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub bitrate_kbps: Option<u32>,
    pub codec: String,
}

impl Track {
    pub fn load(path: PathBuf) -> Result<Self, String> {
        let fallback_title = path
            .file_stem()
            .map(|s| s.to_string_lossy().trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Untitled".to_owned());

        let tagged = lofty::read_from_path(&path).map_err(|e| format!("{e}"))?;
        let props = tagged.properties();
        let duration = props.duration();
        let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

        let title = tag
            .and_then(|t| t.title())
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback_title);
        let artist = tag
            .and_then(|t| t.artist())
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_default();
        let album = tag
            .and_then(|t| t.album())
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_default();
        let year = tag.and_then(|t| t.date()).map(|date| date.year.to_string());
        let codec = codec_name(tagged.file_type());

        Ok(Self {
            path,
            title,
            artist,
            album,
            year,
            duration_ms: Some(duration.as_millis() as u64),
            sample_rate: props.sample_rate(),
            channels: props.channels(),
            bitrate_kbps: props.audio_bitrate(),
            codec,
        })
    }

    pub fn display_title(&self) -> String {
        if self.artist.is_empty() {
            self.title.clone()
        } else {
            format!("{} — {}", self.artist, self.title)
        }
    }

    pub fn duration(&self) -> Option<Duration> {
        self.duration_ms.map(Duration::from_millis)
    }

    pub fn sample_rate_label(&self) -> String {
        match self.sample_rate {
            Some(rate) => {
                if rate % 1000 == 0 {
                    format!("{} kHz", rate / 1000)
                } else {
                    format!("{:.1} kHz", rate as f32 / 1000.0)
                }
            }
            None => "— kHz".to_owned(),
        }
    }
}

fn codec_name(ty: FileType) -> String {
    match ty {
        FileType::Mpeg => "MP3".to_owned(),
        FileType::Flac => "FLAC".to_owned(),
        FileType::Wav => "WAV".to_owned(),
        FileType::Mp4 => "M4A".to_owned(),
        FileType::Aac => "AAC".to_owned(),
        FileType::Vorbis => "OGG".to_owned(),
        other => format!("{other:?}").to_uppercase(),
    }
}

pub fn is_supported_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|supported| ext.eq_ignore_ascii_case(supported))
        })
}

pub fn collect_audio_files(path: &Path) -> Vec<PathBuf> {
    if is_supported_audio(path) {
        return vec![path.to_path_buf()];
    }
    if path.is_dir() {
        let mut files: Vec<PathBuf> = WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.into_path())
            .filter(|path| is_supported_audio(path))
            .collect();
        files.sort();
        files
    } else {
        Vec::new()
    }
}

#[derive(Debug)]
pub enum LoaderEvent {
    Loaded(Track),
    Failed(String),
    BatchStarted(usize),
    BatchFinished,
}

pub struct TrackLoader {
    request_tx: Sender<Vec<PathBuf>>,
    event_rx: Receiver<LoaderEvent>,
}

impl TrackLoader {
    pub fn new(repaint: Context) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<Vec<PathBuf>>();
        let (event_tx, event_rx) = mpsc::channel::<LoaderEvent>();
        thread::Builder::new()
            .name("macamp-tags".to_owned())
            .spawn(move || {
                for request in request_rx {
                    let mut paths = Vec::new();
                    for path in request {
                        paths.extend(collect_audio_files(&path));
                    }
                    paths.sort();
                    paths.dedup();
                    let _ = event_tx.send(LoaderEvent::BatchStarted(paths.len()));
                    repaint.request_repaint();
                    for path in paths {
                        match Track::load(path.clone()) {
                            Ok(track) => {
                                if event_tx.send(LoaderEvent::Loaded(track)).is_err() {
                                    return;
                                }
                                repaint.request_repaint();
                            }
                            Err(error) => {
                                let name = path
                                    .file_name()
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| path.display().to_string());
                                let _ =
                                    event_tx.send(LoaderEvent::Failed(format!("{name}: {error}")));
                                repaint.request_repaint();
                            }
                        }
                    }
                    let _ = event_tx.send(LoaderEvent::BatchFinished);
                    repaint.request_repaint();
                }
            })
            .expect("spawn metadata reader");
        Self {
            request_tx,
            event_rx,
        }
    }

    pub fn request<P: Into<PathBuf>>(&self, paths: impl IntoIterator<Item = P>) {
        let paths: Vec<PathBuf> = paths.into_iter().map(Into::into).collect();
        if !paths.is_empty() {
            let _ = self.request_tx.send(paths);
        }
    }

    pub fn poll(&self) -> Vec<LoaderEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_formats_are_case_insensitive() {
        assert!(is_supported_audio(Path::new("/tmp/song.MP3")));
        assert!(is_supported_audio(Path::new("/tmp/song.flac")));
        assert!(!is_supported_audio(Path::new("/tmp/cover.png")));
        assert!(!is_supported_audio(Path::new("/tmp/song.opus")));
    }

    #[test]
    fn display_title_omits_empty_artist() {
        let track = Track {
            path: PathBuf::from("/tmp/a.flac"),
            title: "A".into(),
            artist: String::new(),
            album: String::new(),
            year: None,
            duration_ms: Some(1),
            sample_rate: None,
            channels: None,
            bitrate_kbps: None,
            codec: "FLAC".into(),
        };
        assert_eq!(track.display_title(), "A");
    }
}
