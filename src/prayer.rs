//! Pure Rust Astronomical Prayer Times & Celestial Sun Arc Engine
//!
//! Computes accurate Islamic prayer times using astronomical algorithms (Jean Meeus / PrayTimes / FocusGrow)
//! without external API calls or network dependencies.
//! Supports standard calculation methods (Kemenag RI, Muslim World League, Egyptian, Umm al-Qura),
//! custom user prayer offsets, and asynchronous Windows GPS & IP auto-detection.

use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalculationMethod {
    Kemenag,       // Kementerian Agama Republik Indonesia (Fajr 20°, Isha 18°, +2m ihtiyat)
    MWL,           // Muslim World League (Fajr 18°, Isha 17°)
    Egyptian,      // Egyptian General Authority of Survey (Fajr 19.5°, Isha 17.5°)
    UmmAlQura,     // Umm al-Qura University, Makkah (Fajr 18.5°, Isha 90m after Maghrib)
}

impl CalculationMethod {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "mwl" => CalculationMethod::MWL,
            "egypt" | "egyptian" => CalculationMethod::Egyptian,
            "makkah" | "ummalqura" => CalculationMethod::UmmAlQura,
            _ => CalculationMethod::Kemenag,
        }
    }

    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            CalculationMethod::Kemenag => "Kemenag RI",
            CalculationMethod::MWL => "Muslim World League",
            CalculationMethod::Egyptian => "Egyptian Survey",
            CalculationMethod::UmmAlQura => "Umm Al-Qura",
        }
    }

    pub fn fajr_angle(&self) -> f64 {
        match self {
            CalculationMethod::Kemenag => 20.0,
            CalculationMethod::MWL => 18.0,
            CalculationMethod::Egyptian => 19.5,
            CalculationMethod::UmmAlQura => 18.5,
        }
    }

    pub fn isha_angle(&self) -> f64 {
        match self {
            CalculationMethod::Kemenag => 18.0,
            CalculationMethod::MWL => 17.0,
            CalculationMethod::Egyptian => 17.5,
            CalculationMethod::UmmAlQura => 0.0, // Uses 90 min fixed interval
        }
    }

    pub fn ihtiyat_mins(&self) -> f64 {
        match self {
            CalculationMethod::Kemenag => 2.0, // Standard Indonesian caution buffer
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CityPreset {
    pub name: &'static str,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: f64,
}

pub const PRESET_CITIES: &[CityPreset] = &[
    CityPreset { name: "Jakarta", latitude: -6.2088, longitude: 106.8456, timezone: 7.0 },
    CityPreset { name: "Bandung", latitude: -6.9175, longitude: 107.6191, timezone: 7.0 },
    CityPreset { name: "Surabaya", latitude: -7.2575, longitude: 112.7521, timezone: 7.0 },
    CityPreset { name: "Medan", latitude: 3.5952, longitude: 98.6722, timezone: 7.0 },
    CityPreset { name: "Makassar", latitude: -5.1477, longitude: 119.4327, timezone: 8.0 },
    CityPreset { name: "Yogyakarta", latitude: -7.7956, longitude: 110.3695, timezone: 7.0 },
    CityPreset { name: "Makkah", latitude: 21.4225, longitude: 39.8262, timezone: 3.0 },
    CityPreset { name: "Tokyo", latitude: 35.6762, longitude: 139.6503, timezone: 9.0 },
    CityPreset { name: "London", latitude: 51.5074, longitude: -0.1278, timezone: 0.0 },
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrayerTimes {
    pub fajr: f64,      // Fractional hours from midnight (e.g. 4.15 = 04:09)
    pub sunrise: f64,   // e.g. 5.48 = 05:29
    pub dhuhr: f64,     // e.g. 11.75 = 11:45
    pub asr: f64,       // e.g. 15.05 = 15:03
    pub maghrib: f64,   // e.g. 17.85 = 17:51
    pub isha: f64,      // e.g. 18.98 = 18:59
}

impl PrayerTimes {
    pub fn fajr_str(&self) -> String { format_time_hm(self.fajr) }
    pub fn sunrise_str(&self) -> String { format_time_hm(self.sunrise) }
    pub fn dhuhr_str(&self) -> String { format_time_hm(self.dhuhr) }
    pub fn asr_str(&self) -> String { format_time_hm(self.asr) }
    pub fn maghrib_str(&self) -> String { format_time_hm(self.maghrib) }
    pub fn isha_str(&self) -> String { format_time_hm(self.isha) }
}

#[derive(Debug, Clone)]
pub struct NextPrayerInfo {
    pub name: String,
    pub prayer_index: usize, // 0: Subuh, 1: Terbit, 2: Dzuhur, 3: Ashar, 4: Maghrib, 5: Isya
    pub time_str: String,
    pub countdown_str: String,
    pub progress_ratio: f32, // 0.0 .. 1.0 (elapsed ratio in current interval)
    #[allow(dead_code)]
    pub is_near_adzan: bool, // True if within 5 mins of prayer time
}

#[derive(Debug, Clone)]
pub struct SunArcInfo {
    pub is_daylight: bool,
    pub sun_progress: f32, // 0.0 (sunrise) .. 1.0 (sunset)
    #[allow(dead_code)]
    pub sunrise_str: String,
    #[allow(dead_code)]
    pub sunset_str: String,
    pub dhuhr_ratio: f32,
    pub asr_ratio: f32,
    pub status_text: String,
}

#[derive(Debug, Clone)]
pub struct PrayerTracker {
    pub city: String,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: f64,
    pub offset_mins: i32,
    pub method: CalculationMethod,
    pub cached_date: (u16, u16, u16),
    pub cached_times: PrayerTimes,
    pub next_prayer: NextPrayerInfo,
    pub sun_arc: SunArcInfo,
    pub sub_times_str: String,
    pub compact_sub_str: String,
    pub prayer_color_rgb: (f32, f32, f32), // Dynamic Apple-style theme color
    pub is_gps_detecting: bool,
    pub gps_status: Option<String>,
}

impl Default for PrayerTracker {
    fn default() -> Self {
        Self::new("Jakarta", -6.2088, 106.8456, 7.0, 0, CalculationMethod::Kemenag)
    }
}

impl PrayerTracker {
    pub fn new(city: &str, lat: f64, lon: f64, tz: f64, offset_mins: i32, method: CalculationMethod) -> Self {
        let (y, m, d, now_hours) = get_current_date_and_hours();
        let times = calculate_prayer_times(y, m, d, lat, lon, tz, method, offset_mins);
        let next_prayer = calculate_next_prayer(&times, now_hours);
        let sun_arc = calculate_sun_arc(&times, now_hours);
        let prayer_color_rgb = get_prayer_color(next_prayer.prayer_index);

        let sub_times_str = format!(
            "Subuh {} • Terbit {} • Dzuhur {} • Ashar {} • Isya {}",
            times.fajr_str(),
            times.sunrise_str(),
            times.dhuhr_str(),
            times.asr_str(),
            times.isha_str()
        );

        let compact_sub_str = format!(
            "Sub {} • Dzu {} • Ash {} • Mag {} • Isy {}",
            times.fajr_str(),
            times.dhuhr_str(),
            times.asr_str(),
            times.maghrib_str(),
            times.isha_str()
        );

        Self {
            city: city.to_string(),
            latitude: lat,
            longitude: lon,
            timezone: tz,
            offset_mins,
            method,
            cached_date: (y, m, d),
            cached_times: times,
            next_prayer,
            sun_arc,
            sub_times_str,
            compact_sub_str,
            prayer_color_rgb,
            is_gps_detecting: false,
            gps_status: None,
        }
    }

    /// Refresh calculations against real-time clock
    pub fn update(&mut self) {
        let (y, m, d, now_hours) = get_current_date_and_hours();
        if self.cached_date != (y, m, d) {
            self.cached_date = (y, m, d);
            self.cached_times = calculate_prayer_times(
                y, m, d,
                self.latitude, self.longitude, self.timezone,
                self.method,
                self.offset_mins,
            );
            self.sub_times_str = format!(
                "Subuh {} • Terbit {} • Dzuhur {} • Ashar {} • Isya {}",
                self.cached_times.fajr_str(),
                self.cached_times.sunrise_str(),
                self.cached_times.dhuhr_str(),
                self.cached_times.asr_str(),
                self.cached_times.isha_str()
            );
            self.compact_sub_str = format!(
                "Sub {} • Dzu {} • Ash {} • Mag {} • Isy {}",
                self.cached_times.fajr_str(),
                self.cached_times.dhuhr_str(),
                self.cached_times.asr_str(),
                self.cached_times.maghrib_str(),
                self.cached_times.isha_str()
            );
        }

        self.next_prayer = calculate_next_prayer(&self.cached_times, now_hours);
        self.sun_arc = calculate_sun_arc(&self.cached_times, now_hours);
        self.prayer_color_rgb = get_prayer_color(self.next_prayer.prayer_index);
    }

    /// Updates location and recalculates
    pub fn set_location(&mut self, city: &str, lat: f64, lon: f64, tz: f64) {
        self.city = city.to_string();
        self.latitude = lat;
        self.longitude = lon;
        self.timezone = tz;

        let (y, m, d, _) = get_current_date_and_hours();
        self.cached_date = (y, m, d);
        self.cached_times = calculate_prayer_times(
            y, m, d,
            self.latitude, self.longitude, self.timezone,
            self.method,
            self.offset_mins,
        );
        self.update();
    }

    /// Updates minute offset and recalculates
    #[allow(dead_code)]
    pub fn set_offset_mins(&mut self, offset_mins: i32) {
        self.offset_mins = offset_mins.clamp(-30, 30);
        let (y, m, d, _) = get_current_date_and_hours();
        self.cached_times = calculate_prayer_times(
            y, m, d,
            self.latitude, self.longitude, self.timezone,
            self.method,
            self.offset_mins,
        );
        self.update();
    }

    /// Cycles through preset cities (e.g. when clicked in UI)
    pub fn cycle_preset_city(&mut self) {
        let current_idx = PRESET_CITIES.iter().position(|c| c.name.eq_ignore_ascii_case(&self.city));
        let next_idx = match current_idx {
            Some(idx) => (idx + 1) % PRESET_CITIES.len(),
            None => 0,
        };
        let preset = PRESET_CITIES[next_idx];
        self.set_location(preset.name, preset.latitude, preset.longitude, preset.timezone);
    }

    /// Cycles calculation method (Kemenag -> MWL -> Egyptian -> Umm Al-Qura)
    pub fn cycle_method(&mut self) {
        self.method = match self.method {
            CalculationMethod::Kemenag => CalculationMethod::MWL,
            CalculationMethod::MWL => CalculationMethod::Egyptian,
            CalculationMethod::Egyptian => CalculationMethod::UmmAlQura,
            CalculationMethod::UmmAlQura => CalculationMethod::Kemenag,
        };
        let (y, m, d, _) = get_current_date_and_hours();
        self.cached_times = calculate_prayer_times(
            y, m, d,
            self.latitude, self.longitude, self.timezone,
            self.method,
            self.offset_mins,
        );
        self.update();
    }
}

/// Dynamic Apple-style color palette for each prayer time
pub fn get_prayer_color(prayer_index: usize) -> (f32, f32, f32) {
    match prayer_index {
        0 => (0.19, 0.82, 0.35), // Subuh: Emerald Mint Green
        1 => (1.00, 0.75, 0.15), // Terbit: Morning Golden Sun
        2 => (1.00, 0.84, 0.04), // Dzuhur: Solar Sun Gold
        3 => (1.00, 0.58, 0.04), // Ashar: Warm Amber Sunset
        4 => (1.00, 0.28, 0.25), // Maghrib: Coral Rose Red
        _ => (0.72, 0.38, 0.98), // Isya: Deep Twilight Indigo/Violet
    }
}

// --- Astronomical Math Core ---

#[inline]
fn deg_to_rad(deg: f64) -> f64 {
    deg * PI / 180.0
}

#[inline]
fn rad_to_deg(rad: f64) -> f64 {
    rad * 180.0 / PI
}

#[inline]
fn sin_d(deg: f64) -> f64 {
    deg_to_rad(deg).sin()
}

#[inline]
fn cos_d(deg: f64) -> f64 {
    deg_to_rad(deg).cos()
}

#[inline]
fn tan_d(deg: f64) -> f64 {
    deg_to_rad(deg).tan()
}

#[inline]
fn asin_d(x: f64) -> f64 {
    rad_to_deg(x.clamp(-1.0, 1.0).asin())
}

#[inline]
fn acos_d(x: f64) -> f64 {
    rad_to_deg(x.clamp(-1.0, 1.0).acos())
}

#[inline]
fn atan_d(x: f64) -> f64 {
    rad_to_deg(x.atan())
}

#[inline]
fn fix_angle(deg: f64) -> f64 {
    deg.rem_euclid(360.0)
}

#[inline]
fn fix_hour(hour: f64) -> f64 {
    hour.rem_euclid(24.0)
}

/// Calculate Julian Date for a Gregorian Calendar date
pub fn julian_date(year: u16, month: u16, day: u16) -> f64 {
    let mut y = year as i32;
    let mut m = month as i32;
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let a = (y as f64 / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();
    (365.25 * (y as f64 + 4716.0)).floor()
        + (30.6001 * (m as f64 + 1.0)).floor()
        + (day as f64)
        + b
        - 1524.5
}

/// Computes Sun's Right Ascension (RA), Declination (delta), and Equation of Time (EqT in hours)
pub fn sun_coordinates(jd: f64) -> (f64, f64, f64) {
    let d = jd - 2451545.0; // Days since J2000.0 epoch
    let g = fix_angle(357.529 + 0.98560028 * d);
    let q = fix_angle(280.459 + 0.98564736 * d);
    let l = fix_angle(q + 1.915 * sin_d(g) + 0.020 * sin_d(2.0 * g));

    let e = 23.439 - 0.00000036 * d; // Obliquity of ecliptic
    let delta = asin_d(sin_d(e) * sin_d(l));

    let ra = fix_angle(rad_to_deg(
        (cos_d(e) * sin_d(l)).atan2(cos_d(l))
    ));

    let eqt = (q - ra) / 15.0; // Equation of time in hours
    (ra, delta, eqt)
}

/// Calculate Hour Angle (omega in hours) for sun reaching a given altitude
fn compute_hour_angle(altitude: f64, lat: f64, delta: f64) -> f64 {
    let cos_omega = (sin_d(altitude) - sin_d(lat) * sin_d(delta)) / (cos_d(lat) * cos_d(delta));
    let omega = acos_d(cos_omega);
    omega / 15.0
}

/// Calculates exact prayer times for given date, location coordinates, and custom user offset
pub fn calculate_prayer_times(
    year: u16,
    month: u16,
    day: u16,
    latitude: f64,
    longitude: f64,
    timezone: f64,
    method: CalculationMethod,
    offset_mins: i32,
) -> PrayerTimes {
    // Evaluate solar orbital elements near local midday rather than 00:00 UT (FocusGrow alignment)
    let jd = julian_date(year, month, day) + (0.5 - timezone / 24.0);
    let (_, delta, eqt) = sun_coordinates(jd);

    // Solar noon (Transit / Zawal)
    let transit = fix_hour(12.0 + timezone - (longitude / 15.0) - eqt);

    // Safety buffer (ihtiyat in hours) + user offset
    let ihtiyat = (method.ihtiyat_mins() + offset_mins as f64) / 60.0;

    // 1. Fajr (Subuh): sun is at -fajr_angle
    let fajr_angle = -method.fajr_angle();
    let fajr_ha = compute_hour_angle(fajr_angle, latitude, delta);
    let fajr = fix_hour(transit - fajr_ha + ihtiyat);

    // 2. Sunrise (Syuruq): geometric horizon + refraction (-0.8333°)
    let sunrise_ha = compute_hour_angle(-0.8333, latitude, delta);
    let sunrise = fix_hour(transit - sunrise_ha);

    // 3. Dhuhr (Dzuhur): solar noon + slight offset + ihtiyat
    let dhuhr = fix_hour(transit + (2.0 / 60.0) + ihtiyat);

    // 4. Asr (Ashar): standard Shafi'i shadow multiplier (n = 1)
    let asr_alt = atan_d(1.0 / (1.0 + tan_d((latitude - delta).abs())));
    let asr_ha = compute_hour_angle(asr_alt, latitude, delta);
    let asr = fix_hour(transit + asr_ha + ihtiyat);

    // 5. Sunset / Maghrib: sun reaches -0.8333°
    let maghrib = fix_hour(transit + sunrise_ha + ihtiyat);

    // 6. Isha (Isya)
    let isha = if method == CalculationMethod::UmmAlQura {
        fix_hour(maghrib + 1.5) // Exactly 90 minutes after Maghrib
    } else {
        let isha_angle = -method.isha_angle();
        let isha_ha = compute_hour_angle(isha_angle, latitude, delta);
        fix_hour(transit + isha_ha + ihtiyat)
    };

    PrayerTimes {
        fajr,
        sunrise,
        dhuhr,
        asr,
        maghrib,
        isha,
    }
}

pub fn format_time_hm(hours: f64) -> String {
    let total_mins = (hours * 60.0).round() as i32;
    let total_mins = total_mins.rem_euclid(24 * 60);
    let h = total_mins / 60;
    let m = total_mins % 60;
    format!("{:02}:{:02}", h, m)
}

/// Helper to get local Windows time as (year, month, day, fractional_hours)
pub fn get_current_date_and_hours() -> (u16, u16, u16, f64) {
    unsafe {
        use windows::Win32::System::SystemInformation::GetLocalTime;
        let st = GetLocalTime();
        let hours = (st.wHour as f64) + (st.wMinute as f64 / 60.0) + (st.wSecond as f64 / 3600.0);
        (st.wYear, st.wMonth, st.wDay, hours)
    }
}

/// Determine which prayer is coming up next, remaining time, and interval progress
pub fn calculate_next_prayer(times: &PrayerTimes, now: f64) -> NextPrayerInfo {
    let (name, p_idx, target_time, prev_time) = if now < times.fajr {
        ("Subuh", 0, times.fajr, times.isha - 24.0)
    } else if now < times.sunrise {
        ("Terbit", 1, times.sunrise, times.fajr)
    } else if now < times.dhuhr {
        ("Dzuhur", 2, times.dhuhr, times.sunrise)
    } else if now < times.asr {
        ("Ashar", 3, times.asr, times.dhuhr)
    } else if now < times.maghrib {
        ("Maghrib", 4, times.maghrib, times.asr)
    } else if now < times.isha {
        ("Isya", 5, times.isha, times.maghrib)
    } else {
        // After Isha: next is Fajr tomorrow
        ("Subuh", 0, times.fajr + 24.0, times.isha)
    };

    let diff_hours = target_time - now;
    let diff_mins = (diff_hours * 60.0).round() as i32;

    let countdown_str = if diff_mins <= 0 {
        "Adzan sekarang".to_string()
    } else if diff_mins < 60 {
        format!("in {}m", diff_mins)
    } else {
        let h = diff_mins / 60;
        let m = diff_mins % 60;
        if m == 0 {
            format!("in {}h", h)
        } else {
            format!("in {}h {}m", h, m)
        }
    };

    let total_interval = (target_time - prev_time).max(0.1);
    let elapsed = (now - prev_time).max(0.0);
    let progress_ratio = (elapsed / total_interval).clamp(0.0, 1.0) as f32;

    let is_near_adzan = diff_mins.abs() <= 5;

    NextPrayerInfo {
        name: name.to_string(),
        prayer_index: p_idx,
        time_str: format_time_hm(target_time % 24.0),
        countdown_str,
        progress_ratio,
        is_near_adzan,
    }
}

/// Computes solar arc position across daytime and waypoints
pub fn calculate_sun_arc(times: &PrayerTimes, now: f64) -> SunArcInfo {
    let sunrise = times.sunrise;
    let sunset = times.maghrib;
    let day_duration = (sunset - sunrise).max(1.0);

    let dhuhr_ratio = ((times.dhuhr - sunrise) / day_duration).clamp(0.0, 1.0) as f32;
    let asr_ratio = ((times.asr - sunrise) / day_duration).clamp(0.0, 1.0) as f32;

    let is_daylight = now >= sunrise && now <= sunset;
    let (sun_progress, status_text) = if is_daylight {
        let t = ((now - sunrise) / day_duration).clamp(0.0, 1.0) as f32;
        let mins_to_sunset = ((sunset - now) * 60.0).round() as i32;
        let txt = if mins_to_sunset <= 30 {
            format!("Golden hour • Sunset {}", times.maghrib_str())
        } else {
            format!("Daylight • Sunset {}", times.maghrib_str())
        };
        (t, txt)
    } else {
        let _mins_to_sunrise = if now < sunrise {
            ((sunrise - now) * 60.0).round() as i32
        } else {
            ((sunrise + 24.0 - now) * 60.0).round() as i32
        };
        let txt = format!("Night Sky • Sunrise {}", times.sunrise_str());
        // For night mode: smoothly rest near horizon
        (if now > sunset { 1.0 } else { 0.0 }, txt)
    };

    SunArcInfo {
        is_daylight,
        sun_progress,
        sunrise_str: times.sunrise_str(),
        sunset_str: times.maghrib_str(),
        dhuhr_ratio,
        asr_ratio,
        status_text,
    }
}

/// Asynchronously detects location using Windows Location Service (GeoCoordinateWatcher) + IP Fallback
pub fn detect_gps_location_async(callback: std::sync::mpsc::Sender<Result<(f64, f64, f64, String), String>>) {
    std::thread::spawn(move || {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let script = concat!(
            "try { Add-Type -AssemblyName System.Device -ErrorAction Stop; ",
            "$w = New-Object System.Device.Location.GeoCoordinateWatcher('Default'); ",
            "$null = $w.TryStart($false, [TimeSpan]::FromMilliseconds(3500)); ",
            "$deadline = (Get-Date).AddMilliseconds(3500); ",
            "while ($w.Position.Location.IsUnknown -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 150 }; ",
            "$loc = $w.Position.Location; $w.Stop(); ",
            "if (-not $loc.IsUnknown) { ",
            "$off = [int][Math]::Round([TimeZoneInfo]::Local.GetUtcOffset([DateTime]::Now).TotalHours); ",
            "Write-Output ('OK|' + $loc.Latitude.ToString([Globalization.CultureInfo]::InvariantCulture) + '|' + $loc.Longitude.ToString([Globalization.CultureInfo]::InvariantCulture) + '|' + $off + '|Windows GPS'); ",
            "exit 0 } } catch {}; ",
            "try { $r = Invoke-RestMethod -Uri 'http://ip-api.com/json/?fields=status,city,regionName,lat,lon,offset' -TimeoutSec 6; ",
            "if ($r.status -eq 'success') { ",
            "$tz = [int][Math]::Round($r.offset / 3600.0); ",
            "$lbl = (@($r.city, $r.regionName) | Where-Object { $_ }) -join ', '; if (-not $lbl) { $lbl = 'IP Geolocation' }; ",
            "Write-Output ('OK|' + ([double]$r.lat).ToString([Globalization.CultureInfo]::InvariantCulture) + '|' + ([double]$r.lon).ToString([Globalization.CultureInfo]::InvariantCulture) + '|' + $tz + '|' + ($lbl -replace '\\|',' ')); ",
            "exit 0 } } catch {}; ",
            "Write-Output 'ERR|GPS/Network unavailable. Please select city manually.'"
        );

        let res = match std::process::Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let line = stdout.lines().rev().find(|l| l.starts_with("OK|") || l.starts_with("ERR|"));
                match line {
                    Some(l) if l.starts_with("OK|") => {
                        let parts: Vec<&str> = l.splitn(5, '|').collect();
                        if parts.len() == 5 {
                            if let (Ok(lat), Ok(lng), Ok(tz)) = (
                                parts[1].trim().parse::<f64>(),
                                parts[2].trim().parse::<f64>(),
                                parts[3].trim().parse::<f64>(),
                            ) {
                                Ok((lat, lng, tz.clamp(-12.0, 14.0), parts[4].trim().to_string()))
                            } else {
                                Err("Invalid GPS coordinate response".to_string())
                            }
                        } else {
                            Err("Unexpected GPS response format".to_string())
                        }
                    }
                    Some(l) if l.starts_with("ERR|") => {
                        Err(l.trim_start_matches("ERR|").to_string())
                    }
                    _ => Err("GPS unavailable; please pick a city".to_string()),
                }
            }
            Err(e) => Err(format!("GPS launch failed: {e}")),
        };

        let _ = callback.send(res);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jakarta_prayer_times_sanity() {
        // November 15, 2026 in Jakarta (-6.2088, 106.8456, UTC+7) with 0 offset
        let times = calculate_prayer_times(2026, 11, 15, -6.2088, 106.8456, 7.0, CalculationMethod::Kemenag, 0);

        // Standard Jakarta prayer times in November:
        // Subuh: around 04:05 - 04:15
        assert!(times.fajr >= 4.0 && times.fajr <= 4.35, "Fajr was: {}", times.fajr_str());
        // Sunrise: around 05:20 - 05:35
        assert!(times.sunrise >= 5.3 && times.sunrise <= 5.7, "Sunrise was: {}", times.sunrise_str());
        // Dzuhur: around 11:35 - 11:50
        assert!(times.dhuhr >= 11.5 && times.dhuhr <= 11.9, "Dhuhr was: {}", times.dhuhr_str());
        // Ashar: around 14:55 - 15:15
        assert!(times.asr >= 14.8 && times.asr <= 15.3, "Asr was: {}", times.asr_str());
        // Maghrib: around 17:45 - 18:00
        assert!(times.maghrib >= 17.7 && times.maghrib <= 18.1, "Maghrib was: {}", times.maghrib_str());
        // Isya: around 18:55 - 19:15
        assert!(times.isha >= 18.9 && times.isha <= 19.35, "Isha was: {}", times.isha_str());
    }

    #[test]
    fn test_offset_adjustment() {
        let base = calculate_prayer_times(2026, 11, 15, -6.2088, 106.8456, 7.0, CalculationMethod::Kemenag, 0);
        let plus_2 = calculate_prayer_times(2026, 11, 15, -6.2088, 106.8456, 7.0, CalculationMethod::Kemenag, 2);
        let diff_mins = ((plus_2.fajr - base.fajr) * 60.0).round() as i32;
        assert_eq!(diff_mins, 2);
    }
}
