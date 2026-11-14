use crate::island::Notification;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use windows::core::*;
use windows::Media::Control::*;
use windows::Storage::Streams::DataReader;

#[derive(Debug, Clone)]
pub enum MediaEvent {
    TrackChanged {
        notif: Notification,
        thumbnail_bytes: Option<Vec<u8>>,
    },
    ThumbnailLoaded(Vec<u8>),
    PlaybackState(bool), // true = playing, false = paused/stopped
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCommand {
    TogglePlayPause,
    Previous,
    Next,
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
/// Prioritizes dedicated music apps (Spotify, YouTube Music) with album titles over generic video playback.
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

            if !is_playing {
                continue;
            }

            let mut score = 20;

            let source = s
                .SourceAppUserModelId()
                .map(|h| h.to_string().to_lowercase())
                .unwrap_or_default();

            if source.contains("spotify") || source.contains("applemusic") || source.contains("deezer") {
                score += 100;
            } else if source.contains("youtube") && source.contains("music") {
                score += 90;
            }

            if let Ok(prop_op) = s.TryGetMediaPropertiesAsync() {
                if let Ok(props) = wait_async(prop_op) {
                    let album = props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
                    let artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
                    if !album.is_empty() {
                        score += 50; // Music tracks have album titles
                    }
                    if !artist.is_empty() {
                        score += 20;
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

/// Sends media transport commands (Play/Pause, Next, Previous) to Windows GSMTC.
pub fn send_media_command(cmd: MediaCommand) {
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

/// Listens for currently playing media in Windows (Spotify, YouTube Music, etc.)
pub fn start_media_listener(sender: Sender<MediaEvent>) {
    thread::spawn(move || {
        let mut last_title = String::new();
        let mut last_artist = String::new();
        let mut last_is_playing = false;
        let mut current_has_thumbnail = false;

        loop {
            thread::sleep(Duration::from_millis(600));

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

            let is_playing = match session.GetPlaybackInfo() {
                Ok(info) => match info.PlaybackStatus() {
                    Ok(status) => {
                        status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing
                    }
                    Err(_) => false,
                },
                Err(_) => false,
            };

            if is_playing != last_is_playing {
                last_is_playing = is_playing;
                let _ = sender.send(MediaEvent::PlaybackState(is_playing));
            }

            if is_playing {
                if let Ok(prop_op) = session.TryGetMediaPropertiesAsync() {
                    if let Ok(props) = wait_async(prop_op) {
                        let title = props.Title().map(|h| h.to_string()).unwrap_or_default();
                        let artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
                        let album = props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();

                        if !title.is_empty() {
                            if title != last_title || artist != last_artist {
                                last_title = title.clone();
                                last_artist = artist.clone();

                                let thumbnail_bytes = fetch_thumbnail_bytes(&props);
                                current_has_thumbnail = thumbnail_bytes.is_some();

                                let source_id = session
                                    .SourceAppUserModelId()
                                    .map(|h| h.to_string())
                                    .unwrap_or_default();
                                let raw_source = source_id.to_lowercase();

                                let is_music = raw_source.contains("spotify")
                                    || raw_source.contains("music")
                                    || !album.is_empty();

                                let app_name = if raw_source.contains("spotify") {
                                    "Spotify".to_string()
                                } else if is_music {
                                    "YouTube Music".to_string()
                                } else if raw_source.contains("chrome")
                                    || raw_source.contains("edge")
                                    || raw_source.contains("brave")
                                {
                                    "YouTube".to_string()
                                } else if !source_id.is_empty() {
                                    crate::notifications::clean_app_name(&source_id)
                                } else {
                                    "Media Player".to_string()
                                };

                                let badge_color = if app_name.contains("Spotify") {
                                    (0.114, 0.725, 0.329)
                                } else if app_name.contains("Music") {
                                    (0.95, 0.10, 0.10)
                                } else {
                                    (1.0, 0.18, 0.18)
                                };

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
                                    time_str: "♫".to_string(),
                                    badge_letter: "▶".to_string(),
                                    badge_color,
                                };

                                let _ = sender.send(MediaEvent::TrackChanged {
                                    notif,
                                    thumbnail_bytes,
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
            }
        }
    });
}
