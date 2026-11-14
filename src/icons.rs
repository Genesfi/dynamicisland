#![allow(unsafe_op_in_unsafe_fn)]
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use windows::{
    core::*,
    Win32::{
        Graphics::{
            Direct2D::*,
            Gdi::*,
            Imaging::*,
        },
        UI::{
            Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON},
            WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO},
        },
    },
};

pub struct IconManager {
    wic_factory: IWICImagingFactory,
    cache: HashMap<String, ID2D1Bitmap>,
    shortcut_cache: HashMap<String, PathBuf>,
    shortcuts_scanned: bool,
}

impl IconManager {
    pub fn new(wic_factory: IWICImagingFactory) -> Self {
        Self {
            wic_factory,
            cache: HashMap::new(),
            shortcut_cache: HashMap::new(),
            shortcuts_scanned: false,
        }
    }

    /// Scans Windows Start Menu programs folders recursively to map installed applications.
    fn ensure_shortcuts_scanned(&mut self) {
        if self.shortcuts_scanned {
            return;
        }
        self.shortcuts_scanned = true;

        let mut search_dirs = Vec::new();
        if let Ok(appdata) = std::env::var("APPDATA") {
            search_dirs.push(PathBuf::from(format!(
                r"{}\Microsoft\Windows\Start Menu\Programs",
                appdata
            )));
        }
        if let Ok(progdata) = std::env::var("PROGRAMDATA") {
            search_dirs.push(PathBuf::from(format!(
                r"{}\Microsoft\Windows\Start Menu\Programs",
                progdata
            )));
        }

        for dir in search_dirs {
            Self::scan_dir_recursive(&dir, &mut self.shortcut_cache, 0);
        }
    }

    fn scan_dir_recursive(dir: &Path, map: &mut HashMap<String, PathBuf>, depth: u32) {
        if depth > 3 {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    Self::scan_dir_recursive(&path, map, depth + 1);
                } else if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    if ext.eq_ignore_ascii_case("lnk") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            let clean = stem.to_lowercase();
                            map.entry(clean).or_insert_with(|| path.clone());
                        }
                    }
                }
            }
        }
    }

    /// Finds the executable or shortcut path for a given application.
    fn find_app_path(&mut self, app_name: &str) -> Option<PathBuf> {
        let lower = app_name.to_lowercase();

        // 1. Known Standard Paths
        if lower.contains("edge") {
            let p1 = Path::new(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe");
            if p1.exists() {
                return Some(p1.to_path_buf());
            }
            let p2 = Path::new(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe");
            if p2.exists() {
                return Some(p2.to_path_buf());
            }
        } else if lower.contains("chrome") {
            let p1 = Path::new(r"C:\Program Files\Google\Chrome\Application\chrome.exe");
            if p1.exists() {
                return Some(p1.to_path_buf());
            }
            let p2 = Path::new(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe");
            if p2.exists() {
                return Some(p2.to_path_buf());
            }
        } else if lower.contains("firefox") {
            let p = Path::new(r"C:\Program Files\Mozilla Firefox\firefox.exe");
            if p.exists() {
                return Some(p.to_path_buf());
            }
        }

        // 2. Scan User & System Start Menu shortcuts
        self.ensure_shortcuts_scanned();

        if let Some(p) = self.shortcut_cache.get(&lower) {
            return Some(p.clone());
        }

        for (k, v) in &self.shortcut_cache {
            if k.contains(&lower) || lower.contains(k) {
                return Some(v.clone());
            }
        }

        // 3. Fallback to Windows Explorer
        let explorer = Path::new(r"C:\Windows\explorer.exe");
        if explorer.exists() {
            Some(explorer.to_path_buf())
        } else {
            None
        }
    }

    /// Extracts large icon directly from file path (.lnk, .exe, or shell item) via Win32 SHGetFileInfoW.
    pub fn get_icon_from_path(
        &self,
        d2d_context: &ID2D1DeviceContext,
        path: &Path,
    ) -> Option<ID2D1Bitmap> {
        let path_str = path.to_str()?;
        let mut wide_path: Vec<u16> = path_str.encode_utf16().collect();
        wide_path.push(0);

        let mut shfi = SHFILEINFOW::default();
        let cb_size = std::mem::size_of::<SHFILEINFOW>() as u32;
        let flags = SHGFI_ICON | SHGFI_LARGEICON;

        unsafe {
            let res = SHGetFileInfoW(
                PCWSTR(wide_path.as_ptr()),
                windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut shfi),
                cb_size,
                flags,
            );

            if res != 0 && !shfi.hIcon.is_invalid() {
                let wic_bitmap = self.wic_factory.CreateBitmapFromHICON(shfi.hIcon).ok();
                let _ = DestroyIcon(shfi.hIcon);
                if let Some(wic_bmp) = wic_bitmap {
                    if let Ok(d2d_bmp) = d2d_context.CreateBitmapFromWicBitmap(&wic_bmp, None) {
                        return Some(d2d_bmp.into());
                    }
                }
            }
        }
        None
    }

    /// Decodes raw PNG/ICO/JPEG bytes into a Direct2D bitmap.
    pub fn decode_image_bytes(
        &self,
        d2d_context: &ID2D1DeviceContext,
        bytes: &[u8],
    ) -> Option<ID2D1Bitmap> {
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
            let bmp1 = d2d_context.CreateBitmapFromWicBitmap(&converter, None).ok()?;
            Some(bmp1.into())
        }
    }

    /// Primary resolver: Retrieves real app icon using bytes, AUMID, or shortcut/executable search.
    pub fn get_icon(
        &mut self,
        d2d_context: &ID2D1DeviceContext,
        app_name: &str,
        aumid: Option<&str>,
        icon_bytes: Option<&[u8]>,
    ) -> Option<ID2D1Bitmap> {
        let key = app_name.to_lowercase();
        if let Some(bitmap) = self.cache.get(&key) {
            return Some(bitmap.clone());
        }

        // 1. Direct logo bytes from WinRT notification
        if let Some(bytes) = icon_bytes {
            if let Some(bitmap) = self.decode_image_bytes(d2d_context, bytes) {
                self.cache.insert(key.clone(), bitmap.clone());
                return Some(bitmap);
            }
        }

        // 2. AUMID Shell virtual folder lookup
        if let Some(id) = aumid {
            let folder_path = format!(r"shell:AppsFolder\{}", id);
            if let Some(bitmap) = self.get_icon_from_path(d2d_context, Path::new(&folder_path)) {
                self.cache.insert(key.clone(), bitmap.clone());
                return Some(bitmap);
            }
        }

        // 3. Start Menu Shortcuts & Executables
        if let Some(app_path) = self.find_app_path(app_name) {
            if let Some(bitmap) = self.get_icon_from_path(d2d_context, &app_path) {
                self.cache.insert(key.clone(), bitmap.clone());
                return Some(bitmap);
            }
        }

        None
    }
}

/// Generates a modern 2-palette rounded squircle Dynamic Island HICON dynamically.
/// Palette 1: Deep Obsidian Dark Slate (#0F131C)
/// Palette 2: Crisp Pure White (#FFFFFF)
pub unsafe fn create_dynamic_island_icon(size: i32) -> Result<HICON> {
    let bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size, // Top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let hdc = CreateCompatibleDC(None);
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let hbm_color = CreateDIBSection(
        Some(hdc),
        &bi,
        DIB_RGB_COLORS,
        &mut bits,
        None,
        0,
    )?;

    if bits.is_null() {
        let _ = DeleteDC(hdc);
        return Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL));
    }

    let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);
    let cx = (size - 1) as f32 * 0.5;
    let cy = (size - 1) as f32 * 0.5;
    let sq_radius = (size as f32) * 0.46;
    let cr = (size as f32) * 0.26;
    let pw = (size as f32) * 0.62;
    let ph = (size as f32) * 0.28;
    let pr = ph * 0.5;

    for y in 0..size {
        for x in 0..size {
            let qx = (x as f32 - cx).abs() - (sq_radius - cr);
            let qy = (y as f32 - cy).abs() - (sq_radius - cr);
            let sq_dist = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - cr;

            let idx = (y * size + x) as usize;
            if sq_dist > 0.5 {
                pixels[idx] = 0x00000000; // 100% Transparent outside squircle
                continue;
            }

            let sq_alpha = (0.5 - sq_dist).clamp(0.0, 1.0);

            // Check if inside central dynamic island pill
            let px = (x as f32 - cx).abs() - (pw * 0.5 - pr);
            let py = (y as f32 - cy).abs() - (ph * 0.5 - pr);
            let pill_dist = px.max(0.0).hypot(py.max(0.0)) + px.max(py).min(0.0) - pr;

            let (r, g, b) = if pill_dist <= 0.0 {
                // Inside pill
                let pill_alpha = (0.5 - pill_dist).clamp(0.0, 1.0);
                // Check camera notch cutout on left
                let dot_cx = cx - pw * 0.26;
                let dot_r = ph * 0.25;
                let dot_dist = (x as f32 - dot_cx).hypot(y as f32 - cy) - dot_r;

                // Check audio bar cutouts on right
                let bar_x = cx + pw * 0.24;
                let in_bar = (x as f32 - bar_x).abs() <= (size as f32 * 0.05)
                    && (y as f32 - cy).abs() <= (ph * 0.35);

                if dot_dist <= 0.0 || in_bar {
                    // Cutout: Palette 1 (Obsidian)
                    (15.0, 19.0, 28.0)
                } else {
                    // Body: Palette 2 (Crisp Pure White)
                    let (br, bg, bb) = (15.0, 19.0, 28.0);
                    let (wr, wg, wb) = (255.0, 255.0, 255.0);
                    (
                        br + (wr - br) * pill_alpha,
                        bg + (wg - bg) * pill_alpha,
                        bb + (wb - bb) * pill_alpha,
                    )
                }
            } else {
                // Squircle background: Palette 1 (Deep Obsidian Slate)
                (15.0, 19.0, 28.0)
            };

            let a = (sq_alpha * 255.0) as u32;
            let pr = ((r / 255.0) * (a as f32)) as u32;
            let pg = ((g / 255.0) * (a as f32)) as u32;
            let pb = ((b / 255.0) * (a as f32)) as u32;

            pixels[idx] = (a << 24) | (pr << 16) | (pg << 8) | pb;
        }
    }

    let hbm_mask = CreateBitmap(size, size, 1, 1, None);

    let icon_info = ICONINFO {
        fIcon: true.into(),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: hbm_mask,
        hbmColor: hbm_color,
    };

    let hicon = CreateIconIndirect(&icon_info);

    let _ = DeleteObject(HGDIOBJ(hbm_color.0));
    let _ = DeleteObject(HGDIOBJ(hbm_mask.0));
    let _ = DeleteDC(hdc);

    hicon
}

