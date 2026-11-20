//! Application state and the hand-painted classic interface.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CursorIcon, Frame, Id, Key, Layout, Modifiers, Pos2,
    Rect, Response, Sense, Stroke, Ui, Vec2, ViewportBuilder, ViewportCommand, ViewportId,
};
use rand::seq::SliceRandom;
use souvlaki::{MediaControlEvent, MediaPlayback, MediaPosition, SeekDirection};

use crate::audio::{DspSettings, Engine};
use crate::config::{Config, RepeatMode};
use crate::drivemusic::{
    self, cache_path, cached_track, is_cached, ArtistHit, ArtistPage, DmEvent, DmService,
    RemoteTrack, SearchResults,
};
use crate::metadata::{LoaderEvent, Track, TrackLoader};
use crate::platform::MediaBridge;
use crate::spectrum::Spectrum;
use crate::theme::{
    bevel, child_rect, gradient_rect, inset, lerp_color, Palette, Skin, DRIVE_H, DRIVE_W, EQ_H,
    EQ_W, MAIN_H, MAIN_W, PLAYLIST_MAX_H, PLAYLIST_MAX_W, PLAYLIST_MIN_H, PLAYLIST_MIN_W, SHADE_H,
    TITLE_H,
};
use crate::widgets::{
    classic_button, draw_icon, draw_marquee, draw_scrollbar, draw_spectrum, led_text,
    scroll_to_value, slider, text_width, tiny_title_button, toggle_button, Icon,
};

const PLAYLIST_VIEWPORT: &str = "macamp_playlist";
const EQ_VIEWPORT: &str = "macamp_equalizer";
const DRIVE_VIEWPORT: &str = "macamp_drive";
const ROW_H: f32 = 22.0;
const SNAP_DIST: f32 = 12.0;
const WINDOW_MAIN: usize = 0;
const WINDOW_PLAYLIST: usize = 1;
const WINDOW_EQ: usize = 2;
const WINDOW_DRIVE: usize = 3;
const WINDOW_COUNT: usize = 4;

pub fn playlist_viewport_id() -> ViewportId {
    ViewportId::from_hash_of(PLAYLIST_VIEWPORT)
}

pub fn eq_viewport_id() -> ViewportId {
    ViewportId::from_hash_of(EQ_VIEWPORT)
}

pub fn drive_viewport_id() -> ViewportId {
    ViewportId::from_hash_of(DRIVE_VIEWPORT)
}

const EQ_PRESETS: [(&str, [f32; 10]); 6] = [
    ("Flat", [0.0; 10]),
    (
        "Full Bass & Treble",
        [7.0, 6.0, 5.0, 3.0, 0.0, -1.0, 0.0, 2.0, 5.0, 7.0],
    ),
    ("Dance", [5.0, 4.0, 1.0, 0.0, -1.0, 0.0, 2.0, 3.0, 4.0, 5.0]),
    (
        "Classical",
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -2.0, -3.0, -3.0, -4.0],
    ),
    (
        "Vocal",
        [-3.0, -2.0, -1.0, 1.0, 4.0, 5.0, 4.0, 2.0, 0.0, -1.0],
    ),
    ("Rock", [5.0, 4.0, 3.0, 1.0, -1.0, -1.0, 1.0, 3.0, 4.0, 4.0]),
];
const EQ_LABELS: [&str; 10] = [
    "60", "170", "310", "600", "1K", "3K", "6K", "12K", "14K", "16K",
];

pub struct StartupOptions {
    pub config: Config,
    pub initial_paths: Vec<PathBuf>,
    pub autoplay: bool,
    pub screenshot: Option<PathBuf>,
    pub playlist_root: bool,
    pub eq_root: bool,
    pub drive_root: bool,
}

#[derive(Clone, Copy, Debug)]
enum MenuAction {
    Skin(Skin),
    AlwaysOnTop,
    Windowshade,
    DoubleSize,
    Playlist,
    DriveMusic,
    OpenFiles,
    OpenFolder,
    About,
    Quit,
}

struct Toast {
    text: String,
    until: Instant,
    error: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DownloadIntent {
    Play,
    Add,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DriveAction {
    None,
    Open(usize),
    Play(usize),
    Add(usize),
    AddAll,
    LoadMore,
    Back,
}

enum DriveView {
    Empty,
    Artists(Vec<ArtistHit>),
    Artist(ArtistView),
}

struct ArtistView {
    name: String,
    tracks: Vec<RemoteTrack>,
    next_url: Option<String>,
    page: usize,
}

struct DriveUi {
    service: DmService,
    query: String,
    view: DriveView,
    search_artists: Vec<ArtistHit>,
    selected: Option<usize>,
    status: String,
    error: bool,
    loading: Option<String>,
    downloads: HashMap<String, Option<f32>>,
    intents: HashMap<String, DownloadIntent>,
    pending_play: Option<String>,
    scroll: f32,
    initial_search: Option<String>,
    initial_artist: Option<String>,
    frames: u32,
}

impl DriveUi {
    fn new(ctx: egui::Context) -> Self {
        let query = std::env::var("MACAMP_DRIVE_SEARCH").unwrap_or_default();
        Self {
            service: DmService::new(ctx),
            query: query.clone(),
            view: DriveView::Empty,
            search_artists: Vec::new(),
            selected: None,
            status: "ПОИСК ПО DRIVEMUSIC".to_owned(),
            error: false,
            loading: None,
            downloads: HashMap::new(),
            intents: HashMap::new(),
            pending_play: None,
            scroll: 0.0,
            initial_search: (!query.trim().is_empty()).then_some(query),
            initial_artist: std::env::var("MACAMP_DRIVE_ARTIST").ok(),
            frames: 0,
        }
    }

    fn start_search(&mut self, query: String) {
        let query = query.trim().to_owned();
        if query.is_empty() {
            self.status = "ВВЕДИ ИМЯ ИСПОЛНИТЕЛЯ".to_owned();
            self.error = true;
            return;
        }
        self.loading = Some(format!("ПОИСК · {query}"));
        self.error = false;
        self.status = "ПОИСК…".to_owned();
        self.service.search(&query);
    }

    fn open_artist(&mut self, url: String) {
        self.loading = Some("ЗАГРУЗКА АРТИСТА…".to_owned());
        self.error = false;
        self.status = "ЗАГРУЗКА…".to_owned();
        self.scroll = 0.0;
        self.service.artist(&url, false);
    }

    fn load_more(&mut self) {
        let DriveView::Artist(artist) = &self.view else {
            return;
        };
        let Some(next) = artist.next_url.clone() else {
            return;
        };
        self.loading = Some("ЗАГРУЗКА СТРАНИЦЫ…".to_owned());
        self.error = false;
        self.status = "ЗАГРУЗКА…".to_owned();
        self.service.artist(&next, true);
    }
}

pub struct MacAmpApp {
    config: Config,
    skin: Skin,
    palette: Palette,
    engine: Option<Engine>,
    media: Option<MediaBridge>,
    audio_error: Option<String>,
    dsp: Arc<DspSettings>,
    loader: TrackLoader,
    loading_batches: usize,
    loading_files: usize,

    selected: Option<usize>,
    current: Option<usize>,
    play_order: Vec<usize>,
    play_pos: usize,
    started: bool,
    playing: bool,
    position: Duration,
    serial: u64,
    last_serial: u64,
    appended: Option<(usize, u64)>,
    seek_target: Option<Duration>,
    spectrum: Spectrum,

    volume: f32,
    balance: f32,
    repeat: RepeatMode,
    shuffle: bool,
    show_playlist: bool,
    playlist_pos: Option<Pos2>,
    playlist_size: Vec2,
    playlist_resize_accum: Option<Vec2>,
    playlist_size_guard_until: Instant,
    playlist_just_opened: bool,
    main_pos: Option<Pos2>,
    always_on_top: bool,
    shaded: bool,
    double_size: bool,
    pending_zoom_resize: bool,
    restore_positions_after_zoom: bool,
    zoom_guard_until: Instant,
    rects: [Option<Rect>; WINDOW_COUNT],
    prev_rects: [Option<Rect>; WINDOW_COUNT],
    geometry_ready: bool,
    moving_last_frame: [bool; WINDOW_COUNT],
    skip_motion_frames: [u8; WINDOW_COUNT],
    show_eq: bool,
    eq_enabled: bool,
    eq_gains: [f32; 10],
    eq_preset: Option<usize>,
    eq_pos: Option<Pos2>,
    eq_just_opened: bool,
    eq_root: bool,
    show_drive: bool,
    drive_pos: Option<Pos2>,
    drive_just_opened: bool,
    drive_root: bool,
    drive: DriveUi,
    playlist_scroll: f32,
    filter: String,
    drag_track: Option<usize>,
    drag_insert: Option<usize>,
    follow_current: bool,
    marquee_clock: Instant,

    toast: Option<Toast>,
    dirty: bool,
    last_save: Instant,
    last_position_mark: Instant,

    autoplay_initial: bool,
    autoplay_done: bool,

    screenshot: Option<PathBuf>,
    playlist_root: bool,
    screenshot_requested: bool,
    playlist_frames: u32,
    frame_count: u32,
    created_at: Instant,
    pending_window_setup: bool,
}

impl MacAmpApp {
    pub fn new(cc: &eframe::CreationContext<'_>, options: StartupOptions) -> Self {
        let StartupOptions {
            mut config,
            initial_paths,
            autoplay,
            screenshot,
            playlist_root,
            eq_root,
            drive_root,
        } = options;
        config.normalize();
        let skin = config.skin;
        let palette = skin.palette();
        let dsp = Arc::new(DspSettings::new());
        dsp.set_balance(config.balance);
        let (engine, audio_error) = match Engine::new(dsp.clone()) {
            Ok(engine) => {
                engine.set_volume(config.volume);
                (Some(engine), None)
            }
            Err(error) => {
                tracing::error!("audio engine failed: {error}");
                (None, Some(error.to_string()))
            }
        };
        let media = MediaBridge::new(cc.egui_ctx.clone());

        let current = config.current.filter(|index| *index < config.tracks.len());
        let selected = current.or(if config.tracks.is_empty() {
            None
        } else {
            Some(0)
        });
        let position = if current.is_some() {
            Duration::from_millis(config.position_ms)
        } else {
            Duration::ZERO
        };
        let show_playlist = config.show_playlist;

        let loader = TrackLoader::new(cc.egui_ctx.clone());
        if !initial_paths.is_empty() {
            loader.request(initial_paths);
        }

        let mut app = Self {
            volume: config.volume,
            balance: config.balance,
            repeat: config.repeat,
            shuffle: config.shuffle,
            show_playlist,
            playlist_pos: config.playlist_pos.map(|[x, y]| pos2(x, y)),
            playlist_size: vec2(config.playlist_size[0], config.playlist_size[1]),
            playlist_resize_accum: None,
            playlist_size_guard_until: Instant::now(),
            playlist_just_opened: true,
            main_pos: config.main_pos.map(|[x, y]| pos2(x, y)),
            always_on_top: config.always_on_top,
            shaded: config.shaded,
            double_size: config.double_size,
            pending_zoom_resize: config.double_size,
            restore_positions_after_zoom: config.double_size,
            zoom_guard_until: if config.double_size {
                Instant::now() + Duration::from_millis(900)
            } else {
                Instant::now()
            },
            rects: [None; WINDOW_COUNT],
            prev_rects: [None; WINDOW_COUNT],
            geometry_ready: false,
            moving_last_frame: [false; WINDOW_COUNT],
            skip_motion_frames: [0; WINDOW_COUNT],
            show_eq: config.show_eq,
            eq_enabled: config.eq_enabled,
            eq_gains: config.eq_gains,
            eq_preset: config.eq_preset,
            eq_pos: config.eq_pos.map(|[x, y]| pos2(x, y)),
            eq_just_opened: true,
            eq_root,
            show_drive: config.show_drive,
            drive_pos: config.drive_pos.map(|[x, y]| pos2(x, y)),
            drive_just_opened: true,
            drive_root,
            drive: DriveUi::new(cc.egui_ctx.clone()),
            config,
            skin,
            palette,
            engine,
            media,
            audio_error,
            dsp,
            loader,
            loading_batches: 0,
            loading_files: 0,
            selected,
            current,
            play_order: Vec::new(),
            play_pos: 0,
            started: false,
            playing: false,
            position,
            serial: 0,
            last_serial: 0,
            appended: None,
            seek_target: None,
            spectrum: Spectrum::new(),
            playlist_scroll: 0.0,
            filter: std::env::var("MACAMP_PLAYLIST_FILTER").unwrap_or_default(),
            drag_track: None,
            drag_insert: None,
            follow_current: false,
            marquee_clock: Instant::now(),
            toast: None,
            dirty: false,
            last_save: Instant::now(),
            last_position_mark: Instant::now(),
            autoplay_initial: autoplay,
            autoplay_done: false,
            screenshot,
            playlist_root,
            screenshot_requested: false,
            playlist_frames: 0,
            frame_count: 0,
            created_at: Instant::now(),
            pending_window_setup: true,
        };
        if app.audio_error.is_some() {
            app.toast = Some(Toast {
                text: "AUDIO OFFLINE · проверь устройство вывода".to_owned(),
                until: Instant::now() + Duration::from_secs(8),
                error: true,
            });
        }
        if let Some(media) = app.media.as_mut() {
            media.set_shuffle(app.shuffle);
        }
        app.dsp.set_eq(app.eq_enabled, app.eq_gains, 0.0);
        app.rebuild_order();
        if let Some(current) = app.current {
            app.play_pos = app
                .play_order
                .iter()
                .position(|index| *index == current)
                .unwrap_or(0);
        }
        app
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn toast<S: Into<String>>(&mut self, text: S, error: bool) {
        let text = text.into();
        if error {
            tracing::warn!("{text}");
        } else {
            tracing::info!("{text}");
        }
        self.toast = Some(Toast {
            text,
            until: Instant::now() + Duration::from_secs(4),
            error,
        });
    }

    fn next_serial(&mut self) -> u64 {
        self.serial = self.serial.wrapping_add(1);
        self.serial
    }

    fn tracks(&self) -> &[Track] {
        &self.config.tracks
    }

    fn current_track(&self) -> Option<&Track> {
        self.current.and_then(|index| self.tracks().get(index))
    }

    fn display_track_index(&self) -> Option<usize> {
        self.current.or(self.selected)
    }

    fn display_track(&self) -> Option<&Track> {
        self.display_track_index()
            .and_then(|index| self.tracks().get(index))
    }

    fn total_duration(&self) -> Duration {
        self.tracks().iter().filter_map(Track::duration).sum()
    }

    fn filter_active(&self) -> bool {
        !self.filter.trim().is_empty()
    }

    /// Playlist indices that match the current filter, in playlist order.
    fn visible_indices(&self) -> Vec<usize> {
        if !self.filter_active() {
            return (0..self.tracks().len()).collect();
        }
        let needle = self.filter.trim().to_lowercase();
        self.tracks()
            .iter()
            .enumerate()
            .filter_map(|(index, track)| track_matches(track, &needle).then_some(index))
            .collect()
    }

    fn move_track(&mut self, from: usize, to: usize) {
        let len = self.tracks().len();
        if from >= len {
            return;
        }
        let to = to.min(len - 1);
        if from == to {
            return;
        }
        let track = self.config.tracks.remove(from);
        self.config.tracks.insert(to, track);
        self.current = self.current.map(|index| remap_after_move(index, from, to));
        self.selected = self.selected.map(|index| remap_after_move(index, from, to));
        for index in &mut self.play_order {
            *index = remap_after_move(*index, from, to);
        }
        self.refresh_order_playback();
        self.mark_dirty();
    }

    fn rebuild_order(&mut self) {
        let len = self.tracks().len();
        if len == 0 {
            self.play_order.clear();
            self.play_pos = 0;
            return;
        }
        let current = self.current.filter(|index| *index < len);
        let mut order: Vec<usize> = (0..len).collect();
        if self.shuffle {
            let mut rng = rand::rng();
            if let Some(current) = current {
                order.retain(|index| *index != current);
                order.shuffle(&mut rng);
                order.insert(0, current);
            } else {
                order.shuffle(&mut rng);
            }
        }
        self.play_order = order;
        self.play_pos = current
            .and_then(|current| self.play_order.iter().position(|index| *index == current))
            .unwrap_or(0);
    }

    fn sync_order_after_add(&mut self, new_index: usize) {
        if self.shuffle && !self.play_order.is_empty() {
            self.play_order.push(new_index);
        } else {
            self.rebuild_order();
        }
    }

    fn auto_next_pos(&self, pos: usize) -> Option<usize> {
        if self.play_order.is_empty() {
            return None;
        }
        if self.repeat == RepeatMode::One {
            return Some(pos);
        }
        if pos + 1 < self.play_order.len() {
            Some(pos + 1)
        } else if self.repeat == RepeatMode::All {
            Some(0)
        } else {
            None
        }
    }

    fn manual_next_pos(&self, pos: usize) -> Option<usize> {
        if self.play_order.is_empty() {
            None
        } else if pos + 1 < self.play_order.len() {
            Some(pos + 1)
        } else if self.repeat == RepeatMode::All {
            Some(0)
        } else {
            None
        }
    }

    fn manual_prev_pos(&self, pos: usize) -> Option<usize> {
        if self.play_order.is_empty() {
            None
        } else if pos > 0 {
            Some(pos - 1)
        } else if self.repeat == RepeatMode::All {
            Some(self.play_order.len() - 1)
        } else {
            None
        }
    }

    fn start_order_pos(&mut self, order_pos: usize, start_position: Duration, play: bool) -> bool {
        let Some(&track_index) = self.play_order.get(order_pos) else {
            self.toast("Не удалось определить трек в очереди", true);
            return false;
        };
        let Some(track) = self.tracks().get(track_index).cloned() else {
            self.toast("Трек исчез из плейлиста", true);
            return false;
        };
        if self.engine.is_none() {
            self.audio_error
                .get_or_insert_with(|| "Аудиоустройство недоступно".to_owned());
            self.toast("Аудиоустройство недоступно", true);
            return false;
        }
        tracing::info!(
            "starting order pos {order_pos} (track {track_index}): {}",
            track.display_title()
        );
        let serial = self.next_serial();
        let start_result = self
            .engine
            .as_ref()
            .map(|engine| engine.start(&track, order_pos, serial));
        match start_result {
            Some(Ok(())) => {}
            Some(Err(error)) => {
                self.toast(format!("Не удалось открыть трек: {error}"), true);
                self.started = false;
                self.playing = false;
                return false;
            }
            None => return false,
        }
        self.started = true;
        self.playing = play;
        self.current = Some(track_index);
        self.selected = Some(track_index);
        self.play_pos = order_pos;
        self.last_serial = serial;
        self.appended = None;
        self.position = start_position;
        self.marquee_clock = Instant::now();
        self.follow_current = true;
        self.config.current = Some(track_index);
        self.config.position_ms = start_position.as_millis() as u64;
        self.mark_dirty();

        if start_position > Duration::from_millis(250) {
            let seek_result = self
                .engine
                .as_ref()
                .map(|engine| engine.seek(start_position));
            if let Some(Err(error)) = seek_result {
                tracing::warn!("initial seek failed: {error}");
                self.position = Duration::ZERO;
                self.toast("Этот файл не поддерживает перемотку", true);
            }
        }
        if let Some(engine) = self.engine.as_ref() {
            if play {
                engine.resume();
            } else {
                engine.pause();
            }
        }
        self.update_now_playing();
        if play {
            self.ensure_lookahead();
        }
        true
    }

    fn start_track_index(&mut self, index: usize, start_position: Duration, play: bool) -> bool {
        if index >= self.tracks().len() {
            return false;
        }
        if self.shuffle {
            let mut order: Vec<usize> = (0..self.tracks().len()).collect();
            order.retain(|candidate| *candidate != index);
            order.shuffle(&mut rand::rng());
            order.insert(0, index);
            self.play_order = order;
            self.play_pos = 0;
            self.start_order_pos(0, start_position, play)
        } else {
            self.rebuild_order();
            let pos = index.min(self.play_order.len().saturating_sub(1));
            self.start_order_pos(pos, start_position, play)
        }
    }

    fn display_position(&self) -> Duration {
        if self.started {
            self.position.min(
                self.display_track()
                    .and_then(Track::duration)
                    .unwrap_or(self.position),
            )
        } else {
            self.position
        }
    }

    fn play_selected_or_current(&mut self) {
        if self.started {
            if let Some(engine) = self.engine.as_ref() {
                engine.resume();
            }
            self.playing = true;
            self.mark_dirty();
            self.update_now_playing();
            return;
        }
        let index = self
            .selected
            .or(self.current)
            .or(if self.tracks().is_empty() {
                None
            } else {
                Some(0)
            });
        let Some(index) = index else {
            self.toast("Сначала добавьте музыку", false);
            return;
        };
        let start = if Some(index) == self.current {
            self.position
        } else {
            Duration::ZERO
        };
        self.start_track_index(index, start, true);
    }

    fn toggle_play(&mut self) {
        if self.playing {
            if let Some(engine) = self.engine.as_ref() {
                engine.pause();
            }
            self.position = self
                .engine
                .as_ref()
                .map(Engine::position)
                .unwrap_or(self.position);
            self.playing = false;
            self.mark_dirty();
            self.update_now_playing();
        } else {
            self.play_selected_or_current();
        }
    }

    fn stop_playback(&mut self) {
        if let Some(engine) = self.engine.as_ref() {
            engine.stop();
        }
        self.started = false;
        self.playing = false;
        self.position = Duration::ZERO;
        self.appended = None;
        self.last_serial = 0;
        self.seek_target = None;
        self.spectrum.reset();
        self.mark_dirty();
        self.update_now_playing();
    }

    fn next_track(&mut self) {
        let pos = self.play_pos;
        let Some(next_pos) = self.manual_next_pos(pos) else {
            if self.repeat == RepeatMode::Off {
                self.toast("Это последний трек", false);
            }
            return;
        };
        let Some(&index) = self.play_order.get(next_pos) else {
            return;
        };
        if self.playing {
            if self.appended.map(|(appended_pos, _)| appended_pos) == Some(next_pos) {
                if let Some(engine) = self.engine.as_ref() {
                    engine.skip_one();
                }
                return;
            }
            self.start_order_pos(next_pos, Duration::ZERO, true);
        } else {
            self.prepare_track(index, next_pos);
        }
    }

    fn previous_track(&mut self) {
        if self.playing && self.position > Duration::from_secs(3) {
            if let Some(index) = self.current {
                self.start_track_index(index, Duration::ZERO, true);
            }
            return;
        }
        let Some(prev_pos) = self.manual_prev_pos(self.play_pos) else {
            if let Some(index) = self.current {
                if self.playing {
                    self.start_track_index(index, Duration::ZERO, true);
                } else {
                    self.prepare_track(index, self.play_pos);
                }
            }
            return;
        };
        let Some(&index) = self.play_order.get(prev_pos) else {
            return;
        };
        if self.playing {
            self.start_order_pos(prev_pos, Duration::ZERO, true);
        } else {
            self.prepare_track(index, prev_pos);
        }
    }

    fn prepare_track(&mut self, index: usize, order_pos: usize) {
        if let Some(engine) = self.engine.as_ref() {
            engine.clear();
        }
        self.started = false;
        self.playing = false;
        self.appended = None;
        self.last_serial = 0;
        self.position = Duration::ZERO;
        self.play_pos = order_pos;
        self.current = Some(index);
        self.selected = Some(index);
        self.config.current = Some(index);
        self.config.position_ms = 0;
        self.follow_current = true;
        self.spectrum.reset();
        self.mark_dirty();
        self.update_now_playing();
    }

    fn seek_to(&mut self, target: Duration) {
        let Some(track) = self
            .current
            .and_then(|index| self.tracks().get(index))
            .or_else(|| self.selected.and_then(|index| self.tracks().get(index)))
            .cloned()
        else {
            return;
        };
        let duration = track.duration().unwrap_or(Duration::ZERO);
        let target = if duration > Duration::ZERO {
            target.min(duration)
        } else {
            target
        };
        if self.started {
            if let Some(engine) = self.engine.as_ref() {
                if let Err(error) = engine.seek(target) {
                    tracing::warn!("seek failed: {error}");
                    self.toast("Перемотка не поддерживается этим файлом", true);
                    return;
                }
            }
            self.seek_target = Some(target);
        }
        self.position = target;
        self.config.position_ms = target.as_millis() as u64;
        self.mark_dirty();
        if let Some(media) = self.media.as_mut() {
            media.set_progress(target, self.playing);
        }
    }

    fn seek_by(&mut self, delta: f32) {
        let base = self.display_position();
        let target = if delta.is_sign_negative() {
            base.saturating_sub(Duration::from_secs_f32(delta.abs()))
        } else {
            base + Duration::from_secs_f32(delta)
        };
        self.seek_to(target);
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(engine) = self.engine.as_ref() {
            engine.set_volume(self.volume);
        }
        self.config.volume = self.volume;
        self.mark_dirty();
    }

    fn set_balance(&mut self, balance: f32) {
        self.balance = balance.clamp(-1.0, 1.0);
        self.dsp.set_balance(self.balance);
        self.config.balance = self.balance;
        self.mark_dirty();
    }

    /// Restart the current track at its position after the play order changed.
    /// The lookahead source (if any) was appended for the old order, so it has
    /// to be replaced before it can play once with stale neighbours.
    fn refresh_order_playback(&mut self) {
        if !self.started || self.appended.is_none() {
            return;
        }
        let Some(pos) = self
            .current
            .and_then(|current| self.play_order.iter().position(|index| *index == current))
        else {
            return;
        };
        let position = self.position;
        let play = self.playing;
        self.start_order_pos(pos, position, play);
    }

    fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.config.shuffle = self.shuffle;
        self.rebuild_order();
        self.refresh_order_playback();
        self.mark_dirty();
        if let Some(media) = self.media.as_mut() {
            media.set_shuffle(self.shuffle);
        }
        if self.shuffle {
            self.toast("Shuffle включён", false);
        } else {
            self.toast("Shuffle выключен", false);
        }
    }

    fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.cycle();
        self.config.repeat = self.repeat;
        self.refresh_order_playback();
        self.mark_dirty();
        self.toast(
            match self.repeat {
                RepeatMode::Off => "Повтор выключен",
                RepeatMode::All => "Повтор всего плейлиста",
                RepeatMode::One => "Повтор текущего трека",
            },
            false,
        );
    }

    fn toggle_playlist(&mut self, ctx: &egui::Context) {
        self.show_playlist = !self.show_playlist;
        self.config.show_playlist = self.show_playlist;
        if self.show_playlist {
            self.playlist_just_opened = true;
        } else {
            self.drag_track = None;
            self.drag_insert = None;
            ctx.send_viewport_cmd_to(playlist_viewport_id(), ViewportCommand::Close);
        }
        self.mark_dirty();
    }

    fn toggle_always_on_top(&mut self, ctx: &egui::Context) {
        self.always_on_top = !self.always_on_top;
        self.config.always_on_top = self.always_on_top;
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if self.always_on_top {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        }));
        self.mark_dirty();
        self.toast(
            if self.always_on_top {
                "Поверх всех окон"
            } else {
                "Обычный уровень окна"
            },
            false,
        );
    }

    fn toggle_shade(&mut self, ctx: &egui::Context) {
        self.shaded = !self.shaded;
        self.config.shaded = self.shaded;
        self.mark_dirty();
        self.apply_main_size(ctx);
        self.geometry_ready = false;
    }

    fn apply_main_size(&self, ctx: &egui::Context) {
        let size = vec2(MAIN_W, if self.shaded { SHADE_H } else { MAIN_H });
        ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(size));
        ctx.send_viewport_cmd(ViewportCommand::MaxInnerSize(size));
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
    }

    fn toggle_double_size(&mut self, _ctx: &egui::Context) {
        self.double_size = !self.double_size;
        self.config.double_size = self.double_size;
        self.pending_zoom_resize = true;
        self.zoom_guard_until = Instant::now() + Duration::from_millis(900);
        self.mark_dirty();
        self.toast(
            if self.double_size {
                "Double size · окна стали вдвое больше"
            } else {
                "Обычный размер окон"
            },
            false,
        );
    }

    fn apply_zoom_layout(&mut self, ctx: &egui::Context) {
        if self.playlist_root {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(self.playlist_size));
            return;
        }
        if self.eq_root {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(EQ_W, EQ_H)));
            return;
        }
        if self.drive_root {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(DRIVE_W, DRIVE_H)));
            return;
        }
        self.apply_main_size(ctx);
        if self.show_playlist {
            let size = self.playlist_size;
            ctx.send_viewport_cmd_to(playlist_viewport_id(), ViewportCommand::InnerSize(size));
        }
        if self.show_eq {
            ctx.send_viewport_cmd_to(
                eq_viewport_id(),
                ViewportCommand::InnerSize(vec2(EQ_W, EQ_H)),
            );
        }
        if self.show_drive {
            ctx.send_viewport_cmd_to(
                drive_viewport_id(),
                ViewportCommand::InnerSize(vec2(DRIVE_W, DRIVE_H)),
            );
        }
        if self.restore_positions_after_zoom {
            self.restore_positions_after_zoom = false;
            if let Some(pos) = self.main_pos {
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos));
            }
            if self.show_playlist {
                if let Some(pos) = self.playlist_pos {
                    ctx.send_viewport_cmd_to(
                        playlist_viewport_id(),
                        ViewportCommand::OuterPosition(pos),
                    );
                }
            }
            if self.show_eq {
                if let Some(pos) = self.eq_pos {
                    ctx.send_viewport_cmd_to(eq_viewport_id(), ViewportCommand::OuterPosition(pos));
                }
            }
            if self.show_drive {
                if let Some(pos) = self.drive_pos {
                    ctx.send_viewport_cmd_to(
                        drive_viewport_id(),
                        ViewportCommand::OuterPosition(pos),
                    );
                }
            }
        }
    }

    fn send_window_position(&self, ctx: &egui::Context, slot: usize, pos: Pos2) {
        let command = ViewportCommand::OuterPosition(pos);
        match slot {
            WINDOW_MAIN => ctx.send_viewport_cmd(command),
            WINDOW_PLAYLIST => ctx.send_viewport_cmd_to(playlist_viewport_id(), command),
            WINDOW_EQ => ctx.send_viewport_cmd_to(eq_viewport_id(), command),
            WINDOW_DRIVE => ctx.send_viewport_cmd_to(drive_viewport_id(), command),
            _ => {}
        }
    }

    fn process_window_geometry(&mut self, ctx: &egui::Context) {
        if self.playlist_root || self.eq_root || self.drive_root {
            return;
        }
        if !self.geometry_ready || Instant::now() < self.zoom_guard_until {
            self.prev_rects = self.rects;
            self.geometry_ready = true;
            // Dock freshly opened or re-scaled tool windows to the main window,
            // but never move the main window on the first settle.
            self.moving_last_frame = [false, true, true, true];
            self.skip_motion_frames = [0; WINDOW_COUNT];
            return;
        }

        let prev = self.prev_rects;
        let mut cur = self.rects;
        let mut moved = [false; WINDOW_COUNT];
        for slot in 0..WINDOW_COUNT {
            if self.skip_motion_frames[slot] > 0 {
                self.skip_motion_frames[slot] -= 1;
                continue;
            }
            if let (Some(current), Some(previous)) = (cur[slot], prev[slot]) {
                moved[slot] = current.min.distance(previous.min) > 0.75;
            }
        }

        // The main window carries every window docked to it.
        if moved[WINDOW_MAIN] {
            if let (Some(current_main), Some(previous_main)) = (cur[WINDOW_MAIN], prev[WINDOW_MAIN])
            {
                let delta = current_main.min - previous_main.min;
                if delta.length_sq() > 0.01 {
                    for slot in [WINDOW_PLAYLIST, WINDOW_EQ, WINDOW_DRIVE] {
                        if let (Some(current), Some(previous)) = (cur[slot], prev[slot]) {
                            if windows_attached(previous_main, previous) {
                                let target = previous.min + delta;
                                self.send_window_position(ctx, slot, target);
                                cur[slot] = Some(Rect::from_min_size(target, current.size()));
                                self.skip_motion_frames[slot] = 2;
                            }
                        }
                    }
                }
            }
        }

        // Once a window stops moving, pull it flush against a neighbour.
        for slot in 0..WINDOW_COUNT {
            if self.skip_motion_frames[slot] > 0 || !self.moving_last_frame[slot] || moved[slot] {
                continue;
            }
            let Some(moving) = cur[slot] else {
                continue;
            };
            let mut snapped = moving;
            for (other_slot, other) in cur.iter().enumerate() {
                if other_slot != slot {
                    if let Some(other) = *other {
                        snapped = snapped.translate(snap_offset(snapped, other, SNAP_DIST));
                    }
                }
            }
            let offset = snapped.min - moving.min;
            if offset.length_sq() > 0.01 {
                self.send_window_position(ctx, slot, snapped.min);
                cur[slot] = Some(snapped);
                self.skip_motion_frames[slot] = 2;
            }
        }

        for slot in 0..WINDOW_COUNT {
            self.moving_last_frame[slot] = self.skip_motion_frames[slot] == 0 && moved[slot];
        }
        self.rects = cur;
        self.prev_rects = cur;
    }

    fn apply_skin(&mut self, skin: Skin) {
        self.skin = skin;
        self.palette = skin.palette();
        self.config.skin = skin;
        self.mark_dirty();
    }

    fn toggle_eq_window(&mut self, ctx: &egui::Context) {
        self.show_eq = !self.show_eq;
        self.config.show_eq = self.show_eq;
        if self.show_eq {
            self.eq_just_opened = true;
        } else {
            ctx.send_viewport_cmd_to(eq_viewport_id(), ViewportCommand::Close);
        }
        self.mark_dirty();
    }

    fn toggle_drive_window(&mut self, ctx: &egui::Context) {
        self.show_drive = !self.show_drive;
        self.config.show_drive = self.show_drive;
        if self.show_drive {
            self.drive_just_opened = true;
        } else {
            ctx.send_viewport_cmd_to(drive_viewport_id(), ViewportCommand::Close);
        }
        self.mark_dirty();
    }

    fn apply_eq(&mut self) {
        self.dsp.set_eq(self.eq_enabled, self.eq_gains, 0.0);
        self.config.eq_enabled = self.eq_enabled;
        self.config.eq_gains = self.eq_gains;
        self.config.eq_preset = self.eq_preset;
        self.mark_dirty();
    }

    fn cycle_eq_preset(&mut self) {
        let next = match self.eq_preset {
            Some(index) if index + 1 < EQ_PRESETS.len() => index + 1,
            _ => 0,
        };
        self.eq_preset = Some(next);
        self.eq_gains = EQ_PRESETS[next].1;
        self.eq_enabled = true;
        self.apply_eq();
    }

    fn eq_preset_name(&self) -> &'static str {
        self.eq_preset
            .and_then(|index| EQ_PRESETS.get(index).map(|preset| preset.0))
            .unwrap_or("Custom")
    }

    fn eq_position(&self, ctx: &egui::Context) -> Pos2 {
        if let Some(pos) = self.eq_pos {
            return pos;
        }
        let main_rect = ctx
            .input(|input| input.viewport().outer_rect)
            .unwrap_or_else(|| Rect::from_min_size(pos2(80.0, 80.0), vec2(MAIN_W, MAIN_H)));
        let monitor = ctx
            .input(|input| input.viewport().monitor_size)
            .unwrap_or(vec2(1440.0, 900.0));
        let x = main_rect.left();
        let y = (main_rect.top() + MAIN_H).min((monitor.y - EQ_H - 2.0).max(0.0));
        pos2(x, y)
    }

    fn open_files(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Аудио", &["mp3", "flac", "wav", "m4a", "aac", "ogg"])
            .pick_files()
        {
            self.loader.request(paths);
        }
    }

    fn open_folder(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.loader.request([path]);
        }
    }

    fn remove_selected(&mut self) {
        let Some(index) = self.selected else {
            self.toast("Сначала выберите трек", false);
            return;
        };
        if index >= self.tracks().len() {
            return;
        }
        if self.started && self.current == Some(index) {
            self.toast("Играющий трек удалить нельзя · сначала Stop", true);
            return;
        }
        self.config.tracks.remove(index);
        self.drag_track = None;
        self.drag_insert = None;
        if self.started {
            self.current = self.current.map(|current| {
                if current > index {
                    current - 1
                } else {
                    current
                }
            });
            if !remove_track_from_order(&mut self.play_order, &mut self.play_pos, index) {
                self.rebuild_order();
            }
            // A queued lookahead may now point at the wrong order position.
            // Restarting the current track replaces it before it can play.
            self.refresh_order_playback();
        } else {
            self.current = match self.current {
                Some(current) if current == index => None,
                Some(current) if current > index => Some(current - 1),
                other => other,
            };
            self.position = Duration::ZERO;
            self.rebuild_order();
        }
        self.selected = if self.config.tracks.is_empty() {
            None
        } else {
            Some(index.min(self.config.tracks.len() - 1))
        };
        self.config.current = self.current;
        self.follow_current = !self.started;
        self.mark_dirty();
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.loader.request(dropped);
            ctx.request_repaint();
        }
    }

    fn poll_loader(&mut self) {
        let events = self.loader.poll();
        for event in events {
            match event {
                LoaderEvent::BatchStarted(count) => {
                    self.loading_batches += 1;
                    self.loading_files += count;
                }
                LoaderEvent::Loaded(track) => {
                    if self
                        .config
                        .tracks
                        .iter()
                        .any(|existing| existing.path == track.path)
                    {
                        continue;
                    }
                    let index = self.config.tracks.len();
                    self.config.tracks.push(track);
                    self.sync_order_after_add(index);
                    if self.selected.is_none() {
                        self.selected = Some(index);
                    }
                    if self.autoplay_initial && !self.autoplay_done && !self.started {
                        self.autoplay_done = true;
                        self.selected = Some(index);
                        self.start_track_index(index, Duration::ZERO, true);
                    }
                    self.mark_dirty();
                }
                LoaderEvent::Failed(error) => {
                    self.toast(error, true);
                }
                LoaderEvent::BatchFinished => {
                    self.loading_batches = self.loading_batches.saturating_sub(1);
                    if self.loading_batches == 0 {
                        self.loading_files = 0;
                        self.mark_dirty();
                    }
                }
            }
        }
    }

    fn poll_drive(&mut self) {
        for event in self.drive.service.poll() {
            self.handle_drive_event(event);
        }
    }

    fn handle_drive_event(&mut self, event: DmEvent) {
        match event {
            DmEvent::Search { query, result } => {
                self.drive.loading = None;
                match result {
                    Ok(results) => {
                        let SearchResults { artists, tracks } = results;
                        self.drive.error = false;
                        self.drive.scroll = 0.0;
                        self.drive.selected = None;
                        let open_url = self.drive.initial_artist.take().and_then(|slug| {
                            artists
                                .iter()
                                .find(|artist| artist.url.contains(&slug))
                                .map(|artist| artist.url.clone())
                        });
                        if !artists.is_empty() {
                            let count = artists.len();
                            self.drive.search_artists = artists.clone();
                            self.drive.view = DriveView::Artists(artists);
                            self.drive.status = format!("НАЙДЕНО АРТИСТОВ: {count}");
                            if let Some(url) = open_url {
                                self.drive.open_artist(url);
                            }
                        } else if !tracks.is_empty() {
                            let count = tracks.len();
                            self.drive.view = DriveView::Artist(ArtistView {
                                name: format!("ПОИСК · {query}"),
                                tracks,
                                next_url: None,
                                page: 1,
                            });
                            self.drive.selected = Some(0);
                            self.drive.status = format!("НАЙДЕНО ТРЕКОВ: {count}");
                        } else {
                            self.drive.view = DriveView::Empty;
                            self.drive.status = format!("НИЧЕГО НЕ НАЙДЕНО · {query}");
                            self.drive.error = true;
                        }
                    }
                    Err(error) => {
                        self.drive.error = true;
                        self.drive.status = error.clone();
                        self.toast(format!("DriveMusic: {error}"), true);
                    }
                }
            }
            DmEvent::Artist { append, result } => {
                self.drive.loading = None;
                match result {
                    Ok(page) => {
                        self.drive.error = false;
                        let ArtistPage {
                            name,
                            tracks,
                            next_url,
                        } = page;
                        let mut page_tracks = Some(tracks);
                        let mut page_next = next_url;
                        let mut page_name = Some(name);
                        let mut appended = None;
                        if append {
                            if let DriveView::Artist(artist) = &mut self.drive.view {
                                if let Some(tracks) = page_tracks.take() {
                                    artist.tracks.extend(tracks);
                                }
                                artist.next_url = page_next.take();
                                artist.page += 1;
                                if artist.name.trim().is_empty() {
                                    if let Some(name) = page_name.take() {
                                        artist.name = name;
                                    }
                                }
                                appended = Some(artist.tracks.len());
                            }
                        }
                        if let Some(count) = appended {
                            self.drive.status = format!("ЗАГРУЖЕНО: {count}");
                        } else {
                            let auto_play = std::env::var_os("MACAMP_DRIVE_PLAY").is_some();
                            let tracks = page_tracks.unwrap_or_default();
                            let next_url = page_next;
                            let count = tracks.len();
                            let name = page_name.take().unwrap_or_default();
                            let name = if name.trim().is_empty() {
                                "РЕЗУЛЬТАТЫ".to_owned()
                            } else {
                                name
                            };
                            self.drive.view = DriveView::Artist(ArtistView {
                                name,
                                tracks,
                                next_url,
                                page: 1,
                            });
                            self.drive.selected = (count > 0).then_some(0);
                            self.drive.scroll = 0.0;
                            self.drive.status = if count == 0 {
                                "НЕТ ТРЕКОВ".to_owned()
                            } else {
                                format!("ТРЕКОВ: {count}")
                            };
                            if auto_play {
                                let first = if let DriveView::Artist(artist) = &self.drive.view {
                                    artist.tracks.first().cloned()
                                } else {
                                    None
                                };
                                if let Some(track) = first {
                                    self.request_remote(track, DownloadIntent::Play);
                                }
                            }
                        }
                    }
                    Err(error) => {
                        self.drive.error = true;
                        self.drive.status = error.clone();
                        self.toast(format!("DriveMusic: {error}"), true);
                    }
                }
            }
            DmEvent::Progress { key, fraction } => {
                if let Some(slot) = self.drive.downloads.get_mut(&key) {
                    *slot = fraction;
                }
            }
            DmEvent::Downloaded {
                track,
                path,
                result,
            } => {
                let key = track.cache_key.clone();
                self.drive.downloads.remove(&key);
                self.drive.intents.remove(&key);
                let should_play = self.drive.pending_play.as_deref() == Some(key.as_str());
                if should_play {
                    self.drive.pending_play = None;
                }
                match result {
                    Ok(()) => {
                        let label = remote_title(&track);
                        let index =
                            self.ensure_track_index(drivemusic::track_from_file(&track, path));
                        if should_play {
                            self.selected = Some(index);
                            if self.start_track_index(index, Duration::ZERO, true) {
                                self.drive.status = format!("ИГРАЮ · {label}");
                            }
                        } else {
                            self.selected = Some(index);
                            self.drive.status = format!("ДОБАВЛЕНО · {label}");
                            if !self.show_drive {
                                self.toast(format!("DriveMusic · добавлено: {label}"), false);
                            }
                        }
                    }
                    Err(error) => {
                        self.drive.error = true;
                        self.drive.status = format!("ОШИБКА · {error}");
                        self.toast(format!("DriveMusic: {error}"), true);
                    }
                }
            }
        }
    }

    fn ensure_track_index(&mut self, track: Track) -> usize {
        if let Some(index) = self
            .config
            .tracks
            .iter()
            .position(|existing| existing.path == track.path)
        {
            return index;
        }
        let index = self.config.tracks.len();
        self.config.tracks.push(track);
        self.sync_order_after_add(index);
        if self.selected.is_none() {
            self.selected = Some(index);
        }
        self.mark_dirty();
        index
    }

    fn request_remote(&mut self, remote: RemoteTrack, intent: DownloadIntent) {
        let key = remote.cache_key.clone();
        let label = remote_title(&remote);
        self.drive.error = false;
        if let Some(track) = cached_track(&remote) {
            let index = self.ensure_track_index(track);
            self.selected = Some(index);
            if intent == DownloadIntent::Play {
                self.drive.pending_play = None;
                if self.start_track_index(index, Duration::ZERO, true) {
                    self.drive.status = format!("ИГРАЮ · {label}");
                }
            } else {
                self.drive.status = format!("УЖЕ В КЭШЕ · {label}");
                if !self.show_drive {
                    self.toast(format!("DriveMusic · добавлено: {label}"), false);
                }
            }
            return;
        }
        if self.drive.downloads.contains_key(&key) {
            if intent == DownloadIntent::Play {
                self.drive.pending_play = Some(key.clone());
                self.drive.intents.insert(key, DownloadIntent::Play);
            }
            self.drive.status = format!("УЖЕ СКАЧИВАЕТСЯ · {label}");
            return;
        }
        let Some(path) = cache_path(&remote) else {
            self.toast("DriveMusic: нет каталога для кэша", true);
            return;
        };
        self.drive.downloads.insert(key.clone(), None);
        self.drive.intents.insert(key.clone(), intent);
        if intent == DownloadIntent::Play {
            self.drive.pending_play = Some(key);
            self.drive.status = format!("СКАЧИВАНИЕ ДЛЯ PLAY · {label}");
        } else {
            self.drive.status = format!("СКАЧИВАНИЕ · {label}");
        }
        self.drive.service.download(remote, path);
    }

    fn add_all_remote(&mut self) {
        let tracks = match &self.drive.view {
            DriveView::Artist(artist) => artist.tracks.clone(),
            _ => return,
        };
        if tracks.is_empty() {
            return;
        }
        let count = tracks.len();
        for track in tracks {
            self.request_remote(track, DownloadIntent::Add);
        }
        self.drive.status = format!("В ОЧЕРЕДЬ: {count}");
    }

    fn ensure_lookahead(&mut self) {
        if !self.playing || self.appended.is_some() || self.play_order.is_empty() {
            return;
        }
        let duration = self.current_track().and_then(Track::duration);
        let position = self.position;
        let remaining = duration.map(|duration| duration.saturating_sub(position));
        let near_end = remaining
            .map(|remaining| remaining <= Duration::from_secs(2))
            .unwrap_or(true);
        if !near_end {
            return;
        }
        let Some(next_pos) = self.auto_next_pos(self.play_pos) else {
            return;
        };
        let Some(&track_index) = self.play_order.get(next_pos) else {
            return;
        };
        let Some(track) = self.tracks().get(track_index).cloned() else {
            return;
        };
        let serial = self.next_serial();
        let result = self
            .engine
            .as_ref()
            .map(|engine| engine.append(&track, next_pos, serial));
        match result {
            Some(Ok(())) => {
                tracing::info!("queued next source for order pos {next_pos}");
                self.appended = Some((next_pos, serial));
            }
            Some(Err(error)) => {
                self.toast(format!("Следующий трек не открылся: {error}"), true);
            }
            None => {}
        }
    }

    fn sync_audio(&mut self) {
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        if !self.started {
            return;
        }
        let (current, position, empty, tap) = (
            engine.current(),
            engine.position(),
            engine.empty(),
            engine.tap().take(),
        );

        if let Some((order_pos, serial)) = current {
            if serial != self.last_serial {
                self.last_serial = serial;
                self.appended = None;
                if let Some(&track_index) = self.play_order.get(order_pos) {
                    self.current = Some(track_index);
                    self.selected = Some(track_index);
                    self.play_pos = order_pos;
                    self.position = Duration::ZERO;
                    self.config.current = Some(track_index);
                    self.marquee_clock = Instant::now();
                    self.follow_current = true;
                    self.mark_dirty();
                    tracing::info!(
                        "now playing order pos {order_pos} (track {track_index}): {}",
                        self.config.tracks[track_index].display_title()
                    );
                    self.update_now_playing();
                } else {
                    tracing::warn!("playback order position {order_pos} has no track");
                    self.stop_playback();
                    return;
                }
            }
        }

        self.position = position;
        if let Some(track) = self.current_track() {
            let sample_rate = track.sample_rate;
            if !tap.is_empty() {
                self.spectrum.push(&tap, sample_rate);
            }
        }
        self.spectrum.analyze(Instant::now());

        if self.playing && empty && self.appended.is_none() {
            self.started = false;
            self.playing = false;
            self.position = Duration::ZERO;
            self.appended = None;
            self.last_serial = 0;
            self.seek_target = None;
            self.spectrum.reset();
            self.mark_dirty();
            tracing::info!("playback reached the end of the queue");
            self.update_now_playing();
            if let Some(engine) = self.engine.as_ref() {
                engine.stop();
            }
            return;
        }

        if self.playing {
            if self.last_position_mark.elapsed() >= Duration::from_millis(1000) {
                self.config.position_ms = self.position.as_millis() as u64;
                self.last_position_mark = Instant::now();
                self.mark_dirty();
            }
            self.ensure_lookahead();
        }
        if let Some(target) = self.seek_target.take() {
            // Keep the UI in sync with the decode position after a seek.
            self.position = target;
        }
    }

    fn update_now_playing(&mut self) {
        if let Some(engine) = self.engine.as_ref() {
            engine.set_volume(self.volume);
        }
        self.config.current = self.current;
        let track = self
            .current
            .and_then(|index| self.config.tracks.get(index))
            .cloned();
        let playback = if track.is_none() {
            MediaPlayback::Stopped
        } else if self.playing {
            MediaPlayback::Playing {
                progress: Some(MediaPosition(self.position)),
            }
        } else if self.started {
            MediaPlayback::Paused {
                progress: Some(MediaPosition(self.position)),
            }
        } else {
            MediaPlayback::Stopped
        };
        if let Some(media) = self.media.as_mut() {
            media.update(track.as_ref(), playback);
        }
    }

    fn poll_media(&mut self, ctx: &egui::Context) {
        let events = self
            .media
            .as_ref()
            .map(MediaBridge::poll)
            .unwrap_or_default();
        for event in events {
            match event {
                MediaControlEvent::Play => {
                    if !self.playing {
                        self.play_selected_or_current();
                    }
                }
                MediaControlEvent::Pause => {
                    if self.playing {
                        self.toggle_play();
                    }
                }
                MediaControlEvent::Toggle => self.toggle_play(),
                MediaControlEvent::Next => self.next_track(),
                MediaControlEvent::Previous => self.previous_track(),
                MediaControlEvent::Stop => self.stop_playback(),
                MediaControlEvent::Seek(direction) => {
                    let delta = match direction {
                        SeekDirection::Forward => 5.0,
                        SeekDirection::Backward => -5.0,
                    };
                    self.seek_by(delta);
                }
                MediaControlEvent::SeekBy(direction, duration) => {
                    let mut seconds = duration.as_secs_f32();
                    if matches!(direction, SeekDirection::Backward) {
                        seconds = -seconds;
                    }
                    self.seek_by(seconds);
                }
                MediaControlEvent::SetPosition(position) => {
                    self.seek_to(position.0);
                }
                MediaControlEvent::SetVolume(volume) => {
                    self.set_volume(volume as f32);
                }
                MediaControlEvent::SetShuffle(shuffle) => {
                    if self.shuffle != shuffle {
                        self.toggle_shuffle();
                    }
                }
                MediaControlEvent::Raise => {
                    ctx.send_viewport_cmd(ViewportCommand::Focus);
                }
                MediaControlEvent::Quit => {
                    self.save_now();
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
                _ => {}
            }
        }
    }

    fn save_now(&mut self) {
        self.config.current = self.current;
        self.config.position_ms = self.position.as_millis() as u64;
        self.config.volume = self.volume;
        self.config.balance = self.balance;
        self.config.skin = self.skin;
        self.config.show_playlist = self.show_playlist;
        self.config.repeat = self.repeat;
        self.config.shuffle = self.shuffle;
        self.config.always_on_top = self.always_on_top;
        self.config.shaded = self.shaded;
        self.config.double_size = self.double_size;
        self.config.playlist_size = [self.playlist_size.x, self.playlist_size.y];
        self.config.show_drive = self.show_drive;
        self.config.show_eq = self.show_eq;
        self.config.eq_enabled = self.eq_enabled;
        self.config.eq_gains = self.eq_gains;
        self.config.eq_preset = self.eq_preset;
        self.config.main_pos = self.main_pos.map(|pos| [pos.x, pos.y]);
        self.config.playlist_pos = self.playlist_pos.map(|pos| [pos.x, pos.y]);
        self.config.drive_pos = self.drive_pos.map(|pos| [pos.x, pos.y]);
        self.config.eq_pos = self.eq_pos.map(|pos| [pos.x, pos.y]);
        if let Err(error) = self.config.save() {
            tracing::warn!("could not save config: {error}");
        } else {
            self.dirty = false;
        }
        self.last_save = Instant::now();
    }

    fn autosave(&mut self) {
        if self.dirty && self.last_save.elapsed() >= Duration::from_secs(3) {
            self.save_now();
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context, playlist_focused: bool) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let mut open_files = false;
        let mut open_folder = false;
        let mut toggle = false;
        let mut stop = false;
        let mut next = false;
        let mut previous = false;
        let mut play_selected = false;
        let mut remove = false;
        let mut volume_delta = 0.0_f32;
        let mut seek_delta = 0.0_f32;
        let mut select_delta = 0_i32;
        let mut double_size = false;
        let mut clear_filter = false;
        let mut toggle_drive = false;

        ctx.input_mut(|input| {
            if input.consume_key(Modifiers::COMMAND, Key::O) {
                open_files = true;
            }
            if input.consume_key(Modifiers::COMMAND, Key::D) {
                double_size = true;
            }
            if input.consume_key(Modifiers::COMMAND, Key::L) {
                toggle_drive = true;
            }
            if input.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::O) {
                open_folder = true;
            }
            if input.consume_key(Modifiers::NONE, Key::Space) {
                toggle = true;
            }
            if input.consume_key(Modifiers::NONE, Key::Z) {
                previous = true;
            }
            if input.consume_key(Modifiers::NONE, Key::X) {
                play_selected = true;
            }
            if input.consume_key(Modifiers::NONE, Key::C) {
                // Pause is a no-op if playback is stopped.
                if self.started && self.playing {
                    toggle = true;
                }
            }
            if input.consume_key(Modifiers::NONE, Key::V) {
                stop = true;
            }
            if input.consume_key(Modifiers::NONE, Key::B) {
                next = true;
            }
            if input.consume_key(Modifiers::NONE, Key::Enter) {
                play_selected = true;
            }
            if input.consume_key(Modifiers::NONE, Key::Delete) {
                remove = true;
            }
            if input.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                seek_delta = -5.0;
            }
            if input.consume_key(Modifiers::NONE, Key::ArrowRight) {
                seek_delta = 5.0;
            }
            if input.consume_key(Modifiers::NONE, Key::ArrowUp) {
                if playlist_focused {
                    select_delta = -1;
                } else {
                    volume_delta = 0.05;
                }
            }
            if input.consume_key(Modifiers::NONE, Key::ArrowDown) {
                if playlist_focused {
                    select_delta = 1;
                } else {
                    volume_delta = -0.05;
                }
            }
            if input.consume_key(Modifiers::NONE, Key::Equals)
                || input.consume_key(Modifiers::NONE, Key::Plus)
            {
                volume_delta = 0.05;
            }
            if input.consume_key(Modifiers::NONE, Key::Minus) {
                volume_delta = -0.05;
            }
            if playlist_focused && input.consume_key(Modifiers::NONE, Key::Escape) {
                clear_filter = true;
            }
        });

        if open_files {
            self.open_files();
        }
        if open_folder {
            self.open_folder();
        }
        if previous {
            self.previous_track();
        }
        if toggle {
            self.toggle_play();
        }
        if stop {
            self.stop_playback();
        }
        if next {
            self.next_track();
        }
        if play_selected {
            self.play_selected_or_current();
        }
        if remove {
            self.remove_selected();
        }
        if volume_delta != 0.0 {
            self.set_volume(self.volume + volume_delta);
        }
        if seek_delta != 0.0 {
            self.seek_by(seek_delta);
        }
        if select_delta != 0 {
            self.move_selection(select_delta);
        }
        if double_size {
            self.toggle_double_size(ctx);
        }
        if toggle_drive {
            self.toggle_drive_window(ctx);
        }
        if clear_filter && !self.filter.is_empty() {
            self.filter.clear();
            self.playlist_scroll = 0.0;
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let visible = self.visible_indices();
        let Some(last) = visible.len().checked_sub(1) else {
            return;
        };
        let position = self
            .selected
            .and_then(|selected| visible.iter().position(|index| *index == selected));
        let next = match position {
            Some(position) => (position as i32 + delta).clamp(0, last as i32) as usize,
            None if delta < 0 => last,
            None => 0,
        };
        self.selected = Some(visible[next]);
        self.follow_current = false;
        self.scroll_to_index(next);
    }

    /// Scroll so a row position in the filtered list is visible. The lower
    /// bound is clamped immediately; the upper bound is corrected once the
    /// playlist viewport height is known.
    fn scroll_to_index(&mut self, position: usize) {
        let target = position as f32 * ROW_H;
        if target < self.playlist_scroll {
            self.playlist_scroll = target;
        }
        self.playlist_scroll = self.playlist_scroll.max(0.0);
    }

    fn sync_window_state(&mut self, ctx: &egui::Context) {
        if Instant::now() >= self.zoom_guard_until {
            if let Some(rect) = ctx.input(|input| input.viewport().outer_rect) {
                let pos = rect.min;
                if self
                    .main_pos
                    .map(|old| old.distance(pos) > 0.5)
                    .unwrap_or(true)
                {
                    self.main_pos = Some(pos);
                    self.mark_dirty();
                }
            }
        }
        if ctx.input(|input| input.viewport().close_requested()) {
            self.save_now();
        }
    }

    fn request_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else {
            return;
        };
        let delay = std::env::var("MACAMP_SCREENSHOT_DELAY")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .map(Duration::from_secs_f32)
            .unwrap_or(Duration::from_secs(2));
        if self.screenshot_requested || self.created_at.elapsed() < delay {
            return;
        }
        self.screenshot_requested = true;
        tracing::info!("requesting screenshot");
        ctx.request_screenshot(move |image| {
            save_color_image(&path, &image);
        });
        ctx.request_repaint();
    }

    fn draw_main(&mut self, ui: &mut Ui) {
        self.rects[WINDOW_MAIN] = ui.ctx().input(|input| input.viewport().outer_rect);
        let p = self.palette;
        let rect = ui.max_rect();
        if self.shaded {
            self.draw_shaded(ui, rect);
            return;
        }
        self.draw_window_base(ui, rect, "MACAMP 2.0");

        let title_rect = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE_H));
        let drag_rect =
            Rect::from_min_max(rect.min, pos2(rect.right() - 82.0, rect.top() + TITLE_H));
        let drag = ui.interact(
            drag_rect,
            Id::unique("main_titlebar"),
            Sense::click_and_drag(),
        );
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            self.toggle_shade(ui.ctx());
        }

        let menu_rect = Rect::from_min_size(pos2(rect.right() - 78.0, 6.0), vec2(16.0, 16.0));
        let shade_rect = Rect::from_min_size(pos2(rect.right() - 58.0, 6.0), vec2(16.0, 16.0));
        let minimize_rect = Rect::from_min_size(pos2(rect.right() - 38.0, 6.0), vec2(16.0, 16.0));
        let close_rect = Rect::from_min_size(pos2(rect.right() - 18.0, 6.0), vec2(16.0, 16.0));

        let menu_response = tiny_title_button(ui, menu_rect, "menu", &p, Icon::Menu);
        if tiny_title_button(ui, shade_rect, "shade", &p, Icon::Shade)
            .on_hover_text("Windowshade · двойной клик по тайтлбару")
            .clicked()
        {
            self.toggle_shade(ui.ctx());
        }
        if tiny_title_button(ui, minimize_rect, "minimize", &p, Icon::Minimize).clicked() {
            ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
        }
        if tiny_title_button(ui, close_rect, "close", &p, Icon::Close).clicked() {
            self.save_now();
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        }

        let body = Rect::from_min_max(pos2(rect.left(), title_rect.bottom()), rect.max);
        let right_strip = 24.0;
        let screen = child_rect(
            body,
            6.0,
            6.0,
            (body.width() - 6.0 - right_strip - 6.0).max(200.0),
            88.0,
        );
        self.draw_screen(ui, screen);

        let trackline = Rect::from_min_size(
            pos2(screen.left() + 2.0, screen.bottom() + 4.0),
            vec2(screen.width() - 4.0, 14.0),
        );
        self.draw_trackline(ui, trackline);

        let seek_rect = Rect::from_min_size(
            pos2(screen.left(), trackline.bottom() + 3.0),
            vec2(screen.width(), 14.0),
        );
        self.draw_seek(ui, seek_rect);

        let controls_y = seek_rect.bottom() + 6.0;
        let button_w = 36.0;
        let button_h = 28.0;
        let mut x = screen.left();
        let prev = classic_button(
            ui,
            Rect::from_min_size(pos2(x, controls_y), vec2(button_w, button_h)),
            "prev",
            &p,
            Some(Icon::Previous),
            None,
            false,
            true,
        );
        x += button_w + 4.0;
        let play = classic_button(
            ui,
            Rect::from_min_size(pos2(x, controls_y), vec2(button_w, button_h)),
            "play",
            &p,
            Some(Icon::Play),
            None,
            self.playing,
            true,
        );
        x += button_w + 4.0;
        let pause = classic_button(
            ui,
            Rect::from_min_size(pos2(x, controls_y), vec2(button_w, button_h)),
            "pause",
            &p,
            Some(Icon::Pause),
            None,
            self.started && !self.playing,
            true,
        );
        x += button_w + 4.0;
        let stop = classic_button(
            ui,
            Rect::from_min_size(pos2(x, controls_y), vec2(button_w, button_h)),
            "stop",
            &p,
            Some(Icon::Stop),
            None,
            false,
            true,
        );
        x += button_w + 4.0;
        let next = classic_button(
            ui,
            Rect::from_min_size(pos2(x, controls_y), vec2(button_w, button_h)),
            "next",
            &p,
            Some(Icon::Next),
            None,
            false,
            true,
        );

        let toggle_w = 34.0;
        let toggle_h = 24.0;
        let toggle_y = controls_y + (button_h - toggle_h) * 0.5;
        let mut toggle_x = screen.right() - toggle_w * 5.0 - 12.0;
        let drive_toggle = toggle_button(
            ui,
            Rect::from_min_size(pos2(toggle_x, toggle_y), vec2(toggle_w, toggle_h)),
            "toggle_drive",
            &p,
            "DRV",
            self.show_drive,
            "DriveMusic · поиск артистов и песен",
        );
        toggle_x += toggle_w + 3.0;
        let eq_toggle = toggle_button(
            ui,
            Rect::from_min_size(pos2(toggle_x, toggle_y), vec2(toggle_w, toggle_h)),
            "toggle_eq",
            &p,
            "EQ",
            self.show_eq,
            "Показать или скрыть эквалайзер",
        );
        toggle_x += toggle_w + 3.0;
        let pl_toggle = toggle_button(
            ui,
            Rect::from_min_size(pos2(toggle_x, toggle_y), vec2(toggle_w, toggle_h)),
            "toggle_playlist",
            &p,
            "PL",
            self.show_playlist,
            "Показать или скрыть плейлист",
        );
        toggle_x += toggle_w + 3.0;
        let shuffle_toggle = toggle_button(
            ui,
            Rect::from_min_size(pos2(toggle_x, toggle_y), vec2(toggle_w, toggle_h)),
            "toggle_shuffle",
            &p,
            "SHUF",
            self.shuffle,
            "Перемешать очередь",
        );
        toggle_x += toggle_w + 3.0;
        let repeat_toggle = toggle_button(
            ui,
            Rect::from_min_size(pos2(toggle_x, toggle_y), vec2(toggle_w, toggle_h)),
            "toggle_repeat",
            &p,
            "REP",
            self.repeat.is_on(),
            &format!("Повтор: {}", self.repeat.label()),
        );

        if prev.clicked() {
            self.previous_track();
        }
        if play.clicked() {
            self.play_selected_or_current();
        }
        if pause.clicked() && self.started && self.playing {
            self.toggle_play();
        }
        if stop.clicked() {
            self.stop_playback();
        }
        if next.clicked() {
            self.next_track();
        }
        if drive_toggle.clicked() {
            self.toggle_drive_window(ui.ctx());
        }
        if eq_toggle.clicked() {
            self.toggle_eq_window(ui.ctx());
        }
        if pl_toggle.clicked() {
            self.toggle_playlist(ui.ctx());
        }
        if shuffle_toggle.clicked() {
            self.toggle_shuffle();
        }
        if repeat_toggle.clicked() {
            self.cycle_repeat();
        }

        let mix_y = controls_y + button_h + 6.0;
        let vol_label = pos2(screen.left(), mix_y + 2.0);
        ui.painter()
            .text(vol_label, Align2::LEFT_TOP, "VOL", p.text_font(7.5), p.dim);
        let vol_rect = Rect::from_min_size(pos2(screen.left() + 30.0, mix_y), vec2(240.0, 14.0));
        let (_, volume) = slider(
            ui,
            vol_rect,
            "main_volume",
            &p,
            self.volume,
            false,
            "Громкость",
        );
        if let Some(volume) = volume {
            self.set_volume(volume);
        }
        let bal_label = pos2(screen.left() + 286.0, mix_y + 2.0);
        ui.painter()
            .text(bal_label, Align2::LEFT_TOP, "BAL", p.text_font(7.5), p.dim);
        let bal_rect = Rect::from_min_size(
            pos2(screen.left() + 316.0, mix_y),
            vec2((screen.right() - screen.left() - 316.0).max(60.0), 14.0),
        );
        let (_, balance) = slider(
            ui,
            bal_rect,
            "main_balance",
            &p,
            (self.balance + 1.0) * 0.5,
            true,
            "Баланс",
        );
        if let Some(balance) = balance {
            self.set_balance(balance * 2.0 - 1.0);
        }

        self.draw_clutter(ui, screen, body);

        if let Some(toast) = &self.toast {
            if Instant::now() < toast.until {
                let toast_rect = Rect::from_min_size(
                    pos2(rect.left() + 8.0, rect.bottom() - 28.0),
                    vec2(rect.width() - 46.0, 21.0),
                );
                let color = if toast.error { p.red } else { p.amber };
                ui.painter()
                    .rect_filled(toast_rect, 0.0, Color32::from_black_alpha(220));
                ui.painter().rect_stroke(
                    toast_rect,
                    0.0,
                    Stroke::new(1.0, color),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    toast_rect.left_center() + vec2(8.0, 0.0),
                    Align2::LEFT_CENTER,
                    &toast.text,
                    p.text_font(8.0),
                    color,
                );
            }
        }

        self.menu_popup(ui.ctx(), &menu_response);
    }

    fn draw_shaded(&mut self, ui: &mut Ui, rect: Rect) {
        let p = self.palette;
        self.draw_window_base(ui, rect, "MACAMP 2.0");

        let drag_rect = Rect::from_min_max(rect.min, pos2(rect.right() - 82.0, rect.bottom()));
        let drag = ui.interact(
            drag_rect,
            Id::unique("main_shade_drag"),
            Sense::click_and_drag(),
        );
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            self.toggle_shade(ui.ctx());
        }

        let menu_rect = Rect::from_min_size(pos2(rect.right() - 78.0, 6.0), vec2(16.0, 16.0));
        let shade_rect = Rect::from_min_size(pos2(rect.right() - 58.0, 6.0), vec2(16.0, 16.0));
        let minimize_rect = Rect::from_min_size(pos2(rect.right() - 38.0, 6.0), vec2(16.0, 16.0));
        let close_rect = Rect::from_min_size(pos2(rect.right() - 18.0, 6.0), vec2(16.0, 16.0));

        let menu_response = tiny_title_button(ui, menu_rect, "menu", &p, Icon::Menu);
        if tiny_title_button(ui, shade_rect, "shade", &p, Icon::Shade)
            .on_hover_text("Вернуть обычное окно · двойной клик по полосе")
            .clicked()
        {
            self.toggle_shade(ui.ctx());
        }
        ui.painter().rect_filled(
            Rect::from_min_size(
                pos2(shade_rect.left() + 3.0, shade_rect.bottom() + 1.0),
                vec2(10.0, 1.0),
            ),
            0.0,
            p.led,
        );
        if tiny_title_button(ui, minimize_rect, "minimize", &p, Icon::Minimize).clicked() {
            ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
        }
        if tiny_title_button(ui, close_rect, "close", &p, Icon::Close).clicked() {
            self.save_now();
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        }

        let track = self.display_track();
        let lcd = Rect::from_min_size(
            pos2(rect.left() + 124.0, rect.top() + 5.0),
            vec2(92.0, 20.0),
        );
        inset(ui.painter(), lcd, p.screen, p.edge_lo, p.edge_hi);
        let time = if track.is_some() {
            format_time(self.display_position())
        } else {
            "--:--".to_owned()
        };
        ui.painter().text(
            pos2(lcd.right() - 4.0, lcd.center().y),
            Align2::RIGHT_CENTER,
            "88:88",
            p.text_font(14.0),
            p.led.gamma_multiply(0.08),
        );
        led_text(
            ui.painter(),
            pos2(lcd.right() - 4.0, lcd.center().y),
            Align2::RIGHT_CENTER,
            &time,
            14.0,
            if track.is_some() {
                p.led
            } else {
                p.led.gamma_multiply(0.4)
            },
            p.screen,
        );

        let button_y = rect.top() + 6.0;
        let button_h = 18.0;
        let mut x = rect.left() + 222.0;
        let prev = classic_button(
            ui,
            Rect::from_min_size(pos2(x, button_y), vec2(28.0, button_h)),
            "shade_prev",
            &p,
            Some(Icon::Previous),
            None,
            false,
            true,
        );
        x += 31.0;
        let play = classic_button(
            ui,
            Rect::from_min_size(pos2(x, button_y), vec2(28.0, button_h)),
            "shade_play",
            &p,
            Some(Icon::Play),
            None,
            self.playing,
            true,
        );
        x += 31.0;
        let pause = classic_button(
            ui,
            Rect::from_min_size(pos2(x, button_y), vec2(28.0, button_h)),
            "shade_pause",
            &p,
            Some(Icon::Pause),
            None,
            self.started && !self.playing,
            true,
        );
        x += 31.0;
        let stop = classic_button(
            ui,
            Rect::from_min_size(pos2(x, button_y), vec2(28.0, button_h)),
            "shade_stop",
            &p,
            Some(Icon::Stop),
            None,
            false,
            true,
        );
        x += 31.0;
        let next = classic_button(
            ui,
            Rect::from_min_size(pos2(x, button_y), vec2(28.0, button_h)),
            "shade_next",
            &p,
            Some(Icon::Next),
            None,
            false,
            true,
        );

        let eq_toggle = toggle_button(
            ui,
            Rect::from_min_size(pos2(rect.left() + 382.0, button_y), vec2(30.0, button_h)),
            "shade_eq",
            &p,
            "EQ",
            self.show_eq,
            "Показать или скрыть эквалайзер",
        );
        let pl_toggle = toggle_button(
            ui,
            Rect::from_min_size(pos2(rect.left() + 415.0, button_y), vec2(30.0, button_h)),
            "shade_pl",
            &p,
            "PL",
            self.show_playlist,
            "Показать или скрыть плейлист",
        );

        if prev.clicked() {
            self.previous_track();
        }
        if play.clicked() {
            self.play_selected_or_current();
        }
        if pause.clicked() && self.started && self.playing {
            self.toggle_play();
        }
        if stop.clicked() {
            self.stop_playback();
        }
        if next.clicked() {
            self.next_track();
        }
        if eq_toggle.clicked() {
            self.toggle_eq_window(ui.ctx());
        }
        if pl_toggle.clicked() {
            self.toggle_playlist(ui.ctx());
        }

        self.menu_popup(ui.ctx(), &menu_response);
    }

    fn draw_window_base(&self, ui: &mut Ui, rect: Rect, title: &str) {
        let p = self.palette;
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, p.frame);
        painter.rect_stroke(
            rect.shrink(0.5),
            0.0,
            Stroke::new(1.0, p.edge_lo),
            egui::StrokeKind::Inside,
        );
        let title_rect = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE_H));
        gradient_rect(painter, title_rect, p.title_top, p.title_bottom, 24);
        painter.line_segment(
            [title_rect.left_bottom(), title_rect.right_bottom()],
            Stroke::new(1.0, p.edge_lo),
        );
        painter.line_segment(
            [title_rect.left_top(), title_rect.right_top()],
            Stroke::new(1.0, p.edge_hi),
        );
        painter.line_segment(
            [
                title_rect.left_bottom() + vec2(1.0, -1.0),
                title_rect.right_bottom() + vec2(-1.0, -1.0),
            ],
            Stroke::new(1.0, p.title_top.gamma_multiply(0.35)),
        );

        let logo_x = rect.left() + 9.0;
        let logo_bottom = rect.top() + 21.0;
        let heights = [4.0, 8.0, 6.0, 11.0, 5.0];
        for (index, height) in heights.iter().enumerate() {
            let color = if index == 3 { p.led } else { p.amber };
            painter.rect_filled(
                Rect::from_min_size(
                    pos2(logo_x + index as f32 * 3.0, logo_bottom - height),
                    vec2(2.0, *height),
                ),
                0.0,
                color,
            );
        }
        painter.text(
            pos2(rect.left() + 28.0, rect.top() + 8.0),
            Align2::LEFT_TOP,
            title,
            p.text_font(10.0),
            p.title_text,
        );
        painter.text(
            pos2(rect.left() + 28.0, rect.top() + 17.5),
            Align2::LEFT_TOP,
            "CLASSIC",
            p.text_font(6.0),
            p.title_text.gamma_multiply(0.55),
        );

        let body = Rect::from_min_max(pos2(rect.left(), title_rect.bottom()), rect.max);
        painter.rect_filled(body, 0.0, p.frame);
        painter.line_segment(
            [body.left_top(), body.left_bottom()],
            Stroke::new(1.0, p.edge_hi.gamma_multiply(0.45)),
        );
    }

    fn draw_screen(&mut self, ui: &mut Ui, rect: Rect) {
        let p = self.palette;
        inset(ui.painter(), rect, p.screen, p.edge_lo, p.edge_hi);
        let inner = rect.shrink(2.0);
        ui.painter().line_segment(
            [inner.left_top(), inner.right_top()],
            Stroke::new(1.0, Color32::from_black_alpha(120)),
        );
        for y in (inner.top() as i32..inner.bottom() as i32).step_by(3) {
            ui.painter().line_segment(
                [pos2(inner.left(), y as f32), pos2(inner.right(), y as f32)],
                Stroke::new(1.0, Color32::from_black_alpha(55)),
            );
        }

        let lcd_w = 118.0;
        let divider_x = inner.right() - lcd_w - 6.0;
        ui.painter().line_segment(
            [
                pos2(divider_x, inner.top() + 4.0),
                pos2(divider_x, inner.bottom() - 4.0),
            ],
            Stroke::new(1.0, p.edge_lo),
        );
        let lcd = Rect::from_min_max(
            pos2(divider_x + 4.0, inner.top() + 3.0),
            pos2(inner.right() - 2.0, inner.bottom() - 3.0),
        );
        let track = self.display_track();
        let time = if track.is_some() {
            format_time(self.display_position())
        } else {
            "--:--".to_owned()
        };

        // Ghost segments behind the live time.
        ui.painter().text(
            pos2(lcd.right(), lcd.top() + 1.0),
            Align2::RIGHT_TOP,
            "88:88",
            p.text_font(27.0),
            p.led.gamma_multiply(0.065),
        );
        led_text(
            ui.painter(),
            pos2(lcd.right(), lcd.top() + 1.0),
            Align2::RIGHT_TOP,
            &time,
            27.0,
            if track.is_some() {
                p.led
            } else {
                p.led.gamma_multiply(0.4)
            },
            p.screen,
        );

        let (line1, line2) = if let Some(track) = track {
            let bitrate = track
                .bitrate_kbps
                .map(|rate| format!("{rate} KBPS"))
                .unwrap_or_else(|| "— KBPS".to_owned());
            let channels = match track.channels {
                Some(1) => "MONO",
                Some(2) => "STEREO",
                _ => "— CH",
            };
            (
                format!("{} · {bitrate}", track.codec),
                format!("{} · {channels}", track.sample_rate_label()),
            )
        } else {
            ("READY".to_owned(), "NO TRACK".to_owned())
        };
        ui.painter().text(
            pos2(lcd.right(), lcd.bottom() - 17.0),
            Align2::RIGHT_TOP,
            line1,
            p.mono(7.0),
            p.dim,
        );
        ui.painter().text(
            pos2(lcd.right(), lcd.bottom() - 8.0),
            Align2::RIGHT_TOP,
            line2,
            p.mono(7.0),
            p.dim,
        );
        if self.audio_error.is_some() {
            ui.painter().text(
                pos2(inner.left() + 6.0, inner.bottom() - 9.0),
                Align2::LEFT_TOP,
                "AUDIO OFFLINE",
                p.text_font(6.0),
                p.red,
            );
        }

        let spectrum_rect = Rect::from_min_max(
            pos2(inner.left() + 6.0, inner.top() + 5.0),
            pos2(divider_x - 6.0, inner.bottom() - 5.0),
        );
        if track.is_some() {
            draw_spectrum(ui.painter(), spectrum_rect, self.spectrum.levels(), &p);
        } else {
            ui.painter().text(
                spectrum_rect.center(),
                Align2::CENTER_CENTER,
                "DROP AUDIO FILES",
                p.text_font(10.0),
                p.led.gamma_multiply(0.28),
            );
        }
    }

    fn draw_trackline(&mut self, ui: &mut Ui, rect: Rect) {
        let p = self.palette;
        let track = self.display_track();
        let text = track
            .map(Track::display_title)
            .unwrap_or_else(|| "NO TRACK LOADED".to_owned());
        let font = p.text_font(10.5);
        let width = text_width(ui, &text, font);
        let offset = if self.playing && width > rect.width() - 40.0 {
            let cycle = width + 52.0;
            (self.marquee_clock.elapsed().as_secs_f32() * 26.0) % cycle
        } else {
            0.0
        };
        draw_marquee(
            ui,
            rect,
            &text,
            offset,
            &p,
            10.5,
            if track.is_some() { p.led } else { p.dim },
        );
        let count = self.tracks().len();
        let index_text = if count == 0 {
            "0/0".to_owned()
        } else {
            format!(
                "{}/{}",
                self.play_pos.min(count.saturating_sub(1)) + 1,
                count
            )
        };
        ui.painter().text(
            pos2(rect.right(), rect.top() + 1.0),
            Align2::RIGHT_TOP,
            index_text,
            p.mono(7.5),
            p.dim,
        );
    }

    fn draw_seek(&mut self, ui: &mut Ui, rect: Rect) {
        let p = self.palette;
        let duration = self
            .display_track()
            .and_then(Track::duration)
            .unwrap_or(Duration::ZERO);
        let value = if duration > Duration::ZERO {
            (self.display_position().as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (_response, new_value) =
            slider(ui, rect, "main_seek", &p, value, false, "Позиция трека");
        if let Some(value) = new_value {
            self.seek_to(duration.mul_f32(value));
        }
    }

    fn draw_clutter(&mut self, ui: &mut Ui, screen: Rect, body: Rect) {
        let p = self.palette;
        let x = screen.right() + 8.0;
        let mut y = screen.top() + 3.0;
        let letters = ["O", "A", "I", "D", "V"];
        for (index, letter) in letters.iter().enumerate() {
            let active = match index {
                1 => self.always_on_top,
                _ => false,
            };
            ui.painter().text(
                pos2(x, y),
                Align2::LEFT_TOP,
                *letter,
                p.text_font(7.0),
                if active {
                    p.led
                } else {
                    p.dim.gamma_multiply(0.8)
                },
            );
            y += 27.0;
        }
        // A small, honest label for the reserved strip.
        ui.painter().text(
            pos2(x - 1.0, body.bottom() - 11.0),
            Align2::LEFT_TOP,
            "2.0",
            p.text_font(5.0),
            p.dim.gamma_multiply(0.55),
        );
    }

    fn draw_playlist(&mut self, ui: &mut Ui) {
        self.playlist_frames = self.playlist_frames.saturating_add(1);
        if self.playlist_frames == 1 || self.playlist_frames.is_multiple_of(120) {
            tracing::info!("playlist ui frame {}", self.playlist_frames);
        }
        let outer_rect = ui.ctx().input(|input| input.viewport().outer_rect);
        let inner_rect = ui.ctx().input(|input| input.viewport().inner_rect);
        self.rects[WINDOW_PLAYLIST] = outer_rect;
        let p = self.palette;
        let rect = ui.max_rect();
        self.draw_window_base(ui, rect, "PLAYLIST EDITOR");

        let title_rect = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE_H));
        let drag_rect =
            Rect::from_min_max(rect.min, pos2(rect.right() - 28.0, rect.top() + TITLE_H));
        let drag = ui.interact(
            drag_rect,
            Id::unique("playlist_titlebar"),
            Sense::click_and_drag(),
        );
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        let close_rect = Rect::from_min_size(pos2(rect.right() - 18.0, 6.0), vec2(16.0, 16.0));
        if tiny_title_button(ui, close_rect, "playlist_close", &p, Icon::Close).clicked() {
            self.show_playlist = false;
            self.config.show_playlist = false;
            self.drag_track = None;
            self.drag_insert = None;
            self.mark_dirty();
        }
        if ui.ctx().input(|input| input.viewport().close_requested()) {
            self.show_playlist = false;
            self.config.show_playlist = false;
            self.drag_track = None;
            self.drag_insert = None;
            self.mark_dirty();
        }
        if Instant::now() >= self.zoom_guard_until {
            if let Some(outer) = outer_rect {
                let pos = outer.min;
                if self
                    .playlist_pos
                    .map(|old| old.distance(pos) > 0.5)
                    .unwrap_or(true)
                {
                    self.playlist_pos = Some(pos);
                    self.mark_dirty();
                }
            }
            if !self.playlist_root
                && self.playlist_resize_accum.is_none()
                && Instant::now() >= self.playlist_size_guard_until
            {
                if let Some(inner) = inner_rect {
                    let size = inner.size();
                    if size.x > 1.0 && size.y > 1.0 && (size - self.playlist_size).length() > 0.75 {
                        self.playlist_size = vec2(
                            size.x.clamp(PLAYLIST_MIN_W, PLAYLIST_MAX_W),
                            size.y.clamp(PLAYLIST_MIN_H, PLAYLIST_MAX_H),
                        );
                        self.mark_dirty();
                    }
                }
            }
        }

        let body = Rect::from_min_max(
            pos2(rect.left() + 5.0, title_rect.bottom() + 5.0),
            pos2(rect.right() - 5.0, rect.bottom() - 5.0),
        );
        let toolbar = Rect::from_min_size(body.min, vec2(body.width(), 24.0));
        let add_rect = Rect::from_min_size(toolbar.min, vec2(74.0, 22.0));
        let folder_rect = Rect::from_min_size(toolbar.min + vec2(79.0, 0.0), vec2(88.0, 22.0));
        let remove_rect = Rect::from_min_size(toolbar.min + vec2(172.0, 0.0), vec2(70.0, 22.0));
        let add = classic_button(
            ui,
            add_rect,
            "pl_add",
            &p,
            Some(Icon::Add),
            Some("ADD"),
            false,
            true,
        );
        let folder = classic_button(
            ui,
            folder_rect,
            "pl_folder",
            &p,
            Some(Icon::Folder),
            Some("FOLDER"),
            false,
            true,
        );
        let can_remove = self
            .selected
            .is_some_and(|selected| !self.started || self.current != Some(selected));
        let remove = classic_button(
            ui,
            remove_rect,
            "pl_remove",
            &p,
            Some(Icon::Remove),
            Some("REM"),
            false,
            can_remove,
        );
        let remove = remove.on_hover_text(if self.selected.is_none() {
            "Сначала выберите трек"
        } else if !can_remove {
            "Играющий трек: сначала Stop"
        } else {
            "Удалить выбранный трек"
        });
        if add.clicked() {
            self.open_files();
        }
        if folder.clicked() {
            self.open_folder();
        }
        if remove.clicked() {
            self.remove_selected();
        }
        let status = if self.loading_files > 0 {
            format!("READING TAGS · {}", self.loading_files)
        } else if self.audio_error.is_some() {
            "AUDIO ERROR".to_owned()
        } else {
            "READY".to_owned()
        };
        ui.painter().text(
            pos2(toolbar.right(), toolbar.top() + 7.0),
            Align2::RIGHT_TOP,
            status,
            p.text_font(6.5),
            if self.loading_files > 0 {
                p.amber
            } else {
                p.dim
            },
        );

        let filter_row = Rect::from_min_size(
            pos2(body.left(), toolbar.bottom() + 4.0),
            vec2(body.width(), 20.0),
        );
        ui.painter().text(
            pos2(filter_row.left() + 4.0, filter_row.center().y),
            Align2::LEFT_CENTER,
            "FIND",
            p.text_font(6.5),
            p.dim,
        );
        let field = Rect::from_min_max(
            pos2(filter_row.left() + 32.0, filter_row.top()),
            pos2(filter_row.right() - 2.0, filter_row.bottom()),
        );
        inset(ui.painter(), field, p.inset, p.edge_lo, p.edge_hi);
        let text_rect = Rect::from_min_max(
            field.min + vec2(4.0, 2.0),
            pos2(field.right() - 22.0, field.bottom() - 2.0),
        );
        let editor = egui::TextEdit::singleline(&mut self.filter)
            .font(p.text_font(8.5))
            .text_color(p.text)
            .hint_text("title · artist · album")
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .desired_width(text_rect.width());
        let edit_response = ui
            .scope(|ui| {
                ui.style_mut().visuals.selection.bg_fill = p.select;
                ui.style_mut().visuals.selection.stroke = Stroke::new(1.0, p.led);
                ui.style_mut().visuals.widgets.active.bg_fill = Color32::TRANSPARENT;
                ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
                ui.style_mut().visuals.widgets.hovered.bg_fill = Color32::TRANSPARENT;
                ui.put(text_rect, editor)
            })
            .inner;
        if edit_response.has_focus() && ui.input(|input| input.key_pressed(Key::Escape)) {
            self.filter.clear();
        }
        if self.filter_active() {
            let clear_rect = Rect::from_min_size(
                pos2(field.right() - 18.0, field.top() + 2.0),
                vec2(14.0, 16.0),
            );
            if tiny_title_button(ui, clear_rect, "pl_filter_clear", &p, Icon::Close)
                .on_hover_text("Очистить фильтр")
                .clicked()
            {
                self.filter.clear();
            }
        }
        let filtering = self.filter_active();
        let visible = self.visible_indices();
        let count_text = if filtering {
            format!("{} / {}", visible.len(), self.tracks().len())
        } else {
            format!("{} TRACKS", self.tracks().len())
        };
        ui.painter().text(
            pos2(rect.right() - 26.0, rect.top() + 9.0),
            Align2::RIGHT_TOP,
            count_text,
            p.mono(7.5),
            p.dim,
        );

        let header_y = filter_row.bottom() + 5.0;
        let header = Rect::from_min_size(
            pos2(body.left() + 4.0, header_y),
            vec2(body.width() - 14.0, 12.0),
        );
        ui.painter()
            .text(header.left_top(), Align2::LEFT_TOP, "#", p.mono(7.5), p.dim);
        ui.painter().text(
            header.left_top() + vec2(28.0, 0.0),
            Align2::LEFT_TOP,
            "TITLE",
            p.mono(7.5),
            p.dim,
        );
        ui.painter().text(
            pos2(header.right(), header.top()),
            Align2::RIGHT_TOP,
            "TIME",
            p.mono(7.5),
            p.dim,
        );

        let footer_h = 18.0;
        let list = Rect::from_min_max(
            pos2(body.left() + 4.0, header.bottom() + 4.0),
            pos2(body.right() - 4.0, body.bottom() - footer_h - 4.0),
        );
        inset(ui.painter(), list, p.inset, p.edge_hi, p.edge_lo);
        let scrollbar_w = 10.0;
        let list_inner = Rect::from_min_max(
            list.min + vec2(1.0, 1.0),
            pos2(list.right() - scrollbar_w - 1.0, list.bottom() - 1.0),
        );
        let content_h = visible.len() as f32 * ROW_H;
        let view_h = list_inner.height();
        let max_scroll = (content_h - view_h).max(0.0);
        if ui.rect_contains_pointer(list) {
            let wheel = ui.input(|input| input.smooth_scroll_delta.y);
            if wheel != 0.0 {
                self.playlist_scroll -= wheel;
            }
        }
        self.playlist_scroll = self.playlist_scroll.clamp(0.0, max_scroll);

        if self.follow_current {
            if let Some(position) = self
                .current
                .and_then(|current| visible.iter().position(|index| *index == current))
            {
                let top = position as f32 * ROW_H;
                let bottom = top + ROW_H;
                if top < self.playlist_scroll {
                    self.playlist_scroll = top;
                } else if bottom > self.playlist_scroll + view_h {
                    self.playlist_scroll = bottom - view_h;
                }
                self.playlist_scroll = self.playlist_scroll.clamp(0.0, max_scroll);
            }
            self.follow_current = false;
        }

        if visible.is_empty() {
            ui.painter().text(
                list_inner.center(),
                Align2::CENTER_CENTER,
                if filtering {
                    "NO MATCHES · ESC TO CLEAR"
                } else {
                    "DROP AUDIO FILES OR PRESS ADD"
                },
                p.text_font(9.5),
                p.led.gamma_multiply(0.35),
            );
        } else {
            let first = (self.playlist_scroll / ROW_H).floor().max(0.0) as usize;
            let visible_rows = (view_h / ROW_H).ceil() as usize + 1;
            let end = (first + visible_rows).min(visible.len());
            let list_painter = ui.painter().with_clip_rect(list_inner);
            let mut clicked: Option<usize> = None;
            for (position, track_index) in visible.iter().enumerate().take(end).skip(first) {
                let track_index = *track_index;
                let y = list_inner.top() + position as f32 * ROW_H - self.playlist_scroll;
                let row = Rect::from_min_size(
                    pos2(list_inner.left(), y),
                    vec2(list_inner.width(), ROW_H),
                );
                if row.bottom() < list_inner.top() || row.top() > list_inner.bottom() {
                    continue;
                }
                let response = ui.interact(
                    row,
                    Id::unique(("playlist_row", track_index)),
                    Sense::click_and_drag(),
                );
                if response.clicked() {
                    self.selected = Some(track_index);
                }
                if response.double_clicked() {
                    clicked = Some(track_index);
                }
                if !filtering && response.drag_started() {
                    self.drag_track = Some(track_index);
                    self.drag_insert = Some(position);
                    self.selected = Some(track_index);
                }
                let is_current = self.current == Some(track_index);
                let is_selected = self.selected == Some(track_index);
                let is_dragging = self.drag_track == Some(track_index);
                let hovered = response.hovered();
                if is_dragging {
                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                } else if hovered && !filtering {
                    ui.ctx().set_cursor_icon(CursorIcon::Grab);
                }
                let bg = if is_dragging {
                    lerp_color(p.face_dark, p.led, 0.28)
                } else if is_current {
                    p.select
                } else if is_selected {
                    lerp_color(p.face_dark, p.select, 0.35)
                } else if hovered {
                    lerp_color(p.inset, p.face_dark, 0.4)
                } else if position % 2 == 0 {
                    p.screen
                } else {
                    p.inset
                };
                list_painter.rect_filled(row, 0.0, bg);
                if is_selected && !is_current {
                    list_painter.rect_stroke(
                        row.shrink(0.5),
                        0.0,
                        Stroke::new(1.0, p.edge_hi.gamma_multiply(0.6)),
                        egui::StrokeKind::Inside,
                    );
                }
                let number_color = if is_current { p.led } else { p.dim };
                let index_text = format!("{:02}", track_index + 1);
                list_painter.text(
                    pos2(row.left() + 5.0, row.center().y),
                    Align2::LEFT_CENTER,
                    index_text,
                    p.mono(8.0),
                    number_color,
                );
                if is_current {
                    let icon_rect = Rect::from_min_size(
                        pos2(row.left() + 25.0, row.center().y - 4.0),
                        vec2(8.0, 8.0),
                    );
                    draw_icon(
                        &list_painter,
                        if self.playing {
                            Icon::Play
                        } else {
                            Icon::Pause
                        },
                        icon_rect,
                        p.led,
                    );
                }
                let title_rect = Rect::from_min_max(
                    pos2(row.left() + 38.0, row.top()),
                    pos2(row.right() - 52.0, row.bottom()),
                );
                let title_painter = list_painter.with_clip_rect(title_rect);
                title_painter.text(
                    pos2(title_rect.left(), row.center().y),
                    Align2::LEFT_CENTER,
                    self.tracks()[track_index].display_title(),
                    p.text_font(9.5),
                    if is_current { p.select_text } else { p.text },
                );
                list_painter.text(
                    pos2(row.right() - 5.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    self.tracks()[track_index]
                        .duration()
                        .map(format_time)
                        .unwrap_or_else(|| "--:--".to_owned()),
                    p.mono(8.0),
                    if is_current { p.select_text } else { p.dim },
                );
                if hovered && !is_dragging {
                    let track = &self.tracks()[track_index];
                    let mut tip = track.display_title();
                    if !track.album.is_empty() {
                        tip.push_str(" · ");
                        tip.push_str(&track.album);
                    }
                    response.on_hover_text(tip);
                }
            }
            if let Some(index) = clicked {
                self.selected = Some(index);
                self.start_track_index(index, Duration::ZERO, true);
            }
            if let Some(slot) = self.drag_insert {
                if self.drag_track.is_some() {
                    let y = list_inner.top() + slot as f32 * ROW_H - self.playlist_scroll;
                    if y >= list_inner.top() - 1.0 && y <= list_inner.bottom() + 1.0 {
                        let line = Rect::from_min_max(
                            pos2(list_inner.left(), y - 1.0),
                            pos2(list_inner.right(), y + 1.0),
                        );
                        list_painter.rect_filled(line.intersect(list_inner), 0.0, p.led);
                    }
                }
            }
            if content_h > view_h + 0.5 {
                let scrollbar = Rect::from_min_max(
                    pos2(list.right() - scrollbar_w - 2.0, list.top() + 2.0),
                    pos2(list.right() - 2.0, list.bottom() - 2.0),
                );
                let sb_response = draw_scrollbar(
                    ui,
                    scrollbar,
                    "pl_scroll",
                    &p,
                    self.playlist_scroll,
                    content_h,
                    view_h,
                );
                if sb_response.dragged() || sb_response.clicked() {
                    if let Some(t) = scroll_to_value(ui, "pl_scroll") {
                        self.playlist_scroll = t * max_scroll;
                    }
                }
            }
        }

        if self.drag_track.is_some() {
            if filtering {
                self.drag_track = None;
                self.drag_insert = None;
            } else if !ui.input(|input| input.pointer.any_down()) {
                let from = self.drag_track.take().unwrap();
                let slot = self.drag_insert.take().unwrap_or(from);
                let to = if slot > from { slot - 1 } else { slot };
                self.selected = Some(from);
                self.move_track(from, to);
            } else if let Some(pointer) = ui.ctx().pointer_interact_pos() {
                let slot = ((pointer.y - list_inner.top() + self.playlist_scroll) / ROW_H)
                    .round()
                    .clamp(0.0, visible.len() as f32) as usize;
                self.drag_insert = Some(slot);
                if pointer.y < list_inner.top() + ROW_H && self.playlist_scroll > 0.0 {
                    self.playlist_scroll = (self.playlist_scroll - 6.0).max(0.0);
                } else if pointer.y > list_inner.bottom() - ROW_H
                    && self.playlist_scroll < max_scroll
                {
                    self.playlist_scroll = (self.playlist_scroll + 6.0).min(max_scroll);
                }
                ui.ctx().request_repaint_after(Duration::from_millis(16));
            }
        }

        let footer = Rect::from_min_size(
            pos2(body.left(), body.bottom() - footer_h),
            vec2(body.width(), footer_h),
        );
        if !self.playlist_root {
            let grip = Rect::from_min_size(
                pos2(body.right() - 14.0, body.bottom() - 14.0),
                vec2(14.0, 14.0),
            );
            let grip_response = ui
                .interact(grip, Id::unique("pl_resize_grip"), Sense::drag())
                .on_hover_text("Потяните, чтобы изменить размер плейлиста");
            if grip_response.hovered() || grip_response.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeNwSe);
            }
            if grip_response.drag_started() {
                self.playlist_resize_accum = Some(self.playlist_size);
            }
            if grip_response.dragged() {
                let mut accum = self.playlist_resize_accum.unwrap_or(self.playlist_size);
                accum += grip_response.drag_delta();
                self.playlist_resize_accum = Some(accum);
                let size = vec2(
                    accum.x.clamp(PLAYLIST_MIN_W, PLAYLIST_MAX_W),
                    accum.y.clamp(PLAYLIST_MIN_H, PLAYLIST_MAX_H),
                );
                if (size - self.playlist_size).length() > 0.5 {
                    self.playlist_size = size;
                    self.playlist_size_guard_until = Instant::now() + Duration::from_millis(350);
                    ui.ctx().send_viewport_cmd_to(
                        playlist_viewport_id(),
                        ViewportCommand::InnerSize(size),
                    );
                    self.mark_dirty();
                }
            }
            if grip_response.drag_stopped() {
                self.playlist_resize_accum = None;
                self.playlist_size_guard_until = Instant::now() + Duration::from_millis(350);
            }
            for index in 0..3 {
                let offset = 4.0 + index as f32 * 3.0;
                ui.painter().line_segment(
                    [
                        pos2(grip.right() - offset, grip.bottom() - 2.0),
                        pos2(grip.right() - 2.0, grip.bottom() - offset),
                    ],
                    Stroke::new(1.0, p.dim.gamma_multiply(0.55)),
                );
            }
        }
        let footer_text = if self.tracks().is_empty() {
            "0 TRACKS".to_owned()
        } else if filtering {
            let shown: Duration = visible
                .iter()
                .filter_map(|index| self.tracks()[*index].duration())
                .sum();
            format!(
                "{} / {} TRACKS · {}",
                visible.len(),
                self.tracks().len(),
                format_long_duration(shown)
            )
        } else {
            format!(
                "{} TRACKS · {}",
                self.tracks().len(),
                format_long_duration(self.total_duration())
            )
        };
        ui.painter().text(
            pos2(footer.left() + 4.0, footer.center().y),
            Align2::LEFT_CENTER,
            footer_text,
            p.text_font(7.5),
            p.dim,
        );
        ui.painter().text(
            pos2(footer.right() - 18.0, footer.center().y),
            Align2::RIGHT_CENTER,
            if self.playing {
                "PLAYING"
            } else if self.started {
                "PAUSED"
            } else {
                "DOUBLE-CLICK TO PLAY"
            },
            p.text_font(6.5),
            if self.started { p.led } else { p.dim },
        );

        self.handle_keys(ui.ctx(), true);
        self.handle_drops(ui.ctx());
    }

    fn draw_drive(&mut self, ui: &mut Ui) {
        self.drive.frames = self.drive.frames.saturating_add(1);
        if self.drive.frames == 1 || self.drive.frames.is_multiple_of(120) {
            tracing::info!("drive ui frame {}", self.drive.frames);
        }
        let outer_rect = ui.ctx().input(|input| input.viewport().outer_rect);
        self.rects[WINDOW_DRIVE] = outer_rect;
        let p = self.palette;
        let rect = ui.max_rect();
        self.draw_window_base(ui, rect, "DRIVEMUSIC");

        let title_rect = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE_H));
        let drag_rect =
            Rect::from_min_max(rect.min, pos2(rect.right() - 28.0, rect.top() + TITLE_H));
        let drag = ui.interact(
            drag_rect,
            Id::unique("drive_titlebar"),
            Sense::click_and_drag(),
        );
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        let close_rect = Rect::from_min_size(pos2(rect.right() - 18.0, 6.0), vec2(16.0, 16.0));
        if tiny_title_button(ui, close_rect, "drive_close", &p, Icon::Close).clicked() {
            self.show_drive = false;
            self.config.show_drive = false;
            self.mark_dirty();
        }
        if ui.ctx().input(|input| input.viewport().close_requested()) {
            self.show_drive = false;
            self.config.show_drive = false;
            self.mark_dirty();
        }
        ui.painter().text(
            pos2(rect.right() - 26.0, rect.top() + 9.0),
            Align2::RIGHT_TOP,
            "ONLINE",
            p.mono(7.5),
            p.dim,
        );
        if !self.drive_root && Instant::now() >= self.zoom_guard_until {
            if let Some(outer) = outer_rect {
                let pos = outer.min;
                if self
                    .drive_pos
                    .map(|old| old.distance(pos) > 0.5)
                    .unwrap_or(true)
                {
                    self.drive_pos = Some(pos);
                    self.mark_dirty();
                }
            }
        }

        let body = Rect::from_min_max(
            pos2(rect.left() + 5.0, title_rect.bottom() + 5.0),
            pos2(rect.right() - 5.0, rect.bottom() - 5.0),
        );
        let search_row = Rect::from_min_size(body.min, vec2(body.width(), 24.0));
        ui.painter().text(
            pos2(search_row.left() + 4.0, search_row.center().y),
            Align2::LEFT_CENTER,
            "FIND",
            p.text_font(6.5),
            p.dim,
        );
        let field = Rect::from_min_max(
            pos2(search_row.left() + 32.0, search_row.top()),
            pos2(search_row.right() - 72.0, search_row.bottom()),
        );
        inset(ui.painter(), field, p.inset, p.edge_lo, p.edge_hi);
        let text_rect = Rect::from_min_max(field.min + vec2(4.0, 2.0), field.max - vec2(4.0, 2.0));
        let editor = egui::TextEdit::singleline(&mut self.drive.query)
            .font(p.text_font(8.5))
            .text_color(p.text)
            .hint_text("исполнитель или трек")
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .desired_width(text_rect.width());
        let edit_response = ui
            .scope(|ui| {
                ui.style_mut().visuals.selection.bg_fill = p.select;
                ui.style_mut().visuals.selection.stroke = Stroke::new(1.0, p.led);
                ui.style_mut().visuals.widgets.active.bg_fill = Color32::TRANSPARENT;
                ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
                ui.style_mut().visuals.widgets.hovered.bg_fill = Color32::TRANSPARENT;
                ui.put(text_rect, editor)
            })
            .inner;
        let enter = ui.input(|input| input.key_pressed(Key::Enter))
            && (edit_response.has_focus() || edit_response.lost_focus());
        let find_rect = Rect::from_min_size(
            pos2(search_row.right() - 66.0, search_row.top() + 1.0),
            vec2(64.0, 22.0),
        );
        let find = classic_button(
            ui,
            find_rect,
            "dm_find",
            &p,
            None,
            Some("FIND"),
            false,
            true,
        );
        let search_now = find.clicked() || enter;

        let header = Rect::from_min_size(
            pos2(body.left(), search_row.bottom() + 5.0),
            vec2(body.width(), 14.0),
        );
        let footer_h = 16.0;
        let footer = Rect::from_min_size(
            pos2(body.left(), body.bottom() - footer_h),
            vec2(body.width(), footer_h),
        );
        let buttons_y = footer.top() - 28.0;
        let list = Rect::from_min_max(
            pos2(body.left(), header.bottom() + 4.0),
            pos2(body.right(), buttons_y - 4.0),
        );
        inset(ui.painter(), list, p.inset, p.edge_hi, p.edge_lo);
        let scrollbar_w = 10.0;
        let list_inner = Rect::from_min_max(
            list.min + vec2(1.0, 1.0),
            pos2(list.right() - scrollbar_w - 1.0, list.bottom() - 1.0),
        );

        let mut selected = self.drive.selected;
        let mut scroll = self.drive.scroll;
        let mut action = DriveAction::None;

        {
            let view = &self.drive.view;
            let downloads = &self.drive.downloads;
            let count = match view {
                DriveView::Empty => 0,
                DriveView::Artists(artists) => artists.len(),
                DriveView::Artist(artist) => artist.tracks.len(),
            };
            let (heading, count_text) = match view {
                DriveView::Empty => ("DRIVEMUSIC", "ONLINE".to_owned()),
                DriveView::Artists(artists) => {
                    ("ИСПОЛНИТЕЛИ", format!("{} АРТИСТОВ", artists.len()))
                }
                DriveView::Artist(artist) => (
                    artist.name.as_str(),
                    format!("{} ТРЕКОВ · СТР. {}", artist.tracks.len(), artist.page),
                ),
            };
            ui.painter().text(
                pos2(header.left(), header.top()),
                Align2::LEFT_TOP,
                heading,
                p.mono(8.0),
                p.text,
            );
            ui.painter().text(
                pos2(header.right(), header.top()),
                Align2::RIGHT_TOP,
                count_text,
                p.mono(7.5),
                p.dim,
            );

            let content_h = count as f32 * ROW_H;
            let view_h = list_inner.height();
            let max_scroll = (content_h - view_h).max(0.0);
            if ui.rect_contains_pointer(list) {
                let wheel = ui.input(|input| input.smooth_scroll_delta.y);
                if wheel != 0.0 {
                    scroll -= wheel;
                }
            }
            scroll = scroll.clamp(0.0, max_scroll);

            if count == 0 {
                let message = if self.drive.loading.is_some() {
                    "ЗАГРУЗКА…"
                } else if matches!(view, DriveView::Empty) {
                    "ВВЕДИ ИМЯ ИСПОЛНИТЕЛЯ И НАЖМИ FIND"
                } else {
                    "ПУСТО"
                };
                ui.painter().text(
                    list_inner.center(),
                    Align2::CENTER_CENTER,
                    message,
                    p.text_font(9.0),
                    p.led.gamma_multiply(0.3),
                );
            } else {
                let first = (scroll / ROW_H).floor().max(0.0) as usize;
                let visible_rows = (view_h / ROW_H).ceil() as usize + 1;
                let end = (first + visible_rows).min(count);
                let list_painter = ui.painter().with_clip_rect(list_inner);
                for index in first..end {
                    let y = list_inner.top() + index as f32 * ROW_H - scroll;
                    let row = Rect::from_min_size(
                        pos2(list_inner.left(), y),
                        vec2(list_inner.width(), ROW_H),
                    );
                    if row.bottom() < list_inner.top() || row.top() > list_inner.bottom() {
                        continue;
                    }
                    let response = ui.interact(
                        row,
                        Id::unique(("dm_row", matches!(view, DriveView::Artist(_)), index)),
                        Sense::click(),
                    );
                    if response.clicked() {
                        selected = Some(index);
                    }
                    if response.double_clicked() {
                        action = match view {
                            DriveView::Artists(_) => DriveAction::Open(index),
                            DriveView::Artist(_) => DriveAction::Play(index),
                            DriveView::Empty => DriveAction::None,
                        };
                    }
                    let is_selected = selected == Some(index);
                    let hovered = response.hovered();
                    if hovered {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    let bg = if is_selected {
                        lerp_color(p.face_dark, p.select, 0.45)
                    } else if hovered {
                        lerp_color(p.inset, p.face_dark, 0.4)
                    } else if index % 2 == 0 {
                        p.screen
                    } else {
                        p.inset
                    };
                    list_painter.rect_filled(row, 0.0, bg);
                    list_painter.text(
                        pos2(row.left() + 5.0, row.center().y),
                        Align2::LEFT_CENTER,
                        format!("{:02}", index + 1),
                        p.mono(8.0),
                        if is_selected { p.led } else { p.dim },
                    );
                    match view {
                        DriveView::Artists(artists) => {
                            let artist = &artists[index];
                            let name_rect =
                                Rect::from_min_max(pos2(row.left() + 32.0, row.top()), row.max);
                            list_painter.with_clip_rect(name_rect).text(
                                pos2(name_rect.left(), row.center().y),
                                Align2::LEFT_CENTER,
                                &artist.name,
                                p.text_font(9.5),
                                if is_selected { p.select_text } else { p.text },
                            );
                            if hovered {
                                response.on_hover_text(format!(
                                    "{}\nдвойной клик — открыть артиста",
                                    artist.name
                                ));
                            }
                        }
                        DriveView::Artist(artist_view) => {
                            let track = &artist_view.tracks[index];
                            let title_rect = Rect::from_min_max(
                                pos2(row.left() + 32.0, row.top()),
                                pos2(row.right() - 100.0, row.bottom()),
                            );
                            list_painter.with_clip_rect(title_rect).text(
                                pos2(title_rect.left(), row.center().y),
                                Align2::LEFT_CENTER,
                                remote_title(track),
                                p.text_font(9.0),
                                if is_selected { p.select_text } else { p.text },
                            );
                            let duration = track
                                .duration_ms
                                .map(|ms| format_time(Duration::from_millis(ms)))
                                .unwrap_or_else(|| "--:--".to_owned());
                            list_painter.text(
                                pos2(row.right() - 52.0, row.center().y),
                                Align2::RIGHT_CENTER,
                                duration,
                                p.mono(8.0),
                                if is_selected { p.select_text } else { p.dim },
                            );
                            let status = if let Some(progress) = downloads.get(&track.cache_key) {
                                (
                                    progress
                                        .map(|fraction| format!("{}%", (fraction * 100.0) as u32))
                                        .unwrap_or_else(|| "…".to_owned()),
                                    p.amber,
                                )
                            } else if is_cached(track) {
                                ("✓".to_owned(), p.led)
                            } else {
                                (String::new(), p.dim)
                            };
                            if !status.0.is_empty() {
                                list_painter.text(
                                    pos2(row.right() - 4.0, row.center().y),
                                    Align2::RIGHT_CENTER,
                                    status.0,
                                    p.mono(7.5),
                                    status.1,
                                );
                            }
                            if hovered {
                                let mut tip = remote_title(track);
                                tip.push_str("\nдвойной клик — играть");
                                response.on_hover_text(tip);
                            }
                        }
                        DriveView::Empty => {}
                    }
                }
                if content_h > view_h + 0.5 {
                    let scrollbar = Rect::from_min_max(
                        pos2(list.right() - scrollbar_w - 2.0, list.top() + 2.0),
                        pos2(list.right() - 2.0, list.bottom() - 2.0),
                    );
                    let sb_response =
                        draw_scrollbar(ui, scrollbar, "dm_scroll", &p, scroll, content_h, view_h);
                    if sb_response.dragged() || sb_response.clicked() {
                        if let Some(t) = scroll_to_value(ui, "dm_scroll") {
                            scroll = t * max_scroll;
                        }
                    }
                }
            }

            // Keyboard navigation, unless the search field has focus.
            if !ui.ctx().egui_wants_keyboard_input() && count > 0 {
                let down = ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowDown));
                let up = ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowUp));
                if down || up {
                    let current = selected.unwrap_or(0) as i32 + if down { 1 } else { -1 };
                    selected = Some(current.clamp(0, count as i32 - 1) as usize);
                    let top = selected.unwrap() as f32 * ROW_H;
                    if top < scroll {
                        scroll = top;
                    } else if top + ROW_H > scroll + view_h {
                        scroll = top + ROW_H - view_h;
                    }
                    scroll = scroll.clamp(0.0, max_scroll);
                }
                if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
                    if let Some(index) = selected {
                        action = match view {
                            DriveView::Artists(_) => DriveAction::Open(index),
                            DriveView::Artist(_) => DriveAction::Play(index),
                            DriveView::Empty => DriveAction::None,
                        };
                    }
                }
            }

            // Action bar.
            let mut x = body.left();
            let button_h = 24.0;
            match view {
                DriveView::Empty => {}
                DriveView::Artists(_) => {
                    let enabled = selected.is_some();
                    if classic_button(
                        ui,
                        Rect::from_min_size(pos2(x, buttons_y), vec2(76.0, button_h)),
                        "dm_open",
                        &p,
                        None,
                        Some("OPEN"),
                        false,
                        enabled,
                    )
                    .clicked()
                    {
                        if let Some(index) = selected {
                            action = DriveAction::Open(index);
                        }
                    }
                }
                DriveView::Artist(artist) => {
                    if classic_button(
                        ui,
                        Rect::from_min_size(pos2(x, buttons_y), vec2(58.0, button_h)),
                        "dm_back",
                        &p,
                        None,
                        Some("BACK"),
                        false,
                        true,
                    )
                    .clicked()
                    {
                        action = DriveAction::Back;
                    }
                    x += 62.0;
                    let enabled = selected.is_some_and(|index| index < artist.tracks.len());
                    if classic_button(
                        ui,
                        Rect::from_min_size(pos2(x, buttons_y), vec2(62.0, button_h)),
                        "dm_play",
                        &p,
                        None,
                        Some("PLAY"),
                        false,
                        enabled,
                    )
                    .clicked()
                    {
                        if let Some(index) = selected {
                            action = DriveAction::Play(index);
                        }
                    }
                    x += 66.0;
                    if classic_button(
                        ui,
                        Rect::from_min_size(pos2(x, buttons_y), vec2(54.0, button_h)),
                        "dm_add",
                        &p,
                        None,
                        Some("ADD"),
                        false,
                        enabled,
                    )
                    .clicked()
                    {
                        if let Some(index) = selected {
                            action = DriveAction::Add(index);
                        }
                    }
                    x += 58.0;
                    if classic_button(
                        ui,
                        Rect::from_min_size(pos2(x, buttons_y), vec2(74.0, button_h)),
                        "dm_add_all",
                        &p,
                        None,
                        Some("ADD ALL"),
                        false,
                        !artist.tracks.is_empty(),
                    )
                    .clicked()
                    {
                        action = DriveAction::AddAll;
                    }
                    x += 78.0;
                    if artist.next_url.is_some()
                        && classic_button(
                            ui,
                            Rect::from_min_size(pos2(x, buttons_y), vec2(64.0, button_h)),
                            "dm_more",
                            &p,
                            None,
                            Some("MORE"),
                            false,
                            self.drive.loading.is_none(),
                        )
                        .clicked()
                    {
                        action = DriveAction::LoadMore;
                    }
                }
            }
        }

        self.drive.selected = selected;
        self.drive.scroll = scroll;

        ui.painter()
            .with_clip_rect(Rect::from_min_max(
                footer.min,
                pos2(footer.right() - 44.0, footer.bottom()),
            ))
            .text(
                pos2(footer.left() + 2.0, footer.center().y),
                Align2::LEFT_CENTER,
                self.drive
                    .loading
                    .as_deref()
                    .unwrap_or(self.drive.status.as_str()),
                p.text_font(7.0),
                if self.drive.error {
                    p.red
                } else if self.drive.loading.is_some() {
                    p.amber
                } else {
                    p.dim
                },
            );
        if !self.drive.downloads.is_empty() {
            ui.painter().text(
                pos2(footer.right() - 2.0, footer.center().y),
                Align2::RIGHT_CENTER,
                format!("↓ {}", self.drive.downloads.len()),
                p.mono(8.0),
                p.amber,
            );
        }

        if search_now {
            self.drive.start_search(self.drive.query.clone());
        }
        match action {
            DriveAction::None => {}
            DriveAction::Open(index) => {
                if let DriveView::Artists(artists) = &self.drive.view {
                    if let Some(artist) = artists.get(index) {
                        let url = artist.url.clone();
                        self.drive.open_artist(url);
                    }
                }
            }
            DriveAction::Play(index) | DriveAction::Add(index) => {
                if let DriveView::Artist(artist) = &self.drive.view {
                    if let Some(track) = artist.tracks.get(index).cloned() {
                        let intent = match action {
                            DriveAction::Play(_) => DownloadIntent::Play,
                            _ => DownloadIntent::Add,
                        };
                        self.request_remote(track, intent);
                    }
                }
            }
            DriveAction::AddAll => self.add_all_remote(),
            DriveAction::LoadMore => self.drive.load_more(),
            DriveAction::Back => {
                self.drive.selected = None;
                self.drive.scroll = 0.0;
                self.drive.view = if self.drive.search_artists.is_empty() {
                    DriveView::Empty
                } else {
                    DriveView::Artists(self.drive.search_artists.clone())
                };
                self.drive.status = "АРТИСТЫ".to_owned();
                self.drive.error = false;
            }
        }

        self.handle_keys(ui.ctx(), false);
    }

    fn draw_eq(&mut self, ui: &mut Ui) {
        let outer_rect = ui.ctx().input(|input| input.viewport().outer_rect);
        self.rects[WINDOW_EQ] = outer_rect;
        let p = self.palette;
        let rect = ui.max_rect();
        self.draw_window_base(ui, rect, "EQUALIZER");

        let title_rect = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE_H));
        let drag_rect =
            Rect::from_min_max(rect.min, pos2(rect.right() - 28.0, rect.top() + TITLE_H));
        let drag = ui.interact(
            drag_rect,
            Id::unique("eq_titlebar"),
            Sense::click_and_drag(),
        );
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        let close_rect = Rect::from_min_size(pos2(rect.right() - 18.0, 6.0), vec2(16.0, 16.0));
        if tiny_title_button(ui, close_rect, "eq_close", &p, Icon::Close).clicked() {
            self.show_eq = false;
            self.config.show_eq = false;
            self.mark_dirty();
        }
        if ui.ctx().input(|input| input.viewport().close_requested()) {
            self.show_eq = false;
            self.config.show_eq = false;
            self.mark_dirty();
        }
        if Instant::now() >= self.zoom_guard_until {
            if let Some(outer) = outer_rect {
                let pos = outer.min;
                if self
                    .eq_pos
                    .map(|old| old.distance(pos) > 0.5)
                    .unwrap_or(true)
                {
                    self.eq_pos = Some(pos);
                    self.mark_dirty();
                }
            }
        }
        ui.painter().text(
            pos2(rect.right() - 26.0, rect.top() + 9.0),
            Align2::RIGHT_TOP,
            "10 BAND",
            p.mono(7.5),
            p.dim,
        );

        let body = Rect::from_min_max(
            pos2(rect.left() + 6.0, title_rect.bottom() + 6.0),
            pos2(rect.right() - 6.0, rect.bottom() - 6.0),
        );
        let top = Rect::from_min_size(body.min, vec2(body.width(), 23.0));
        let on_rect = Rect::from_min_size(top.min, vec2(40.0, 21.0));
        let on = toggle_button(
            ui,
            on_rect,
            "eq_on",
            &p,
            "ON",
            self.eq_enabled,
            "Включить или выключить эквалайзер",
        );
        if on.clicked() {
            self.eq_enabled = !self.eq_enabled;
            self.apply_eq();
        }
        let preset_rect =
            Rect::from_min_size(pos2(top.left() + 46.0, top.top()), vec2(230.0, 21.0));
        let preset_name = self.eq_preset_name();
        let preset = classic_button(
            ui,
            preset_rect,
            "eq_preset",
            &p,
            None,
            Some(preset_name),
            false,
            true,
        );
        if preset.clicked() {
            self.cycle_eq_preset();
        }
        ui.painter().text(
            pos2(preset_rect.right() + 8.0, top.top() + 7.0),
            Align2::LEFT_TOP,
            "PRESET",
            p.text_font(6.0),
            p.dim,
        );

        let graph = Rect::from_min_size(
            pos2(body.left(), top.bottom() + 6.0),
            vec2(body.width(), 42.0),
        );
        inset(ui.painter(), graph, p.screen, p.edge_lo, p.edge_hi);
        let plot = graph.shrink2(vec2(9.0, 7.0));
        ui.painter().line_segment(
            [
                pos2(plot.left(), plot.center().y),
                pos2(plot.right(), plot.center().y),
            ],
            Stroke::new(1.0, p.dim.gamma_multiply(0.35)),
        );
        for i in 0..10 {
            let x = plot.left() + i as f32 / 9.0 * plot.width();
            ui.painter().line_segment(
                [pos2(x, plot.top()), pos2(x, plot.bottom())],
                Stroke::new(1.0, p.dim.gamma_multiply(0.08)),
            );
        }
        let curve_color = if self.eq_enabled {
            p.led
        } else {
            p.led.gamma_multiply(0.35)
        };
        let mut last_point = None;
        let mut points = Vec::with_capacity(10);
        for (index, gain) in self.eq_gains.iter().enumerate() {
            let x = plot.left() + index as f32 / 9.0 * plot.width();
            let y = plot.center().y - (gain.clamp(-12.0, 12.0) / 12.0) * (plot.height() * 0.42);
            let point = pos2(x, y);
            if let Some(previous) = last_point {
                ui.painter()
                    .line_segment([previous, point], Stroke::new(1.5, curve_color));
            }
            last_point = Some(point);
            points.push(point);
        }
        for point in points {
            ui.painter()
                .circle_filled(point, 1.7, curve_color.gamma_multiply(0.9));
        }

        let rack_top = graph.bottom() + 7.0;
        let label_height = 12.0;
        let slot_height = (body.bottom() - rack_top - label_height).max(30.0);
        let gap = 4.0;
        let column_width = ((body.width() - gap * 9.0) / 10.0).max(18.0);
        let mut changed = false;
        for band in 0..10 {
            let x = body.left() + band as f32 * (column_width + gap);
            let slot = Rect::from_min_size(pos2(x, rack_top), vec2(column_width, slot_height));
            let response =
                ui.interact(slot, Id::unique(("eq_band", band)), Sense::click_and_drag());
            if response.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
            }
            if (response.clicked() || response.dragged()) && self.eq_enabled {
                if let Some(position) = response.interact_pointer_pos() {
                    let ratio =
                        1.0 - ((position.y - slot.top()) / slot.height().max(1.0)).clamp(0.0, 1.0);
                    let value = ((ratio * 24.0 - 12.0) * 2.0).round() / 2.0;
                    if (self.eq_gains[band] - value).abs() > 0.01 {
                        self.eq_gains[band] = value;
                        self.eq_preset = None;
                        changed = true;
                    }
                }
            }
            let gain = self.eq_gains[band];
            let ratio = ((gain + 12.0) / 24.0).clamp(0.0, 1.0);
            let thumb_y = slot.bottom() - ratio * slot.height();
            inset(ui.painter(), slot, p.inset, p.edge_lo, p.edge_hi);
            let center_y = slot.center().y;
            ui.painter().line_segment(
                [pos2(slot.left(), center_y), pos2(slot.right(), center_y)],
                Stroke::new(1.0, p.dim.gamma_multiply(0.32)),
            );
            let color = if self.eq_enabled {
                p.led
            } else {
                p.led.gamma_multiply(0.3)
            };
            let fill_top = thumb_y.min(center_y);
            let fill_bottom = thumb_y.max(center_y);
            ui.painter().rect_filled(
                Rect::from_min_max(
                    pos2(slot.center().x - 2.0, fill_top),
                    pos2(slot.center().x + 2.0, fill_bottom),
                ),
                0.0,
                color.gamma_multiply(0.5),
            );
            let thumb = Rect::from_min_max(
                pos2(slot.left() + 2.0, thumb_y - 3.0),
                pos2(slot.right() - 2.0, thumb_y + 3.0),
            );
            bevel(ui.painter(), thumb, p.thumb, p.edge_hi, p.edge_lo);
            ui.painter().line_segment(
                [thumb.center_top(), thumb.center_bottom()],
                Stroke::new(1.0, p.face_dark),
            );
            ui.painter().text(
                pos2(slot.center().x, slot.bottom() + 3.0),
                Align2::CENTER_TOP,
                EQ_LABELS[band],
                p.text_font(6.5),
                p.dim,
            );
        }
        if changed {
            self.apply_eq();
        }

        self.handle_keys(ui.ctx(), false);
        self.handle_drops(ui.ctx());
    }

    fn menu_popup(&mut self, ctx: &egui::Context, anchor: &Response) {
        let p = self.palette;
        let result = egui::Popup::from_toggle_button_response(anchor)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .layout(Layout::top_down(Align::Min))
            .frame(
                Frame::NONE
                    .fill(p.face_dark)
                    .stroke(Stroke::new(1.0, p.edge_lo))
                    .inner_margin(5.0),
            )
            .show(|ui| {
                ui.set_width(204.0);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let mut action = None;
                menu_section(ui, &p, "SKIN");
                for skin in Skin::ALL {
                    if menu_row(ui, &p, skin.name(), Some(self.skin == skin), true).clicked() {
                        action = Some(MenuAction::Skin(skin));
                    }
                }
                menu_separator(ui, &p);
                menu_section(ui, &p, "WINDOW");
                if menu_row(ui, &p, "Always on top", Some(self.always_on_top), true).clicked() {
                    action = Some(MenuAction::AlwaysOnTop);
                }
                if menu_row(ui, &p, "Windowshade", Some(self.shaded), true).clicked() {
                    action = Some(MenuAction::Windowshade);
                }
                if menu_row(ui, &p, "Double size", Some(self.double_size), true).clicked() {
                    action = Some(MenuAction::DoubleSize);
                }
                if menu_row(ui, &p, "Playlist", Some(self.show_playlist), true).clicked() {
                    action = Some(MenuAction::Playlist);
                }
                if menu_row(ui, &p, "DriveMusic", Some(self.show_drive), true).clicked() {
                    action = Some(MenuAction::DriveMusic);
                }
                menu_separator(ui, &p);
                menu_section(ui, &p, "FILES");
                if menu_row(ui, &p, "Open files…", None, true).clicked() {
                    action = Some(MenuAction::OpenFiles);
                }
                if menu_row(ui, &p, "Open folder…", None, true).clicked() {
                    action = Some(MenuAction::OpenFolder);
                }
                menu_separator(ui, &p);
                if menu_row(ui, &p, "About MacAmp", None, true).clicked() {
                    action = Some(MenuAction::About);
                }
                if menu_row(ui, &p, "Quit", None, true).clicked() {
                    action = Some(MenuAction::Quit);
                }
                action
            });
        if let Some(action) = result.and_then(|inner| inner.inner) {
            match action {
                MenuAction::Skin(skin) => self.apply_skin(skin),
                MenuAction::AlwaysOnTop => self.toggle_always_on_top(ctx),
                MenuAction::Windowshade => self.toggle_shade(ctx),
                MenuAction::DoubleSize => self.toggle_double_size(ctx),
                MenuAction::Playlist => self.toggle_playlist(ctx),
                MenuAction::DriveMusic => self.toggle_drive_window(ctx),
                MenuAction::OpenFiles => self.open_files(),
                MenuAction::OpenFolder => self.open_folder(),
                MenuAction::About => {
                    self.toast("MacAmp 2.0 · Rust, egui, Silkscreen и немного ламы.", false)
                }
                MenuAction::Quit => {
                    self.save_now();
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
        }
        if self.toast.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn drive_position(&self, ctx: &egui::Context) -> Pos2 {
        if let Some(pos) = self.drive_pos {
            return pos;
        }
        let main_rect = self.rects[WINDOW_MAIN]
            .or_else(|| ctx.input(|input| input.viewport().outer_rect))
            .unwrap_or_else(|| Rect::from_min_size(pos2(80.0, 80.0), vec2(MAIN_W, MAIN_H)));
        let anchor = if self.show_playlist {
            self.rects[WINDOW_PLAYLIST].unwrap_or(main_rect)
        } else {
            main_rect
        };
        let monitor = ctx
            .input(|input| input.viewport().monitor_size)
            .unwrap_or(vec2(1440.0, 900.0));
        let x = anchor.right();
        let y = anchor.top().min((monitor.y - DRIVE_H - 2.0).max(0.0));
        if x + DRIVE_W <= monitor.x {
            pos2(x, y)
        } else if main_rect.top() + MAIN_H + DRIVE_H <= monitor.y {
            pos2(main_rect.left(), main_rect.top() + MAIN_H)
        } else if anchor.top() + anchor.height() + DRIVE_H <= monitor.y {
            pos2(anchor.left(), anchor.top() + anchor.height())
        } else {
            pos2((main_rect.left() - DRIVE_W).max(0.0), y)
        }
    }

    fn playlist_position(&self, ctx: &egui::Context) -> Pos2 {
        if let Some(pos) = self.playlist_pos {
            return pos;
        }
        let main_rect = ctx
            .input(|input| input.viewport().outer_rect)
            .unwrap_or_else(|| Rect::from_min_size(pos2(80.0, 80.0), vec2(MAIN_W, MAIN_H)));
        let monitor = ctx
            .input(|input| input.viewport().monitor_size)
            .unwrap_or(vec2(1440.0, 900.0));
        let size = self.playlist_size;
        let right = main_rect.left() + MAIN_W;
        if right + size.x <= monitor.x {
            let y = main_rect.top().min((monitor.y - size.y - 2.0).max(0.0));
            pos2(right, y)
        } else if main_rect.top() + MAIN_H + size.y <= monitor.y {
            pos2(main_rect.left(), main_rect.top() + MAIN_H)
        } else {
            pos2(
                (main_rect.left() - size.x).max(0.0),
                main_rect.top().min((monitor.y - size.y - 2.0).max(0.0)),
            )
        }
    }
}

impl eframe::App for MacAmpApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_count = self.frame_count.saturating_add(1);
        if self.pending_zoom_resize {
            let target = if self.double_size { 2.0 } else { 1.0 };
            if (ctx.zoom_factor() - target).abs() > 0.01 {
                ctx.set_zoom_factor(target);
            } else {
                self.apply_zoom_layout(ctx);
                self.pending_zoom_resize = false;
            }
        }
        if self.pending_window_setup {
            self.pending_window_setup = false;
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if self.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            }));
        }
        if ctx.input(|input| input.viewport().close_requested()) {
            self.save_now();
        }
        if std::env::var_os("MACAMP_OPEN_MENU").is_some() && self.frame_count == 1 {
            egui::Popup::open_id(ctx, Id::unique(("title_button", "menu")).with("popup"));
        }
        self.handle_keys(ctx, false);
        self.poll_media(ctx);
        self.handle_drops(ctx);
        self.poll_loader();
        if let Some(query) = self.drive.initial_search.take() {
            self.drive.start_search(query);
        }
        self.poll_drive();
        self.sync_audio();
        self.sync_window_state(ctx);
        self.autosave();

        if self.screenshot.is_some() && !self.screenshot_requested {
            self.request_screenshot(ctx);
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|toast| Instant::now() < toast.until)
        {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        if self.playing {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
        if self.loading_batches > 0 {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.rects = [None; WINDOW_COUNT];
        if self.eq_root {
            self.draw_eq(ui);
            return;
        }
        if self.playlist_root {
            self.draw_playlist(ui);
            return;
        }
        if self.drive_root {
            self.draw_drive(ui);
            return;
        }
        self.draw_main(ui);

        if self.show_playlist {
            let position = self.playlist_position(ui.ctx());
            let size = self.playlist_size;
            let mut builder = ViewportBuilder::default()
                .with_title("MacAmp · Playlist")
                .with_decorations(false)
                .with_resizable(false)
                .with_inner_size([size.x, size.y])
                .with_min_inner_size([PLAYLIST_MIN_W, PLAYLIST_MIN_H])
                .with_max_inner_size([PLAYLIST_MAX_W, PLAYLIST_MAX_H]);
            if self.playlist_just_opened {
                builder = builder.with_position(position);
            }
            self.playlist_just_opened = false;
            ui.ctx()
                .show_viewport_immediate(playlist_viewport_id(), builder, |ui, _class| {
                    self.draw_playlist(ui);
                });
        }

        if self.show_eq {
            let position = self.eq_position(ui.ctx());
            let builder = if self.eq_just_opened {
                egui::ViewportBuilder::default()
                    .with_title("MacAmp · Equalizer")
                    .with_decorations(false)
                    .with_resizable(false)
                    .with_inner_size([EQ_W, EQ_H])
                    .with_min_inner_size([EQ_W, EQ_H])
                    .with_max_inner_size([EQ_W, EQ_H])
                    .with_position(position)
            } else {
                egui::ViewportBuilder::default()
                    .with_title("MacAmp · Equalizer")
                    .with_decorations(false)
                    .with_resizable(false)
                    .with_inner_size([EQ_W, EQ_H])
                    .with_min_inner_size([EQ_W, EQ_H])
                    .with_max_inner_size([EQ_W, EQ_H])
            };
            self.eq_just_opened = false;
            ui.ctx()
                .show_viewport_immediate(eq_viewport_id(), builder, |ui, _class| {
                    self.draw_eq(ui);
                });
        }

        if self.show_drive {
            let position = self.drive_position(ui.ctx());
            let mut builder = ViewportBuilder::default()
                .with_title("MacAmp · DriveMusic")
                .with_decorations(false)
                .with_resizable(false)
                .with_inner_size([DRIVE_W, DRIVE_H])
                .with_min_inner_size([DRIVE_W, DRIVE_H])
                .with_max_inner_size([DRIVE_W, DRIVE_H]);
            if self.drive_just_opened {
                builder = builder.with_position(position);
            }
            self.drive_just_opened = false;
            ui.ctx()
                .show_viewport_immediate(drive_viewport_id(), builder, |ui, _class| {
                    self.draw_drive(ui);
                });
        }

        self.process_window_geometry(ui.ctx());
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let [r, g, b, a] = self.palette.frame.to_normalized_gamma_f32();
        [r, g, b, a]
    }
}

impl Drop for MacAmpApp {
    fn drop(&mut self) {
        self.save_now();
    }
}

fn menu_section(ui: &mut Ui, p: &Palette, label: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 11.0), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 7.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        p.text_font(6.0),
        p.dim,
    );
}

fn menu_separator(ui: &mut Ui, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
    ui.painter().line_segment(
        [
            pos2(rect.left() + 4.0, rect.center().y),
            pos2(rect.right() - 4.0, rect.center().y),
        ],
        Stroke::new(1.0, p.edge_lo),
    );
}

fn menu_row(
    ui: &mut Ui,
    p: &Palette,
    label: &str,
    checked: Option<bool>,
    enabled: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::click());
    let hovered = enabled && response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        ui.painter().rect_filled(rect, 0.0, p.face);
    }
    if let Some(checked) = checked {
        let dot = Rect::from_center_size(pos2(rect.left() + 10.0, rect.center().y), vec2(5.0, 5.0));
        ui.painter()
            .rect_filled(dot, 0.0, if checked { p.led } else { p.edge_lo });
    }
    ui.painter().text(
        pos2(
            rect.left() + if checked.is_some() { 21.0 } else { 9.0 },
            rect.center().y,
        ),
        Align2::LEFT_CENTER,
        label,
        p.text_font(8.5),
        if enabled { p.text } else { p.dim },
    );
    response
}

fn remote_title(track: &RemoteTrack) -> String {
    if track.artist.trim().is_empty() {
        track.title.clone()
    } else {
        format!("{} — {}", track.artist, track.title)
    }
}

fn format_time(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn format_long_duration(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Case-insensitive match over title, artist, album and path. `needle` is
/// expected to be lowercase already.
fn track_matches(track: &Track, needle: &str) -> bool {
    let mut haystack = String::with_capacity(96);
    haystack.push_str(&track.display_title());
    haystack.push(' ');
    haystack.push_str(&track.album);
    haystack.push(' ');
    haystack.push_str(&track.path.to_string_lossy());
    haystack.to_lowercase().contains(needle)
}

/// Remove a track from the play order, remapping the remaining entries and
/// keeping `play_pos` on the same track. Returns false when the track was not
/// part of the order (caller should rebuild it).
fn remove_track_from_order(order: &mut Vec<usize>, play_pos: &mut usize, removed: usize) -> bool {
    let Some(position) = order.iter().position(|index| *index == removed) else {
        return false;
    };
    order.retain(|index| *index != removed);
    for index in order.iter_mut() {
        if *index > removed {
            *index -= 1;
        }
    }
    if position < *play_pos {
        *play_pos -= 1;
    }
    *play_pos = (*play_pos).min(order.len().saturating_sub(1));
    true
}

/// New index of an old playlist entry after moving `from` to `to`.
fn remap_after_move(index: usize, from: usize, to: usize) -> usize {
    if index == from {
        to
    } else if from < to && index > from && index <= to {
        index - 1
    } else if to < from && index >= to && index < from {
        index + 1
    } else {
        index
    }
}

fn rects_overlap_x(a: Rect, b: Rect) -> bool {
    a.left().max(b.left()) < a.right().min(b.right()) - 0.5
}

fn rects_overlap_y(a: Rect, b: Rect) -> bool {
    a.top().max(b.top()) < a.bottom().min(b.bottom()) - 0.5
}

/// True when two window rectangles touch along an edge and share some of the
/// perpendicular span, i.e. they form a docked pair.
fn windows_attached(a: Rect, b: Rect) -> bool {
    const TOUCH: f32 = 1.5;
    let side_by_side = ((a.right() - b.left()).abs() <= TOUCH
        || (a.left() - b.right()).abs() <= TOUCH)
        && rects_overlap_y(a, b);
    let stacked = ((a.bottom() - b.top()).abs() <= TOUCH || (a.top() - b.bottom()).abs() <= TOUCH)
        && rects_overlap_x(a, b);
    side_by_side || stacked
}

/// Offset that docks `moving` flush against `fixed` when within `threshold`.
/// Only edge-to-edge docking is considered so windows never snap into overlap.
fn snap_offset(moving: Rect, fixed: Rect, threshold: f32) -> Vec2 {
    let mut dx = 0.0;
    let mut best_x = threshold + 1.0;
    if rects_overlap_y(moving, fixed) {
        for delta in [fixed.left() - moving.right(), fixed.right() - moving.left()] {
            let distance = delta.abs();
            if distance <= threshold && distance < best_x {
                best_x = distance;
                dx = delta;
            }
        }
    }
    let mut dy = 0.0;
    let mut best_y = threshold + 1.0;
    if rects_overlap_x(moving, fixed) {
        for delta in [fixed.top() - moving.bottom(), fixed.bottom() - moving.top()] {
            let distance = delta.abs();
            if distance <= threshold && distance < best_y {
                best_y = distance;
                dy = delta;
            }
        }
    }
    vec2(dx, dy)
}

fn save_color_image(path: &std::path::Path, image: &egui::ColorImage) {
    let width = image.size[0] as u32;
    let height = image.size[1] as u32;
    let mut bytes = Vec::with_capacity(image.pixels.len() * 4);
    for pixel in &image.pixels {
        bytes.extend_from_slice(&pixel.to_array());
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) =
        image::save_buffer(path, &bytes, width, height, image::ExtendedColorType::Rgba8)
    {
        tracing::error!("screenshot failed: {error}");
    } else {
        tracing::info!("screenshot written to {}", path.display());
    }
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Rect;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    fn track(title: &str, artist: &str, album: &str, path: &str) -> Track {
        Track {
            path: PathBuf::from(path),
            title: title.to_owned(),
            artist: artist.to_owned(),
            album: album.to_owned(),
            year: None,
            duration_ms: None,
            sample_rate: None,
            channels: None,
            bitrate_kbps: None,
            codec: "FLAC".to_owned(),
        }
    }

    #[test]
    fn filter_matches_title_artist_album_and_path() {
        let track = track(
            "Teardrop",
            "Massive Attack",
            "Mezzanine",
            "/Music/Massive Attack/01 Teardrop.flac",
        );
        assert!(track_matches(&track, "teardrop"));
        assert!(track_matches(&track, "massive attack"));
        assert!(track_matches(&track, "mezzanine"));
        assert!(track_matches(&track, "01 teardrop.flac"));
        assert!(!track_matches(&track, "portishead"));
    }

    #[test]
    fn removing_earlier_order_entry_keeps_current_track() {
        let mut order = vec![2, 0, 1];
        let mut play_pos = 1; // current track is 0
        assert!(remove_track_from_order(&mut order, &mut play_pos, 2));
        assert_eq!(order, vec![0, 1]);
        assert_eq!(play_pos, 0);
    }

    #[test]
    fn removing_later_order_entry_keeps_current_track() {
        let mut order = vec![2, 0, 1];
        let mut play_pos = 0; // current track is 2
        assert!(remove_track_from_order(&mut order, &mut play_pos, 0));
        assert_eq!(order, vec![1, 0]);
        assert_eq!(play_pos, 0);
    }

    #[test]
    fn removing_unknown_track_falls_back_to_rebuild() {
        let mut order = vec![0, 1];
        let mut play_pos = 0;
        assert!(!remove_track_from_order(&mut order, &mut play_pos, 5));
        assert_eq!(order, vec![0, 1]);
        assert_eq!(play_pos, 0);
    }

    #[test]
    fn remap_tracks_after_reorder() {
        // 2 moved down to 5.
        assert_eq!(remap_after_move(2, 2, 5), 5);
        assert_eq!(remap_after_move(3, 2, 5), 2);
        assert_eq!(remap_after_move(4, 2, 5), 3);
        assert_eq!(remap_after_move(5, 2, 5), 4);
        assert_eq!(remap_after_move(6, 2, 5), 6);
        // 5 moved up to 2.
        assert_eq!(remap_after_move(5, 5, 2), 2);
        assert_eq!(remap_after_move(2, 5, 2), 3);
        assert_eq!(remap_after_move(4, 5, 2), 5);
        assert_eq!(remap_after_move(1, 5, 2), 1);
    }

    #[test]
    fn adjacent_windows_are_attached() {
        let main = rect(0.0, 0.0, 550.0, 232.0);
        let playlist = rect(550.0, 0.0, 550.0, 464.0);
        assert!(windows_attached(main, playlist));
        assert!(windows_attached(playlist, main));
    }

    #[test]
    fn detached_windows_are_not_attached() {
        let main = rect(0.0, 0.0, 550.0, 232.0);
        assert!(!windows_attached(main, rect(570.0, 0.0, 550.0, 464.0)));
        // Touching an edge but with no shared span is not a dock.
        assert!(!windows_attached(main, rect(550.0, 300.0, 550.0, 200.0)));
    }

    #[test]
    fn snapping_docks_side_by_side() {
        let main = rect(0.0, 0.0, 550.0, 232.0);
        let playlist = rect(556.0, 40.0, 550.0, 464.0);
        assert_eq!(snap_offset(playlist, main, SNAP_DIST), vec2(-6.0, 0.0));
        assert_eq!(snap_offset(main, playlist, SNAP_DIST), vec2(6.0, 0.0));
    }

    #[test]
    fn snapping_stacks_vertically() {
        let main = rect(0.0, 0.0, 550.0, 232.0);
        let playlist = rect(30.0, 239.0, 550.0, 464.0);
        assert_eq!(snap_offset(playlist, main, SNAP_DIST), vec2(0.0, -7.0));
    }

    #[test]
    fn snapping_ignores_far_or_unrelated_windows() {
        let main = rect(0.0, 0.0, 550.0, 232.0);
        assert_eq!(
            snap_offset(rect(600.0, 0.0, 400.0, 400.0), main, SNAP_DIST),
            Vec2::ZERO
        );
        assert_eq!(
            snap_offset(rect(556.0, 400.0, 550.0, 200.0), main, SNAP_DIST),
            Vec2::ZERO
        );
    }
}
