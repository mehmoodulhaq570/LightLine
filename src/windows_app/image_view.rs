//! Read-only raster image preview (PNG, JPEG, GIF, BMP, ICO) for tabs whose
//! content isn't UTF-8 text. Decoding uses GDI+ (already present on every
//! supported Windows version) once per open; the decoded bitmap is then a
//! plain HBITMAP, drawn through the same GDI paint path as everything else
//! in this app, so no GDI+ calls happen during regular repaints.

use super::*;
use windows_sys::Win32::Graphics::GdiPlus::*;

const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "bmp", "ico"];

pub(super) fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            IMAGE_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

const fn argb(r: u8, g: u8, b: u8) -> u32 {
    0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

// Matches self.theme.editor_bg, so transparent pixels composite into the pane
// background instead of GDI+'s default white.
const IMAGE_BACKGROUND: u32 = argb(12, 21, 35);

fn ensure_gdiplus() {
    static STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    STARTED.get_or_init(|| unsafe {
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        let mut token: usize = 0;
        // Never shut down: the token would need to outlive every ImageAsset,
        // and the process tearing down reclaims everything anyway.
        GdiplusStartup(&mut token, &input, null_mut());
    });
}

pub(super) struct ImageAsset {
    bitmap: HBITMAP,
    pub(super) width: i32,
    pub(super) height: i32,
}

impl Drop for ImageAsset {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.bitmap);
        }
    }
}

pub(super) fn load_image(path: &Path) -> Option<ImageAsset> {
    ensure_gdiplus();
    unsafe {
        let wide_path = wide(&path.to_string_lossy());
        let mut image: *mut GpImage = null_mut();
        if GdipLoadImageFromFile(wide_path.as_ptr(), &mut image) != Ok || image.is_null() {
            return None;
        }
        let mut width: u32 = 0;
        let mut height: u32 = 0;
        GdipGetImageWidth(image, &mut width);
        GdipGetImageHeight(image, &mut height);
        let mut bitmap: HBITMAP = null_mut();
        // GpBitmap is a GpImage subtype in GDI+'s object model; the flat C
        // API only distinguishes them by pointer type, so this cast is the
        // normal way to call bitmap-specific functions on a loaded image.
        let status = GdipCreateHBITMAPFromBitmap(image.cast(), &mut bitmap, IMAGE_BACKGROUND);
        GdipDisposeImage(image);
        if status != Ok || bitmap.is_null() || width == 0 || height == 0 {
            return None;
        }
        Some(ImageAsset {
            bitmap,
            width: width as i32,
            height: height as i32,
        })
    }
}

impl App {
    pub(in crate::windows_app) fn paint_image_pane(
        &self,
        hdc: HDC,
        image: &ImageAsset,
        bounds: RECT,
    ) {
        unsafe {
            Self::fill(hdc, bounds, self.theme.editor_bg);
            let pane_w = (bounds.right - bounds.left).max(1);
            let pane_h = (bounds.bottom - bounds.top).max(1);
            let fit = (pane_w as f64 / image.width as f64).min(pane_h as f64 / image.height as f64);
            let scale = fit.min(1.0);
            let draw_w = ((image.width as f64) * scale).round().max(1.0) as i32;
            let draw_h = ((image.height as f64) * scale).round().max(1.0) as i32;
            let x = bounds.left + (pane_w - draw_w) / 2;
            let y = bounds.top + (pane_h - draw_h) / 2;
            let mem_dc = CreateCompatibleDC(hdc);
            if mem_dc.is_null() {
                return;
            }
            let old = SelectObject(mem_dc, image.bitmap);
            if draw_w == image.width && draw_h == image.height {
                BitBlt(hdc, x, y, draw_w, draw_h, mem_dc, 0, 0, SRCCOPY);
            } else {
                SetStretchBltMode(hdc, HALFTONE);
                StretchBlt(
                    hdc,
                    x,
                    y,
                    draw_w,
                    draw_h,
                    mem_dc,
                    0,
                    0,
                    image.width,
                    image.height,
                    SRCCOPY,
                );
            }
            SelectObject(mem_dc, old);
            DeleteDC(mem_dc);
        }
    }
}
