use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HydrationData {
    pub current_ml: u32,
    pub target_ml: u32,
    pub interval_mins: u32,
    pub reminder_enabled: bool,
    pub last_date: String,
}

impl Default for HydrationData {
    fn default() -> Self {
        Self {
            current_ml: 900,
            target_ml: 1500,
            interval_mins: 45,
            reminder_enabled: true,
            last_date: Self::get_today_string(),
        }
    }
}

impl HydrationData {
    pub fn get_today_string() -> String {
        unsafe {
            use windows::Win32::System::SystemInformation::GetLocalTime;
            let st = GetLocalTime();
            format!("{:04}-{:02}-{:02}", st.wYear, st.wMonth, st.wDay)
        }
    }
}

#[derive(Debug)]
pub struct HydrationTracker {
    pub data: HydrationData,
    #[allow(dead_code)]
    pub last_drink_time: Instant,
    pub next_reminder_secs: f32,
    pub trigger_alert: bool,
}

impl HydrationTracker {
    pub fn load() -> Self {
        let mut data = Self::read_data().unwrap_or_default();
        let today = HydrationData::get_today_string();
        if data.last_date != today {
            // New day detected: reset daily intake!
            data.current_ml = 0;
            data.last_date = today;
            let _ = Self::write_data(&data);
        }

        let interval_secs = (data.interval_mins as f32) * 60.0;
        Self {
            data,
            last_drink_time: Instant::now(),
            next_reminder_secs: interval_secs,
            trigger_alert: false,
        }
    }

    pub fn drink(&mut self, amount_ml: u32) {
        self.check_date_reset();
        self.data.current_ml = (self.data.current_ml + amount_ml).min(10000);
        self.last_drink_time = Instant::now();
        self.next_reminder_secs = (self.data.interval_mins as f32) * 60.0;
        self.trigger_alert = false;
        let _ = Self::write_data(&self.data);
    }

    pub fn cycle_target(&mut self) {
        // Cycles target: 1500 -> 2000 -> 2500 -> 3000 -> 1000 -> 1500
        self.data.target_ml = match self.data.target_ml {
            1000 => 1500,
            1500 => 2000,
            2000 => 2500,
            2500 => 3000,
            _ => 1500,
        };
        let _ = Self::write_data(&self.data);
    }

    #[allow(dead_code)]
    pub fn toggle_reminder(&mut self) {
        self.data.reminder_enabled = !self.data.reminder_enabled;
        let _ = Self::write_data(&self.data);
    }

    pub fn update(&mut self, dt: f32) {
        self.check_date_reset();
        if self.data.reminder_enabled {
            self.next_reminder_secs -= dt;
            if self.next_reminder_secs <= 0.0 {
                self.trigger_alert = true;
                // Re-arm for another interval
                self.next_reminder_secs = (self.data.interval_mins as f32) * 60.0;
            }
        }
    }

    pub fn check_date_reset(&mut self) {
        let today = HydrationData::get_today_string();
        if self.data.last_date != today {
            self.data.current_ml = 0;
            self.data.last_date = today;
            let _ = Self::write_data(&self.data);
        }
    }

    pub fn get_percentage(&self) -> f32 {
        if self.data.target_ml == 0 {
            return 1.0;
        }
        (self.data.current_ml as f32 / self.data.target_ml as f32).clamp(0.0, 1.0)
    }

    pub fn get_next_reminder_mins(&self) -> u32 {
        ((self.next_reminder_secs / 60.0).max(0.0).ceil() as u32).max(1)
    }

    fn get_file_path() -> PathBuf {
        if let Ok(mut p) = std::env::current_exe() {
            p.pop();
            let exe_json = p.join("hydration.json");
            if exe_json.exists() {
                return exe_json;
            }
            let repo_json = p.join("..").join("..").join("hydration.json");
            if repo_json.exists() {
                if let Ok(c) = repo_json.canonicalize() {
                    return c;
                }
            }
            return exe_json;
        }
        PathBuf::from("hydration.json")
    }

    fn read_data() -> Option<HydrationData> {
        let path = Self::get_file_path();
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn write_data(data: &HydrationData) -> Result<(), std::io::Error> {
        let path = Self::get_file_path();
        let json = serde_json::to_string_pretty(data)?;
        fs::write(path, json)
    }
}
