//! On-demand DriveMusic browser.
//!
//! The app only talks to the site when the user explicitly searches, opens an
//! artist or asks to play a song. Pages are parsed with `scraper`; audio is
//! downloaded into the app cache and then handed to the regular local-track
//! pipeline, so playback, seek, EQ and gapless keep working unchanged.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use egui::Context;
use scraper::{ElementRef, Html, Selector};

use crate::metadata::Track;

pub const BASE: &str = "https://drivemusic.club";
const USER_AGENT: &str = "MacAmp/0.2 (personal music player)";

#[derive(Clone, Debug)]
pub struct ArtistHit {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct RemoteTrack {
    pub title: String,
    pub artist: String,
    pub duration_ms: Option<u64>,
    pub page_url: String,
    pub online_url: Option<String>,
    pub cache_key: String,
}

#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    pub artists: Vec<ArtistHit>,
    pub tracks: Vec<RemoteTrack>,
}

#[derive(Clone, Debug)]
pub struct ArtistPage {
    pub name: String,
    pub tracks: Vec<RemoteTrack>,
    pub next_url: Option<String>,
}

#[derive(Debug)]
pub enum DmEvent {
    Search {
        query: String,
        result: Result<SearchResults, String>,
    },
    Artist {
        append: bool,
        result: Result<ArtistPage, String>,
    },
    Progress {
        key: String,
        fraction: Option<f32>,
    },
    Downloaded {
        track: RemoteTrack,
        path: PathBuf,
        result: Result<(), String>,
    },
}

enum HtmlRequest {
    Search(String),
    Artist { url: String, append: bool },
}

struct DownloadRequest {
    track: RemoteTrack,
    dest: PathBuf,
}

pub struct DmService {
    html_tx: Sender<HtmlRequest>,
    download_tx: Sender<DownloadRequest>,
    events: Receiver<DmEvent>,
}

impl DmService {
    pub fn new(ctx: Context) -> Self {
        prepare_cache();
        let (html_tx, html_rx) = mpsc::channel();
        let (download_tx, download_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        {
            let events = event_tx.clone();
            let ctx = ctx.clone();
            thread::spawn(move || html_worker(html_rx, events, ctx));
        }
        {
            let events = event_tx;
            let ctx = ctx.clone();
            thread::spawn(move || download_worker(download_rx, events, ctx));
        }
        Self {
            html_tx,
            download_tx,
            events,
        }
    }

    pub fn search(&self, query: &str) {
        let _ = self.html_tx.send(HtmlRequest::Search(query.to_owned()));
    }

    pub fn artist(&self, url: &str, append: bool) {
        let _ = self.html_tx.send(HtmlRequest::Artist {
            url: url.to_owned(),
            append,
        });
    }

    pub fn download(&self, track: RemoteTrack, dest: PathBuf) {
        let _ = self.download_tx.send(DownloadRequest { track, dest });
    }

    pub fn poll(&self) -> Vec<DmEvent> {
        self.events.try_iter().collect()
    }
}

fn html_worker(requests: Receiver<HtmlRequest>, events: Sender<DmEvent>, ctx: Context) {
    let agent = ureq::AgentBuilder::new()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(30))
        .build();
    while let Ok(request) = requests.recv() {
        let event = match request {
            HtmlRequest::Search(query) => DmEvent::Search {
                result: run_search(&agent, &query),
                query,
            },
            HtmlRequest::Artist { url, append } => DmEvent::Artist {
                append,
                result: run_artist(&agent, &url),
            },
        };
        if events.send(event).is_err() {
            break;
        }
        ctx.request_repaint();
    }
}

fn download_worker(requests: Receiver<DownloadRequest>, events: Sender<DmEvent>, ctx: Context) {
    let agent = ureq::AgentBuilder::new()
        .user_agent(USER_AGENT)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .timeout_write(Duration::from_secs(30))
        .build();
    while let Ok(request) = requests.recv() {
        let remote = request.track;
        let result = download_one(&agent, &remote, &request.dest, &events, &ctx);
        if events
            .send(DmEvent::Downloaded {
                track: remote,
                path: request.dest,
                result,
            })
            .is_err()
        {
            break;
        }
        ctx.request_repaint();
    }
}

fn run_search(agent: &ureq::Agent, query: &str) -> Result<SearchResults, String> {
    let response = agent
        .get(BASE)
        .query("do", "search")
        .query("subaction", "search")
        .query("story", query)
        .call()
        .map_err(format_http_error)?;
    let html = response.into_string().map_err(|error| error.to_string())?;
    Ok(parse_search(&html, BASE))
}

fn run_artist(agent: &ureq::Agent, url: &str) -> Result<ArtistPage, String> {
    if !is_drive_url(url) {
        return Err("ссылка ведёт не на DriveMusic".to_owned());
    }
    let response = agent.get(url).call().map_err(format_http_error)?;
    let html = response.into_string().map_err(|error| error.to_string())?;
    Ok(parse_artist(&html, BASE))
}

fn download_one(
    agent: &ureq::Agent,
    remote: &RemoteTrack,
    dest: &PathBuf,
    events: &Sender<DmEvent>,
    ctx: &Context,
) -> Result<(), String> {
    let url = resolve_download_url(agent, remote)
        .ok_or_else(|| "не удалось получить ссылку на MP3".to_owned())?;
    if !is_drive_url(&url) {
        return Err("ссылка на MP3 ведёт не на DriveMusic".to_owned());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let part = dest.with_extension("part");
    let result = (|| -> Result<(), String> {
        let response = agent.get(&url).call().map_err(format_http_error)?;
        let total = response
            .header("Content-Length")
            .and_then(|value| value.parse::<u64>().ok());
        let mut reader = response.into_reader();
        let mut file = BufWriter::new(File::create(&part).map_err(|error| error.to_string())?);
        let mut buffer = [0u8; 64 * 1024];
        let mut downloaded = 0u64;
        let mut last_sent = 0u64;
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])
                .map_err(|error| error.to_string())?;
            downloaded += read as u64;
            if downloaded - last_sent >= 256 * 1024 {
                last_sent = downloaded;
                let fraction =
                    total.map(|total| (downloaded as f32 / total.max(1) as f32).min(1.0));
                let _ = events.send(DmEvent::Progress {
                    key: remote.cache_key.clone(),
                    fraction,
                });
                ctx.request_repaint();
            }
        }
        file.flush().map_err(|error| error.to_string())?;
        drop(file);
        if downloaded == 0 {
            return Err("CDN вернул пустой файл".to_owned());
        }
        fs::rename(&part, dest).map_err(|error| error.to_string())?;
        let _ = events.send(DmEvent::Progress {
            key: remote.cache_key.clone(),
            fraction: Some(1.0),
        });
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

fn resolve_download_url(agent: &ureq::Agent, remote: &RemoteTrack) -> Option<String> {
    let from_page = if is_drive_url(&remote.page_url) {
        agent
            .get(&remote.page_url)
            .call()
            .ok()
            .and_then(|response| response.into_string().ok())
            .and_then(|html| parse_download_url(&html, BASE))
    } else {
        None
    };
    from_page.or_else(|| remote.online_url.clone())
}

fn format_http_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(code, _) => format!("HTTP {code}"),
        other => other.to_string(),
    }
}

fn is_drive_url(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    host == "drivemusic.club" || host.ends_with(".drivemusic.club")
}

pub fn parse_search(html: &str, base: &str) -> SearchResults {
    let document = Html::parse_document(html);
    let selectors = selectors();
    let mut seen = HashSet::new();
    let artists = document
        .select(&selectors.search_artist)
        .filter_map(|anchor| {
            let href = anchor.value().attr("href")?;
            let url = absolute_url(base, href);
            if !seen.insert(url.clone()) {
                return None;
            }
            let name = anchor
                .select(&selectors.carousel_text)
                .next()
                .map(clean_text)
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| clean_text(anchor));
            (!name.is_empty()).then_some(ArtistHit { name, url })
        })
        .collect();
    let tracks = document
        .select(&selectors.search_song_row)
        .filter_map(|row| parse_song_row(row, base))
        .collect();
    SearchResults { artists, tracks }
}

pub fn parse_artist(html: &str, base: &str) -> ArtistPage {
    let document = Html::parse_document(html);
    let selectors = selectors();
    let name = document
        .select(&selectors.heading)
        .next()
        .map(clean_text)
        .unwrap_or_default();
    let tracks = document
        .select(&selectors.artist_song_row)
        .filter_map(|row| parse_song_row(row, base))
        .collect();
    let next_url = document
        .select(&selectors.next_page)
        .next()
        .and_then(|anchor| anchor.value().attr("href"))
        .map(|href| absolute_url(base, href));
    ArtistPage {
        name,
        tracks,
        next_url,
    }
}

pub fn parse_download_url(html: &str, base: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selectors = selectors();
    document
        .select(&selectors.download)
        .filter_map(|anchor| anchor.value().attr("href"))
        .map(|href| absolute_url(base, href))
        .find(|url| url.contains("download_music") || url.ends_with(".mp3"))
}

fn parse_song_row(row: ElementRef<'_>, base: &str) -> Option<RemoteTrack> {
    let selectors = selectors();
    let button = row.select(&selectors.play_button).next();
    let online_url = button.and_then(|button| button.value().attr("data-url").map(str::to_owned));
    let title_link = row.select(&selectors.title_link).next()?;
    let title = clean_text(title_link);
    if title.is_empty() {
        return None;
    }
    let page_url = title_link
        .value()
        .attr("href")
        .map(|href| absolute_url(base, href))
        .unwrap_or_default();
    let mut artists = Vec::new();
    for anchor in row.select(&selectors.artist_links) {
        let name = clean_text(anchor);
        if !name.is_empty() && !artists.contains(&name) {
            artists.push(name);
        }
    }
    let artist = artists.join(", ");
    let duration_ms = row
        .select(&selectors.duration)
        .next()
        .and_then(|element| parse_duration(&clean_text(element)));
    let cache_key = if !page_url.is_empty() {
        cache_key_from_url(&page_url)
    } else {
        fallback_cache_key(&artist, &title, online_url.as_deref())
    };
    Some(RemoteTrack {
        title,
        artist,
        duration_ms,
        page_url,
        online_url,
        cache_key,
    })
}

fn parse_duration(text: &str) -> Option<u64> {
    let parts: Vec<u64> = text
        .trim()
        .split(':')
        .map(|part| part.trim().parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    match parts.as_slice() {
        [minutes, seconds] => Some((minutes * 60 + seconds) * 1000),
        [hours, minutes, seconds] => Some((hours * 3600 + minutes * 60 + seconds) * 1000),
        _ => None,
    }
}

fn cache_key_from_url(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.rsplit('/').next().unwrap_or(path);
    let stem = last.strip_suffix(".html").unwrap_or(last);
    let key = sanitize(stem);
    if key.is_empty() {
        format!("track-{:08x}", fnv1a(url.as_bytes()))
    } else {
        truncate_key(key)
    }
}

fn fallback_cache_key(artist: &str, title: &str, source: Option<&str>) -> String {
    let base = sanitize(&format!("{artist}-{title}"));
    let prefix = if base.is_empty() {
        "track".to_owned()
    } else {
        truncate_key(base)
    };
    let source = source.unwrap_or("track");
    format!("{prefix}-{:08x}", fnv1a(source.as_bytes()))
}

fn truncate_key(mut key: String) -> String {
    if key.len() > 120 {
        key.truncate(120);
        while key.ends_with('-') {
            key.pop();
        }
    }
    key
}

fn sanitize(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
            result.push(character.to_ascii_lowercase());
        } else if !result.ends_with('-') {
            result.push('-');
        }
    }
    result.trim_matches('-').to_owned()
}

fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in bytes {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn absolute_url(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        href.to_owned()
    } else if let Some(rest) = href.strip_prefix('/') {
        format!("{base}/{rest}")
    } else {
        format!("{base}/{href}")
    }
}

fn clean_text(element: ElementRef<'_>) -> String {
    element
        .text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn cache_dir() -> Option<PathBuf> {
    crate::config::data_dir().map(|dir| dir.join("cache").join("drivemusic"))
}

pub fn cache_path(track: &RemoteTrack) -> Option<PathBuf> {
    cache_dir().map(|dir| dir.join(format!("{}.mp3", track.cache_key)))
}

pub fn is_cached(track: &RemoteTrack) -> bool {
    cache_path(track)
        .and_then(|path| fs::metadata(path).ok())
        .is_some_and(|metadata| metadata.len() >= 1024)
}

pub fn cached_track(track: &RemoteTrack) -> Option<Track> {
    let path = cache_path(track)?;
    let metadata = fs::metadata(&path).ok()?;
    if metadata.len() < 1024 {
        return None;
    }
    Some(track_from_file(track, path))
}

pub fn track_from_file(remote: &RemoteTrack, path: PathBuf) -> Track {
    match Track::load(path.clone()) {
        Ok(mut track) => {
            // The site writes "[drivemusic.me]" into the artist tag; the
            // page metadata is cleaner and matches what the browser shows.
            if !remote.title.trim().is_empty() {
                track.title = remote.title.clone();
            }
            if !remote.artist.trim().is_empty() {
                track.artist = remote.artist.clone();
            }
            if track.album.to_ascii_lowercase().contains("drivemusic") {
                track.album.clear();
            }
            if track.duration_ms.is_none() {
                track.duration_ms = remote.duration_ms;
            }
            track
        }
        Err(_) => Track {
            path,
            title: remote.title.clone(),
            artist: remote.artist.clone(),
            album: String::new(),
            year: None,
            duration_ms: remote.duration_ms,
            sample_rate: None,
            channels: None,
            bitrate_kbps: Some(320),
            codec: "MP3".to_owned(),
        },
    }
}

fn prepare_cache() {
    let Some(dir) = cache_dir() else {
        return;
    };
    let _ = fs::create_dir_all(&dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().ends_with(".part") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

struct Selectors {
    search_artist: Selector,
    carousel_text: Selector,
    search_song_row: Selector,
    artist_song_row: Selector,
    play_button: Selector,
    title_link: Selector,
    artist_links: Selector,
    duration: Selector,
    heading: Selector,
    next_page: Selector,
    download: Selector,
}

fn selectors() -> &'static Selectors {
    static SELECTORS: OnceLock<Selectors> = OnceLock::new();
    SELECTORS.get_or_init(|| Selectors {
        search_artist: Selector::parse(".c-search-results__artists a.carousel-slide").unwrap(),
        carousel_text: Selector::parse(".carousel-slide__text").unwrap(),
        search_song_row: Selector::parse(".c-search-results .music-popular-wrapper").unwrap(),
        artist_song_row: Selector::parse(
            "div.inline_player_playlist_main div.music-popular-wrapper",
        )
        .unwrap(),
        play_button: Selector::parse("button.popular-play__item").unwrap(),
        title_link: Selector::parse("a.popular-play-author").unwrap(),
        artist_links: Selector::parse("div.popular-play-composition a").unwrap(),
        duration: Selector::parse("div.popular-download-number").unwrap(),
        heading: Selector::parse("h1").unwrap(),
        next_page: Selector::parse(r#"a[aria-label="Далее"]"#).unwrap(),
        download: Selector::parse("a.btn-download").unwrap(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH_HTML: &str = r#"
        <div class="c-search-results">
          <div class="c-search-results__artists">
            <a href="/artist/zivert.html" class="carousel-slide">
              <span class="carousel-slide__text">Zivert</span>
            </a>
            <a href="/artist/zivert.html" class="carousel-slide">
              <span class="carousel-slide__text">Zivert (dup)</span>
            </a>
            <a href="/artist/basta.html" class="carousel-slide">
              <span class="carousel-slide__text">Баста</span>
            </a>
          </div>
          <div class="music-popular-wrapper">
            <button data-url="https://cdn1.drivemusic.club/dl/online/token/1/download_music/2026/11/hit.mp3"
                    class="popular-play__item">play</button>
            <a href="/pop_music/123-hit.html" class="popular-play-author">Хит</a>
            <div class="popular-play-composition"><a href="/artist/zivert.html">Zivert</a></div>
            <div class="popular-download-number">03:08</div>
          </div>
        </div>
    "#;

    const ARTIST_HTML: &str = r#"
        <h1>Zivert</h1>
        <div class="inline_player_playlist_main">
          <div class="music-popular-wrapper">
            <button data-url="https://cdn1.drivemusic.club/dl/online/token/2/download_music/2018/11/life.mp3"
                    class="popular-play__item">play</button>
            <a href="/new_music/147920-zivert-life.html" class="popular-play-author">Life</a>
            <div class="popular-play-composition"><a href="/artist/zivert.html">Zivert</a></div>
            <div class="popular-download-number">03:08</div>
          </div>
        </div>
        <a aria-label="Далее" href="/artist/zivert/page-2.html">Далее</a>
    "#;

    #[test]
    fn parses_search_artists_and_tracks() {
        let results = parse_search(SEARCH_HTML, BASE);
        assert_eq!(results.artists.len(), 2);
        assert_eq!(results.artists[0].name, "Zivert");
        assert_eq!(
            results.artists[0].url,
            "https://drivemusic.club/artist/zivert.html"
        );
        assert_eq!(results.artists[1].name, "Баста");
        assert_eq!(results.tracks.len(), 1);
        assert_eq!(results.tracks[0].title, "Хит");
        assert_eq!(results.tracks[0].artist, "Zivert");
        assert_eq!(results.tracks[0].duration_ms, Some(188_000));
        assert_eq!(results.tracks[0].cache_key, "123-hit");
    }

    #[test]
    fn parses_artist_page_and_pagination() {
        let page = parse_artist(ARTIST_HTML, BASE);
        assert_eq!(page.name, "Zivert");
        assert_eq!(page.tracks.len(), 1);
        assert_eq!(page.tracks[0].title, "Life");
        assert_eq!(
            page.next_url.as_deref(),
            Some("https://drivemusic.club/artist/zivert/page-2.html")
        );
    }

    #[test]
    fn parses_download_link() {
        let html = r#"<a class="song-author-btn btn-download"
            href="https://d11.drivemusic.club/dl/x/1/download_music/2026/11/sipe-stereo.mp3">Скачать</a>"#;
        assert_eq!(
            parse_download_url(html, BASE).as_deref(),
            Some("https://d11.drivemusic.club/dl/x/1/download_music/2026/11/sipe-stereo.mp3")
        );
    }

    #[test]
    fn duration_and_cache_keys() {
        assert_eq!(parse_duration("03:08"), Some(188_000));
        assert_eq!(parse_duration("1:02:03"), Some(3_723_000));
        assert_eq!(parse_duration("n/a"), None);
        assert_eq!(
            cache_key_from_url("https://drivemusic.club/pop_music/2952831-sipe-stereo.html"),
            "2952831-sipe-stereo"
        );
        assert!(is_drive_url(
            "https://cdn4.drivemusic.club/dl/online/x/y.mp3"
        ));
        assert!(is_drive_url("https://drivemusic.club/artist/zivert.html"));
        assert!(!is_drive_url("https://example.com/x.mp3"));
        assert!(!is_drive_url("https://evil.com/.drivemusic.club/x.mp3"));
    }
}
