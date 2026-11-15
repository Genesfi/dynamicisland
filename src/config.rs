use std::fs;
use std::path::PathBuf;

use windows::{
    core::*,
    Win32::System::Registry::*,
};

pub fn get_config_path() -> PathBuf {
    if let Ok(mut exe_path) = std::env::current_exe() {
        exe_path.pop();
        let exe_dir_cfg = exe_path.join("config.txt");
        if exe_dir_cfg.exists() {
            return exe_dir_cfg;
        }
        // If running inside target/debug or target/release, check root repo config.txt
        let repo_cfg = exe_path.join("..").join("..").join("config.txt");
        if repo_cfg.exists() {
            if let Ok(canonical) = repo_cfg.canonicalize() {
                return canonical;
            }
        }
        return exe_dir_cfg;
    }
    PathBuf::from("config.txt")
}

pub fn is_autostart_enabled() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, Some(0), KEY_QUERY_VALUE, &mut hkey).is_ok() {
            let mut val_type = REG_VALUE_TYPE::default();
            let mut buf = [0u16; 512];
            let mut size = (buf.len() * 2) as u32;
            let res = RegQueryValueExW(
                hkey,
                w!("DynamicIsland"),
                None,
                Some(&mut val_type),
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut size),
            );
            let _ = RegCloseKey(hkey);
            res.is_ok()
        } else {
            false
        }
    }
}

pub fn set_autostart_enabled(enable: bool) {
    unsafe {
        let mut hkey = HKEY::default();
        let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, Some(0), KEY_SET_VALUE, &mut hkey).is_ok() {
            if enable {
                if let Ok(exe_path) = std::env::current_exe() {
                    let path_str = format!("\"{}\"", exe_path.to_string_lossy());
                    let utf16: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();
                    let _ = RegSetValueExW(
                        hkey,
                        w!("DynamicIsland"),
                        Some(0),
                        REG_SZ,
                        Some(std::slice::from_raw_parts(
                            utf16.as_ptr() as *const u8,
                            utf16.len() * 2,
                        )),
                    );
                }
            } else {
                let _ = RegDeleteValueW(hkey, w!("DynamicIsland"));
            }
            let _ = RegCloseKey(hkey);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandStyle {
    SleekCapsule,   // 510 x 48 (Ultra-slim wide capsule)
    BalancedPill,   // 430 x 58 (Classic balanced Dynamic Island)
    SpaciousCard,   // 460 x 74 (High-profile card view)
    CompactMinimal, // 380 x 44 (Ultra-compact minimal)
}

impl IslandStyle {
    pub fn dimensions(&self) -> (f32, f32, f32) {
        match self {
            IslandStyle::SleekCapsule => (510.0, 48.0, 24.0),
            IslandStyle::BalancedPill => (430.0, 58.0, 29.0),
            IslandStyle::SpaciousCard => (460.0, 74.0, 32.0),
            IslandStyle::CompactMinimal => (380.0, 44.0, 22.0),
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            IslandStyle::SleekCapsule => "Sleek Capsule (Long)",
            IslandStyle::BalancedPill => "Balanced Pill",
            IslandStyle::SpaciousCard => "Spacious Card (Wide)",
            IslandStyle::CompactMinimal => "Compact Minimal",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            IslandStyle::SleekCapsule => "510 x 48 px • Ultra-slim & long",
            IslandStyle::BalancedPill => "430 x 58 px • Classic Dynamic Island",
            IslandStyle::SpaciousCard => "460 x 74 px • Generous height & width",
            IslandStyle::CompactMinimal => "380 x 44 px • Compact & unobtrusive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactDisplayMode {
    Combined,  // Marquee Title + Audio Visualizer Waves
    TitleOnly, // Marquee Title Only
    WaveOnly,  // Audio Visualizer Waves Only
}

impl CompactDisplayMode {
    #[allow(dead_code)]
    pub fn display_name(&self) -> &'static str {
        match self {
            CompactDisplayMode::Combined => "Title & Wave",
            CompactDisplayMode::TitleOnly => "Title Only",
            CompactDisplayMode::WaveOnly => "Wave Only",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub top_margin: f32,
    pub auto_collapse_secs: f32,
    pub island_style: IslandStyle,
    pub island_opacity: f32,
    pub glow_intensity: f32,
    pub vinyl_speed: f32,
    pub compact_mode_view: CompactDisplayMode,
    pub compact_music_width: f32,
    pub compact_lyrics_width: f32,
    pub dynamic_album_color: bool,
    pub enable_media_island: bool,
    pub enable_vinyl_animation: bool,
    pub hover_expand_media: bool,
    pub idle_click_through: bool,
    pub smart_contour: bool,
    pub contour_nudge_y: f32,
    pub start_with_windows: bool,
    pub use_full_media_card: bool,
    pub show_lyrics_in_capsule: bool,
    pub romanize_cjk_lyrics: bool,
    pub prayer_city: String,
    pub prayer_latitude: f64,
    pub prayer_longitude: f64,
    pub prayer_timezone: f64,
    pub prayer_method: String,
    pub prayer_offset_mins: i32,
    pub calendar_url: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            top_margin: 0.0,
            auto_collapse_secs: 5.5,
            island_style: IslandStyle::SleekCapsule,
            island_opacity: 0.95,
            glow_intensity: 0.60,
            vinyl_speed: 1.0,
            compact_mode_view: CompactDisplayMode::Combined,
            compact_music_width: 240.0,
            compact_lyrics_width: 310.0,
            dynamic_album_color: true,
            enable_media_island: true,
            enable_vinyl_animation: true,
            hover_expand_media: true,
            idle_click_through: true,
            smart_contour: true,
            contour_nudge_y: 34.0,
            start_with_windows: is_autostart_enabled(),
            use_full_media_card: false,
            show_lyrics_in_capsule: true,
            romanize_cjk_lyrics: false,
            prayer_city: "Jakarta".to_string(),
            prayer_latitude: -6.2088,
            prayer_longitude: 106.8456,
            prayer_timezone: 7.0,
            prayer_method: "kemenag".to_string(),
            prayer_offset_mins: 0,
            calendar_url: String::new(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let path = get_config_path();
        if let Ok(content) = fs::read_to_string(&path) {
            let mut cfg = Self::default();
            for line in content.lines() {
                let parts: Vec<&str> = line.split('=').map(|s| s.trim()).collect();
                if parts.len() == 2 {
                    match parts[0] {
                        "top_margin" => {
                            if let Ok(val) = parts[1].parse::<f32>() {
                                cfg.top_margin = val.clamp(0.0, 80.0);
                            }
                        }
                        "auto_collapse_secs" => {
                            if let Ok(val) = parts[1].parse::<f32>() {
                                cfg.auto_collapse_secs = val.clamp(2.0, 30.0);
                            }
                        }
                        "island_style" => {
                            cfg.island_style = match parts[1].to_lowercase().as_str() {
                                "balanced" => IslandStyle::BalancedPill,
                                "spacious" => IslandStyle::SpaciousCard,
                                "compact" => IslandStyle::CompactMinimal,
                                _ => IslandStyle::SleekCapsule,
                            };
                        }
                        "island_opacity" => {
                            if let Ok(val) = parts[1].parse::<f32>() {
                                cfg.island_opacity = val.clamp(0.20, 1.0);
                            }
                        }
                        "glow_intensity" => {
                            if let Ok(val) = parts[1].parse::<f32>() {
                                cfg.glow_intensity = val.clamp(0.0, 1.0);
                            }
                        }
                        "vinyl_speed" => {
                            if let Ok(val) = parts[1].parse::<f32>() {
                                cfg.vinyl_speed = val.clamp(0.1, 2.5);
                            }
                        }
                        "compact_mode_view" => {
                            cfg.compact_mode_view = match parts[1].to_lowercase().as_str() {
                                "title" => CompactDisplayMode::TitleOnly,
                                "wave" => CompactDisplayMode::WaveOnly,
                                _ => CompactDisplayMode::Combined,
                            };
                        }
                        "compact_music_width" => {
                            if let Ok(val) = parts[1].parse::<f32>() {
                                cfg.compact_music_width = val.clamp(160.0, 450.0);
                            }
                        }
                        "compact_lyrics_width" => {
                            if let Ok(val) = parts[1].parse::<f32>() {
                                cfg.compact_lyrics_width = val.clamp(180.0, 480.0);
                            }
                        }
                        "dynamic_album_color" => {
                            cfg.dynamic_album_color = parts[1] == "true" || parts[1] == "1";
                        }
                        "enable_media_island" => {
                            cfg.enable_media_island = parts[1] == "true" || parts[1] == "1";
                        }
                        "enable_vinyl_animation" => {
                            cfg.enable_vinyl_animation = parts[1] == "true" || parts[1] == "1";
                        }
                        "hover_expand_media" => {
                            cfg.hover_expand_media = parts[1] == "true" || parts[1] == "1";
                        }
                        "idle_click_through" => {
                            cfg.idle_click_through = parts[1] == "true" || parts[1] == "1";
                        }
                        "smart_contour" => {
                            cfg.smart_contour = parts[1] == "true" || parts[1] == "1";
                        }
                        "contour_nudge_y" => {
                            if let Ok(val) = parts[1].parse::<f32>() {
                                cfg.contour_nudge_y = val.clamp(20.0, 60.0);
                            }
                        }
                        "start_with_windows" => {
                            cfg.start_with_windows = parts[1] == "true" || parts[1] == "1";
                        }
                        "use_full_media_card" => {
                            cfg.use_full_media_card = parts[1] == "true" || parts[1] == "1";
                        }
                        "show_lyrics_in_capsule" => {
                            cfg.show_lyrics_in_capsule = parts[1] == "true" || parts[1] == "1";
                        }
                        "romanize_cjk_lyrics" => {
                            cfg.romanize_cjk_lyrics = parts[1] == "true" || parts[1] == "1";
                        }
                        "prayer_city" => {
                            cfg.prayer_city = parts[1].to_string();
                        }
                        "prayer_latitude" => {
                            if let Ok(val) = parts[1].parse::<f64>() {
                                cfg.prayer_latitude = val.clamp(-90.0, 90.0);
                            }
                        }
                        "prayer_longitude" => {
                            if let Ok(val) = parts[1].parse::<f64>() {
                                cfg.prayer_longitude = val.clamp(-180.0, 180.0);
                            }
                        }
                        "prayer_timezone" => {
                            if let Ok(val) = parts[1].parse::<f64>() {
                                cfg.prayer_timezone = val.clamp(-12.0, 14.0);
                            }
                        }
                        "prayer_method" => {
                            cfg.prayer_method = parts[1].to_string();
                        }
                        "prayer_offset_mins" => {
                            if let Ok(val) = parts[1].parse::<i32>() {
                                cfg.prayer_offset_mins = val.clamp(-30, 30);
                            }
                        }
                        "calendar_url" => {
                            cfg.calendar_url = parts[1].to_string();
                        }
                        _ => {}
                    }
                }
            }
            // Ensure registry matches loaded setting
            set_autostart_enabled(cfg.start_with_windows);
            cfg
        } else {
            let def = Self::default();
            def.save();
            def
        }
    }

    pub fn save(&self) {
        set_autostart_enabled(self.start_with_windows);

        let style_str = match self.island_style {
            IslandStyle::SleekCapsule => "sleek",
            IslandStyle::BalancedPill => "balanced",
            IslandStyle::SpaciousCard => "spacious",
            IslandStyle::CompactMinimal => "compact",
        };

        let view_str = match self.compact_mode_view {
            CompactDisplayMode::Combined => "combined",
            CompactDisplayMode::TitleOnly => "title",
            CompactDisplayMode::WaveOnly => "wave",
        };

        let content = format!(
            "# Dynamic Island Settings\ntop_margin={}\nauto_collapse_secs={}\nisland_style={}\nisland_opacity={:.2}\nglow_intensity={:.2}\nvinyl_speed={:.2}\ncompact_mode_view={}\ncompact_music_width={:.1}\ncompact_lyrics_width={:.1}\ndynamic_album_color={}\nenable_media_island={}\nenable_vinyl_animation={}\nhover_expand_media={}\nidle_click_through={}\nsmart_contour={}\ncontour_nudge_y={:.1}\nstart_with_windows={}\nuse_full_media_card={}\nshow_lyrics_in_capsule={}\nromanize_cjk_lyrics={}\nprayer_city={}\nprayer_latitude={:.4}\nprayer_longitude={:.4}\nprayer_timezone={:.1}\nprayer_method={}\nprayer_offset_mins={}\ncalendar_url={}\n",
            self.top_margin,
            self.auto_collapse_secs,
            style_str,
            self.island_opacity,
            self.glow_intensity,
            self.vinyl_speed,
            view_str,
            self.compact_music_width,
            self.compact_lyrics_width,
            self.dynamic_album_color,
            self.enable_media_island,
            self.enable_vinyl_animation,
            self.hover_expand_media,
            self.idle_click_through,
            self.smart_contour,
            self.contour_nudge_y,
            self.start_with_windows,
            self.use_full_media_card,
            self.show_lyrics_in_capsule,
            self.romanize_cjk_lyrics,
            self.prayer_city,
            self.prayer_latitude,
            self.prayer_longitude,
            self.prayer_timezone,
            self.prayer_method,
            self.prayer_offset_mins,
            self.calendar_url
        );
        let path = get_config_path();
        let _ = fs::write(&path, &content);

        // Also sync to parent repo directory if in development (e.g. target/release -> root)
        if let Ok(mut exe_path) = std::env::current_exe() {
            exe_path.pop();
            let repo_cfg = exe_path.join("..").join("..").join("config.txt");
            if repo_cfg.exists() && repo_cfg != path {
                let _ = fs::write(repo_cfg, &content);
            }
        }
    }
}
