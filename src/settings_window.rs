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
const CLIENT_HEIGHT: u32 = 560;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Appearance,
    Music,
    System,
}

// Tab navigation buttons
const TAB_APPEARANCE: (f32, f32, f32, f32) = (20.0, 70.0, 172.0, 102.0);
const TAB_MUSIC: (f32, f32, f32, f32) = (178.0, 70.0, 338.0, 102.0);
const TAB_SYSTEM: (f32, f32, f32, f32) = (344.0, 70.0, 500.0, 102.0);

// --- Tab 0: Appearance Coordinates ---
const CARD_STYLE_0: (f32, f32, f32, f32) = (20.0, 134.0, 254.0, 184.0);
const CARD_STYLE_1: (f32, f32, f32, f32) = (266.0, 134.0, 500.0, 184.0);
const CARD_STYLE_2: (f32, f32, f32, f32) = (20.0, 190.0, 254.0, 240.0);
const CARD_STYLE_3: (f32, f32, f32, f32) = (266.0, 190.0, 500.0, 240.0);

const CHIP_VIEW_0: (f32, f32, f32, f32) = (20.0, 274.0, 172.0, 312.0);
const CHIP_VIEW_1: (f32, f32, f32, f32) = (178.0, 274.0, 336.0, 312.0);
const CHIP_VIEW_2: (f32, f32, f32, f32) = (342.0, 274.0, 500.0, 312.0);

const SLIDER_CARD_MARGIN: (f32, f32, f32, f32) = (20.0, 346.0, 500.0, 394.0);
const SLIDER_CARD_OPACITY: (f32, f32, f32, f32) = (20.0, 400.0, 500.0, 448.0);
const SLIDER_CARD_GLOW: (f32, f32, f32, f32) = (20.0, 454.0, 500.0, 502.0);

// --- Tab 1: Music & Audio Coordinates ---
const TOGGLE_CARD_DYNAMIC_COLOR: (f32, f32, f32, f32) = (20.0, 134.0, 500.0, 178.0);
const TOGGLE_CARD_AUDIO_VIZ: (f32, f32, f32, f32) = (20.0, 184.0, 500.0, 228.0);
const TOGGLE_CARD_VINYL_ANIM: (f32, f32, f32, f32) = (20.0, 234.0, 500.0, 278.0);
const TOGGLE_CARD_HOVER_EXPAND: (f32, f32, f32, f32) = (20.0, 284.0, 500.0, 328.0);
const SLIDER_CARD_VINYL_SPEED: (f32, f32, f32, f32) = (20.0, 358.0, 500.0, 406.0);
const SLIDER_CARD_COMPACT_WIDTH: (f32, f32, f32, f32) = (20.0, 436.0, 500.0, 484.0);

// --- Tab 2: System & Behavior Coordinates ---
const TOGGLE_CARD_STARTUP: (f32, f32, f32, f32) = (20.0, 134.0, 500.0, 180.0);
const TOGGLE_CARD_CLICK_THROUGH: (f32, f32, f32, f32) = (20.0, 188.0, 500.0, 234.0);
const TOGGLE_CARD_SMART_CONTOUR: (f32, f32, f32, f32) = (20.0, 242.0, 500.0, 288.0);
const SLIDER_CARD_TIMEOUT: (f32, f32, f32, f32) = (20.0, 324.0, 500.0, 372.0);

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
    format_tab: IDWriteTextFormat,
    format_sec_title: IDWriteTextFormat,
    format_card_title: IDWriteTextFormat,
    format_card_sub: IDWriteTextFormat,
    format_badge: IDWriteTextFormat,

    brush_card: ID2D1SolidColorBrush,
    brush_card_hover: ID2D1SolidColorBrush,
    brush_card_border: ID2D1SolidColorBrush,
    brush_card_selected_bg: ID2D1SolidColorBrush,
    brush_tab_active: ID2D1SolidColorBrush,
    brush_accent: ID2D1SolidColorBrush,
    brush_text_white: ID2D1SolidColorBrush,
    brush_text_sub: ID2D1SolidColorBrush,
    brush_toggle_on: ID2D1SolidColorBrush,
    brush_toggle_off: ID2D1SolidColorBrush,
    brush_badge_bg: ID2D1SolidColorBrush,
    brush_auto_saved: ID2D1SolidColorBrush,
    brush_icon: ID2D1SolidColorBrush,

    active_tab: SettingsTab,

    top_margin: f32,
    duration: f32,
    island_opacity: f32,
    glow_intensity: f32,
    vinyl_speed: f32,
    compact_music_width: f32,
    island_style: IslandStyle,
    compact_view: CompactDisplayMode,
    dynamic_color: bool,
    enable_media: bool,
    enable_vinyl: bool,
    hover_expand: bool,
    click_through: bool,
    smart_contour: bool,
    contour_nudge_y: f32,
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
            compact_music_width: self.compact_music_width,
            dynamic_album_color: self.dynamic_color,
            enable_media_island: self.enable_media,
            enable_vinyl_animation: self.enable_vinyl,
            hover_expand_media: self.hover_expand,
            idle_click_through: self.click_through,
            smart_contour: self.smart_contour,
            contour_nudge_y: self.contour_nudge_y,
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
            point: windows_numerics::Vector2 { X: cx - 4.5, Y: cy },
            radiusX: 2.5,
            radiusY: 2.5,
        };
        self.d2d_target.FillEllipse(&dot, &self.brush_card);
    }

    unsafe fn draw_desktop_icon(&self, cx: f32, cy: f32) {
        let monitor = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: cx - 7.0, top: cy - 6.0, right: cx + 7.0, bottom: cy + 3.0 },
            radiusX: 1.5,
            radiusY: 1.5,
        };
        self.d2d_target.DrawRoundedRectangle(&monitor, &self.brush_icon, 1.4, None);
        self.d2d_target.DrawLine(pt(cx, cy + 3.0), pt(cx, cy + 6.0), &self.brush_icon, 1.4, None);
        self.d2d_target.DrawLine(pt(cx - 4.0, cy + 6.0), pt(cx + 4.0, cy + 6.0), &self.brush_icon, 1.4, None);
    }

    unsafe fn draw_timer_icon(&self, cx: f32, cy: f32) {
        let clock = D2D1_ELLIPSE {
            point: windows_numerics::Vector2 { X: cx, Y: cy },
            radiusX: 6.5,
            radiusY: 6.5,
        };
        self.d2d_target.DrawEllipse(&clock, &self.brush_icon, 1.4, None);
        self.d2d_target.DrawLine(pt(cx, cy), pt(cx, cy - 3.8), &self.brush_icon, 1.4, None);
        self.d2d_target.DrawLine(pt(cx, cy), pt(cx + 2.8, cy), &self.brush_icon, 1.4, None);
    }

    unsafe fn draw_palette_icon(&self, cx: f32, cy: f32) {
        let circle = D2D1_ELLIPSE {
            point: windows_numerics::Vector2 { X: cx, Y: cy },
            radiusX: 6.5,
            radiusY: 6.5,
        };
        self.d2d_target.DrawEllipse(&circle, &self.brush_icon, 1.4, None);
        let dot1 = D2D1_ELLIPSE { point: windows_numerics::Vector2 { X: cx - 2.5, Y: cy - 2.0 }, radiusX: 1.0, radiusY: 1.0 };
        let dot2 = D2D1_ELLIPSE { point: windows_numerics::Vector2 { X: cx + 2.5, Y: cy - 2.0 }, radiusX: 1.0, radiusY: 1.0 };
        let dot3 = D2D1_ELLIPSE { point: windows_numerics::Vector2 { X: cx, Y: cy + 2.5 }, radiusX: 1.0, radiusY: 1.0 };
        self.d2d_target.FillEllipse(&dot1, &self.brush_icon);
        self.d2d_target.FillEllipse(&dot2, &self.brush_icon);
        self.d2d_target.FillEllipse(&dot3, &self.brush_icon);
    }

    unsafe fn draw_visualizer_icon(&self, cx: f32, cy: f32) {
        let bars = [-4.5, -1.5, 1.5, 4.5];
        let heights = [3.5, 6.0, 4.5, 2.5];
        for (i, &bx) in bars.iter().enumerate() {
            let h = heights[i];
            self.d2d_target.DrawLine(
                pt(cx + bx, cy - h * 0.5),
                pt(cx + bx, cy + h * 0.5),
                &self.brush_icon,
                1.5,
                None,
            );
        }
    }

    unsafe fn draw_vinyl_icon(&self, cx: f32, cy: f32) {
        let outer = D2D1_ELLIPSE { point: windows_numerics::Vector2 { X: cx, Y: cy }, radiusX: 6.5, radiusY: 6.5 };
        let inner = D2D1_ELLIPSE { point: windows_numerics::Vector2 { X: cx, Y: cy }, radiusX: 3.5, radiusY: 3.5 };
        let hole = D2D1_ELLIPSE { point: windows_numerics::Vector2 { X: cx, Y: cy }, radiusX: 1.2, radiusY: 1.2 };
        self.d2d_target.DrawEllipse(&outer, &self.brush_icon, 1.3, None);
        self.d2d_target.DrawEllipse(&inner, &self.brush_icon, 1.0, None);
        self.d2d_target.FillEllipse(&hole, &self.brush_icon);
    }

    unsafe fn draw_cursor_icon(&self, cx: f32, cy: f32) {
        self.d2d_target.DrawLine(pt(cx - 4.5, cy - 4.5), pt(cx + 4.5, cy + 4.5), &self.brush_icon, 1.4, None);
        self.d2d_target.DrawLine(pt(cx + 4.5, cy + 4.5), pt(cx + 1.0, cy + 4.5), &self.brush_icon, 1.4, None);
        self.d2d_target.DrawLine(pt(cx + 4.5, cy + 4.5), pt(cx + 4.5, cy + 1.0), &self.brush_icon, 1.4, None);
    }

    unsafe fn draw_click_icon(&self, cx: f32, cy: f32) {
        let box_r = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: cx - 6.5, top: cy - 6.5, right: cx + 6.5, bottom: cy + 6.5 },
            radiusX: 1.5,
            radiusY: 1.5,
        };
        self.d2d_target.DrawRoundedRectangle(&box_r, &self.brush_icon, 1.4, None);
        self.d2d_target.DrawLine(pt(cx - 3.0, cy - 3.0), pt(cx + 3.0, cy + 3.0), &self.brush_icon, 1.4, None);
        self.d2d_target.DrawLine(pt(cx + 3.0, cy + 3.0), pt(cx + 3.0, cy + 0.5), &self.brush_icon, 1.4, None);
        self.d2d_target.DrawLine(pt(cx + 3.0, cy + 3.0), pt(cx + 0.5, cy + 3.0), &self.brush_icon, 1.4, None);
    }

    unsafe fn draw_startup_icon(&self, cx: f32, cy: f32) {
        let pc = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: cx - 6.5, top: cy - 6.0, right: cx + 6.5, bottom: cy + 2.5 },
            radiusX: 1.5,
            radiusY: 1.5,
        };
        self.d2d_target.DrawRoundedRectangle(&pc, &self.brush_icon, 1.4, None);
        self.d2d_target.DrawLine(pt(cx, cy + 2.5), pt(cx, cy + 5.5), &self.brush_icon, 1.4, None);
        self.d2d_target.DrawLine(pt(cx - 3.5, cy + 5.5), pt(cx + 3.5, cy + 5.5), &self.brush_icon, 1.4, None);
        let power = D2D1_ELLIPSE {
            point: windows_numerics::Vector2 { X: cx, Y: cy - 1.5 },
            radiusX: 1.8,
            radiusY: 1.8,
        };
        self.d2d_target.DrawEllipse(&power, &self.brush_accent, 1.2, None);
    }
}

pub fn open_settings_window(parent_hwnd: HWND) -> Result<()> {
    if SETTINGS_OPEN.swap(true, Ordering::SeqCst) {
        unsafe {
            let class_name = w!("DynamicIslandSettingsClass");
            if let Ok(existing_hwnd) = FindWindowW(class_name, None) {
                if !existing_hwnd.is_invalid() {
                    let _ = SetForegroundWindow(existing_hwnd);
                }
            }
        }
        return Ok(());
    }

    unsafe {
        let hinstance: HMODULE = GetModuleHandleW(None)?;
        let class_name = w!("DynamicIslandSettingsClass");

        let icon_h = LoadImageW(
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
            lpfnWndProc: Some(settings_wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance.into(),
            hIcon: icon_h,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: class_name,
            hIconSm: icon_sm,
        };

        RegisterClassExW(&wnd_class);

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let pos_x = (screen_w - CLIENT_WIDTH as i32) / 2;
        let pos_y = ((screen_h - CLIENT_HEIGHT as i32) / 2).max(40);

        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            class_name,
            w!("Dynamic Island Settings"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            pos_x,
            pos_y,
            CLIENT_WIDTH as i32 + 16,
            CLIENT_HEIGHT as i32 + 39,
            None,
            None,
            Some(HINSTANCE(hinstance.0)),
            None,
        )?;

        let dark_mode = BOOL(1);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode as *const _ as *const _,
            std::mem::size_of::<BOOL>() as u32,
        );

        let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(icon_h.0 as isize)));
        let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(icon_sm.0 as isize)));

        // Direct2D Setup
        let d2d_factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let render_props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U {
                width: CLIENT_WIDTH,
                height: CLIENT_HEIGHT,
            },
            presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
        };
        let d2d_target = d2d_factory.CreateHwndRenderTarget(&render_props, &hwnd_props)?;

        let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

        let format_title = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Display"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            16.0,
            w!("en-us"),
        )?;
        format_title.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;

        let format_subtitle = dwrite_factory.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            11.5,
            w!("en-us"),
        )?;
        format_subtitle.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;

        let format_tab = dwrite_factory.CreateTextFormat(
            w!("Segoe UI Variable Display"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            12.5,
            w!("en-us"),
        )?;
        format_tab.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        format_tab.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;

        let format_sec_title = dwrite_factory.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            10.5,
            w!("en-us"),
        )?;
        format_sec_title.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;

        let format_card_title = dwrite_factory.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            12.5,
            w!("en-us"),
        )?;
        format_card_title.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;

        let format_card_sub = dwrite_factory.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            10.5,
            w!("en-us"),
        )?;
        format_card_sub.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;

        let format_badge = dwrite_factory.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            11.0,
            w!("en-us"),
        )?;
        format_badge.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        format_badge.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;

        // Brushes
        let brush_card = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.10, g: 0.11, b: 0.13, a: 0.95 }, None)?;
        let brush_card_hover = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.14, g: 0.15, b: 0.18, a: 0.95 }, None)?;
        let brush_card_border = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.22, g: 0.23, b: 0.27, a: 0.85 }, None)?;
        let brush_card_selected_bg = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.08, g: 0.18, b: 0.32, a: 0.95 }, None)?;
        let brush_tab_active = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.48, b: 1.0, a: 0.20 }, None)?;
        let brush_accent = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.52, b: 1.0, a: 1.0 }, None)?;
        let brush_text_white = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.96, g: 0.96, b: 0.98, a: 1.0 }, None)?;
        let brush_text_sub = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.65, g: 0.67, b: 0.73, a: 0.90 }, None)?;
        let brush_toggle_on = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.12, g: 0.80, b: 0.42, a: 1.0 }, None)?;
        let brush_toggle_off = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.30, g: 0.32, b: 0.38, a: 0.90 }, None)?;
        let brush_badge_bg = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.08, g: 0.28, b: 0.16, a: 0.50 }, None)?;
        let brush_auto_saved = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.25, g: 0.90, b: 0.48, a: 1.0 }, None)?;
        let brush_icon = d2d_target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.45, g: 0.65, b: 0.95, a: 1.0 }, None)?;

        let cfg = AppConfig::load();
        let state = Box::new(SettingsState {
            parent_hwnd,
            d2d_target,
            format_title,
            format_subtitle,
            format_tab,
            format_sec_title,
            format_card_title,
            format_card_sub,
            format_badge,
            brush_card,
            brush_card_hover,
            brush_card_border,
            brush_card_selected_bg,
            brush_tab_active,
            brush_accent,
            brush_text_white,
            brush_text_sub,
            brush_toggle_on,
            brush_toggle_off,
            brush_badge_bg,
            brush_auto_saved,
            brush_icon,
            active_tab: SettingsTab::Appearance,
            top_margin: cfg.top_margin,
            duration: cfg.auto_collapse_secs,
            island_opacity: cfg.island_opacity,
            glow_intensity: cfg.glow_intensity,
            vinyl_speed: cfg.vinyl_speed,
            compact_music_width: cfg.compact_music_width,
            island_style: cfg.island_style,
            compact_view: cfg.compact_mode_view,
            dynamic_color: cfg.dynamic_album_color,
            enable_media: cfg.enable_media_island,
            enable_vinyl: cfg.enable_vinyl_animation,
            hover_expand: cfg.hover_expand_media,
            click_through: cfg.idle_click_through,
            smart_contour: cfg.smart_contour,
            contour_nudge_y: cfg.contour_nudge_y,
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
                let sub_utf: Vec<u16> = "Personalize layout, reactive visualizers & behavior".encode_utf16().collect();
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

                // 3. Tab Bar Navigation
                let tabs = [
                    (TAB_APPEARANCE, SettingsTab::Appearance, "🎨 Appearance"),
                    (TAB_MUSIC, SettingsTab::Music, "🎵 Music & Audio"),
                    (TAB_SYSTEM, SettingsTab::System, "⚙ System"),
                ];

                for (rect_f, tab, label) in tabs {
                    let is_active = state.active_tab == tab;
                    let tab_rect = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: rect_f.0, top: rect_f.1, right: rect_f.2, bottom: rect_f.3 },
                        radiusX: 8.0,
                        radiusY: 8.0,
                    };

                    if is_active {
                        state.d2d_target.FillRoundedRectangle(&tab_rect, &state.brush_tab_active);
                        state.d2d_target.DrawRoundedRectangle(&tab_rect, &state.brush_accent, 1.4, None);
                    } else {
                        state.d2d_target.FillRoundedRectangle(&tab_rect, &state.brush_card);
                        state.d2d_target.DrawRoundedRectangle(&tab_rect, &state.brush_card_border, 1.0, None);
                    }

                    let text_brush = if is_active { &state.brush_accent } else { &state.brush_text_sub };
                    let lbl_utf: Vec<u16> = label.encode_utf16().collect();
                    let text_rect = D2D_RECT_F { left: rect_f.0, top: rect_f.1, right: rect_f.2, bottom: rect_f.3 };
                    state.d2d_target.DrawText(&lbl_utf, &state.format_tab, &text_rect as *const _, text_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                }

                // 4. Tab Contents
                match state.active_tab {
                    SettingsTab::Appearance => {
                        // Section 1: ISLAND APPEARANCE & STYLE
                        let sec1_rect = D2D_RECT_F { left: 22.0, top: 114.0, right: 400.0, bottom: 130.0 };
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
                            state.d2d_target.DrawRoundedRectangle(&card_rect, border_brush, if is_selected { 1.6 } else { 1.0 }, None);

                            let name_utf: Vec<u16> = style.display_name().encode_utf16().collect();
                            let desc_utf: Vec<u16> = style.description().encode_utf16().collect();
                            let t_rect = D2D_RECT_F { left: rect_f.0 + 36.0, top: rect_f.1 + 8.0, right: rect_f.2 - 30.0, bottom: rect_f.1 + 26.0 };
                            let d_rect = D2D_RECT_F { left: rect_f.0 + 36.0, top: rect_f.1 + 27.0, right: rect_f.2 - 10.0, bottom: rect_f.1 + 43.0 };

                            state.d2d_target.DrawText(&name_utf, &state.format_card_title, &t_rect as *const _, &state.brush_text_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                            state.d2d_target.DrawText(&desc_utf, &state.format_card_sub, &d_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                            // Pill preview icon on left of card
                            let icon_r = D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: rect_f.0 + 12.0, top: rect_f.1 + 19.0, right: rect_f.0 + 28.0, bottom: rect_f.1 + 29.0 },
                                radiusX: 5.0,
                                radiusY: 5.0,
                            };
                            state.d2d_target.DrawRoundedRectangle(&icon_r, border_brush, 1.2, None);

                            if is_selected {
                                let check_cx = rect_f.2 - 18.0;
                                let check_cy = rect_f.1 + 18.0;
                                let dot = D2D1_ELLIPSE {
                                    point: windows_numerics::Vector2 { X: check_cx, Y: check_cy },
                                    radiusX: 7.0,
                                    radiusY: 7.0,
                                };
                                state.d2d_target.FillEllipse(&dot, &state.brush_accent);
                                state.d2d_target.DrawLine(pt(check_cx - 3.0, check_cy), pt(check_cx - 1.0, check_cy + 2.5), &state.brush_text_white, 1.5, None);
                                state.d2d_target.DrawLine(pt(check_cx - 1.0, check_cy + 2.5), pt(check_cx + 3.5, check_cy - 2.5), &state.brush_text_white, 1.5, None);
                            }
                        }

                        // Section 2: COMPACT MUSIC VIEW (IDLE BAR)
                        let sec2_rect = D2D_RECT_F { left: 22.0, top: 254.0, right: 400.0, bottom: 270.0 };
                        let sec2_utf: Vec<u16> = "COMPACT MUSIC VIEW (IDLE BAR)".encode_utf16().collect();
                        state.d2d_target.DrawText(&sec2_utf, &state.format_sec_title, &sec2_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let chips = [
                            (CHIP_VIEW_0, CompactDisplayMode::Combined, "Combined (Title + Wave)"),
                            (CHIP_VIEW_1, CompactDisplayMode::TitleOnly, "Title Only"),
                            (CHIP_VIEW_2, CompactDisplayMode::WaveOnly, "Wave Only"),
                        ];

                        for &(rect_f, mode, label) in &chips {
                            let is_selected = state.compact_view == mode;
                            let card_rect = D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: rect_f.0, top: rect_f.1, right: rect_f.2, bottom: rect_f.3 },
                                radiusX: 6.0,
                                radiusY: 6.0,
                            };
                            let bg_brush = if is_selected { &state.brush_card_selected_bg } else { &state.brush_card };
                            state.d2d_target.FillRoundedRectangle(&card_rect, bg_brush);
                            let border_brush = if is_selected { &state.brush_accent } else { &state.brush_card_border };
                            state.d2d_target.DrawRoundedRectangle(&card_rect, border_brush, if is_selected { 1.5 } else { 1.0 }, None);

                            let text_brush = if is_selected { &state.brush_text_white } else { &state.brush_text_sub };
                            let lbl_utf: Vec<u16> = label.encode_utf16().collect();
                            let text_rect = D2D_RECT_F { left: rect_f.0 + 8.0, top: rect_f.1 + 8.0, right: rect_f.2 - 8.0, bottom: rect_f.3 - 8.0 };
                            state.d2d_target.DrawText(&lbl_utf, &state.format_card_sub, &text_rect as *const _, text_brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                        }

                        // Section 3: FINE-TUNING SLIDERS
                        let sec3_rect = D2D_RECT_F { left: 22.0, top: 326.0, right: 400.0, bottom: 342.0 };
                        let sec3_utf: Vec<u16> = "FINE-TUNING CONTROLS".encode_utf16().collect();
                        state.d2d_target.DrawText(&sec3_utf, &state.format_sec_title, &sec3_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let sliders = [
                            (SLIDER_CARD_MARGIN, "Top Screen Margin", format!("{} px", state.top_margin as i32), (state.top_margin / 60.0).clamp(0.0, 1.0), 0),
                            (SLIDER_CARD_OPACITY, "Island Background Opacity", format!("{:.0} %", state.island_opacity * 100.0), ((state.island_opacity - 0.30) / 0.70).clamp(0.0, 1.0), 2),
                            (SLIDER_CARD_GLOW, "Border Glow & Halo Intensity", format!("{:.0} %", state.glow_intensity * 100.0), state.glow_intensity.clamp(0.0, 1.0), 3),
                        ];

                        for (rect_f, title, val_str, norm, s_idx) in sliders {
                            let card_rect = D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: rect_f.0, top: rect_f.1, right: rect_f.2, bottom: rect_f.3 },
                                radiusX: 7.0,
                                radiusY: 7.0,
                            };
                            state.d2d_target.FillRoundedRectangle(&card_rect, &state.brush_card);
                            state.d2d_target.DrawRoundedRectangle(&card_rect, &state.brush_card_border, 1.0, None);

                            let icon_cx = rect_f.0 + 22.0;
                            let icon_cy = rect_f.1 + 19.0;
                            match s_idx {
                                0 => state.draw_desktop_icon(icon_cx, icon_cy),
                                2 => state.draw_palette_icon(icon_cx, icon_cy),
                                3 => state.draw_palette_icon(icon_cx, icon_cy),
                                _ => {}
                            }

                            let title_utf: Vec<u16> = title.encode_utf16().collect();
                            let t_rect = D2D_RECT_F { left: rect_f.0 + 38.0, top: rect_f.1 + 8.0, right: rect_f.2 - 80.0, bottom: rect_f.1 + 26.0 };
                            state.d2d_target.DrawText(&title_utf, &state.format_card_title, &t_rect as *const _, &state.brush_text_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                            let val_utf: Vec<u16> = val_str.encode_utf16().collect();
                            let v_rect = D2D_RECT_F { left: rect_f.2 - 90.0, top: rect_f.1 + 8.0, right: rect_f.2 - 14.0, bottom: rect_f.1 + 26.0 };
                            state.d2d_target.DrawText(&val_utf, &state.format_badge, &v_rect as *const _, &state.brush_accent, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                            let track_y = rect_f.1 + 35.0;
                            let track_x1 = rect_f.0 + 14.0;
                            let track_x2 = rect_f.2 - 14.0;

                            state.d2d_target.DrawLine(pt(track_x1, track_y), pt(track_x2, track_y), &state.brush_card_border, 3.5, None);
                            let fill_x = track_x1 + norm * (track_x2 - track_x1);
                            state.d2d_target.DrawLine(pt(track_x1, track_y), pt(fill_x, track_y), &state.brush_accent, 3.5, None);

                            let thumb = D2D1_ELLIPSE {
                                point: windows_numerics::Vector2 { X: fill_x, Y: track_y },
                                radiusX: 6.5,
                                radiusY: 6.5,
                            };
                            state.d2d_target.FillEllipse(&thumb, &state.brush_text_white);
                        }
                    }

                    SettingsTab::Music => {
                        // Section 1: MUSIC PLAYBACK & VISUALIZER
                        let sec1_rect = D2D_RECT_F { left: 22.0, top: 114.0, right: 400.0, bottom: 130.0 };
                        let sec1_utf: Vec<u16> = "MUSIC PLAYBACK & VISUALIZER".encode_utf16().collect();
                        state.d2d_target.DrawText(&sec1_utf, &state.format_sec_title, &sec1_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let toggles = [
                            (TOGGLE_CARD_DYNAMIC_COLOR, "Dynamic Album Color Accent", "Tint border, halo glow & waveform adaptively by album art", state.dynamic_color, 0),
                            (TOGGLE_CARD_AUDIO_VIZ, "Real-time Audio Visualizer (WASAPI)", "Reactive symmetric waveform driven by PC speaker sound", state.enable_media, 1),
                            (TOGGLE_CARD_VINYL_ANIM, "Spinning Vinyl Disc Animation", "Rotating vinyl record & album cover artwork during playback", state.enable_vinyl, 2),
                            (TOGGLE_CARD_HOVER_EXPAND, "Hover to Expand Media", "Smoothly expand compact island into full controls when hovered", state.hover_expand, 3),
                        ];

                        for (rect_f, title, desc, is_on, t_idx) in toggles {
                            let card_rect = D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: rect_f.0, top: rect_f.1, right: rect_f.2, bottom: rect_f.3 },
                                radiusX: 7.0,
                                radiusY: 7.0,
                            };
                            state.d2d_target.FillRoundedRectangle(&card_rect, &state.brush_card);
                            state.d2d_target.DrawRoundedRectangle(&card_rect, &state.brush_card_border, 1.0, None);

                            let icon_cx = rect_f.0 + 22.0;
                            let icon_cy = (rect_f.1 + rect_f.3) * 0.5;
                            match t_idx {
                                0 => state.draw_palette_icon(icon_cx, icon_cy),
                                1 => state.draw_visualizer_icon(icon_cx, icon_cy),
                                2 => state.draw_vinyl_icon(icon_cx, icon_cy),
                                3 => state.draw_cursor_icon(icon_cx, icon_cy),
                                _ => {}
                            }

                            let title_utf: Vec<u16> = title.encode_utf16().collect();
                            let t_rect = D2D_RECT_F { left: rect_f.0 + 38.0, top: rect_f.1 + 6.0, right: rect_f.2 - 60.0, bottom: rect_f.1 + 24.0 };
                            state.d2d_target.DrawText(&title_utf, &state.format_card_title, &t_rect as *const _, &state.brush_text_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                            let desc_utf: Vec<u16> = desc.encode_utf16().collect();
                            let d_rect = D2D_RECT_F { left: rect_f.0 + 38.0, top: rect_f.1 + 24.0, right: rect_f.2 - 60.0, bottom: rect_f.3 - 4.0 };
                            state.d2d_target.DrawText(&desc_utf, &state.format_card_sub, &d_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                            // Toggle Switch pill
                            let sw_r = rect_f.2 - 16.0;
                            let sw_l = sw_r - 36.0;
                            let sw_cy = (rect_f.1 + rect_f.3) * 0.5;
                            let sw_pill = D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: sw_l, top: sw_cy - 10.0, right: sw_r, bottom: sw_cy + 10.0 },
                                radiusX: 10.0,
                                radiusY: 10.0,
                            };
                            let sw_brush = if is_on { &state.brush_toggle_on } else { &state.brush_toggle_off };
                            state.d2d_target.FillRoundedRectangle(&sw_pill, sw_brush);

                            let thumb_x = if is_on { sw_r - 10.0 } else { sw_l + 10.0 };
                            let sw_thumb = D2D1_ELLIPSE {
                                point: windows_numerics::Vector2 { X: thumb_x, Y: sw_cy },
                                radiusX: 7.5,
                                radiusY: 7.5,
                            };
                            state.d2d_target.FillEllipse(&sw_thumb, &state.brush_text_white);
                        }

                        // Section 2: VINYL ROTATION SPEED
                        let sec2_rect = D2D_RECT_F { left: 22.0, top: 344.0, right: 400.0, bottom: 360.0 };
                        let sec2_utf: Vec<u16> = "VINYL ROTATION SPEED".encode_utf16().collect();
                        state.d2d_target.DrawText(&sec2_utf, &state.format_sec_title, &sec2_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let card_rect = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: SLIDER_CARD_VINYL_SPEED.0, top: SLIDER_CARD_VINYL_SPEED.1, right: SLIDER_CARD_VINYL_SPEED.2, bottom: SLIDER_CARD_VINYL_SPEED.3 },
                            radiusX: 7.0,
                            radiusY: 7.0,
                        };
                        state.d2d_target.FillRoundedRectangle(&card_rect, &state.brush_card);
                        state.d2d_target.DrawRoundedRectangle(&card_rect, &state.brush_card_border, 1.0, None);

                        state.draw_vinyl_icon(SLIDER_CARD_VINYL_SPEED.0 + 22.0, SLIDER_CARD_VINYL_SPEED.1 + 19.0);

                        let title_utf: Vec<u16> = "Rotation Rate (0.1x - 2.0x)".encode_utf16().collect();
                        let t_rect = D2D_RECT_F { left: SLIDER_CARD_VINYL_SPEED.0 + 38.0, top: SLIDER_CARD_VINYL_SPEED.1 + 8.0, right: SLIDER_CARD_VINYL_SPEED.2 - 80.0, bottom: SLIDER_CARD_VINYL_SPEED.1 + 26.0 };
                        state.d2d_target.DrawText(&title_utf, &state.format_card_title, &t_rect as *const _, &state.brush_text_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let val_str = format!("{:.1}x", state.vinyl_speed);
                        let val_utf: Vec<u16> = val_str.encode_utf16().collect();
                        let v_rect = D2D_RECT_F { left: SLIDER_CARD_VINYL_SPEED.2 - 90.0, top: SLIDER_CARD_VINYL_SPEED.1 + 8.0, right: SLIDER_CARD_VINYL_SPEED.2 - 14.0, bottom: SLIDER_CARD_VINYL_SPEED.1 + 26.0 };
                        state.d2d_target.DrawText(&val_utf, &state.format_badge, &v_rect as *const _, &state.brush_accent, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let norm = ((state.vinyl_speed - 0.1) / 1.9).clamp(0.0, 1.0);
                        let track_y = SLIDER_CARD_VINYL_SPEED.1 + 35.0;
                        let track_x1 = SLIDER_CARD_VINYL_SPEED.0 + 14.0;
                        let track_x2 = SLIDER_CARD_VINYL_SPEED.2 - 14.0;

                        state.d2d_target.DrawLine(pt(track_x1, track_y), pt(track_x2, track_y), &state.brush_card_border, 3.5, None);
                        let fill_x = track_x1 + norm * (track_x2 - track_x1);
                        state.d2d_target.DrawLine(pt(track_x1, track_y), pt(fill_x, track_y), &state.brush_accent, 3.5, None);

                        let thumb = D2D1_ELLIPSE {
                            point: windows_numerics::Vector2 { X: fill_x, Y: track_y },
                            radiusX: 6.5,
                            radiusY: 6.5,
                        };
                        state.d2d_target.FillEllipse(&thumb, &state.brush_text_white);

                        // Section 3: COMPACT MUSIC PILL WIDTH
                        let sec3_rect = D2D_RECT_F { left: 22.0, top: 418.0, right: 400.0, bottom: 434.0 };
                        let sec3_utf: Vec<u16> = "COMPACT MUSIC PILL WIDTH (IDLE)".encode_utf16().collect();
                        state.d2d_target.DrawText(&sec3_utf, &state.format_sec_title, &sec3_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let card_rect = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: SLIDER_CARD_COMPACT_WIDTH.0, top: SLIDER_CARD_COMPACT_WIDTH.1, right: SLIDER_CARD_COMPACT_WIDTH.2, bottom: SLIDER_CARD_COMPACT_WIDTH.3 },
                            radiusX: 7.0,
                            radiusY: 7.0,
                        };
                        state.d2d_target.FillRoundedRectangle(&card_rect, &state.brush_card);
                        state.d2d_target.DrawRoundedRectangle(&card_rect, &state.brush_card_border, 1.0, None);

                        state.draw_desktop_icon(SLIDER_CARD_COMPACT_WIDTH.0 + 22.0, SLIDER_CARD_COMPACT_WIDTH.1 + 19.0);

                        let title_utf: Vec<u16> = "Capsule Width (160px - 420px)".encode_utf16().collect();
                        let t_rect = D2D_RECT_F { left: SLIDER_CARD_COMPACT_WIDTH.0 + 38.0, top: SLIDER_CARD_COMPACT_WIDTH.1 + 8.0, right: SLIDER_CARD_COMPACT_WIDTH.2 - 80.0, bottom: SLIDER_CARD_COMPACT_WIDTH.1 + 26.0 };
                        state.d2d_target.DrawText(&title_utf, &state.format_card_title, &t_rect as *const _, &state.brush_text_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let val_str = format!("{:.0} px", state.compact_music_width);
                        let val_utf: Vec<u16> = val_str.encode_utf16().collect();
                        let v_rect = D2D_RECT_F { left: SLIDER_CARD_COMPACT_WIDTH.2 - 90.0, top: SLIDER_CARD_COMPACT_WIDTH.1 + 8.0, right: SLIDER_CARD_COMPACT_WIDTH.2 - 14.0, bottom: SLIDER_CARD_COMPACT_WIDTH.1 + 26.0 };
                        state.d2d_target.DrawText(&val_utf, &state.format_badge, &v_rect as *const _, &state.brush_accent, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let norm = ((state.compact_music_width - 160.0) / 260.0).clamp(0.0, 1.0);
                        let track_y = SLIDER_CARD_COMPACT_WIDTH.1 + 35.0;
                        let track_x1 = SLIDER_CARD_COMPACT_WIDTH.0 + 14.0;
                        let track_x2 = SLIDER_CARD_COMPACT_WIDTH.2 - 14.0;

                        state.d2d_target.DrawLine(pt(track_x1, track_y), pt(track_x2, track_y), &state.brush_card_border, 3.5, None);
                        let fill_x = track_x1 + norm * (track_x2 - track_x1);
                        state.d2d_target.DrawLine(pt(track_x1, track_y), pt(fill_x, track_y), &state.brush_accent, 3.5, None);

                        let thumb = D2D1_ELLIPSE {
                            point: windows_numerics::Vector2 { X: fill_x, Y: track_y },
                            radiusX: 6.5,
                            radiusY: 6.5,
                        };
                        state.d2d_target.FillEllipse(&thumb, &state.brush_text_white);
                    }

                    SettingsTab::System => {
                        // Section 1: SYSTEM INTEGRATION
                        let sec1_rect = D2D_RECT_F { left: 22.0, top: 114.0, right: 400.0, bottom: 130.0 };
                        let sec1_utf: Vec<u16> = "SYSTEM INTEGRATION & INPUT".encode_utf16().collect();
                        state.d2d_target.DrawText(&sec1_utf, &state.format_sec_title, &sec1_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let toggles = [
                            (TOGGLE_CARD_STARTUP, "Start with Windows", "Launch Dynamic Island automatically when system boots up", state.start_with_windows, 0),
                            (TOGGLE_CARD_CLICK_THROUGH, "Idle Click-Through", "Pass mouse clicks directly to browser tabs and windows underneath", state.click_through, 1),
                            (TOGGLE_CARD_SMART_CONTOUR, "Smart Contour (Dynamic Avoidance)", "Auto-glides island down to prevent obstructing browser tabs & active text", state.smart_contour, 2),
                        ];

                        for (rect_f, title, desc, is_on, t_idx) in toggles {
                            let card_rect = D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: rect_f.0, top: rect_f.1, right: rect_f.2, bottom: rect_f.3 },
                                radiusX: 7.0,
                                radiusY: 7.0,
                            };
                            state.d2d_target.FillRoundedRectangle(&card_rect, &state.brush_card);
                            state.d2d_target.DrawRoundedRectangle(&card_rect, &state.brush_card_border, 1.0, None);

                            let icon_cx = rect_f.0 + 22.0;
                            let icon_cy = (rect_f.1 + rect_f.3) * 0.5;
                            match t_idx {
                                0 => state.draw_startup_icon(icon_cx, icon_cy),
                                1 => state.draw_click_icon(icon_cx, icon_cy),
                                2 => state.draw_click_icon(icon_cx, icon_cy),
                                _ => {}
                            }

                            let title_utf: Vec<u16> = title.encode_utf16().collect();
                            let t_rect = D2D_RECT_F { left: rect_f.0 + 38.0, top: rect_f.1 + 6.0, right: rect_f.2 - 60.0, bottom: rect_f.1 + 24.0 };
                            state.d2d_target.DrawText(&title_utf, &state.format_card_title, &t_rect as *const _, &state.brush_text_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                            let desc_utf: Vec<u16> = desc.encode_utf16().collect();
                            let d_rect = D2D_RECT_F { left: rect_f.0 + 38.0, top: rect_f.1 + 24.0, right: rect_f.2 - 60.0, bottom: rect_f.3 - 4.0 };
                            state.d2d_target.DrawText(&desc_utf, &state.format_card_sub, &d_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                            // Toggle Switch pill
                            let sw_r = rect_f.2 - 16.0;
                            let sw_l = sw_r - 36.0;
                            let sw_cy = (rect_f.1 + rect_f.3) * 0.5;
                            let sw_pill = D2D1_ROUNDED_RECT {
                                rect: D2D_RECT_F { left: sw_l, top: sw_cy - 10.0, right: sw_r, bottom: sw_cy + 10.0 },
                                radiusX: 10.0,
                                radiusY: 10.0,
                            };
                            let sw_brush = if is_on { &state.brush_toggle_on } else { &state.brush_toggle_off };
                            state.d2d_target.FillRoundedRectangle(&sw_pill, sw_brush);

                            let thumb_x = if is_on { sw_r - 10.0 } else { sw_l + 10.0 };
                            let sw_thumb = D2D1_ELLIPSE {
                                point: windows_numerics::Vector2 { X: thumb_x, Y: sw_cy },
                                radiusX: 7.5,
                                radiusY: 7.5,
                            };
                            state.d2d_target.FillEllipse(&sw_thumb, &state.brush_text_white);
                        }

                        // Section 2: NOTIFICATION TIMEOUT
                        let sec2_rect = D2D_RECT_F { left: 22.0, top: 304.0, right: 400.0, bottom: 320.0 };
                        let sec2_utf: Vec<u16> = "NOTIFICATION DISPLAY DURATION".encode_utf16().collect();
                        state.d2d_target.DrawText(&sec2_utf, &state.format_sec_title, &sec2_rect as *const _, &state.brush_text_sub, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let card_rect = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: SLIDER_CARD_TIMEOUT.0, top: SLIDER_CARD_TIMEOUT.1, right: SLIDER_CARD_TIMEOUT.2, bottom: SLIDER_CARD_TIMEOUT.3 },
                            radiusX: 7.0,
                            radiusY: 7.0,
                        };
                        state.d2d_target.FillRoundedRectangle(&card_rect, &state.brush_card);
                        state.d2d_target.DrawRoundedRectangle(&card_rect, &state.brush_card_border, 1.0, None);

                        state.draw_timer_icon(SLIDER_CARD_TIMEOUT.0 + 22.0, SLIDER_CARD_TIMEOUT.1 + 19.0);

                        let title_utf: Vec<u16> = "Auto-Collapse Timeout".encode_utf16().collect();
                        let t_rect = D2D_RECT_F { left: SLIDER_CARD_TIMEOUT.0 + 38.0, top: SLIDER_CARD_TIMEOUT.1 + 8.0, right: SLIDER_CARD_TIMEOUT.2 - 80.0, bottom: SLIDER_CARD_TIMEOUT.1 + 26.0 };
                        state.d2d_target.DrawText(&title_utf, &state.format_card_title, &t_rect as *const _, &state.brush_text_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let val_str = format!("{:.1} s", state.duration);
                        let val_utf: Vec<u16> = val_str.encode_utf16().collect();
                        let v_rect = D2D_RECT_F { left: SLIDER_CARD_TIMEOUT.2 - 90.0, top: SLIDER_CARD_TIMEOUT.1 + 8.0, right: SLIDER_CARD_TIMEOUT.2 - 14.0, bottom: SLIDER_CARD_TIMEOUT.1 + 26.0 };
                        state.d2d_target.DrawText(&val_utf, &state.format_badge, &v_rect as *const _, &state.brush_accent, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let norm = ((state.duration - 2.0) / 13.0).clamp(0.0, 1.0);
                        let track_y = SLIDER_CARD_TIMEOUT.1 + 35.0;
                        let track_x1 = SLIDER_CARD_TIMEOUT.0 + 14.0;
                        let track_x2 = SLIDER_CARD_TIMEOUT.2 - 14.0;

                        state.d2d_target.DrawLine(pt(track_x1, track_y), pt(track_x2, track_y), &state.brush_card_border, 3.5, None);
                        let fill_x = track_x1 + norm * (track_x2 - track_x1);
                        state.d2d_target.DrawLine(pt(track_x1, track_y), pt(fill_x, track_y), &state.brush_accent, 3.5, None);

                        let thumb = D2D1_ELLIPSE {
                            point: windows_numerics::Vector2 { X: fill_x, Y: track_y },
                            radiusX: 6.5,
                            radiusY: 6.5,
                        };
                        state.d2d_target.FillEllipse(&thumb, &state.brush_text_white);
                    }
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

                // 1. Tab switches
                if in_rect(x, y, TAB_APPEARANCE) {
                    state.active_tab = SettingsTab::Appearance;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    return LRESULT(0);
                } else if in_rect(x, y, TAB_MUSIC) {
                    state.active_tab = SettingsTab::Music;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    return LRESULT(0);
                } else if in_rect(x, y, TAB_SYSTEM) {
                    state.active_tab = SettingsTab::System;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    return LRESULT(0);
                }

                match state.active_tab {
                    SettingsTab::Appearance => {
                        // Style cards
                        let style_cards = [
                            (CARD_STYLE_0, IslandStyle::SleekCapsule),
                            (CARD_STYLE_1, IslandStyle::BalancedPill),
                            (CARD_STYLE_2, IslandStyle::SpaciousCard),
                            (CARD_STYLE_3, IslandStyle::CompactMinimal),
                        ];
                        for &(rect_f, style) in &style_cards {
                            if in_rect(x, y, rect_f) {
                                state.island_style = style;
                                state.auto_save();
                                let _ = InvalidateRect(Some(hwnd), None, false);
                                return LRESULT(0);
                            }
                        }

                        // Compact view chips
                        let chips = [
                            (CHIP_VIEW_0, CompactDisplayMode::Combined),
                            (CHIP_VIEW_1, CompactDisplayMode::TitleOnly),
                            (CHIP_VIEW_2, CompactDisplayMode::WaveOnly),
                        ];
                        for &(rect_f, mode) in &chips {
                            if in_rect(x, y, rect_f) {
                                state.compact_view = mode;
                                state.auto_save();
                                let _ = InvalidateRect(Some(hwnd), None, false);
                                return LRESULT(0);
                            }
                        }

                        // Sliders in Tab 0
                        if in_rect(x, y, SLIDER_CARD_MARGIN) {
                            state.dragging_slider = Some(0);
                            let norm = ((x - (SLIDER_CARD_MARGIN.0 + 14.0)) / (SLIDER_CARD_MARGIN.2 - SLIDER_CARD_MARGIN.0 - 28.0)).clamp(0.0, 1.0);
                            state.top_margin = (norm * 60.0).round();
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
                        }
                    }

                    SettingsTab::Music => {
                        if in_rect(x, y, TOGGLE_CARD_DYNAMIC_COLOR) {
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
                        } else if in_rect(x, y, SLIDER_CARD_VINYL_SPEED) {
                            state.dragging_slider = Some(4);
                            let norm = ((x - (SLIDER_CARD_VINYL_SPEED.0 + 14.0)) / (SLIDER_CARD_VINYL_SPEED.2 - SLIDER_CARD_VINYL_SPEED.0 - 28.0)).clamp(0.0, 1.0);
                            state.vinyl_speed = ((0.1 + norm * 1.9) * 10.0).round() / 10.0;
                            state.auto_save();
                            SetCapture(hwnd);
                        } else if in_rect(x, y, SLIDER_CARD_COMPACT_WIDTH) {
                            state.dragging_slider = Some(5);
                            let norm = ((x - (SLIDER_CARD_COMPACT_WIDTH.0 + 14.0)) / (SLIDER_CARD_COMPACT_WIDTH.2 - SLIDER_CARD_COMPACT_WIDTH.0 - 28.0)).clamp(0.0, 1.0);
                            state.compact_music_width = ((160.0 + norm * 260.0) / 5.0).round() * 5.0;
                            state.auto_save();
                            SetCapture(hwnd);
                        }
                    }

                    SettingsTab::System => {
                        if in_rect(x, y, TOGGLE_CARD_STARTUP) {
                            state.start_with_windows = !state.start_with_windows;
                            crate::config::set_autostart_enabled(state.start_with_windows);
                            state.auto_save();
                        } else if in_rect(x, y, TOGGLE_CARD_CLICK_THROUGH) {
                            state.click_through = !state.click_through;
                            state.auto_save();
                        } else if in_rect(x, y, TOGGLE_CARD_SMART_CONTOUR) {
                            state.smart_contour = !state.smart_contour;
                            state.auto_save();
                        } else if in_rect(x, y, SLIDER_CARD_TIMEOUT) {
                            state.dragging_slider = Some(1);
                            let norm = ((x - (SLIDER_CARD_TIMEOUT.0 + 14.0)) / (SLIDER_CARD_TIMEOUT.2 - SLIDER_CARD_TIMEOUT.0 - 28.0)).clamp(0.0, 1.0);
                            state.duration = ((2.0 + norm * 13.0) * 10.0).round() / 10.0;
                            state.auto_save();
                            SetCapture(hwnd);
                        }
                    }
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
                        5 => {
                            let norm = ((x - (SLIDER_CARD_COMPACT_WIDTH.0 + 14.0)) / (SLIDER_CARD_COMPACT_WIDTH.2 - SLIDER_CARD_COMPACT_WIDTH.0 - 28.0)).clamp(0.0, 1.0);
                            state.compact_music_width = ((160.0 + norm * 260.0) / 5.0).round() * 5.0;
                            state.auto_save();
                        }
                        _ => {}
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
                } else if state.active_tab == SettingsTab::Appearance {
                    let old_hover = state.hovered_item;
                    let mut new_hover = None;

                    let style_cards = [CARD_STYLE_0, CARD_STYLE_1, CARD_STYLE_2, CARD_STYLE_3];
                    for (i, &rect_f) in style_cards.iter().enumerate() {
                        if in_rect(x, y, rect_f) {
                            new_hover = Some(i);
                            break;
                        }
                    }

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
                }
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsState;
            if !state_ptr.is_null() {
                let _ = Box::from_raw(state_ptr);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            SETTINGS_OPEN.store(false, Ordering::SeqCst);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
