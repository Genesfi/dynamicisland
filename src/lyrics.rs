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

/// Estimates the phonetic length (syllables/morae) of a lyric string.
/// In CJK scripts (Japanese Kanji/Kana, Chinese, Korean), every ideograph/syllable block
/// counts as 1 mora/syllable. In Latin/Romaji, syllables are estimated from words & characters.
fn estimate_lyric_units(text: &str) -> f32 {
    let mut cjk_count = 0usize;
    let mut non_cjk_chars = 0usize;
    let mut non_cjk_words = 0usize;
    let mut in_word = false;

    for c in text.chars() {
        let is_cjk = matches!(c as u32,
            0x3040..=0x309F   // Hiragana
            | 0x30A0..=0x30FF // Katakana
            | 0x4E00..=0x9FFF // CJK Unified Ideographs (Kanji / Hanzi)
            | 0x3400..=0x4DBF // CJK Extension A
            | 0xAC00..=0xD7AF // Hangul Syllables (Korean)
            | 0xFF00..=0xFFEF // Fullwidth Forms
        );

        if is_cjk {
            cjk_count += 1;
            if in_word {
                non_cjk_words += 1;
                in_word = false;
            }
        } else if c.is_alphanumeric() {
            non_cjk_chars += 1;
            in_word = true;
        } else if c.is_whitespace() || c.is_ascii_punctuation() {
            if in_word {
                non_cjk_words += 1;
                in_word = false;
            }
        }
    }
    if in_word {
        non_cjk_words += 1;
    }

    if cjk_count > 0 {
        // In CJK (Japanese / Chinese / Korean): each character is a spoken mora/syllable
        let latin_syllables = (non_cjk_words as f32) * 1.5;
        (cjk_count as f32) + latin_syllables
    } else {
        // Western / Romaji / Latin text
        let words = (non_cjk_words as f32).max(1.0);
        let chars = (non_cjk_chars as f32).max(1.0);
        (words * 1.6 + chars * 0.15).max(words)
    }
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

        // Accurate phonetic syllables/morae measurement for both CJK and Latin scripts
        let units = estimate_lyric_units(curr);

        // Vocal phrase duration modeling:
        // In pop/ballad music, typical singing pace is ~0.28s - 0.34s per mora/syllable.
        let est_singing_time = (units * 0.30 + 0.45).max(0.8);

        // Calculate realistic active singing duration within the available gap between lines:
        let active_duration = if gap <= 4.0 {
            // Normal connected lyric lines:
            // Singing occupies almost the entire gap, leaving a natural breath window of ~0.20s - 0.38s
            // before the next line starts.
            let breath = (gap * 0.10).clamp(0.20, 0.38);
            (gap - breath).max(0.4)
        } else if gap <= 6.5 {
            // Moderate pause between musical phrases:
            let natural_max = gap * 0.88;
            est_singing_time.clamp(2.5, natural_max)
        } else {
            // Extended gap (instrumental interlude / verse-chorus break):
            // The vocal line completes naturally, then the rest of the gap is an instrumental wait.
            est_singing_time.clamp(2.0, gap - 1.2)
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

    /// Lazily romanizes CJK text for active playback only when needed.
    /// Defers expensive WinRT JapanesePhoneticAnalyzer COM calls so they are only invoked
    /// for the chosen/displayed song, completely eliminating allocation spikes during candidate search!
    pub fn ensure_romanized(&mut self) {
        if self.has_cjk {
            for line in &mut self.lines {
                if line.romaji_text.is_none() && crate::transliterate::contains_cjk(&line.text) {
                    line.romaji_text = Some(crate::transliterate::romanize_line(&line.text));
                }
            }
        }
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
            for t in timestamps {
                lines.push(LyricLine {
                    time_secs: t,
                    text: lyric_text.to_string(),
                    romaji_text: None,
                });
            }
        }
    }

    lines.sort_by(|a, b| a.time_secs.partial_cmp(&b.time_secs).unwrap_or(std::cmp::Ordering::Equal));

    let has_cjk = lines.iter().any(|l| crate::transliterate::contains_cjk(&l.text));

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
    q: &QueryParts,
    original_title: &str,
    original_artist: &str,
    target_duration: f32,
) -> Vec<(f32, SongLyrics)> {
    let mut queries: Vec<String> = Vec::new();
    if !q.artist.is_empty() {
        queries.push(format!("{} {}", q.core_title, q.artist));
    }
    if let Some(ref alt_a) = q.alt_artist {
        queries.push(format!("{} {}", q.core_title, alt_a));
    }
    if let Some(ref alt_t) = q.alt_title {
        if !q.artist.is_empty() {
            queries.push(format!("{} {}", alt_t, q.artist));
        }
    }
    queries.push(q.core_title.clone());
    queries.dedup();

    let mut ranked: Vec<(f32, NeteaseSong)> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for query in queries {
        let enc_query = url_encode(query.trim());
        let search_url = format!(
            "https://music.163.com/api/search/get/?s={}&type=1&limit=8",
            enc_query
        );
        let Some(json_resp) = fetch_netease_string(&search_url) else { continue };
        let Ok(search_data) = serde_json::from_str::<NeteaseSearchResponse>(&json_resp) else { continue };
        let Some(songs) = search_data.result.and_then(|r| r.songs) else { continue };

        let mut strong_hit = false;
        for song in songs {
            if !seen_ids.insert(song.id) {
                continue;
            }
            let s_title = song.name.as_deref().unwrap_or("");
            let t_score = score_title_match(s_title, q);
            if t_score <= 0.0 {
                // Hard reject wrong song title (e.g. a different track by the same artist)
                continue;
            }
            let artists_joined = song
                .artists
                .as_ref()
                .map(|list| {
                    list.iter()
                        .filter_map(|a| a.name.as_deref())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let a_score = score_artist_match(&artists_joined, q);
            let dur_secs = song.duration.unwrap_or(0.0) / 1000.0;
            let d_score = score_duration_match(dur_secs, target_duration);
            if target_duration > 15.0 && dur_secs > 10.0 && (dur_secs - target_duration).abs() > 45.0 {
                continue;
            }
            let total = t_score + a_score + d_score;
            if total >= 140.0 {
                strong_hit = true;
            }
            ranked.push((total, song));
        }
        if strong_hit {
            break;
        }
    }

    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut candidates = Vec::new();
    for (score, song) in ranked.into_iter().take(4) {
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
                            if is_real_timed_lyrics(&parsed.lines, target_duration) {
                                // +10 provider tie-breaker for NetEase synced LRC
                                candidates.push((score + 10.0, parsed));
                                if candidates.len() >= 2 {
                                    break;
                                }
                            }
                        }
                    }
                } else if lrc_resp.nolyric == Some(true) && candidates.is_empty() {
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
                    candidates.push((score - 20.0, inst));
                }
            }
        }
    }

    candidates
}

#[derive(Deserialize, Debug)]
struct KugouSongItem {
    #[serde(rename = "FileHash")]
    file_hash: Option<String>,
    #[serde(rename = "SongName")]
    song_name: Option<String>,
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
    q: &QueryParts,
    original_title: &str,
    original_artist: &str,
    target_duration: f32,
) -> Vec<(f32, SongLyrics)> {
    let query = if q.artist.is_empty() {
        q.core_title.clone()
    } else {
        format!("{} {}", q.core_title, q.artist)
    };
    let enc_query = url_encode(query.trim());

    let search_url = format!(
        "https://songsearch.kugou.com/song_search_v2?keyword={}&page=1&pagesize=5&platform=WebFilter&format=json",
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

    for item in lists.into_iter().take(5) {
        let hash = match item.file_hash {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };
        let s_name = item.song_name.as_deref().unwrap_or("");
        let t_score = score_title_match(s_name, q);
        if t_score <= 0.0 {
            continue;
        }
        let dur_s_f = item.duration.unwrap_or(target_duration as u32) as f32;
        if target_duration > 15.0 && dur_s_f > 10.0 && (dur_s_f - target_duration).abs() > 35.0 {
            continue;
        }
        let dur_ms = (dur_s_f as u32) * 1000;
        let singer = item.singer_name.unwrap_or_else(|| original_artist.to_string());
        let score = t_score + score_artist_match(&singer, q) + score_duration_match(dur_s_f, target_duration) + 5.0;

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
                                                if is_real_timed_lyrics(&parsed.lines, target_duration) {
                                                    candidates.push((score, parsed));
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

// =========================================================================
// QQ Music (Tencent) Lyric Provider (Backend for BetterLyrics Potato)
// =========================================================================

#[derive(Deserialize, Debug)]
struct QqSinger {
    name: Option<String>,
}

#[derive(Deserialize, Debug)]
struct QqSongItem {
    songmid: Option<String>,
    #[allow(dead_code)]
    songname: Option<String>,
    interval: Option<u32>,
    singer: Option<Vec<QqSinger>>,
}

#[derive(Deserialize, Debug)]
struct QqSongData {
    list: Option<Vec<QqSongItem>>,
}

#[derive(Deserialize, Debug)]
struct QqSearchData {
    song: Option<QqSongData>,
}

#[derive(Deserialize, Debug)]
struct QqSearchResponse {
    data: Option<QqSearchData>,
}

#[derive(Deserialize, Debug)]
struct QqLyricResponse {
    lyric: Option<String>,
}

fn decode_qq_lyric(raw: &str) -> String {
    if let Some(decoded) = base64_decode_string(raw) {
        if decoded.contains('[') {
            return decoded;
        }
    }
    raw.replace("&#58;", ":")
       .replace("&#10;", "\n")
       .replace("&#32;", " ")
       .replace("&#39;", "'")
       .replace("&apos;", "'")
       .replace("&quot;", "\"")
       .replace("&amp;", "&")
}

fn fetch_qq_candidates(
    q: &QueryParts,
    original_title: &str,
    original_artist: &str,
    target_duration: f32,
) -> Vec<(f32, SongLyrics)> {
    let query = if q.artist.is_empty() {
        q.core_title.clone()
    } else {
        format!("{} {}", q.core_title, q.artist)
    };
    let enc_query = url_encode(query.trim());

    let search_url = format!(
        "https://c.y.qq.com/soso/fcgi-bin/client_search_cp?p=1&n=5&w={}&format=json",
        enc_query
    );

    let mut candidates = Vec::new();

    let json_resp = match fetch_url_with_referer(&search_url, Some("https://y.qq.com/portal/player.html")) {
        Some(j) => j,
        None => return candidates,
    };

    let trimmed = json_resp.trim();
    let json_slice = if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        &trimmed[start..=end]
    } else {
        trimmed
    };

    let search_data: QqSearchResponse = match serde_json::from_str(json_slice) {
        Ok(d) => d,
        Err(_) => return candidates,
    };

    let song_list = match search_data.data.and_then(|d| d.song).and_then(|s| s.list) {
        Some(l) => l,
        None => return candidates,
    };

    for item in song_list.into_iter().take(5) {
        let mid = match item.songmid {
            Some(m) if !m.is_empty() => m,
            _ => continue,
        };
        let s_name = item.songname.as_deref().unwrap_or("");
        let t_score = score_title_match(s_name, q);
        if t_score <= 0.0 {
            continue;
        }

        let singer = item.singer.as_ref()
            .and_then(|s| s.first())
            .and_then(|s| s.name.clone())
            .unwrap_or_else(|| original_artist.to_string());

        let dur_s = item.interval.unwrap_or(target_duration as u32);
        if target_duration > 15.0 && dur_s > 10 && ((dur_s as f32) - target_duration).abs() > 35.0 {
            continue;
        }
        let score = t_score + score_artist_match(&singer, q) + score_duration_match(dur_s as f32, target_duration) + 5.0;

        let lyric_url = format!(
            "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg?songmid={}&format=json",
            mid
        );

        if let Some(lyric_json) = fetch_url_with_referer(&lyric_url, Some("https://y.qq.com/portal/player.html")) {
            let l_trimmed = lyric_json.trim();
            let l_slice = if let (Some(start), Some(end)) = (l_trimmed.find('{'), l_trimmed.rfind('}')) {
                &l_trimmed[start..=end]
            } else {
                l_trimmed
            };

            if let Ok(lyric_data) = serde_json::from_str::<QqLyricResponse>(l_slice) {
                if let Some(raw_encoded) = lyric_data.lyric {
                    let decoded_lrc = decode_qq_lyric(&raw_encoded);
                    if !decoded_lrc.trim().is_empty() && decoded_lrc.contains('[') {
                        let mut parsed = parse_lrc(&decoded_lrc, original_title, original_artist);
                        let mins = dur_s / 60;
                        let secs = dur_s % 60;
                        parsed.candidate_info = format!("{} [QQ Music] ({}:{:02})", singer, mins, secs);
                        if is_real_timed_lyrics(&parsed.lines, target_duration) {
                            candidates.push((score, parsed));
                            break;
                        }
                    }
                }
            }
        }
    }

    candidates
}

// =========================================================================
// Apple TTML & BiniLyrics / BetterLyrics Providers
// =========================================================================

fn parse_ttml_time(text: &str) -> Option<f32> {
    let t = text.trim();
    if let Some(ms) = t.strip_suffix("ms") {
        return ms.trim().parse::<f32>().ok().map(|v| v / 1000.0);
    }
    let t = t.strip_suffix('s').unwrap_or(t);
    if t.contains(':') {
        let mut secs = 0.0f32;
        for part in t.split(':') {
            secs = secs * 60.0 + part.trim().parse::<f32>().ok()?;
        }
        Some(secs)
    } else {
        t.parse::<f32>().ok()
    }
}

pub fn parse_ttml(ttml_text: &str, title: &str, artist: &str) -> SongLyrics {
    let mut lines = Vec::new();
    let mut rest = ttml_text;

    while let Some(p_start) = rest.find("<p ") {
        let p_content = &rest[p_start..];
        let Some(p_end) = p_content.find("</p>") else { break };
        let p_str = &p_content[..p_end];
        rest = &p_content[p_end + 4..];

        // Extract begin="xxx"
        if let Some(begin_idx) = p_str.find("begin=\"") {
            let after_begin = &p_str[begin_idx + 7..];
            if let Some(quote_end) = after_begin.find('"') {
                let time_str = &after_begin[..quote_end];
                if let Some(secs) = parse_ttml_time(time_str) {
                    let inner = if let Some(tag_close) = p_str.find('>') {
                        &p_str[tag_close + 1..]
                    } else {
                        ""
                    };

                    // Strip any XML tags like <span ...> or </span>
                    let mut clean_text = String::new();
                    let mut inside_tag = false;
                    for ch in inner.chars() {
                        if ch == '<' {
                            inside_tag = true;
                        } else if ch == '>' {
                            inside_tag = false;
                        } else if !inside_tag {
                            clean_text.push(ch);
                        }
                    }

                    let clean_text = clean_text
                        .replace("&amp;", "&")
                        .replace("&lt;", "<")
                        .replace("&gt;", ">")
                        .replace("&quot;", "\"")
                        .replace("&apos;", "'");
                    let text = clean_text.split_whitespace().collect::<Vec<_>>().join(" ");

                    if !text.is_empty() {
                        lines.push(LyricLine {
                            time_secs: secs,
                            text,
                            romaji_text: None,
                        });
                    }
                }
            }
        }
    }

    lines.sort_by(|a, b| a.time_secs.partial_cmp(&b.time_secs).unwrap_or(std::cmp::Ordering::Equal));
    let has_cjk = lines.iter().any(|l| crate::transliterate::contains_cjk(&l.text));

    SongLyrics {
        title: title.to_string(),
        artist: artist.to_string(),
        lines,
        is_instrumental: false,
        has_cjk,
        raw_lrc: ttml_text.to_string(),
        candidate_info: String::new(),
        candidates: Vec::new(),
    }
}

fn fetch_bini_candidates(
    q: &QueryParts,
    original_title: &str,
    original_artist: &str,
    target_duration: f32,
) -> Vec<(f32, SongLyrics)> {
    let dur_int = target_duration.round() as i32;
    let mut attempts: Vec<(&str, &str)> = vec![(&q.core_title, &q.artist)];
    if let Some(ref alt_a) = q.alt_artist {
        attempts.push((&q.core_title, alt_a));
    }

    let mut candidates = Vec::new();
    for (track_q, artist_q) in attempts {
        let url = format!(
            "https://lyrics-api.binimum.org/?track={}&artist={}&duration={}",
            url_encode(track_q),
            url_encode(artist_q),
            dur_int
        );
        let Some(json_resp) = fetch_url_string(&url) else { continue };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_resp) else { continue };
        let Some(results) = val.get("results").and_then(|r| r.as_array()) else { continue };

        for hit in results.iter().take(4) {
            let Some(lyrics_url) = hit.get("lyricsUrl").and_then(|u| u.as_str()) else { continue };
            let hit_title = hit
                .get("title")
                .or_else(|| hit.get("name"))
                .or_else(|| hit.get("trackName"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            let t_score = if !hit_title.is_empty() {
                let s = score_title_match(hit_title, q);
                if s <= 0.0 {
                    continue;
                }
                s
            } else {
                60.0
            };
            let hit_artist = hit.get("artist").and_then(|a| a.as_str()).unwrap_or(original_artist);
            let hit_dur = hit.get("duration").and_then(|d| d.as_f64()).unwrap_or(target_duration as f64) as f32;
            if target_duration > 15.0 && hit_dur > 10.0 && (hit_dur - target_duration).abs() > 35.0 {
                continue;
            }
            let mins = (hit_dur / 60.0).floor() as u32;
            let secs = (hit_dur % 60.0).round() as u32;
            let score = t_score + score_artist_match(hit_artist, q) + score_duration_match(hit_dur, target_duration) + 12.0;

            if let Some(raw_content) = fetch_url_string(lyrics_url) {
                let mut parsed = if raw_content.contains("<p ") || raw_content.contains("<tt ") {
                    parse_ttml(&raw_content, original_title, original_artist)
                } else {
                    parse_lrc(&raw_content, original_title, original_artist)
                };
                if is_real_timed_lyrics(&parsed.lines, target_duration) {
                    parsed.candidate_info = format!("{} [BiniLyrics] ({}:{:02})", hit_artist, mins, secs);
                    candidates.push((score, parsed));
                    return candidates;
                }
            }
        }
    }
    candidates
}

fn fetch_better_lyrics_candidates(
    q: &QueryParts,
    original_title: &str,
    original_artist: &str,
    target_duration: f32,
) -> Vec<(f32, SongLyrics)> {
    let dur_int = target_duration.round() as i32;
    let mut attempts: Vec<(&str, &str)> = vec![(&q.core_title, &q.artist)];
    if let Some(ref alt_a) = q.alt_artist {
        attempts.push((&q.core_title, alt_a));
    }

    let mut candidates = Vec::new();
    for (track_q, artist_q) in attempts {
        let url = format!(
            "https://lyrics-api.boidu.dev/getLyrics?s={}&a={}&d={}",
            url_encode(track_q),
            url_encode(artist_q),
            dur_int
        );
        let Some(raw_resp) = fetch_url_string(&url) else { continue };
        let mins = (target_duration / 60.0).floor() as u32;
        let secs = (target_duration % 60.0).round() as u32;

        let mut hit_title = String::new();
        let mut hit_artist = artist_q.to_string();
        let mut hit_dur = target_duration;

        let content = if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw_resp) {
            if let Some(t) = val.get("title").or_else(|| val.get("trackName")).or_else(|| val.get("name")).and_then(|v| v.as_str()) {
                hit_title = t.to_string();
            }
            if let Some(a) = val.get("artist").or_else(|| val.get("artistName")).and_then(|v| v.as_str()) {
                hit_artist = a.to_string();
            }
            if let Some(d) = val.get("duration").and_then(|v| v.as_f64()) {
                hit_dur = d as f32;
            }
            if let Some(ttml) = val.get("ttml").and_then(|t| t.as_str()) {
                Some(ttml.to_string())
            } else if let Some(synced) = val.get("syncedLyrics").and_then(|s| s.as_str()) {
                Some(synced.to_string())
            } else if let Some(lrc) = val.get("lyrics").and_then(|l| l.as_str()) {
                Some(lrc.to_string())
            } else {
                None
            }
        } else {
            Some(raw_resp)
        };

        if !hit_title.is_empty() && score_title_match(&hit_title, q) <= 0.0 {
            continue;
        }
        if target_duration > 15.0 && hit_dur > 10.0 && (hit_dur - target_duration).abs() > 35.0 {
            continue;
        }

        if let Some(text) = content {
            let mut parsed = if text.contains("<p ") || text.contains("<tt ") {
                parse_ttml(&text, original_title, original_artist)
            } else {
                parse_lrc(&text, original_title, original_artist)
            };
            if is_real_timed_lyrics(&parsed.lines, target_duration) {
                let t_score = if hit_title.is_empty() { 70.0 } else { score_title_match(&hit_title, q) };
                let score = t_score + score_artist_match(&hit_artist, q) + score_duration_match(hit_dur, target_duration) + 12.0;
                parsed.candidate_info = format!("{} [BetterLyrics] ({}:{:02})", hit_artist, mins, secs);
                candidates.push((score, parsed));
                return candidates;
            }
        }
    }
    candidates
}

#[derive(Debug, Clone)]
struct QueryParts {
    /// Core song title stripped of brackets, "(cover)", " / Original Artist", etc.
    core_title: String,
    /// Full cleaned title (brackets removed, before splitting " / " or " - ").
    clean_title: String,
    /// Optional alternate/translated title (e.g. "Four o'clock" in "四時にはね。 - Four o'clock").
    alt_title: Option<String>,
    /// Cleaned primary/performing artist.
    artist: String,
    /// Optional original artist parsed from cover titles like "ソラニン / ASIAN KUNG-FU GENERATION (cover)".
    alt_artist: Option<String>,
}

fn strip_all_brackets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' | '[' | '{' | '（' | '【' | '『' | '「' | '〔' | '〈' | '《' => depth += 1,
            ')' | ']' | '}' | '）' | '】' | '』' | '」' | '〕' | '〉' | '》' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn strip_noise_suffix(s: &str) -> String {
    let mut out = s
        .replace('’', "'")
        .replace('‘', "'")
        .replace('“', "\"")
        .replace('”', "\"");
    let noise_words = [
        "feat.", "ft.", "official", "music video", "lyric video", "video", "audio",
        "mv", "remaster", "lyrics", "lyric", "soundtrack", "ost", "ver.", "cover", "covered by",
    ];
    for w in noise_words {
        let lower = out.to_lowercase();
        if let Some(pos) = lower.find(w) {
            if !out.is_char_boundary(pos) {
                continue;
            }
            if let Some(open) = out[..pos].rfind(|c| c == '(' || c == '[' || c == '（' || c == '【' || c == '-') {
                out.truncate(open);
            } else if pos > 2 {
                // Standalone trailing word like "... cover"
                let prev = out[..pos].chars().last().unwrap_or(' ');
                if prev.is_whitespace() || prev == '/' || prev == '|' {
                    out.truncate(pos);
                }
            }
        }
    }
    out.trim_matches(|c: char| c.is_whitespace() || c == '.' || c == '…' || c == '-' || c == '—' || c == '/')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_query_parts(raw_title: &str, raw_artist: &str) -> QueryParts {
    let bracketless_t = strip_all_brackets(raw_title);
    let first_pipe = bracketless_t.split(" | ").next().unwrap_or(&bracketless_t);
    let clean_t = strip_noise_suffix(first_pipe);
    let clean_t = if clean_t.is_empty() {
        strip_noise_suffix(raw_title)
    } else {
        clean_t
    };

    let clean_a = strip_noise_suffix(&strip_all_brackets(raw_artist));
    let clean_a = clean_a
        .strip_suffix(" - Topic")
        .or_else(|| clean_a.strip_suffix(" - topic"))
        .unwrap_or(&clean_a)
        .trim()
        .to_string();

    let mut core_title = clean_t.clone();
    let mut alt_title = None;
    let mut alt_artist = None;

    // Handle cover/Japanese YouTube format: "ソラニン / ASIAN KUNG-FU GENERATION (cover)"
    if let Some((left, right)) = clean_t.split_once(" / ").or_else(|| clean_t.split_once("／")) {
        let l = left.trim();
        let r = strip_noise_suffix(right);
        if !l.is_empty() {
            core_title = l.to_string();
        }
        if !r.is_empty() && norm_cmp(&r) != norm_cmp(&clean_a) {
            alt_artist = Some(r);
        }
    } else if let Some((left, right)) = clean_t.split_once(" - ").or_else(|| clean_t.split_once(" — ")) {
        // Handle bilingual titles like "好きでいて - Suki de Ite" or "四時にはね。 - Four o'clock"
        let l = left.trim();
        let r = strip_noise_suffix(right);
        if !l.is_empty() {
            core_title = l.to_string();
        }
        if !r.is_empty() {
            alt_title = Some(r);
        }
    }

    QueryParts {
        core_title,
        clean_title: clean_t,
        alt_title,
        artist: clean_a,
        alt_artist,
    }
}

/// Normalizes a title/artist string for accurate fuzzy comparison across CJK & Latin scripts.
fn norm_cmp(s: &str) -> String {
    strip_all_brackets(s)
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Validates whether a candidate song title actually refers to the playing song.
/// Returns > 0.0 on a real match, or -100.0 to hard-reject unrelated tracks.
fn score_title_match(cand_title: &str, q: &QueryParts) -> f32 {
    let c_norm = norm_cmp(cand_title);
    if c_norm.is_empty() {
        return -100.0;
    }

    let mut best = -100.0f32;
    let mut targets: Vec<&str> = vec![&q.core_title, &q.clean_title];
    if let Some(ref at) = q.alt_title {
        targets.push(at.as_str());
    }

    for target in targets {
        let t_norm = norm_cmp(target);
        if t_norm.is_empty() {
            continue;
        }
        if c_norm == t_norm {
            return 110.0;
        }
        let min_chars = if crate::transliterate::contains_cjk(&t_norm) { 2 } else { 3 };
        let c_chars = c_norm.chars().count();
        let t_chars = t_norm.chars().count();
        if (t_chars >= min_chars && c_norm.contains(&t_norm))
            || (c_chars >= min_chars && t_norm.contains(&c_norm))
        {
            let ratio = (c_chars.min(t_chars) as f32) / (c_chars.max(t_chars).max(1) as f32);
            best = best.max(65.0 + ratio * 30.0);
            continue;
        }

        // Word-level overlap for multi-word Latin titles
        let t_low = target.to_lowercase();
        let c_low = strip_all_brackets(cand_title).to_lowercase();
        let t_words: Vec<&str> = t_low.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() >= 2).collect();
        let c_words: Vec<&str> = c_low.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() >= 2).collect();
        if !t_words.is_empty() && !c_words.is_empty() {
            let hits = t_words.iter().filter(|w| c_words.contains(w)).count();
            let ratio = hits as f32 / t_words.len() as f32;
            if ratio >= 0.5 {
                best = best.max(35.0 + ratio * 35.0);
            }
        }
    }

    best
}

fn score_artist_match(cand_artist: &str, q: &QueryParts) -> f32 {
    let c_norm = norm_cmp(cand_artist);
    if c_norm.is_empty() {
        return 0.0;
    }

    let check = |target: &str, exact_bonus: f32, sub_bonus: f32| -> Option<f32> {
        let t_norm = norm_cmp(target);
        if t_norm.is_empty() {
            return None;
        }
        if c_norm == t_norm {
            return Some(exact_bonus);
        }
        if c_norm.contains(&t_norm) || t_norm.contains(&c_norm) {
            return Some(sub_bonus);
        }
        let c_low = cand_artist.to_lowercase();
        for part in target.to_lowercase().split(&[' ', ',', '/', '&', '・', '、'][..]) {
            if part.chars().count() >= 3 && c_low.contains(part) {
                return Some(sub_bonus * 0.75);
            }
        }
        None
    };

    // Matching the performing artist (e.g. Harutya) gets top priority
    if let Some(s) = check(&q.artist, 85.0, 65.0) {
        return s;
    }
    // For cover songs ("ソラニン / ASIAN KUNG-FU GENERATION"), matching the original artist is still great!
    if let Some(ref alt_a) = q.alt_artist {
        if let Some(s) = check(alt_a, 65.0, 50.0) {
            return s;
        }
    }
    if q.artist.is_empty() { 0.0 } else { -30.0 }
}

fn score_duration_match(cand_dur: f32, target_duration: f32) -> f32 {
    if target_duration <= 10.0 || cand_dur <= 10.0 {
        return 0.0;
    }
    let diff = (cand_dur - target_duration).abs();
    if diff <= 3.0 {
        60.0
    } else if diff <= 8.0 {
        38.0
    } else if diff <= 15.0 {
        15.0
    } else if diff <= 25.0 {
        0.0
    } else {
        -(diff - 25.0).min(55.0)
    }
}

/// Verifies that parsed lyric lines actually have real progressing timestamps (rejects fake 0:00 or 1s counters).
fn is_real_timed_lyrics(lines: &[LyricLine], target_duration: f32) -> bool {
    let sung: Vec<f32> = lines
        .iter()
        .filter(|l| !l.text.trim().is_empty())
        .map(|l| l.time_secs)
        .collect();
    if sung.len() < 2 {
        return false;
    }
    let mut distinct = sung.clone();
    distinct.dedup_by(|a, b| (*a - *b).abs() < 0.02);
    if distinct.len() * 2 < sung.len() {
        return false;
    }
    let last = *distinct.last().unwrap_or(&0.0);
    if last <= 1.0 {
        return false;
    }
    let span = last - distinct[0];
    if target_duration >= 60.0 && sung.len() >= 8 && span < target_duration * 0.25 {
        return false;
    }
    true
}

fn score_candidate(
    cand: &LrclibResponse,
    q: &QueryParts,
    target_duration: f32,
) -> f32 {
    let c_title = cand.track_name.as_deref().unwrap_or("");
    let c_artist = cand.artist_name.as_deref().unwrap_or("");

    let t_score = score_title_match(c_title, q);
    if t_score <= 0.0 {
        return -100.0;
    }

    let mut score = t_score;
    score += score_artist_match(c_artist, q);
    score += score_duration_match(cand.duration.unwrap_or(0.0), target_duration);

    if cand.synced_lyrics.is_some() {
        score += 40.0;
    }

    let target_has_cjk = crate::transliterate::contains_cjk(&q.core_title)
        || crate::transliterate::contains_cjk(&q.artist);
    let lyrics_text = cand.synced_lyrics.as_deref().or(cand.plain_lyrics.as_deref()).unwrap_or("");
    let lyrics_has_cjk = crate::transliterate::contains_cjk(lyrics_text);

    if target_has_cjk && lyrics_has_cjk {
        score += 25.0;
    } else if target_has_cjk && !lyrics_has_cjk && !lyrics_text.is_empty() && score < 140.0 {
        score -= 45.0;
    }

    // +8 provider tie-breaker for LRCLIB
    score + 8.0
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

fn fetch_url_with_referer(url: &str, referer: Option<&str>) -> Option<String> {
    let mut cmd = std::process::Command::new("curl.exe");
    cmd.args(["-s", "-L", "--max-time", "3", url]);
    cmd.args(["-A", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"]);
    if let Some(ref_val) = referer {
        cmd.args(["-H", &format!("Referer: {}", ref_val)]);
    }
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd.output().ok()?;

    if output.status.success() && !output.stdout.is_empty() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

fn fetch_url_string(url: &str) -> Option<String> {
    fetch_url_with_referer(url, None)
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

        // Step A: Check disk cache off the UI thread (supports both LRC and cached TTML)
        if let Some(dir) = get_lyrics_dir() {
            let safe_filename: String = key.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
            let file_path = dir.join(format!("{}.lrc", safe_filename));
            if let Ok(content) = fs::read_to_string(&file_path) {
                let mut parsed = if content.contains("<p ") || content.contains("<tt ") {
                    parse_ttml(&content, &title, &artist)
                } else {
                    parse_lrc(&content, &title, &artist)
                };
                if !parsed.lines.is_empty() || parsed.is_instrumental {
                    parsed.ensure_romanized();
                    if let Ok(mut guard) = LYRICS_CACHE.lock() {
                        let map = guard.get_or_insert_with(HashMap::new);
                        if map.len() > 60 {
                            map.clear();
                        }
                        map.insert(key, parsed.clone());
                    }
                    let _ = sender.send(MediaEvent::LyricsLoaded(parsed));
                    return;
                }
            }
        }

        // Step B: Smart query extraction (handles covers like "ソラニン / ASIAN KUNG-FU GENERATION (cover)")
        let q = extract_query_parts(&title, &artist);

        let enc_title = url_encode(&q.core_title);
        let enc_artist = url_encode(&q.artist);
        let dur_int = duration_secs.round() as i32;

        let mut collected_candidates: Vec<LrclibResponse> = Vec::new();
        let mut have_synced = false;

        // Strategy 1: Exact lookup with duration
        if dur_int > 0 && !q.artist.is_empty() {
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

        // Strategy 2: Lookup without duration
        if !have_synced && !q.artist.is_empty() {
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

        // Strategy 3: Search by core title + artist
        if !have_synced && !q.artist.is_empty() {
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

        // Strategy 3b: Cover fallback — search by core title + original artist (e.g. "ソラニン" + "ASIAN KUNG-FU GENERATION")
        if !have_synced {
            if let Some(ref alt_a) = q.alt_artist {
                let search_url = format!(
                    "https://lrclib.net/api/search?track_name={}&artist_name={}",
                    enc_title,
                    url_encode(alt_a)
                );
                if let Some(json) = fetch_url_string(&search_url) {
                    if add_lrclib_items(&json, &mut collected_candidates) {
                        have_synced = true;
                    }
                }
            }
        }

        // Strategy 4: Fallback search query `q=core_title artist`
        if !have_synced && collected_candidates.is_empty() {
            let search_query = format!("{} {}", q.core_title, q.artist);
            let search_url = format!("https://lrclib.net/api/search?q={}", url_encode(search_query.trim()));
            if let Some(json) = fetch_url_string(&search_url) {
                if add_lrclib_items(&json, &mut collected_candidates) {
                    have_synced = true;
                }
            }
        }

        // Strategy 5: Search endpoint by core track_name alone
        if !have_synced && collected_candidates.is_empty() {
            let search_url = format!("https://lrclib.net/api/search?track_name={}", enc_title);
            if let Some(json) = fetch_url_string(&search_url) {
                add_lrclib_items(&json, &mut collected_candidates);
            }
        }

        // Deduplicate collected LRCLIB candidates
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

        // Score LRCLIB candidates (strictly rejects unrelated titles)
        let mut scored_lrclib: Vec<(f32, LrclibResponse)> = deduped
            .into_iter()
            .map(|c| {
                let s = score_candidate(&c, &q, duration_secs);
                (s, c)
            })
            .filter(|(s, _)| *s >= 35.0)
            .collect();
        scored_lrclib.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut all_scored: Vec<(f32, SongLyrics)> = Vec::new();
        for (score, cand) in scored_lrclib.into_iter().take(4) {
            let raw = cand.synced_lyrics.as_ref().or(cand.plain_lyrics.as_ref());
            if let Some(raw_text) = raw {
                let mut p = parse_lrc(raw_text, &title, &artist);
                p.is_instrumental = cand.instrumental.unwrap_or(false);
                if p.is_instrumental || is_real_timed_lyrics(&p.lines, duration_secs) {
                    let dur = cand.duration.unwrap_or(0.0);
                    let mins = (dur / 60.0).floor() as u32;
                    let secs = (dur % 60.0).round() as u32;
                    p.candidate_info = format!("{} [LRCLIB] ({}:{:02})", cand.artist_name.as_deref().unwrap_or("?"), mins, secs);
                    all_scored.push((score, p));
                }
            } else if cand.instrumental == Some(true) {
                let p = SongLyrics {
                    title: title.clone(),
                    artist: artist.clone(),
                    lines: Vec::new(),
                    is_instrumental: true,
                    has_cjk: false,
                    raw_lrc: String::new(),
                    candidate_info: format!("{} [LRCLIB] (Instrumental)", cand.artist_name.as_deref().unwrap_or("?")),
                    candidates: Vec::new(),
                };
                all_scored.push((score - 20.0, p));
            }
        }

        // Query validated BiniLyrics, BetterLyrics, and NetEase (all return scored, title-verified hits)
        all_scored.extend(fetch_bini_candidates(&q, &title, &artist, duration_secs));
        all_scored.extend(fetch_better_lyrics_candidates(&q, &title, &artist, duration_secs));
        all_scored.extend(fetch_netease_candidates(&q, &title, &artist, duration_secs));

        // Fallback: If still no synced lyrics found from any above, query validated QQ Music and KuGou
        let has_synced_so_far = all_scored.iter().any(|(_, c)| !c.lines.is_empty());
        if !has_synced_so_far {
            all_scored.extend(fetch_qq_candidates(&q, &title, &artist, duration_secs));
            all_scored.extend(fetch_kugou_candidates(&q, &title, &artist, duration_secs));
        }

        // Unified ranking: best title + artist + duration match across ALL providers wins #1!
        all_scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        all_scored.truncate(5);
        let parsed_candidates: Vec<SongLyrics> = all_scored.into_iter().map(|(_, c)| c).collect();

        let final_lyrics = if let Some(first) = parsed_candidates.first() {
            let mut top = first.clone();
            // Cache top candidate to disk
            if let Some(dir) = get_lyrics_dir() {
                let safe_filename: String = key.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
                let file_path = dir.join(format!("{}.lrc", safe_filename));
                let _ = fs::write(file_path, &top.raw_lrc);
            }
            top.ensure_romanized();
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
