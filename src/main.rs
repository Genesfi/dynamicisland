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
        Graphics::Gdi::HBRUSH,
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
    notif_receiver: Receiver<Notification>,
    media_receiver: Receiver<MediaEvent>,
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
                    // When in idle mode and click-through enabled, pass clicks straight through to browser tabs!
                    if state.config.idle_click_through && state.island.mode == island::IslandMode::Idle {
                        return LRESULT(HTTRANSPARENT as isize);
                    }

                    let screen_x = (lparam.0 & 0xFFFF) as i16 as i32;
                    let screen_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;

                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);

                    if state.island.hit_test(screen_x, screen_y, rect.left, rect.top) {
                        // When in idle or slim mode and not hovered, let clicks pass straight through to browser tabs!
                        if state.config.idle_click_through
                            && (state.island.mode == island::IslandMode::Idle
                                || (state.island.mode == island::IslandMode::Slim && !state.island.is_hovered))
                        {
                            return LRESULT(HTTRANSPARENT as isize);
                        }
                        return LRESULT(HTCLIENT as isize);
                    } else {
                        return LRESULT(HTTRANSPARENT as isize);
                    }
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
                    if state.island.mode == island::IslandMode::Expanded {
                        let mouse_x = (lparam.0 & 0xFFFF) as i16 as f32;
                        let mouse_y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

                        let (_left, top, right, bottom) = state.island.get_pill_rect();
                        let cy = (top + bottom) * 0.5;

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
                        }

                        // Otherwise clicking on the island dismisses it
                        state.island.dismiss();
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
                        if state.island.mode == island::IslandMode::Expanded {
                            state.island.dismiss();
                        } else {
                            let _ = DestroyWindow(hwnd);
                        }
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
                } else if event == WM_LBUTTONDBLCLK {
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
                                    state.island.current_track = Some(crate::island::TrackInfo {
                                        title: notif.title.clone(),
                                        artist: notif.body.clone(),
                                        is_playing: true,
                                        thumbnail_bytes: thumbnail_bytes.clone(),
                                    });
                                    if let Some(ref bytes) = thumbnail_bytes {
                                        state.renderer.set_album_art(bytes, state.config.dynamic_album_color);
                                    } else {
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

                    // Cursor hover detection & hover-to-expand
                    let mut cursor = POINT::default();
                    if GetCursorPos(&mut cursor).is_ok() {
                        let mut rect = RECT::default();
                        let _ = GetWindowRect(hwnd, &mut rect);
                        let is_hover = state.island.hit_test(cursor.x, cursor.y, rect.left, rect.top);
                        state.island.is_hovered = is_hover;

                        // Hover on slim or idle media island expands smoothly without needing a click!
                        if is_hover {
                            if state.island.mode == island::IslandMode::Slim {
                                state.island.expand_full();
                            } else if state.island.mode == island::IslandMode::Idle
                                && (state.island.is_media_playing || state.island.current_track.is_some())
                            {
                                if state.config.hover_expand_media {
                                    state.island.expand_media();
                                }
                            }
                        }
                    }

                    state.island.update(dt);
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

        let icon_big = crate::icons::create_dynamic_island_icon(32).unwrap_or_default();
        let icon_sm = crate::icons::create_dynamic_island_icon(16).unwrap_or_default();

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
            notif_receiver: notif_rx,
            media_receiver: media_rx,
        });

        // Store AppState pointer into window UserData
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app_state) as isize);

        // 9. Start high-precision animation timer
        SetTimer(Some(hwnd), TIMER_ID, TIMER_INTERVAL_MS, None);

        // 10. Initial render
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
        if !state_ptr.is_null() {
            let state = &mut *state_ptr;
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
