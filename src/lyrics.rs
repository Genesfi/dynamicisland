use std::collections::HashMap;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::thread;

use serde::Deserialize;

use crate::media::MediaEvent;

const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone)]
pub struct LyricLine {
    pub time_secs: f32,
    pub text: String,
    pub romaji_text: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SongLyrics {
    #[allow(dead_code)]
    pub title: String,
    #[allow(dead_code)]
    pub artist: String,
    pub lines: Vec<LyricLine>,
    pub is_instrumental: bool,
    #[allow(dead_code)]
    pub has_cjk: bool,
}

#[derive(Debug, Clone)]
pub struct ActiveLyricInfo<'a> {
    pub prev: Option<&'a str>,
    pub curr: &'a str,
    pub next: Option<&'a str>,
    pub progress: f32,      // 0.0 to 1.0 (vocal karaoke sweep progress)
    pub line_progress: f32, // 0.0 to 1.0 (entire line lifetime progress)
    pub fade_alpha: f32,    // 0.0 to 1.0 (smooth in/out opacity fade curve)
}

impl SongLyrics {
    /// Returns full info (previous, current, next, karaoke progress, and fade alpha) based on current audio playback position.
    pub fn get_active_line_info(&self, pos: f32, use_romaji: bool) -> ActiveLyricInfo<'_> {
        if self.is_instrumental {
            return ActiveLyricInfo {
                prev: None,
                curr: "Instrumental Break",
                next: None,
                progress: 0.0,
                line_progress: 0.0,
                fade_alpha: 1.0,
            };
        }

        if self.lines.is_empty() {
            return ActiveLyricInfo {
                prev: None,
                curr: "•••",
                next: None,
                progress: 0.0,
                line_progress: 0.0,
                fade_alpha: 1.0,
            };
        }

        // Find active line: last line with time_secs <= pos
        let mut active_idx = None;
        for (i, line) in self.lines.iter().enumerate() {
            if line.time_secs <= (pos + 0.15) {
                active_idx = Some(i);
            } else {
                break;
            }
        }

        let idx = match active_idx {
            Some(i) => i,
            None => {
                // Before first lyric line
                let next = self.lines.first().map(|l| {
                    if use_romaji {
                        l.romaji_text.as_deref().unwrap_or(l.text.as_str())
                    } else {
                        l.text.as_str()
                    }
                });
                return ActiveLyricInfo {
                    prev: None,
                    curr: "•••",
                    next,
                    progress: 0.0,
                    line_progress: 0.0,
                    fade_alpha: 1.0,
                };
            }
        };

        let curr_line = &self.lines[idx];
        let curr = if use_romaji {
            curr_line.romaji_text.as_deref().unwrap_or(curr_line.text.as_str())
        } else {
            curr_line.text.as_str()
        };

        if curr.trim().is_empty() {
            let next = self.lines.iter().skip(idx + 1).find(|l| !l.text.trim().is_empty()).map(|l| {
                if use_romaji {
                    l.romaji_text.as_deref().unwrap_or(l.text.as_str())
                } else {
                    l.text.as_str()
                }
            });
            let prev = self.lines[..idx].iter().rev().find(|l| !l.text.trim().is_empty()).map(|l| {
                if use_romaji {
                    l.romaji_text.as_deref().unwrap_or(l.text.as_str())
                } else {
                    l.text.as_str()
                }
            });
            return ActiveLyricInfo {
                prev,
                curr: "•••",
                next,
                progress: 0.0,
                line_progress: 0.0,
                fade_alpha: 1.0,
            };
        }

        let prev = self.lines[..idx].iter().rev().find(|l| !l.text.trim().is_empty()).map(|l| {
            if use_romaji {
                l.romaji_text.as_deref().unwrap_or(l.text.as_str())
            } else {
                l.text.as_str()
            }
        });
        let next = self.lines.iter().skip(idx + 1).find(|l| !l.text.trim().is_empty()).map(|l| {
            if use_romaji {
                l.romaji_text.as_deref().unwrap_or(l.text.as_str())
            } else {
                l.text.as_str()
            }
        });

        let t_start = curr_line.time_secs;
        let t_end = if idx + 1 < self.lines.len() {
            self.lines[idx + 1].time_secs
        } else {
            t_start + 4.0
        };
        let gap = (t_end - t_start).max(0.4);

        // Natural vocal pacing: ~0.38s per word + breath attack/decay
        let word_count = curr.split_whitespace().count().max(1) as f32;
        let est_vocal_time = (word_count * 0.38 + 0.25).clamp(0.8, gap);

        let active_duration = if gap > 2.0 {
            est_vocal_time.min(gap * 0.82).max(1.0)
        } else {
            (gap * 0.92).max(0.4)
        };

        let elapsed = (pos - t_start).max(0.0);
        let progress = (elapsed / active_duration).clamp(0.0, 1.0);
        let line_progress = (elapsed / gap).clamp(0.0, 1.0);

        // Smooth Opacity Fade (In and Out):
        // Fades in over the first 0.35s, stays full 1.0, then fades out in the final 0.30s before next line starts
        let fade_in_dur = 0.35f32.min(gap * 0.30);
        let fade_out_dur = 0.30f32.min(gap * 0.30);
        let time_remaining = (t_end - pos).max(0.0);

        let in_factor = if fade_in_dur > 0.0 { (elapsed / fade_in_dur).clamp(0.0, 1.0) } else { 1.0 };
        let out_factor = if fade_out_dur > 0.0 { (time_remaining / fade_out_dur).clamp(0.0, 1.0) } else { 1.0 };
        let raw_fade = in_factor.min(out_factor);
        // Smoothstep curve: S(x) = 3x^2 - 2x^3 (zero derivative at edges for butter-smooth fade)
        let fade_alpha = raw_fade * raw_fade * (3.0 - 2.0 * raw_fade);

        ActiveLyricInfo {
            prev,
            curr,
            next,
            progress,
            line_progress,
            fade_alpha,
        }
    }

    /// Returns (previous_line, active_current_line, next_line) based on current audio playback position.
    #[allow(dead_code)]
    pub fn get_active_lines(&self, pos: f32, use_romaji: bool) -> (Option<&str>, &str, Option<&str>) {
        let info = self.get_active_line_info(pos, use_romaji);
        (info.prev, info.curr, info.next)
    }
}

static LYRICS_CACHE: Mutex<Option<HashMap<String, SongLyrics>>> = Mutex::new(None);

fn get_cache_key(artist: &str, title: &str) -> String {
    format!(
        "{}_{}",
        artist.trim().to_lowercase(),
        title.trim().to_lowercase()
    )
}

fn get_lyrics_dir() -> Option<PathBuf> {
    if let Ok(mut p) = std::env::current_exe() {
        p.pop();
        let dir = p.join("lyrics_cache");
        let _ = fs::create_dir_all(&dir);
        return Some(dir);
    }
    None
}

/// Parses standard .lrc synced lyrics text.
/// Format examples:
/// [01:23.45] Lyric line text
/// [00:15.678] Another line
/// [00:12.00][00:14.00] Repeated line
pub fn parse_lrc(lrc_text: &str, title: &str, artist: &str) -> SongLyrics {
    let mut lines = Vec::new();

    for raw_line in lrc_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        // Extract all timestamps in brackets [mm:ss.xx]
        let mut text_start = 0;
        let mut timestamps = Vec::new();
        let mut chars = line.char_indices().peekable();

        while let Some((start_idx, ch)) = chars.next() {
            if ch == '[' {
                let mut end_idx = None;
                while let Some((idx, close_ch)) = chars.next() {
                    if close_ch == ']' {
                        end_idx = Some(idx);
                        break;
                    }
                }

                if let Some(close_pos) = end_idx {
                    let tag = &line[start_idx + 1..close_pos];
                    if let Some(secs) = parse_timestamp(tag) {
                        timestamps.push(secs);
                        text_start = close_pos + 1;
                    }
                }
            } else {
                break;
            }
        }

        let lyric_text = line[text_start..].trim();
        if !timestamps.is_empty() {
            let romaji = if crate::transliterate::contains_cjk(lyric_text) {
                Some(crate::transliterate::romanize_line(lyric_text))
            } else {
                None
            };

            for t in timestamps {
                lines.push(LyricLine {
                    time_secs: t,
                    text: lyric_text.to_string(),
                    romaji_text: romaji.clone(),
                });
            }
        }
    }

    lines.sort_by(|a, b| a.time_secs.partial_cmp(&b.time_secs).unwrap_or(std::cmp::Ordering::Equal));

    let has_cjk = lines.iter().any(|l| l.romaji_text.is_some());

    SongLyrics {
        title: title.to_string(),
        artist: artist.to_string(),
        lines,
        is_instrumental: false,
        has_cjk,
    }
}

fn parse_timestamp(tag: &str) -> Option<f32> {
    let parts: Vec<&str> = tag.split(':').collect();
    if parts.len() != 2 {
        return None;
    }

    let mins: f32 = parts[0].parse().ok()?;
    let secs: f32 = parts[1].parse().ok()?;
    Some(mins * 60.0 + secs)
}

#[derive(Deserialize)]
struct LrclibResponse {
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain_lyrics: Option<String>,
    instrumental: Option<bool>,
}

/// Cleans track title from redundant noise like "(feat. ...)", "[Official Video]", "- Remastered", etc.
fn clean_query_str(s: &str) -> String {
    let mut out = s.replace('’', "'").replace('‘', "'").replace('“', "\"").replace('”', "\"");

    // Remove text inside parentheses / brackets if it contains typical noise words
    let noise_words = ["feat.", "ft.", "official", "video", "audio", "mv", "remaster", "lyrics", "lyric", "soundtrack", "ost", "theme", "ver."];
    for w in noise_words {
        let lower = out.to_lowercase();
        if let Some(pos) = lower.find(w) {
            // Find surrounding '(' or '[' or '-'
            if let Some(open) = out[..pos].rfind(|c| c == '(' || c == '[' || c == '-') {
                out.truncate(open);
            }
        }
    }

    out.trim().to_string()
}

fn url_encode(input: &str) -> String {
    let mut encoded = String::new();
    for b in input.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}

fn fetch_url_string(url: &str) -> Option<String> {
    let output = std::process::Command::new("curl.exe")
        .args(["-s", "-L", "--max-time", "3", url])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    if output.status.success() && !output.stdout.is_empty() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

/// Asynchronously fetches lyrics from LRCLIB and posts the result back via MediaEvent::LyricsLoaded.
pub fn fetch_lyrics_async(
    title: String,
    artist: String,
    duration_secs: f32,
    sender: Sender<MediaEvent>,
) {
    let key = get_cache_key(&artist, &title);

    // 1. Check in-memory cache
    if let Ok(guard) = LYRICS_CACHE.lock() {
        if let Some(ref map) = *guard {
            if let Some(cached) = map.get(&key) {
                let _ = sender.send(MediaEvent::LyricsLoaded(cached.clone()));
                return;
            }
        }
    }

    // 2. Check disk cache
    if let Some(dir) = get_lyrics_dir() {
        let safe_filename: String = key.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
        let file_path = dir.join(format!("{}.lrc", safe_filename));
        if let Ok(content) = fs::read_to_string(&file_path) {
            let parsed = parse_lrc(&content, &title, &artist);
            if !parsed.lines.is_empty() {
                if let Ok(mut guard) = LYRICS_CACHE.lock() {
                    let map = guard.get_or_insert_with(HashMap::new);
                    map.insert(key, parsed.clone());
                }
                let _ = sender.send(MediaEvent::LyricsLoaded(parsed));
                return;
            }
        }
    }

    // 3. Fast multi-tiered LRCLIB fetcher in background worker (open, public, zero auth needed)
    thread::spawn(move || {
        let clean_title = clean_query_str(&title);
        let clean_artist = clean_query_str(&artist);

        let enc_title = url_encode(&clean_title);
        let enc_artist = url_encode(&clean_artist);
        let dur_int = duration_secs.round() as i32;

        let mut raw_lrc = None;
        let mut is_instr = false;

        // Strategy 1: Exact lookup with duration (Fastest & most accurate)
        if dur_int > 0 && !clean_artist.is_empty() {
            let get_url = format!(
                "https://lrclib.net/api/get?artist_name={}&track_name={}&duration={}",
                enc_artist, enc_title, dur_int
            );
            if let Some(json_text) = fetch_url_string(&get_url) {
                if let Ok(resp) = serde_json::from_str::<LrclibResponse>(&json_text) {
                    is_instr = resp.instrumental.unwrap_or(false);
                    raw_lrc = resp.synced_lyrics.or(resp.plain_lyrics);
                }
            }
        }

        // Strategy 2: Lookup without duration (Handles duration discrepancies)
        if raw_lrc.is_none() && !is_instr && !clean_artist.is_empty() {
            let get_url = format!(
                "https://lrclib.net/api/get?artist_name={}&track_name={}",
                enc_artist, enc_title
            );
            if let Some(json_text) = fetch_url_string(&get_url) {
                if let Ok(resp) = serde_json::from_str::<LrclibResponse>(&json_text) {
                    is_instr = resp.instrumental.unwrap_or(false);
                    raw_lrc = resp.synced_lyrics.or(resp.plain_lyrics);
                }
            }
        }

        // Strategy 3: Search endpoint by track & artist fields
        if raw_lrc.is_none() && !is_instr {
            let search_url = if !clean_artist.is_empty() {
                format!(
                    "https://lrclib.net/api/search?track_name={}&artist_name={}",
                    enc_title, enc_artist
                )
            } else {
                format!("https://lrclib.net/api/search?track_name={}", enc_title)
            };

            if let Some(json_text) = fetch_url_string(&search_url) {
                if let Ok(results) = serde_json::from_str::<Vec<LrclibResponse>>(&json_text) {
                    for item in results {
                        if item.synced_lyrics.is_some() {
                            is_instr = item.instrumental.unwrap_or(false);
                            raw_lrc = item.synced_lyrics;
                            break;
                        } else if raw_lrc.is_none() && item.plain_lyrics.is_some() {
                            raw_lrc = item.plain_lyrics;
                        }
                    }
                }
            }
        }

        // Strategy 4: Fallback search query
        if raw_lrc.is_none() && !is_instr {
            let search_query = format!("{} {}", clean_title, clean_artist);
            let search_url = format!("https://lrclib.net/api/search?q={}", url_encode(search_query.trim()));

            if let Some(json_text) = fetch_url_string(&search_url) {
                if let Ok(results) = serde_json::from_str::<Vec<LrclibResponse>>(&json_text) {
                    for item in results {
                        if item.synced_lyrics.is_some() {
                            is_instr = item.instrumental.unwrap_or(false);
                            raw_lrc = item.synced_lyrics;
                            break;
                        } else if raw_lrc.is_none() && item.plain_lyrics.is_some() {
                            raw_lrc = item.plain_lyrics;
                        }
                    }
                }
            }
        }

        // Strategy 5: Search endpoint by track_name alone (Crucial for anime/OST/game tracks with Japanese/non-English artist names like Persona 3 / ATLUS)
        if raw_lrc.is_none() && !is_instr {
            let search_url = format!("https://lrclib.net/api/search?track_name={}", enc_title);
            if let Some(json_text) = fetch_url_string(&search_url) {
                if let Ok(results) = serde_json::from_str::<Vec<LrclibResponse>>(&json_text) {
                    for item in results {
                        if item.synced_lyrics.is_some() {
                            is_instr = item.instrumental.unwrap_or(false);
                            raw_lrc = item.synced_lyrics;
                            break;
                        } else if raw_lrc.is_none() && item.plain_lyrics.is_some() {
                            raw_lrc = item.plain_lyrics;
                        }
                    }
                }
            }
        }

        // Strategy 6: Search query with title alone
        if raw_lrc.is_none() && !is_instr {
            let search_url = format!("https://lrclib.net/api/search?q={}", enc_title);
            if let Some(json_text) = fetch_url_string(&search_url) {
                if let Ok(results) = serde_json::from_str::<Vec<LrclibResponse>>(&json_text) {
                    for item in results {
                        if item.synced_lyrics.is_some() {
                            is_instr = item.instrumental.unwrap_or(false);
                            raw_lrc = item.synced_lyrics;
                            break;
                        } else if raw_lrc.is_none() && item.plain_lyrics.is_some() {
                            raw_lrc = item.plain_lyrics;
                        }
                    }
                }
            }
        }

        let lyrics = if let Some(lrc_content) = raw_lrc {
            // Save to disk cache
            if let Some(dir) = get_lyrics_dir() {
                let safe_filename: String = key.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
                let file_path = dir.join(format!("{}.lrc", safe_filename));
                let _ = fs::write(file_path, &lrc_content);
            }

            let mut parsed = parse_lrc(&lrc_content, &title, &artist);
            parsed.is_instrumental = is_instr;
            parsed
        } else {
            SongLyrics {
                title: title.clone(),
                artist: artist.clone(),
                lines: Vec::new(),
                is_instrumental: is_instr,
                has_cjk: false,
            }
        };

        // Save to in-memory cache only if valid lyrics or confirmed instrumental
        if !lyrics.lines.is_empty() || lyrics.is_instrumental {
            if let Ok(mut guard) = LYRICS_CACHE.lock() {
                let map = guard.get_or_insert_with(HashMap::new);
                if map.len() > 60 {
                    map.clear();
                }
                map.insert(key, lyrics.clone());
            }
        }

        let _ = sender.send(MediaEvent::LyricsLoaded(lyrics));
    });
}
