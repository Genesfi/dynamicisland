use std::collections::HashMap;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::thread;

use serde::Deserialize;

use crate::media::MediaEvent;

// Run helper processes with NO_WINDOW and background priority so they never interrupt the 60fps/120fps UI loop!
const CREATE_NO_WINDOW: u32 = 0x08000000 | 0x00004000;

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
    pub raw_lrc: String,
    pub candidate_info: String,
    pub candidates: Vec<SongLyrics>,
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

pub fn get_cache_key(artist: &str, title: &str) -> String {
    format!(
        "{}_{}",
        artist.trim().to_lowercase(),
        title.trim().to_lowercase()
    )
}

pub fn get_lyrics_dir() -> Option<PathBuf> {
    static DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        if let Ok(mut p) = std::env::current_exe() {
            p.pop();
            let dir = p.join("lyrics_cache");
            let _ = fs::create_dir_all(&dir);
            Some(dir)
        } else {
            None
        }
    })
    .clone()
}

pub fn remove_from_memory_cache(key: &str) {
    if let Ok(mut guard) = LYRICS_CACHE.lock() {
        if let Some(ref mut map) = *guard {
            map.remove(key);
        }
    }
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
        raw_lrc: lrc_text.to_string(),
        candidate_info: String::new(),
        candidates: Vec::new(),
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

#[derive(Deserialize, Clone, Debug)]
pub struct LrclibResponse {
    pub id: Option<i64>,
    #[serde(rename = "trackName")]
    pub track_name: Option<String>,
    #[serde(rename = "artistName")]
    pub artist_name: Option<String>,
    #[serde(rename = "albumName")]
    #[allow(dead_code)]
    pub album_name: Option<String>,
    pub duration: Option<f32>,
    pub instrumental: Option<bool>,
    #[serde(rename = "plainLyrics")]
    pub plain_lyrics: Option<String>,
    #[serde(rename = "syncedLyrics")]
    pub synced_lyrics: Option<String>,
}

#[derive(Deserialize, Debug)]
struct NeteaseArtist {
    name: Option<String>,
}

#[derive(Deserialize, Debug)]
struct NeteaseSong {
    id: i64,
    #[allow(dead_code)]
    name: Option<String>,
    artists: Option<Vec<NeteaseArtist>>,
    duration: Option<f32>,
}

#[derive(Deserialize, Debug)]
struct NeteaseSearchResult {
    songs: Option<Vec<NeteaseSong>>,
}

#[derive(Deserialize, Debug)]
struct NeteaseSearchResponse {
    result: Option<NeteaseSearchResult>,
}

#[derive(Deserialize, Debug)]
struct NeteaseLyricData {
    lyric: Option<String>,
}

#[derive(Deserialize, Debug)]
struct NeteaseLyricResponse {
    lrc: Option<NeteaseLyricData>,
    #[serde(rename = "nolyric")]
    nolyric: Option<bool>,
    #[allow(dead_code)]
    #[serde(rename = "uncollected")]
    uncollected: Option<bool>,
}

fn fetch_netease_string(url: &str) -> Option<String> {
    let output = std::process::Command::new("curl.exe")
        .args([
            "-s",
            "-L",
            "-A",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
            "-H",
            "Referer: https://music.163.com",
            "-H",
            "Cookie: os=pc",
            "--max-time",
            "4",
            url,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    if output.status.success() && !output.stdout.is_empty() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

fn fetch_netease_candidates(
    clean_title: &str,
    clean_artist: &str,
    original_title: &str,
    original_artist: &str,
    target_duration: f32,
) -> Vec<SongLyrics> {
    let query = if clean_artist.is_empty() {
        clean_title.to_string()
    } else {
        format!("{} {}", clean_title, clean_artist)
    };
    let enc_query = url_encode(query.trim());

    let search_url = format!(
        "https://music.163.com/api/search/get/?s={}&type=1&limit=4",
        enc_query
    );

    let mut candidates = Vec::new();

    let json_resp = match fetch_netease_string(&search_url) {
        Some(j) => j,
        None => return candidates,
    };

    let search_data: NeteaseSearchResponse = match serde_json::from_str(&json_resp) {
        Ok(d) => d,
        Err(_) => return candidates,
    };

    let songs = match search_data.result.and_then(|r| r.songs) {
        Some(s) => s,
        None => return candidates,
    };

    for song in songs.into_iter().take(2) {
        let lyric_url = format!(
            "https://music.163.com/api/song/lyric?id={}&lv=1&kv=1&tv=-1",
            song.id
        );
        if let Some(lrc_json) = fetch_netease_string(&lyric_url) {
            if let Ok(lrc_resp) = serde_json::from_str::<NeteaseLyricResponse>(&lrc_json) {
                let song_artist = song
                    .artists
                    .as_ref()
                    .and_then(|a| a.first())
                    .and_then(|ar| ar.name.clone())
                    .unwrap_or_else(|| original_artist.to_string());
                let dur_secs = song.duration.unwrap_or(0.0) / 1000.0;
                let mins = (dur_secs / 60.0).floor() as u32;
                let secs = (dur_secs % 60.0).round() as u32;

                if let Some(lrc_data) = lrc_resp.lrc {
                    if let Some(raw_lrc) = lrc_data.lyric {
                        if !raw_lrc.trim().is_empty() {
                            let mut parsed = parse_lrc(&raw_lrc, original_title, original_artist);
                            parsed.candidate_info =
                                format!("{} [NetEase] ({}:{:02})", song_artist, mins, secs);
                            if !parsed.lines.is_empty() {
                                candidates.push(parsed);
                                break; // Stop as soon as valid synced lyrics are found
                            }
                        }
                    }
                } else if lrc_resp.nolyric == Some(true) {
                    let inst = SongLyrics {
                        title: original_title.to_string(),
                        artist: original_artist.to_string(),
                        lines: Vec::new(),
                        is_instrumental: true,
                        has_cjk: false,
                        raw_lrc: String::new(),
                        candidate_info: format!("{} [NetEase Instrumental]", song_artist),
                        candidates: Vec::new(),
                    };
                    candidates.push(inst);
                    break;
                }
            }
        }
    }

    if target_duration > 10.0 && candidates.len() > 1 {
        candidates.sort_by(|a, b| {
            let a_has = !a.lines.is_empty();
            let b_has = !b.lines.is_empty();
            b_has.cmp(&a_has)
        });
    }

    candidates
}

#[derive(Deserialize, Debug)]
struct KugouSongItem {
    #[serde(rename = "FileHash")]
    file_hash: Option<String>,
    #[serde(rename = "Duration")]
    duration: Option<u32>,
    #[serde(rename = "SingerName")]
    singer_name: Option<String>,
}

#[derive(Deserialize, Debug)]
struct KugouSearchData {
    lists: Option<Vec<KugouSongItem>>,
}

#[derive(Deserialize, Debug)]
struct KugouSearchResponse {
    data: Option<KugouSearchData>,
}

#[derive(Deserialize, Debug)]
struct KugouLyricCandidate {
    id: Option<String>,
    accesskey: Option<String>,
}

#[derive(Deserialize, Debug)]
struct KugouKrcsResponse {
    candidates: Option<Vec<KugouLyricCandidate>>,
}

#[derive(Deserialize, Debug)]
struct KugouDownloadResponse {
    content: Option<String>,
}

fn base64_decode_string(input: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits = 0;
    for b in input.bytes() {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    let s = String::from_utf8(bytes).ok()?;
    Some(s.strip_prefix('\u{feff}').unwrap_or(&s).to_string())
}

fn fetch_kugou_candidates(
    clean_title: &str,
    clean_artist: &str,
    original_title: &str,
    original_artist: &str,
    target_duration: f32,
) -> Vec<SongLyrics> {
    let query = if clean_artist.is_empty() {
        clean_title.to_string()
    } else {
        format!("{} {}", clean_title, clean_artist)
    };
    let enc_query = url_encode(query.trim());

    let search_url = format!(
        "https://songsearch.kugou.com/song_search_v2?keyword={}&page=1&pagesize=2&platform=WebFilter&format=json",
        enc_query
    );

    let mut candidates = Vec::new();

    let json_resp = match fetch_url_string(&search_url) {
        Some(j) => j,
        None => return candidates,
    };

    let search_data: KugouSearchResponse = match serde_json::from_str(&json_resp) {
        Ok(d) => d,
        Err(_) => return candidates,
    };

    let lists = match search_data.data.and_then(|d| d.lists) {
        Some(l) => l,
        None => return candidates,
    };

    for item in lists.into_iter().take(2) {
        let hash = match item.file_hash {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };
        let dur_ms = item.duration.unwrap_or(target_duration as u32) * 1000;
        let singer = item.singer_name.unwrap_or_else(|| original_artist.to_string());

        let krcs_url = format!(
            "http://krcs.kugou.com/search?ver=1&man=yes&client=mobi&duration={}&hash={}",
            dur_ms, hash
        );

        if let Some(krcs_json) = fetch_url_string(&krcs_url) {
            if let Ok(krcs_data) = serde_json::from_str::<KugouKrcsResponse>(&krcs_json) {
                if let Some(cand_list) = krcs_data.candidates {
                    if let Some(first_cand) = cand_list.first() {
                        if let (Some(id), Some(key)) = (&first_cand.id, &first_cand.accesskey) {
                            let dl_url = format!(
                                "http://lyrics.kugou.com/download?ver=1&client=pc&id={}&accesskey={}&fmt=lrc&charset=utf8",
                                id, key
                            );
                            if let Some(dl_json) = fetch_url_string(&dl_url) {
                                if let Ok(dl_data) = serde_json::from_str::<KugouDownloadResponse>(&dl_json) {
                                    if let Some(b64) = dl_data.content {
                                        if let Some(raw_lrc) = base64_decode_string(&b64) {
                                            if !raw_lrc.trim().is_empty() {
                                                let mut parsed = parse_lrc(&raw_lrc, original_title, original_artist);
                                                let dur_s = dur_ms / 1000;
                                                let mins = dur_s / 60;
                                                let secs = dur_s % 60;
                                                parsed.candidate_info = format!("{} [KuGou] ({}:{:02})", singer, mins, secs);
                                                if !parsed.lines.is_empty() {
                                                    candidates.push(parsed);
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    candidates
}

fn score_candidate(
    cand: &LrclibResponse,
    target_title: &str,
    target_artist: &str,
    target_duration: f32,
) -> f32 {
    let mut score = 0.0f32;

    let c_title = cand.track_name.as_deref().unwrap_or("").trim().to_lowercase();
    let c_artist = cand.artist_name.as_deref().unwrap_or("").trim().to_lowercase();
    let t_title = target_title.trim().to_lowercase();
    let t_artist = target_artist.trim().to_lowercase();

    // 1. Title match
    if !t_title.is_empty() && !c_title.is_empty() {
        if c_title == t_title {
            score += 100.0;
        } else if c_title.contains(&t_title) || t_title.contains(&c_title) {
            score += 65.0;
        } else {
            let t_words: Vec<&str> = t_title.split_whitespace().collect();
            let c_words: Vec<&str> = c_title.split_whitespace().collect();
            let mut matches = 0;
            for w in &t_words {
                if c_words.contains(w) {
                    matches += 1;
                }
            }
            if matches > 0 {
                score += (matches as f32 / t_words.len().max(1) as f32) * 45.0;
            } else {
                score -= 35.0;
            }
        }
    }

    // 2. Artist match
    if !t_artist.is_empty() && !c_artist.is_empty() {
        if c_artist == t_artist {
            score += 90.0;
        } else if c_artist.contains(&t_artist) || t_artist.contains(&c_artist) {
            score += 70.0;
        } else {
            // Check word / separator matches (e.g. "Lotus Juice" in "Lotus Juice, Azumi Takahashi")
            let t_parts: Vec<&str> = t_artist.split(&[' ', ',', '/', '&', '・'][..]).filter(|s| !s.is_empty()).collect();
            let mut matched_part = false;
            for p in &t_parts {
                if p.len() >= 3 && c_artist.contains(p) {
                    matched_part = true;
                    break;
                }
            }
            if matched_part {
                score += 55.0;
            } else {
                // Known artist with 0 overlap: significant penalty
                score -= 60.0;
            }
        }
    }

    // 3. Duration match
    if target_duration > 10.0 {
        if let Some(c_dur) = cand.duration {
            if c_dur > 10.0 {
                let diff = (c_dur - target_duration).abs();
                if diff <= 3.0 {
                    score += 60.0;
                } else if diff <= 8.0 {
                    score += 35.0;
                } else if diff <= 15.0 {
                    score += 10.0;
                } else if diff > 30.0 {
                    score -= (diff - 30.0).min(50.0);
                }
            }
        }
    }

    // 4. Has synced lyrics bonus
    if cand.synced_lyrics.is_some() {
        score += 40.0;
    }

    // 5. CJK language match bonus / mismatch penalty
    let target_has_cjk = crate::transliterate::contains_cjk(target_title) || crate::transliterate::contains_cjk(target_artist);
    let lyrics_text = cand.synced_lyrics.as_deref().or(cand.plain_lyrics.as_deref()).unwrap_or("");
    let lyrics_has_cjk = crate::transliterate::contains_cjk(lyrics_text);

    if target_has_cjk && lyrics_has_cjk {
        score += 35.0;
    } else if target_has_cjk && !lyrics_has_cjk && !lyrics_text.is_empty() {
        // Japanese/CJK song receiving Latin-only lyrics without strong artist match: heavy penalty
        if score < 130.0 {
            score -= 55.0;
        }
    }

    score
}

/// Cleans track title from redundant noise like "(feat. ...)", "[Official Video]", "- Remastered", etc.
fn clean_query_str(s: &str) -> String {
    let mut out = s.replace('’', "'").replace('‘', "'").replace('“', "\"").replace('”', "\"");

    // Remove text inside parentheses / brackets if it contains typical noise words
    let noise_words = ["feat.", "ft.", "official", "video", "audio", "mv", "remaster", "lyrics", "lyric", "soundtrack", "ost", "theme", "ver."];
    for w in noise_words {
        let lower = out.to_lowercase();
        if let Some(pos) = lower.find(w) {
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
        .args(["-s", "-L", "-A", "DynamicIsland/1.0", "--max-time", "3", url])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    if output.status.success() && !output.stdout.is_empty() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

fn add_lrclib_items(json_str: &str, candidates: &mut Vec<LrclibResponse>) -> bool {
    let mut found_synced = false;
    if let Ok(items) = serde_json::from_str::<Vec<LrclibResponse>>(json_str) {
        for item in items {
            if item.synced_lyrics.is_some() {
                found_synced = true;
            }
            if item.synced_lyrics.is_some()
                || item.plain_lyrics.is_some()
                || item.instrumental == Some(true)
            {
                candidates.push(item);
            }
        }
    } else if let Ok(item) = serde_json::from_str::<LrclibResponse>(json_str) {
        if item.synced_lyrics.is_some() {
            found_synced = true;
        }
        if item.synced_lyrics.is_some()
            || item.plain_lyrics.is_some()
            || item.instrumental == Some(true)
        {
            candidates.push(item);
        }
    }
    found_synced
}

/// Asynchronously fetches lyrics from LRCLIB and NetEase, posting the result back via MediaEvent::LyricsLoaded.
/// Entirely non-blocking: in-memory cache is instant; disk cache I/O, parsing, transliteration,
/// and network lookups are executed in a background worker thread.
pub fn fetch_lyrics_async(
    title: String,
    artist: String,
    duration_secs: f32,
    sender: Sender<MediaEvent>,
) {
    let key = get_cache_key(&artist, &title);

    // 1. Instant check in-memory cache (microsecond hash lookup on caller thread)
    if let Ok(guard) = LYRICS_CACHE.lock() {
        if let Some(ref map) = *guard {
            if let Some(cached) = map.get(&key) {
                let _ = sender.send(MediaEvent::LyricsLoaded(cached.clone()));
                return;
            }
        }
    }

    // 2. Spawn background worker immediately: DO NOT do disk I/O, regex parsing,
    // or CJK transliteration on the main UI/animation thread!
    thread::spawn(move || {
        unsafe {
            use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        }

        // Step A: Check disk cache off the UI thread
        if let Some(dir) = get_lyrics_dir() {
            let safe_filename: String = key.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
            let file_path = dir.join(format!("{}.lrc", safe_filename));
            if let Ok(content) = fs::read_to_string(&file_path) {
                let parsed = parse_lrc(&content, &title, &artist);
                if !parsed.lines.is_empty() || parsed.is_instrumental {
                    if let Ok(mut guard) = LYRICS_CACHE.lock() {
                        let map = guard.get_or_insert_with(HashMap::new);
                        map.insert(key, parsed.clone());
                    }
                    let _ = sender.send(MediaEvent::LyricsLoaded(parsed));
                    return;
                }
            }
        }

        // Step B: Online fetch with smart early-exit (avoids spawning redundant curl processes)
        let clean_title = clean_query_str(&title);
        let clean_artist = clean_query_str(&artist);

        let enc_title = url_encode(&clean_title);
        let enc_artist = url_encode(&clean_artist);
        let dur_int = duration_secs.round() as i32;

        let mut collected_candidates: Vec<LrclibResponse> = Vec::new();
        let mut have_synced = false;

        // Strategy 1: Exact lookup with duration (Fastest & most accurate: exactly 1 curl call)
        if dur_int > 0 && !clean_artist.is_empty() {
            let get_url = format!(
                "https://lrclib.net/api/get?artist_name={}&track_name={}&duration={}",
                enc_artist, enc_title, dur_int
            );
            if let Some(json) = fetch_url_string(&get_url) {
                if add_lrclib_items(&json, &mut collected_candidates) {
                    have_synced = true;
                }
            }
        }

        // Strategy 2: Lookup without duration (Only if Strategy 1 did not return synced lyrics)
        if !have_synced && !clean_artist.is_empty() {
            let get_url = format!(
                "https://lrclib.net/api/get?artist_name={}&track_name={}",
                enc_artist, enc_title
            );
            if let Some(json) = fetch_url_string(&get_url) {
                if add_lrclib_items(&json, &mut collected_candidates) {
                    have_synced = true;
                }
            }
        }

        // Strategy 3: Search endpoint by track & artist fields (Only if still no synced lyrics)
        if !have_synced && !clean_artist.is_empty() {
            let search_url = format!(
                "https://lrclib.net/api/search?track_name={}&artist_name={}",
                enc_title, enc_artist
            );
            if let Some(json) = fetch_url_string(&search_url) {
                if add_lrclib_items(&json, &mut collected_candidates) {
                    have_synced = true;
                }
            }
        }

        // Strategy 4: Fallback search query `q=title artist` (Only if still nothing found)
        if !have_synced && collected_candidates.is_empty() {
            let search_query = format!("{} {}", clean_title, clean_artist);
            let search_url = format!("https://lrclib.net/api/search?q={}", url_encode(search_query.trim()));
            if let Some(json) = fetch_url_string(&search_url) {
                if add_lrclib_items(&json, &mut collected_candidates) {
                    have_synced = true;
                }
            }
        }

        // Strategy 5: Search endpoint by track_name alone (Only if still nothing found)
        if !have_synced && collected_candidates.is_empty() {
            let search_url = format!("https://lrclib.net/api/search?track_name={}", enc_title);
            if let Some(json) = fetch_url_string(&search_url) {
                if add_lrclib_items(&json, &mut collected_candidates) {
                    have_synced = true;
                }
            }
        }

        // Strategy 6: Search query with title alone (Only if still nothing found)
        if !have_synced && collected_candidates.is_empty() {
            let search_url = format!("https://lrclib.net/api/search?q={}", enc_title);
            if let Some(json) = fetch_url_string(&search_url) {
                add_lrclib_items(&json, &mut collected_candidates);
            }
        }

        // Deduplicate collected candidates
        let mut deduped: Vec<LrclibResponse> = Vec::new();
        for item in collected_candidates {
            let is_dup = deduped.iter().any(|d| {
                if let (Some(id1), Some(id2)) = (d.id, item.id) {
                    if id1 == id2 { return true; }
                }
                d.track_name == item.track_name && d.artist_name == item.artist_name && d.duration == item.duration
            });
            if !is_dup {
                deduped.push(item);
            }
        }

        // Score candidates against query track & artist & duration
        let mut scored: Vec<(f32, LrclibResponse)> = deduped
            .into_iter()
            .map(|c| {
                let s = score_candidate(&c, &clean_title, &clean_artist, duration_secs);
                (s, c)
            })
            .collect();

        // Sort descending by score
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Filter candidates with positive threshold
        let valid_candidates: Vec<LrclibResponse> = scored
            .into_iter()
            .filter(|(s, _)| *s >= 15.0)
            .map(|(_, c)| c)
            .take(5)
            .collect();

        let mut parsed_candidates: Vec<SongLyrics> = Vec::new();
        for cand in &valid_candidates {
            let raw = cand.synced_lyrics.as_ref().or(cand.plain_lyrics.as_ref());
            if let Some(raw_text) = raw {
                let mut p = parse_lrc(raw_text, &title, &artist);
                p.is_instrumental = cand.instrumental.unwrap_or(false);
                let dur = cand.duration.unwrap_or(0.0);
                let mins = (dur / 60.0).floor() as u32;
                let secs = (dur % 60.0).round() as u32;
                p.candidate_info = format!("{} ({}:{:02})", cand.artist_name.as_deref().unwrap_or("?"), mins, secs);
                parsed_candidates.push(p);
            } else if cand.instrumental == Some(true) {
                let p = SongLyrics {
                    title: title.clone(),
                    artist: artist.clone(),
                    lines: Vec::new(),
                    is_instrumental: true,
                    has_cjk: false,
                    raw_lrc: String::new(),
                    candidate_info: format!("{} (Instrumental)", cand.artist_name.as_deref().unwrap_or("?")),
                    candidates: Vec::new(),
                };
                parsed_candidates.push(p);
            }
        }

        // NetEase Cloud Music Fallback:
        // ONLY query NetEase if LRCLIB found NO synced lyrics at all!
        let has_synced_lrclib = parsed_candidates.iter().any(|c| !c.lines.is_empty());
        if !has_synced_lrclib {
            let netease_cands = fetch_netease_candidates(
                &clean_title,
                &clean_artist,
                &title,
                &artist,
                duration_secs,
            );

            if !netease_cands.is_empty() {
                let old_lrclib = std::mem::replace(&mut parsed_candidates, netease_cands);
                for item in old_lrclib {
                    parsed_candidates.push(item);
                }
            }
        }

        // KuGou Music Fallback:
        // If neither LRCLIB nor NetEase found synced lyrics, query KuGou!
        let has_synced_so_far = parsed_candidates.iter().any(|c| !c.lines.is_empty());
        if !has_synced_so_far {
            let kugou_cands = fetch_kugou_candidates(
                &clean_title,
                &clean_artist,
                &title,
                &artist,
                duration_secs,
            );

            if !kugou_cands.is_empty() {
                let old_cands = std::mem::replace(&mut parsed_candidates, kugou_cands);
                for item in old_cands {
                    parsed_candidates.push(item);
                }
            }
        }

        let final_lyrics = if let Some(first) = parsed_candidates.first() {
            let mut top = first.clone();
            // Cache top candidate to disk
            if let Some(dir) = get_lyrics_dir() {
                let safe_filename: String = key.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
                let file_path = dir.join(format!("{}.lrc", safe_filename));
                let _ = fs::write(file_path, &top.raw_lrc);
            }
            top.candidates = parsed_candidates;
            top
        } else {
            SongLyrics {
                title: title.clone(),
                artist: artist.clone(),
                lines: Vec::new(),
                is_instrumental: false,
                has_cjk: false,
                raw_lrc: String::new(),
                candidate_info: String::new(),
                candidates: Vec::new(),
            }
        };

        // Save to in-memory cache only if valid lyrics or confirmed instrumental
        if !final_lyrics.lines.is_empty() || final_lyrics.is_instrumental {
            if let Ok(mut guard) = LYRICS_CACHE.lock() {
                let map = guard.get_or_insert_with(HashMap::new);
                if map.len() > 60 {
                    map.clear();
                }
                map.insert(key, final_lyrics.clone());
            }
        }

        let _ = sender.send(MediaEvent::LyricsLoaded(final_lyrics));
    });
}
