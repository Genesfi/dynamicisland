#![allow(unsafe_op_in_unsafe_fn)]
#![windows_subsystem = "windows"]

mod animation;
mod audio_meter;
mod battery;
mod calendar;
mod config;
mod hydration;
mod icons;
mod island;
mod lyrics;
mod media;
mod notifications;
mod prayer;
mod renderer;
mod settings_window;
mod tasks;
mod transliterate;
mod tray;

use config::AppConfig;
use island::{Island, Notification};
use media::{start_media_listener, MediaEvent};
use notifications::{check_listener_status, start_notification_listener};
use renderer::Renderer;
use tray::{
    add_tray_icon, remove_tray_icon, show_tray_menu, ID_TRAY_EXIT, ID_TRAY_SETTINGS,
    WM_CONFIG_CHANGED, WM_TRAY_MSG,
};

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::{CreateRectRgn, CreateRoundRectRgn, SetWindowRgn, HBRUSH},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
            Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, SetCapture, ReleaseCapture},
            WindowsAndMessaging::*,
        },
    },
};

const WINDOW_WIDTH: u32 = 720;
const WINDOW_HEIGHT: u32 = 180;
const TIMER_ID: usize = 1001;
const TIMER_INTERVAL_MS: u32 = 16; // ~60 FPS smooth animation tick
const WM_MOUSELEAVE_MSG: u32 = 0x02A3;

/// Trims the process working set by flushing unreferenced DLL initialization pages.
/// Drops Task Manager reported physical RAM down to minimum (~10-15 MB) without performance penalty.
#[inline]
pub fn trim_working_set() {
    unsafe {
        use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

struct AppState {
    island: Island,
    renderer: Renderer,
    config: AppConfig,
    audio_meter: audio_meter::AudioMeter,
    last_tick: Instant,
    smoothed_dt: f32,
    is_tracking_mouse: bool,
    hover_dwell_time: f32,
    idle_settled_timer: f32,
    last_rgn_rect: (i32, i32, i32, i32),
    notif_receiver: Receiver<Notification>,
    media_sender: std::sync::mpsc::Sender<MediaEvent>,
    media_receiver: Receiver<MediaEvent>,
    dragging_seek: bool,
    dragging_volume: bool,
    seek_grace_until: Option<Instant>,
    media_pause_until: Option<Instant>,
    lyrics_retry_count: u32,
    lyrics_retry_at: Option<Instant>,
    gps_sender: std::sync::mpsc::Sender<std::result::Result<(f64, f64, f64, String), String>>,
    gps_receiver: Receiver<std::result::Result<(f64, f64, f64, String), String>>,
}

/// Dynamically syncs the OS window clipping region (GDI HRGN) to the exact pill capsule geometry.
/// Any pixels outside this region physically DO NOT EXIST to the Windows Window Manager,
/// allowing 100% unimpeded clicks directly through to underlying browser tabs, search bars, and breadcrumbs!
fn sync_window_region(hwnd: HWND, state: &mut AppState) {
    let (left, top, right, bottom) = state.island.get_pill_rect();
    let cur_r = state.island.radius_spring.current;

    // Check if in pure idle mode with click-through enabled
    if state.config.idle_click_through
        && state.island.mode == island::IslandMode::Idle
        && !state.island.is_media_playing
        && state.island.current_track.is_none()
    {
        let cur = (0, 0, 0, 0);
        if state.last_rgn_rect != cur {
            state.last_rgn_rect = cur;
            unsafe {
                let empty_rgn = CreateRectRgn(0, 0, 0, 0);
                let _ = SetWindowRgn(hwnd, Some(empty_rgn), false);
            }
        }
        return;
    }

    // Dynamic outer margin: when glow is active, provide generous 16px padding so Direct2D
    // soft ambient glow is completely unclipped and buttery smooth!
    let pad = if state.config.glow_intensity > 0.01 { 16.0f32 } else { 3.0f32 };
    let l = (left - pad).round().max(0.0) as i32;
    let t = (top - pad).round().max(0.0) as i32;
    let r = (right + pad).round().min(WINDOW_WIDTH as f32) as i32;
    let b = (bottom + pad).round().min(WINDOW_HEIGHT as f32) as i32;
    let cur = (l, t, r, b);

    if state.last_rgn_rect != cur {
        state.last_rgn_rect = cur;
        unsafe {
            let diam = (((cur_r + pad) * 2.0).round() as i32).max(1);
            let rgn = CreateRoundRectRgn(l, t, r, b, diam, diam);
            let _ = SetWindowRgn(hwnd, Some(rgn), false);
        }
    }
}

/// Detects if the active foreground window is in Fullscreen mode (Netflix, YouTube Fullscreen, Video Players, Games).
fn is_foreground_fullscreen(fg: HWND, screen_w: i32, screen_h: i32) -> bool {
    if fg.0.is_null() {
        return false;
    }
    unsafe {
        let shell_wnd = GetShellWindow();
        if fg == shell_wnd {
            return false;
        }

        let mut rc = RECT::default();
        if GetWindowRect(fg, &mut rc).is_err() {
            return false;
        }

        // Fullscreen windows cover the full primary monitor resolution (with 2px tolerance for DPI margins)
        let covers_screen = rc.left <= 2 && rc.top <= 2 && rc.right >= (screen_w - 2) && rc.bottom >= (screen_h - 2);
        if !covers_screen {
            return false;
        }

        let mut class_buf = [0u16; 64];
        let len = GetClassNameW(fg, &mut class_buf) as usize;
        let class = String::from_utf16_lossy(&class_buf[..len]);
        if class == "Progman" || class == "WorkerW" || class == "Shell_TrayWnd" {
            return false;
        }

        let mut work_area = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work_area as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );

        // When a window extends beyond the Windows work area down to screen_h (covers the taskbar completely),
        // it is a true Fullscreen player (Netflix, YouTube fullscreen, video playback, game)
        if rc.bottom > work_area.bottom {
            return true;
        }

        let style = GetWindowLongW(fg, GWL_STYLE) as u32;
        let has_caption = (style & WS_CAPTION.0) == WS_CAPTION.0;
        !has_caption
    }
}

/// Automatically nudges the island down when active foreground app is a browser with top tabs,
/// or when FocusGrow Health HUD (F:\Android Project\focusgrow-rs) is actively showing.
fn update_smart_contour(state: &mut AppState, fg: HWND, hwnd: HWND) {
    if !state.config.smart_contour {
        state.island.y_spring.set_target(state.config.top_margin);
        return;
    }

    unsafe {
        // 1. FocusGrow Health HUD detection:
        // FocusGrow (F:\Android Project\focusgrow-rs) creates a top-center posture HUD at Y = 12px with HWND_TOPMOST.
        // If FocusGrow HUD is currently visible, smoothly nudge Dynamic Island down to Y = 54px so it rests cleanly underneath!
        let fg_hud = FindWindowW(w!("FocusGrowHealthHud"), None).ok();
        if let Some(hud) = fg_hud {
            if !hud.0.is_null() && IsWindowVisible(hud).as_bool() {
                state.island.y_spring.set_target(54.0 + state.config.top_margin);
                return;
            }
        }
    }

    if fg.0.is_null() || fg == hwnd {
        return;
    }

    unsafe {
        let mut rc = RECT::default();
        let _ = GetWindowRect(fg, &mut rc);

        // Check if the foreground window is docked or maximized at the top
        let is_at_top = rc.top <= 24 && rc.bottom >= 400 && (rc.right - rc.left) >= 500;

        if is_at_top {
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            if pid != 0 {
                let exe = crate::media::get_process_name(pid).to_lowercase();

                // Strictly target web browsers (Chrome, Edge, Brave, Firefox, Opera, Vivaldi)
                // that always feature a top tab strip when browsing!
                let is_browser = exe.contains("chrome")
                    || exe.contains("msedge")
                    || exe.contains("brave")
                    || exe.contains("firefox")
                    || exe.contains("opera")
                    || exe.contains("vivaldi");

                if is_browser {
                    // Smoothly nudge down to contour_nudge_y (default 34px, just below tab strip, well above bookmarks)
                    state.island.y_spring.set_target(state.config.contour_nudge_y + state.config.top_margin);
                    return;
                }
            }
        }
    }

    // For desktop, File Explorer, settings, or floating windows: stay flush at top!
    state.island.y_spring.set_target(state.config.top_margin);
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match msg {
            WM_NCCREATE => DefWindowProcW(hwnd, msg, wparam, lparam),
            WM_NCHITTEST => {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    let state = &*state_ptr;
                    let screen_x = (lparam.0 & 0xFFFF) as i16 as i32;
                    let screen_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;

                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);

                    // Strictly test whether cursor falls inside the visible capsule pill
                    let is_hit = state.island.hit_test(screen_x, screen_y, rect.left, rect.top);
                    if !is_hit {
                        // Any point outside the island pill is 100% transparent and clicks pass through!
                        return LRESULT(HTTRANSPARENT as isize);
                    }

                    // Inside the pill:
                    if state.island.mode == island::IslandMode::Idle {
                        if state.island.is_media_playing || state.island.current_track.is_some() {
                            // Active media pill: allow clicking to open/focus music player (YouTube Music, Spotify)!
                            return LRESULT(HTCLIENT as isize);
                        } else if state.config.idle_click_through {
                            // Pure idle clock notch with click-through enabled: pass clicks to browser tabs underneath
                            return LRESULT(HTTRANSPARENT as isize);
                        }
                    }

                    return LRESULT(HTCLIENT as isize);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_LBUTTONDOWN => {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    let mouse_x = (lparam.0 & 0xFFFF) as i16 as f32;
                    let mouse_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

                    let (left, top, right, bottom) = state.island.get_pill_rect();
                    let cy = (top + bottom) * 0.5;

                    if state.island.mode == island::IslandMode::FullCard {
                        let cx = (left + right) * 0.5;

                        // 1. Toggle view chevron at top right
                        let chevron_x = right - 22.0;
                        let chevron_y = top + 18.0;
                        let dist_chevron = ((mouse_x - chevron_x).powi(2) + (mouse_y - chevron_y).powi(2)).sqrt();
                        if dist_chevron <= 22.0 || (mouse_x >= right - 38.0 && mouse_y <= top + 34.0) {
                            state.island.toggle_full_card();
                            state.island.remaining_time = 4.0;
                            return LRESULT(0);
                        }

                        // 2. Navigation Chevrons [ < ] on Left Flank and [ > ] on Right Flank
                        let chev_y = (top + bottom) * 0.5;
                        let left_chev_x = left + 18.0;
                        let dist_l = ((mouse_x - left_chev_x).powi(2) + (mouse_y - chev_y).powi(2)).sqrt();
                        if dist_l <= 20.0 || (mouse_x >= left && mouse_x <= left + 36.0 && mouse_y >= chev_y - 24.0 && mouse_y <= chev_y + 24.0) {
                            state.island.prev_card();
                            state.island.remaining_time = 8.0;
                            return LRESULT(0);
                        }

                        let right_chev_x = right - 18.0;
                        let dist_r = ((mouse_x - right_chev_x).powi(2) + (mouse_y - chev_y).powi(2)).sqrt();
                        if dist_r <= 20.0 || (mouse_x >= right - 36.0 && mouse_x <= right && mouse_y >= chev_y - 24.0 && mouse_y <= chev_y + 24.0) {
                            state.island.next_card();
                            state.island.remaining_time = 8.0;
                            return LRESULT(0);
                        }

                        // 3. Bottom Pagination Dots:
                        if mouse_y >= bottom - 22.0 && mouse_y <= bottom {
                            let count = crate::island::SHELF_CARD_COUNT;
                            let spacing = 13.0;
                            let total_w = (count as f32 - 1.0) * spacing;
                            let start_x = cx - total_w * 0.5;
                            for i in 0..count {
                                let item_cx = start_x + (i as f32) * spacing;
                                if (mouse_x - item_cx).abs() <= 8.0 {
                                    state.island.select_card(i);
                                    return LRESULT(0);
                                }
                            }
                        }

                        // 4. Card-specific hit testing
                        if state.island.current_card == 0 {
                            // Card 0: Today Dashboard
                            let top_c = top + 24.0;
                            let col_r = cx + 12.0;
                            let right_c = right - 44.0;

                            // If there is a soon meeting with a link, check [Join] button click!
                            if let Some(meeting) = &state.island.calendar.soon_meeting {
                                if let Some(url) = &meeting.link {
                                    if mouse_x >= right_c - 44.0 && mouse_x <= right_c - 4.0 && mouse_y >= top_c + 54.0 && mouse_y <= top_c + 80.0 {
                                        let wide_url: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
                                        let wide_open: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
                                        unsafe {
                                            windows::Win32::UI::Shell::ShellExecuteW(
                                                None,
                                                windows::core::PCWSTR(wide_open.as_ptr()),
                                                windows::core::PCWSTR(wide_url.as_ptr()),
                                                None,
                                                None,
                                                windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                                            );
                                        }
                                        return LRESULT(0);
                                    }
                                }
                            }

                            // Clicking anywhere on right column (Mini Calendar & Task Status box)
                            // jumps directly to Card 1 (TodoList)!
                            if mouse_x >= col_r && mouse_x <= right_c && mouse_y >= top_c && mouse_y <= top_c + 90.0 {
                                state.island.current_card = 1;
                                state.island.card_scroll_spring.set_target(1.0);
                                state.island.remaining_time = 8.0;
                                return LRESULT(0);
                            }
                        } else if state.island.current_card == 1 {
                            // Card 1: TodoList & Weekly Planner
                            let top_c = top + 26.0;
                            let col_l = left + 44.0;
                            let right_c = right - 44.0;
                            let mid_left = col_l + 152.0;
                            let mid_w = 142.0;

                            // 1. [+ Add Task] Button: col_l .. col_l + 68.0, top_c + 54.0 .. top_c + 76.0
                            if mouse_x >= col_l && mouse_x <= col_l + 68.0 && mouse_y >= top_c + 54.0 && mouse_y <= top_c + 76.0 {
                                state.island.entering_task = Some(String::new());
                                state.island.remaining_time = 15.0;
                                return LRESULT(0);
                            }

                            // 2. [View Toggle] Button: col_l + 74.0 .. col_l + 142.0, top_c + 54.0 .. top_c + 76.0
                            if mouse_x >= col_l + 74.0 && mouse_x <= col_l + 142.0 && mouse_y >= top_c + 54.0 && mouse_y <= top_c + 76.0 {
                                state.island.tasks.toggle_view();
                                state.island.remaining_time = 8.0;
                                return LRESULT(0);
                            }

                            // 3. Task Rows (Row 0, 1, 2) in Middle Column
                            if mouse_x >= mid_left && mouse_x <= mid_left + mid_w {
                                for idx in 0..3 {
                                    let row_top = top_c + 3.0 + (idx as f32) * 26.5;
                                    if mouse_y >= row_top && mouse_y <= row_top + 25.0 {
                                        // If clicked near far right (delete 'X' icon: mid_left + mid_w - 20.0 .. mid_left + mid_w)
                                        if mouse_x >= mid_left + mid_w - 20.0 {
                                            state.island.tasks.delete_task(idx);
                                        } else {
                                            // Clicked checkbox or text: toggle completion!
                                            state.island.tasks.toggle_task(idx);
                                        }
                                        state.island.remaining_time = 8.0;
                                        return LRESULT(0);
                                    }
                                }
                            }

                            // 4. Month Chevrons (< and >) in Right Column
                            let chev_y = top_c + 7.5;
                            if mouse_y >= chev_y - 8.0 && mouse_y <= chev_y + 8.0 {
                                // Prev month '<'
                                if mouse_x >= right_c - 24.0 && mouse_x <= right_c - 14.0 {
                                    state.island.tasks.cycle_month(false);
                                    state.island.remaining_time = 8.0;
                                    return LRESULT(0);
                                }
                                // Next month '>'
                                if mouse_x >= right_c - 13.0 && mouse_x <= right_c - 3.0 {
                                    state.island.tasks.cycle_month(true);
                                    state.island.remaining_time = 8.0;
                                    return LRESULT(0);
                                }
                            }
                        } else if state.island.current_card == 2 {
                            let top_c = top + 26.0;
                            let col_l = left + 44.0;
                            let right_c = right - 44.0;

                            if !state.island.show_lyrics {
                                // Full-Width Player Mode
                                let seek_y = top_c + 49.0;
                                let seek_left = col_l + 42.0;
                                let seek_right = right_c - 46.0;

                                // Seek bar hit test
                                if mouse_y >= seek_y - 10.0 && mouse_y <= seek_y + 10.0 && mouse_x >= seek_left && mouse_x <= seek_right {
                                    state.dragging_seek = true;
                                    let ratio = ((mouse_x - seek_left) / (seek_right - seek_left)).clamp(0.0, 1.0);
                                    if let Some(ref mut track) = state.island.current_track {
                                        let target = ratio * track.duration_secs;
                                        track.position_secs = target;
                                    }
                                    state.island.remaining_time = 8.0;
                                    let _ = SetCapture(hwnd);
                                    return LRESULT(0);
                                }

                                let ctrl_y = top_c + 78.0;

                                // Speech bubble lyrics toggle button [💬] at col_l + 10.0, ctrl_y
                                let dist_speech = ((mouse_x - (col_l + 10.0)).powi(2) + (mouse_y - ctrl_y).powi(2)).sqrt();
                                if dist_speech <= 18.0 || (mouse_x >= col_l - 4.0 && mouse_x <= col_l + 24.0 && mouse_y >= ctrl_y - 14.0 && mouse_y <= ctrl_y + 14.0) {
                                    state.island.show_lyrics = true;
                                    state.island.remaining_time = 8.0;
                                    let needs_fetch = state.island.current_lyrics.as_ref()
                                        .map(|l| l.lines.is_empty() && !l.is_instrumental)
                                        .unwrap_or(true);
                                    if needs_fetch && !state.island.lyrics_loading {
                                        if let Some(ref track) = state.island.current_track {
                                            state.island.lyrics_loading = true;
                                            state.lyrics_retry_count = 0;
                                            crate::lyrics::fetch_lyrics_async(
                                                track.title.clone(),
                                                track.artist.clone(),
                                                track.duration_secs,
                                                state.media_sender.clone(),
                                            );
                                        }
                                    }
                                    return LRESULT(0);
                                }

                                // Volume slider on right (right_c - 94.0 .. right_c - 10.0)
                                let vol_left = right_c - 94.0;
                                let vol_right = right_c - 10.0;
                                if mouse_y >= ctrl_y - 12.0 && mouse_y <= ctrl_y + 12.0 && mouse_x >= vol_left && mouse_x <= vol_right {
                                    state.dragging_volume = true;
                                    let ratio = ((mouse_x - vol_left) / (vol_right - vol_left)).clamp(0.0, 1.0);
                                    state.island.volume = ratio;
                                    state.audio_meter.set_volume(ratio);
                                    state.island.remaining_time = 8.0;
                                    let _ = SetCapture(hwnd);
                                    return LRESULT(0);
                                }

                                // Transport buttons in center
                                let prev_cx = cx - 44.0;
                                let play_cx = cx;
                                let next_cx = cx + 44.0;

                                let dist_prev = ((mouse_x - prev_cx).powi(2) + (mouse_y - ctrl_y).powi(2)).sqrt();
                                let dist_play = ((mouse_x - play_cx).powi(2) + (mouse_y - ctrl_y).powi(2)).sqrt();
                                let dist_next = ((mouse_x - next_cx).powi(2) + (mouse_y - ctrl_y).powi(2)).sqrt();

                                if dist_prev <= 18.0 {
                                    crate::media::send_media_command(crate::media::MediaCommand::Previous);
                                    state.island.remaining_time = 6.0;
                                    return LRESULT(0);
                                } else if dist_play <= 20.0 {
                                    crate::media::send_media_command(crate::media::MediaCommand::TogglePlayPause);
                                    state.island.remaining_time = 6.0;
                                    return LRESULT(0);
                                } else if dist_next <= 18.0 {
                                    crate::media::send_media_command(crate::media::MediaCommand::Next);
                                    state.island.remaining_time = 6.0;
                                    return LRESULT(0);
                                }

                                // Click album art / title: focus music player
                                if mouse_x >= col_l && mouse_x <= col_l + 48.0 && mouse_y >= top_c && mouse_y <= top_c + 50.0 {
                                    crate::media::activate_or_toggle_media_player(state.island.current_track.as_ref());
                                    return LRESULT(0);
                                }
                            } else {
                                // Two-Column Lyrics Split Mode
                                let col_r = cx + 12.0;

                                // Seek bar in left column
                                let seek_y = top_c + 46.0;
                                let seek_left = col_l + 38.0;
                                let seek_right = cx - 42.0;
                                if mouse_y >= seek_y - 10.0 && mouse_y <= seek_y + 10.0 && mouse_x >= seek_left && mouse_x <= seek_right {
                                    state.dragging_seek = true;
                                    let ratio = ((mouse_x - seek_left) / (seek_right - seek_left)).clamp(0.0, 1.0);
                                    if let Some(ref mut track) = state.island.current_track {
                                        let target = ratio * track.duration_secs;
                                        track.position_secs = target;
                                    }
                                    state.island.remaining_time = 8.0;
                                    let _ = SetCapture(hwnd);
                                    return LRESULT(0);
                                }

                                // Volume slider on right column
                                let vol_y = top_c + 75.0;
                                let vol_left = col_r + 18.0;
                                let vol_right = right_c - 10.0;
                                if mouse_y >= vol_y - 10.0 && mouse_y <= vol_y + 10.0 && mouse_x >= vol_left && mouse_x <= vol_right {
                                    state.dragging_volume = true;
                                    let ratio = ((mouse_x - vol_left) / (vol_right - vol_left)).clamp(0.0, 1.0);
                                    state.island.volume = ratio;
                                    state.audio_meter.set_volume(ratio);
                                    state.island.remaining_time = 8.0;
                                    let _ = SetCapture(hwnd);
                                    return LRESULT(0);
                                }

                                let ctrl_y = top_c + 75.0;

                                // Speech bubble lyrics toggle button [💬] at col_l + 8.0: toggles back to full-width!
                                let dist_speech = ((mouse_x - (col_l + 8.0)).powi(2) + (mouse_y - ctrl_y).powi(2)).sqrt();
                                if dist_speech <= 18.0 || (mouse_x >= col_l - 4.0 && mouse_x <= col_l + 20.0 && mouse_y >= ctrl_y - 14.0 && mouse_y <= ctrl_y + 14.0) {
                                    state.island.show_lyrics = false;
                                    state.island.remaining_time = 8.0;
                                    return LRESULT(0);
                                }

                                // Transport buttons in left column
                                let btn_cx = (col_l + 24.0 + cx - 10.0) * 0.5;
                                let prev_cx = btn_cx - 28.0;
                                let play_cx = btn_cx;
                                let next_cx = btn_cx + 28.0;

                                let dist_prev = ((mouse_x - prev_cx).powi(2) + (mouse_y - ctrl_y).powi(2)).sqrt();
                                let dist_play = ((mouse_x - play_cx).powi(2) + (mouse_y - ctrl_y).powi(2)).sqrt();
                                let dist_next = ((mouse_x - next_cx).powi(2) + (mouse_y - ctrl_y).powi(2)).sqrt();

                                if dist_prev <= 16.0 {
                                    crate::media::send_media_command(crate::media::MediaCommand::Previous);
                                    state.island.remaining_time = 6.0;
                                    return LRESULT(0);
                                } else if dist_play <= 18.0 {
                                    crate::media::send_media_command(crate::media::MediaCommand::TogglePlayPause);
                                    state.island.remaining_time = 6.0;
                                    return LRESULT(0);
                                } else if dist_next <= 16.0 {
                                    crate::media::send_media_command(crate::media::MediaCommand::Next);
                                    state.island.remaining_time = 6.0;
                                    return LRESULT(0);
                                }

                                // 1. Quick Romaji Toggle Pill [A / あ] (col_r + 80.0 .. col_r + 128.0)
                                if mouse_x >= col_r + 78.0 && mouse_x <= col_r + 130.0 && mouse_y >= top_c - 2.0 && mouse_y <= top_c + 18.0 {
                                    state.island.romanize_cjk_lyrics = !state.island.romanize_cjk_lyrics;
                                    state.config.romanize_cjk_lyrics = state.island.romanize_cjk_lyrics;
                                    state.config.save();
                                    state.island.remaining_time = 6.0;
                                    return LRESULT(0);
                                }

                                // 2. Candidate Switcher / Retry Pill [1/N Alt] or [Retry] (col_r + 134.0 .. col_r + 184.0)
                                if mouse_x >= col_r + 132.0 && mouse_x <= col_r + 186.0 && mouse_y >= top_c - 2.0 && mouse_y <= top_c + 18.0 {
                                    if state.island.lyrics_candidates.len() > 1 {
                                        state.island.cycle_lyrics_candidate();
                                    } else {
                                        state.island.clear_cached_lyrics();
                                        if let Some(ref track) = state.island.current_track {
                                            state.island.lyrics_loading = true;
                                            state.lyrics_retry_count = 0;
                                            crate::lyrics::fetch_lyrics_async(
                                                track.title.clone(),
                                                track.artist.clone(),
                                                track.duration_secs,
                                                state.media_sender.clone(),
                                            );
                                        }
                                    }
                                    state.island.remaining_time = 6.0;
                                    return LRESULT(0);
                                }

                                // Click in right column lyrics area: if lyrics are missing, trigger instant retry!
                                if mouse_x >= cx + 12.0 && mouse_x <= right_c && mouse_y >= top_c + 12.0 && mouse_y <= top_c + 75.0 {
                                    let needs_fetch = state.island.current_lyrics.as_ref()
                                        .map(|l| l.lines.is_empty() && !l.is_instrumental)
                                        .unwrap_or(true);
                                    if needs_fetch && !state.island.lyrics_loading {
                                        if let Some(ref track) = state.island.current_track {
                                            state.island.lyrics_loading = true;
                                            state.lyrics_retry_count = 0;
                                            crate::lyrics::fetch_lyrics_async(
                                                track.title.clone(),
                                                track.artist.clone(),
                                                track.duration_secs,
                                                state.media_sender.clone(),
                                            );
                                        }
                                    }
                                    state.island.remaining_time = 6.0;
                                    return LRESULT(0);
                                }
                            }
                        } else if state.island.current_card == 3 {
                            // Card 3: Prayer Times & Sun Arc
                            let top_c = top + 26.0;
                            let col_l = left + 44.0;
                            let col_r = cx + 12.0;
                            let right_c = right - 44.0;

                            // 1. Click on [⟳ GPS] badge
                            if mouse_x >= cx - 55.0 && mouse_x <= cx - 10.0 && mouse_y >= top_c && mouse_y <= top_c + 20.0 {
                                if !state.island.prayer.is_gps_detecting {
                                    state.island.prayer.is_gps_detecting = true;
                                    crate::prayer::detect_gps_location_async(state.gps_sender.clone());
                                }
                                state.island.remaining_time = 8.0;
                                return LRESULT(0);
                            }

                            // 2. Left Column Click: Cycle preset cities (Jakarta -> Bandung -> Surabaya -> Medan -> ...)
                            if mouse_x >= col_l && mouse_x <= cx - 10.0 && mouse_y >= top_c && mouse_y <= top_c + 86.0 {
                                state.island.prayer.cycle_preset_city();
                                state.config.prayer_city = state.island.prayer.city.clone();
                                state.config.prayer_latitude = state.island.prayer.latitude;
                                state.config.prayer_longitude = state.island.prayer.longitude;
                                state.config.prayer_timezone = state.island.prayer.timezone;
                                state.config.save();
                                state.island.remaining_time = 8.0;
                                return LRESULT(0);
                            }

                            // 3. Right Column Click: Cycle calculation method
                            if mouse_x >= col_r && mouse_x <= right_c && mouse_y >= top_c && mouse_y <= top_c + 86.0 {
                                state.island.prayer.cycle_method();
                                state.config.prayer_method = match state.island.prayer.method {
                                    crate::prayer::CalculationMethod::Kemenag => "kemenag".to_string(),
                                    crate::prayer::CalculationMethod::MWL => "mwl".to_string(),
                                    crate::prayer::CalculationMethod::Egyptian => "egyptian".to_string(),
                                    crate::prayer::CalculationMethod::UmmAlQura => "ummalqura".to_string(),
                                };
                                state.config.save();
                                state.island.remaining_time = 8.0;
                                return LRESULT(0);
                            }
                        } else if state.island.current_card == 4 {
                            // Card 4: Water Reminder / Hydration Tracker
                            let top_c = top + 26.0;
                            let col_l = left + 44.0;
                            let col_r = cx + 12.0;
                            let right_c = right - 44.0;
                            let gauge_cx = (col_r + right_c) * 0.5;

                            // 1. [+250 ml] Button: col_l .. col_l + 78.0, top_c + 56.0 .. top_c + 78.0
                            if mouse_x >= col_l && mouse_x <= col_l + 78.0 && mouse_y >= top_c + 56.0 && mouse_y <= top_c + 78.0 {
                                state.island.hydration.drink(250);
                                state.island.remaining_time = 8.0;
                                return LRESULT(0);
                            }

                            // 2. [Target] Button: col_l + 84.0 .. col_l + 146.0, top_c + 56.0 .. top_c + 78.0
                            if mouse_x >= col_l + 84.0 && mouse_x <= col_l + 146.0 && mouse_y >= top_c + 56.0 && mouse_y <= top_c + 78.0 {
                                state.island.hydration.cycle_target();
                                state.island.remaining_time = 8.0;
                                return LRESULT(0);
                            }

                            // 3. [Reminder Bell]: gauge_cx - 65.0 .. gauge_cx + 70.0, top_c + 66.0 .. top_c + 84.0
                            if mouse_x >= gauge_cx - 65.0 && mouse_x <= gauge_cx + 70.0 && mouse_y >= top_c + 66.0 && mouse_y <= top_c + 84.0 {
                                state.island.hydration.toggle_reminder();
                                state.island.remaining_time = 8.0;
                                return LRESULT(0);
                            }
                        }
                    } else if state.island.mode == island::IslandMode::Expanded {
                        let prev_cx = right - 124.0;
                        let play_cx = right - 88.0;
                        let next_cx = right - 52.0;
                        let toggle_cx = right - 20.0;

                        let dist_toggle = ((mouse_x - toggle_cx).powi(2) + (mouse_y - cy).powi(2)).sqrt();
                        let dist_prev = ((mouse_x - prev_cx).powi(2) + (mouse_y - cy).powi(2)).sqrt();
                        let dist_play = ((mouse_x - play_cx).powi(2) + (mouse_y - cy).powi(2)).sqrt();
                        let dist_next = ((mouse_x - next_cx).powi(2) + (mouse_y - cy).powi(2)).sqrt();

                        // Generous hit box: clicking the toggle button or anywhere in right cap toggles full card!
                        if dist_toggle <= 20.0 || mouse_x >= right - 36.0 {
                            state.island.toggle_full_card();
                            state.island.remaining_time = 5.0;
                            return LRESULT(0);
                        } else if dist_prev <= 18.0 {
                            crate::media::send_media_command(crate::media::MediaCommand::Previous);
                            state.island.remaining_time = 4.0;
                            return LRESULT(0);
                        } else if dist_play <= 20.0 {
                            crate::media::send_media_command(crate::media::MediaCommand::TogglePlayPause);
                            state.island.remaining_time = 4.0;
                            return LRESULT(0);
                        } else if dist_next <= 18.0 {
                            crate::media::send_media_command(crate::media::MediaCommand::Next);
                            state.island.remaining_time = 4.0;
                            return LRESULT(0);
                        }
                    }
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    state.island.is_hovered = true;

                    let mouse_x = (lparam.0 & 0xFFFF) as i16 as f32;
                    let (left, _top, right, _bottom) = state.island.get_pill_rect();

                    if state.dragging_seek {
                        let (seek_left, seek_right) = if state.island.mode == island::IslandMode::FullCard && !state.island.show_lyrics {
                            (left + 44.0 + 42.0, right - 44.0 - 46.0)
                        } else {
                            let cx = (left + right) * 0.5;
                            (left + 44.0 + 38.0, cx - 42.0)
                        };
                        let ratio = ((mouse_x - seek_left) / (seek_right - seek_left)).clamp(0.0, 1.0);
                        if let Some(ref mut track) = state.island.current_track {
                            let target = ratio * track.duration_secs;
                            track.position_secs = target;
                        }
                        state.island.remaining_time = 8.0;
                    } else if state.dragging_volume {
                        let (vol_left, vol_right) = if state.island.mode == island::IslandMode::FullCard && !state.island.show_lyrics {
                            (right - 44.0 - 94.0, right - 44.0 - 10.0)
                        } else {
                            let cx = (left + right) * 0.5;
                            (cx + 12.0 + 18.0, right - 44.0 - 10.0)
                        };
                        let ratio = ((mouse_x - vol_left) / (vol_right - vol_left)).clamp(0.0, 1.0);
                        state.island.volume = ratio;
                        state.audio_meter.set_volume(ratio);
                        state.island.remaining_time = 8.0;
                    }

                    if !state.is_tracking_mouse {
                        let mut tme = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&mut tme);
                        state.is_tracking_mouse = true;
                    }
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE_MSG => {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    state.island.is_hovered = false;
                    state.is_tracking_mouse = false;
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    if state.dragging_seek {
                        state.dragging_seek = false;
                        let _ = ReleaseCapture();
                        if let Some(ref track) = state.island.current_track {
                            crate::media::seek_playback(track.position_secs);
                            state.seek_grace_until = Some(Instant::now() + std::time::Duration::from_millis(1500));
                        }
                        return LRESULT(0);
                    }
                    if state.dragging_volume {
                        state.dragging_volume = false;
                        let _ = ReleaseCapture();
                        return LRESULT(0);
                    }

                    let mouse_x = (lparam.0 & 0xFFFF) as i16 as f32;
                    let mouse_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

                    let (_left, top, right, bottom) = state.island.get_pill_rect();
                    let cy = (top + bottom) * 0.5;

                    if state.island.mode == island::IslandMode::FullCard {
                        return LRESULT(0);
                    } else if state.island.mode == island::IslandMode::Expanded {
                        let prev_cx = right - 124.0;
                        let play_cx = right - 88.0;
                        let next_cx = right - 52.0;
                        let toggle_cx = right - 20.0;

                        let dist_toggle = ((mouse_x - toggle_cx).powi(2) + (mouse_y - cy).powi(2)).sqrt();
                        let dist_prev = ((mouse_x - prev_cx).powi(2) + (mouse_y - cy).powi(2)).sqrt();
                        let dist_play = ((mouse_x - play_cx).powi(2) + (mouse_y - cy).powi(2)).sqrt();
                        let dist_next = ((mouse_x - next_cx).powi(2) + (mouse_y - cy).powi(2)).sqrt();

                        // If user clicked any control button or the toggle button, strictly DO NOT dismiss or activate window!
                        if dist_toggle <= 20.0 || mouse_x >= right - 36.0 || dist_prev <= 18.0 || dist_play <= 20.0 || dist_next <= 18.0 {
                            return LRESULT(0);
                        }

                        // If expanded with media, check playback control buttons
                        if state.island.active_notification.is_none() && (state.island.current_track.is_some() || state.island.is_media_playing) {
                            // Clicked anywhere else on the expanded pill (album art, title, pill surface):
                            // Focus or toggle YouTube Music / Spotify / Browser tab like focusgrow-rs!
                            crate::media::activate_or_toggle_media_player(state.island.current_track.as_ref());
                            state.island.dismiss();
                            return LRESULT(0);
                        }

                        // Otherwise clicking on the island dismisses it
                        state.island.dismiss();
                    } else if state.island.mode == island::IslandMode::Idle || state.island.mode == island::IslandMode::Slim {
                        if state.island.mode == island::IslandMode::Slim && state.island.active_notification.is_some() {
                            // Clicked notification banner: dismiss immediately!
                            state.island.dismiss();
                            return LRESULT(0);
                        } else if state.island.is_media_playing || state.island.current_track.is_some() {
                            // Clicked compact/slim media pill: open/focus YouTube Music / Spotify / Browser tab!
                            crate::media::activate_or_toggle_media_player(state.island.current_track.as_ref());
                        } else if !state.config.idle_click_through {
                            // Clicked idle clock pill: expand full shelf!
                            state.island.expand_full_card();
                        }
                    }
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    if state.island.mode == island::IslandMode::FullCard {
                        let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
                        if delta < 0 {
                            state.island.next_card();
                        } else if delta > 0 {
                            state.island.prev_card();
                        }
                        return LRESULT(0);
                    }
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_CHAR => {
                let ch = wparam.0 as u32;
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    if let Some(ref mut buf) = state.island.entering_task {
                        state.island.remaining_time = 15.0;
                        if ch == 0x0D {
                            // Enter: commit task!
                            let task_title = buf.trim().to_string();
                            if !task_title.is_empty() {
                                state.island.tasks.add_task(task_title, "All Day".to_string(), 1);
                            }
                            state.island.entering_task = None;
                        } else if ch == 0x1B {
                            // Esc: cancel
                            state.island.entering_task = None;
                        } else if ch == 0x08 {
                            // Backspace
                            buf.pop();
                        } else if let Some(c) = char::from_u32(ch) {
                            if !c.is_control() {
                                buf.push(c);
                            }
                        }
                        return LRESULT(0);
                    }
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                let key = wparam.0 as u32;
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    if state.island.entering_task.is_some() {
                        if key == 0x1B {
                            state.island.entering_task = None;
                            return LRESULT(0);
                        }
                    } else if key == 0x1B {
                        state.island.dismiss();
                    }
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                let cmd_id = (wparam.0 & 0xFFFF) as usize;
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    if cmd_id == ID_TRAY_SETTINGS {
                        let _ = settings_window::open_settings_window(hwnd);
                    } else if cmd_id == tray::ID_TRAY_STARTUP {
                        let curr = crate::config::is_autostart_enabled();
                        state.config.start_with_windows = !curr;
                        state.config.save();
                    } else if cmd_id == ID_TRAY_EXIT {
                        let _ = DestroyWindow(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_CONFIG_CHANGED => {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    state.config = AppConfig::load();
                    state.island.top_margin = state.config.top_margin;
                    state.island.island_style = state.config.island_style;
                    state.island.vinyl_speed = state.config.vinyl_speed;
                    state.island.compact_mode_view = state.config.compact_mode_view;
                    state.island.compact_music_width = state.config.compact_music_width;
                    state.island.compact_lyrics_width = state.config.compact_lyrics_width;
                    state.island.show_lyrics_in_capsule = state.config.show_lyrics_in_capsule;
                    state.island.use_full_media_card = state.config.use_full_media_card;
                    state.island.romanize_cjk_lyrics = state.config.romanize_cjk_lyrics;
                    state.island.prayer = crate::prayer::PrayerTracker::new(
                        &state.config.prayer_city,
                        state.config.prayer_latitude,
                        state.config.prayer_longitude,
                        state.config.prayer_timezone,
                        state.config.prayer_offset_mins,
                        crate::prayer::CalculationMethod::from_str(&state.config.prayer_method),
                    );

                    if state.island.mode == island::IslandMode::Idle {
                        let target_w = state.island.target_idle_width();
                        state.island.width_spring.set_target(target_w);
                    } else if state.island.mode == island::IslandMode::Expanded {
                        let (target_w, target_h, target_r) = state.island.island_style.dimensions();
                        state.island.width_spring.set_target(target_w);
                        state.island.height_spring.set_target(target_h);
                        state.island.radius_spring.set_target(target_r);
                    } else if state.island.mode == island::IslandMode::FullCard {
                        state.island.width_spring.set_target(island::FULL_CARD_WIDTH);
                        state.island.height_spring.set_target(island::FULL_CARD_HEIGHT);
                        state.island.radius_spring.set_target(island::FULL_CARD_RADIUS);
                    }

                    // Immediately re-evaluate contour and vertical positioning for live top_margin updates!
                    let fg = GetForegroundWindow();
                    update_smart_contour(state, fg, hwnd);

                    // Re-sync window region clipping and render frame immediately
                    sync_window_region(hwnd, state);
                    let _ = state.renderer.render(&state.island, &state.config);
                }
                LRESULT(0)
            }
            WM_TRAY_MSG => {
                let event = (lparam.0 & 0xFFFF) as u32;
                if event == WM_RBUTTONUP {
                    let _ = show_tray_menu(hwnd);
                } else if event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK {
                    let _ = settings_window::open_settings_window(hwnd);
                }
                LRESULT(0)
            }
            WM_TIMER => {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;

                    // Sync system master volume when not actively dragging
                    if !state.dragging_volume {
                        state.island.volume = state.audio_meter.get_volume();
                    }

                    // Process incoming GPS location detection result
                    while let Ok(res) = state.gps_receiver.try_recv() {
                        state.island.prayer.is_gps_detecting = false;
                        match res {
                            Ok((lat, lon, tz, city)) => {
                                state.island.prayer.set_location(&city, lat, lon, tz);
                                state.config.prayer_city = city;
                                state.config.prayer_latitude = lat;
                                state.config.prayer_longitude = lon;
                                state.config.prayer_timezone = tz;
                                state.config.save();
                            }
                            Err(err) => {
                                state.island.prayer.gps_status = Some(err);
                            }
                        }
                    }

                    // 1. Process real incoming Windows notifications from listener
                    while let Ok(live_notif) = state.notif_receiver.try_recv() {
                        let is_media = live_notif.app_name.to_lowercase().contains("spotify")
                            || live_notif.app_name.to_lowercase().contains("music")
                            || live_notif.app_name.to_lowercase().contains("youtube");
                        if !is_media {
                            state.island.show_notification(live_notif);
                            state.island.remaining_time = state.config.auto_collapse_secs;
                        }
                    }

                    // 2. Process real media events if enabled in config
                    while let Ok(media_event) = state.media_receiver.try_recv() {
                        if state.config.enable_media_island {
                            match media_event {
                                MediaEvent::TrackChanged { notif, thumbnail_bytes, position_secs, duration_secs } => {
                                    state.media_pause_until = None;

                                    let same_track = state.island.current_track.as_ref().map(|t| {
                                        t.title == notif.title && t.artist == notif.body
                                    }).unwrap_or(false);

                                    let prev_thumb = if same_track {
                                        state.island.current_track.as_ref().and_then(|t| t.thumbnail_bytes.clone())
                                    } else {
                                        None
                                    };

                                    let effective_thumb = thumbnail_bytes.or(prev_thumb);

                                    let effective_pos = if same_track { position_secs } else { 0.0 };

                                    state.island.current_track = Some(crate::island::TrackInfo {
                                        title: notif.title.clone(),
                                        artist: notif.body.clone(),
                                        is_playing: true,
                                        thumbnail_bytes: effective_thumb.clone(),
                                        source: notif.app_name.clone(),
                                        position_secs: effective_pos,
                                        duration_secs,
                                    });
                                    if let Some(ref bytes) = effective_thumb {
                                        state.renderer.set_album_art(bytes, state.config.dynamic_album_color);
                                    } else if !same_track {
                                        state.renderer.clear_album_art();
                                    }

                                    if !same_track {
                                        state.island.current_lyrics = None;
                                        state.island.lyrics_loading = true;
                                        state.lyrics_retry_count = 0;
                                        state.lyrics_retry_at = None;
                                        crate::lyrics::fetch_lyrics_async(
                                            notif.title.clone(),
                                            notif.body.clone(),
                                            duration_secs,
                                            state.media_sender.clone(),
                                        );
                                    }

                                    state.island.is_media_playing = true;
                                    state.island.marquee_offset = 0.0;
                                    state.island.active_notification = None;

                                    if state.island.mode == island::IslandMode::Idle {
                                        let target_w = state.island.target_idle_width();
                                        state.island.width_spring.set_target(target_w);
                                    }
                                }
                                MediaEvent::ThumbnailLoaded(bytes) => {
                                    let is_new = if let Some(ref mut track) = state.island.current_track {
                                        let already_has = track.thumbnail_bytes.as_ref().map(|b| b == &bytes).unwrap_or(false);
                                        if !already_has {
                                            track.thumbnail_bytes = Some(bytes.clone());
                                            true
                                        } else {
                                            false
                                        }
                                    } else {
                                        true
                                    };
                                    if is_new {
                                        state.renderer.set_album_art(&bytes, state.config.dynamic_album_color);
                                    }
                                }
                                MediaEvent::PlaybackState(is_playing) => {
                                    if is_playing {
                                        state.media_pause_until = None;
                                        state.island.is_media_playing = true;
                                        if let Some(ref mut track) = state.island.current_track {
                                            track.is_playing = true;
                                        }
                                        if state.island.mode == island::IslandMode::Idle {
                                            let target_w = state.island.target_idle_width();
                                            state.island.width_spring.set_target(target_w);
                                        }
                                    } else {
                                        // Playback paused or buffering during track transition
                                        if let Some(ref mut track) = state.island.current_track {
                                            track.is_playing = false;
                                            // 2500ms debounce grace period: do NOT collapse to clock immediately!
                                            // Completely eliminates the 2x clock-music oscillation during track skips and buffering.
                                            state.media_pause_until = Some(Instant::now() + Duration::from_millis(2500));
                                        } else {
                                            state.media_pause_until = None;
                                            state.island.is_media_playing = false;
                                            if state.island.mode == island::IslandMode::Idle {
                                                let target_w = state.island.target_idle_width();
                                                state.island.width_spring.set_target(target_w);
                                            }
                                        }
                                    }
                                }
                                MediaEvent::TimelineUpdate { position_secs, duration_secs } => {
                                    if let Some(ref mut track) = state.island.current_track {
                                        let in_grace = state.seek_grace_until
                                            .map(|until| Instant::now() < until)
                                            .unwrap_or(false);
                                        if !state.dragging_seek && !in_grace {
                                            let drift = (track.position_secs - position_secs).abs();
                                            if drift > 1.2 {
                                                // Large jump: User seeked in external music player
                                                track.position_secs = position_secs;
                                            } else if drift > 0.08 {
                                                // Gentle micro-adjustment towards true timeline (avoids karaoke jerking)
                                                track.position_secs += (position_secs - track.position_secs) * 0.20;
                                            }
                                        }
                                        if duration_secs > 0.0 {
                                            track.duration_secs = duration_secs;
                                        }
                                    }
                                }
                                MediaEvent::LyricsLoaded(lyrics) => {
                                    // Ensure lyrics match currently playing track to avoid slow-worker race conditions
                                    let belongs = if let Some(ref track) = state.island.current_track {
                                        lyrics.title.is_empty()
                                            || lyrics.title.eq_ignore_ascii_case(&track.title)
                                            || track.title.to_lowercase().contains(&lyrics.title.to_lowercase())
                                            || lyrics.title.to_lowercase().contains(&track.title.to_lowercase())
                                    } else {
                                        true
                                    };

                                    if belongs {
                                        state.island.active_candidate_idx = 0;
                                        state.island.lyrics_candidates = lyrics.candidates.clone();
                                        if !lyrics.lines.is_empty() || lyrics.is_instrumental {
                                            state.island.current_lyrics = Some(lyrics);
                                            state.island.lyrics_loading = false;
                                        } else {
                                            // Empty result: schedule automatic retry after 3.5s if attempts remain
                                            state.island.current_lyrics = Some(lyrics);
                                            state.island.lyrics_loading = false;
                                            if state.lyrics_retry_count < 2 {
                                                state.lyrics_retry_at = Some(Instant::now() + Duration::from_millis(3500));
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            state.media_pause_until = None;
                            state.island.is_media_playing = false;
                        }
                    }

                    // Check media pause debounce grace period
                    if let Some(pause_until) = state.media_pause_until {
                        if Instant::now() >= pause_until {
                            state.media_pause_until = None;
                            state.island.is_media_playing = false;
                            if state.island.mode == island::IslandMode::Idle {
                                let target_w = state.island.target_idle_width();
                                state.island.width_spring.set_target(target_w);
                            }
                        }
                    }

                    // Auto-retry lyrics search if initial lookup was empty or network timed out
                    if state.island.is_media_playing
                        && !state.island.lyrics_loading
                        && state.lyrics_retry_count < 2
                    {
                        let has_lyrics = state.island.current_lyrics.as_ref()
                            .map(|l| !l.lines.is_empty() || l.is_instrumental)
                            .unwrap_or(false);
                        if !has_lyrics {
                            if let Some(retry_at) = state.lyrics_retry_at {
                                if Instant::now() >= retry_at {
                                    state.lyrics_retry_count += 1;
                                    state.lyrics_retry_at = None;
                                    state.island.lyrics_loading = true;
                                    if let Some(ref track) = state.island.current_track {
                                        crate::lyrics::fetch_lyrics_async(
                                            track.title.clone(),
                                            track.artist.clone(),
                                            track.duration_secs,
                                            state.media_sender.clone(),
                                        );
                                    }
                                }
                            }
                        }
                    }

                    // 3. Advance physics animation with jitter-free smoothed time step
                    let now = Instant::now();
                    let raw_dt = (now - state.last_tick).as_secs_f32().clamp(0.008, 0.033);
                    state.last_tick = now;
                    state.smoothed_dt = state.smoothed_dt * 0.70 + raw_dt * 0.30;
                    let dt = state.smoothed_dt;

                    // Update real PC audio peak amplitude from WASAPI
                    state.island.audio_peak = state.audio_meter.update(dt);

                    let fg = GetForegroundWindow();
                    let screen_w = GetSystemMetrics(SM_CXSCREEN);
                    let screen_h = GetSystemMetrics(SM_CYSCREEN);

                    // 1. Fullscreen Auto-Hide:
                    // If active foreground window is fullscreen (e.g. Netflix, YouTube Fullscreen, Video, Games),
                    // smoothly hide the Dynamic Island completely so it never blocks video or gameplay!
                    if is_foreground_fullscreen(fg, screen_w, screen_h) {
                        state.island.alpha_spring.set_target(0.0);
                        state.island.y_spring.set_target(-100.0);
                        state.island.update(dt);
                        state.island.hydration.update(dt);
                        sync_window_region(hwnd, state);
                        let _ = state.renderer.render(&state.island, &state.config);
                        return LRESULT(0);
                    } else {
                        // Strictly maintain HWND_TOPMOST so Dynamic Island is permanently on top of all windows
                        let _ = SetWindowPos(
                            hwnd,
                            Some(HWND_TOPMOST),
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                        );
                    }

                    // 2. Smart Dynamic Contour Avoidance:
                    update_smart_contour(state, fg, hwnd);

                    // Cursor hover detection & hover-to-expand
                    let mut cursor = POINT::default();
                    if GetCursorPos(&mut cursor).is_ok() {
                        let mut rect = RECT::default();
                        let _ = GetWindowRect(hwnd, &mut rect);
                        let is_hover = state.island.hit_test(cursor.x, cursor.y, rect.left, rect.top);
                        state.island.is_hovered = is_hover;

                        if is_hover {
                            state.hover_dwell_time += dt;

                            if state.island.mode == island::IslandMode::Slim {
                                state.island.expand_full();
                            } else if state.island.mode == island::IslandMode::Idle
                                && (state.island.is_media_playing || state.island.current_track.is_some())
                            {
                                // Only expand media controls if hovered deliberately for >180ms.
                                // This prevents accidental expansion when cursor quickly moves past the top of the screen!
                                if state.config.hover_expand_media && state.hover_dwell_time >= 0.18 {
                                    state.island.expand_media();
                                }
                            }
                        } else {
                            state.hover_dwell_time = 0.0;
                            if state.island.mode == island::IslandMode::Expanded {
                                state.island.dismiss();
                            }
                        }
                    }

                    state.island.update(dt);
                    state.island.hydration.update(dt);

                    if state.island.hydration.trigger_alert {
                        state.island.hydration.trigger_alert = false;
                        if state.island.mode == island::IslandMode::Idle {
                            let notif = Notification {
                                app_name: "Hydration".to_string(),
                                title: "Time to Drink Water".to_string(),
                                body: "Take a sip to stay fresh & focused!".to_string(),
                                time_str: "now".to_string(),
                                badge_letter: "H".to_string(),
                                badge_color: (0.25, 0.78, 0.98),
                                icon_bytes: None,
                                aumid: None,
                            };
                            state.island.show_notification(notif);
                        }
                    }

                    // Periodic Working Set Trimming every 12 seconds to keep RAM strictly low (15-20 MB):
                    state.idle_settled_timer += dt;
                    if state.idle_settled_timer >= 12.0 {
                        state.idle_settled_timer = 0.0;
                        trim_working_set();
                    }

                    sync_window_region(hwnd, state);
                    let _ = state.renderer.render(&state.island, &state.config);
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                remove_tray_icon(hwnd);
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    let _ = KillTimer(Some(hwnd), TIMER_ID);
                    drop(Box::from_raw(state_ptr));
                }
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }));

    match result {
        Ok(lres) => lres,
        Err(e) => {
            let msg_text = format!("PANIC in wndproc msg={}: {:?}", msg, e);
            let _ = std::fs::write("panic.log", msg_text);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("PANIC: {}\nLocation: {:?}", info, info.location());
        let _ = std::fs::write("panic.log", msg);
    }));

    if let Err(e) = run_app() {
        let _ = std::fs::write("error.log", format!("{:?}", e));
    }
}

fn run_app() -> Result<()> {
    unsafe {
        // Ensure working directory is always the executable's folder, even when started by Windows Autostart!
        if let Ok(mut exe_dir) = std::env::current_exe() {
            exe_dir.pop();
            let _ = std::env::set_current_dir(&exe_dir);
        }

        // 0. Single Instance Lock (strictly prevent running 2x!)
        let mutex_name = w!("Local\\DynamicIslandSingleInstanceMutex");
        let mutex = windows::Win32::System::Threading::CreateMutexW(None, true, mutex_name);
        let last_err = windows::Win32::Foundation::GetLastError();
        if let Ok(m) = mutex {
            if last_err == windows::Win32::Foundation::ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(m);
                let class_name = w!("DynamicIslandOverlayClass");
                if let Ok(existing_hwnd) = FindWindowW(class_name, None) {
                    if !existing_hwnd.is_invalid() {
                        let _ = PostMessageW(Some(existing_hwnd), WM_CONFIG_CHANGED, WPARAM(0), LPARAM(0));
                    }
                }
                return Ok(());
            }
        }

        // 1. Enable Per-Monitor V2 High-DPI Awareness
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // 2. Load Configuration
        let config = AppConfig::load();

        // 3. Register Win32 Window Class with 2-Palette Dynamic Island Icon
        let hinstance: HMODULE = GetModuleHandleW(None)?;
        let class_name = w!("DynamicIslandOverlayClass");

        let icon_big = LoadImageW(
            Some(HINSTANCE(hinstance.0)),
            PCWSTR(1 as *const u16),
            IMAGE_ICON,
            32,
            32,
            LR_DEFAULTCOLOR,
        )
        .ok()
        .map(|h| HICON(h.0))
        .unwrap_or_else(|| crate::icons::create_dynamic_island_icon(32).unwrap_or_default());

        let icon_sm = LoadImageW(
            Some(HINSTANCE(hinstance.0)),
            PCWSTR(1 as *const u16),
            IMAGE_ICON,
            16,
            16,
            LR_DEFAULTCOLOR,
        )
        .ok()
        .map(|h| HICON(h.0))
        .unwrap_or_else(|| crate::icons::create_dynamic_island_icon(16).unwrap_or_default());

        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance.into(),
            hIcon: icon_big,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: class_name,
            hIconSm: icon_sm,
        };

        RegisterClassExW(&wnd_class);

        // 4. Compute top-center screen position on primary monitor
        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let pos_x = (screen_width - WINDOW_WIDTH as i32) / 2;
        let pos_y = 0; // Anchored at the top edge

        let ex_style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP;

        let hwnd = CreateWindowExW(
            ex_style,
            class_name,
            w!("Dynamic Island"),
            WS_POPUP | WS_VISIBLE,
            pos_x,
            pos_y,
            WINDOW_WIDTH as i32,
            WINDOW_HEIGHT as i32,
            None,
            None,
            Some(HINSTANCE(hinstance.0)),
            None,
        )?;

        // Explicitly set window icons (Big & Small) for Taskbar & Alt+Tab
        let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(icon_big.0 as isize)));
        let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(icon_sm.0 as isize)));

        // 5. Initialize DirectComposition + Direct2D Renderer
        let renderer = Renderer::new(hwnd, WINDOW_WIDTH, WINDOW_HEIGHT)?;
        let mut island = Island::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32);
        island.top_margin = config.top_margin;
        island.island_style = config.island_style;
        island.vinyl_speed = config.vinyl_speed;
        island.compact_mode_view = config.compact_mode_view;
        island.compact_music_width = config.compact_music_width;
        island.compact_lyrics_width = config.compact_lyrics_width;
        island.show_lyrics_in_capsule = config.show_lyrics_in_capsule;
        island.use_full_media_card = config.use_full_media_card;
        island.romanize_cjk_lyrics = config.romanize_cjk_lyrics;
        island.prayer = crate::prayer::PrayerTracker::new(
            &config.prayer_city,
            config.prayer_latitude,
            config.prayer_longitude,
            config.prayer_timezone,
            config.prayer_offset_mins,
            crate::prayer::CalculationMethod::from_str(&config.prayer_method),
        );

        // 6. Spawn Live Windows Notification Listener
        let (notif_tx, notif_rx) = std::sync::mpsc::channel::<Notification>();
        start_notification_listener(notif_tx);

        // 7. Spawn Windows Media / Music Listener
        let (media_tx, media_rx) = std::sync::mpsc::channel::<MediaEvent>();
        start_media_listener(media_tx.clone());

        // 8. GPS Location Detector Channel
        let (gps_tx, gps_rx) = std::sync::mpsc::channel();

        // 9. Register System Tray Icon
        let _ = add_tray_icon(hwnd);

        let audio_meter = audio_meter::AudioMeter::new();
        island.volume = audio_meter.get_volume();

        let app_state = Box::new(AppState {
            island,
            renderer,
            config,
            audio_meter,
            last_tick: Instant::now(),
            smoothed_dt: 0.016,
            is_tracking_mouse: false,
            hover_dwell_time: 0.0,
            idle_settled_timer: 0.0,
            last_rgn_rect: (-1, -1, -1, -1),
            notif_receiver: notif_rx,
            media_sender: media_tx,
            media_receiver: media_rx,
            dragging_seek: false,
            dragging_volume: false,
            seek_grace_until: None,
            media_pause_until: None,
            lyrics_retry_count: 0,
            lyrics_retry_at: None,
            gps_sender: gps_tx,
            gps_receiver: gps_rx,
        });

        // Store AppState pointer into window UserData
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app_state) as isize);

        // 9. Start high-precision animation timer
        SetTimer(Some(hwnd), TIMER_ID, TIMER_INTERVAL_MS, None);

        // 10. Initial render & window region clipping
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
        if !state_ptr.is_null() {
            let state = &mut *state_ptr;
            sync_window_region(hwnd, state);
            let _ = state.renderer.render(&state.island, &state.config);
        }

        let _ = check_listener_status();

        // 11. Working Set Memory Optimization:
        // Flush one-time DirectX/D3D11/WIC startup DLL initialization pages from physical memory.
        // This drops the reported Task Manager Working Set from ~35 MB down to ~12-14 MB!
        trim_working_set();

        // 12. Standard Win32 Message Loop
        let mut msg = MSG::default();
        loop {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 <= 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(())
}
