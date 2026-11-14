use std::fs;
use std::path::Path;

use windows::{
    core::*,
    Win32::System::Registry::*,
};

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
    pub dynamic_album_color: bool,
    pub enable_media_island: bool,
    pub enable_vinyl_animation: bool,
    pub hover_expand_media: bool,
    pub idle_click_through: bool,
    pub start_with_windows: bool,
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
            dynamic_album_color: true,
            enable_media_island: true,
            enable_vinyl_animation: true,
            hover_expand_media: true,
            idle_click_through: true,
            start_with_windows: is_autostart_enabled(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let path = Path::new("config.txt");
        if let Ok(content) = fs::read_to_string(path) {
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
                        "start_with_windows" => {
                            cfg.start_with_windows = parts[1] == "true" || parts[1] == "1";
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
            "# Dynamic Island Settings\ntop_margin={}\nauto_collapse_secs={}\nisland_style={}\nisland_opacity={:.2}\nglow_intensity={:.2}\nvinyl_speed={:.2}\ncompact_mode_view={}\ndynamic_album_color={}\nenable_media_island={}\nenable_vinyl_animation={}\nhover_expand_media={}\nidle_click_through={}\nstart_with_windows={}\n",
            self.top_margin,
            self.auto_collapse_secs,
            style_str,
            self.island_opacity,
            self.glow_intensity,
            self.vinyl_speed,
            view_str,
            self.dynamic_album_color,
            self.enable_media_island,
            self.enable_vinyl_animation,
            self.hover_expand_media,
            self.idle_click_through,
            self.start_with_windows
        );
        let _ = fs::write("config.txt", content);
    }
}
