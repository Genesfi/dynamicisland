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
    dwrite_factory: IDWriteFactory,

    format_title: IDWriteTextFormat,
    format_body: IDWriteTextFormat,
    format_time: IDWriteTextFormat,
    format_badge: IDWriteTextFormat,
    format_clock: IDWriteTextFormat,
    format_date: IDWriteTextFormat,

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
    pub wic_factory: IWICImagingFactory,
    #[allow(dead_code)]
    pub icon_manager: crate::icons::IconManager,
    #[allow(dead_code)]
    pub cached_is_light: bool,
    #[allow(dead_code)]
    pub lum_frame_counter: u32,
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
                dwrite_factory,
                format_title,
                format_body,
                format_time,
                format_badge,
                format_clock,
                format_date,
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
                icon_manager: crate::icons::IconManager::new(wic_factory.clone()),
                wic_factory,
                cached_is_light: false,
                lum_frame_counter: 0,
            })
        }
    }

    pub fn clear_album_art(&mut self) {
        self.album_art = None;
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

            let mut width: u32 = 0;
            let mut height: u32 = 0;
            converter.GetSize(&mut width, &mut height).ok()?;
            if width == 0 || height == 0 {
                return None;
            }

            let stride = width * 4;
            let total_bytes = (stride * height) as usize;
            let mut pixels = vec![0u8; total_bytes.min(256 * 256 * 4)];
            let hr = converter.CopyPixels(
                std::ptr::null(),
                stride,
                &mut pixels,
            );
            if hr.is_err() {
                return None;
            }

            let step_x = (width / 32).max(1);
            let step_y = (height / 32).max(1);
            let mut sum_r = 0.0f32;
            let mut sum_g = 0.0f32;
            let mut sum_b = 0.0f32;
            let mut count = 0;

            let mut y = 0;
            while y < height {
                let mut x = 0;
                while x < width {
                    let idx = ((y * width + x) * 4) as usize;
                    if idx + 3 < pixels.len() {
                        let b = pixels[idx] as f32 / 255.0;
                        let g = pixels[idx + 1] as f32 / 255.0;
                        let r = pixels[idx + 2] as f32 / 255.0;
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
                    x += step_x;
                }
                y += step_y;
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

    /// Renders the rich Full Media Control Card matching Nusa style with seek bar, master volume, localized date & time.
    fn draw_full_media_card(
        &self,
        island: &Island,
        config: &crate::config::AppConfig,
        track: &crate::island::TrackInfo,
        left: f32,
        top: f32,
        right: f32,
        _bottom: f32,
        alpha: f32,
    ) {
        unsafe {
            // 1. Album Artwork Square on Top-Left (46x46 px, corner radius 8px)
            let art_size = 46.0;
            let art_cx = left + 16.0 + art_size * 0.5;
            let art_cy = top + 16.0 + art_size * 0.5;
            let art_radius = art_size * 0.5;

            if let Some(art) = &self.album_art {
                self.draw_album_art_badge(art, art_cx, art_cy, art_radius, alpha);
            } else {
                let rot = if config.enable_vinyl_animation { island.vinyl_rotation } else { 0.0 };
                self.draw_spinning_vinyl(art_cx, art_cy, art_radius, rot, alpha);
            }

            // 2. Top Header Line (Green dot + Localized Date/Time + Mini Visualizer + Collapse Button)
            let content_left = left + 16.0 + art_size + 12.0;

            // Green glowing status dot
            let dot_x = content_left + 4.0;
            let dot_y = top + 20.0;
            let dot_radius = 3.2;
            let dot_ellipse = D2D1_ELLIPSE {
                point: windows_numerics::Vector2 { X: dot_x, Y: dot_y },
                radiusX: dot_radius,
                radiusY: dot_radius,
            };
            self.brush_idle_dot.SetOpacity(0.95 * alpha);
            self.d2d_context.FillEllipse(&dot_ellipse, &self.brush_idle_dot);

            // Date & Time string formatted like Nusa: e.g. "Sab 14 Nov 2026 17.25"
            let st = windows::Win32::System::SystemInformation::GetLocalTime();
            let day_names = ["Min", "Sen", "Sel", "Rab", "Kam", "Jum", "Sab"];
            let day_idx = (st.wDayOfWeek as usize).min(6);
            let month_names = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
            let month_idx = (st.wMonth.saturating_sub(1) as usize).min(11);
            let date_time_str = format!("{} {:02} {} {:04} {:02}.{:02}", day_names[day_idx], st.wDay, month_names[month_idx], st.wYear, st.wHour, st.wMinute);
            let dt_utf16: Vec<u16> = date_time_str.encode_utf16().collect();

            self.brush_subtext.SetOpacity(0.88 * alpha);
            let dt_rect = D2D_RECT_F {
                left: dot_x + 8.0,
                top: top + 12.0,
                right: right - 56.0,
                bottom: top + 28.0,
            };
            self.d2d_context.DrawText(
                &dt_utf16,
                &self.format_date,
                &dt_rect as *const _,
                &self.brush_subtext,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // Mini audio visualizer on top right
            if config.enable_media_island {
                self.draw_audio_visualizer(
                    right - 44.0,
                    top + 20.0,
                    island.audio_peak,
                    island.audio_phase,
                    7,
                    12.0,
                    alpha * 0.90,
                );
            }

            // Top-right collapse chevron button: "^"
            let chevron_x = right - 22.0;
            let chevron_y = top + 20.0;
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

            // 3. Track Title & Artist
            self.brush_white.SetOpacity(alpha);
            let title_rect = D2D_RECT_F {
                left: content_left,
                top: top + 31.0,
                right: right - 18.0,
                bottom: top + 48.0,
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
                left: content_left,
                top: top + 48.0,
                right: right - 18.0,
                bottom: top + 64.0,
            };
            let artist_str = if track.artist.is_empty() { "Now Playing".to_string() } else { track.artist.clone() };
            let artist_utf16: Vec<u16> = artist_str.encode_utf16().collect();
            self.brush_subtext.SetOpacity(0.80 * alpha);
            self.d2d_context.DrawText(
                &artist_utf16,
                &self.format_body,
                &artist_rect as *const _,
                &self.brush_subtext,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 4. Playback Seek Bar & Time Labels
            let seek_y = top + 76.0;

            // Left Elapsed Time
            let pos_secs = track.position_secs.max(0.0);
            let pos_mins = (pos_secs / 60.0).floor() as u32;
            let pos_rem = (pos_secs % 60.0).floor() as u32;
            let pos_str = format!("{:02}:{:02}", pos_mins, pos_rem);
            let pos_utf16: Vec<u16> = pos_str.encode_utf16().collect();

            let pos_rect = D2D_RECT_F {
                left: left + 14.0,
                top: seek_y - 8.0,
                right: left + 54.0,
                bottom: seek_y + 8.0,
            };
            self.brush_time.SetOpacity(0.85 * alpha);
            self.d2d_context.DrawText(
                &pos_utf16,
                &self.format_time,
                &pos_rect as *const _,
                &self.brush_time,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // Center Track Line
            let seek_left = left + 58.0;
            let seek_right = right - 58.0;
            let seek_w = (seek_right - seek_left).max(10.0);

            let ratio = if track.duration_secs > 0.0 {
                (pos_secs / track.duration_secs).clamp(0.0, 1.0)
            } else {
                0.0
            };

            // Background gray track
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.16 * alpha });
            self.d2d_context.DrawLine(
                pt(seek_left, seek_y),
                pt(seek_right, seek_y),
                &self.brush_btn_bg,
                3.6,
                None,
            );

            // Active played progress
            let played_x = seek_left + ratio * seek_w;
            self.brush_white.SetOpacity(0.95 * alpha);
            if ratio > 0.005 {
                self.d2d_context.DrawLine(
                    pt(seek_left, seek_y),
                    pt(played_x, seek_y),
                    &self.brush_white,
                    3.6,
                    None,
                );
            }

            // Draggable Knob / Thumb
            let thumb = D2D1_ELLIPSE {
                point: windows_numerics::Vector2 { X: played_x, Y: seek_y },
                radiusX: 4.5,
                radiusY: 4.5,
            };
            self.d2d_context.FillEllipse(&thumb, &self.brush_white);

            // Right Remaining Time
            let rem_str = if track.duration_secs > 0.0 {
                let rem_secs = (track.duration_secs - pos_secs).max(0.0);
                let rem_mins = (rem_secs / 60.0).floor() as u32;
                let rem_s = (rem_secs % 60.0).floor() as u32;
                format!("-{:02}:{:02}", rem_mins, rem_s)
            } else {
                "--:--".to_string()
            };
            let rem_utf16: Vec<u16> = rem_str.encode_utf16().collect();
            let rem_rect = D2D_RECT_F {
                left: right - 54.0,
                top: seek_y - 8.0,
                right: right - 14.0,
                bottom: seek_y + 8.0,
            };
            self.d2d_context.DrawText(
                &rem_utf16,
                &self.format_date,
                &rem_rect as *const _,
                &self.brush_time,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // 5. Bottom Controls (Left info bubble, Center media transport, Right volume)
            let ctrl_y = top + 106.0;

            // Info / lyrics bubble icon on left [💬]
            let bubble_cx = left + 28.0;
            let bubble_rect = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: bubble_cx - 9.0,
                    top: ctrl_y - 7.0,
                    right: bubble_cx + 9.0,
                    bottom: ctrl_y + 6.0,
                },
                radiusX: 3.5,
                radiusY: 3.5,
            };
            self.brush_white.SetOpacity(0.65 * alpha);
            self.d2d_context.DrawRoundedRectangle(&bubble_rect, &self.brush_white, 1.3, None);
            self.d2d_context.DrawLine(pt(bubble_cx - 4.0, ctrl_y + 6.0), pt(bubble_cx - 7.0, ctrl_y + 9.0), &self.brush_white, 1.3, None);
            self.d2d_context.DrawLine(pt(bubble_cx - 7.0, ctrl_y + 9.0), pt(bubble_cx - 2.0, ctrl_y + 6.0), &self.brush_white, 1.3, None);
            let bdot1 = D2D1_ELLIPSE { point: windows_numerics::Vector2 { X: bubble_cx - 3.5, Y: ctrl_y - 0.5 }, radiusX: 1.0, radiusY: 1.0 };
            let bdot2 = D2D1_ELLIPSE { point: windows_numerics::Vector2 { X: bubble_cx + 3.5, Y: ctrl_y - 0.5 }, radiusX: 1.0, radiusY: 1.0 };
            self.d2d_context.FillEllipse(&bdot1, &self.brush_white);
            self.d2d_context.FillEllipse(&bdot2, &self.brush_white);

            // Center Media Transport: Previous, Play/Pause, Next
            let cx = (left + right) * 0.5;

            // Previous: cx - 38.0
            let prev_cx = cx - 38.0;
            self.brush_white.SetOpacity(0.85 * alpha);
            self.d2d_context.DrawLine(pt(prev_cx + 4.0, ctrl_y - 5.5), pt(prev_cx - 3.5, ctrl_y), &self.brush_white, 1.7, None);
            self.d2d_context.DrawLine(pt(prev_cx + 4.0, ctrl_y + 5.5), pt(prev_cx - 3.5, ctrl_y), &self.brush_white, 1.7, None);
            self.d2d_context.DrawLine(pt(prev_cx + 4.0, ctrl_y - 5.5), pt(prev_cx + 4.0, ctrl_y + 5.5), &self.brush_white, 1.7, None);
            self.d2d_context.DrawLine(pt(prev_cx - 5.0, ctrl_y - 5.0), pt(prev_cx - 5.0, ctrl_y + 5.0), &self.brush_white, 1.7, None);

            // Play / Pause in Center: cx
            let play_bg = D2D1_ELLIPSE {
                point: windows_numerics::Vector2 { X: cx, Y: ctrl_y },
                radiusX: 16.0,
                radiusY: 16.0,
            };
            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.15 * alpha });
            self.d2d_context.FillEllipse(&play_bg, &self.brush_btn_bg);

            self.brush_white.SetOpacity(0.95 * alpha);
            if island.is_media_playing {
                // Pause: two bars
                self.d2d_context.DrawLine(pt(cx - 3.2, ctrl_y - 5.5), pt(cx - 3.2, ctrl_y + 5.5), &self.brush_white, 2.2, None);
                self.d2d_context.DrawLine(pt(cx + 3.2, ctrl_y - 5.5), pt(cx + 3.2, ctrl_y + 5.5), &self.brush_white, 2.2, None);
            } else {
                // Play: triangle
                self.d2d_context.DrawLine(pt(cx - 3.5, ctrl_y - 6.0), pt(cx + 5.5, ctrl_y), &self.brush_white, 1.8, None);
                self.d2d_context.DrawLine(pt(cx - 3.5, ctrl_y + 6.0), pt(cx + 5.5, ctrl_y), &self.brush_white, 1.8, None);
                self.d2d_context.DrawLine(pt(cx - 3.5, ctrl_y - 6.0), pt(cx - 3.5, ctrl_y + 6.0), &self.brush_white, 1.8, None);
            }

            // Next: cx + 38.0
            let next_cx = cx + 38.0;
            self.brush_white.SetOpacity(0.85 * alpha);
            self.d2d_context.DrawLine(pt(next_cx - 4.0, ctrl_y - 5.5), pt(next_cx + 3.5, ctrl_y), &self.brush_white, 1.7, None);
            self.d2d_context.DrawLine(pt(next_cx - 4.0, ctrl_y + 5.5), pt(next_cx + 3.5, ctrl_y), &self.brush_white, 1.7, None);
            self.d2d_context.DrawLine(pt(next_cx - 4.0, ctrl_y - 5.5), pt(next_cx - 4.0, ctrl_y + 5.5), &self.brush_white, 1.7, None);
            self.d2d_context.DrawLine(pt(next_cx + 5.0, ctrl_y - 5.0), pt(next_cx + 5.0, ctrl_y + 5.0), &self.brush_white, 1.7, None);

            // Right Volume Control: Speaker icon + Volume slider
            let spk_x = right - 86.0;
            self.brush_white.SetOpacity(0.75 * alpha);
            // Speaker horn
            self.d2d_context.DrawLine(pt(spk_x - 4.0, ctrl_y - 2.5), pt(spk_x - 1.5, ctrl_y - 2.5), &self.brush_white, 1.3, None);
            self.d2d_context.DrawLine(pt(spk_x - 1.5, ctrl_y - 2.5), pt(spk_x + 2.0, ctrl_y - 5.5), &self.brush_white, 1.3, None);
            self.d2d_context.DrawLine(pt(spk_x + 2.0, ctrl_y - 5.5), pt(spk_x + 2.0, ctrl_y + 5.5), &self.brush_white, 1.3, None);
            self.d2d_context.DrawLine(pt(spk_x + 2.0, ctrl_y + 5.5), pt(spk_x - 1.5, ctrl_y + 2.5), &self.brush_white, 1.3, None);
            self.d2d_context.DrawLine(pt(spk_x - 1.5, ctrl_y + 2.5), pt(spk_x - 4.0, ctrl_y + 2.5), &self.brush_white, 1.3, None);
            self.d2d_context.DrawLine(pt(spk_x - 4.0, ctrl_y - 2.5), pt(spk_x - 4.0, ctrl_y + 2.5), &self.brush_white, 1.3, None);
            // Speaker wave arc
            self.d2d_context.DrawLine(pt(spk_x + 4.0, ctrl_y - 3.0), pt(spk_x + 5.0, ctrl_y), &self.brush_white, 1.3, None);
            self.d2d_context.DrawLine(pt(spk_x + 5.0, ctrl_y), pt(spk_x + 4.0, ctrl_y + 3.0), &self.brush_white, 1.3, None);

            // Volume Slider track
            let vol_left = right - 76.0;
            let vol_right = right - 18.0;
            let vol_w = vol_right - vol_left;
            let vol_val = island.volume.clamp(0.0, 1.0);

            self.brush_btn_bg.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.16 * alpha });
            self.d2d_context.DrawLine(pt(vol_left, ctrl_y), pt(vol_right, ctrl_y), &self.brush_btn_bg, 3.2, None);

            let vol_fill_x = vol_left + vol_val * vol_w;
            self.brush_white.SetOpacity(0.90 * alpha);
            if vol_val > 0.02 {
                self.d2d_context.DrawLine(pt(vol_left, ctrl_y), pt(vol_fill_x, ctrl_y), &self.brush_white, 3.2, None);
            }
            let vol_thumb = D2D1_ELLIPSE {
                point: windows_numerics::Vector2 { X: vol_fill_x, Y: ctrl_y },
                radiusX: 3.8,
                radiusY: 3.8,
            };
            self.d2d_context.FillEllipse(&vol_thumb, &self.brush_white);
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

            // 5. Render Content based on state
            let alpha = island.alpha_spring.current.clamp(0.0, 1.0);

            if island.mode == IslandMode::Idle || alpha < 0.15 {
                if island.is_media_playing {
                    let center_y = (top + bottom) * 0.5;

                    // 1. Circular Rotating Album Artwork Thumbnail on Left (Nest tightly in left cap "rata kiri")
                    let thumb_cx = left + 14.0;
                    let thumb_r = 10.0;

                    if let Some(art) = &self.album_art {
                        let rot = if config.enable_vinyl_animation { island.vinyl_rotation } else { 0.0 };
                        self.draw_rotating_album_circle(art, thumb_cx, center_y, thumb_r, rot, 0.95);

                        let thumb_circle = D2D1_ELLIPSE {
                            point: windows_numerics::Vector2 { X: thumb_cx, Y: center_y },
                            radiusX: thumb_r,
                            radiusY: thumb_r,
                        };
                        let rim_color = D2D1_COLOR_F { r: self.accent_color.0, g: self.accent_color.1, b: self.accent_color.2, a: 0.70 };
                        self.brush_border.SetColor(&rim_color);
                        self.d2d_context.DrawEllipse(&thumb_circle, &self.brush_border, 1.2, None);
                    } else {
                        let note_rect = D2D_RECT_F {
                            left: thumb_cx - 8.0,
                            top: center_y - 8.0,
                            right: thumb_cx + 8.0,
                            bottom: center_y + 8.0,
                        };
                        let utf16: Vec<u16> = "♫".encode_utf16().collect();
                        self.brush_accent.SetColor(&D2D1_COLOR_F {
                            r: self.accent_color.0,
                            g: self.accent_color.1,
                            b: self.accent_color.2,
                            a: 0.95,
                        });
                        self.d2d_context.DrawText(
                            &utf16,
                            &self.format_badge,
                            &note_rect as *const _,
                            &self.brush_accent,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );

                        let thumb_circle = D2D1_ELLIPSE {
                            point: windows_numerics::Vector2 { X: thumb_cx, Y: center_y },
                            radiusX: thumb_r,
                            radiusY: thumb_r,
                        };
                        self.brush_border.SetOpacity(0.60);
                        self.d2d_context.DrawEllipse(&thumb_circle, &self.brush_border, 1.2, None);
                    }

                    // 2. Compact View Modes: Combined (Title + Wave), TitleOnly, or WaveOnly
                    match config.compact_mode_view {
                        crate::config::CompactDisplayMode::Combined => {
                            let clip_left = left + 30.0;
                            let clip_right = if config.enable_media_island { right - 66.0 } else { right - 22.0 };
                            if clip_right > clip_left {
                                let clip_rect = D2D_RECT_F { left: clip_left, top, right: clip_right, bottom };
                                self.d2d_context.PushAxisAlignedClip(&clip_rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

                                let title_text = if let Some(track) = &island.current_track {
                                    format!("{} • {}", track.title, track.artist)
                                } else {
                                    "Music Playing".to_string()
                                };
                                let utf16: Vec<u16> = title_text.encode_utf16().collect();
                                let available_w = clip_right - clip_left;

                                // Measure exact text width using DirectWrite
                                let text_width = if let Ok(layout) = self.dwrite_factory.CreateTextLayout(&utf16, &self.format_body, 2000.0, 30.0) {
                                    let mut metrics = DWRITE_TEXT_METRICS::default();
                                    if layout.GetMetrics(&mut metrics).is_ok() {
                                        metrics.width
                                    } else {
                                        (utf16.len() as f32) * 6.8
                                    }
                                } else {
                                    (utf16.len() as f32) * 6.8
                                };

                                self.brush_white.SetOpacity(0.98);
                                if text_width <= available_w {
                                    // Text fits cleanly: stay static, no sliding!
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
                                    // Text exceeds capsule: marquee scroll
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
                                self.d2d_context.PopAxisAlignedClip();
                            }

                            if config.enable_media_island {
                                self.draw_audio_visualizer(
                                    right - 25.0,
                                    center_y,
                                    island.audio_peak,
                                    island.audio_phase,
                                    11,
                                    14.0,
                                    0.95,
                                );
                            }
                        }
                        crate::config::CompactDisplayMode::TitleOnly => {
                            let clip_left = left + 30.0;
                            let clip_right = right - 22.0;
                            if clip_right > clip_left {
                                let clip_rect = D2D_RECT_F { left: clip_left, top, right: clip_right, bottom };
                                self.d2d_context.PushAxisAlignedClip(&clip_rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

                                let title_text = if let Some(track) = &island.current_track {
                                    format!("{} • {}", track.title, track.artist)
                                } else {
                                    "Music Playing".to_string()
                                };
                                let utf16: Vec<u16> = title_text.encode_utf16().collect();
                                let available_w = clip_right - clip_left;

                                let text_width = if let Ok(layout) = self.dwrite_factory.CreateTextLayout(&utf16, &self.format_body, 2000.0, 30.0) {
                                    let mut metrics = DWRITE_TEXT_METRICS::default();
                                    if layout.GetMetrics(&mut metrics).is_ok() {
                                        metrics.width
                                    } else {
                                        (utf16.len() as f32) * 6.8
                                    }
                                } else {
                                    (utf16.len() as f32) * 6.8
                                };

                                self.brush_white.SetOpacity(0.98);
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
                                self.d2d_context.PopAxisAlignedClip();
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
                                0.95,
                            );
                        }
                    }
                } else {
                    // Standard idle: render active glance pill with clock & date, plus subtle glowing status dot
                    let center_y = (top + bottom) * 0.5;
                    let dot_cx = left + 14.0;
                    let dot_cy = center_y;
                    let dot_radius = 3.5;

                    let ellipse = D2D1_ELLIPSE {
                        point: windows_numerics::Vector2 { X: dot_cx, Y: dot_cy },
                        radiusX: dot_radius,
                        radiusY: dot_radius,
                    };
                    self.brush_idle_dot.SetOpacity(0.90);
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

                    self.brush_white.SetOpacity(1.0);
                    self.brush_white.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
                    let time_rect = D2D_RECT_F {
                        left: left + 24.0,
                        top,
                        right: left + 62.0,
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

                    self.brush_subtext.SetOpacity(1.0);
                    self.brush_subtext.SetColor(&D2D1_COLOR_F { r: 0.82, g: 0.85, b: 0.92, a: 1.0 });
                    let date_rect = D2D_RECT_F {
                        left: left + 64.0,
                        top,
                        right: right - 10.0,
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

            if alpha > 0.05 {
                let cy = (top + bottom) * 0.5;
                let pill_h = bottom - top;

                if let Some(notif) = &island.active_notification {
                    // A. Live Notification Mode
                    self.brush_white.SetOpacity(alpha);
                    self.brush_subtext.SetOpacity(alpha);
                    self.brush_time.SetOpacity(alpha);

                    let is_media_notif = notif.app_name.to_lowercase().contains("music")
                        || notif.app_name.to_lowercase().contains("spotify")
                        || notif.app_name.to_lowercase().contains("youtube")
                        || island.is_media_playing;

                    if pill_h <= 36.0 {
                        // Stage 1: Ultra-slim horizontal single-line strip (Flush with top edge)
                        let badge_radius = 9.0;
                        let badge_cx = left + 18.0;

                        self.draw_notification_badge(notif, is_media_notif, badge_cx, cy, badge_radius, alpha);

                        // Single-line text in middle
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

                        // Time label on right
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
                    } else {
                        // Stage 2: Full Rich Expanded Card (on Hover)
                        let badge_radius = (pill_h * 0.32).clamp(15.0, 19.0);
                        let badge_cx = left + badge_radius + 14.0;

                        self.draw_notification_badge(notif, is_media_notif, badge_cx, cy, badge_radius, alpha);

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
                } else if let Some(track) = &island.current_track {
                    if island.mode == IslandMode::FullCard || pill_h >= 90.0 {
                        self.draw_full_media_card(island, config, track, left, top, right, bottom, alpha);
                    } else {
                        // B. Expanded Music Player Mode (Hover or Playback)
                        self.brush_white.SetOpacity(alpha);
                        self.brush_subtext.SetOpacity(alpha);

                        let vinyl_radius = (pill_h * 0.34).clamp(16.0, 20.0);
                        let vinyl_cx = left + vinyl_radius + 14.0;

                        // 1. Draw Spinning Vinyl Record ("Vynl mutar")
                        let rot = if config.enable_vinyl_animation { island.vinyl_rotation } else { 0.0 };
                        self.draw_spinning_vinyl(
                            vinyl_cx,
                            cy,
                            vinyl_radius,
                            rot,
                            alpha,
                        );

                        // 2. Track Title & Artist Text
                        let text_left = vinyl_cx + vinyl_radius + 12.0;
                        let text_right = right - 144.0; // Space reserved for media controls + expand button

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

                        // Real-time audio visualizer waves in expanded mode
                        if config.enable_media_island {
                            self.draw_audio_visualizer(
                                text_right - 26.0,
                                top + (pill_h * 0.68),
                                island.audio_peak,
                                island.audio_phase,
                                11,
                                18.0,
                                alpha * 0.95,
                            );
                        }

                        // 3. Draw Vector Media Controls (⏮ ⏯ ⏭)
                        self.draw_media_controls(
                            right,
                            cy,
                            island.is_media_playing,
                            alpha,
                        );
                    }
                }
            }

            self.d2d_context.EndDraw(None, None)?;
            self.swapchain.Present(1, DXGI_PRESENT(0)).ok()?;
        }

        Ok(())
    }
}
