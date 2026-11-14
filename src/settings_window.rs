#![allow(unsafe_op_in_unsafe_fn)]

use crate::config::{AppConfig, CompactDisplayMode, IslandStyle};
use std::sync::atomic::{AtomicBool, Ordering};

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::*,
            Direct2D::Common::*,
            DirectWrite::*,
            Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE},
            Gdi::*,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
            WindowsAndMessaging::*,
        },
    },
};

static SETTINGS_OPEN: AtomicBool = AtomicBool::new(false);

const CLIENT_WIDTH: u32 = 520;
const CLIENT_HEIGHT: u32 = 906;

// Exact geometry rects (left, top, right, bottom)
const CARD_STYLE_0: (f32, f32, f32, f32) = (20.0, 88.0, 254.0, 138.0);
const CARD_STYLE_1: (f32, f32, f32, f32) = (266.0, 88.0, 500.0, 138.0);
const CARD_STYLE_2: (f32, f32, f32, f32) = (20.0, 144.0, 254.0, 194.0);
const CARD_STYLE_3: (f32, f32, f32, f32) = (266.0, 144.0, 500.0, 194.0);

const CHIP_VIEW_0: (f32, f32, f32, f32) = (20.0, 226.0, 172.0, 264.0);
const CHIP_VIEW_1: (f32, f32, f32, f32) = (178.0, 226.0, 336.0, 264.0);
const CHIP_VIEW_2: (f32, f32, f32, f32) = (342.0, 226.0, 500.0, 264.0);

const SLIDER_CARD_MARGIN: (f32, f32, f32, f32) = (20.0, 296.0, 500.0, 344.0);
const SLIDER_CARD_TIMEOUT: (f32, f32, f32, f32) = (20.0, 350.0, 500.0, 398.0);
const SLIDER_CARD_OPACITY: (f32, f32, f32, f32) = (20.0, 404.0, 500.0, 452.0);
const SLIDER_CARD_GLOW: (f32, f32, f32, f32) = (20.0, 458.0, 500.0, 506.0);
const SLIDER_CARD_VINYL_SPEED: (f32, f32, f32, f32) = (20.0, 512.0, 500.0, 560.0);

const TOGGLE_CARD_DYNAMIC_COLOR: (f32, f32, f32, f32) = (20.0, 594.0, 500.0, 638.0);
const TOGGLE_CARD_AUDIO_VIZ: (f32, f32, f32, f32) = (20.0, 644.0, 500.0, 688.0);
const TOGGLE_CARD_VINYL_ANIM: (f32, f32, f32, f32) = (20.0, 694.0, 500.0, 738.0);
const TOGGLE_CARD_HOVER_EXPAND: (f32, f32, f32, f32) = (20.0, 744.0, 500.0, 788.0);
const TOGGLE_CARD_CLICK_THROUGH: (f32, f32, f32, f32) = (20.0, 794.0, 500.0, 838.0);
const TOGGLE_CARD_STARTUP: (f32, f32, f32, f32) = (20.0, 844.0, 500.0, 888.0);

#[inline]
fn in_rect(x: f32, y: f32, r: (f32, f32, f32, f32)) -> bool {
    x >= r.0 && x <= r.2 && y >= r.1 && y <= r.3
}

#[inline]
fn pt(x: f32, y: f32) -> windows_numerics::Vector2 {
    windows_numerics::Vector2 { X: x, Y: y }
}

struct SettingsState {
    parent_hwnd: HWND,
    d2d_target: ID2D1HwndRenderTarget,

    format_title: IDWriteTextFormat,
    format_subtitle: IDWriteTextFormat,
    format_sec_title: IDWriteTextFormat,
    format_card_title: IDWriteTextFormat,
    format_card_sub: IDWriteTextFormat,
    format_badge: IDWriteTextFormat,

    brush_card: ID2D1SolidColorBrush,
    brush_card_hover: ID2D1SolidColorBrush,
    brush_card_border: ID2D1SolidColorBrush,
    brush_card_selected_bg: ID2D1SolidColorBrush,
    brush_accent: ID2D1SolidColorBrush,
    brush_accent_hover: ID2D1SolidColorBrush,
    brush_text_white: ID2D1SolidColorBrush,
    brush_text_sub: ID2D1SolidColorBrush,
    brush_toggle_on: ID2D1SolidColorBrush,
    brush_toggle_off: ID2D1SolidColorBrush,
    brush_badge_bg: ID2D1SolidColorBrush,
    brush_auto_saved: ID2D1SolidColorBrush,
    brush_icon: ID2D1SolidColorBrush,

    top_margin: f32,
    duration: f32,
    island_opacity: f32,
    glow_intensity: f32,
    vinyl_speed: f32,
    island_style: IslandStyle,
    compact_view: CompactDisplayMode,
    dynamic_color: bool,
    enable_media: bool,
    enable_vinyl: bool,
    hover_expand: bool,
    click_through: bool,
    start_with_windows: bool,

    dragging_slider: Option<usize>,
    hovered_item: Option<usize>,
}

impl SettingsState {
    fn auto_save(&self) {
        let cfg = AppConfig {
            top_margin: self.top_margin,
            auto_collapse_secs: self.duration,
            island_style: self.island_style,
            island_opacity: self.island_opacity,
            glow_intensity: self.glow_intensity,
            vinyl_speed: self.vinyl_speed,
            compact_mode_view: self.compact_view,
            dynamic_album_color: self.dynamic_color,
            enable_media_island: self.enable_media,
            enable_vinyl_animation: self.enable_vinyl,
            hover_expand_media: self.hover_expand,
            idle_click_through: self.click_through,
            start_with_windows: self.start_with_windows,
        };
        cfg.save();

        let _ = unsafe {
            PostMessageW(
                Some(self.parent_hwnd),
                crate::tray::WM_CONFIG_CHANGED,
                WPARAM(0),
                LPARAM(0),
            )
        };
    }

    // --- Vector SVG Drawing Helpers ---

    unsafe fn draw_island_header_icon(&self, cx: f32, cy: f32) {
        let pill = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: cx - 12.0, top: cy - 7.0, right: cx + 12.0, bottom: cy + 7.0 },
            radiusX: 7.0,
            radiusY: 7.0,
        };
        self.d2d_target.FillRoundedRectangle(&pill, &self.brush_accent);

        let dot = D2D1_ELLIPSE {
            point: pt(cx - 4.5, cy),
            radiusX: 2.2,
            radiusY: 2.2,
        };
        self.d2d_target.FillEllipse(&dot, &self.brush_text_white);

        self.d2d_target.DrawLine(pt(cx + 2.0, cy), pt(cx + 6.0, cy), &self.brush_text_white, 1.5, None);
    }

    unsafe fn draw_checkmark(&self, cx: f32, cy: f32) {
        let bg_circle = D2D1_ELLIPSE {
            point: pt(cx, cy),
            radiusX: 7.5,
            radiusY: 7.5,
        };
        self.d2d_target.FillEllipse(&bg_circle, &self.brush_accent);

        self.d2d_target.DrawLine(pt(cx - 3.8, cy + 0.2), pt(cx - 1.0, cy + 2.8), &self.brush_text_white, 1.7, None);
        self.d2d_target.DrawLine(pt(cx - 1.0, cy + 2.8), pt(cx + 3.8, cy - 2.8), &self.brush_text_white, 1.7, None);
    }

    unsafe fn draw_style_preview_icon(&self, style: IslandStyle, cx: f32, cy: f32, is_selected: bool) {
        let brush = if is_selected { &self.brush_accent } else { &self.brush_icon };
        match style {
            IslandStyle::SleekCapsule => {
                let rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: cx - 11.0, top: cy - 4.5, right: cx + 11.0, bottom: cy + 4.5 },
                    radiusX: 4.5,
                    radiusY: 4.5,
                };
                self.d2d_target.DrawRoundedRectangle(&rect, brush, 1.4, None);
            }
            IslandStyle::BalancedPill => {
                let rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: cx - 9.0, top: cy - 6.0, right: cx + 9.0, bottom: cy + 6.0 },
                    radiusX: 6.0,
                    radiusY: 6.0,
                };
                self.d2d_target.DrawRoundedRectangle(&rect, brush, 1.4, None);
            }
            IslandStyle::SpaciousCard => {
                let rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: cx - 10.0, top: cy - 7.5, right: cx + 10.0, bottom: cy + 7.5 },
                    radiusX: 3.5,
                    radiusY: 3.5,
                };
                self.d2d_target.DrawRoundedRectangle(&rect, brush, 1.4, None);
            }
            IslandStyle::CompactMinimal => {
                let rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: cx - 7.5, top: cy - 4.5, right: cx + 7.5, bottom: cy + 4.5 },
                    radiusX: 4.5,
                    radiusY: 4.5,
                };
                self.d2d_target.DrawRoundedRectangle(&rect, brush, 1.4, None);
            }
        }
    }

    unsafe fn draw_sound_wave_icon(&self, cx: f32, cy: f32, is_active: bool) {
        let brush = if is_active { &self.brush_accent } else { &self.brush_icon };
        let bars = [-4.5, -1.5, 1.5, 4.5];
        let heights = [6.0, 13.0, 9.0, 5.0];

        for (i, &offset_x) in bars.iter().enumerate() {
            let h = heights[i] * 0.5;
            self.d2d_target.DrawLine(pt(cx + offset_x, cy - h), pt(cx + offset_x, cy + h), brush, 1.7, None);
        }
    }

    unsafe fn draw_vinyl_icon(&self, cx: f32, cy: f32, is_active: bool) {
        let brush = if is_active { &self.brush_accent } else { &self.brush_icon };
        let disc = D2D1_ELLIPSE {
            point: pt(cx, cy),
            radiusX: 7.5,
            radiusY: 7.5,
        };
        self.d2d_target.DrawEllipse(&disc, brush, 1.3, None);

        let groove = D2D1_ELLIPSE {
            point: pt(cx, cy),
            radiusX: 4.2,
            radiusY: 4.2,
        };
        self.d2d_target.DrawEllipse(&groove, brush, 0.9, None);

        let spindle = D2D1_ELLIPSE {
            point: pt(cx, cy),
            radiusX: 1.6,
            radiusY: 1.6,
        };
        self.d2d_target.FillEllipse(&spindle, brush);

        self.d2d_target.DrawLine(pt(cx + 6.0, cy - 7.5), pt(cx + 3.8, cy - 1.8), brush, 1.3, None);
    }

    unsafe fn draw_palette_icon(&self, cx: f32, cy: f32, is_active: bool) {
        let brush = if is_active { &self.brush_accent } else { &self.brush_icon };
        let circle = D2D1_ELLIPSE {
            point: pt(cx, cy),
            radiusX: 7.2,
            radiusY: 7.2,
        };
        self.d2d_target.DrawEllipse(&circle, brush, 1.3, None);

        // 3 paint dab dots
        for (dx, dy) in [(-3.0, -2.5), (2.0, -3.2), (3.2, 1.5)] {
            let dab = D2D1_ELLIPSE {
                point: pt(cx + dx, cy + dy),
                radiusX: 1.2,
                radiusY: 1.2,
            };
            self.d2d_target.FillEllipse(&dab, brush);
        }
    }

    unsafe fn draw_expand_icon(&self, cx: f32, cy: f32, is_active: bool) {
        let brush = if is_active { &self.brush_accent } else { &self.brush_icon };
        let center_box = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: cx - 4.0, top: cy - 2.5, right: cx + 4.0, bottom: cy + 2.5 },
            radiusX: 2.0,
            radiusY: 2.0,
        };
        self.d2d_target.FillRoundedRectangle(&center_box, brush);

        self.d2d_target.DrawLine(pt(cx - 7.5, cy - 4.0), pt(cx - 7.5, cy - 7.5), brush, 1.3, None);
        self.d2d_target.DrawLine(pt(cx - 7.5, cy - 7.5), pt(cx - 4.0, cy - 7.5), brush, 1.3, None);

        self.d2d_target.DrawLine(pt(cx + 7.5, cy + 4.0), pt(cx + 7.5, cy + 7.5), brush, 1.3, None);
        self.d2d_target.DrawLine(pt(cx + 7.5, cy + 7.5), pt(cx + 4.0, cy + 7.5), brush, 1.3, None);
    }

    unsafe fn draw_click_through_icon(&self, cx: f32, cy: f32, is_active: bool) {
        let brush = if is_active { &self.brush_accent } else { &self.brush_icon };
        let rect = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: cx - 7.5, top: cy - 6.5, right: cx + 7.5, bottom: cy + 6.5 },
            radiusX: 2.5,
            radiusY: 2.5,
        };
        self.d2d_target.DrawRoundedRectangle(&rect, brush, 1.2, None);

        // Arrow passing through
        self.d2d_target.DrawLine(pt(cx - 3.5, cy - 2.5), pt(cx + 3.5, cy + 3.0), brush, 1.5, None);
        self.d2d_target.DrawLine(pt(cx + 3.5, cy + 3.0), pt(cx + 0.5, cy + 3.0), brush, 1.4, None);
        self.d2d_target.DrawLine(pt(cx + 3.5, cy + 3.0), pt(cx + 3.5, cy + 0.0), brush, 1.4, None);
    }

    unsafe fn draw_slider_track(&self, rect: (f32, f32, f32, f32), norm: f32, is_dragging: bool) {
        let track_y = rect.1 + 34.0;
        let track_left = rect.0 + 14.0;
        let track_right = rect.2 - 14.0;
        let track_width = track_right - track_left;
        let thumb_x = track_left + norm * track_width;

        // Background track rail
        self.brush_toggle_off.SetColor(&D2D1_COLOR_F { r: 0.16, g: 0.18, b: 0.22, a: 1.0 });
        self.d2d_target.DrawLine(pt(track_left, track_y), pt(track_right, track_y), &self.brush_toggle_off, 4.0, None);

        // Active highlighted rail
        let active_brush = if is_dragging { &self.brush_accent_hover } else { &self.brush_accent };
        if thumb_x > track_left {
            self.d2d_target.DrawLine(pt(track_left, track_y), pt(thumb_x, track_y), active_brush, 4.0, None);
        }

        // Thumb circle
        let thumb_r = if is_dragging { 8.5 } else { 7.0 };
        let thumb = D2D1_ELLIPSE {
            point: pt(thumb_x, track_y),
            radiusX: thumb_r,
            radiusY: thumb_r,
        };
        self.d2d_target.FillEllipse(&thumb, &self.brush_text_white);

        let border_brush = if is_dragging { &self.brush_accent_hover } else { &self.brush_accent };
        self.d2d_target.DrawEllipse(&thumb, border_brush, 2.0, None);
    }

    unsafe fn draw_toggle(&self, cx: f32, cy: f32, is_on: bool) {
        let pill = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: cx - 18.0, top: cy - 10.0, right: cx + 18.0, bottom: cy + 10.0 },
            radiusX: 10.0,
            radiusY: 10.0,
        };
        let (bg_brush, thumb_x) = if is_on {
            (&self.brush_toggle_on, cx + 8.0)
        } else {
            (&self.brush_toggle_off, cx - 8.0)
        };
        self.d2d_target.FillRoundedRectangle(&pill, bg_brush);

        let thumb = D2D1_ELLIPSE {
            point: pt(thumb_x, cy),
            radiusX: 6.8,
            radiusY: 6.8,
        };
        self.d2d_target.FillEllipse(&thumb, &self.brush_text_white);
    }

    unsafe fn draw_startup_icon(&self, cx: f32, cy: f32, is_on: bool) {
        let brush = if is_on { &self.brush_accent } else { &self.brush_icon };
        let sz = 4.5;
        let gap = 2.0;
        for &dx in &[-sz - gap * 0.5, gap * 0.5] {
            for &dy in &[-sz - gap * 0.5, gap * 0.5] {
                let tile = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: cx + dx, top: cy + dy, right: cx + dx + sz, bottom: cy + dy + sz },
                    radiusX: 1.2,
                    radiusY: 1.2,
                };
                self.d2d_target.FillRoundedRectangle(&tile, brush);
            }
        }
    }
}

pub fn open_settings_window(parent_hwnd: HWND) -> Result<()> {
    if SETTINGS_OPEN.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    unsafe {
        let hinstance = GetModuleHandleW(PCWSTR::null())?;
        let class_name = w!("DynamicIslandSettingsClass");

        let icon_big = crate::icons::create_dynamic_island_icon(32).unwrap_or_default();
        let icon_sm = crate::icons::create_dynamic_island_icon(16).unwrap_or_default();

        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(settings_wndproc),
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

        let mut rc = RECT {
            left: 0,
            top: 0,
            right: CLIENT_WIDTH as i32,
            bottom: CLIENT_HEIGHT as i32,
        };
        let _ = AdjustWindowRectEx(&mut rc, WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, false, WINDOW_EX_STYLE::default());

        let outer_w = rc.right - rc.left;
        let outer_h = rc.bottom - rc.top;

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let pos_x = (screen_w - outer_w) / 2;
        let pos_y = (screen_h - outer_h) / 2;

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Dynamic Island Settings"),
            WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_VISIBLE,
            pos_x,
            pos_y,
            outer_w,
            outer_h,
            Some(parent_hwnd),
            None,
            Some(HINSTANCE(hinstance.0)),
            None,
        )?;

        let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(icon_big.0 as isize)));
        let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(icon_sm.0 as isize)));

        let dark_mode = BOOL(1);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode as *const _ as *const _,
            std::mem::size_of::<BOOL>() as u32,
        );

        // Direct2D Setup
        let d2d_factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

        let render_props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };

        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: D2D_SIZE_U {
                width: CLIENT_WIDTH,
                height: CLIENT_HEIGHT,
            },
            presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
        };

        let d2d_target = d2d_factory.CreateHwndRenderTarget(&render_props, &hwnd_props)?;

        // Typography
        let format_title = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Display"),
            None,
            DWRITE_FONT_WEIGHT_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            15.5,
            w!("en-US"),
        )?;

        let format_subtitle = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_REGULAR,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            11.0,
            w!("en-US"),
        )?;

        let format_sec_title = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Display"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            10.5,
            w!("en-US"),
        )?;

        let format_card_title = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            12.0,
            w!("en-US"),
        )?;

        let format_card_sub = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_REGULAR,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            10.0,
            w!("en-US"),
        )?;

        let format_badge = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Text"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            10.5,
            w!("en-US"),
        )?;

        // Brushes
        let brush_card = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.10, g: 0.11, b: 0.13, a: 0.95 }, None)?;
        let brush_card_hover = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.14, g: 0.15, b: 0.18, a: 0.95 }, None)?;
        let brush_card_border = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.19, g: 0.21, b: 0.25, a: 0.85 }, None)?;
        let brush_card_selected_bg = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.05, g: 0.20, b: 0.38, a: 0.40 }, None)?;
        let brush_accent = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.48, b: 1.0, a: 1.0 }, None)?;
        let brush_accent_hover = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.15, g: 0.58, b: 1.0, a: 1.0 }, None)?;
        let brush_text_white = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.96, g: 0.96, b: 0.98, a: 1.0 }, None)?;
        let brush_text_sub = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.60, g: 0.63, b: 0.68, a: 1.0 }, None)?;
        let brush_toggle_on = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.118, g: 0.843, b: 0.376, a: 1.0 }, None)?;
        let brush_toggle_off = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.20, g: 0.22, b: 0.26, a: 1.0 }, None)?;
        let brush_badge_bg = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.06, g: 0.22, b: 0.13, a: 0.85 }, None)?;
        let brush_auto_saved = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.22, g: 0.88, b: 0.45, a: 1.0 }, None)?;
        let brush_icon = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.50, g: 0.54, b: 0.62, a: 1.0 }, None)?;

        let cfg = AppConfig::load();

        let state = Box::new(SettingsState {
            parent_hwnd,
            d2d_target,
            format_title,
            format_subtitle,
            format_sec_title,
            format_card_title,
            format_card_sub,
            format_badge,
            brush_card,
            brush_card_hover,
            brush_card_border,
            brush_card_selected_bg,
            brush_accent,
            brush_accent_hover,
            brush_text_white,
            brush_text_sub,
            brush_toggle_on,
            brush_toggle_off,
            brush_badge_bg,
            brush_auto_saved,
            brush_icon,
            top_margin: cfg.top_margin,
            duration: cfg.auto_collapse_secs,
            island_opacity: cfg.island_opacity,
            glow_intensity: cfg.glow_intensity,
            vinyl_speed: cfg.vinyl_speed,
            island_style: cfg.island_style,
            compact_view: cfg.compact_mode_view,
            dynamic_color: cfg.dynamic_album_color,
            enable_media: cfg.enable_media_island,
            enable_vinyl: cfg.enable_vinyl_animation,
            hover_expand: cfg.hover_expand_media,
            click_through: cfg.idle_click_through,
            start_with_windows: cfg.start_with_windows,
            dragging_slider: None,
            hovered_item: None,
        });

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
    }

    Ok(())
}

unsafe extern "system" fn settings_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _hdc = BeginPaint(hwnd, &mut ps);

            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsState;
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                state.d2d_target.BeginDraw();

                // 1. Deep OLED dark canvas background
                state.d2d_target.Clear(Some(&D2D1_COLOR_F { r: 0.05, g: 0.05, b: 0.06, a: 1.0 }));

                // 2. Header
                state.draw_island_header_icon(36.0, 34.0);

                let title_rect = D2D_RECT_F { left: 56.0, top: 20.0, right: 380.0, bottom: 42.0 };
                let title_utf: Vec<u16> = "Dynamic Island Settings".encode_utf16().collect();
                state.d2d_target.DrawText(&title_utf, &state.format_title, &title_rect as *const _, &state.brush_text_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                let sub_rect = D2D_RECT_F { left: 56.0, top: 42.0, right: 380.0, bottom: 62.0 };
                let sub_utf: Vec<u16> = "Personalize layout, reactive visualizers, vinyl speeds & colors".encode_utf16().collect();
                state.d2d_target.DrawText(&sub_utf, &state.format_subtitle, &sub_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                // Auto-saved badge
                let badge_pill = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: 394.0, top: 22.0, right: 500.0, bottom: 46.0 },
                    radiusX: 12.0,
                    radiusY: 12.0,
                };
                state.d2d_target.FillRoundedRectangle(&badge_pill, &state.brush_badge_bg);
                let badge_text_rect = D2D_RECT_F { left: 404.0, top: 26.0, right: 494.0, bottom: 44.0 };
                let badge_utf: Vec<u16> = "✓ Auto-saved".encode_utf16().collect();
                state.d2d_target.DrawText(&badge_utf, &state.format_badge, &badge_text_rect as *const _, &state.brush_auto_saved, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                // --- Section 1: ISLAND APPEARANCE & STYLE ---
                let sec1_rect = D2D_RECT_F { left: 22.0, top: 68.0, right: 400.0, bottom: 84.0 };
                let sec1_utf: Vec<u16> = "ISLAND APPEARANCE & STYLE".encode_utf16().collect();
                state.d2d_target.DrawText(&sec1_utf, &state.format_sec_title, &sec1_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                let style_cards = [
                    (CARD_STYLE_0, IslandStyle::SleekCapsule),
                    (CARD_STYLE_1, IslandStyle::BalancedPill),
                    (CARD_STYLE_2, IslandStyle::SpaciousCard),
                    (CARD_STYLE_3, IslandStyle::CompactMinimal),
                ];

                for (idx, &(rect_f, style)) in style_cards.iter().enumerate() {
                    let is_selected = state.island_style == style;
                    let is_hovered = state.hovered_item == Some(idx);

                    let card_rect = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: rect_f.0, top: rect_f.1, right: rect_f.2, bottom: rect_f.3 },
                        radiusX: 7.0,
                        radiusY: 7.0,
                    };

                    let bg_brush = if is_selected {
                        &state.brush_card_selected_bg
                    } else if is_hovered {
                        &state.brush_card_hover
                    } else {
                        &state.brush_card
                    };
                    state.d2d_target.FillRoundedRectangle(&card_rect, bg_brush);

                    let border_brush = if is_selected { &state.brush_accent } else { &state.brush_card_border };
                    let stroke = if is_selected { 1.6 } else { 1.0 };
                    state.d2d_target.DrawRoundedRectangle(&card_rect, border_brush, stroke, None);

                    state.draw_style_preview_icon(style, rect_f.0 + 20.0, rect_f.1 + 25.0, is_selected);

                    let title_r = D2D_RECT_F { left: rect_f.0 + 38.0, top: rect_f.1 + 8.0, right: rect_f.2 - 28.0, bottom: rect_f.1 + 26.0 };
                    let t_utf: Vec<u16> = style.display_name().encode_utf16().collect();
                    state.d2d_target.DrawText(&t_utf, &state.format_card_title, &title_r as *const _, &state.brush_text_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                    let sub_r = D2D_RECT_F { left: rect_f.0 + 38.0, top: rect_f.1 + 26.0, right: rect_f.2 - 8.0, bottom: rect_f.1 + 44.0 };
                    let s_utf: Vec<u16> = style.description().encode_utf16().collect();
                    state.d2d_target.DrawText(&s_utf, &state.format_card_sub, &sub_r as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                    if is_selected {
                        state.draw_checkmark(rect_f.2 - 18.0, rect_f.1 + 18.0);
                    }
                }

                // --- Section 2: COMPACT MUSIC DISPLAY VIEW ---
                let sec2_rect = D2D_RECT_F { left: 22.0, top: 206.0, right: 400.0, bottom: 222.0 };
                let sec2_utf: Vec<u16> = "COMPACT MUSIC VIEW (IDLE BAR)".encode_utf16().collect();
                state.d2d_target.DrawText(&sec2_utf, &state.format_sec_title, &sec2_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                let chip_items = [
                    (CHIP_VIEW_0, CompactDisplayMode::Combined, "Combined (Title + Wave)"),
                    (CHIP_VIEW_1, CompactDisplayMode::TitleOnly, "Title Only"),
                    (CHIP_VIEW_2, CompactDisplayMode::WaveOnly, "Wave Only"),
                ];

                for (idx, &(rect_f, mode, label)) in chip_items.iter().enumerate() {
                    let is_selected = state.compact_view == mode;
                    let is_hovered = state.hovered_item == Some(10 + idx);

                    let chip_rect = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: rect_f.0, top: rect_f.1, right: rect_f.2, bottom: rect_f.3 },
                        radiusX: 7.0,
                        radiusY: 7.0,
                    };

                    let bg_brush = if is_selected {
                        &state.brush_card_selected_bg
                    } else if is_hovered {
                        &state.brush_card_hover
                    } else {
                        &state.brush_card
                    };
                    state.d2d_target.FillRoundedRectangle(&chip_rect, bg_brush);

                    let border_brush = if is_selected { &state.brush_accent } else { &state.brush_card_border };
                    let stroke = if is_selected { 1.6 } else { 1.0 };
                    state.d2d_target.DrawRoundedRectangle(&chip_rect, border_brush, stroke, None);

                    let label_r = D2D_RECT_F { left: rect_f.0 + 8.0, top: rect_f.1 + 10.0, right: rect_f.2 - 8.0, bottom: rect_f.3 - 8.0 };
                    let l_utf: Vec<u16> = label.encode_utf16().collect();
                    let text_brush = if is_selected { &state.brush_text_white } else { &state.brush_text_sub };
                    state.d2d_target.DrawText(&l_utf, &state.format_badge, &label_r as *const _, text_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                }

                // --- Section 3: FINE-TUNING SLIDERS ---
                let sec3_rect = D2D_RECT_F { left: 22.0, top: 278.0, right: 400.0, bottom: 294.0 };
                let sec3_utf: Vec<u16> = "FINE-TUNING CONTROLS".encode_utf16().collect();
                state.d2d_target.DrawText(&sec3_utf, &state.format_sec_title, &sec3_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                let sliders = [
                    (SLIDER_CARD_MARGIN, "Top Screen Margin", format!("{} px", state.top_margin as i32), (state.top_margin / 60.0).clamp(0.0, 1.0), 0),
                    (SLIDER_CARD_TIMEOUT, "Notification Timeout", format!("{:.1} s", state.duration), ((state.duration - 2.0) / 13.0).clamp(0.0, 1.0), 1),
                    (SLIDER_CARD_OPACITY, "Island Background Opacity", format!("{:.0} %", state.island_opacity * 100.0), ((state.island_opacity - 0.30) / 0.70).clamp(0.0, 1.0), 2),
                    (SLIDER_CARD_GLOW, "Border Glow & Halo Intensity", format!("{:.0} %", state.glow_intensity * 100.0), state.glow_intensity.clamp(0.0, 1.0), 3),
                    (SLIDER_CARD_VINYL_SPEED, "Vinyl Rotation Speed", format!("{:.1}x", state.vinyl_speed), ((state.vinyl_speed - 0.1) / 1.9).clamp(0.0, 1.0), 4),
                ];

                for (rect_f, title, val_str, norm, s_idx) in sliders {
                    let card_rect = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: rect_f.0, top: rect_f.1, right: rect_f.2, bottom: rect_f.3 },
                        radiusX: 7.0,
                        radiusY: 7.0,
                    };
                    state.d2d_target.FillRoundedRectangle(&card_rect, &state.brush_card);
                    state.d2d_target.DrawRoundedRectangle(&card_rect, &state.brush_card_border, 1.0, None);

                    let t_rect = D2D_RECT_F { left: rect_f.0 + 14.0, top: rect_f.1 + 8.0, right: rect_f.2 - 90.0, bottom: rect_f.1 + 26.0 };
                    let t_utf: Vec<u16> = title.encode_utf16().collect();
                    state.d2d_target.DrawText(&t_utf, &state.format_card_title, &t_rect as *const _, &state.brush_text_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                    let v_rect = D2D_RECT_F { left: rect_f.2 - 80.0, top: rect_f.1 + 8.0, right: rect_f.2 - 14.0, bottom: rect_f.1 + 26.0 };
                    let v_utf: Vec<u16> = val_str.encode_utf16().collect();
                    state.d2d_target.DrawText(&v_utf, &state.format_badge, &v_rect as *const _, &state.brush_accent, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                    let is_dragging = state.dragging_slider == Some(s_idx);
                    state.draw_slider_track(rect_f, norm, is_dragging);
                }

                // --- Section 4: INTERACTIVE TOGGLES ---
                let sec4_rect = D2D_RECT_F { left: 22.0, top: 574.0, right: 400.0, bottom: 590.0 };
                let sec4_utf: Vec<u16> = "BEHAVIOR & ANIMATION".encode_utf16().collect();
                state.d2d_target.DrawText(&sec4_utf, &state.format_sec_title, &sec4_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                let toggles = [
                    (TOGGLE_CARD_DYNAMIC_COLOR, "Dynamic Album Color Accent", "Tint border, halo glow & waves adaptively by album cover", state.dynamic_color, 20),
                    (TOGGLE_CARD_AUDIO_VIZ, "Real-time Audio Visualizer (WASAPI)", "Reactive equalizer waves driven by active PC speaker sound", state.enable_media, 21),
                    (TOGGLE_CARD_VINYL_ANIM, "Spinning Vinyl Disc Animation", "Rotating vinyl record & album artwork cover during music playback", state.enable_vinyl, 22),
                    (TOGGLE_CARD_HOVER_EXPAND, "Hover to Expand Media", "Smoothly expand compact island into full controls when cursor hovers", state.hover_expand, 23),
                    (TOGGLE_CARD_CLICK_THROUGH, "Idle Click-Through", "Pass mouse clicks directly to browser tabs and windows underneath", state.click_through, 24),
                    (TOGGLE_CARD_STARTUP, "Start with Windows", "Launch Dynamic Island automatically when system boots", state.start_with_windows, 25),
                ];

                for (rect_f, title, desc, is_on, t_idx) in toggles {
                    let is_hovered = state.hovered_item == Some(t_idx);
                    let card_rect = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: rect_f.0, top: rect_f.1, right: rect_f.2, bottom: rect_f.3 },
                        radiusX: 7.0,
                        radiusY: 7.0,
                    };
                    let bg_brush = if is_hovered { &state.brush_card_hover } else { &state.brush_card };
                    state.d2d_target.FillRoundedRectangle(&card_rect, bg_brush);
                    state.d2d_target.DrawRoundedRectangle(&card_rect, &state.brush_card_border, 1.0, None);

                    let icon_cx = rect_f.0 + 20.0;
                    let icon_cy = (rect_f.1 + rect_f.3) * 0.5;

                    match t_idx {
                        20 => state.draw_palette_icon(icon_cx, icon_cy, is_on),
                        21 => state.draw_sound_wave_icon(icon_cx, icon_cy, is_on),
                        22 => state.draw_vinyl_icon(icon_cx, icon_cy, is_on),
                        23 => state.draw_expand_icon(icon_cx, icon_cy, is_on),
                        24 => state.draw_click_through_icon(icon_cx, icon_cy, is_on),
                        25 => state.draw_startup_icon(icon_cx, icon_cy, is_on),
                        _ => {}
                    }

                    let t_rect = D2D_RECT_F { left: rect_f.0 + 38.0, top: rect_f.1 + 6.0, right: rect_f.2 - 58.0, bottom: rect_f.1 + 24.0 };
                    let t_utf: Vec<u16> = title.encode_utf16().collect();
                    state.d2d_target.DrawText(&t_utf, &state.format_card_title, &t_rect as *const _, &state.brush_text_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                    let d_rect = D2D_RECT_F { left: rect_f.0 + 38.0, top: rect_f.1 + 24.0, right: rect_f.2 - 58.0, bottom: rect_f.1 + 40.0 };
                    let d_utf: Vec<u16> = desc.encode_utf16().collect();
                    state.d2d_target.DrawText(&d_utf, &state.format_card_sub, &d_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                    state.draw_toggle(rect_f.2 - 28.0, (rect_f.1 + rect_f.3) * 0.5, is_on);
                }

                let _ = state.d2d_target.EndDraw(None, None);
            }

            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsState;
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;

                // 1. Style Selection
                if in_rect(x, y, CARD_STYLE_0) {
                    state.island_style = IslandStyle::SleekCapsule;
                    state.auto_save();
                } else if in_rect(x, y, CARD_STYLE_1) {
                    state.island_style = IslandStyle::BalancedPill;
                    state.auto_save();
                } else if in_rect(x, y, CARD_STYLE_2) {
                    state.island_style = IslandStyle::SpaciousCard;
                    state.auto_save();
                } else if in_rect(x, y, CARD_STYLE_3) {
                    state.island_style = IslandStyle::CompactMinimal;
                    state.auto_save();
                }
                // 2. Compact View Chip Selection
                else if in_rect(x, y, CHIP_VIEW_0) {
                    state.compact_view = CompactDisplayMode::Combined;
                    state.auto_save();
                } else if in_rect(x, y, CHIP_VIEW_1) {
                    state.compact_view = CompactDisplayMode::TitleOnly;
                    state.auto_save();
                } else if in_rect(x, y, CHIP_VIEW_2) {
                    state.compact_view = CompactDisplayMode::WaveOnly;
                    state.auto_save();
                }
                // 3. Sliders Drag Start
                else if in_rect(x, y, SLIDER_CARD_MARGIN) {
                    state.dragging_slider = Some(0);
                    let norm = ((x - (SLIDER_CARD_MARGIN.0 + 14.0)) / (SLIDER_CARD_MARGIN.2 - SLIDER_CARD_MARGIN.0 - 28.0)).clamp(0.0, 1.0);
                    state.top_margin = (norm * 60.0).round();
                    state.auto_save();
                    SetCapture(hwnd);
                } else if in_rect(x, y, SLIDER_CARD_TIMEOUT) {
                    state.dragging_slider = Some(1);
                    let norm = ((x - (SLIDER_CARD_TIMEOUT.0 + 14.0)) / (SLIDER_CARD_TIMEOUT.2 - SLIDER_CARD_TIMEOUT.0 - 28.0)).clamp(0.0, 1.0);
                    state.duration = ((2.0 + norm * 13.0) * 10.0).round() / 10.0;
                    state.auto_save();
                    SetCapture(hwnd);
                } else if in_rect(x, y, SLIDER_CARD_OPACITY) {
                    state.dragging_slider = Some(2);
                    let norm = ((x - (SLIDER_CARD_OPACITY.0 + 14.0)) / (SLIDER_CARD_OPACITY.2 - SLIDER_CARD_OPACITY.0 - 28.0)).clamp(0.0, 1.0);
                    state.island_opacity = ((0.30 + norm * 0.70) * 100.0).round() / 100.0;
                    state.auto_save();
                    SetCapture(hwnd);
                } else if in_rect(x, y, SLIDER_CARD_GLOW) {
                    state.dragging_slider = Some(3);
                    let norm = ((x - (SLIDER_CARD_GLOW.0 + 14.0)) / (SLIDER_CARD_GLOW.2 - SLIDER_CARD_GLOW.0 - 28.0)).clamp(0.0, 1.0);
                    state.glow_intensity = (norm * 100.0).round() / 100.0;
                    state.auto_save();
                    SetCapture(hwnd);
                } else if in_rect(x, y, SLIDER_CARD_VINYL_SPEED) {
                    state.dragging_slider = Some(4);
                    let norm = ((x - (SLIDER_CARD_VINYL_SPEED.0 + 14.0)) / (SLIDER_CARD_VINYL_SPEED.2 - SLIDER_CARD_VINYL_SPEED.0 - 28.0)).clamp(0.0, 1.0);
                    state.vinyl_speed = ((0.1 + norm * 1.9) * 10.0).round() / 10.0;
                    state.auto_save();
                    SetCapture(hwnd);
                }
                // 4. Interactive Toggles (Full card clickable!)
                else if in_rect(x, y, TOGGLE_CARD_DYNAMIC_COLOR) {
                    state.dynamic_color = !state.dynamic_color;
                    state.auto_save();
                } else if in_rect(x, y, TOGGLE_CARD_AUDIO_VIZ) {
                    state.enable_media = !state.enable_media;
                    state.auto_save();
                } else if in_rect(x, y, TOGGLE_CARD_VINYL_ANIM) {
                    state.enable_vinyl = !state.enable_vinyl;
                    state.auto_save();
                } else if in_rect(x, y, TOGGLE_CARD_HOVER_EXPAND) {
                    state.hover_expand = !state.hover_expand;
                    state.auto_save();
                } else if in_rect(x, y, TOGGLE_CARD_CLICK_THROUGH) {
                    state.click_through = !state.click_through;
                    state.auto_save();
                } else if in_rect(x, y, TOGGLE_CARD_STARTUP) {
                    state.start_with_windows = !state.start_with_windows;
                    state.auto_save();
                }

                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xFFFF) as i16 as f32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;

            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsState;
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;

                if let Some(s_idx) = state.dragging_slider {
                    match s_idx {
                        0 => {
                            let norm = ((x - (SLIDER_CARD_MARGIN.0 + 14.0)) / (SLIDER_CARD_MARGIN.2 - SLIDER_CARD_MARGIN.0 - 28.0)).clamp(0.0, 1.0);
                            state.top_margin = (norm * 60.0).round();
                            state.auto_save();
                        }
                        1 => {
                            let norm = ((x - (SLIDER_CARD_TIMEOUT.0 + 14.0)) / (SLIDER_CARD_TIMEOUT.2 - SLIDER_CARD_TIMEOUT.0 - 28.0)).clamp(0.0, 1.0);
                            state.duration = ((2.0 + norm * 13.0) * 10.0).round() / 10.0;
                            state.auto_save();
                        }
                        2 => {
                            let norm = ((x - (SLIDER_CARD_OPACITY.0 + 14.0)) / (SLIDER_CARD_OPACITY.2 - SLIDER_CARD_OPACITY.0 - 28.0)).clamp(0.0, 1.0);
                            state.island_opacity = ((0.30 + norm * 0.70) * 100.0).round() / 100.0;
                            state.auto_save();
                        }
                        3 => {
                            let norm = ((x - (SLIDER_CARD_GLOW.0 + 14.0)) / (SLIDER_CARD_GLOW.2 - SLIDER_CARD_GLOW.0 - 28.0)).clamp(0.0, 1.0);
                            state.glow_intensity = (norm * 100.0).round() / 100.0;
                            state.auto_save();
                        }
                        4 => {
                            let norm = ((x - (SLIDER_CARD_VINYL_SPEED.0 + 14.0)) / (SLIDER_CARD_VINYL_SPEED.2 - SLIDER_CARD_VINYL_SPEED.0 - 28.0)).clamp(0.0, 1.0);
                            state.vinyl_speed = ((0.1 + norm * 1.9) * 10.0).round() / 10.0;
                            state.auto_save();
                        }
                        _ => {}
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
                } else {
                    let old_hover = state.hovered_item;
                    let mut new_hover = None;

                    if in_rect(x, y, CARD_STYLE_0) { new_hover = Some(0); }
                    else if in_rect(x, y, CARD_STYLE_1) { new_hover = Some(1); }
                    else if in_rect(x, y, CARD_STYLE_2) { new_hover = Some(2); }
                    else if in_rect(x, y, CARD_STYLE_3) { new_hover = Some(3); }
                    else if in_rect(x, y, CHIP_VIEW_0) { new_hover = Some(10); }
                    else if in_rect(x, y, CHIP_VIEW_1) { new_hover = Some(11); }
                    else if in_rect(x, y, CHIP_VIEW_2) { new_hover = Some(12); }
                    else if in_rect(x, y, TOGGLE_CARD_DYNAMIC_COLOR) { new_hover = Some(20); }
                    else if in_rect(x, y, TOGGLE_CARD_AUDIO_VIZ) { new_hover = Some(21); }
                    else if in_rect(x, y, TOGGLE_CARD_VINYL_ANIM) { new_hover = Some(22); }
                    else if in_rect(x, y, TOGGLE_CARD_HOVER_EXPAND) { new_hover = Some(23); }
                    else if in_rect(x, y, TOGGLE_CARD_CLICK_THROUGH) { new_hover = Some(24); }
                    else if in_rect(x, y, TOGGLE_CARD_STARTUP) { new_hover = Some(25); }

                    if old_hover != new_hover {
                        state.hovered_item = new_hover;
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsState;
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                if state.dragging_slider.is_some() {
                    state.dragging_slider = None;
                    let _ = ReleaseCapture();
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsState;
            if !state_ptr.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(state_ptr));
            }
            SETTINGS_OPEN.store(false, Ordering::SeqCst);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
