#![allow(dead_code)]

use crate::animation::Spring;

#[derive(Debug, Clone)]
pub struct Notification {
    pub app_name: String,
    pub title: String,
    pub body: String,
    pub time_str: String,
    pub badge_letter: String,
    pub badge_color: (f32, f32, f32), // RGB [0.0, 1.0]
    pub icon_bytes: Option<Vec<u8>>,
    pub aumid: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub title: String,
    pub artist: String,
    pub is_playing: bool,
    pub thumbnail_bytes: Option<Vec<u8>>,
    pub source: String,
    pub position_secs: f32,
    pub duration_secs: f32,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum IslandMode {
    Idle,
    Slim,      // Stage 1: Ultra-slim horizontal strip (height 30px, wide horizontal)
    Expanded,  // Stage 2: Sleek capsule on hover (height 50-74px)
    FullCard,  // Stage 3: Full Media & Shelf Carousel Card (height 140px, width 540px)
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum ShelfCard {
    Today = 0,
    TodoList = 1,
    MusicLyrics = 2,
    PrayerTimes = 3,
    WaterReminder = 4,
}

pub const SHELF_CARD_COUNT: usize = 5;

impl ShelfCard {
    pub fn from_index(idx: usize) -> Self {
        match idx % SHELF_CARD_COUNT {
            0 => ShelfCard::Today,
            1 => ShelfCard::TodoList,
            2 => ShelfCard::MusicLyrics,
            3 => ShelfCard::PrayerTimes,
            _ => ShelfCard::WaterReminder,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            ShelfCard::Today => "Today",
            ShelfCard::TodoList => "TodoList",
            ShelfCard::MusicLyrics => "Music",
            ShelfCard::PrayerTimes => "Prayer Times",
            ShelfCard::WaterReminder => "Hydration",
        }
    }
}

pub struct Island {
    pub mode: IslandMode,
    pub width_spring: Spring,
    pub height_spring: Spring,
    pub radius_spring: Spring,
    pub alpha_spring: Spring,
    pub y_spring: Spring,
    pub active_notification: Option<Notification>,
    pub current_track: Option<TrackInfo>,
    pub vinyl_rotation: f32,
    pub marquee_offset: f32,
    pub remaining_time: f32,
    pub is_hovered: bool,
    pub is_media_playing: bool,
    pub audio_phase: f32,
    pub audio_peak: f32,
    pub window_width: f32,
    pub window_height: f32,
    pub top_margin: f32,
    pub vinyl_speed: f32,
    pub compact_mode_view: crate::config::CompactDisplayMode,
    pub compact_music_width: f32,
    pub compact_lyrics_width: f32,
    pub show_lyrics_in_capsule: bool,
    pub island_style: crate::config::IslandStyle,
    pub volume: f32,
    pub use_full_media_card: bool,
    pub hover_leave_timer: f32,
    pub current_card: usize,
    pub card_scroll_spring: Spring,
    pub shelf_expand: Spring,
    pub show_lyrics: bool,
    pub romanize_cjk_lyrics: bool,
    pub current_lyrics: Option<crate::lyrics::SongLyrics>,
    pub lyrics_loading: bool,
    pub battery: crate::battery::BatteryStatus,
    pub battery_timer: f32,
    pub hydration: crate::hydration::HydrationTracker,
}

pub const IDLE_WIDTH: f32 = 148.0;
pub const IDLE_HEIGHT: f32 = 28.0;
pub const IDLE_RADIUS: f32 = 14.0;

pub const SLIM_HEIGHT: f32 = 30.0;
pub const SLIM_RADIUS: f32 = 15.0;

pub const FULL_CARD_WIDTH: f32 = 560.0;
pub const FULL_CARD_HEIGHT: f32 = 148.0;
pub const FULL_CARD_RADIUS: f32 = 26.0;

impl Island {
    pub fn new(window_width: f32, window_height: f32) -> Self {
        Self {
            mode: IslandMode::Idle,
            // Snappy spring constants calibrated for iOS Dynamic Island feel
            width_spring: Spring::new(IDLE_WIDTH, 290.0, 26.0),
            height_spring: Spring::new(IDLE_HEIGHT, 260.0, 24.0),
            radius_spring: Spring::new(IDLE_RADIUS, 260.0, 24.0),
            alpha_spring: Spring::new(0.0, 260.0, 24.0),
            y_spring: Spring::new(0.0, 260.0, 22.0),
            active_notification: None,
            current_track: None,
            vinyl_rotation: 0.0,
            marquee_offset: 0.0,
            remaining_time: 0.0,
            is_hovered: false,
            is_media_playing: false,
            audio_phase: 0.0,
            audio_peak: 0.0,
            window_width,
            window_height,
            top_margin: 0.0,
            vinyl_speed: 1.0,
            compact_mode_view: crate::config::CompactDisplayMode::Combined,
            compact_music_width: 240.0,
            compact_lyrics_width: 310.0,
            show_lyrics_in_capsule: true,
            island_style: crate::config::IslandStyle::SleekCapsule,
            volume: 0.5,
            use_full_media_card: false,
            hover_leave_timer: 0.0,
            current_card: 2, // Default to MusicLyrics card
            card_scroll_spring: Spring::new(2.0, 270.0, 25.0),
            shelf_expand: Spring::new(0.0, 260.0, 23.0),
            show_lyrics: false,
            romanize_cjk_lyrics: false,
            current_lyrics: None,
            lyrics_loading: false,
            battery: crate::battery::BatteryStatus::query(),
            battery_timer: 0.0,
            hydration: crate::hydration::HydrationTracker::load(),
        }
    }

    pub fn has_active_lyrics(&self) -> bool {
        if !self.is_media_playing {
            return false;
        }
        if let Some(ref lyrics) = self.current_lyrics {
            if lyrics.lines.is_empty() || lyrics.is_instrumental {
                return false;
            }
            let pos = self.current_track.as_ref().map(|t| t.position_secs).unwrap_or(0.0);
            let info = lyrics.get_active_line_info(pos, self.romanize_cjk_lyrics);
            !info.curr.is_empty() && info.curr != "•••" && info.curr != "Instrumental Break" && info.curr != "No lyrics available"
        } else {
            false
        }
    }

    pub fn target_idle_width(&self) -> f32 {
        if self.is_media_playing {
            let base_w = if self.show_lyrics_in_capsule && self.has_active_lyrics() {
                self.compact_lyrics_width
            } else {
                self.compact_music_width
            };

            match self.compact_mode_view {
                crate::config::CompactDisplayMode::Combined => base_w,
                crate::config::CompactDisplayMode::TitleOnly => (base_w - 55.0).max(150.0),
                crate::config::CompactDisplayMode::WaveOnly => 135.0,
            }
        } else {
            IDLE_WIDTH
        }
    }

    pub fn show_notification(&mut self, notif: Notification) {
        self.active_notification = Some(notif);
        self.mode = IslandMode::Slim;
        self.remaining_time = 5.0; // Display as sleek slim bar for 5.0 seconds

        let (target_w, _, _) = self.island_style.dimensions();
        self.width_spring.set_target(target_w);
        self.height_spring.set_target(SLIM_HEIGHT);
        self.radius_spring.set_target(SLIM_RADIUS);
        self.alpha_spring.set_target(1.0);
    }

    pub fn expand_full(&mut self) {
        if self.active_notification.is_none() && self.use_full_media_card && (self.is_media_playing || self.current_track.is_some()) {
            self.expand_full_card();
        } else {
            self.expand_capsule();
        }
    }

    pub fn expand_capsule(&mut self) {
        self.mode = IslandMode::Expanded;
        self.remaining_time = 5.0;
        self.hover_leave_timer = 0.0;

        let (target_w, target_h, target_r) = self.island_style.dimensions();
        self.width_spring.set_target(target_w);
        self.height_spring.set_target(target_h);
        self.radius_spring.set_target(target_r);
        self.alpha_spring.set_target(1.0);
    }

    pub fn expand_full_card(&mut self) {
        self.mode = IslandMode::FullCard;
        self.remaining_time = 8.0;
        self.hover_leave_timer = 0.0;
        self.shelf_expand.set_target(1.0);
        self.width_spring.set_target(FULL_CARD_WIDTH);
        self.height_spring.set_target(FULL_CARD_HEIGHT);
        self.radius_spring.set_target(FULL_CARD_RADIUS);
        self.alpha_spring.set_target(1.0);
    }

    pub fn next_card(&mut self) {
        self.current_card = (self.current_card + 1) % SHELF_CARD_COUNT;
        self.card_scroll_spring.set_target(self.current_card as f32);
        self.remaining_time = 6.0;
        self.hover_leave_timer = 0.0;
    }

    pub fn prev_card(&mut self) {
        self.current_card = if self.current_card == 0 {
            SHELF_CARD_COUNT - 1
        } else {
            self.current_card - 1
        };
        self.card_scroll_spring.set_target(self.current_card as f32);
        self.remaining_time = 6.0;
        self.hover_leave_timer = 0.0;
    }

    pub fn select_card(&mut self, idx: usize) {
        self.current_card = idx.min(SHELF_CARD_COUNT - 1);
        self.card_scroll_spring.set_target(self.current_card as f32);
        self.remaining_time = 6.0;
        self.hover_leave_timer = 0.0;
    }

    pub fn toggle_full_card(&mut self) {
        if self.mode == IslandMode::FullCard {
            self.dismiss();
        } else {
            self.expand_full_card();
        }
    }

    pub fn expand_media(&mut self) {
        self.expand_full();
    }

    pub fn dismiss(&mut self) {
        self.mode = IslandMode::Idle;
        self.active_notification = None;
        self.remaining_time = 0.0;
        self.hover_leave_timer = 0.0;
        self.shelf_expand.set_target(0.0);

        let target_idle_w = self.target_idle_width();
        self.width_spring.set_target(target_idle_w);
        self.height_spring.set_target(IDLE_HEIGHT);
        self.radius_spring.set_target(IDLE_RADIUS);
        self.alpha_spring.set_target(1.0);
    }

    /// Advances simulation. Returns true if island is actively animating or timer is running.
    pub fn update(&mut self, dt: f32) -> bool {
        if self.mode == IslandMode::Idle {
            let target_idle_w = self.target_idle_width();
            if (self.width_spring.target - target_idle_w).abs() > 1.0 {
                self.width_spring.set_target(target_idle_w);
            }
        }

        let moving_w = self.width_spring.update(dt);
        let moving_h = self.height_spring.update(dt);
        let moving_r = self.radius_spring.update(dt);
        let moving_a = self.alpha_spring.update(dt);
        let moving_y = self.y_spring.update(dt);
        let moving_card = self.card_scroll_spring.update(dt);
        let moving_shelf = self.shelf_expand.update(dt);

        self.battery_timer += dt;
        if self.battery_timer >= 3.0 {
            self.battery_timer = 0.0;
            self.battery = crate::battery::BatteryStatus::query();
        }

        // Advance vinyl disc spin rotation & marquee running title when music is actively playing
        let is_really_playing = self.current_track.as_ref().map(|t| t.is_playing).unwrap_or(self.is_media_playing);
        if is_really_playing {
            self.vinyl_rotation += dt * 1.2 * self.vinyl_speed;
            if self.vinyl_rotation > std::f32::consts::PI * 2.0 {
                self.vinyl_rotation -= std::f32::consts::PI * 2.0;
            }
            self.marquee_offset += dt * 38.0;

            if let Some(ref mut track) = self.current_track {
                track.position_secs += dt;
                if track.duration_secs > 0.0 && track.position_secs > track.duration_secs {
                    track.position_secs = track.duration_secs;
                }
            }
        }

        // Graceful auto-collapse when cursor leaves hover with hysteresis delay
        if self.is_hovered {
            self.hover_leave_timer = 0.0;
        } else {
            self.hover_leave_timer += dt;

            // For Expanded capsule: 0.35s delay (prevents flicker if cursor moves past border)
            // For FullCard view: 0.8s delay (comfortable interaction without getting stuck)
            let max_leave = if self.mode == IslandMode::FullCard { 0.8 } else { 0.35 };

            if self.mode == IslandMode::Expanded || self.mode == IslandMode::FullCard {
                if self.hover_leave_timer >= max_leave {
                    self.dismiss();
                }
            }

            if self.mode == IslandMode::Slim && self.remaining_time > 0.0 {
                self.remaining_time -= dt;
                if self.remaining_time <= 0.0 {
                    self.dismiss();
                    return true;
                }
            }
        }

        // Clean up notification reference once in idle mode
        if self.mode == IslandMode::Idle && self.active_notification.is_some() {
            self.active_notification = None;
        }

        if self.is_media_playing {
            self.audio_phase += dt * 7.5;
            if self.audio_phase > 10000.0 {
                self.audio_phase -= 10000.0;
            }
        }

        moving_w || moving_h || moving_r || moving_a || moving_y || moving_card || moving_shelf || self.mode != IslandMode::Idle || self.is_media_playing
    }

    /// Returns the current bounding rectangle of the island pill inside window coordinates.
    pub fn get_pill_rect(&self) -> (f32, f32, f32, f32) {
        let cur_w = self.width_spring.current;
        let cur_h = self.height_spring.current;
        let left = (self.window_width - cur_w) * 0.5;
        let top = self.y_spring.current;
        let right = left + cur_w;
        let bottom = top + cur_h;
        (left, top, right, bottom)
    }

    /// Hit-test: checks if screen coordinates (screen_x, screen_y) fall inside the island pill.
    pub fn hit_test(&self, screen_x: i32, screen_y: i32, win_x: i32, win_y: i32) -> bool {
        let local_x = (screen_x - win_x) as f32;
        let local_y = (screen_y - win_y) as f32;

        let (left, top, right, bottom) = self.get_pill_rect();
        // Generous 10px buffer around borders ensures hover isn't lost during fast animations or near rounded corners
        let pad = 10.0;

        local_x >= (left - pad)
            && local_x <= (right + pad)
            && local_y >= (top - pad)
            && local_y <= (bottom + pad)
    }
}
