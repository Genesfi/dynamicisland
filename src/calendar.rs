//! Astronomical Calendar Math & iCal / Google Calendar Synchronization Engine
//!
//! Inspired by Nock's pure mathematical civil calendar calculations (Howard Hinnant & Sakamoto algorithms)
//! and asynchronous RFC 5545 iCalendar (.ics) synchronization with automatic Google Meet, Zoom,
//! Microsoft Teams, and Webex meeting link detection.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use windows::Win32::System::SystemInformation::GetLocalTime;

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub start: i64,            // Local epoch seconds
    pub title: String,
    pub link: Option<String>,  // Zoom / Google Meet / Teams link
}

/// Thread-safe storage for parsed calendar events
pub static CACHED_EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());
static IS_FETCHING: AtomicBool = AtomicBool::new(false);

const LOOKBACK_SECS: i64 = 600;          // 10 minutes past
const LOOKAHEAD_SECS: i64 = 36 * 3600;    // 36 hours ahead

/// Howard Hinnant's algorithm: Days from civil 1970-01-01 (local epoch day count).
/// Runs in O(1) with zero heap allocations.
#[inline]
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}

/// Howard Hinnant's inverse algorithm: Converts days from 1970-01-01 back to (year, month, day).
#[inline]
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as i64, d as i64)
}

/// Day of week for a given civil day count: 0 = Sunday, 1 = Monday, ..., 6 = Saturday.
#[inline]
pub fn weekday(day: i64) -> i64 {
    (day + 4).rem_euclid(7)
}

/// Sakamoto's algorithm: Day of week (0 = Sunday .. 6 = Saturday) for the 1st of any year and month.
#[inline]
pub fn first_of_month(y: i32, m: u32) -> u32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    ((y + y / 4 - y / 100 + y / 400 + T[(m as usize - 1) % 12] + 1).rem_euclid(7)) as u32
}

/// Number of days in a given Gregorian month with 400-year leap year rule.
#[inline]
pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

/// Weekday (0 = Sunday) of the 1st of the month, given current day of month and weekday.
#[inline]
#[allow(dead_code)]
pub fn first_weekday(day: u32, wday: u32) -> u32 {
    (wday + 7 - (day - 1) % 7) % 7
}

/// Current local time as (local epoch seconds, local - UTC offset seconds, year, month, day, weekday, hour, min).
pub fn current_local_time() -> (i64, i64, u16, u16, u16, u16, u16, u16) {
    let st = unsafe { GetLocalTime() };
    let y = st.wYear as i64;
    let m = st.wMonth as i64;
    let d = st.wDay as i64;
    let h = st.wHour as i64;
    let mi = st.wMinute as i64;
    let s = st.wSecond as i64;

    let local_epoch = days_from_civil(y, m, d) * 86400 + (h * 3600 + mi * 60 + s);
    let utc_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.as_secs() as i64)
        .unwrap_or(local_epoch);

    (local_epoch, local_epoch - utc_epoch, st.wYear, st.wMonth, st.wDay, st.wDayOfWeek, st.wHour, st.wMinute)
}

/// Parses an iCal datetime string (e.g. `20261019T100000Z` or `20261019T100000`) into local epoch seconds.
fn parse_ical_dt(v: &str, off: i64) -> Option<i64> {
    let v = v.trim();
    if v.len() < 15 || !v.is_char_boundary(8) || v.as_bytes()[8] != b'T' {
        return None;
    }
    let n = |a: usize, b: usize| v.get(a..b)?.parse::<i64>().ok();
    let t = days_from_civil(n(0, 4)?, n(4, 6)?, n(6, 8)?) * 86400
        + n(9, 11)? * 3600
        + n(11, 13)? * 60
        + n(13, 15)?;
    Some(if v.ends_with('Z') { t + off } else { t })
}

fn unescape_ical_text(s: &str) -> String {
    s.replace("\\n", " ")
        .replace("\\N", " ")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

/// Detects meeting URLs (Google Meet, Zoom, Microsoft Teams, Webex) in text or URL fields.
pub fn extract_meeting_link(text: &str) -> Option<String> {
    const MEETING_HOSTS: [&str; 5] = [
        "meet.google.com",
        "zoom.us",
        "teams.microsoft.com",
        "teams.live.com",
        "webex.com",
    ];
    let mut rest = text;
    while let Some(i) = rest.find("https://") {
        let tail = &rest[i..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '\\' | '<' | '>' | '"' | '\''))
            .unwrap_or(tail.len());
        let url = &tail[..end];
        if MEETING_HOSTS.iter().any(|h| url.contains(h)) {
            return Some(url.to_string());
        }
        if tail.len() > 8 {
            rest = &tail[8..];
        } else {
            break;
        }
    }
    None
}

#[derive(Default)]
struct RawVEvent {
    uid: String,
    start: Option<i64>,
    title: String,
    text: String,
    url: String,
    rrule: String,
    exdates: Vec<i64>,
    recurrence_id: Option<i64>,
    cancelled: bool,
}

/// Parses raw RFC 5545 iCalendar data into upcoming Events.
pub fn parse_ics(ics_content: &str, now: i64, off: i64) -> Vec<Event> {
    let mut unfolded: Vec<String> = Vec::new();
    for l in ics_content.lines() {
        if l.starts_with(' ') || l.starts_with('\t') {
            if let Some(last) = unfolded.last_mut() {
                last.push_str(&l[1..]);
            }
        } else {
            unfolded.push(l.to_string());
        }
    }

    let mut raws: Vec<RawVEvent> = Vec::new();
    let mut cur: Option<RawVEvent> = None;
    for l in &unfolded {
        if l == "BEGIN:VEVENT" {
            cur = Some(RawVEvent::default());
        } else if l == "END:VEVENT" {
            if let Some(r) = cur.take() {
                raws.push(r);
            }
        } else if let (Some(r), Some((head, val))) = (cur.as_mut(), l.split_once(':')) {
            let name = head.split(';').next().unwrap_or("").to_ascii_uppercase();
            let all_day = head.to_ascii_uppercase().contains("VALUE=DATE")
                && !head.to_ascii_uppercase().contains("DATE-TIME");
            match name.as_str() {
                "UID" => r.uid = val.to_string(),
                "DTSTART" if !all_day => r.start = parse_ical_dt(val, off),
                "SUMMARY" => r.title = unescape_ical_text(val),
                "DESCRIPTION" | "LOCATION" => {
                    r.text.push(' ');
                    r.text.push_str(val);
                }
                "URL" => r.url = val.to_string(),
                "RRULE" => r.rrule = val.to_string(),
                "EXDATE" if !all_day => {
                    r.exdates.extend(val.split(',').filter_map(|v| parse_ical_dt(v, off)))
                }
                "RECURRENCE-ID" if !all_day => r.recurrence_id = parse_ical_dt(val, off),
                "STATUS" => r.cancelled = val.trim().eq_ignore_ascii_case("CANCELLED"),
                _ => {}
            }
        }
    }

    let lo = now - LOOKBACK_SECS;
    let hi = now + LOOKAHEAD_SECS;
    let moved: Vec<(&str, i64)> = raws
        .iter()
        .filter_map(|r| r.recurrence_id.map(|t| (r.uid.as_str(), t.div_euclid(86400))))
        .collect();

    let mut out = Vec::new();
    for r in raws.iter().filter(|r| !r.cancelled) {
        let Some(start) = r.start else { continue };
        let link = extract_meeting_link(&r.text).or_else(|| extract_meeting_link(&r.url));
        let mut push = |t: i64| {
            if (lo..=hi).contains(&t) {
                out.push(Event {
                    start: t,
                    title: r.title.clone(),
                    link: link.clone(),
                });
            }
        };

        let rule: Vec<(&str, &str)> = r.rrule.split(';').filter_map(|p| p.split_once('=')).collect();
        let get = |k: &str| rule.iter().find(|(a, _)| *a == k).map(|(_, v)| *v);
        let (base, tod) = (start.div_euclid(86400), start.rem_euclid(86400));
        let freq = get("FREQ");

        if r.rrule.is_empty() || !matches!(freq, Some("DAILY" | "WEEKLY")) {
            push(start);
            continue;
        }

        let interval = get("INTERVAL").and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
        let until = get("UNTIL").and_then(|v| parse_ical_dt(v, off).or_else(|| parse_ical_dt(&format!("{v}T235959"), off)));
        let byday: Vec<i64> = get("BYDAY")
            .map(|v| {
                v.split(',')
                    .filter_map(|d| match d.trim_start_matches(|c: char| c.is_ascii_digit() || c == '+' || c == '-') {
                        "SU" => Some(0),
                        "MO" => Some(1),
                        "TU" => Some(2),
                        "WE" => Some(3),
                        "TH" => Some(4),
                        "FR" => Some(5),
                        "SA" => Some(6),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();

        for day in (lo.div_euclid(86400) - 1)..=(hi.div_euclid(86400) + 1) {
            if day < base || r.exdates.iter().any(|e| e.div_euclid(86400) == day) || moved.contains(&(r.uid.as_str(), day)) {
                continue;
            }
            let hit = if freq == Some("DAILY") {
                (day - base) % interval == 0
            } else {
                let days = if byday.is_empty() { vec![weekday(base)] } else { byday.clone() };
                (day - base).div_euclid(7) % interval == 0 && days.contains(&weekday(day))
            };
            let t = day * 86400 + tod;
            if hit && until.is_none_or(|u| t <= u) {
                push(t);
            }
        }
    }

    out.sort_by_key(|e| e.start);
    out.dedup();
    out
}

/// Asynchronously fetches calendar ICS from Google Calendar / Outlook in background.
pub fn refresh_calendar_async(url: &str) {
    if url.trim().is_empty() || IS_FETCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let url = url.trim().replacen("webcal://", "https://", 1).replace('\'', "''");
    std::thread::spawn(move || {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let script = format!(
            "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; [Text.Encoding]::UTF8.GetString((Invoke-WebRequest -UseBasicParsing -Uri '{url}' -TimeoutSec 10).RawContentStream.ToArray())"
        );
        if let Ok(out) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                let (now, off, ..) = current_local_time();
                let events = parse_ics(&text, now, off);
                if let Ok(mut lock) = CACHED_EVENTS.lock() {
                    *lock = events;
                }
            }
        }
        IS_FETCHING.store(false, Ordering::SeqCst);
    });
}

/// Finds the next meeting starting within 5 minutes or in the last minute.
pub fn next_soon_meeting(events: &[Event], now_epoch: i64) -> Option<&Event> {
    events.iter().find(|e| (-60..=300).contains(&(e.start - now_epoch)))
}

/// Calendar and Productivity State for Island Cards
#[derive(Debug, Clone)]
pub struct CalendarTracker {
    pub calendar_url: String,
    pub cached_events: Vec<Event>,
    pub last_fetch_time: f32,
    pub soon_meeting: Option<Event>,
}

impl Default for CalendarTracker {
    fn default() -> Self {
        Self {
            calendar_url: String::new(),
            cached_events: Vec::new(),
            last_fetch_time: 0.0,
            soon_meeting: None,
        }
    }
}

impl CalendarTracker {
    #[allow(dead_code)]
    pub fn new(url: &str) -> Self {
        let s = Self {
            calendar_url: url.to_string(),
            cached_events: Vec::new(),
            last_fetch_time: 0.0,
            soon_meeting: None,
        };
        if !url.trim().is_empty() {
            refresh_calendar_async(url);
        }
        s
    }

    pub fn update(&mut self, dt: f32) {
        self.last_fetch_time += dt;
        // Re-fetch every 45 minutes if URL is configured
        if self.last_fetch_time >= 2700.0 && !self.calendar_url.trim().is_empty() {
            self.last_fetch_time = 0.0;
            refresh_calendar_async(&self.calendar_url);
        }

        // Sync local cache from static global
        if let Ok(lock) = CACHED_EVENTS.lock() {
            if !lock.is_empty() && *lock != self.cached_events {
                self.cached_events = lock.clone();
            }
        }

        let (now, ..) = current_local_time();
        self.soon_meeting = next_soon_meeting(&self.cached_events, now).cloned();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calendar_days_and_weekdays() {
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29); // Leap year
        assert_eq!(days_in_month(2026, 11), 30);
        assert_eq!(first_of_month(2026, 11), 0); // Nov 2026 starts on Sunday (0)
        assert_eq!(first_weekday(15, 0), 0);

        let d = days_from_civil(2026, 11, 15);
        assert_eq!(civil_from_days(d), (2026, 11, 15));
        let d2 = days_from_civil(2000, 2, 29);
        assert_eq!(civil_from_days(d2), (2000, 2, 29));
    }

    #[test]
    fn test_meeting_link_extraction() {
        let text = "Please join https://meet.google.com/abc-defg-hij for our sprint planning";
        assert_eq!(
            extract_meeting_link(text).as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
        let zoom = "Join Zoom Meeting: https://zoom.us/j/123456789 pwd=secret";
        assert_eq!(
            extract_meeting_link(zoom).as_deref(),
            Some("https://zoom.us/j/123456789")
        );
    }
}
