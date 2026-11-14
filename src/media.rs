use crate::island::Notification;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use windows::core::*;
use windows::Media::Control::*;
use windows::Storage::Streams::DataReader;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::System::Threading::*;

#[derive(Debug, Clone)]
pub enum MediaEvent {
    TrackChanged {
        notif: Notification,
        thumbnail_bytes: Option<Vec<u8>>,
        position_secs: f32,
        duration_secs: f32,
    },
    ThumbnailLoaded(Vec<u8>),
    PlaybackState(bool), // true = playing, false = paused/stopped
    TimelineUpdate {
        position_secs: f32,
        duration_secs: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCommand {
    TogglePlayPause,
    Previous,
    Next,
}

static LAST_SEEK_TIME: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Seeks media playback to the specified position in seconds.
/// Automatically routes to YTMPX (Tauri YouTube Music) and/or Windows GSMTC (Spotify Desktop, Chrome, Edge).
pub fn seek_playback(seconds: f32) {
    if let Ok(mut last) = LAST_SEEK_TIME.lock() {
        let now = std::time::Instant::now();
        if let Some(prev) = *last {
            if now.duration_since(prev) < std::time::Duration::from_millis(150) {
                return;
            }
        }
        *last = Some(now);
    }

    if is_ytmpx_connected() {
        send_ytmpx_command(&format!(r#"{{"command":"seek","value":{:.2}}}"#, seconds));
        send_ytmpx_command(&format!(r#"{{"command":"seekTo","value":{:.2}}}"#, seconds));
    }

    thread::spawn(move || {
        let manager = match GlobalSystemMediaTransportControlsSessionManager::RequestAsync() {
            Ok(op) => match wait_async(op) {
                Ok(m) => m,
                Err(_) => return,
            },
            Err(_) => return,
        };

        if let Some(session) = get_best_media_session(&manager) {
            let pos_100ns = (seconds as f64 * 10_000_000.0).round() as i64;
            if let Ok(seek_op) = session.TryChangePlaybackPositionAsync(pos_100ns) {
                let _ = wait_async(seek_op);
            }
        }
    });
}

fn wait_async<T: RuntimeType + 'static>(
    op: windows_future::IAsyncOperation<T>,
) -> Result<T> {
    let info: windows_future::IAsyncInfo = op.cast()?;
    while info.Status()? == windows_future::AsyncStatus::Started {
        thread::sleep(Duration::from_millis(20));
    }
    op.GetResults()
}

/// Helper to select the most relevant media session.
/// Prioritizes dedicated music apps (Tauri YTM, Spotify, YouTube Music) with album titles over generic video playback.
fn get_best_media_session(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
) -> Option<GlobalSystemMediaTransportControlsSession> {
    if let Ok(sessions) = manager.GetSessions() {
        let mut best: Option<(i32, GlobalSystemMediaTransportControlsSession)> = None;

        for s in sessions {
            let is_playing = s
                .GetPlaybackInfo()
                .ok()
                .and_then(|info| info.PlaybackStatus().ok())
                .map(|status| status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
                .unwrap_or(false);

            let mut score: i32 = 20;

            let source = s
                .SourceAppUserModelId()
                .map(|h| h.to_string().to_lowercase())
                .unwrap_or_default();

            // Highest priority to Tauri YTM, Spotify, Apple Music
            if source.contains("tauri") || source.contains("ytm") {
                score += 250;
            } else if source.contains("spotify") {
                score += 200;
            } else if source.contains("applemusic") || source.contains("deezer") {
                score += 150;
            } else if source.contains("youtube") && source.contains("music") {
                score += 120;
            }

            // Actively playing session ALWAYS takes strict priority over any paused session!
            if is_playing {
                score += 500;
            }

            if let Ok(prop_op) = s.TryGetMediaPropertiesAsync() {
                if let Ok(props) = wait_async(prop_op) {
                    let title = props.Title().map(|h| h.to_string().to_lowercase()).unwrap_or_default();
                    let album = props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
                    let artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
                    if !album.is_empty() {
                        score += 80; // Music tracks have album titles
                    }
                    if !artist.is_empty() {
                        score += 30;
                    }
                    // Mild penalty for generic video playback only if dedicated music tracks exist
                    if title.contains(" - youtube") || title.contains("youtube.com") {
                        score = score.saturating_sub(15).max(5);
                    }
                }
            }

            if best.as_ref().map(|(prev_score, _)| score > *prev_score).unwrap_or(true) {
                best = Some((score, s));
            }
        }

        if let Some((_, s)) = best {
            return Some(s);
        }
    }

    manager.GetCurrentSession().ok()
}

/// Fetches binary bytes of the album art thumbnail stream from Windows GSMTC.
fn fetch_thumbnail_bytes(
    props: &GlobalSystemMediaTransportControlsSessionMediaProperties,
) -> Option<Vec<u8>> {
    let stream_ref = props.Thumbnail().ok()?;
    let op = stream_ref.OpenReadAsync().ok()?;
    let stream = wait_async(op).ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > 15_000_000 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let load_op = reader.LoadAsync(size as u32).ok()?;
    wait_async(load_op).ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
    Some(buf)
}

// =========================================================================
// YTMPX WebSocket Integration (Tauri YouTube Music Desktop App Bridge)
// =========================================================================

#[derive(serde::Deserialize)]
struct RawYtmpxPayload {
    command: Option<String>,
    event: Option<String>,
    #[allow(dead_code)]
    source: Option<String>,
    is_playing: Option<bool>,
    image: Option<String>,
    thumbnail: Option<String>,
    cover: Option<String>,
    art: Option<String>,
    metadata: Option<RawYtmpxMeta>,
}

#[derive(serde::Deserialize)]
struct RawYtmpxMeta {
    title: Option<String>,
    artist: Option<String>,
    author: Option<String>,
    album: Option<String>,
    image: Option<String>,
    thumbnail: Option<String>,
    cover: Option<String>,
    art: Option<String>,
    status: Option<String>,
    #[serde(rename = "totalDuration")]
    total_duration_ms: Option<f64>,
    duration: Option<f64>,
    elapsed: Option<f64>,
}

static YTMPX_CMD_TX: std::sync::Mutex<Option<std::sync::mpsc::Sender<String>>> = std::sync::Mutex::new(None);
static YTMPX_CONNECTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn is_ytmpx_connected() -> bool {
    YTMPX_CONNECTED.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn send_ytmpx_command(cmd: &str) {
    if let Ok(guard) = YTMPX_CMD_TX.lock() {
        if let Some(ref tx) = *guard {
            let _ = tx.send(cmd.to_string());
        }
    }
}

static COVER_CACHE: std::sync::Mutex<Option<std::collections::HashMap<String, Vec<u8>>>> = std::sync::Mutex::new(None);

pub fn get_cached_cover(url: &str) -> Option<Vec<u8>> {
    if let Ok(guard) = COVER_CACHE.lock() {
        if let Some(ref map) = *guard {
            return map.get(url).cloned();
        }
    }
    None
}

pub fn save_cached_cover(url: String, bytes: Vec<u8>) {
    if let Ok(mut guard) = COVER_CACHE.lock() {
        let map = guard.get_or_insert_with(std::collections::HashMap::new);
        if map.len() > 50 {
            map.clear();
        }
        map.insert(url, bytes);
    }
}

fn fetch_curl_bytes(url: &str) -> Option<Vec<u8>> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let output = std::process::Command::new("curl.exe")
        .args(["-s", "-L", "--max-time", "3", url])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if output.status.success() && !output.stdout.is_empty() {
        Some(output.stdout)
    } else {
        None
    }
}

fn fetch_cover_bytes(url: &str) -> Option<Vec<u8>> {
    let clean = url.trim();
    if clean.is_empty() {
        return None;
    }
    if let Some(cached) = get_cached_cover(clean) {
        return Some(cached);
    }

    let bytes = if clean.starts_with("http://") || clean.starts_with("https://") {
        let mut target_url = clean.to_string();
        if target_url.contains("googleusercontent.com") {
            if target_url.contains("-rw") {
                target_url = target_url.replace("-rw", "-rj");
            } else if !target_url.contains("-rj") {
                if target_url.contains('=') {
                    target_url.push_str("-rj");
                } else {
                    target_url.push_str("=w512-h512-l90-rj");
                }
            }
        }
        fetch_curl_bytes(&target_url).or_else(|| fetch_curl_bytes(clean))
    } else if let Some(path) = clean.strip_prefix("file:///") {
        std::fs::read(path).ok()
    } else if let Some(path) = clean.strip_prefix("file://") {
        std::fs::read(path).ok()
    } else if std::path::Path::new(clean).exists() {
        std::fs::read(clean).ok()
    } else {
        None
    };

    if let Some(ref b) = bytes {
        save_cached_cover(clean.to_string(), b.clone());
    }
    bytes
}

fn handle_ytmpx_json(text: &str, sender: &Sender<MediaEvent>) -> Option<String> {
    let payload = serde_json::from_str::<RawYtmpxPayload>(text).ok()?;
    if payload.command.is_some() {
        return None;
    }

    let event = payload.event.as_deref().unwrap_or("");
    if event == "stop" || event == "pause" {
        let _ = sender.send(MediaEvent::PlaybackState(false));
        return None;
    } else if event == "resume" {
        let _ = sender.send(MediaEvent::PlaybackState(true));
        return None;
    }

    if let Some(meta) = payload.metadata {
        let title = meta.title.unwrap_or_default().trim().to_string();
        if !title.is_empty() {
            let artist = meta.author.or(meta.artist).unwrap_or_default().trim().to_string();
            let album = meta.album.unwrap_or_default();
            let art_url = meta.image.or(meta.thumbnail).or(meta.cover).or(meta.art)
                .or(payload.image).or(payload.thumbnail).or(payload.cover).or(payload.art)
                .unwrap_or_default();

            let is_playing = payload.is_playing.unwrap_or_else(|| {
                meta.status.as_deref() != Some("paused")
            });

            let notif = Notification {
                app_name: "YouTube Music".to_string(),
                title: title.clone(),
                body: if !artist.is_empty() { artist } else if !album.is_empty() { album } else { "YouTube Music".to_string() },
                time_str: "♪".to_string(),
                badge_letter: "▶".to_string(),
                badge_color: (0.95, 0.10, 0.10),
                icon_bytes: None,
                aumid: None,
            };

            let cached_thumb = if !art_url.is_empty() {
                get_cached_cover(&art_url)
            } else {
                None
            };

            let duration_secs = meta.total_duration_ms
                .map(|ms| ms as f32 / 1000.0)
                .or_else(|| meta.duration.map(|d| d as f32))
                .unwrap_or(0.0);
            let position_secs = meta.elapsed.map(|e| e as f32).unwrap_or(0.0);

            let _ = sender.send(MediaEvent::TrackChanged {
                notif,
                thumbnail_bytes: cached_thumb,
                position_secs,
                duration_secs,
            });
            let _ = sender.send(MediaEvent::PlaybackState(is_playing));

            if !art_url.is_empty() {
                return Some(art_url);
            }
        }
    }
    None
}

fn start_ytmpx_bridge(sender: Sender<MediaEvent>) {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<String>();
    if let Ok(mut guard) = YTMPX_CMD_TX.lock() {
        *guard = Some(cmd_tx);
    }

    thread::spawn(move || {
        let mut last_art_url = String::new();

        loop {
            // DynamicNotification connects purely as a WebSocket client to Tauri YouTube Music on 127.0.0.1:8765.
            // By remaining a client, port 8765 is never blocked or stolen from Tauri YTM,
            // allowing Tauri YTM (server), FocusGrow (client), and DynamicNotification (client) to coexist seamlessly.
            if let Ok(stream) = std::net::TcpStream::connect("127.0.0.1:8765") {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
                if let Ok((mut ws, _)) = tungstenite::client("ws://127.0.0.1:8765", stream) {
                    let _ = ws.get_ref().set_read_timeout(Some(Duration::from_millis(15)));
                    YTMPX_CONNECTED.store(true, std::sync::atomic::Ordering::Relaxed);
                    loop {
                        while let Ok(cmd) = cmd_rx.try_recv() {
                            let payload = if cmd.starts_with('{') {
                                cmd
                            } else {
                                format!(r#"{{"command":"{}"}}"#, cmd)
                            };
                            if ws.send(tungstenite::Message::text(&payload)).is_err() {
                                break;
                            }
                        }
                        match ws.read() {
                            Ok(msg) => {
                                if let Ok(text) = msg.to_text() {
                                    if let Some(art) = handle_ytmpx_json(text, &sender) {
                                        if art != last_art_url {
                                            last_art_url = art.clone();
                                            let s_art = sender.clone();
                                            thread::spawn(move || {
                                                if let Some(bytes) = fetch_cover_bytes(&art) {
                                                    let _ = s_art.send(MediaEvent::ThumbnailLoaded(bytes));
                                                }
                                            });
                                        } else if let Some(cached) = get_cached_cover(&art) {
                                            let _ = sender.send(MediaEvent::ThumbnailLoaded(cached));
                                        }
                                    }
                                }
                            }
                            Err(tungstenite::Error::Io(ref e))
                                if e.kind() == std::io::ErrorKind::WouldBlock
                                    || e.kind() == std::io::ErrorKind::TimedOut => {}
                            Err(_) => break,
                        }
                        thread::sleep(Duration::from_millis(40));
                    }
                    YTMPX_CONNECTED.store(false, std::sync::atomic::Ordering::Relaxed);
                }
            }

            thread::sleep(Duration::from_millis(1000));
        }
    });
}

static LAST_MEDIA_CMD_TIME: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Sends media transport commands (Play/Pause, Next, Previous) to Windows GSMTC or YTMPX.
/// Debounced and single-routed to strictly prevent accidental double-clicks or dual-dispatch double-skipping!
pub fn send_media_command(cmd: MediaCommand) {
    // 1. Debounce check (250ms threshold to prevent hardware/rapid double-triggering)
    if let Ok(mut last) = LAST_MEDIA_CMD_TIME.lock() {
        let now = std::time::Instant::now();
        if let Some(prev) = *last {
            if now.duration_since(prev) < std::time::Duration::from_millis(250) {
                return;
            }
        }
        *last = Some(now);
    }

    // 2. If YTMPX WebSocket is connected (e.g. Tauri YouTube Music):
    // Dispatch exclusively through WebSocket and return immediately!
    // NEVER send both WebSocket AND GSMTC simultaneously, otherwise the player skips 2 tracks!
    if is_ytmpx_connected() {
        match cmd {
            MediaCommand::TogglePlayPause => send_ytmpx_command("playPause"),
            MediaCommand::Previous => send_ytmpx_command("prev"),
            MediaCommand::Next => send_ytmpx_command("next"),
        }
        return;
    }

    // 3. Fallback: Dispatch through Windows GSMTC
    thread::spawn(move || {
        let manager = match GlobalSystemMediaTransportControlsSessionManager::RequestAsync() {
            Ok(op) => match wait_async(op) {
                Ok(m) => m,
                Err(_) => return,
            },
            Err(_) => return,
        };

        if let Some(session) = get_best_media_session(&manager) {
            match cmd {
                MediaCommand::TogglePlayPause => {
                    let _ = session.TryTogglePlayPauseAsync();
                }
                MediaCommand::Previous => {
                    let _ = session.TrySkipPreviousAsync();
                }
                MediaCommand::Next => {
                    let _ = session.TrySkipNextAsync();
                }
            }
        }
    });
}

struct SpotifyWindowContext {
    found: bool,
    is_playing: bool,
    artist: String,
    title: String,
}

unsafe extern "system" fn enum_spotify_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if !IsWindow(Some(hwnd)).as_bool() || !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return BOOL(1);
    }
    let exe = get_process_name(pid).to_lowercase();
    if exe == "spotify.exe" {
        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title_buf) as usize;
        if len > 0 {
            let title = String::from_utf16_lossy(&title_buf[..len]);
            let lower = title.to_lowercase();
            let out = &mut *(lparam.0 as *mut SpotifyWindowContext);
            out.found = true;
            if lower != "spotify" && lower != "spotify free" && lower != "spotify premium" && title.contains(" - ") {
                let parts: Vec<&str> = title.splitn(2, " - ").collect();
                out.is_playing = true;
                out.artist = parts[0].trim().to_string();
                out.title = parts[1].trim().to_string();
                return BOOL(0);
            }
        }
    }
    BOOL(1)
}

/// Directly queries Spotify Desktop's Win32 window title to get immediate zero-latency play/pause state.
fn check_spotify_desktop_window() -> Option<(bool, String, String)> {
    let mut ctx = SpotifyWindowContext {
        found: false,
        is_playing: false,
        artist: String::new(),
        title: String::new(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_spotify_window),
            LPARAM(&mut ctx as *mut _ as isize),
        );
    }
    if ctx.found {
        Some((ctx.is_playing, ctx.artist, ctx.title))
    } else {
        None
    }
}

/// Listens for currently playing media in Windows (Spotify, YouTube Music, etc.)
pub fn start_media_listener(sender: Sender<MediaEvent>) {
    // Start YTMPX bridge in parallel
    start_ytmpx_bridge(sender.clone());

    thread::spawn(move || {
        let mut last_title = String::new();
        let mut last_artist = String::new();
        let mut last_is_playing = false;
        let mut current_has_thumbnail = false;
        let mut timeline_tick = 0u32;

        loop {
            thread::sleep(Duration::from_millis(250));

            // Fast Spotify Desktop inspection for zero-latency sync
            let spotify_win = check_spotify_desktop_window();

            let manager = match GlobalSystemMediaTransportControlsSessionManager::RequestAsync() {
                Ok(op) => match wait_async(op) {
                    Ok(m) => m,
                    Err(_) => continue,
                },
                Err(_) => continue,
            };

            let session = match get_best_media_session(&manager) {
                Some(s) => s,
                None => {
                    if last_is_playing {
                        last_is_playing = false;
                        let _ = sender.send(MediaEvent::PlaybackState(false));
                    }
                    continue;
                }
            };

            let source_id = session
                .SourceAppUserModelId()
                .map(|h| h.to_string())
                .unwrap_or_default();
            let raw_source = source_id.to_lowercase();
            let is_spotify_session = raw_source.contains("spotify");

            let mut is_playing = match session.GetPlaybackInfo() {
                Ok(info) => match info.PlaybackStatus() {
                    Ok(status) => {
                        status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing
                    }
                    Err(_) => false,
                },
                Err(_) => false,
            };

            // For Spotify Desktop only: cross-reference with Spotify Win32 window title for instant sync!
            if is_spotify_session {
                if let Some(ref win) = spotify_win {
                    if win.0 {
                        is_playing = true;
                    } else if !win.0 && win.1.is_empty() {
                        is_playing = false;
                    }
                }
            }

            let state_changed = is_playing != last_is_playing;
            if state_changed {
                last_is_playing = is_playing;
                let _ = sender.send(MediaEvent::PlaybackState(is_playing));
            }

            if is_playing {
                let mut pos_secs = 0.0f32;
                let mut dur_secs = 0.0f32;
                if let Ok(tl) = session.GetTimelineProperties() {
                    let base_pos = tl.Position().map(|p| p.Duration as f64 / 10_000_000.0).unwrap_or(0.0);
                    dur_secs = tl.EndTime().map(|e| e.Duration as f32 / 10_000_000.0).unwrap_or(0.0);
                    let last_update = tl.LastUpdatedTime().map(|t| t.UniversalTime as u64).unwrap_or(0);
                    if last_update > 0 {
                        unsafe {
                            let ft = windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime();
                            let now_ft = ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64);
                            let elapsed = (now_ft.saturating_sub(last_update)) as f64 / 10_000_000.0;
                            if elapsed >= 0.0 && elapsed < 43200.0 {
                                pos_secs = (base_pos + elapsed) as f32;
                            } else {
                                pos_secs = base_pos as f32;
                            }
                        }
                    } else {
                        pos_secs = base_pos as f32;
                    }
                    if dur_secs > 0.0 && pos_secs > dur_secs {
                        pos_secs = dur_secs;
                    }
                }

                timeline_tick += 1;
                if timeline_tick % 4 == 0 && dur_secs > 0.0 {
                    let _ = sender.send(MediaEvent::TimelineUpdate {
                        position_secs: pos_secs,
                        duration_secs: dur_secs,
                    });
                }

                if let Ok(prop_op) = session.TryGetMediaPropertiesAsync() {
                    if let Ok(props) = wait_async(prop_op) {
                        let mut title = props.Title().map(|h| h.to_string()).unwrap_or_default();
                        let mut artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
                        let album = props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();

                        // Fallback to Spotify window title if GSMTC string is temporarily blank
                        if title.is_empty() && is_spotify_session {
                            if let Some(ref win) = spotify_win {
                                if !win.2.is_empty() {
                                    title = win.2.clone();
                                    artist = win.1.clone();
                                }
                            }
                        }

                        if !title.is_empty() {
                            let track_changed = title != last_title || artist != last_artist;
                            let play_resumed = is_playing && state_changed;

                            if track_changed || play_resumed {
                                last_title = title.clone();
                                last_artist = artist.clone();

                                let thumbnail_bytes = fetch_thumbnail_bytes(&props);
                                current_has_thumbnail = thumbnail_bytes.is_some();

                                let (app_name, badge_color) = detect_media_service(&raw_source, &title, &artist, &album);

                                let notif = Notification {
                                    app_name,
                                    title: title.clone(),
                                    body: if artist.is_empty() {
                                        if !album.is_empty() {
                                            album
                                        } else {
                                            "Now Playing".to_string()
                                        }
                                    } else {
                                        artist.clone()
                                    },
                                    time_str: "♪".to_string(),
                                    badge_letter: "▶".to_string(),
                                    badge_color,
                                    icon_bytes: None,
                                    aumid: None,
                                };

                                let _ = sender.send(MediaEvent::TrackChanged {
                                    notif,
                                    thumbnail_bytes,
                                    position_secs: pos_secs,
                                    duration_secs: dur_secs,
                                });
                            } else if !current_has_thumbnail {
                                // Thumbnail stream might have arrived slightly after track metadata!
                                if let Some(bytes) = fetch_thumbnail_bytes(&props) {
                                    current_has_thumbnail = true;
                                    let _ = sender.send(MediaEvent::ThumbnailLoaded(bytes));
                                }
                            }
                        }
                    }
                }
            } else {
                timeline_tick = 0;
            }
        }
    });
}

// =========================================================================
// Focus / Activate Media Player Window (YouTube, Spotify, Browser Tab, Video)
// =========================================================================

pub fn detect_media_service(
    raw_source: &str,
    title: &str,
    artist: &str,
    album: &str,
) -> (String, (f32, f32, f32)) {
    let lower_src = raw_source.to_lowercase();
    let lower_title = title.to_lowercase();
    let lower_artist = artist.to_lowercase();
    let lower_album = album.to_lowercase();

    // 1. Direct App Check (Desktop apps, PWA, or specific UWP IDs)
    if lower_src.contains("spotify") || lower_title.contains("spotify") || lower_artist.contains("spotify") {
        return ("Spotify".to_string(), (0.114, 0.725, 0.329));
    }
    if lower_src.contains("netflix") || lower_title.contains("netflix") || lower_artist.contains("netflix") || lower_album.contains("netflix") {
        return ("Netflix".to_string(), (0.898, 0.035, 0.078));
    }
    if lower_src.contains("tauri") || lower_src.contains("ytm") {
        return ("YouTube Music".to_string(), (0.95, 0.10, 0.10));
    }
    if lower_src.contains("applemusic") || lower_src.contains("cider") {
        return ("Apple Music".to_string(), (0.98, 0.20, 0.35));
    }
    if lower_src.contains("deezer") {
        return ("Deezer".to_string(), (0.65, 0.25, 0.90));
    }
    if lower_src.contains("tidal") {
        return ("Tidal".to_string(), (0.0, 0.0, 0.0));
    }

    // 2. Browser Media Check
    let is_browser = lower_src.contains("chrome")
        || lower_src.contains("edge")
        || lower_src.contains("brave")
        || lower_src.contains("firefox")
        || lower_src.contains("opera")
        || lower_src.contains("vivaldi");

    if is_browser {
        // Enumerate open browser windows to inspect active tab titles!
        let mut found_service: Option<(&'static str, (f32, f32, f32))> = None;
        unsafe {
            let _ = EnumWindows(
                Some(enum_detect_browser_service),
                LPARAM(&mut found_service as *mut _ as isize),
            );
        }
        if let Some((name, color)) = found_service {
            return (name.to_string(), color);
        }

        // Check metadata keywords
        if lower_title.contains("netflix") || lower_artist.contains("netflix") {
            return ("Netflix".to_string(), (0.898, 0.035, 0.078));
        }
        if lower_title.contains("youtube music") || lower_album.contains("youtube music") {
            return ("YouTube Music".to_string(), (0.95, 0.10, 0.10));
        }
        if lower_title.contains("youtube") || lower_artist.contains("youtube") {
            return ("YouTube".to_string(), (1.0, 0.18, 0.18));
        }
        if lower_title.contains("soundcloud") {
            return ("SoundCloud".to_string(), (1.0, 0.45, 0.0));
        }
        if lower_title.contains("twitch") {
            return ("Twitch".to_string(), (0.57, 0.28, 0.93));
        }
        if lower_title.contains("bilibili") {
            return ("Bilibili".to_string(), (0.14, 0.75, 0.90));
        }
        if lower_title.contains("disney") {
            return ("Disney+".to_string(), (0.12, 0.25, 0.70));
        }
        if lower_title.contains("prime video") {
            return ("Prime Video".to_string(), (0.0, 0.65, 0.90));
        }
        if !album.is_empty() {
            return ("YouTube Music".to_string(), (0.95, 0.10, 0.10));
        }

        // Fallback for browsers: Use the actual browser name rather than falsely branding it YouTube!
        if lower_src.contains("edge") {
            return ("Microsoft Edge".to_string(), (0.0, 0.471, 0.831));
        }
        if lower_src.contains("chrome") {
            return ("Google Chrome".to_string(), (0.26, 0.52, 0.96));
        }
        if lower_src.contains("firefox") {
            return ("Firefox".to_string(), (1.0, 0.44, 0.12));
        }
        if lower_src.contains("brave") {
            return ("Brave".to_string(), (1.0, 0.33, 0.13));
        }
        return ("Web Media".to_string(), (0.20, 0.65, 1.0));
    }

    if !raw_source.is_empty() {
        (crate::notifications::clean_app_name(raw_source), (0.20, 0.65, 1.0))
    } else {
        ("Media Player".to_string(), (0.20, 0.65, 1.0))
    }
}

unsafe extern "system" fn enum_detect_browser_service(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    let mut title_buf = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut title_buf) as usize;
    if len == 0 {
        return BOOL(1);
    }
    let title = String::from_utf16_lossy(&title_buf[..len]).to_lowercase();

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let exe = get_process_name(pid).to_lowercase();

    let is_browser = exe.contains("chrome")
        || exe.contains("msedge")
        || exe.contains("brave")
        || exe.contains("firefox")
        || exe.contains("opera")
        || exe.contains("vivaldi");

    if is_browser {
        let out = &mut *(lparam.0 as *mut Option<(&'static str, (f32, f32, f32))>);
        if title.contains("netflix") {
            *out = Some(("Netflix", (0.898, 0.035, 0.078)));
            return BOOL(0);
        }
        if title.contains("spotify") {
            *out = Some(("Spotify", (0.114, 0.725, 0.329)));
            return BOOL(0);
        }
        if title.contains("youtube music") {
            *out = Some(("YouTube Music", (0.95, 0.10, 0.10)));
            return BOOL(0);
        }
        if title.contains("youtube") {
            *out = Some(("YouTube", (1.0, 0.18, 0.18)));
            return BOOL(0);
        }
        if title.contains("twitch") {
            *out = Some(("Twitch", (0.57, 0.28, 0.93)));
            return BOOL(0);
        }
        if title.contains("soundcloud") {
            *out = Some(("SoundCloud", (1.0, 0.45, 0.0)));
            return BOOL(0);
        }
        if title.contains("bilibili") {
            *out = Some(("Bilibili", (0.14, 0.75, 0.90)));
            return BOOL(0);
        }
        if title.contains("disney") {
            *out = Some(("Disney+", (0.12, 0.25, 0.70)));
            return BOOL(0);
        }
        if title.contains("prime video") {
            *out = Some(("Prime Video", (0.0, 0.65, 0.90)));
            return BOOL(0);
        }
    }
    BOOL(1)
}

struct WindowFindContext<'a> {
    found: Option<HWND>,
    track_title: Option<&'a str>,
    fallback_browser: Option<HWND>,
}

unsafe extern "system" fn enum_music_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if !IsWindow(Some(hwnd)).as_bool() || (!IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool()) {
        return BOOL(1);
    }

    let mut title_buf = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut title_buf) as usize;
    if len == 0 {
        return BOOL(1);
    }
    let title = String::from_utf16_lossy(&title_buf[..len]).to_lowercase();

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let exe = get_process_name(pid).to_lowercase();

    let ctx = &mut *(lparam.0 as *mut WindowFindContext<'_>);

    // 1. Direct match on track title in window title
    // Supports YouTube videos ("Call My Name Again | Full Cover - YouTube"), Spotify Web,
    // Twitter / X video, Facebook video, Twitch, etc.!
    if let Some(track) = ctx.track_title {
        let clean = track.trim().to_lowercase();
        if !clean.is_empty() {
            if title.contains(&clean) {
                ctx.found = Some(hwnd);
                return BOOL(0);
            }
            // Check first segment before delimiters
            for delim in &['|', '—', '-', '•', ':'] {
                if let Some(part) = clean.split(*delim).next() {
                    let p = part.trim();
                    if p.len() >= 4 && title.contains(p) {
                        ctx.found = Some(hwnd);
                        return BOOL(0);
                    }
                }
            }
            // Check prefix
            let prefix: String = clean.chars().take(18).collect();
            if prefix.len() >= 6 && title.contains(&prefix) {
                ctx.found = Some(hwnd);
                return BOOL(0);
            }
        }
    }

    // 2. YouTube Music (Tauri desktop app, Chrome/Edge PWA, or browser tab)
    if title.contains("youtube music") || exe.contains("youtube music") || exe.contains("ytm") || exe.contains("tauri") {
        ctx.found = Some(hwnd);
        return BOOL(0);
    }

    // 3. Spotify (Desktop app or web player)
    if title.contains("spotify") || exe.contains("spotify") {
        ctx.found = Some(hwnd);
        return BOOL(0);
    }

    // 4. Apple Music, Cider, Deezer, Tidal, Foobar2000, AIMP, VLC
    if title.contains("apple music")
        || title.contains("cider")
        || title.contains("tidal")
        || exe.contains("foobar2000")
        || exe.contains("aimp")
        || exe.contains("vlc")
    {
        ctx.found = Some(hwnd);
        return BOOL(0);
    }

    // 5. Browser media windows (YouTube, Twitch, Twitter/X, Facebook)
    let is_browser = exe.contains("chrome")
        || exe.contains("msedge")
        || exe.contains("brave")
        || exe.contains("firefox")
        || exe.contains("opera")
        || exe.contains("vivaldi");

    if is_browser {
        if title.contains("youtube")
            || title.contains("twitch")
            || title.contains("twitter")
            || title.contains("x.com")
            || title.contains("facebook")
        {
            if ctx.found.is_none() {
                ctx.found = Some(hwnd);
                return BOOL(0);
            }
        }
        if ctx.fallback_browser.is_none() {
            let mut rc = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rc);
            if (rc.right - rc.left) > 300 && (rc.bottom - rc.top) > 200 {
                ctx.fallback_browser = Some(hwnd);
            }
        }
    }

    BOOL(1)
}

pub fn get_process_name(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            ).is_ok();
            let _ = CloseHandle(handle);
            if ok && len > 0 {
                let full = String::from_utf16_lossy(&buf[..len as usize]);
                if let Some(name) = full.rsplit(|c| c == '\\' || c == '/').next() {
                    return name.to_string();
                }
            }
        }
    }
    String::new()
}

/// Reliably activates or toggles the media player application window (YouTube Music, Spotify, or playing browser tab).
/// Bypasses Windows 11 foreground lock restriction.
pub fn bring_or_toggle_media_window(hwnd: HWND) {
    unsafe {
        let fg = GetForegroundWindow();
        if fg == hwnd {
            // If already the foreground active window, minimize it!
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
            return;
        }

        // Restore if minimized, or show if hidden
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }

        let cur_thread = GetCurrentThreadId();
        let fg_thread = GetWindowThreadProcessId(fg, None);

        // Win32 Foreground lock bypass trick using AttachThreadInput
        if fg_thread != 0 && fg_thread != cur_thread {
            let _ = AttachThreadInput(cur_thread, fg_thread, true);
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
            let _ = AttachThreadInput(cur_thread, fg_thread, false);
        } else {
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
        }

        // SwitchToThisWindow forces Windows task switch to the window
        SwitchToThisWindow(hwnd, true);
    }
}

fn find_tauri_window() -> Option<HWND> {
    let mut found: Option<HWND> = None;
    unsafe {
        let _ = EnumWindows(
            Some(enum_find_tauri_window),
            LPARAM(&mut found as *mut _ as isize),
        );
    }
    found
}

unsafe extern "system" fn enum_find_tauri_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if !IsWindow(Some(hwnd)).as_bool() {
        return BOOL(1);
    }
    let is_vis = IsWindowVisible(hwnd).as_bool();
    let is_ico = IsIconic(hwnd).as_bool();
    if !is_vis && !is_ico {
        return BOOL(1);
    }

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return BOOL(1);
    }
    let exe = get_process_name(pid).to_lowercase();

    // Never match web browsers when hunting for dedicated Tauri / YTM desktop window
    let is_browser = exe.contains("chrome")
        || exe.contains("msedge")
        || exe.contains("brave")
        || exe.contains("firefox")
        || exe.contains("opera")
        || exe.contains("vivaldi");
    if is_browser {
        return BOOL(1);
    }

    let mut title_buf = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut title_buf) as usize;
    let title = String::from_utf16_lossy(&title_buf[..len]).to_lowercase();

    // Ignore generic YouTube videos/podcasts
    if title.contains(" - youtube") && !title.contains("youtube music") {
        return BOOL(1);
    }

    let is_match = exe.contains("tauri")
        || exe.contains("youtube music")
        || exe.contains("ytm")
        || title.contains("youtube music");

    if is_match {
        let mut rc = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rc);
        if is_ico || ((rc.right - rc.left) > 150 && (rc.bottom - rc.top) > 100) {
            let out = &mut *(lparam.0 as *mut Option<HWND>);
            *out = Some(hwnd);
            return BOOL(0);
        }
    }
    BOOL(1)
}

/// Activates or toggles the music player application window (YouTube Music Tauri, Spotify, or playing browser tab).
pub fn activate_or_toggle_media_player(track: Option<&crate::island::TrackInfo>) {
    let is_spotify = track.map(|t| {
        let s = t.source.to_lowercase();
        s.contains("spotify")
    }).unwrap_or(false);

    let is_ytm = track.map(|t| {
        let s = t.source.to_lowercase();
        s.contains("tauri") || s.contains("ytm") || s.contains("youtube music")
    }).unwrap_or(false);

    // 1. If Tauri YouTube Music (YTMPX) is connected:
    // Send toggleMusicWindow directly so Tauri natively toggles/restores from tray.
    if !is_spotify {
        send_ytmpx_command("toggleMusicWindow");
        if is_ytmpx_connected() {
            return;
        }
    }

    // 2. Also check if dedicated Tauri / YTM desktop window exists on screen
    if !is_spotify {
        if let Some(hwnd) = find_tauri_window() {
            bring_or_toggle_media_window(hwnd);
            return;
        }
        // If track is explicitly from Tauri/YTM desktop app, NEVER mistakenly focus a random browser window!
        if is_ytm {
            return;
        }
    }

    // 3. Fallback: Locate Spotify or playing browser tab (YouTube video, Twitter, FB)
    let mut ctx = WindowFindContext {
        found: None,
        track_title: track.map(|t| t.title.as_str()),
        fallback_browser: None,
    };

    unsafe {
        let _ = EnumWindows(
            Some(enum_music_windows),
            LPARAM(&mut ctx as *mut _ as isize),
        );
    }

    let target_hwnd = ctx.found.or(ctx.fallback_browser);

    if let Some(hwnd) = target_hwnd {
        bring_or_toggle_media_window(hwnd);
    }
}
