#![allow(unsafe_op_in_unsafe_fn)]
#![windows_subsystem = "windows"]

mod animation;
mod audio_meter;
mod config;
mod icons;
mod island;
mod media;
mod notifications;
mod renderer;
mod settings_window;
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
use std::time::Instant;

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::{CreateRectRgn, CreateRoundRectRgn, SetWindowRgn, HBRUSH},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
            Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT},
            WindowsAndMessaging::*,
        },
    },
};

const WINDOW_WIDTH: u32 = 720;
const WINDOW_HEIGHT: u32 = 180;
const TIMER_ID: usize = 1001;
const TIMER_INTERVAL_MS: u32 = 16; // ~60 FPS smooth animation tick
const WM_MOUSELEAVE_MSG: u32 = 0x02A3;

struct AppState {
    island: Island,
    renderer: Renderer,
    config: AppConfig,
    audio_meter: audio_meter::AudioMeter,
    last_tick: Instant,
    smoothed_dt: f32,
    is_tracking_mouse: bool,
    hover_dwell_time: f32,
    last_rgn_rect: (i32, i32, i32, i32),
    notif_receiver: Receiver<Notification>,
    media_receiver: Receiver<MediaEvent>,
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
                let _ = SetWindowRgn(hwnd, Some(empty_rgn), true);
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
            let _ = SetWindowRgn(hwnd, Some(rgn), true);
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

/// Automatically nudges the island down when active foreground app is a browser with top tabs.
fn update_smart_contour(state: &mut AppState, fg: HWND, hwnd: HWND) {
    if !state.config.smart_contour {
        state.island.y_spring.set_target(state.config.top_margin);
        return;
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
            WM_MOUSEMOVE => {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    state.island.is_hovered = true;

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
                    let mouse_x = (lparam.0 & 0xFFFF) as i16 as f32;
                    let mouse_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

                    let (_left, top, right, bottom) = state.island.get_pill_rect();
                    let cy = (top + bottom) * 0.5;

                    if state.island.mode == island::IslandMode::Expanded {
                        // If expanded with media, check playback control buttons
                        if state.island.current_track.is_some() || state.island.is_media_playing {
                            let prev_x = right - 92.0;
                            let play_x = right - 58.0;
                            let next_x = right - 24.0;

                            let dist_prev = ((mouse_x - prev_x).powi(2) + (mouse_y - cy).powi(2)).sqrt();
                            let dist_play = ((mouse_x - play_x).powi(2) + (mouse_y - cy).powi(2)).sqrt();
                            let dist_next = ((mouse_x - next_x).powi(2) + (mouse_y - cy).powi(2)).sqrt();

                            if dist_prev <= 16.0 {
                                crate::media::send_media_command(crate::media::MediaCommand::Previous);
                                state.island.remaining_time = 4.0;
                                return LRESULT(0);
                            } else if dist_play <= 18.0 {
                                crate::media::send_media_command(crate::media::MediaCommand::TogglePlayPause);
                                state.island.remaining_time = 4.0;
                                return LRESULT(0);
                            } else if dist_next <= 16.0 {
                                crate::media::send_media_command(crate::media::MediaCommand::Next);
                                state.island.remaining_time = 4.0;
                                return LRESULT(0);
                            }

                            // Clicked anywhere else on the expanded pill (album art, title, pill surface):
                            // Focus or toggle YouTube Music / Spotify / Browser tab like focusgrow-rs!
                            crate::media::activate_or_toggle_media_player(state.island.current_track.as_ref());
                            state.island.dismiss();
                            return LRESULT(0);
                        }

                        // Otherwise clicking on the island dismisses it
                        state.island.dismiss();
                    } else if state.island.mode == island::IslandMode::Idle || state.island.mode == island::IslandMode::Slim {
                        if state.island.is_media_playing || state.island.current_track.is_some() {
                            // Clicked compact/slim media pill: open/focus YouTube Music / Spotify / Browser tab!
                            crate::media::activate_or_toggle_media_player(state.island.current_track.as_ref());
                        }
                    }
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                let key = wparam.0 as u32;
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
                if !state_ptr.is_null() {
                    let state = &mut *state_ptr;
                    if key == 0x1B {
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
                    if state.island.mode == island::IslandMode::Expanded {
                        let (target_w, target_h, target_r) = state.island.island_style.dimensions();
                        state.island.width_spring.set_target(target_w);
                        state.island.height_spring.set_target(target_h);
                        state.island.radius_spring.set_target(target_r);
                    }
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

                    // 1. Process real incoming Windows notifications from listener
                    while let Ok(live_notif) = state.notif_receiver.try_recv() {
                        state.island.show_notification(live_notif);
                        state.island.remaining_time = state.config.auto_collapse_secs;
                    }

                    // 2. Process real media events if enabled in config
                    while let Ok(media_event) = state.media_receiver.try_recv() {
                        if state.config.enable_media_island {
                            match media_event {
                                MediaEvent::TrackChanged { notif, thumbnail_bytes } => {
                                    let same_track = state.island.current_track.as_ref().map(|t| {
                                        t.title == notif.title && t.artist == notif.body
                                    }).unwrap_or(false);

                                    let prev_thumb = if same_track {
                                        state.island.current_track.as_ref().and_then(|t| t.thumbnail_bytes.clone())
                                    } else {
                                        None
                                    };

                                    let effective_thumb = thumbnail_bytes.or(prev_thumb);

                                    state.island.current_track = Some(crate::island::TrackInfo {
                                        title: notif.title.clone(),
                                        artist: notif.body.clone(),
                                        is_playing: true,
                                        thumbnail_bytes: effective_thumb.clone(),
                                        source: notif.app_name.clone(),
                                    });
                                    if let Some(ref bytes) = effective_thumb {
                                        state.renderer.set_album_art(bytes, state.config.dynamic_album_color);
                                    } else if !same_track {
                                        state.renderer.clear_album_art();
                                    }
                                    state.island.show_notification(notif);
                                    state.island.remaining_time = 4.0;
                                }
                                MediaEvent::ThumbnailLoaded(bytes) => {
                                    if let Some(ref mut track) = state.island.current_track {
                                        track.thumbnail_bytes = Some(bytes.clone());
                                    }
                                    state.renderer.set_album_art(&bytes, state.config.dynamic_album_color);
                                }
                                MediaEvent::PlaybackState(is_playing) => {
                                    state.island.is_media_playing = is_playing;
                                    if let Some(ref mut track) = state.island.current_track {
                                        track.is_playing = is_playing;
                                    }
                                }
                            }
                        } else {
                            state.island.is_media_playing = false;
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
                        sync_window_region(hwnd, state);
                        let _ = state.renderer.render(&state.island, &state.config);
                        return LRESULT(0);
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

                    // Auto-rehydrate album art if current track has thumbnail bytes but renderer bitmap is empty
                    if state.renderer.album_art.is_none() {
                        if let Some(ref track) = state.island.current_track {
                            if let Some(ref bytes) = track.thumbnail_bytes {
                                state.renderer.set_album_art(bytes, state.config.dynamic_album_color);
                            }
                        }
                    }

                    state.island.update(dt);
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

        // 6. Spawn Live Windows Notification Listener
        let (notif_tx, notif_rx) = std::sync::mpsc::channel::<Notification>();
        start_notification_listener(notif_tx);

        // 7. Spawn Windows Media / Music Listener
        let (media_tx, media_rx) = std::sync::mpsc::channel::<MediaEvent>();
        start_media_listener(media_tx);

        // 8. Register System Tray Icon
        let _ = add_tray_icon(hwnd);

        let audio_meter = audio_meter::AudioMeter::new();

        let app_state = Box::new(AppState {
            island,
            renderer,
            config,
            audio_meter,
            last_tick: Instant::now(),
            smoothed_dt: 0.016,
            is_tracking_mouse: false,
            hover_dwell_time: 0.0,
            last_rgn_rect: (-1, -1, -1, -1),
            notif_receiver: notif_rx,
            media_receiver: media_rx,
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

        // 11. Standard Win32 Message Loop
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
