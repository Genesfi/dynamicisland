#![allow(unsafe_op_in_unsafe_fn)]

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::*,
            Direct2D::Common::*,
            Direct3D::*,
            Direct3D11::*,
            DirectComposition::*,
            DirectWrite::*,
            Dxgi::*,
            Dxgi::Common::*,
            Gdi::*,
            Imaging::*,
        },
        System::Com::*,
    },
};

use crate::island::{Island, IslandMode};

pub struct Renderer {
    _d3d_device: ID3D11Device,
    _d3d_context: ID3D11DeviceContext,
    swapchain: IDXGISwapChain1,
    _dcomp_device: IDCompositionDevice,
    _dcomp_target: IDCompositionTarget,
    _dcomp_visual: IDCompositionVisual,
    d2d_context: ID2D1DeviceContext,
    d2d_factory: ID2D1Factory1,
    #[allow(dead_code)]
    dwrite_factory: IDWriteFactory,

    format_title: IDWriteTextFormat,
    format_body: IDWriteTextFormat,
    format_time: IDWriteTextFormat,
    format_badge: IDWriteTextFormat,
    format_clock: IDWriteTextFormat,
    format_date: IDWriteTextFormat,
    #[allow(dead_code)]
    format_hero: IDWriteTextFormat,
    format_display: IDWriteTextFormat,
    format_tag: IDWriteTextFormat,
    format_pill_tag: IDWriteTextFormat,
    format_lyrics: IDWriteTextFormat,
    format_lyrics_sub: IDWriteTextFormat,

    brush_bg: ID2D1SolidColorBrush,
    brush_border: ID2D1SolidColorBrush,
    brush_white: ID2D1SolidColorBrush,
    brush_subtext: ID2D1SolidColorBrush,
    brush_time: ID2D1SolidColorBrush,
    brush_badge: ID2D1SolidColorBrush,
    brush_idle_dot: ID2D1SolidColorBrush,
    brush_vinyl_base: ID2D1SolidColorBrush,
    brush_vinyl_groove: ID2D1SolidColorBrush,
    brush_vinyl_sheen: ID2D1SolidColorBrush,
    brush_vinyl_label: ID2D1SolidColorBrush,
    brush_btn_bg: ID2D1SolidColorBrush,
    brush_accent: ID2D1SolidColorBrush,

    pub accent_color: (f32, f32, f32),
    pub album_art: Option<ID2D1Bitmap>,
    pub last_cover_hash: u64,
    pub wic_factory: IWICImagingFactory,
    #[allow(dead_code)]
    pub icon_manager: crate::icons::IconManager,
    #[allow(dead_code)]
    pub cached_is_light: bool,
    #[allow(dead_code)]
    pub lum_frame_counter: u32,
}

#[inline]
fn fast_cover_hash(bytes: &[u8]) -> u64 {
    let mut h = bytes.len() as u64;
    for &b in bytes.iter().take(64) {
        h = h.wrapping_mul(31).wrapping_add(b as u64);
    }
    for &b in bytes.iter().rev().take(64) {
        h = h.wrapping_mul(31).wrapping_add(b as u64);
    }
    h
}

#[inline]
fn pt(x: f32, y: f32) -> windows_numerics::Vector2 {
    windows_numerics::Vector2 { X: x, Y: y }
}

impl Renderer {
    pub fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self> {
        unsafe {
            // 1. Initialize Direct3D 11 with BGRA support for D2D interop
            let mut d3d_device: Option<ID3D11Device> = None;
            let mut d3d_context: Option<ID3D11DeviceContext> = None;
            let mut feature_level = D3D_FEATURE_LEVEL_11_0;

            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut d3d_device),
                Some(&mut feature_level),
                Some(&mut d3d_context),
            )?;

            let d3d_device = d3d_device.unwrap();
            let d3d_context = d3d_context.unwrap();

            // 2. Query DXGI device and factory
            let dxgi_device: IDXGIDevice = d3d_device.cast()?;
            let dxgi_adapter: IDXGIAdapter = dxgi_device.GetAdapter()?;
            let dxgi_factory: IDXGIFactory2 = dxgi_adapter.GetParent()?;

            // 3. Create SwapChain with premultiplied alpha for true per-pixel transparency
            let swapchain_desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width,
                Height: height,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                Stereo: false.into(),
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                Flags: 0,
            };

            let swapchain =
                dxgi_factory.CreateSwapChainForComposition(&d3d_device, &swapchain_desc, None)?;

            // 4. Attach SwapChain to Window via DirectComposition
            let dcomp_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
            let dcomp_target = dcomp_device.CreateTargetForHwnd(hwnd, true)?;
            let dcomp_visual = dcomp_device.CreateVisual()?;
            dcomp_visual.SetContent(&swapchain)?;
            dcomp_target.SetRoot(&dcomp_visual)?;
            dcomp_device.Commit()?;

            // 5. Initialize Direct2D 1.1 Context
            let d2d_factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let d2d_device = d2d_factory.CreateDevice(&dxgi_device)?;
            let d2d_context = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

            let surface: IDXGISurface = swapchain.GetBuffer(0)?;
            let bitmap_props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: std::mem::ManuallyDrop::new(None),
            };

            let target_bitmap =
                d2d_context.CreateBitmapFromDxgiSurface(&surface, Some(&bitmap_props))?;
            d2d_context.SetTarget(&target_bitmap);

            // 6. DirectWrite Typography Formats
            let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let format_title = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                13.0,
                w!("en-us"),
            )?;
            format_title.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format_title.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

            let format_body = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                11.5,
                w!("en-us"),
            )?;
            format_body.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format_body.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

            let format_time = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                10.5,
                w!("en-us"),
            )?;
            format_time.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format_time.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING)?;
            format_time.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

            let format_badge = dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                14.0,
                w!("en-us"),
            )?;
            format_badge.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format_badge.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;

            let format_clock = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Display"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                12.5,
                w!("en-us"),
            )?;
            format_clock.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format_clock.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

            let format_date = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Display"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                11.0,
                w!("en-us"),
            )?;
            format_date.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format_date.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

            let format_hero = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Display"),
                None,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                18.0,
                w!("en-us"),
            )?;
            format_hero.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format_hero.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

            let format_display = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Display"),
                None,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                24.0,
                w!("en-us"),
            )?;
            format_display.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format_display.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

            let format_tag = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Text"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                9.0,
                w!("en-us"),
            )?;
            format_tag.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format_tag.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

            let format_pill_tag = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Text"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                9.5,
                w!("en-us"),
            )?;
            format_pill_tag.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format_pill_tag.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            format_pill_tag.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;

            let format_lyrics = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Display"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                13.0,
                w!("en-us"),
            )?;
            format_lyrics.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
            format_lyrics.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;

            let format_lyrics_sub = dwrite_factory.CreateTextFormat(
                w!("Segoe UI Variable Display"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                11.0,
                w!("en-us"),
            )?;
            format_lyrics_sub.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
            format_lyrics_sub.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;

            // 7. Solid Brushes
            let brush_bg = d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 0.05, g: 0.05, b: 0.06, a: 0.98 },
                None,
            )?;
            let brush_border = d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.14 },
                None,
            )?;
            let brush_white = d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                None,
            )?;
            let brush_subtext = d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 0.82, g: 0.82, b: 0.86, a: 0.90 },
                None,
            )?;
            let brush_time = d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 0.55, g: 0.55, b: 0.60, a: 0.80 },
                None,
            )?;
            let brush_badge = d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 0.0, g: 0.48, b: 1.0, a: 1.0 },
                None,
            )?;
            let brush_idle_dot = d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 0.20, g: 0.78, b: 0.35, a: 0.90 },
                None,
            )?;
            let brush_vinyl_base = d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 0.07, g: 0.07, b: 0.08, a: 1.0 },
                None,
            )?;
            let brush_vinyl_groove = d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 },
                None,
            )?;
            let brush_vinyl_sheen = d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.26 },
                None,
            )?;
            let brush_vinyl_label = d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 0.88, g: 0.11, b: 0.28, a: 1.0 },
                None,
            )?;
            let brush_btn_bg = d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 },
                None,
            )?;
            let brush_accent = d2d_context.CreateSolidColorBrush(
                &D2D1_COLOR_F { r: 0.0, g: 0.47, b: 0.83, a: 1.0 },
                None,
            )?;

            let wic_factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;

            Ok(Self {
                _d3d_device: d3d_device,
                _d3d_context: d3d_context,
                swapchain,
                _dcomp_device: dcomp_device,
                _dcomp_target: dcomp_target,
                _dcomp_visual: dcomp_visual,
                d2d_context,
                d2d_factory,
                dwrite_factory,
                format_title,
                format_body,
                format_time,
                format_badge,
                format_clock,
                format_date,
                format_hero,
                format_display,
                format_tag,
                format_pill_tag,
                format_lyrics,
                format_lyrics_sub,
                brush_bg,
                brush_border,
                brush_white,
                brush_subtext,
                brush_time,
                brush_badge,
                brush_idle_dot,
                brush_vinyl_base,
                brush_vinyl_groove,
                brush_vinyl_sheen,
                brush_vinyl_label,
                brush_btn_bg,
                brush_accent,
                accent_color: (0.20, 0.75, 1.0),
                album_art: None,
                last_cover_hash: 0,
                icon_manager: crate::icons::IconManager::new(wic_factory.clone()),
                wic_factory,
                cached_is_light: false,
                lum_frame_counter: 0,
            })
        }
    }

    pub fn clear_album_art(&mut self) {
        self.album_art = None;
        self.last_cover_hash = 0;
        self.accent_color = (0.20, 0.75, 1.0);
        unsafe {
            let _ = self.brush_accent.SetColor(&D2D1_COLOR_F {
                r: self.accent_color.0,
                g: self.accent_color.1,
                b: self.accent_color.2,
                a: 1.0,
            });
        }
    }

    pub fn set_album_art(&mut self, bytes: &[u8], dynamic_color: bool) {
        let hash = fast_cover_hash(bytes);
        if self.last_cover_hash == hash && self.album_art.is_some() {
            return;
        }
        self.last_cover_hash = hash;
        self.album_art = self.decode_image_bytes(bytes);
        if dynamic_color {
            if let Some(col) = self.extract_dominant_color(bytes) {
                self.accent_color = col;
                unsafe {
                    self.brush_accent.SetColor(&D2D1_COLOR_F {
                        r: col.0,
                        g: col.1,
                        b: col.2,
                        a: 1.0,
                    });
                }
            }
        }
    }

    pub fn extract_dominant_color(&self, bytes: &[u8]) -> Option<(f32, f32, f32)> {
        unsafe {
            let stream: IWICStream = self.wic_factory.CreateStream().ok()?;
            stream.InitializeFromMemory(bytes).ok()?;
            let decoder = self
                .wic_factory
                .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                .ok()?;
            let frame = decoder.GetFrame(0).ok()?;
            let converter = self.wic_factory.CreateFormatConverter().ok()?;
            converter
                .Initialize(
                    &frame,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )
                .ok()?;

            // Downscale to 16x16 thumbnail using SIMD/hardware-accelerated area averaging.
            // Takes <0.2ms and avoids allocating megabytes of raw image pixels on the UI thread!
            let scaler: IWICBitmapScaler = self.wic_factory.CreateBitmapScaler().ok()?;
            scaler
                .Initialize(&converter, 16, 16, WICBitmapInterpolationModeFant)
                .ok()?;

            let mut pixels = [0u8; 16 * 16 * 4];
            scaler
                .CopyPixels(std::ptr::null(), 16 * 4, &mut pixels)
                .ok()?;

            let mut sum_r = 0.0f32;
            let mut sum_g = 0.0f32;
            let mut sum_b = 0.0f32;
            let mut count = 0;

            for chunk in pixels.chunks_exact(4) {
                let b = chunk[0] as f32 / 255.0;
                let g = chunk[1] as f32 / 255.0;
                let r = chunk[2] as f32 / 255.0;
                let max_c = r.max(g).max(b);
                let min_c = r.min(g).min(b);

                if max_c > 0.15 && max_c < 0.95 {
                    let sat = (max_c - min_c) / max_c;
                    if sat > 0.18 {
                        sum_r += r;
                        sum_g += g;
                        sum_b += b;
                        count += 1;
                    }
                }
            }

            if count >= 8 {
                let avg_r = sum_r / (count as f32);
                let avg_g = sum_g / (count as f32);
                let avg_b = sum_b / (count as f32);
                let max_v = avg_r.max(avg_g).max(avg_b).max(0.01);
                let boost = (0.95 / max_v).min(1.4);
                Some((avg_r * boost, avg_g * boost, avg_b * boost))
            } else {
                None
            }
        }
    }

    fn decode_image_bytes(&self, bytes: &[u8]) -> Option<ID2D1Bitmap> {
        unsafe {
            let stream: IWICStream = self.wic_factory.CreateStream().ok()?;
            stream.InitializeFromMemory(bytes).ok()?;
            let decoder = self
                .wic_factory
                .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                .ok()?;
            let frame = decoder.GetFrame(0).ok()?;
            let converter = self.wic_factory.CreateFormatConverter().ok()?;
            converter
                .Initialize(
                    &frame,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )
                .ok()?;
            let bmp1 = self.d2d_context.CreateBitmapFromWicBitmap(&converter, None).ok()?;
            Some(bmp1.into())
        }
    }

    /// Renders real album artwork as a modern 2-palette rounded square badge.
    fn draw_album_art_badge(
        &self,
        art: &ID2D1Bitmap,
        cx: f32,
        cy: f32,
        radius: f32,
        alpha: f32,
    ) {
        unsafe {
            let corner_r = (radius * 0.30).clamp(4.0, 6.0);
            let badge_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: cx - radius,
                    top: cy - radius,
                    right: cx + radius,
                    bottom: cy + radius,
                },
                radiusX: corner_r,
                radiusY: corner_r,
            };

            let bmp_size = art.GetSize();
            let min_dim = bmp_size.width.min(bmp_size.height).max(1.0);
            let scale = (radius * 2.0) / min_dim;

            let brush_transform = windows_numerics::Matrix3x2::translation(-bmp_size.width * 0.5, -bmp_size.height * 0.5)
                * windows_numerics::Matrix3x2::scale(scale, scale)
                * windows_numerics::Matrix3x2::translation(cx, cy);

            let bmp_props = D2D1_BITMAP_BRUSH_PROPERTIES1 {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
            };
            let brush_props = D2D1_BRUSH_PROPERTIES {
                opacity: alpha,
                transform: brush_transform,
            };

            if let Ok(bmp_brush) = self.d2d_context.CreateBitmapBrush(art, Some(&bmp_props), Some(&brush_props)) {
                self.d2d_context.FillRoundedRectangle(&badge_rect, &bmp_brush);
            }

            self.brush_border.SetColor(&D2D1_COLOR_F {
                r: self.accent_color.0,
                g: self.accent_color.1,
                b: self.accent_color.2,
                a: 0.85 * alpha,
            });
            self.d2d_context.DrawRoundedRectangle(&badge_rect, &self.brush_border, 1.2, None);
        }
    }

    /// Renders the app notification badge: prioritizes album art, bespoke vector brand badges,
    /// real extracted Windows application icons, and finally fallback initial letter squircles.
    fn draw_notification_badge(
        &mut self,
        notif: &crate::island::Notification,
        is_media_notif: bool,
        cx: f32,
        cy: f32,
        radius: f32,
        alpha: f32,
    ) {
        // 1. If it's a media notification and we have album art, draw the album art!
        if is_media_notif {
            if let Some(art) = &self.album_art {
                self.draw_album_art_badge(art, cx, cy, radius, alpha);
                return;
            }
        }

        // 2. If it's one of our bespoke vector brand badges (Netflix, YouTube, Discord, WhatsApp, Spotify, Telegram, Edge)
        let lower = notif.app_name.to_lowercase();
        let is_bespoke_vector = lower.contains("netflix")
            || lower.contains("youtube")
            || lower.contains("discord")
            || lower.contains("whatsapp")
            || lower.contains("spotify")
            || lower.contains("telegram")
            || lower.contains("edge")
            || (lower.contains("music") && !lower.contains("amazon"));

        if is_bespoke_vector {
            self.draw_vector_app_icon(
                &notif.app_name,
                cx,
                cy,
                radius,
                &notif.badge_letter,
                notif.badge_color,
                alpha,
            );
            return;
        }

        // 3. For all other apps: retrieve real application icon!
        let real_icon = self.icon_manager.get_icon(
            &self.d2d_context,
            &notif.app_name,
            notif.aumid.as_deref(),
            notif.icon_bytes.as_deref(),
        );

        if let Some(ref bmp) = real_icon {
            self.draw_album_art_badge(bmp, cx, cy, radius, alpha);
        } else {
            // 4. Fallback to letter squircle
            self.draw_vector_app_icon(
                &notif.app_name,
                cx,
                cy,
                radius,
                &notif.badge_letter,
                notif.badge_color,
                alpha,
            );
        }
    }

    /// Renders modern 2-palette rounded square ("rounded kotak") vector app icons.
    fn draw_vector_app_icon(
        &self,
        app_name: &str,
        cx: f32,
        cy: f32,
        radius: f32,
        fallback_letter: &str,
        fallback_color: (f32, f32, f32),
        alpha: f32,
    ) {
        unsafe {
            let lower = app_name.to_lowercase();
            let corner_r = (radius * 0.30).clamp(4.0, 6.0);
            let squircle = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: cx - radius,
                    top: cy - radius,
                    right: cx + radius,
                    bottom: cy + radius,
                },
                radiusX: corner_r,
                radiusY: corner_r,
            };

            if lower.contains("netflix") {
                // Netflix: Iconic Deep Charcoal Squircle (#141414) + Signature Netflix Red "N" (#E50914)
                self.brush_badge.SetColor(&D2D1_COLOR_F { r: 0.08, g: 0.08, b: 0.08, a: alpha });
                self.d2d_context.FillRoundedRectangle(&squircle, &self.brush_badge);

                self.brush_badge.SetColor(&D2D1_COLOR_F { r: 0.898, g: 0.035, b: 0.078, a: alpha });
                let nh = radius * 0.58;
                let nw = radius * 0.40;
                // Left vertical stem
                self.d2d_context.DrawLine(pt(cx - nw, cy - nh), pt(cx - nw, cy + nh), &self.brush_badge, 2.0, None);
                // Right vertical stem
                self.d2d_context.DrawLine(pt(cx + nw, cy - nh), pt(cx + nw, cy + nh), &self.brush_badge, 2.0, None);
                // Diagonal ribbon
                self.d2d_context.DrawLine(pt(cx - nw, cy - nh), pt(cx + nw, cy + nh), &self.brush_badge, 2.2, None);
            } else if lower.contains("youtube") || (lower.contains("music") && !lower.contains("spotify")) {
                // 1. YouTube / YouTube Music: YouTube Red background (#FF0000)
                self.brush_badge.SetColor(&D2D1_COLOR_F { r: 0.95, g: 0.10, b: 0.10, a: alpha });
                self.d2d_context.FillRoundedRectangle(&squircle, &self.brush_badge);

                // Palette 2: Crisp pure white outer play circle + inner play triangle
                self.brush_white.SetOpacity(0.95 * alpha);
                let ring_r = radius * 0.52;
                let ring = D2D1_ELLIPSE {
                    point: windows_numerics::Vector2 { X: cx, Y: cy },
                    radiusX: ring_r,
                    radiusY: ring_r,
                };
                self.d2d_context.DrawEllipse(&ring, &self.brush_white, 1.4, None);

                let tr = radius * 0.28;
                let p_top = pt(cx - tr * 0.7, cy - tr);
                let p_tip = pt(cx + tr * 1.1, cy);
                let p_bot = pt(cx - tr * 0.7, cy + tr);
                self.d2d_context.DrawLine(p_top, p_tip, &self.brush_white, 1.6, None);
                self.d2d_context.DrawLine(p_tip, p_bot, &self.brush_white, 1.6, None);
                self.d2d_context.DrawLine(p_bot, p_top, &self.brush_white, 1.6, None);
            } else if lower.contains("discord") {
                // Discord Blurple (#5865F2)
                self.brush_badge.SetColor(&D2D1_COLOR_F { r: 0.345, g: 0.396, b: 0.949, a: alpha });
                self.d2d_context.FillRoundedRectangle(&squircle, &self.brush_badge);

                // Palette 2: Pure White controller mask + cutouts
                self.brush_white.SetOpacity(0.95 * alpha);
                let face_w = radius * 0.58;
                let face_h = radius * 0.38;
                let face_rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: cx - face_w,
                        top: cy - face_h * 0.8,
                        right: cx + face_w,
                        bottom: cy + face_h * 1.1,
                    },
                    radiusX: 3.5,
                    radiusY: 3.5,
                };
                self.d2d_context.FillRoundedRectangle(&face_rect, &self.brush_white);

                // Controller ears
                self.d2d_context.DrawLine(pt(cx - face_w * 0.8, cy - face_h * 0.6), pt(cx - face_w * 0.6, cy - face_h * 1.4), &self.brush_white, 1.6, None);
                self.d2d_context.DrawLine(pt(cx + face_w * 0.8, cy - face_h * 0.6), pt(cx + face_w * 0.6, cy - face_h * 1.4), &self.brush_white, 1.6, None);

                // Cut-out Blurple eyes
                let eye_left = D2D1_ELLIPSE {
                    point: windows_numerics::Vector2 { X: cx - radius * 0.28, Y: cy + 0.5 },
                    radiusX: 1.8,
                    radiusY: 1.8,
                };
                let eye_right = D2D1_ELLIPSE {
                    point: windows_numerics::Vector2 { X: cx + radius * 0.28, Y: cy + 0.5 },
                    radiusX: 1.8,
                    radiusY: 1.8,
                };
                self.d2d_context.FillEllipse(&eye_left, &self.brush_badge);
                self.d2d_context.FillEllipse(&eye_right, &self.brush_badge);
            } else if lower.contains("whatsapp") {
                // WhatsApp Emerald (#25D366)
                self.brush_badge.SetColor(&D2D1_COLOR_F { r: 0.145, g: 0.827, b: 0.400, a: alpha });
                self.d2d_context.FillRoundedRectangle(&squircle, &self.brush_badge);

                // Palette 2: Pure White speech bubble
                self.brush_white.SetOpacity(0.95 * alpha);
                let bubble_w = radius * 0.56;
                let bubble_h = radius * 0.44;
                let bubble_rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: cx - bubble_w,
                        top: cy - bubble_h,
                        right: cx + bubble_w,
                        bottom: cy + bubble_h * 0.7,
                    },
                    radiusX: 4.0,
                    radiusY: 4.0,
                };
                self.d2d_context.FillRoundedRectangle(&bubble_rect, &self.brush_white);

                // Tail
                let p1 = pt(cx - bubble_w * 0.5, cy + bubble_h * 0.5);
                let p2 = pt(cx - bubble_w * 0.9, cy + bubble_h * 1.2);
                let p3 = pt(cx - bubble_w * 0.1, cy + bubble_h * 0.7);
                self.d2d_context.DrawLine(p1, p2, &self.brush_white, 1.8, None);
                self.d2d_context.DrawLine(p2, p3, &self.brush_white, 1.8, None);

                // 3 Emerald Dots
                for &offset in &[-3.2, 0.0, 3.2] {
                    let dot = D2D1_ELLIPSE {
                        point: windows_numerics::Vector2 { X: cx + offset, Y: cy - 1.0 },
                        radiusX: 1.2,
                        radiusY: 1.2,
                    };
                    self.d2d_context.FillEllipse(&dot, &self.brush_badge);
                }
            } else if lower.contains("spotify") {
                // Spotify Vibrant Green (#1DB954)
                self.brush_badge.SetColor(&D2D1_COLOR_F { r: 0.114, g: 0.725, b: 0.329, a: alpha });
                self.d2d_context.FillRoundedRectangle(&squircle, &self.brush_badge);

                // Palette 2: White 3 soundwave arcs
                self.brush_white.SetOpacity(0.95 * alpha);
                for (r_arc, w_arc, dy) in [(radius * 0.62, 1.8, -2.5), (radius * 0.44, 1.6, 0.5), (radius * 0.26, 1.4, 3.2)] {
                    self.d2d_context.DrawLine(
                        pt(cx - r_arc * 0.75, cy + dy + 1.5),
                        pt(cx, cy + dy - 1.2),
                        &self.brush_white,
                        w_arc,
                        None,
                    );
                    self.d2d_context.DrawLine(
                        pt(cx, cy + dy - 1.2),
                        pt(cx + r_arc * 0.75, cy + dy + 1.5),
                        &self.brush_white,
                        w_arc,
                        None,
                    );
                }
            } else if lower.contains("telegram") {
                // Telegram Bright Cyan (#24A1DE)
                self.brush_badge.SetColor(&D2D1_COLOR_F { r: 0.141, g: 0.631, b: 0.871, a: alpha });
                self.d2d_context.FillRoundedRectangle(&squircle, &self.brush_badge);

                // Palette 2: Pure White paper airplane
                self.brush_white.SetOpacity(0.95 * alpha);
                let p_nose = pt(cx + radius * 0.55, cy - radius * 0.45);
                let p_left = pt(cx - radius * 0.55, cy + 0.5);
                let p_tail = pt(cx - radius * 0.20, cy + radius * 0.50);

                self.d2d_context.DrawLine(p_nose, p_left, &self.brush_white, 1.6, None);
                self.d2d_context.DrawLine(p_left, p_tail, &self.brush_white, 1.6, None);
                self.d2d_context.DrawLine(p_tail, p_nose, &self.brush_white, 1.6, None);
                self.d2d_context.DrawLine(p_nose, pt(cx - radius * 0.12, cy + radius * 0.18), &self.brush_white, 1.4, None);
            } else if lower.contains("edge") || lower.contains("chrome") || lower.contains("browser") {
                // Azure Browser Blue (#0078D4)
                self.brush_badge.SetColor(&D2D1_COLOR_F { r: 0.0, g: 0.471, b: 0.831, a: alpha });
                self.d2d_context.FillRoundedRectangle(&squircle, &self.brush_badge);

                // Palette 2: Pure White globe
                self.brush_white.SetOpacity(0.95 * alpha);
                let globe_r = radius * 0.52;
                let globe = D2D1_ELLIPSE {
                    point: windows_numerics::Vector2 { X: cx, Y: cy },
                    radiusX: globe_r,
                    radiusY: globe_r,
                };
                self.d2d_context.DrawEllipse(&globe, &self.brush_white, 1.3, None);
                self.d2d_context.DrawLine(pt(cx - globe_r, cy), pt(cx + globe_r, cy), &self.brush_white, 1.2, None);
                self.d2d_context.DrawLine(pt(cx, cy - globe_r), pt(cx, cy + globe_r), &self.brush_white, 1.2, None);
                let inner_meridian = D2D1_ELLIPSE {
                    point: windows_numerics::Vector2 { X: cx, Y: cy },
                    radiusX: globe_r * 0.5,
                    radiusY: globe_r,
                };
                self.d2d_context.DrawEllipse(&inner_meridian, &self.brush_white, 1.1, None);
            } else {
                // Fallback: 2-Palette Solid Brand Rounded Square + Crisp Pure White Letter
                self.brush_badge.SetColor(&D2D1_COLOR_F {
                    r: fallback_color.0,
                    g: fallback_color.1,
                    b: fallback_color.2,
                    a: alpha,
                });
                self.d2d_context.FillRoundedRectangle(&squircle, &self.brush_badge);

                let text_rect = D2D_RECT_F {
                    left: cx - radius,
                    top: cy - radius,
                    right: cx + radius,
                    bottom: cy + radius,
                };
                let utf16: Vec<u16> = fallback_letter.encode_utf16().collect();
                self.brush_white.SetOpacity(alpha);
                self.d2d_context.DrawText(
                    &utf16,
                    &self.format_badge,
                    &text_rect as *const _,
                    &self.brush_white,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    /// Detects if the screen background behind the island is predominantly light or dark.
    #[allow(dead_code)]
    pub fn is_background_light(&self, screen_x: i32, screen_y: i32, width: i32, height: i32) -> bool {
        unsafe {
            let hdc = GetDC(None);
            if hdc.is_invalid() {
                return false;
            }
            let mut total_lum = 0.0f32;
            let mut sample_count = 0;
            let sample_xs = [0.20f32, 0.40, 0.60, 0.80];
            let sample_ys = [0.35f32, 0.65];
            for &ry in &sample_ys {
                let cy = screen_y + ((height as f32) * ry) as i32;
                for &rx in &sample_xs {
                    let cx = screen_x + ((width as f32) * rx) as i32;
                    let col = GetPixel(hdc, cx, cy);
                    if col.0 == 0xFFFFFFFF {
                        continue;
                    }
                    let red = (col.0 & 0xFF) as f32;
                    let green = ((col.0 >> 8) & 0xFF) as f32;
                    let blue = ((col.0 >> 16) & 0xFF) as f32;
                    let lum = (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255.0;
                    total_lum += lum;
                    sample_count += 1;
                }
            }
            let _ = ReleaseDC(None, hdc);
            if sample_count == 0 {
                return false;
            }
            let avg_lum = total_lum / (sample_count as f32);
            avg_lum > 0.44
        }
    }

    /// Renders a perfectly circular, center-cropped, rotating album art image.
    fn draw_rotating_album_circle(
        &self,
        art: &ID2D1Bitmap,
        cx: f32,
        cy: f32,
        radius: f32,
        rotation_rad: f32,
        alpha: f32,
    ) {
        unsafe {
            let circle = D2D1_ELLIPSE {
                point: windows_numerics::Vector2 { X: cx, Y: cy },
                radiusX: radius,
                radiusY: radius,
            };

            let bmp_size = art.GetSize();
            let min_dim = bmp_size.width.min(bmp_size.height).max(1.0);
            let scale = (radius * 2.0) / min_dim;

            let brush_transform = windows_numerics::Matrix3x2::translation(-bmp_size.width * 0.5, -bmp_size.height * 0.5)
                * windows_numerics::Matrix3x2::scale(scale, scale)
                * windows_numerics::Matrix3x2::translation(cx, cy);

            let bmp_props = D2D1_BITMAP_BRUSH_PROPERTIES1 {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
            };
            let brush_props = D2D1_BRUSH_PROPERTIES {
                opacity: alpha,
                transform: brush_transform,
            };

            if let Ok(bmp_brush) = self.d2d_context.CreateBitmapBrush(art, Some(&bmp_props), Some(&brush_props)) {
                let angle_deg = rotation_rad.to_degrees();
                let center = windows_numerics::Vector2 { X: cx, Y: cy };
                self.d2d_context.SetTransform(&windows_numerics::Matrix3x2::rotation_around(angle_deg, center));
                self.d2d_context.FillEllipse(&circle, &bmp_brush);
                self.d2d_context.SetTransform(&windows_numerics::Matrix3x2::identity());
            }
        }
    }

    /// Renders realistic spinning vinyl disc with microgrooves and anisotropic specular light sheen.
    fn draw_spinning_vinyl(&self, cx: f32, cy: f32, radius: f32, rotation: f32, alpha: f32) {
        unsafe {
            // 1. Vinyl disc base (Deep black)
            let disc = D2D1_ELLIPSE {
                point: windows_numerics::Vector2 { X: cx, Y: cy },
                radiusX: radius,
                radiusY: radius,
            };
            self.brush_vinyl_base.SetOpacity(alpha);
            self.d2d_context.FillEllipse(&disc, &self.brush_vinyl_base);

            // Subtle outer rim highlight
            self.brush_border.SetOpacity(0.35 * alpha);
            self.d2d_context.DrawEllipse(&disc, &self.brush_border, 1.0, None);

            // 2. Concentric microgrooves (4 fine rings)
            self.brush_vinyl_groove.SetOpacity(0.18 * alpha);
            for factor in [0.88, 0.76, 0.64, 0.52] {
                let groove = D2D1_ELLIPSE {
                    point: windows_numerics::Vector2 { X: cx, Y: cy },
                    radiusX: radius * factor,
                    radiusY: radius * factor,
                };
                self.d2d_context.DrawEllipse(&groove, &self.brush_vinyl_groove, 0.8, None);
            }

            // 3. Specular Light Sheen (Two rotating anisotropic reflection highlights at 180 deg)
            self.brush_vinyl_sheen.SetOpacity(0.24 * alpha);
            let cos_r = rotation.cos();
            let sin_r = rotation.sin();
            let r_start = radius * 0.44;
            let r_end = radius * 0.94;

            // Opposing highlight ray 1
            let p1_start = pt(cx + cos_r * r_start, cy + sin_r * r_start);
            let p1_end = pt(cx + cos_r * r_end, cy + sin_r * r_end);
            self.d2d_context.DrawLine(p1_start, p1_end, &self.brush_vinyl_sheen, 3.8, None);

            // Opposing highlight ray 2
            let p2_start = pt(cx - cos_r * r_start, cy - sin_r * r_start);
            let p2_end = pt(cx - cos_r * r_end, cy - sin_r * r_end);
            self.d2d_context.DrawLine(p2_start, p2_end, &self.brush_vinyl_sheen, 3.8, None);

            // 4. Center Record Label (REAL Rotating Album Artwork if available, or vibrant disc label!)
            if let Some(art) = &self.album_art {
                let art_r = radius * 0.84;
                self.draw_rotating_album_circle(art, cx, cy, art_r, rotation, alpha);

                let art_circle = D2D1_ELLIPSE {
                    point: windows_numerics::Vector2 { X: cx, Y: cy },
                    radiusX: art_r,
                    radiusY: art_r,
                };
                self.brush_border.SetColor(&D2D1_COLOR_F {
                    r: self.accent_color.0,
                    g: self.accent_color.1,
                    b: self.accent_color.2,
                    a: 0.85 * alpha,
                });
                self.d2d_context.DrawEllipse(&art_circle, &self.brush_border, 1.4, None);
            } else {
                let label_r = radius * 0.44;
                let label_disc = D2D1_ELLIPSE {
                    point: windows_numerics::Vector2 { X: cx, Y: cy },
                    radiusX: label_r,
                    radiusY: label_r,
                };
                self.brush_vinyl_label.SetOpacity(alpha);
                self.d2d_context.FillEllipse(&label_disc, &self.brush_vinyl_label);

                self.brush_white.SetOpacity(0.35 * alpha);
                self.d2d_context.DrawEllipse(&label_disc, &self.brush_white, 1.0, None);
            }

            // 5. Center spindle hole
            let hole_r = 2.4;
            let hole = D2D1_ELLIPSE {
                point: windows_numerics::Vector2 { X: cx, Y: cy },
                radiusX: hole_r,
                radiusY: hole_r,
            };
            self.brush_vinyl_base.SetOpacity(alpha);
            self.d2d_context.FillEllipse(&hole, &self.brush_vinyl_base);

            self.brush_white.SetOpacity(0.60 * alpha);
            self.d2d_context.DrawEllipse(&hole, &self.brush_white, 0.7, None);
        }
    }

    /// Renders dynamic Apple-style symmetric audio visualizer waveform with rounded capsule bars.
    fn draw_audio_visualizer(
        &self,
        cx: f32,
        cy: f32,
        peak: f32,
        phase: f32,
        bar_count: usize,
        max_h: f32,
        alpha: f32,
    ) {
        unsafe {
            let bar_width = 1.6;
            let bar_spacing = 1.9;
            let total_w = (bar_count as f32) * bar_width + ((bar_count - 1) as f32) * bar_spacing;
            let start_x = cx - total_w * 0.5;

            // Always use radiant dynamic album accent color!
            self.brush_accent.SetColor(&D2D1_COLOR_F {
                r: self.accent_color.0,
                g: self.accent_color.1,
                b: self.accent_color.2,
                a: alpha,
            });

            let center_idx = (bar_count as f32 - 1.0) * 0.5;
            let sigma = (center_idx * 0.60).max(1.0);

            for i in 0..bar_count {
                let bx = start_x + (i as f32) * (bar_width + bar_spacing);
                let dist = ((i as f32) - center_idx).abs();
                // Symmetric bell-curve envelope: center is tall, edges taper to delicate dots
                let envelope = (-0.5 * (dist / sigma).powi(2)).exp();

                let harmonic = ((phase * 2.8 + dist * 1.2).sin() * 0.5 + 0.5) * 0.45;
                let dynamic_factor = (peak * 1.10 + harmonic * 0.55).clamp(0.12, 1.0);

                let min_h = 2.4; // Rounded pill dot at edges
                let h = (min_h + dynamic_factor * envelope * (max_h - min_h)).max(min_h);

                let y1 = cy - h * 0.5;
                let y2 = cy + h * 0.5;

                let rounded_rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: bx,
                        top: y1,
                        right: bx + bar_width,
                        bottom: y2,
                    },
                    radiusX: bar_width * 0.5,
                    radiusY: bar_width * 0.5,
                };
                self.d2d_context.FillRoundedRectangle(&rounded_rect, &self.brush_accent);
            }
        }
    }

    fn draw_battery_ring(&self, cx: f32, cy: f32, battery: &crate::battery::BatteryStatus, alpha: f32) {
        unsafe {
            let r = 6.5;
            let ellipse = D2D1_ELLIPSE {
                point: pt(cx, cy),
                radiusX: r,
                radiusY: r,
            };
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.18 * alpha });
            self.d2d_context.DrawEllipse(&ellipse, &self.brush_btn_bg, 1.6, None);

            let pct = if battery.has_battery {
                (battery.percent as f32 / 100.0).clamp(0.05, 1.0)
            } else {
                1.0
            };
            let color = if battery.is_charging || !battery.has_battery {
                D2D1_COLOR_F { r: 0.20, g: 0.88, b: 0.45, a: 0.95 * alpha } // Electric green
            } else if battery.percent <= 20 {
                D2D1_COLOR_F { r: 1.0, g: 0.32, b: 0.28, a: 0.95 * alpha } // Red warning
            } else {
                D2D1_COLOR_F { r: 0.25, g: 0.78, b: 0.95, a: 0.90 * alpha } // Cyan
            };
            self.brush_accent.SetColor(&color);

            let fill_r = (r - 2.2) * pct.sqrt();
            if fill_r > 1.0 {
                let inner = D2D1_ELLIPSE {
                    point: pt(cx, cy),
                    radiusX: fill_r,
                    radiusY: fill_r,
                };
                self.d2d_context.FillEllipse(&inner, &self.brush_accent);
            }

            if battery.is_charging {
                self.brush_white.SetOpacity(0.95 * alpha);
                self.d2d_context.DrawLine(pt(cx + 0.8, cy - 3.2), pt(cx - 1.0, cy + 0.2), &self.brush_white, 1.1, None);
                self.d2d_context.DrawLine(pt(cx - 1.0, cy + 0.2), pt(cx + 0.4, cy + 0.2), &self.brush_white, 0.9, None);
                self.d2d_context.DrawLine(pt(cx + 0.4, cy + 0.2), pt(cx - 0.6, cy + 3.2), &self.brush_white, 1.1, None);
            }
        }
    }

    fn draw_shelf_top_bar(
        &self,
        island: &Island,
        config: &crate::config::AppConfig,
        left: f32,
        top: f32,
        right: f32,
        alpha: f32,
    ) {
        unsafe {
            // Module Title on Left
            let card_type = crate::island::ShelfCard::from_index(island.current_card);
            let title_str = card_type.title();
            let title_utf16: Vec<u16> = title_str.encode_utf16().collect();
            let title_rect = D2D_RECT_F {
                left: left + 22.0,
                top: top + 10.0,
                right: left + 160.0,
                bottom: top + 26.0,
            };
            self.brush_white.SetOpacity(0.95 * alpha);
            self.d2d_context.DrawText(
                &title_utf16,
                &self.format_title,
                &title_rect as *const _,
                &self.brush_white,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // Center: Battery Ring + English Date & Time (e.g. Sat 14 Nov • 20:32)
            let cx = (left + right) * 0.5;
            let st = windows::Win32::System::SystemInformation::GetLocalTime();
            let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
            let day_idx = (st.wDayOfWeek as usize).min(6);
            let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            let month_idx = (st.wMonth.saturating_sub(1) as usize).min(11);
            let date_time_str = format!(
                "{} {:02} {} • {:02}:{:02}",
                day_names[day_idx], st.wDay, month_names[month_idx], st.wHour, st.wMinute
            );
            let dt_utf16: Vec<u16> = date_time_str.encode_utf16().collect();

            // Center the combined (battery ring + date text)
            let dt_w = 124.0;
            let start_x = cx - (dt_w + 18.0) * 0.5;
            self.draw_battery_ring(start_x + 5.0, top + 18.0, &island.battery, alpha);

            let dt_rect = D2D_RECT_F {
                left: start_x + 16.0,
                top: top + 10.0,
                right: start_x + 16.0 + dt_w,
                bottom: top + 26.0,
            };
            self.brush_subtext.SetOpacity(0.85 * alpha);
            self.d2d_context.DrawText(
                &dt_utf16,
                &self.format_date,
                &dt_rect as *const _,
                &self.brush_subtext,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // Right: Mini Audio Visualizer + Collapse Button
            if config.enable_media_island && island.is_media_playing {
                self.draw_audio_visualizer(
                    right - 48.0,
                    top + 18.0,
                    island.audio_peak,
                    island.audio_phase,
                    7,
                    12.0,
                    alpha * 0.90,
                );
            }

            // Top-right collapse chevron button: "^"
            let chevron_x = right - 22.0;
            let chevron_y = top + 18.0;
            self.brush_white.SetOpacity(0.75 * alpha);
            self.d2d_context.DrawLine(
                pt(chevron_x - 4.5, chevron_y + 2.0),
                pt(chevron_x, chevron_y - 2.5),
                &self.brush_white,
                1.6,
                None,
            );
            self.d2d_context.DrawLine(
                pt(chevron_x, chevron_y - 2.5),
                pt(chevron_x + 4.5, chevron_y + 2.0),
                &self.brush_white,
                1.6,
                None,
            );
        }
    }

    fn draw_shelf_bottom_bar(
        &self,
        island: &Island,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        alpha: f32,
    ) {
        unsafe {
            let cx = (left + right) * 0.5;
            let py = bottom - 9.0;
            let cur_pos = island.card_scroll_spring.current;

            // 1. Sleek Apple-style Morphing Pagination Capsules (5 cards: 0..5)
            let count = crate::island::SHELF_CARD_COUNT;
            let spacing = 13.0;
            let total_width = (count as f32 - 1.0) * spacing;
            let start_x = cx - total_width * 0.5;

            for i in 0..count {
                let item_cx = start_x + (i as f32) * spacing;
                let dist = (cur_pos - i as f32).abs().min(1.0);
                let morph = 1.0 - dist;

                let w = 4.5 + 11.5 * morph;
                let h = 4.0;
                let rad = 2.0;
                let item_rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: item_cx - w * 0.5,
                        top: py - h * 0.5,
                        right: item_cx + w * 0.5,
                        bottom: py + h * 0.5,
                    },
                    radiusX: rad,
                    radiusY: rad,
                };
                let item_alpha = (0.28 + 0.68 * morph) * alpha;
                self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                self.brush_white.SetOpacity(item_alpha);
                self.d2d_context.FillRoundedRectangle(&item_rect, &self.brush_white);
            }

            // 2. Navigation Chevrons [ < ] and [ > ] on Left & Right Flanks (Prominent, High-Contrast Translucent Glass)
            let chevron_y = (top + bottom) * 0.5;
            let chev_r = 13.5;

            // Left chevron button at left + 18.0
            let left_chev_x = left + 18.0;
            let left_chev_bg = D2D1_ELLIPSE {
                point: pt(left_chev_x, chevron_y),
                radiusX: chev_r,
                radiusY: chev_r,
            };
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.18 * alpha });
            self.d2d_context.FillEllipse(&left_chev_bg, &self.brush_btn_bg);
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.32 * alpha });
            self.d2d_context.DrawEllipse(&left_chev_bg, &self.brush_btn_bg, 1.2, None);

            // Left chevron arrow: [ < ] (Crisp white stroke)
            self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
            self.brush_white.SetOpacity(0.98 * alpha);
            self.d2d_context.DrawLine(pt(left_chev_x + 2.5, chevron_y - 5.5), pt(left_chev_x - 3.0, chevron_y), &self.brush_white, 2.0, None);
            self.d2d_context.DrawLine(pt(left_chev_x - 3.0, chevron_y), pt(left_chev_x + 2.5, chevron_y + 5.5), &self.brush_white, 2.0, None);

            // Right chevron button at right - 18.0
            let right_chev_x = right - 18.0;
            let right_chev_bg = D2D1_ELLIPSE {
                point: pt(right_chev_x, chevron_y),
                radiusX: chev_r,
                radiusY: chev_r,
            };
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.18 * alpha });
            self.d2d_context.FillEllipse(&right_chev_bg, &self.brush_btn_bg);
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.32 * alpha });
            self.d2d_context.DrawEllipse(&right_chev_bg, &self.brush_btn_bg, 1.2, None);

            // Right chevron arrow: [ > ] (Crisp white stroke)
            self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
            self.brush_white.SetOpacity(0.98 * alpha);
            self.d2d_context.DrawLine(pt(right_chev_x - 2.5, chevron_y - 5.5), pt(right_chev_x + 3.0, chevron_y), &self.brush_white, 2.0, None);
            self.d2d_context.DrawLine(pt(right_chev_x + 3.0, chevron_y), pt(right_chev_x - 2.5, chevron_y + 5.5), &self.brush_white, 2.0, None);
        }
    }

    fn draw_card_today(
        &mut self,
        island: &Island,
        left: f32,
        top: f32,
        right: f32,
        _bottom: f32,
        alpha: f32,
    ) {
        unsafe {
            let cx = (left + right) * 0.5;
            let top_c = top + 24.0;
            let bottom_c = top + 124.0;
            let col_l = left + 44.0;
            let col_r = cx + 12.0;
            let right_c = right - 44.0;

            // Hairline vertical divider between columns
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.08 * alpha });
            self.d2d_context.DrawLine(pt(cx, top_c + 2.0), pt(cx, bottom_c - 2.0), &self.brush_btn_bg, 1.0, None);

            // --- Left Column: 3 Stacked Notification Cards (Matching 01_today_dashboard.png) ---
            let notif_w = cx - 12.0 - col_l;
            let card_h = 24.0;
            let spacing = 5.0;

            let dummy_notifs = [
                ("Rina", "Meeting moved to 3 PM", "now", D2D1_COLOR_F { r: 0.15, g: 0.83, b: 0.40, a: 0.95 * alpha }, 0),
                ("Mail", "Your order has shipped", "12m", D2D1_COLOR_F { r: 0.0, g: 0.47, b: 0.83, a: 0.95 * alpha }, 1),
                ("Calendar", "Weekly sync at 11:00", "1h", D2D1_COLOR_F { r: 1.0, g: 0.58, b: 0.0, a: 0.95 * alpha }, 2),
            ];

            for idx in 0..3 {
                let row_y = top_c + 2.0 + (idx as f32) * (card_h + spacing);
                let container_rect = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: col_l,
                        top: row_y,
                        right: col_l + notif_w,
                        bottom: row_y + card_h,
                    },
                    radiusX: 7.0,
                    radiusY: 7.0,
                };
                self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.06 * alpha });
                self.d2d_context.FillRoundedRectangle(&container_rect, &self.brush_btn_bg);

                let (app_title, body_text, time_str, icon_color, icon_type) = if idx == 0 {
                    if let Some(active) = &island.active_notification {
                        (active.app_name.as_str(), active.title.as_str(), active.time_str.as_str(), D2D1_COLOR_F { r: 0.15, g: 0.83, b: 0.40, a: 0.95 * alpha }, 0)
                    } else {
                        dummy_notifs[idx]
                    }
                } else {
                    dummy_notifs[idx]
                };

                // Vector app icon badge
                let icon_cx = col_l + 13.0;
                let icon_cy = row_y + card_h * 0.5;
                let icon_badge = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: icon_cx - 8.0,
                        top: icon_cy - 8.0,
                        right: icon_cx + 8.0,
                        bottom: icon_cy + 8.0,
                    },
                    radiusX: 3.8,
                    radiusY: 3.8,
                };
                self.brush_accent.SetColor(&icon_color);
                self.d2d_context.FillRoundedRectangle(&icon_badge, &self.brush_accent);

                // Draw custom white vector icon inside badge (strictly no emojis!)
                let white_color = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 * alpha };
                match icon_type {
                    0 => {
                        self.draw_speech_bubble_icon(icon_cx, icon_cy, false, alpha);
                    }
                    1 => {
                        self.draw_vector_envelope(icon_cx, icon_cy, 9.0, 6.5, &white_color);
                    }
                    _ => {
                        self.draw_vector_calendar_icon(icon_cx, icon_cy, 0.75, &white_color);
                    }
                }

                // App title / sender
                let title_utf: Vec<u16> = app_title.encode_utf16().collect();
                let title_rect = D2D_RECT_F {
                    left: col_l + 26.0,
                    top: row_y + 1.0,
                    right: col_l + notif_w - 38.0,
                    bottom: row_y + 13.0,
                };
                self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 * alpha });
                self.d2d_context.DrawText(&title_utf, &self.format_title, &title_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                // Snippet body
                let body_utf: Vec<u16> = body_text.encode_utf16().collect();
                let body_rect = D2D_RECT_F {
                    left: col_l + 26.0,
                    top: row_y + 12.0,
                    right: col_l + notif_w - 38.0,
                    bottom: row_y + card_h - 1.0,
                };
                self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.65, g: 0.68, b: 0.74, a: 0.85 * alpha });
                self.d2d_context.DrawText(&body_utf, &self.format_body, &body_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                // Time on right
                let time_utf: Vec<u16> = time_str.encode_utf16().collect();
                let time_rect = D2D_RECT_F {
                    left: col_l + notif_w - 36.0,
                    top: row_y + 4.0,
                    right: col_l + notif_w - 6.0,
                    bottom: row_y + 16.0,
                };
                self.brush_time.SetColor(&D2D1_COLOR_F { r: 0.50, g: 0.52, b: 0.58, a: 0.80 * alpha });
                self.d2d_context.DrawText(&time_utf, &self.format_date, &time_rect as *const _, &self.brush_time, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            }

            // --- Right Column: Mini Calendar & Task Status (Matching 01_today_dashboard.png) ---
            let days = ["M", "T", "W", "T", "F", "S", "S"];
            let (now_epoch, _off, y, m, d, _wday, _h, _mi) = crate::calendar::current_local_time();
            let (y, m, d) = (y as i64, m as i64, d as i64);
            let today_day_num = crate::calendar::days_from_civil(y, m, d);
            let today_wday = (crate::calendar::weekday(today_day_num) + 6) % 7; // Monday=0..Sunday=6
            let today_idx = today_wday as usize;

            let cal_w = right_c - col_r;
            let col_step = cal_w / 7.0;

            for (idx, day) in days.iter().enumerate() {
                let day_cx = col_r + (idx as f32 + 0.5) * col_step;

                // Day name (M T W T F S S)
                let day_utf = day.encode_utf16().collect::<Vec<u16>>();
                let d_rect = D2D_RECT_F { left: day_cx - 10.0, top: top_c + 2.0, right: day_cx + 10.0, bottom: top_c + 15.0 };
                self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.50, g: 0.52, b: 0.58, a: 0.80 * alpha });
                self.d2d_context.DrawText(&day_utf, &self.format_tag, &d_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                // Date number using Howard Hinnant's civil_from_days
                let date_cy = top_c + 26.0;
                let target_day_num = today_day_num - today_wday + idx as i64;
                let (_cy, _cm, target_day) = crate::calendar::civil_from_days(target_day_num);
                let num_str = format!("{}", target_day);
                let num_utf = num_str.encode_utf16().collect::<Vec<u16>>();

                if idx == today_idx {
                    // Highlight blue circle
                    let hl = D2D1_ELLIPSE { point: pt(day_cx, date_cy), radiusX: 9.0, radiusY: 9.0 };
                    self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.0, g: 0.48, b: 1.0, a: 0.95 * alpha });
                    self.d2d_context.FillEllipse(&hl, &self.brush_accent);
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 * alpha });
                } else {
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.92, a: 0.85 * alpha });
                }

                let n_rect = D2D_RECT_F { left: day_cx - 10.0, top: date_cy - 7.0, right: day_cx + 10.0, bottom: date_cy + 7.0 };
                self.d2d_context.DrawText(&num_utf, &self.format_body, &n_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                // Category dots under dates (blue=meeting, green=task, red=deadline, amber=event)
                let dot_y = date_cy + 13.0;
                let dot_color = if idx == today_idx {
                    if island.calendar.soon_meeting.is_some() {
                        Some(D2D1_COLOR_F { r: 0.0, g: 0.71, b: 0.85, a: 0.90 * alpha }) // Cyan for meeting
                    } else if !island.tasks.data.tasks.is_empty() {
                        Some(D2D1_COLOR_F { r: 0.18, g: 0.77, b: 0.71, a: 0.90 * alpha }) // Green for tasks
                    } else {
                        None
                    }
                } else {
                    match idx {
                        1 => Some(D2D1_COLOR_F { r: 0.0, g: 0.71, b: 0.85, a: 0.90 * alpha }),
                        4 => Some(D2D1_COLOR_F { r: 0.91, g: 0.11, b: 0.21, a: 0.90 * alpha }),
                        _ => None,
                    }
                };
                if let Some(col) = dot_color {
                    let cat_dot = D2D1_ELLIPSE { point: pt(day_cx, dot_y), radiusX: 1.8, radiusY: 1.8 };
                    self.brush_accent.SetColor(&col);
                    self.d2d_context.FillEllipse(&cat_dot, &self.brush_accent);
                }
            }

            // Bottom task / meeting summary container
            let sum_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: col_r, top: top_c + 49.0, right: right_c, bottom: top_c + 85.0 },
                radiusX: 8.0,
                radiusY: 8.0,
            };
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.06 * alpha });
            self.d2d_context.FillRoundedRectangle(&sum_rect, &self.brush_btn_bg);

            if let Some(meeting) = &island.calendar.soon_meeting {
                // Meeting alert banner with [Join] button
                let dot_m = D2D1_ELLIPSE { point: pt(col_r + 12.0, top_c + 60.0), radiusX: 3.0, radiusY: 3.0 };
                self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.0, g: 0.75, b: 1.0, a: 0.95 * alpha });
                self.d2d_context.FillEllipse(&dot_m, &self.brush_accent);

                let m_title = if meeting.title.len() > 16 {
                    format!("{}...", &meeting.title[..15])
                } else {
                    meeting.title.clone()
                };
                let m_title_utf: Vec<u16> = m_title.encode_utf16().collect();
                let mt_rect = D2D_RECT_F { left: col_r + 20.0, top: top_c + 53.0, right: right_c - 46.0, bottom: top_c + 67.0 };
                self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 * alpha });
                self.d2d_context.DrawText(&m_title_utf, &self.format_title, &mt_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                let mins_until = (meeting.start - now_epoch) / 60;
                let time_str = if mins_until <= 0 {
                    "Now".to_string()
                } else {
                    format!("Starts in {}m", mins_until)
                };
                let time_utf: Vec<u16> = time_str.encode_utf16().collect();
                let mtime_rect = D2D_RECT_F { left: col_r + 20.0, top: top_c + 67.0, right: right_c - 46.0, bottom: top_c + 81.0 };
                self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.40, g: 0.80, b: 1.0, a: 0.90 * alpha });
                self.d2d_context.DrawText(&time_utf, &self.format_date, &mtime_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                // [Join] pill button on the right
                let join_btn = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: right_c - 42.0, top: top_c + 56.0, right: right_c - 6.0, bottom: top_c + 78.0 },
                    radiusX: 6.0,
                    radiusY: 6.0,
                };
                self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.0, g: 0.48, b: 1.0, a: 0.90 * alpha });
                self.d2d_context.FillRoundedRectangle(&join_btn, &self.brush_accent);
                let join_txt = "Join".encode_utf16().collect::<Vec<u16>>();
                let jt_rect = D2D_RECT_F { left: right_c - 42.0, top: top_c + 59.0, right: right_c - 6.0, bottom: top_c + 75.0 };
                self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 * alpha });
                self.d2d_context.DrawText(&join_txt, &self.format_tag, &jt_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            } else {
                // Task summary
                let (completed, total) = island.tasks.stats();
                let dot_g = D2D1_ELLIPSE { point: pt(col_r + 12.0, top_c + 60.0), radiusX: 2.8, radiusY: 2.8 };
                self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.13, g: 0.87, b: 0.50, a: 0.95 * alpha });
                self.d2d_context.FillEllipse(&dot_g, &self.brush_accent);

                let sum_str = format!("{} / {} tasks today", completed, total);
                let sum_txt = sum_str.encode_utf16().collect::<Vec<u16>>();
                let st_rect = D2D_RECT_F { left: col_r + 20.0, top: top_c + 53.0, right: right_c - 6.0, bottom: top_c + 67.0 };
                self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 * alpha });
                self.d2d_context.DrawText(&sum_txt, &self.format_title, &st_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                let next_task_str = if let Some(first_pending) = island.tasks.next_pending_task() {
                    format!("Next: {}, {}", first_pending.title, first_pending.time_range)
                } else if !island.tasks.data.tasks.is_empty() {
                    "All tasks completed!".to_string()
                } else {
                    "No pending tasks".to_string()
                };
                let next_txt = next_task_str.encode_utf16().collect::<Vec<u16>>();
                let nt_rect = D2D_RECT_F { left: col_r + 20.0, top: top_c + 67.0, right: right_c - 6.0, bottom: top_c + 81.0 };
                self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.60, g: 0.63, b: 0.70, a: 0.85 * alpha });
                self.d2d_context.DrawText(&next_txt, &self.format_date, &nt_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
            }
        }
    }

    fn draw_card_todolist(
        &self,
        island: &Island,
        left: f32,
        top: f32,
        right: f32,
        _bottom: f32,
        alpha: f32,
    ) {
        unsafe {
            let top_c = top + 26.0;
            let col_l = left + 44.0;
            let right_c = right - 44.0;

            // --- Left Column: Stats, localized date, [+] Add Task, [Week/Month] View toggle ---
            let (completed, total) = island.tasks.stats();
            let count_str = format!("{}/{}", completed, total);
            let count_txt = count_str.encode_utf16().collect::<Vec<u16>>();
            let c_rect = D2D_RECT_F { left: col_l, top: top_c + 1.0, right: col_l + 54.0, bottom: top_c + 32.0 };
            self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 * alpha });
            self.d2d_context.DrawText(&count_txt, &self.format_display, &c_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            // Dark green circular badge with green vector calendar icon
            let cal_badge = D2D1_ELLIPSE { point: pt(col_l + 65.0, top_c + 16.0), radiusX: 11.0, radiusY: 11.0 };
            self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.08, g: 0.22, b: 0.13, a: 0.90 * alpha });
            self.d2d_context.FillEllipse(&cal_badge, &self.brush_accent);
            let green_color = D2D1_COLOR_F { r: 0.13, g: 0.87, b: 0.50, a: 0.95 * alpha };
            self.draw_vector_calendar_icon(col_l + 65.0, top_c + 16.0, 0.75, &green_color);

            let (_now_epoch, _off, y, m, d, _wday, h, mi) = crate::calendar::current_local_time();
            let (y, m, d) = (y as i64, m as i64, d as i64);
            let now_sec = (h as u32) * 3600 + (mi as u32) * 60;
            let day_num = crate::calendar::days_from_civil(y, m, d);
            let wday = crate::calendar::weekday(day_num) as usize;
            let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
            let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            let m_idx = (m.saturating_sub(1) as usize).min(11);
            let date_txt = format!("{}, {} {:02}", day_names[wday], month_names[m_idx], d).encode_utf16().collect::<Vec<u16>>();
            let d_rect = D2D_RECT_F { left: col_l, top: top_c + 32.0, right: col_l + 130.0, bottom: top_c + 46.0 };
            self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.60, g: 0.63, b: 0.70, a: 0.85 * alpha });
            self.d2d_context.DrawText(&date_txt, &self.format_date, &d_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            // Pill Button 1: (+) Add Task
            let btn_add = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: col_l, top: top_c + 54.0, right: col_l + 68.0, bottom: top_c + 76.0 },
                radiusX: 11.0,
                radiusY: 11.0,
            };
            let add_bg = if island.entering_task.is_some() {
                D2D1_COLOR_F { r: 0.0, g: 0.48, b: 1.0, a: 0.35 * alpha }
            } else {
                D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 * alpha }
            };
            self.brush_btn_bg.SetColor(&add_bg);
            self.d2d_context.FillRoundedRectangle(&btn_add, &self.brush_btn_bg);
            let white_btn_color = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 * alpha };
            self.draw_vector_plus_circle(col_l + 10.0, top_c + 65.0, 0.70, &white_btn_color);
            let add_txt = "Add Task".encode_utf16().collect::<Vec<u16>>();
            let at_rect = D2D_RECT_F { left: col_l + 18.0, top: top_c + 56.0, right: col_l + 66.0, bottom: top_c + 74.0 };
            self.brush_white.SetColor(&white_btn_color);
            self.d2d_context.DrawText(&add_txt, &self.format_tag, &at_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            // Pill Button 2: (🗓) View Toggle [Week | Month]
            let btn_view = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: col_l + 74.0, top: top_c + 54.0, right: col_l + 142.0, bottom: top_c + 76.0 },
                radiusX: 11.0,
                radiusY: 11.0,
            };
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 * alpha });
            self.d2d_context.FillRoundedRectangle(&btn_view, &self.brush_btn_bg);
            self.draw_vector_calendar_icon(col_l + 84.0, top_c + 65.0, 0.70, &white_btn_color);
            let view_label = match island.tasks.data.view {
                crate::tasks::CalendarView::Week => "Week View",
                crate::tasks::CalendarView::Month => "Month View",
            };
            let vt_utf: Vec<u16> = view_label.encode_utf16().collect();
            let vt_rect = D2D_RECT_F { left: col_l + 93.0, top: top_c + 56.0, right: col_l + 140.0, bottom: top_c + 74.0 };
            self.d2d_context.DrawText(&vt_utf, &self.format_tag, &vt_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            // --- Middle Column: Interactive Tasks Container ---
            let mid_left = col_l + 152.0;
            let mid_w = 142.0;
            let box_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: mid_left, top: top_c + 1.0, right: mid_left + mid_w, bottom: top_c + 84.0 },
                radiusX: 8.0,
                radiusY: 8.0,
            };
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.05 * alpha });
            self.d2d_context.FillRoundedRectangle(&box_rect, &self.brush_btn_bg);

            // If entering a task, show inline input box with blinking caret!
            if let Some(buf) = &island.entering_task {
                let caret = if (island.caret_timer * 2.0).fract() < 0.5 { "│" } else { " " };
                let input_str = if buf.is_empty() {
                    format!("New task...{}", caret)
                } else {
                    format!("{}{}", buf, caret)
                };
                let in_pill = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: mid_left + 4.0, top: top_c + 4.0, right: mid_left + mid_w - 4.0, bottom: top_c + 28.0 },
                    radiusX: 5.0,
                    radiusY: 5.0,
                };
                self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.0, g: 0.48, b: 1.0, a: 0.20 * alpha });
                self.d2d_context.FillRoundedRectangle(&in_pill, &self.brush_accent);

                let in_utf: Vec<u16> = input_str.encode_utf16().collect();
                let in_rect = D2D_RECT_F { left: mid_left + 10.0, top: top_c + 7.0, right: mid_left + mid_w - 6.0, bottom: top_c + 25.0 };
                self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 * alpha });
                self.d2d_context.DrawText(&in_utf, &self.format_body, &in_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                let hint_utf: Vec<u16> = "Press Enter to save, Esc to cancel".encode_utf16().collect();
                let hint_rect = D2D_RECT_F { left: mid_left + 6.0, top: top_c + 36.0, right: mid_left + mid_w - 6.0, bottom: top_c + 50.0 };
                self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.55, g: 0.58, b: 0.65, a: 0.75 * alpha });
                self.d2d_context.DrawText(&hint_utf, &self.format_pill_tag, &hint_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
            } else {
                // Normal task rendering (up to 3 items)
                let display_tasks = island.tasks.data.tasks.iter().take(3);
                for (idx, task) in display_tasks.enumerate() {
                    let row_y = top_c + 3.0 + (idx as f32) * 26.5;
                    let row_cy = row_y + 11.0;

                    let (cat_r, cat_g, cat_b) = match task.category {
                        0 => (0.0, 0.48, 1.0),   // Meeting: Blue
                        1 => (0.13, 0.87, 0.50), // Task: Green
                        2 => (0.95, 0.25, 0.30), // Deadline: Red
                        _ => (1.0, 0.65, 0.15),  // Event: Amber
                    };

                    let chk = D2D1_ELLIPSE { point: pt(mid_left + 12.0, row_cy), radiusX: 5.2, radiusY: 5.2 };

                    if task.completed {
                        // Background row pill for completed
                        let p_rect = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: mid_left + 4.0, top: row_y, right: mid_left + mid_w - 6.0, bottom: row_y + 24.0 },
                            radiusX: 6.0,
                            radiusY: 6.0,
                        };
                        self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.08, g: 0.20, b: 0.12, a: 0.70 * alpha });
                        self.d2d_context.FillRoundedRectangle(&p_rect, &self.brush_accent);

                        // Checkmark inside filled circle
                        self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.13, g: 0.87, b: 0.50, a: 0.95 * alpha });
                        self.d2d_context.FillEllipse(&chk, &self.brush_accent);
                        self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 * alpha });
                        self.d2d_context.DrawLine(pt(mid_left + 9.8, row_cy), pt(mid_left + 11.5, row_cy + 2.0), &self.brush_white, 1.2, None);
                        self.d2d_context.DrawLine(pt(mid_left + 11.5, row_cy + 2.0), pt(mid_left + 14.5, row_cy - 2.0), &self.brush_white, 1.2, None);

                        // Text with strikethrough
                        let t_utf: Vec<u16> = task.title.encode_utf16().collect();
                        let t_rect = D2D_RECT_F { left: mid_left + 22.0, top: row_y + 2.0, right: mid_left + mid_w - 20.0, bottom: row_y + 15.0 };
                        self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.55, g: 0.58, b: 0.65, a: 0.70 * alpha });
                        self.d2d_context.DrawText(&t_utf, &self.format_body, &t_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
                        // Strikethrough line
                        self.d2d_context.DrawLine(pt(mid_left + 22.0, row_cy - 1.0), pt(mid_left + mid_w - 26.0, row_cy - 1.0), &self.brush_subtext, 1.0, None);
                    } else {
                        // Uncompleted: category outline ring
                        self.brush_accent.SetColor(&D2D1_COLOR_F { r: cat_r, g: cat_g, b: cat_b, a: 0.95 * alpha });
                        self.d2d_context.DrawEllipse(&chk, &self.brush_accent, 1.4, None);

                        let t_utf: Vec<u16> = task.title.encode_utf16().collect();
                        let t_rect = D2D_RECT_F { left: mid_left + 22.0, top: row_y + 1.0, right: mid_left + mid_w - 20.0, bottom: row_y + 13.0 };
                        self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 * alpha });
                        self.d2d_context.DrawText(&t_utf, &self.format_body, &t_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                        let sub_utf: Vec<u16> = task.time_range.encode_utf16().collect();
                        let sub_rect = D2D_RECT_F { left: mid_left + 22.0, top: row_y + 12.0, right: mid_left + mid_w - 20.0, bottom: row_y + 23.0 };
                        self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.50, g: 0.53, b: 0.60, a: 0.80 * alpha });
                        self.d2d_context.DrawText(&sub_utf, &self.format_tag, &sub_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
                    }

                    // Delete X icon on far right of task row
                    let x_cx = mid_left + mid_w - 12.0;
                    self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.50, g: 0.53, b: 0.60, a: 0.40 * alpha });
                    self.d2d_context.DrawLine(pt(x_cx - 2.5, row_cy - 2.5), pt(x_cx + 2.5, row_cy + 2.5), &self.brush_subtext, 1.1, None);
                    self.d2d_context.DrawLine(pt(x_cx + 2.5, row_cy - 2.5), pt(x_cx - 2.5, row_cy + 2.5), &self.brush_subtext, 1.1, None);
                }
            }

            // --- Right Column: Real Dynamic Calendar & Timeline Bar ---
            let r_start = mid_left + mid_w + 14.0;
            let cal2_w = right_c - r_start;

            let display_month = ((m - 1 + island.tasks.data.month_offset as i64).rem_euclid(12) + 1) as u32;
            let display_year = (y as i32) + ((m - 1 + island.tasks.data.month_offset as i64).div_euclid(12)) as i32;
            let full_month_names = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
            let m_label_str = format!("{} {}", full_month_names[(display_month - 1) as usize], display_year);
            let m_label = m_label_str.encode_utf16().collect::<Vec<u16>>();
            let ml_rect = D2D_RECT_F { left: r_start, top: top_c + 1.0, right: right_c - 20.0, bottom: top_c + 14.0 };
            self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.95 * alpha });
            self.d2d_context.DrawText(&m_label, &self.format_tag, &ml_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            // Chevrons < > next to month
            let chev_m_y = top_c + 7.5;
            self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.60, g: 0.63, b: 0.70, a: 0.85 * alpha });
            self.d2d_context.DrawLine(pt(right_c - 16.0, chev_m_y - 3.0), pt(right_c - 19.0, chev_m_y), &self.brush_subtext, 1.1, None);
            self.d2d_context.DrawLine(pt(right_c - 19.0, chev_m_y), pt(right_c - 16.0, chev_m_y + 3.0), &self.brush_subtext, 1.1, None);
            self.d2d_context.DrawLine(pt(right_c - 8.0, chev_m_y - 3.0), pt(right_c - 5.0, chev_m_y), &self.brush_subtext, 1.1, None);
            self.d2d_context.DrawLine(pt(right_c - 5.0, chev_m_y), pt(right_c - 8.0, chev_m_y + 3.0), &self.brush_subtext, 1.1, None);

            match island.tasks.data.view {
                crate::tasks::CalendarView::Week => {
                    // Days S M T W T F S
                    let c2_step = cal2_w / 7.0;
                    let s_days = ["S", "M", "T", "W", "T", "F", "S"];
                    let today_day_num = crate::calendar::days_from_civil(y, m, d);
                    let today_wday = crate::calendar::weekday(today_day_num); // 0=Sun .. 6=Sat

                    for (i, day_sym) in s_days.iter().enumerate() {
                        let dx = r_start + (i as f32 + 0.5) * c2_step;
                        let d_utf = day_sym.encode_utf16().collect::<Vec<u16>>();
                        let d_rect = D2D_RECT_F { left: dx - 8.0, top: top_c + 17.0, right: dx + 8.0, bottom: top_c + 28.0 };
                        self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.50, g: 0.53, b: 0.60, a: 0.80 * alpha });
                        self.d2d_context.DrawText(&d_utf, &self.format_tag, &d_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                        let target_day_num = today_day_num - today_wday + i as i64;
                        let (_, _, day_val) = crate::calendar::civil_from_days(target_day_num);
                        let date_num = format!("{}", day_val);
                        let dn_utf = date_num.encode_utf16().collect::<Vec<u16>>();
                        let dn_cy = top_c + 38.0;

                        if i as i64 == today_wday && island.tasks.data.month_offset == 0 {
                            let active_c = D2D1_ELLIPSE { point: pt(dx, dn_cy), radiusX: 7.8, radiusY: 7.8 };
                            self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.13, g: 0.87, b: 0.50, a: 0.95 * alpha });
                            self.d2d_context.FillEllipse(&active_c, &self.brush_accent);
                            self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 * alpha });
                            // Category dots
                            let d1 = D2D1_ELLIPSE { point: pt(dx - 2.5, dn_cy + 10.5), radiusX: 1.3, radiusY: 1.3 };
                            let d2 = D2D1_ELLIPSE { point: pt(dx + 2.5, dn_cy + 10.5), radiusX: 1.3, radiusY: 1.3 };
                            self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.0, g: 0.48, b: 1.0, a: 0.95 * alpha });
                            self.d2d_context.FillEllipse(&d1, &self.brush_accent);
                            self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.13, g: 0.87, b: 0.50, a: 0.95 * alpha });
                            self.d2d_context.FillEllipse(&d2, &self.brush_accent);
                        } else {
                            self.brush_white.SetColor(&D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.92, a: 0.85 * alpha });
                        }
                        let dn_rect = D2D_RECT_F { left: dx - 8.0, top: dn_cy - 6.0, right: dx + 8.0, bottom: dn_cy + 6.0 };
                        self.d2d_context.DrawText(&dn_utf, &self.format_tag, &dn_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                    }

                    // Timeline bar: 00   06   12   18   24 + current time indicator
                    let tl_y = top_c + 70.0;
                    let times = ["00", "06", "12", "18", "24"];
                    let t_step = cal2_w / 4.0;
                    for (i, t_str) in times.iter().enumerate() {
                        let tx = r_start + (i as f32) * t_step;
                        let t_utf = t_str.encode_utf16().collect::<Vec<u16>>();
                        let tr = D2D_RECT_F { left: tx - 8.0, top: tl_y, right: tx + 8.0, bottom: tl_y + 11.0 };
                        self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.45, g: 0.48, b: 0.55, a: 0.75 * alpha });
                        self.d2d_context.DrawText(&t_utf, &self.format_tag, &tr as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                    }

                    // Dynamic current hour indicator pill
                    let time_ratio = (now_sec as f32 / 86400.0).clamp(0.0, 1.0);
                    let ev_x = r_start + time_ratio * cal2_w;
                    let ev_pill = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: ev_x - 6.0, top: tl_y - 6.0, right: ev_x + 6.0, bottom: tl_y - 2.0 },
                        radiusX: 2.0,
                        radiusY: 2.0,
                    };
                    self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.0, g: 0.48, b: 1.0, a: 0.95 * alpha });
                    self.d2d_context.FillRoundedRectangle(&ev_pill, &self.brush_accent);
                }
                crate::tasks::CalendarView::Month => {
                    // Compact Monthly Grid!
                    let first_w = crate::calendar::first_of_month(display_year, display_month) as usize; // 0=Sun .. 6=Sat
                    let total_days = crate::calendar::days_in_month(display_year, display_month);
                    let col_step = cal2_w / 7.0;
                    let s_days = ["S", "M", "T", "W", "T", "F", "S"];

                    // Header row
                    for (i, day_sym) in s_days.iter().enumerate() {
                        let dx = r_start + (i as f32 + 0.5) * col_step;
                        let d_utf = day_sym.encode_utf16().collect::<Vec<u16>>();
                        let d_rect = D2D_RECT_F { left: dx - 6.0, top: top_c + 15.0, right: dx + 6.0, bottom: top_c + 24.0 };
                        self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.45, g: 0.48, b: 0.55, a: 0.75 * alpha });
                        self.d2d_context.DrawText(&d_utf, &self.format_pill_tag, &d_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                    }

                    // Month cells
                    let row_h = 10.5;
                    let grid_top = top_c + 25.0;
                    for day_val in 1..=total_days {
                        let slot = first_w + (day_val as usize - 1);
                        let col = slot % 7;
                        let row = slot / 7;
                        if row >= 5 {
                            break; // Fit nicely in card
                        }
                        let dx = r_start + (col as f32 + 0.5) * col_step;
                        let dy = grid_top + (row as f32) * row_h;

                        let is_today = island.tasks.data.month_offset == 0 && day_val == d as u32;
                        if is_today {
                            let hl = D2D1_ELLIPSE { point: pt(dx, dy + 5.0), radiusX: 5.5, radiusY: 5.5 };
                            self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.13, g: 0.87, b: 0.50, a: 0.95 * alpha });
                            self.d2d_context.FillEllipse(&hl, &self.brush_accent);
                            self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 * alpha });
                        } else {
                            self.brush_white.SetColor(&D2D1_COLOR_F { r: 0.75, g: 0.78, b: 0.85, a: 0.80 * alpha });
                        }
                        let num_str = format!("{}", day_val);
                        let num_utf: Vec<u16> = num_str.encode_utf16().collect();
                        let nr = D2D_RECT_F { left: dx - 6.0, top: dy, right: dx + 6.0, bottom: dy + 10.0 };
                        self.d2d_context.DrawText(&num_utf, &self.format_pill_tag, &nr as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                    }
                }
            }
        }
    }

    fn draw_vector_water_drop(&self, cx: f32, cy: f32, scale: f32, color: &D2D1_COLOR_F) {
        unsafe {
            if let Ok(path) = self.d2d_factory.CreatePathGeometry() {
                if let Ok(sink) = path.Open() {
                    sink.BeginFigure(pt(cx, cy - 5.5 * scale), D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: pt(cx + 1.2 * scale, cy - 2.5 * scale),
                        point2: pt(cx + 3.8 * scale, cy + 1.0 * scale),
                        point3: pt(cx + 3.0 * scale, cy + 3.6 * scale),
                    });
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: pt(cx + 2.0 * scale, cy + 5.2 * scale),
                        point2: pt(cx - 2.0 * scale, cy + 5.2 * scale),
                        point3: pt(cx - 3.0 * scale, cy + 3.6 * scale),
                    });
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: pt(cx - 3.8 * scale, cy + 1.0 * scale),
                        point2: pt(cx - 1.2 * scale, cy - 2.5 * scale),
                        point3: pt(cx, cy - 5.5 * scale),
                    });
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    let _ = sink.Close();
                    self.brush_accent.SetColor(color);
                    self.d2d_context.FillGeometry(&path, &self.brush_accent, None);
                }
            }
        }
    }

    fn draw_vector_sliders(&self, cx: f32, cy: f32, scale: f32, color: &D2D1_COLOR_F) {
        unsafe {
            self.brush_accent.SetColor(color);
            self.d2d_context.DrawLine(pt(cx - 5.5 * scale, cy - 2.2 * scale), pt(cx + 5.5 * scale, cy - 2.2 * scale), &self.brush_accent, 1.2 * scale, None);
            self.d2d_context.DrawLine(pt(cx - 1.5 * scale, cy - 4.2 * scale), pt(cx - 1.5 * scale, cy - 0.2 * scale), &self.brush_accent, 1.8 * scale, None);
            self.d2d_context.DrawLine(pt(cx - 5.5 * scale, cy + 2.2 * scale), pt(cx + 5.5 * scale, cy + 2.2 * scale), &self.brush_accent, 1.2 * scale, None);
            self.d2d_context.DrawLine(pt(cx + 1.5 * scale, cy + 0.2 * scale), pt(cx + 1.5 * scale, cy + 4.2 * scale), &self.brush_accent, 1.8 * scale, None);
        }
    }

    #[allow(dead_code)]
    fn draw_vector_pencil(&self, cx: f32, cy: f32, scale: f32, color: &D2D1_COLOR_F) {
        unsafe {
            self.brush_accent.SetColor(color);
            self.d2d_context.DrawLine(pt(cx - 3.5 * scale, cy + 3.5 * scale), pt(cx + 3.5 * scale, cy - 3.5 * scale), &self.brush_accent, 1.8 * scale, None);
            self.d2d_context.DrawLine(pt(cx - 3.5 * scale, cy + 3.5 * scale), pt(cx - 5.0 * scale, cy + 5.0 * scale), &self.brush_accent, 1.2 * scale, None);
            self.d2d_context.DrawLine(pt(cx + 2.2 * scale, cy - 4.2 * scale), pt(cx + 4.2 * scale, cy - 2.2 * scale), &self.brush_accent, 1.4 * scale, None);
        }
    }

    fn draw_vector_calendar_icon(&self, cx: f32, cy: f32, scale: f32, color: &D2D1_COLOR_F) {
        unsafe {
            self.brush_accent.SetColor(color);
            let bw = 11.0 * scale;
            let bh = 10.0 * scale;
            let cal_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: cx - bw * 0.5,
                    top: cy - bh * 0.5,
                    right: cx + bw * 0.5,
                    bottom: cy + bh * 0.5,
                },
                radiusX: 2.0 * scale,
                radiusY: 2.0 * scale,
            };
            self.d2d_context.DrawRoundedRectangle(&cal_rect, &self.brush_accent, 1.2 * scale, None);
            self.d2d_context.DrawLine(pt(cx - bw * 0.5, cy - bh * 0.5 + 3.2 * scale), pt(cx + bw * 0.5, cy - bh * 0.5 + 3.2 * scale), &self.brush_accent, 1.0 * scale, None);
            self.d2d_context.DrawLine(pt(cx - 2.8 * scale, cy - bh * 0.5 - 1.2 * scale), pt(cx - 2.8 * scale, cy - bh * 0.5 + 1.0 * scale), &self.brush_accent, 1.2 * scale, None);
            self.d2d_context.DrawLine(pt(cx + 2.8 * scale, cy - bh * 0.5 - 1.2 * scale), pt(cx + 2.8 * scale, cy - bh * 0.5 + 1.0 * scale), &self.brush_accent, 1.2 * scale, None);
        }
    }

    fn draw_vector_plus_circle(&self, cx: f32, cy: f32, scale: f32, color: &D2D1_COLOR_F) {
        unsafe {
            self.brush_accent.SetColor(color);
            let r = 5.2 * scale;
            let circle = D2D1_ELLIPSE { point: pt(cx, cy), radiusX: r, radiusY: r };
            self.d2d_context.DrawEllipse(&circle, &self.brush_accent, 1.2 * scale, None);
            self.d2d_context.DrawLine(pt(cx - 2.5 * scale, cy), pt(cx + 2.5 * scale, cy), &self.brush_accent, 1.2 * scale, None);
            self.d2d_context.DrawLine(pt(cx, cy - 2.5 * scale), pt(cx, cy + 2.5 * scale), &self.brush_accent, 1.2 * scale, None);
        }
    }

    fn draw_vector_bell_icon(&self, cx: f32, cy: f32, scale: f32, color: &D2D1_COLOR_F) {
        unsafe {
            self.brush_accent.SetColor(color);
            self.d2d_context.DrawLine(pt(cx - 3.2 * scale, cy + 1.8 * scale), pt(cx - 2.0 * scale, cy - 2.2 * scale), &self.brush_accent, 1.2 * scale, None);
            self.d2d_context.DrawLine(pt(cx - 2.0 * scale, cy - 2.2 * scale), pt(cx + 2.0 * scale, cy - 2.2 * scale), &self.brush_accent, 1.2 * scale, None);
            self.d2d_context.DrawLine(pt(cx + 2.0 * scale, cy - 2.2 * scale), pt(cx + 3.2 * scale, cy + 1.8 * scale), &self.brush_accent, 1.2 * scale, None);
            self.d2d_context.DrawLine(pt(cx - 4.2 * scale, cy + 2.4 * scale), pt(cx + 4.2 * scale, cy + 2.4 * scale), &self.brush_accent, 1.3 * scale, None);
            let clapper = D2D1_ELLIPSE { point: pt(cx, cy + 3.8 * scale), radiusX: 1.1 * scale, radiusY: 1.1 * scale };
            self.d2d_context.FillEllipse(&clapper, &self.brush_accent);
        }
    }

    fn draw_vector_envelope(&self, cx: f32, cy: f32, w: f32, h: f32, color: &D2D1_COLOR_F) {
        unsafe {
            self.brush_accent.SetColor(color);
            let env_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: cx - w * 0.5,
                    top: cy - h * 0.5,
                    right: cx + w * 0.5,
                    bottom: cy + h * 0.5,
                },
                radiusX: 1.8,
                radiusY: 1.8,
            };
            self.d2d_context.DrawRoundedRectangle(&env_rect, &self.brush_accent, 1.1, None);
            self.d2d_context.DrawLine(pt(cx - w * 0.5, cy - h * 0.5), pt(cx, cy + 1.0), &self.brush_accent, 1.1, None);
            self.d2d_context.DrawLine(pt(cx + w * 0.5, cy - h * 0.5), pt(cx, cy + 1.0), &self.brush_accent, 1.1, None);
        }
    }

    fn draw_vector_music_note(&self, cx: f32, cy: f32, scale: f32, color: &D2D1_COLOR_F) {
        unsafe {
            self.brush_accent.SetColor(color);
            let n1 = D2D1_ELLIPSE { point: pt(cx - 3.2 * scale, cy + 2.8 * scale), radiusX: 2.2 * scale, radiusY: 1.6 * scale };
            let n2 = D2D1_ELLIPSE { point: pt(cx + 3.2 * scale, cy + 1.2 * scale), radiusX: 2.2 * scale, radiusY: 1.6 * scale };
            self.d2d_context.FillEllipse(&n1, &self.brush_accent);
            self.d2d_context.FillEllipse(&n2, &self.brush_accent);
            self.d2d_context.DrawLine(pt(cx - 1.2 * scale, cy + 2.8 * scale), pt(cx - 1.2 * scale, cy - 3.8 * scale), &self.brush_accent, 1.2 * scale, None);
            self.d2d_context.DrawLine(pt(cx + 5.2 * scale, cy + 1.2 * scale), pt(cx + 5.2 * scale, cy - 5.4 * scale), &self.brush_accent, 1.2 * scale, None);
            self.d2d_context.DrawLine(pt(cx - 1.2 * scale, cy - 3.8 * scale), pt(cx + 5.2 * scale, cy - 5.4 * scale), &self.brush_accent, 1.8 * scale, None);
        }
    }

    fn draw_speech_bubble_icon(&self, cx: f32, cy: f32, is_active: bool, alpha: f32) {
        unsafe {
            let bw = 17.0;
            let bh = 13.0;
            let bubble_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: cx - bw * 0.5,
                    top: cy - bh * 0.5 - 1.0,
                    right: cx + bw * 0.5,
                    bottom: cy + bh * 0.5 - 1.0,
                },
                radiusX: 3.5,
                radiusY: 3.5,
            };

            let stroke_color = if is_active {
                D2D1_COLOR_F { r: 0.20, g: 0.78, b: 1.0, a: 0.95 * alpha }
            } else {
                D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.75 * alpha }
            };
            self.brush_accent.SetColor(&stroke_color);
            self.d2d_context.DrawRoundedRectangle(&bubble_rect, &self.brush_accent, 1.4, None);

            // Speech bubble tail at bottom-left
            let tail_x = cx - 3.5;
            let tail_y = cy + bh * 0.5 - 1.0;
            self.d2d_context.DrawLine(pt(tail_x, tail_y), pt(tail_x - 2.5, tail_y + 3.0), &self.brush_accent, 1.4, None);
            self.d2d_context.DrawLine(pt(tail_x - 2.5, tail_y + 3.0), pt(tail_x + 2.5, tail_y), &self.brush_accent, 1.4, None);

            // Two dots inside bubble
            let dot1 = D2D1_ELLIPSE { point: pt(cx - 3.0, cy - 1.0), radiusX: 1.1, radiusY: 1.1 };
            let dot2 = D2D1_ELLIPSE { point: pt(cx + 3.0, cy - 1.0), radiusX: 1.1, radiusY: 1.1 };
            self.d2d_context.FillEllipse(&dot1, &self.brush_accent);
            self.d2d_context.FillEllipse(&dot2, &self.brush_accent);
        }
    }

    fn draw_vector_location_pin(&self, cx: f32, cy: f32, scale: f32, color: &D2D1_COLOR_F) {
        unsafe {
            self.brush_accent.SetColor(color);
            let head = D2D1_ELLIPSE { point: pt(cx, cy - 1.5 * scale), radiusX: 3.2 * scale, radiusY: 3.2 * scale };
            self.d2d_context.DrawEllipse(&head, &self.brush_accent, 1.2 * scale, None);
            let inner = D2D1_ELLIPSE { point: pt(cx, cy - 1.5 * scale), radiusX: 1.1 * scale, radiusY: 1.1 * scale };
            self.d2d_context.FillEllipse(&inner, &self.brush_accent);
            self.d2d_context.DrawLine(pt(cx - 2.2 * scale, cy - 0.2 * scale), pt(cx, cy + 3.8 * scale), &self.brush_accent, 1.2 * scale, None);
            self.d2d_context.DrawLine(pt(cx + 2.2 * scale, cy - 0.2 * scale), pt(cx, cy + 3.8 * scale), &self.brush_accent, 1.2 * scale, None);
        }
    }

    fn draw_vector_sun(&self, cx: f32, cy: f32, scale: f32, color: &D2D1_COLOR_F) {
        unsafe {
            self.brush_accent.SetColor(color);
            let disc = D2D1_ELLIPSE { point: pt(cx, cy), radiusX: 2.8 * scale, radiusY: 2.8 * scale };
            self.d2d_context.FillEllipse(&disc, &self.brush_accent);
            let r1 = 3.8 * scale;
            let r2 = 5.2 * scale;
            let ray_dirs = [
                (0.0, -1.0), (0.707, -0.707), (1.0, 0.0), (0.707, 0.707),
                (0.0, 1.0), (-0.707, 0.707), (-1.0, 0.0), (-0.707, -0.707),
            ];
            for (dx, dy) in ray_dirs {
                self.d2d_context.DrawLine(
                    pt(cx + dx * r1, cy + dy * r1),
                    pt(cx + dx * r2, cy + dy * r2),
                    &self.brush_accent,
                    1.1 * scale,
                    None,
                );
            }
        }
    }

    fn draw_vector_crescent_moon(&self, cx: f32, cy: f32, scale: f32, color: &D2D1_COLOR_F) {
        unsafe {
            self.brush_accent.SetColor(color);
            if let Ok(path) = self.d2d_factory.CreatePathGeometry() {
                if let Ok(sink) = path.Open() {
                    sink.BeginFigure(pt(cx, cy - 4.5 * scale), D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddArc(&D2D1_ARC_SEGMENT {
                        point: pt(cx, cy + 4.5 * scale),
                        size: D2D_SIZE_F { width: 4.5 * scale, height: 4.5 * scale },
                        rotationAngle: 0.0,
                        sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                        arcSize: D2D1_ARC_SIZE_SMALL,
                    });
                    sink.AddArc(&D2D1_ARC_SEGMENT {
                        point: pt(cx, cy - 4.5 * scale),
                        size: D2D_SIZE_F { width: 3.2 * scale, height: 4.0 * scale },
                        rotationAngle: 0.0,
                        sweepDirection: D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
                        arcSize: D2D1_ARC_SIZE_SMALL,
                    });
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    let _ = sink.Close();
                    self.d2d_context.FillGeometry(&path, &self.brush_accent, None);
                }
            }
        }
    }

    fn draw_card_music_lyrics(
        &self,
        island: &Island,
        config: &crate::config::AppConfig,
        left: f32,
        top: f32,
        right: f32,
        _bottom: f32,
        alpha: f32,
    ) {
        unsafe {
            let cx = (left + right) * 0.5;
            let top_c = top + 26.0;
            let col_l = left + 44.0;
            let right_c = right - 44.0;

            // Check if we render the unified full-width media player OR the 2-column lyrics split
            if !island.show_lyrics {
                // =========================================================================
                // VIEW A: Full-Width Spacious Media Player (Default View)
                // Guaranteed 19.5px vertical clearance above bottom pagination dots
                // =========================================================================
                if let Some(track) = &island.current_track {
                    let art_size = 40.0;
                    let art_cx = col_l + art_size * 0.5;
                    let art_cy = top_c + 2.0 + art_size * 0.5;
                    let art_radius = art_size * 0.5;

                    if let Some(art) = &self.album_art {
                        self.draw_album_art_badge(art, art_cx, art_cy, art_radius, alpha);
                    } else {
                        let rot = if config.enable_vinyl_animation { island.vinyl_rotation } else { 0.0 };
                        self.draw_spinning_vinyl(art_cx, art_cy, art_radius, rot, alpha);
                    }

                    // Title & Artist beside album art
                    let text_left = col_l + art_size + 12.0;
                    let title_rect = D2D_RECT_F {
                        left: text_left,
                        top: top_c + 3.0,
                        right: right_c - 10.0,
                        bottom: top_c + 21.0,
                    };
                    let title_utf: Vec<u16> = track.title.encode_utf16().collect();
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                    self.brush_white.SetOpacity(alpha);
                    self.d2d_context.DrawText(&title_utf, &self.format_title, &title_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                    let artist_rect = D2D_RECT_F {
                        left: text_left,
                        top: top_c + 22.0,
                        right: right_c - 10.0,
                        bottom: top_c + 38.0,
                    };
                    let artist_str = if track.artist.is_empty() { "Now Playing".to_string() } else { track.artist.clone() };
                    let artist_utf: Vec<u16> = artist_str.encode_utf16().collect();
                    self.brush_subtext.SetOpacity(0.85 * alpha);
                    self.d2d_context.DrawText(&artist_utf, &self.format_body, &artist_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                    // Full-width Seek Bar (Positioned at top_c + 49.0 = top + 75.0)
                    let seek_y = top_c + 49.0;
                    let pos_secs = track.position_secs.max(0.0);
                    let pos_mins = (pos_secs / 60.0).floor() as u32;
                    let pos_rem = (pos_secs % 60.0).floor() as u32;
                    let pos_str = format!("{:02}:{:02}", pos_mins, pos_rem);
                    let pos_utf: Vec<u16> = pos_str.encode_utf16().collect();

                    let pos_rect = D2D_RECT_F { left: col_l, top: seek_y - 7.0, right: col_l + 38.0, bottom: seek_y + 7.0 };
                    self.brush_time.SetOpacity(0.85 * alpha);
                    self.d2d_context.DrawText(&pos_utf, &self.format_date, &pos_rect as *const _, &self.brush_time, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                    let seek_left = col_l + 42.0;
                    let seek_right = right_c - 46.0;
                    let seek_w = (seek_right - seek_left).max(10.0);
                    let ratio = if track.duration_secs > 0.0 { (pos_secs / track.duration_secs).clamp(0.0, 1.0) } else { 0.0 };

                    self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.16 * alpha });
                    self.d2d_context.DrawLine(pt(seek_left, seek_y), pt(seek_right, seek_y), &self.brush_btn_bg, 3.2, None);

                    let played_x = seek_left + ratio * seek_w;
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                    self.brush_white.SetOpacity(0.95 * alpha);
                    if ratio > 0.005 {
                        self.d2d_context.DrawLine(pt(seek_left, seek_y), pt(played_x, seek_y), &self.brush_white, 3.2, None);
                    }
                    let thumb = D2D1_ELLIPSE { point: pt(played_x, seek_y), radiusX: 4.2, radiusY: 4.2 };
                    self.d2d_context.FillEllipse(&thumb, &self.brush_white);

                    let rem_str = if track.duration_secs > 0.0 {
                        let rem_secs = (track.duration_secs - pos_secs).max(0.0);
                        let rem_mins = (rem_secs / 60.0).floor() as u32;
                        let rem_s = (rem_secs % 60.0).floor() as u32;
                        format!("-{:02}:{:02}", rem_mins, rem_s)
                    } else {
                        "--:--".to_string()
                    };
                    let rem_utf: Vec<u16> = rem_str.encode_utf16().collect();
                    let rem_rect = D2D_RECT_F { left: right_c - 42.0, top: seek_y - 7.0, right: right_c, bottom: seek_y + 7.0 };
                    self.brush_time.SetOpacity(0.85 * alpha);
                    self.d2d_context.DrawText(&rem_utf, &self.format_date, &rem_rect as *const _, &self.brush_time, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                    // Bottom Control Row: [💬] on left, Prev/Play/Next in center, Volume on right
                    // ctrl_y = top_c + 78.0 = top + 104.0; play circle bottom = top + 117.5.
                    // Bottom pagination bar is at bottom - 9.0 = top + 139.0 -> 21.5px vertical clearance!
                    let ctrl_y = top_c + 78.0;

                    // Speech bubble lyrics toggle button [💬] at col_l + 10.0
                    self.draw_speech_bubble_icon(col_l + 10.0, ctrl_y, false, alpha);

                    // Center transport buttons
                    let prev_cx = cx - 44.0;
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                    self.brush_white.SetOpacity(0.85 * alpha);
                    self.d2d_context.DrawLine(pt(prev_cx + 4.0, ctrl_y - 5.0), pt(prev_cx - 3.5, ctrl_y), &self.brush_white, 1.8, None);
                    self.d2d_context.DrawLine(pt(prev_cx + 4.0, ctrl_y + 5.0), pt(prev_cx - 3.5, ctrl_y), &self.brush_white, 1.8, None);
                    self.d2d_context.DrawLine(pt(prev_cx + 4.0, ctrl_y - 5.0), pt(prev_cx + 4.0, ctrl_y + 5.0), &self.brush_white, 1.8, None);
                    self.d2d_context.DrawLine(pt(prev_cx - 5.0, ctrl_y - 4.5), pt(prev_cx - 5.0, ctrl_y + 4.5), &self.brush_white, 1.8, None);

                    let play_bg = D2D1_ELLIPSE { point: pt(cx, ctrl_y), radiusX: 13.5, radiusY: 13.5 };
                    self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.16 * alpha });
                    self.d2d_context.FillEllipse(&play_bg, &self.brush_btn_bg);

                    self.brush_white.SetOpacity(0.95 * alpha);
                    let is_really_playing = island.current_track.as_ref().map(|t| t.is_playing).unwrap_or(island.is_media_playing);
                    if is_really_playing {
                        self.d2d_context.DrawLine(pt(cx - 3.0, ctrl_y - 4.8), pt(cx - 3.0, ctrl_y + 4.8), &self.brush_white, 2.2, None);
                        self.d2d_context.DrawLine(pt(cx + 3.0, ctrl_y - 4.8), pt(cx + 3.0, ctrl_y + 4.8), &self.brush_white, 2.2, None);
                    } else {
                        self.d2d_context.DrawLine(pt(cx - 3.0, ctrl_y - 5.0), pt(cx + 4.5, ctrl_y), &self.brush_white, 1.8, None);
                        self.d2d_context.DrawLine(pt(cx - 3.0, ctrl_y + 5.0), pt(cx + 4.5, ctrl_y), &self.brush_white, 1.8, None);
                        self.d2d_context.DrawLine(pt(cx - 3.0, ctrl_y - 5.0), pt(cx - 3.0, ctrl_y + 5.0), &self.brush_white, 1.8, None);
                    }

                    let next_cx = cx + 44.0;
                    self.brush_white.SetOpacity(0.85 * alpha);
                    self.d2d_context.DrawLine(pt(next_cx - 4.0, ctrl_y - 5.0), pt(next_cx + 3.5, ctrl_y), &self.brush_white, 1.8, None);
                    self.d2d_context.DrawLine(pt(next_cx - 4.0, ctrl_y + 5.0), pt(next_cx + 3.5, ctrl_y), &self.brush_white, 1.8, None);
                    self.d2d_context.DrawLine(pt(next_cx - 4.0, ctrl_y - 5.0), pt(next_cx - 4.0, ctrl_y + 5.0), &self.brush_white, 1.8, None);
                    self.d2d_context.DrawLine(pt(next_cx + 5.0, ctrl_y - 4.5), pt(next_cx + 5.0, ctrl_y + 4.5), &self.brush_white, 1.8, None);

                    // Speaker icon & volume slider on right
                    let spk_x = right_c - 106.0;
                    self.brush_white.SetOpacity(0.75 * alpha);
                    self.d2d_context.DrawLine(pt(spk_x - 3.0, ctrl_y - 2.0), pt(spk_x - 1.0, ctrl_y - 2.0), &self.brush_white, 1.2, None);
                    self.d2d_context.DrawLine(pt(spk_x - 1.0, ctrl_y - 2.0), pt(spk_x + 2.0, ctrl_y - 4.5), &self.brush_white, 1.2, None);
                    self.d2d_context.DrawLine(pt(spk_x + 2.0, ctrl_y - 4.5), pt(spk_x + 2.0, ctrl_y + 4.5), &self.brush_white, 1.2, None);
                    self.d2d_context.DrawLine(pt(spk_x + 2.0, ctrl_y + 4.5), pt(spk_x - 1.0, ctrl_y + 2.0), &self.brush_white, 1.2, None);
                    self.d2d_context.DrawLine(pt(spk_x - 1.0, ctrl_y + 2.0), pt(spk_x - 3.0, ctrl_y + 2.0), &self.brush_white, 1.2, None);
                    self.d2d_context.DrawLine(pt(spk_x - 3.0, ctrl_y - 2.0), pt(spk_x - 3.0, ctrl_y + 2.0), &self.brush_white, 1.2, None);

                    let vol_left = right_c - 94.0;
                    let vol_right = right_c - 10.0;
                    let vol_w = vol_right - vol_left;
                    let vol_val = island.volume.clamp(0.0, 1.0);

                    self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.16 * alpha });
                    self.d2d_context.DrawLine(pt(vol_left, ctrl_y), pt(vol_right, ctrl_y), &self.brush_btn_bg, 3.2, None);

                    let vol_fill_x = vol_left + vol_val * vol_w;
                    self.brush_white.SetOpacity(0.90 * alpha);
                    if vol_val > 0.02 {
                        self.d2d_context.DrawLine(pt(vol_left, ctrl_y), pt(vol_fill_x, ctrl_y), &self.brush_white, 3.2, None);
                    }
                    let vol_thumb = D2D1_ELLIPSE { point: pt(vol_fill_x, ctrl_y), radiusX: 3.8, radiusY: 3.8 };
                    self.d2d_context.FillEllipse(&vol_thumb, &self.brush_white);
                } else {
                    let disc_cx = col_l + 28.0;
                    let disc_cy = top_c + 44.0;
                    self.draw_spinning_vinyl(disc_cx, disc_cy, 22.0, 0.0, alpha * 0.6);

                    let no_title = "No Active Music".encode_utf16().collect::<Vec<u16>>();
                    let nt_rect = D2D_RECT_F { left: col_l + 62.0, top: top_c + 24.0, right: right_c, bottom: top_c + 44.0 };
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                    self.brush_white.SetOpacity(0.90 * alpha);
                    self.d2d_context.DrawText(&no_title, &self.format_title, &nt_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                    let no_sub = "Play Spotify or YouTube Music".encode_utf16().collect::<Vec<u16>>();
                    let ns_rect = D2D_RECT_F { left: col_l + 62.0, top: top_c + 44.0, right: right_c, bottom: top_c + 64.0 };
                    self.brush_subtext.SetOpacity(0.70 * alpha);
                    self.d2d_context.DrawText(&no_sub, &self.format_date, &ns_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
                }
            } else {
                // =========================================================================
                // VIEW B: Two-Column Split (Compact Player on Left, Synced Lyrics on Right)
                // Triggered when user clicks [💬] toggle button (Strictly NO emojis!)
                // =========================================================================
                let col_r = cx + 12.0;

                // Vertical divider between columns
                self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.08 * alpha });
                self.d2d_context.DrawLine(pt(cx, top_c + 4.0), pt(cx, top_c + 86.0), &self.brush_btn_bg, 1.0, None);

                // Left Column: Compact Media Controller
                if let Some(track) = &island.current_track {
                    let art_size = 36.0;
                    let art_cx = col_l + art_size * 0.5;
                    let art_cy = top_c + 2.0 + art_size * 0.5;
                    let art_radius = art_size * 0.5;

                    if let Some(art) = &self.album_art {
                        self.draw_album_art_badge(art, art_cx, art_cy, art_radius, alpha);
                    } else {
                        let rot = if config.enable_vinyl_animation { island.vinyl_rotation } else { 0.0 };
                        self.draw_spinning_vinyl(art_cx, art_cy, art_radius, rot, alpha);
                    }

                    // Title & Artist
                    let title_rect = D2D_RECT_F {
                        left: col_l + art_size + 10.0,
                        top: top_c + 2.0,
                        right: cx - 10.0,
                        bottom: top_c + 19.0,
                    };
                    let title_utf: Vec<u16> = track.title.encode_utf16().collect();
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                    self.brush_white.SetOpacity(alpha);
                    self.d2d_context.DrawText(&title_utf, &self.format_title, &title_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                    let artist_rect = D2D_RECT_F {
                        left: col_l + art_size + 10.0,
                        top: top_c + 19.0,
                        right: cx - 10.0,
                        bottom: top_c + 34.0,
                    };
                    let artist_str = if track.artist.is_empty() { "Now Playing".to_string() } else { track.artist.clone() };
                    let artist_utf: Vec<u16> = artist_str.encode_utf16().collect();
                    self.brush_subtext.SetOpacity(0.80 * alpha);
                    self.d2d_context.DrawText(&artist_utf, &self.format_body, &artist_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                    // Playback Seek Bar
                    let seek_y = top_c + 46.0;
                    let pos_secs = track.position_secs.max(0.0);
                    let pos_mins = (pos_secs / 60.0).floor() as u32;
                    let pos_rem = (pos_secs % 60.0).floor() as u32;
                    let pos_str = format!("{:02}:{:02}", pos_mins, pos_rem);
                    let pos_utf: Vec<u16> = pos_str.encode_utf16().collect();

                    let pos_rect = D2D_RECT_F { left: col_l, top: seek_y - 7.0, right: col_l + 34.0, bottom: seek_y + 7.0 };
                    self.brush_time.SetOpacity(0.85 * alpha);
                    self.d2d_context.DrawText(&pos_utf, &self.format_date, &pos_rect as *const _, &self.brush_time, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                    let seek_left = col_l + 38.0;
                    let seek_right = cx - 42.0;
                    let seek_w = (seek_right - seek_left).max(10.0);
                    let ratio = if track.duration_secs > 0.0 { (pos_secs / track.duration_secs).clamp(0.0, 1.0) } else { 0.0 };

                    self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.16 * alpha });
                    self.d2d_context.DrawLine(pt(seek_left, seek_y), pt(seek_right, seek_y), &self.brush_btn_bg, 3.0, None);

                    let played_x = seek_left + ratio * seek_w;
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                    self.brush_white.SetOpacity(0.95 * alpha);
                    if ratio > 0.005 {
                        self.d2d_context.DrawLine(pt(seek_left, seek_y), pt(played_x, seek_y), &self.brush_white, 3.0, None);
                    }
                    let thumb = D2D1_ELLIPSE { point: pt(played_x, seek_y), radiusX: 3.8, radiusY: 3.8 };
                    self.d2d_context.FillEllipse(&thumb, &self.brush_white);

                    let rem_str = if track.duration_secs > 0.0 {
                        let rem_secs = (track.duration_secs - pos_secs).max(0.0);
                        let rem_mins = (rem_secs / 60.0).floor() as u32;
                        let rem_s = (rem_secs % 60.0).floor() as u32;
                        format!("-{:02}:{:02}", rem_mins, rem_s)
                    } else {
                        "--:--".to_string()
                    };
                    let rem_utf: Vec<u16> = rem_str.encode_utf16().collect();
                    let rem_rect = D2D_RECT_F { left: cx - 38.0, top: seek_y - 7.0, right: cx - 4.0, bottom: seek_y + 7.0 };
                    self.d2d_context.DrawText(&rem_utf, &self.format_date, &rem_rect as *const _, &self.brush_time, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

                    // Transport buttons + [💬] speech bubble
                    let ctrl_y = top_c + 75.0;

                    // Speech bubble [💬] highlighted in cyan
                    self.draw_speech_bubble_icon(col_l + 8.0, ctrl_y, true, alpha);

                    let btn_cx = (col_l + 24.0 + cx - 10.0) * 0.5;

                    // Prev: btn_cx - 28.0
                    let prev_cx = btn_cx - 28.0;
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                    self.brush_white.SetOpacity(0.85 * alpha);
                    self.d2d_context.DrawLine(pt(prev_cx + 3.5, ctrl_y - 4.5), pt(prev_cx - 3.0, ctrl_y), &self.brush_white, 1.6, None);
                    self.d2d_context.DrawLine(pt(prev_cx + 3.5, ctrl_y + 4.5), pt(prev_cx - 3.0, ctrl_y), &self.brush_white, 1.6, None);
                    self.d2d_context.DrawLine(pt(prev_cx + 3.5, ctrl_y - 4.5), pt(prev_cx + 3.5, ctrl_y + 4.5), &self.brush_white, 1.6, None);
                    self.d2d_context.DrawLine(pt(prev_cx - 4.5, ctrl_y - 4.0), pt(prev_cx - 4.5, ctrl_y + 4.0), &self.brush_white, 1.6, None);

                    // Play / Pause in Center: btn_cx
                    let play_bg = D2D1_ELLIPSE { point: pt(btn_cx, ctrl_y), radiusX: 12.5, radiusY: 12.5 };
                    self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.16 * alpha });
                    self.d2d_context.FillEllipse(&play_bg, &self.brush_btn_bg);

                    self.brush_white.SetOpacity(0.95 * alpha);
                    let is_really_playing = island.current_track.as_ref().map(|t| t.is_playing).unwrap_or(island.is_media_playing);
                    if is_really_playing {
                        self.d2d_context.DrawLine(pt(btn_cx - 2.5, ctrl_y - 4.2), pt(btn_cx - 2.5, ctrl_y + 4.2), &self.brush_white, 1.8, None);
                        self.d2d_context.DrawLine(pt(btn_cx + 2.5, ctrl_y - 4.2), pt(btn_cx + 2.5, ctrl_y + 4.2), &self.brush_white, 1.8, None);
                    } else {
                        self.d2d_context.DrawLine(pt(btn_cx - 2.8, ctrl_y - 4.5), pt(btn_cx + 4.0, ctrl_y), &self.brush_white, 1.6, None);
                        self.d2d_context.DrawLine(pt(btn_cx - 2.8, ctrl_y + 4.5), pt(btn_cx + 4.0, ctrl_y), &self.brush_white, 1.6, None);
                        self.d2d_context.DrawLine(pt(btn_cx - 2.8, ctrl_y - 4.5), pt(btn_cx - 2.8, ctrl_y + 4.5), &self.brush_white, 1.6, None);
                    }

                    // Next: btn_cx + 28.0
                    let next_cx = btn_cx + 28.0;
                    self.brush_white.SetOpacity(0.85 * alpha);
                    self.d2d_context.DrawLine(pt(next_cx - 3.5, ctrl_y - 4.5), pt(next_cx + 3.0, ctrl_y), &self.brush_white, 1.6, None);
                    self.d2d_context.DrawLine(pt(next_cx - 3.5, ctrl_y + 4.5), pt(next_cx + 3.0, ctrl_y), &self.brush_white, 1.6, None);
                    self.d2d_context.DrawLine(pt(next_cx - 3.5, ctrl_y - 4.5), pt(next_cx - 3.5, ctrl_y + 4.5), &self.brush_white, 1.6, None);
                    self.d2d_context.DrawLine(pt(next_cx + 4.5, ctrl_y - 4.0), pt(next_cx + 4.5, ctrl_y + 4.0), &self.brush_white, 1.6, None);
                } else {
                    let disc_cx = col_l + 24.0;
                    let disc_cy = top_c + 40.0;
                    self.draw_spinning_vinyl(disc_cx, disc_cy, 20.0, 0.0, alpha * 0.6);

                    let no_title = "No Active Music".encode_utf16().collect::<Vec<u16>>();
                    let nt_rect = D2D_RECT_F { left: col_l + 54.0, top: top_c + 24.0, right: cx - 10.0, bottom: top_c + 42.0 };
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                    self.brush_white.SetOpacity(0.90 * alpha);
                    self.d2d_context.DrawText(&no_title, &self.format_title, &nt_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                    let no_sub = "Play Spotify or YouTube".encode_utf16().collect::<Vec<u16>>();
                    let ns_rect = D2D_RECT_F { left: col_l + 54.0, top: top_c + 42.0, right: cx - 10.0, bottom: top_c + 60.0 };
                    self.brush_subtext.SetOpacity(0.70 * alpha);
                    self.d2d_context.DrawText(&no_sub, &self.format_date, &ns_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
                }

                // Right Column: Synced Lyrics (Spotify & Better Lyrics multiline view with wrap)
                let lyr_label = "SYNCED LYRICS".encode_utf16().collect::<Vec<u16>>();
                let ll_rect = D2D_RECT_F { left: col_r, top: top_c + 2.0, right: col_r + 76.0, bottom: top_c + 14.0 };
                self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.38, g: 0.78, b: 1.0, a: 0.85 * alpha });
                self.d2d_context.DrawText(&lyr_label, &self.format_tag, &ll_rect as *const _, &self.brush_accent, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

                // Quick Romaji Toggle Pill [A / あ] (col_r + 80.0 .. col_r + 128.0, top_c .. top_c + 16.0)
                let rom_pill = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: col_r + 80.0, top: top_c, right: col_r + 128.0, bottom: top_c + 16.0 },
                    radiusX: 4.0,
                    radiusY: 4.0,
                };
                if island.romanize_cjk_lyrics {
                    self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.38, g: 0.78, b: 1.0, a: 0.25 * alpha });
                    self.d2d_context.FillRoundedRectangle(&rom_pill, &self.brush_accent);
                    self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.50, g: 0.88, b: 1.0, a: 0.90 * alpha });
                    self.d2d_context.DrawRoundedRectangle(&rom_pill, &self.brush_accent, 1.0, None);
                    let rom_txt = "A / あ".encode_utf16().collect::<Vec<u16>>();
                    self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.55, g: 0.90, b: 1.0, a: 0.95 * alpha });
                    self.d2d_context.DrawText(&rom_txt, &self.format_pill_tag, &rom_pill.rect as *const _, &self.brush_accent, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
                } else {
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.08 * alpha });
                    self.d2d_context.FillRoundedRectangle(&rom_pill, &self.brush_white);
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.25 * alpha });
                    self.d2d_context.DrawRoundedRectangle(&rom_pill, &self.brush_white, 1.0, None);
                    let rom_txt = "A / あ".encode_utf16().collect::<Vec<u16>>();
                    self.brush_subtext.SetOpacity(0.70 * alpha);
                    self.d2d_context.DrawText(&rom_txt, &self.format_pill_tag, &rom_pill.rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
                }

                // Candidate Switcher Pill [1/N Alt] or [Retry] (col_r + 134.0 .. col_r + 184.0, top_c .. top_c + 16.0)
                let total_cand = island.lyrics_candidates.len();
                let cand_label = if total_cand > 1 {
                    format!("{}/{} Alt", island.active_candidate_idx + 1, total_cand)
                } else {
                    "Retry".to_string()
                };
                let cand_pill = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: col_r + 134.0, top: top_c, right: col_r + 184.0, bottom: top_c + 16.0 },
                    radiusX: 4.0,
                    radiusY: 4.0,
                };
                self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.08 * alpha });
                self.d2d_context.FillRoundedRectangle(&cand_pill, &self.brush_white);
                if total_cand > 1 {
                    self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.45, g: 0.78, b: 1.0, a: 0.40 * alpha });
                    self.d2d_context.DrawRoundedRectangle(&cand_pill, &self.brush_accent, 1.0, None);
                    let cand_txt = cand_label.encode_utf16().collect::<Vec<u16>>();
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 0.88, g: 0.92, b: 1.0, a: 0.90 * alpha });
                    self.d2d_context.DrawText(&cand_txt, &self.format_pill_tag, &cand_pill.rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
                } else {
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.25 * alpha });
                    self.d2d_context.DrawRoundedRectangle(&cand_pill, &self.brush_white, 1.0, None);
                    let cand_txt = cand_label.encode_utf16().collect::<Vec<u16>>();
                    self.brush_subtext.SetOpacity(0.70 * alpha);
                    self.d2d_context.DrawText(&cand_txt, &self.format_pill_tag, &cand_pill.rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
                }

                let (prev_line_opt, curr_line_str, next_line_opt, _line_progress, fade_alpha) = if let Some(ref lyrics) = island.current_lyrics {
                    let pos = island.current_track.as_ref().map(|t| t.position_secs).unwrap_or(0.0);
                    let info = lyrics.get_active_line_info(pos, island.romanize_cjk_lyrics);
                    let c = if info.curr == "•••" {
                        if lyrics.lines.is_empty() {
                            if island.lyrics_loading {
                                "Syncing lyrics..."
                            } else {
                                "No synced lyrics found"
                            }
                        } else if info.prev.is_none() {
                            "••• Instrumental Intro •••"
                        } else {
                            "••• Instrumental Break •••"
                        }
                    } else {
                        info.curr
                    };
                    let sub = if lyrics.lines.is_empty() {
                        if island.lyrics_loading {
                            Some("Searching LRCLIB database...".to_string())
                        } else {
                            Some("Click to retry search".to_string())
                        }
                    } else {
                        info.next.map(|s| s.to_string())
                    };
                    (info.prev.map(|s| s.to_string()), c.to_string(), sub, info.progress, info.fade_alpha)
                } else if island.lyrics_loading {
                    (None, "Syncing lyrics...".to_string(), Some("Searching LRCLIB database...".to_string()), 0.0, 1.0)
                } else if island.current_track.is_some() {
                    (None, "No synced lyrics found".to_string(), Some("Click to retry search".to_string()), 0.0, 1.0)
                } else {
                    (None, "Play music to see lyrics".to_string(), None, 0.0, 1.0)
                };

                let lyrics_box = D2D_RECT_F {
                    left: col_r,
                    top: top_c + 16.0,
                    right: right_c,
                    bottom: top_c + 72.0,
                };
                self.d2d_context.PushAxisAlignedClip(&lyrics_box, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

                let is_long_curr = curr_line_str.len() > 32;

                if is_long_curr {
                    // Long line: Active current line wraps nicely across 2-3 lines (height ~38px)
                    let cur_utf: Vec<u16> = curr_line_str.encode_utf16().collect();
                    let cur_rect = D2D_RECT_F {
                        left: col_r,
                        top: top_c + 16.0,
                        right: right_c,
                        bottom: top_c + 54.0,
                    };
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                    self.brush_white.SetOpacity((0.72 + 0.26 * fade_alpha) * alpha);
                    self.d2d_context.DrawText(
                        &cur_utf,
                        &self.format_lyrics,
                        &cur_rect as *const _,
                        &self.brush_white,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    // Next line below it in dim gray
                    if let Some(next) = next_line_opt {
                        let next_utf: Vec<u16> = next.encode_utf16().collect();
                        let n_rect = D2D_RECT_F {
                            left: col_r,
                            top: top_c + 55.0,
                            right: right_c,
                            bottom: top_c + 70.0,
                        };
                        self.brush_subtext.SetOpacity(0.42 * alpha);
                        self.d2d_context.DrawText(
                            &next_utf,
                            &self.format_lyrics_sub,
                            &n_rect as *const _,
                            &self.brush_subtext,
                            D2D1_DRAW_TEXT_OPTIONS_CLIP,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                } else {
                    // Standard line: Shows Previous line (top), Active line (middle), Next line (bottom)
                    if let Some(prev) = prev_line_opt {
                        let prev_utf: Vec<u16> = prev.encode_utf16().collect();
                        let p_rect = D2D_RECT_F {
                            left: col_r,
                            top: top_c + 16.0,
                            right: right_c,
                            bottom: top_c + 29.0,
                        };
                        self.brush_subtext.SetOpacity(0.42 * alpha);
                        self.d2d_context.DrawText(
                            &prev_utf,
                            &self.format_lyrics_sub,
                            &p_rect as *const _,
                            &self.brush_subtext,
                            D2D1_DRAW_TEXT_OPTIONS_CLIP,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }

                    let cur_utf: Vec<u16> = curr_line_str.encode_utf16().collect();
                    let cur_rect = D2D_RECT_F {
                        left: col_r,
                        top: top_c + 30.0,
                        right: right_c,
                        bottom: top_c + 50.0,
                    };
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                    self.brush_white.SetOpacity((0.72 + 0.26 * fade_alpha) * alpha);
                    self.d2d_context.DrawText(
                        &cur_utf,
                        &self.format_lyrics,
                        &cur_rect as *const _,
                        &self.brush_white,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    if let Some(next) = next_line_opt {
                        let next_utf: Vec<u16> = next.encode_utf16().collect();
                        let n_rect = D2D_RECT_F {
                            left: col_r,
                            top: top_c + 53.0,
                            right: right_c,
                            bottom: top_c + 70.0,
                        };
                        self.brush_subtext.SetOpacity(0.42 * alpha);
                        self.d2d_context.DrawText(
                            &next_utf,
                            &self.format_lyrics_sub,
                            &n_rect as *const _,
                            &self.brush_subtext,
                            D2D1_DRAW_TEXT_OPTIONS_CLIP,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                }

                self.d2d_context.PopAxisAlignedClip();

                // Volume Control Slider on bottom right
                let vol_y = top_c + 75.0;
                let spk_x = col_r + 8.0;
                self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                self.brush_white.SetOpacity(0.75 * alpha);
                self.d2d_context.DrawLine(pt(spk_x - 3.0, vol_y - 2.0), pt(spk_x - 1.0, vol_y - 2.0), &self.brush_white, 1.2, None);
                self.d2d_context.DrawLine(pt(spk_x - 1.0, vol_y - 2.0), pt(spk_x + 2.0, vol_y - 4.5), &self.brush_white, 1.2, None);
                self.d2d_context.DrawLine(pt(spk_x + 2.0, vol_y - 4.5), pt(spk_x + 2.0, vol_y + 4.5), &self.brush_white, 1.2, None);
                self.d2d_context.DrawLine(pt(spk_x + 2.0, vol_y + 4.5), pt(spk_x - 1.0, vol_y + 2.0), &self.brush_white, 1.2, None);
                self.d2d_context.DrawLine(pt(spk_x - 1.0, vol_y + 2.0), pt(spk_x - 3.0, vol_y + 2.0), &self.brush_white, 1.2, None);
                self.d2d_context.DrawLine(pt(spk_x - 3.0, vol_y - 2.0), pt(spk_x - 3.0, vol_y + 2.0), &self.brush_white, 1.2, None);

                let vol_left = col_r + 18.0;
                let vol_right = right_c - 10.0;
                let vol_w = vol_right - vol_left;
                let vol_val = island.volume.clamp(0.0, 1.0);

                self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.16 * alpha });
                self.d2d_context.DrawLine(pt(vol_left, vol_y), pt(vol_right, vol_y), &self.brush_btn_bg, 3.0, None);

                let vol_fill_x = vol_left + vol_val * vol_w;
                self.brush_white.SetOpacity(0.90 * alpha);
                if vol_val > 0.02 {
                    self.d2d_context.DrawLine(pt(vol_left, vol_y), pt(vol_fill_x, vol_y), &self.brush_white, 3.0, None);
                }
                let vol_thumb = D2D1_ELLIPSE { point: pt(vol_fill_x, vol_y), radiusX: 3.8, radiusY: 3.8 };
                self.d2d_context.FillEllipse(&vol_thumb, &self.brush_white);
            }
        }
    }

    fn draw_card_prayer_times(
        &self,
        island: &Island,
        left: f32,
        top: f32,
        right: f32,
        _bottom: f32,
        alpha: f32,
    ) {
        unsafe {
            let cx = (left + right) * 0.5;
            let top_c = top + 26.0;
            let col_l = left + 44.0;
            let col_r = cx + 12.0;
            let right_c = right - 44.0;

            // Hairline vertical divider between Left (Schedule) & Right (Arc)
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.08 * alpha });
            self.d2d_context.DrawLine(pt(cx, top_c + 4.0), pt(cx, top_c + 86.0), &self.brush_btn_bg, 1.0, None);

            let (pr, pg, pb) = island.prayer.prayer_color_rgb;
            let prayer_theme_color = D2D1_COLOR_F { r: pr, g: pg, b: pb, a: 0.95 * alpha };

            // ==================== LEFT COLUMN: PRAYER SCHEDULE & COUNTDOWN ====================
            // 1. Location Pin Icon & City Header
            self.draw_vector_location_pin(col_l + 3.0, top_c + 7.5, 0.85, &prayer_theme_color);

            let city_upper = island.prayer.city.to_uppercase();
            let pray_label_str = format!("PRAYER TIMES • {}", city_upper);
            let pray_label = pray_label_str.encode_utf16().collect::<Vec<u16>>();
            let pl_rect = D2D_RECT_F { left: col_l + 12.0, top: top_c + 2.0, right: cx - 55.0, bottom: top_c + 15.0 };
            self.brush_accent.SetColor(&prayer_theme_color);
            self.d2d_context.DrawText(&pray_label, &self.format_tag, &pl_rect as *const _, &self.brush_accent, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

            // Subtle interactive GPS pill badge [⟳ GPS]
            let gps_badge_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: cx - 48.0, top: top_c + 1.0, right: cx - 12.0, bottom: top_c + 15.0 },
                radiusX: 6.0,
                radiusY: 6.0,
            };
            let gps_bg = if island.prayer.is_gps_detecting {
                D2D1_COLOR_F { r: 0.0, g: 0.48, b: 1.0, a: 0.28 * alpha }
            } else {
                D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.08 * alpha }
            };
            self.brush_btn_bg.SetColor(&gps_bg);
            self.d2d_context.FillRoundedRectangle(&gps_badge_rect, &self.brush_btn_bg);
            let gps_text = if island.prayer.is_gps_detecting { "GPS..." } else { "⟳ GPS" };
            let gps_utf: Vec<u16> = gps_text.encode_utf16().collect();
            let gps_t_rect = D2D_RECT_F { left: cx - 48.0, top: top_c + 1.0, right: cx - 12.0, bottom: top_c + 15.0 };
            let gps_color = if island.prayer.is_gps_detecting {
                D2D1_COLOR_F { r: 0.40, g: 0.80, b: 1.0, a: 0.95 * alpha }
            } else {
                D2D1_COLOR_F { r: 0.70, g: 0.75, b: 0.85, a: 0.75 * alpha }
            };
            self.brush_subtext.SetColor(&gps_color);
            self.d2d_context.DrawText(&gps_utf, &self.format_pill_tag, &gps_t_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

            // 2. Large Bold Prayer Name (e.g. "Subuh", "Dzuhur", "Ashar")
            let next_sholat_str = island.prayer.next_prayer.name.clone();
            let next_sholat = next_sholat_str.encode_utf16().collect::<Vec<u16>>();
            let ns_rect = D2D_RECT_F { left: col_l, top: top_c + 15.0, right: cx - 10.0, bottom: top_c + 40.0 };
            self.brush_white.SetColor(&D2D1_COLOR_F { r: 0.98, g: 0.98, b: 1.0, a: 0.98 * alpha });
            self.d2d_context.DrawText(&next_sholat, &self.format_display, &ns_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

            // 3. Time and Countdown (e.g. "04:08   in 8h 25m")
            let countdown_str = format!("{}   {}", island.prayer.next_prayer.time_str, island.prayer.next_prayer.countdown_str);
            let countdown = countdown_str.encode_utf16().collect::<Vec<u16>>();
            let cd_rect = D2D_RECT_F { left: col_l, top: top_c + 40.0, right: cx - 10.0, bottom: top_c + 54.0 };
            self.brush_accent.SetColor(&prayer_theme_color);
            self.d2d_context.DrawText(&countdown, &self.format_body, &cd_rect as *const _, &self.brush_accent, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

            // 4. Progress bar towards next prayer
            let pbar_y = top_c + 57.0;
            let pb_left = col_l;
            let pb_right = cx - 14.0;
            let pb_w = pb_right - pb_left;
            let pb_ratio = island.prayer.next_prayer.progress_ratio.clamp(0.0, 1.0);

            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 * alpha });
            self.d2d_context.DrawLine(pt(pb_left, pbar_y), pt(pb_right, pbar_y), &self.brush_btn_bg, 3.2, None);

            self.brush_accent.SetColor(&prayer_theme_color);
            self.d2d_context.DrawLine(pt(pb_left, pbar_y), pt(pb_left + pb_ratio * pb_w, pbar_y), &self.brush_accent, 3.2, None);

            // 5. Compact Clipped Sub-Times (Fixing text overlap into right column!)
            let line1_str = format!(
                "Sub {}  •  Dzu {}  •  Ash {}",
                island.prayer.cached_times.fajr_str(),
                island.prayer.cached_times.dhuhr_str(),
                island.prayer.cached_times.asr_str()
            );
            let line2_str = format!(
                "Mag {}  •  Isy {}",
                island.prayer.cached_times.maghrib_str(),
                island.prayer.cached_times.isha_str()
            );
            let line1_utf: Vec<u16> = line1_str.encode_utf16().collect();
            let line2_utf: Vec<u16> = line2_str.encode_utf16().collect();
            let st_rect1 = D2D_RECT_F { left: col_l, top: top_c + 63.0, right: cx - 12.0, bottom: top_c + 75.0 };
            let st_rect2 = D2D_RECT_F { left: col_l, top: top_c + 75.0, right: cx - 12.0, bottom: top_c + 87.0 };
            self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.72, g: 0.75, b: 0.82, a: 0.75 * alpha });
            self.d2d_context.DrawText(&line1_utf, &self.format_tag, &st_rect1 as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
            self.d2d_context.DrawText(&line2_utf, &self.format_tag, &st_rect2 as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

            // ==================== RIGHT COLUMN: CELESTIAL SUN ARC ====================
            // Header
            let arc_label = "CELESTIAL SUN ARC".encode_utf16().collect::<Vec<u16>>();
            let al_rect = D2D_RECT_F { left: col_r, top: top_c + 2.0, right: right_c, bottom: top_c + 15.0 };
            self.brush_accent.SetColor(&D2D1_COLOR_F { r: 1.0, g: 0.78, b: 0.35, a: 0.85 * alpha });
            self.d2d_context.DrawText(&arc_label, &self.format_tag, &al_rect as *const _, &self.brush_accent, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);

            // Parabola Bézier Arc Geometry
            let arc_left = col_r + 10.0;
            let arc_right = right_c - 10.0;
            let arc_base_y = top_c + 66.0;
            let arc_top_y = top_c + 22.0;

            // Draw arc baseline
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.09 * alpha });
            self.d2d_context.DrawLine(pt(arc_left - 3.0, arc_base_y), pt(arc_right + 3.0, arc_base_y), &self.brush_btn_bg, 1.0, None);

            // Multi-stop gradient along the parabola trajectory (24 smooth segments)
            let steps = 24;
            let is_daylight = island.prayer.sun_arc.is_daylight;
            for s in 0..steps {
                let t1 = s as f32 / steps as f32;
                let t2 = (s + 1) as f32 / steps as f32;
                let t_mid = (t1 + t2) * 0.5;

                let x1 = arc_left + t1 * (arc_right - arc_left);
                let y1 = arc_base_y - (1.0 - 4.0 * (t1 - 0.5).powi(2)).max(0.0) * (arc_base_y - arc_top_y);

                let x2 = arc_left + t2 * (arc_right - arc_left);
                let y2 = arc_base_y - (1.0 - 4.0 * (t2 - 0.5).powi(2)).max(0.0) * (arc_base_y - arc_top_y);

                // Dynamic Apple-style celestial gradient colors
                let seg_color = if is_daylight {
                    if t_mid <= 0.40 {
                        // Dawn Coral Gold -> Midday Brilliant Sun Gold
                        let f = t_mid / 0.40;
                        D2D1_COLOR_F {
                            r: 1.0,
                            g: 0.55 + f * 0.35,
                            b: 0.25 - f * 0.15,
                            a: 0.65 * alpha,
                        }
                    } else if t_mid <= 0.70 {
                        // Midday Sun Gold -> Afternoon Amber
                        let f = (t_mid - 0.40) / 0.30;
                        D2D1_COLOR_F {
                            r: 1.0,
                            g: 0.90 - f * 0.32,
                            b: 0.10 + f * 0.10,
                            a: 0.65 * alpha,
                        }
                    } else {
                        // Afternoon Amber -> Sunset Coral Rose
                        let f = (t_mid - 0.70) / 0.30;
                        D2D1_COLOR_F {
                            r: 1.0 - f * 0.05,
                            g: 0.58 - f * 0.26,
                            b: 0.20 + f * 0.20,
                            a: 0.65 * alpha,
                        }
                    }
                } else {
                    // Night: Twilight Indigo -> Midnight Lavender
                    let f = (t_mid - 0.5).abs() * 2.0;
                    D2D1_COLOR_F {
                        r: 0.42 + (1.0 - f) * 0.18,
                        g: 0.45 + (1.0 - f) * 0.10,
                        b: 0.88 + (1.0 - f) * 0.08,
                        a: 0.50 * alpha,
                    }
                };

                self.brush_accent.SetColor(&seg_color);
                self.d2d_context.DrawLine(pt(x1, y1), pt(x2, y2), &self.brush_accent, 2.0, None);
            }

            // Waypoints & Micro-Labels for Prayer Times along the curve
            let dhuhr_t = island.prayer.sun_arc.dhuhr_ratio.clamp(0.25, 0.55);
            let asr_t = island.prayer.sun_arc.asr_ratio.clamp(0.56, 0.85);
            let waypoints: [(f32, &str, D2D1_COLOR_F); 4] = [
                (0.04, "Subuh", D2D1_COLOR_F { r: 0.19, g: 0.82, b: 0.35, a: 0.85 * alpha }),
                (dhuhr_t, "Dzuhur", D2D1_COLOR_F { r: 1.00, g: 0.84, b: 0.04, a: 0.85 * alpha }),
                (asr_t, "Ashar", D2D1_COLOR_F { r: 1.00, g: 0.58, b: 0.04, a: 0.85 * alpha }),
                (0.96, "Maghrib", D2D1_COLOR_F { r: 1.00, g: 0.28, b: 0.25, a: 0.85 * alpha }),
            ];

            for (wp, label, wp_color) in waypoints {
                let wx = arc_left + wp * (arc_right - arc_left);
                let wy = arc_base_y - (1.0 - 4.0 * (wp - 0.5).powi(2)).max(0.0) * (arc_base_y - arc_top_y);

                // Waypoint dot
                let dot = D2D1_ELLIPSE { point: pt(wx, wy), radiusX: 2.4, radiusY: 2.4 };
                self.brush_accent.SetColor(&wp_color);
                self.d2d_context.FillEllipse(&dot, &self.brush_accent);

                // Micro-label above or below waypoint
                let label_utf: Vec<u16> = label.encode_utf16().collect();
                let lbl_top = if wy < arc_base_y - 18.0 { wy - 12.0 } else { wy + 2.0 };
                let lbl_rect = D2D_RECT_F {
                    left: wx - 20.0,
                    top: lbl_top,
                    right: wx + 20.0,
                    bottom: lbl_top + 10.0,
                };
                self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.82, g: 0.85, b: 0.90, a: 0.65 * alpha });
                self.d2d_context.DrawText(&label_utf, &self.format_pill_tag, &lbl_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
            }

            // Animated Celestial Sun / Moon Orb (Driven by island.sun_anim_t)
            let sun_t = island.sun_anim_t.current.clamp(0.0, 1.0);
            let sun_x = arc_left + sun_t * (arc_right - arc_left);
            let sun_h = (1.0f32 - 4.0f32 * (sun_t - 0.5f32).powi(2)).max(0.0f32);
            let sun_y = arc_base_y - sun_h * (arc_base_y - arc_top_y);

            // Ambient soft glow halo around orb
            let halo_color = if is_daylight {
                D2D1_COLOR_F { r: 1.0, g: 0.75, b: 0.20, a: 0.22 * alpha }
            } else {
                D2D1_COLOR_F { r: 0.50, g: 0.65, b: 1.0, a: 0.18 * alpha }
            };
            let glow = D2D1_ELLIPSE { point: pt(sun_x, sun_y), radiusX: 11.0, radiusY: 11.0 };
            self.brush_accent.SetColor(&halo_color);
            self.d2d_context.FillEllipse(&glow, &self.brush_accent);

            // Inner Core Vector Icon (Sun with micro rays or Crescent Moon)
            if is_daylight {
                let sun_color = D2D1_COLOR_F { r: 1.0, g: 0.92, b: 0.40, a: 0.98 * alpha };
                self.draw_vector_sun(sun_x, sun_y, 0.95, &sun_color);
            } else {
                let moon_color = D2D1_COLOR_F { r: 0.82, g: 0.88, b: 1.0, a: 0.98 * alpha };
                self.draw_vector_crescent_moon(sun_x, sun_y, 0.95, &moon_color);
            }

            // Bottom Right Status with Vector Celestial Icon (Sun or Moon)
            if is_daylight {
                self.draw_vector_sun(col_r + 6.0, top_c + 78.0, 0.75, &D2D1_COLOR_F { r: 1.0, g: 0.80, b: 0.30, a: 0.85 * alpha });
            } else {
                self.draw_vector_crescent_moon(col_r + 6.0, top_c + 78.0, 0.75, &D2D1_COLOR_F { r: 0.75, g: 0.82, b: 1.0, a: 0.85 * alpha });
            }

            let status_utf: Vec<u16> = island.prayer.sun_arc.status_text.encode_utf16().collect();
            let ls_rect = D2D_RECT_F { left: col_r + 16.0, top: top_c + 71.0, right: right_c, bottom: top_c + 85.0 };
            self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.94, a: 0.75 * alpha });
            self.d2d_context.DrawText(&status_utf, &self.format_date, &ls_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
        }
    }

    fn draw_card_water_reminder(
        &self,
        island: &Island,
        left: f32,
        top: f32,
        right: f32,
        _bottom: f32,
        alpha: f32,
    ) {
        unsafe {
            let cx = (left + right) * 0.5;
            let top_c = top + 26.0;
            let col_l = left + 44.0;
            let col_r = cx + 12.0;
            let right_c = right - 44.0;

            // Hairline vertical divider
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.08 * alpha });
            self.d2d_context.DrawLine(pt(cx, top_c + 4.0), pt(cx, top_c + 86.0), &self.brush_btn_bg, 1.0, None);

            let current_l = island.hydration.data.current_ml as f32 / 1000.0;
            let target_l = island.hydration.data.target_ml as f32 / 1000.0;
            let pct = island.hydration.get_percentage();
            let pct_val = (pct * 100.0).round() as u32;

            // Left Column: Intake stats & quick drink button (Matching 05_water_reminder.png)
            let wr_label = "HYDRATION TRACKER".encode_utf16().collect::<Vec<u16>>();
            let wl_rect = D2D_RECT_F { left: col_l, top: top_c + 2.0, right: cx - 10.0, bottom: top_c + 15.0 };
            let cyan_color = D2D1_COLOR_F { r: 0.25, g: 0.78, b: 0.98, a: 0.95 * alpha };
            self.brush_accent.SetColor(&cyan_color);
            self.d2d_context.DrawText(&wr_label, &self.format_tag, &wl_rect as *const _, &self.brush_accent, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            let vol_str = format!("{:.1}", current_l);
            let vol_utf: Vec<u16> = vol_str.encode_utf16().collect();
            let vs_rect = D2D_RECT_F { left: col_l, top: top_c + 14.0, right: col_l + 44.0, bottom: top_c + 40.0 };
            self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
            self.brush_white.SetOpacity(0.98 * alpha);
            self.d2d_context.DrawText(&vol_utf, &self.format_display, &vs_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            let target_str = format!("/ {:.1} L", target_l);
            let ts_utf: Vec<u16> = target_str.encode_utf16().collect();
            let ts_rect = D2D_RECT_F { left: col_l + 46.0, top: top_c + 22.0, right: cx - 10.0, bottom: top_c + 38.0 };
            self.brush_subtext.SetOpacity(0.75 * alpha);
            self.d2d_context.DrawText(&ts_utf, &self.format_body, &ts_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            let status_str = format!("Daily Goal: {:.1} L ({}%)", target_l, pct_val);
            let ss_utf: Vec<u16> = status_str.encode_utf16().collect();
            let ss_rect = D2D_RECT_F { left: col_l, top: top_c + 40.0, right: cx - 10.0, bottom: top_c + 54.0 };
            self.d2d_context.DrawText(&ss_utf, &self.format_date, &ss_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            // Pill Button 1: [+250 ml] with custom vector water drop (strictly NO emojis!)
            let btn_water = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: col_l, top: top_c + 56.0, right: col_l + 78.0, bottom: top_c + 78.0 },
                radiusX: 11.0,
                radiusY: 11.0,
            };
            self.brush_accent.SetColor(&D2D1_COLOR_F { r: 0.18, g: 0.65, b: 0.95, a: 0.20 * alpha });
            self.d2d_context.FillRoundedRectangle(&btn_water, &self.brush_accent);
            self.brush_accent.SetColor(&cyan_color);
            self.d2d_context.DrawRoundedRectangle(&btn_water, &self.brush_accent, 1.0, None);

            self.draw_vector_water_drop(col_l + 12.0, top_c + 67.0, 0.70, &cyan_color);
            let bw_txt = "+250 ml".encode_utf16().collect::<Vec<u16>>();
            let bwt_rect = D2D_RECT_F { left: col_l + 22.0, top: top_c + 58.0, right: col_l + 76.0, bottom: top_c + 76.0 };
            self.brush_white.SetOpacity(0.95 * alpha);
            self.d2d_context.DrawText(&bw_txt, &self.format_tag, &bwt_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            // Pill Button 2: [Target] with custom vector sliders (Click to cycle target)
            let btn_settings = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: col_l + 84.0, top: top_c + 56.0, right: col_l + 146.0, bottom: top_c + 78.0 },
                radiusX: 11.0,
                radiusY: 11.0,
            };
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.10 * alpha });
            self.d2d_context.FillRoundedRectangle(&btn_settings, &self.brush_btn_bg);
            let white_color = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.90 * alpha };
            self.draw_vector_sliders(col_l + 96.0, top_c + 67.0, 0.75, &white_color);
            let set_txt = "Target".encode_utf16().collect::<Vec<u16>>();
            let st_rect = D2D_RECT_F { left: col_l + 106.0, top: top_c + 58.0, right: col_l + 144.0, bottom: top_c + 76.0 };
            self.d2d_context.DrawText(&set_txt, &self.format_tag, &st_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            // Right Column: Circular Progress Arc Gauge (Matching 05_water_reminder.png)
            let gauge_cx = (col_r + right_c) * 0.5;
            let gauge_cy = top_c + 38.0;
            let gauge_r = 24.0;

            // Background circle
            let bg_circle = D2D1_ELLIPSE { point: pt(gauge_cx, gauge_cy), radiusX: gauge_r, radiusY: gauge_r };
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.12 * alpha });
            self.d2d_context.DrawEllipse(&bg_circle, &self.brush_btn_bg, 3.6, None);

            // Filled progress arc (pct around circle)
            let arc_steps = 28;
            let filled_steps = ((arc_steps as f32) * pct).round() as usize;
            self.brush_accent.SetColor(&cyan_color);

            let start_angle = -std::f32::consts::FRAC_PI_2;
            for s in 0..filled_steps {
                let a1 = start_angle + (s as f32 / arc_steps as f32) * std::f32::consts::PI * 2.0;
                let a2 = start_angle + ((s + 1) as f32 / arc_steps as f32) * std::f32::consts::PI * 2.0;
                let p1 = pt(gauge_cx + gauge_r * a1.cos(), gauge_cy + gauge_r * a1.sin());
                let p2 = pt(gauge_cx + gauge_r * a2.cos(), gauge_cy + gauge_r * a2.sin());
                self.d2d_context.DrawLine(p1, p2, &self.brush_accent, 3.6, None);
            }

            // Center percentage text
            let pct_str = format!("{}%", pct_val);
            let pct_txt: Vec<u16> = pct_str.encode_utf16().collect();
            let p_rect = D2D_RECT_F { left: gauge_cx - 24.0, top: gauge_cy - 12.0, right: gauge_cx + 24.0, bottom: gauge_cy + 8.0 };
            self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
            self.brush_white.SetOpacity(0.98 * alpha);
            self.d2d_context.DrawText(&pct_txt, &self.format_display, &p_rect as *const _, &self.brush_white, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);

            // Vector water drop below percentage
            self.draw_vector_water_drop(gauge_cx, gauge_cy + 13.0, 0.50, &cyan_color);

            let rem_label = if pct >= 1.0 {
                "Daily goal achieved! Stay fresh".to_string()
            } else if island.hydration.data.reminder_enabled {
                format!("Next reminder in {}m", island.hydration.get_next_reminder_mins())
            } else {
                "Reminder paused".to_string()
            };
            let rl_utf: Vec<u16> = rem_label.encode_utf16().collect();
            let rl_rect = D2D_RECT_F { left: gauge_cx - 52.0, top: top_c + 68.0, right: gauge_cx + 70.0, bottom: top_c + 82.0 };
            let bell_col = D2D1_COLOR_F { r: 0.55, g: 0.58, b: 0.65, a: 0.75 * alpha };
            self.draw_vector_bell_icon(gauge_cx - 60.0, top_c + 75.0, 0.70, &bell_col);
            self.brush_subtext.SetOpacity(0.70 * alpha);
            self.d2d_context.DrawText(&rl_utf, &self.format_date, &rl_rect as *const _, &self.brush_subtext, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
    }

    /// Renders the rich modular Shelf Container (Carousel) supporting 5 interactive cards.
    fn draw_shelf_container(
        &mut self,
        island: &Island,
        config: &crate::config::AppConfig,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        alpha: f32,
    ) {
        unsafe {
            let outer_clip = D2D_RECT_F { left, top, right, bottom };
            self.d2d_context.PushAxisAlignedClip(&outer_clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

            // 1. Top Status Bar (Module Title, Battery Ring, Date & Time, Audio Visualizer, Close Button)
            self.draw_shelf_top_bar(island, config, left, top, right, alpha);

            // 2. Sliding Content Area (Two Columns, Carousel sliding between 5 cards)
            let clip_rect = D2D_RECT_F {
                left: left + 14.0,
                top: top + 26.0,
                right: right - 14.0,
                bottom: bottom - 18.0,
            };
            self.d2d_context.PushAxisAlignedClip(&clip_rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

            let card_width = right - left;
            let cur_pos = island.card_scroll_spring.current;

            for i in 0..crate::island::SHELF_CARD_COUNT {
                let diff = (i as f32) - cur_pos;
                if diff.abs() < 1.05 {
                    let dx = diff * card_width;
                    let card_alpha = (1.0 - diff.abs()).clamp(0.0, 1.0) * alpha;

                    match crate::island::ShelfCard::from_index(i) {
                        crate::island::ShelfCard::Today => {
                            self.draw_card_today(island, left + dx, top, right + dx, bottom, card_alpha);
                        }
                        crate::island::ShelfCard::TodoList => {
                            self.draw_card_todolist(island, left + dx, top, right + dx, bottom, card_alpha);
                        }
                        crate::island::ShelfCard::MusicLyrics => {
                            self.draw_card_music_lyrics(island, config, left + dx, top, right + dx, bottom, card_alpha);
                        }
                        crate::island::ShelfCard::PrayerTimes => {
                            self.draw_card_prayer_times(island, left + dx, top, right + dx, bottom, card_alpha);
                        }
                        crate::island::ShelfCard::WaterReminder => {
                            self.draw_card_water_reminder(island, left + dx, top, right + dx, bottom, card_alpha);
                        }
                    }
                }
            }

            self.d2d_context.PopAxisAlignedClip();

            // 3. Bottom Pagination Bar (Morphing capsule dots + Side chevrons < >)
            self.draw_shelf_bottom_bar(island, left, top, right, bottom, alpha);

            self.d2d_context.PopAxisAlignedClip();
        }
    }

    /// Renders vector media controls (Previous, Play/Pause, Next) on the right side of the pill.
    fn draw_media_controls(&self, right: f32, cy: f32, is_playing: bool, alpha: f32) {
        unsafe {
            // Previous button: cx = right - 124.0
            let prev_cx = right - 124.0;
            let prev_bg = D2D1_ELLIPSE {
                point: windows_numerics::Vector2 { X: prev_cx, Y: cy },
                radiusX: 13.5,
                radiusY: 13.5,
            };
            self.brush_btn_bg.SetOpacity(0.50 * alpha);
            self.d2d_context.FillEllipse(&prev_bg, &self.brush_btn_bg);

            self.brush_white.SetOpacity(0.90 * alpha);
            // Vertical bar
            self.d2d_context.DrawLine(
                pt(prev_cx - 5.0, cy - 4.5),
                pt(prev_cx - 5.0, cy + 4.5),
                &self.brush_white,
                1.6,
                None,
            );
            // Left triangle lines
            self.d2d_context.DrawLine(
                pt(prev_cx + 4.0, cy - 5.0),
                pt(prev_cx - 3.5, cy),
                &self.brush_white,
                1.6,
                None,
            );
            self.d2d_context.DrawLine(
                pt(prev_cx + 4.0, cy + 5.0),
                pt(prev_cx - 3.5, cy),
                &self.brush_white,
                1.6,
                None,
            );
            self.d2d_context.DrawLine(
                pt(prev_cx + 4.0, cy - 5.0),
                pt(prev_cx + 4.0, cy + 5.0),
                &self.brush_white,
                1.6,
                None,
            );

            // Play / Pause button: cx = right - 88.0 (prominent, radius 16.0)
            let play_cx = right - 88.0;
            let play_bg = D2D1_ELLIPSE {
                point: windows_numerics::Vector2 { X: play_cx, Y: cy },
                radiusX: 16.0,
                radiusY: 16.0,
            };
            self.brush_btn_bg.SetOpacity(0.85 * alpha);
            self.d2d_context.FillEllipse(&play_bg, &self.brush_btn_bg);

            self.brush_white.SetOpacity(1.0 * alpha);
            if is_playing {
                // Pause: two vertical bars
                self.d2d_context.DrawLine(
                    pt(play_cx - 3.2, cy - 5.5),
                    pt(play_cx - 3.2, cy + 5.5),
                    &self.brush_white,
                    2.2,
                    None,
                );
                self.d2d_context.DrawLine(
                    pt(play_cx + 3.2, cy - 5.5),
                    pt(play_cx + 3.2, cy + 5.5),
                    &self.brush_white,
                    2.2,
                    None,
                );
            } else {
                // Play: right triangle
                self.d2d_context.DrawLine(
                    pt(play_cx - 3.5, cy - 6.0),
                    pt(play_cx + 5.5, cy),
                    &self.brush_white,
                    1.8,
                    None,
                );
                self.d2d_context.DrawLine(
                    pt(play_cx - 3.5, cy + 6.0),
                    pt(play_cx + 5.5, cy),
                    &self.brush_white,
                    1.8,
                    None,
                );
                self.d2d_context.DrawLine(
                    pt(play_cx - 3.5, cy - 6.0),
                    pt(play_cx - 3.5, cy + 6.0),
                    &self.brush_white,
                    1.8,
                    None,
                );
            }

            // Next button: cx = right - 52.0
            let next_cx = right - 52.0;
            let next_bg = D2D1_ELLIPSE {
                point: windows_numerics::Vector2 { X: next_cx, Y: cy },
                radiusX: 13.5,
                radiusY: 13.5,
            };
            self.brush_btn_bg.SetOpacity(0.50 * alpha);
            self.d2d_context.FillEllipse(&next_bg, &self.brush_btn_bg);

            self.brush_white.SetOpacity(0.90 * alpha);
            // Right triangle
            self.d2d_context.DrawLine(
                pt(next_cx - 4.0, cy - 5.0),
                pt(next_cx + 3.5, cy),
                &self.brush_white,
                1.6,
                None,
            );
            self.d2d_context.DrawLine(
                pt(next_cx - 4.0, cy + 5.0),
                pt(next_cx + 3.5, cy),
                &self.brush_white,
                1.6,
                None,
            );
            self.d2d_context.DrawLine(
                pt(next_cx - 4.0, cy - 5.0),
                pt(next_cx - 4.0, cy + 5.0),
                &self.brush_white,
                1.6,
                None,
            );
            // Vertical bar
            self.d2d_context.DrawLine(
                pt(next_cx + 5.0, cy - 4.5),
                pt(next_cx + 5.0, cy + 4.5),
                &self.brush_white,
                1.6,
                None,
            );

            // 4. Dedicated Expand View Button: cx = right - 20.0
            let toggle_cx = right - 20.0;
            let toggle_bg = D2D1_ELLIPSE {
                point: windows_numerics::Vector2 { X: toggle_cx, Y: cy },
                radiusX: 12.5,
                radiusY: 12.5,
            };
            self.brush_btn_bg.SetOpacity(0.60 * alpha);
            self.d2d_context.FillEllipse(&toggle_bg, &self.brush_btn_bg);

            self.brush_white.SetOpacity(0.90 * alpha);
            self.d2d_context.DrawLine(
                pt(toggle_cx - 3.8, cy - 2.0),
                pt(toggle_cx, cy + 2.5),
                &self.brush_white,
                1.7,
                None,
            );
            self.d2d_context.DrawLine(
                pt(toggle_cx, cy + 2.5),
                pt(toggle_cx + 3.8, cy - 2.0),
                &self.brush_white,
                1.7,
                None,
            );
        }
    }

    unsafe fn draw_compact_lyric_or_title(
        &self,
        island: &Island,
        config: &crate::config::AppConfig,
        clip_left: f32,
        clip_right: f32,
        top: f32,
        bottom: f32,
        compact_alpha: f32,
    ) {
        let clip_rect = D2D_RECT_F { left: clip_left, top, right: clip_right, bottom };
        self.d2d_context.PushAxisAlignedClip(&clip_rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

        let lyric_info_opt = if config.show_lyrics_in_capsule {
            if let Some(ref lyrics) = island.current_lyrics {
                let pos = island.current_track.as_ref().map(|t| t.position_secs).unwrap_or(0.0);
                let info = lyrics.get_active_line_info(pos, island.romanize_cjk_lyrics);
                if !info.curr.is_empty() && info.curr != "•••" && info.curr != "No lyrics available" && info.curr != "Instrumental Break" {
                    Some((info.curr.to_string(), info.progress, info.line_progress, info.fade_alpha))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let available_w = (clip_right - clip_left).max(10.0);

        if let Some((curr_str, progress, _line_progress, fade_alpha)) = lyric_info_opt {
            let utf16: Vec<u16> = curr_str.encode_utf16().collect();

            // Accurately measure the exact rendered glyph width with DirectWrite
            let text_width = if let Ok(layout) = self.dwrite_factory.CreateTextLayout(&utf16, &self.format_body, 3000.0, 40.0) {
                let mut metrics = DWRITE_TEXT_METRICS::default();
                if layout.GetMetrics(&mut metrics).is_ok() {
                    metrics.widthIncludingTrailingWhitespace.max(metrics.width)
                } else {
                    (utf16.len() as f32) * 6.0
                }
            } else {
                (utf16.len() as f32) * 6.0
            };

            // Stable, High-Legibility Karaoke Display:
            // 1. Text begins cleanly at clip_left + 4.0.
            // 2. Fitting lines (text_width <= available_w):
            //    Remains 100% stationary at clip_left + 4.0! Zero scrolling, zero cut-off!
            // 3. Overflowing lines (text_width > available_w):
            //    - Initial Reading Phase: First words sit stationary at clip_left + 4.0 while being sung!
            //    - Gentle Tracking Phase: Only when vocal progress approaches the right margin of the capsule
            //      (sung_w > available_w - 38.0), the line rolls gently left so the active word stays in view!
            let sung_w = text_width * progress;
            let draw_x = if text_width <= available_w {
                clip_left + 4.0
            } else {
                let overflow = text_width - available_w + 10.0;
                let trigger_point = (available_w - 38.0).max(40.0);
                let target_scroll = if sung_w > trigger_point {
                    (sung_w - trigger_point).clamp(0.0, overflow)
                } else {
                    0.0
                };
                clip_left + 4.0 - target_scroll
            };

            let max_draw_w = text_width.max(available_w) + 20.0;
            let text_rect = D2D_RECT_F {
                left: draw_x,
                top: top + 4.0,
                right: draw_x + max_draw_w,
                bottom: bottom - 4.0,
            };

            // Pass A: Unsung base text with In/Out Opacity Fade (0.40 * compact_alpha * fade_alpha)
            self.brush_subtext.SetOpacity(0.40 * compact_alpha * fade_alpha);
            self.d2d_context.DrawText(
                &utf16,
                &self.format_body,
                &text_rect as *const _,
                &self.brush_subtext,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // Pass B: Sung text with In/Out Opacity Fade (0.98 * compact_alpha * fade_alpha)
            let sung_w = text_width * progress;
            if sung_w > 0.5 && fade_alpha > 0.01 {
                let sung_clip = D2D_RECT_F {
                    left: clip_left,
                    top,
                    right: (draw_x + sung_w).min(clip_right).max(clip_left),
                    bottom,
                };
                if sung_clip.right > sung_clip.left {
                    self.d2d_context.PushAxisAlignedClip(&sung_clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                    self.brush_white.SetOpacity(0.98 * compact_alpha * fade_alpha);
                    self.d2d_context.DrawText(
                        &utf16,
                        &self.format_body,
                        &text_rect as *const _,
                        &self.brush_white,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    self.d2d_context.PopAxisAlignedClip();
                }
            }
        } else {
            // Standard Title + Artist view (Static or Marquee) during Intro / Instrumental breaks
            let display_text = if let Some(track) = &island.current_track {
                format!("{} • {}", track.title, track.artist)
            } else {
                "Music Playing".to_string()
            };
            let utf16: Vec<u16> = display_text.encode_utf16().collect();
            let text_width = (utf16.len() as f32) * 6.8;

            self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
            self.brush_white.SetOpacity(0.98 * compact_alpha);
            if text_width <= available_w {
                let rect_static = D2D_RECT_F {
                    left: clip_left,
                    top: top + 4.0,
                    right: clip_right,
                    bottom: bottom - 4.0,
                };
                self.d2d_context.DrawText(
                    &utf16,
                    &self.format_body,
                    &rect_static as *const _,
                    &self.brush_white,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            } else {
                let cycle = text_width + 42.0;
                let offset = island.marquee_offset % cycle;

                let rect1 = D2D_RECT_F {
                    left: clip_left - offset,
                    top: top + 4.0,
                    right: clip_left - offset + text_width,
                    bottom: bottom - 4.0,
                };
                self.d2d_context.DrawText(
                    &utf16,
                    &self.format_body,
                    &rect1 as *const _,
                    &self.brush_white,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );

                let rect2 = D2D_RECT_F {
                    left: clip_left - offset + cycle,
                    top: top + 4.0,
                    right: clip_left - offset + cycle + text_width,
                    bottom: bottom - 4.0,
                };
                self.d2d_context.DrawText(
                    &utf16,
                    &self.format_body,
                    &rect2 as *const _,
                    &self.brush_white,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }

        self.d2d_context.PopAxisAlignedClip();
    }

    pub fn render(&mut self, island: &Island, config: &crate::config::AppConfig) -> Result<()> {
        unsafe {
            self.d2d_context.BeginDraw();

            // 1. Clear background to 100% transparent
            self.d2d_context.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));

            let (left, top, right, bottom) = island.get_pill_rect();
            let radius = island.radius_spring.current;

            let pill_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left, top, right, bottom },
                radiusX: radius,
                radiusY: radius,
            };

            // The island is an obsidian capsule overlay; text and buttons remain crisp white for high contrast & legibility
            self.brush_white.SetOpacity(1.0);
            self.brush_subtext.SetOpacity(1.0);
            self.brush_time.SetOpacity(1.0);
            self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
            self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.80, g: 0.82, b: 0.88, a: 1.0 });
            self.brush_time.SetColor(&D2D1_COLOR_F { r: 0.70, g: 0.72, b: 0.78, a: 1.0 });

            let btn_bg_color = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.14 };
            self.brush_btn_bg.SetColor(&btn_bg_color);

            // 2. Draw outer ambient luminous glow halo using dynamic album color
            if config.glow_intensity > 0.01 {
                let glow_alpha = (config.glow_intensity * 0.35).min(0.45);
                let (gr, gg, gb) = if config.dynamic_album_color {
                    self.accent_color
                } else {
                    (0.18, 0.70, 1.0)
                };
                for i in 1..=4 {
                    let spread = (i as f32) * 1.5;
                    let glow_rect = D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: left - spread,
                            top: top - spread,
                            right: right + spread,
                            bottom: bottom + spread,
                        },
                        radiusX: radius + spread,
                        radiusY: radius + spread,
                    };
                    let ring_alpha = glow_alpha * (1.0 - (i as f32) * 0.22);
                    self.brush_accent.SetColor(&D2D1_COLOR_F { r: gr, g: gg, b: gb, a: ring_alpha });
                    self.d2d_context.DrawRoundedRectangle(&glow_rect, &self.brush_accent, 1.2, None);
                }
            }

            // 3. Draw pill background with user-configured opacity (Strictly reset opacity)
            self.brush_bg.SetOpacity(1.0);
            let op = config.island_opacity.clamp(0.20, 1.0);
            self.brush_bg.SetColor(&D2D1_COLOR_F {
                r: 0.04 * op,
                g: 0.04 * op,
                b: 0.05 * op,
                a: op,
            });
            self.d2d_context.FillRoundedRectangle(&pill_rect, &self.brush_bg);

            // 4. Draw subtle luminous rim / border (tinted dynamically with album color)
            let (br, bg, bb) = if config.dynamic_album_color {
                self.accent_color
            } else {
                (1.0, 1.0, 1.0)
            };
            let border_alpha = 0.30 + config.glow_intensity * 0.45;
            self.brush_border.SetColor(&D2D1_COLOR_F { r: br, g: bg, b: bb, a: border_alpha });
            self.d2d_context.DrawRoundedRectangle(&pill_rect, &self.brush_border, 1.2, None);

            // 5. Render Content with Seamless Dynamic Cross-Fade
            let alpha = island.alpha_spring.current.clamp(0.0, 1.0);
            let shelf_factor = island.shelf_expand.current.clamp(0.0, 1.0);
            let pill_h = bottom - top;

            // --- A. Render Shelf Container when shelf_expand spring is active ---
            if shelf_factor > 0.005 && pill_h >= 45.0 {
                self.draw_shelf_container(island, config, left, top, right, bottom, shelf_factor);
            }

            // --- B. Render Compact / Idle / Notification Capsule when shelf is collapsed or collapsing ---
            if shelf_factor < 0.995 {
                let compact_alpha = (1.0 - shelf_factor).clamp(0.0, 1.0);

                let has_active_notif = (island.mode == IslandMode::Slim || island.mode == IslandMode::Expanded)
                    && island.active_notification.is_some();

                if has_active_notif {
                    let notif = island.active_notification.as_ref().unwrap();
                    // 1. Notification Mode (Slim or Expanded)
                    if alpha > 0.05 {
                        let cy = (top + bottom) * 0.5;
                        let notif_alpha = compact_alpha * alpha;
                        self.brush_white.SetOpacity(notif_alpha);
                        self.brush_subtext.SetOpacity(notif_alpha);
                        self.brush_time.SetOpacity(notif_alpha);

                        let lower_app = notif.app_name.to_lowercase();
                        let is_media_notif = lower_app.contains("music")
                            || lower_app.contains("spotify")
                            || lower_app.contains("apple music")
                            || lower_app.contains("deezer")
                            || lower_app.contains("tidal");

                        if pill_h <= 36.0 {
                            // Stage 1: Slim notification strip
                            let badge_radius = 9.0;
                            let badge_cx = left + 18.0;

                            self.draw_notification_badge(notif, is_media_notif, badge_cx, cy, badge_radius, notif_alpha);

                            let text_rect = D2D_RECT_F {
                                left: badge_cx + 14.0,
                                top: top + 4.0,
                                right: right - 50.0,
                                bottom: bottom - 4.0,
                            };
                            let line_text = if notif.body.is_empty() {
                                format!("{} • {}", notif.app_name, notif.title)
                            } else {
                                format!("{} • {} — {}", notif.app_name, notif.title, notif.body)
                            };
                            let line_utf16: Vec<u16> = line_text.encode_utf16().collect();
                            self.d2d_context.DrawText(
                                &line_utf16,
                                &self.format_title,
                                &text_rect as *const _,
                                &self.brush_white,
                                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );

                            let time_rect = D2D_RECT_F {
                                left: right - 48.0,
                                top: top + 4.0,
                                right: right - 14.0,
                                bottom: bottom - 4.0,
                            };
                            let time_utf16: Vec<u16> = notif.time_str.encode_utf16().collect();
                            self.d2d_context.DrawText(
                                &time_utf16,
                                &self.format_time,
                                &time_rect as *const _,
                                &self.brush_time,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                        } else if is_media_notif {
                            // Stage 2 for Media: Rich Media Capsule on Hover with Playback Controls!
                            let vinyl_radius = (pill_h * 0.34).clamp(16.0, 20.0);
                            let vinyl_cx = left + vinyl_radius + 14.0;

                            if let Some(art) = &self.album_art {
                                self.draw_album_art_badge(art, vinyl_cx, cy, vinyl_radius, notif_alpha);
                            } else {
                                let rot = if config.enable_vinyl_animation { island.vinyl_rotation } else { 0.0 };
                                self.draw_spinning_vinyl(vinyl_cx, cy, vinyl_radius, rot, notif_alpha);
                            }

                            let text_left = vinyl_cx + vinyl_radius + 12.0;
                            let text_right = right - 144.0; // Space reserved for media controls + expand button

                            let title_rect = D2D_RECT_F {
                                left: text_left,
                                top: top + (pill_h * 0.16),
                                right: text_right - 48.0,
                                bottom: top + (pill_h * 0.50),
                            };
                            let title_text = if let Some(track) = &island.current_track {
                                &track.title
                            } else {
                                &notif.title
                            };
                            let title_utf16: Vec<u16> = title_text.encode_utf16().collect();
                            self.brush_white.SetOpacity(notif_alpha);
                            self.d2d_context.DrawText(
                                &title_utf16,
                                &self.format_title,
                                &title_rect as *const _,
                                &self.brush_white,
                                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );

                            let artist_rect = D2D_RECT_F {
                                left: text_left,
                                top: top + (pill_h * 0.50),
                                right: text_right - 42.0,
                                bottom: top + (pill_h * 0.88),
                            };
                            let artist_text = if let Some(track) = &island.current_track {
                                if track.artist.is_empty() { "Now Playing" } else { &track.artist }
                            } else if notif.body.is_empty() {
                                "Now Playing"
                            } else {
                                &notif.body
                            };
                            let artist_utf16: Vec<u16> = artist_text.encode_utf16().collect();
                            self.brush_subtext.SetOpacity(notif_alpha);
                            self.d2d_context.DrawText(
                                &artist_utf16,
                                &self.format_body,
                                &artist_rect as *const _,
                                &self.brush_subtext,
                                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );

                            // Real-time audio visualizer waves
                            if config.enable_media_island {
                                self.draw_audio_visualizer(
                                    text_right - 26.0,
                                    top + (pill_h * 0.68),
                                    island.audio_peak,
                                    island.audio_phase,
                                    11,
                                    18.0,
                                    notif_alpha * 0.95,
                                );
                            }

                            // Draw Vector Media Controls (⏮ ⏯ ⏭ ^)
                            let is_really_playing = island.current_track.as_ref().map(|t| t.is_playing).unwrap_or(island.is_media_playing);
                            self.draw_media_controls(
                                right,
                                cy,
                                is_really_playing,
                                notif_alpha,
                            );
                        } else {
                            // Stage 2: Normal non-media notification (e.g. Snipping tool, mail)
                            let badge_radius = (pill_h * 0.32).clamp(15.0, 19.0);
                            let badge_cx = left + badge_radius + 14.0;

                            self.draw_notification_badge(notif, is_media_notif, badge_cx, cy, badge_radius, notif_alpha);

                            let text_left = badge_cx + badge_radius + 12.0;
                            let text_right = right - 54.0;

                            let title_rect = D2D_RECT_F {
                                left: text_left,
                                top: top + (pill_h * 0.16),
                                right: text_right,
                                bottom: top + (pill_h * 0.50),
                            };
                            let title_text = format!("{} • {}", notif.app_name, notif.title);
                            let title_utf16: Vec<u16> = title_text.encode_utf16().collect();
                            self.d2d_context.DrawText(
                                &title_utf16,
                                &self.format_title,
                                &title_rect as *const _,
                                &self.brush_white,
                                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );

                            let body_rect = D2D_RECT_F {
                                left: text_left,
                                top: top + (pill_h * 0.50),
                                right: text_right + 34.0,
                                bottom: top + (pill_h * 0.88),
                            };
                            let body_utf16: Vec<u16> = notif.body.encode_utf16().collect();
                            self.d2d_context.DrawText(
                                &body_utf16,
                                &self.format_body,
                                &body_rect as *const _,
                                &self.brush_subtext,
                                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );

                            let time_rect = D2D_RECT_F {
                                left: right - 64.0,
                                top: top + (pill_h * 0.16),
                                right: right - 16.0,
                                bottom: top + (pill_h * 0.50),
                            };
                            let time_utf16: Vec<u16> = notif.time_str.encode_utf16().collect();
                            self.d2d_context.DrawText(
                                &time_utf16,
                                &self.format_time,
                                &time_rect as *const _,
                                &self.brush_time,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                        }
                    }
                } else if island.mode == IslandMode::Expanded && (island.current_track.is_some() || island.is_media_playing) {
                    // 2. Expanded Music Capsule Mode (Hover or Playback)
                    let cy = (top + bottom) * 0.5;
                    let exp_alpha = compact_alpha * alpha;
                    self.brush_white.SetOpacity(exp_alpha);
                    self.brush_subtext.SetOpacity(exp_alpha);

                    let vinyl_radius = (pill_h * 0.34).clamp(16.0, 20.0);
                    let vinyl_cx = left + vinyl_radius + 14.0;

                    let rot = if config.enable_vinyl_animation { island.vinyl_rotation } else { 0.0 };
                    self.draw_spinning_vinyl(
                        vinyl_cx,
                        cy,
                        vinyl_radius,
                        rot,
                        exp_alpha,
                    );

                    let text_left = vinyl_cx + vinyl_radius + 12.0;
                    let text_right = right - 144.0;

                    if let Some(track) = &island.current_track {
                        let title_rect = D2D_RECT_F {
                            left: text_left,
                            top: top + (pill_h * 0.16),
                            right: text_right - 48.0,
                            bottom: top + (pill_h * 0.50),
                        };
                        let title_utf16: Vec<u16> = track.title.encode_utf16().collect();
                        self.d2d_context.DrawText(
                            &title_utf16,
                            &self.format_title,
                            &title_rect as *const _,
                            &self.brush_white,
                            D2D1_DRAW_TEXT_OPTIONS_CLIP,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        let artist_rect = D2D_RECT_F {
                            left: text_left,
                            top: top + (pill_h * 0.50),
                            right: text_right - 42.0,
                            bottom: top + (pill_h * 0.88),
                        };
                        let artist_text = if track.artist.is_empty() {
                            "Now Playing".to_string()
                        } else {
                            track.artist.clone()
                        };
                        let artist_utf16: Vec<u16> = artist_text.encode_utf16().collect();
                        self.d2d_context.DrawText(
                            &artist_utf16,
                            &self.format_body,
                            &artist_rect as *const _,
                            &self.brush_subtext,
                            D2D1_DRAW_TEXT_OPTIONS_CLIP,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }

                    if config.enable_media_island {
                        self.draw_audio_visualizer(
                            text_right - 26.0,
                            top + (pill_h * 0.68),
                            island.audio_peak,
                            island.audio_phase,
                            11,
                            18.0,
                            exp_alpha * 0.95,
                        );
                    }

                    let is_really_playing = island.current_track.as_ref().map(|t| t.is_playing).unwrap_or(island.is_media_playing);
                    self.draw_media_controls(
                        right,
                        cy,
                        is_really_playing,
                        exp_alpha,
                    );
                } else if island.is_media_playing {
                    // 3. Compact Active Media Pill (Album thumbnail + Marquee Title + Audio wave)
                    let center_y = (top + bottom) * 0.5;
                    let thumb_cx = left + 14.0;
                    let thumb_r = 10.0;

                    if let Some(art) = &self.album_art {
                        let rot = if config.enable_vinyl_animation { island.vinyl_rotation } else { 0.0 };
                        self.draw_rotating_album_circle(art, thumb_cx, center_y, thumb_r, rot, 0.95 * compact_alpha);

                        let thumb_circle = D2D1_ELLIPSE {
                            point: windows_numerics::Vector2 { X: thumb_cx, Y: center_y },
                            radiusX: thumb_r,
                            radiusY: thumb_r,
                        };
                        let rim_color = D2D1_COLOR_F { r: self.accent_color.0, g: self.accent_color.1, b: self.accent_color.2, a: 0.70 * compact_alpha };
                        self.brush_border.SetColor(&rim_color);
                        self.d2d_context.DrawEllipse(&thumb_circle, &self.brush_border, 1.2, None);
                    } else {
                        let note_color = D2D1_COLOR_F {
                            r: self.accent_color.0,
                            g: self.accent_color.1,
                            b: self.accent_color.2,
                            a: 0.95 * compact_alpha,
                        };
                        self.draw_vector_music_note(thumb_cx, center_y, 0.85, &note_color);

                        let thumb_circle = D2D1_ELLIPSE {
                            point: windows_numerics::Vector2 { X: thumb_cx, Y: center_y },
                            radiusX: thumb_r,
                            radiusY: thumb_r,
                        };
                        self.brush_border.SetOpacity(0.60 * compact_alpha);
                        self.d2d_context.DrawEllipse(&thumb_circle, &self.brush_border, 1.2, None);
                    }

                    match config.compact_mode_view {
                        crate::config::CompactDisplayMode::Combined => {
                            let clip_left = left + 30.0;
                            let clip_right = if config.enable_media_island { right - 46.0 } else { right - 18.0 };
                            if clip_right > clip_left {
                                self.draw_compact_lyric_or_title(island, config, clip_left, clip_right, top, bottom, compact_alpha);
                            }

                            if config.enable_media_island {
                                self.draw_audio_visualizer(
                                    right - 25.0,
                                    center_y,
                                    island.audio_peak,
                                    island.audio_phase,
                                    11,
                                    14.0,
                                    0.95 * compact_alpha,
                                );
                            }
                        }
                        crate::config::CompactDisplayMode::TitleOnly => {
                            let clip_left = left + 30.0;
                            let clip_right = right - 28.0;
                            if clip_right > clip_left {
                                self.draw_compact_lyric_or_title(island, config, clip_left, clip_right, top, bottom, compact_alpha);
                            }

                            // Mini pause button on right
                            let btn_cx = right - 16.0;
                            let btn_circle = D2D1_ELLIPSE {
                                point: windows_numerics::Vector2 { X: btn_cx, Y: center_y },
                                radiusX: 8.0,
                                radiusY: 8.0,
                            };
                            let btn_bg_color = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.14 };
                            self.brush_btn_bg.SetColor(&btn_bg_color);
                            self.d2d_context.FillEllipse(&btn_circle, &self.brush_btn_bg);
                            self.d2d_context.DrawLine(pt(btn_cx - 2.0, center_y - 3.2), pt(btn_cx - 2.0, center_y + 3.2), &self.brush_white, 1.8, None);
                            self.d2d_context.DrawLine(pt(btn_cx + 2.0, center_y - 3.2), pt(btn_cx + 2.0, center_y + 3.2), &self.brush_white, 1.8, None);
                        }
                        crate::config::CompactDisplayMode::WaveOnly => {
                            let wave_cx = (left + 36.0 + right - 14.0) * 0.5;
                            self.draw_audio_visualizer(
                                wave_cx,
                                center_y,
                                island.audio_peak,
                                island.audio_phase,
                                17,
                                15.0,
                                0.95 * compact_alpha,
                            );
                        }
                    }
                } else {
                    // 4. Standard Idle: Glance pill with clock & English date, perfectly centered around cx
                    let cx = (left + right) * 0.5;
                    let center_y = (top + bottom) * 0.5;
                    let dot_cx = cx - 56.0;
                    let dot_cy = center_y;
                    let dot_radius = 3.5;

                    let ellipse = D2D1_ELLIPSE {
                        point: windows_numerics::Vector2 { X: dot_cx, Y: dot_cy },
                        radiusX: dot_radius,
                        radiusY: dot_radius,
                    };
                    self.brush_idle_dot.SetOpacity(0.90 * compact_alpha);
                    self.d2d_context.FillEllipse(&ellipse, &self.brush_idle_dot);

                    let st = windows::Win32::System::SystemInformation::GetLocalTime();
                    let time_str = format!("{:02}:{:02}", st.wHour, st.wMinute);
                    let time_utf16: Vec<u16> = time_str.encode_utf16().collect();

                    let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
                    let day_idx = (st.wDayOfWeek as usize).min(6);
                    let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
                    let month_idx = (st.wMonth.saturating_sub(1) as usize).min(11);
                    let date_str = format!("{} {} {}", day_names[day_idx], st.wDay, month_names[month_idx]);
                    let date_utf16: Vec<u16> = date_str.encode_utf16().collect();

                    self.brush_white.SetOpacity(compact_alpha);
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                    let time_rect = D2D_RECT_F {
                        left: cx - 46.0,
                        top,
                        right: cx - 10.0,
                        bottom,
                    };
                    self.d2d_context.DrawText(
                        &time_utf16,
                        &self.format_clock,
                        &time_rect as *const _,
                        &self.brush_white,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );

                    self.brush_subtext.SetOpacity(compact_alpha);
                    self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.82, g: 0.85, b: 0.92, a: 1.0 });
                    let date_rect = D2D_RECT_F {
                        left: cx - 6.0,
                        top,
                        right: cx + 64.0,
                        bottom,
                    };
                    self.d2d_context.DrawText(
                        &date_utf16,
                        &self.format_date,
                        &date_rect as *const _,
                        &self.brush_subtext,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }

            self.d2d_context.EndDraw(None, None)?;
            self.swapchain.Present(1, DXGI_PRESENT(0)).ok()?;
        }

        Ok(())
    }
}
