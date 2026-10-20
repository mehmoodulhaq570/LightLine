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

// The pane background (in the active theme), so transparent pixels
// composite into it instead of GDI+'s default white.
fn image_background() -> u32 {
    let color = super::themed(super::rgb(12, 21, 35));
    argb(color as u8, (color >> 8) as u8, (color >> 16) as u8)
}

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

// A GDI color (0x00BBGGRR) as GDI+'s opaque ARGB.
fn argb_of(color: u32) -> u32 {
    argb(color as u8, (color >> 8) as u8, (color >> 16) as u8)
}

/// An anti-aliased radio button centered on (`cx`, `cy`): a disc of `body`
/// with a `ring`-colored outline `width` thick, and an inner dot of `dot`
/// (radius, color) when chosen. GDI's own ellipses have jagged edges at
/// this size; GDI+ is already loaded for images, and this runs only for
/// the few radios on screen.
#[allow(clippy::too_many_arguments)]
pub(super) fn smooth_radio(
    hdc: HDC,
    cx: f32,
    cy: f32,
    radius: f32,
    width: f32,
    ring: u32,
    body: u32,
    dot: Option<(f32, u32)>,
) {
    ensure_gdiplus();
    unsafe {
        let mut graphics = null_mut();
        if GdipCreateFromHDC(hdc, &mut graphics) != 0 {
            return;
        }
        GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
        GdipSetPixelOffsetMode(graphics, PixelOffsetModeHalf);
        let mut brush = null_mut();
        GdipCreateSolidFill(argb_of(body), &mut brush);
        GdipFillEllipse(
            graphics,
            brush as *mut GpBrush,
            cx - radius,
            cy - radius,
            radius * 2.0,
            radius * 2.0,
        );
        GdipDeleteBrush(brush as *mut GpBrush);
        // The pen is centered on the path: inset it so the ring stays
        // inside `radius`.
        let mut pen = null_mut();
        GdipCreatePen1(argb_of(ring), width, UnitPixel, &mut pen);
        let r = radius - width / 2.0;
        GdipDrawEllipse(graphics, pen, cx - r, cy - r, r * 2.0, r * 2.0);
        GdipDeletePen(pen);
        if let Some((dot_radius, color)) = dot {
            let mut brush = null_mut();
            GdipCreateSolidFill(argb_of(color), &mut brush);
            GdipFillEllipse(
                graphics,
                brush as *mut GpBrush,
                cx - dot_radius,
                cy - dot_radius,
                dot_radius * 2.0,
                dot_radius * 2.0,
            );
            GdipDeleteBrush(brush as *mut GpBrush);
        }
        GdipDeleteGraphics(graphics);
    }
}

pub(super) struct ImageAsset {
    bitmap: HBITMAP,
    pub(super) width: i32,
    pub(super) height: i32,
    // Bitmap pixels per 96-dpi pixel: 1 for decoded files, the display scale
    // for SVGs, which are rasterized at screen resolution to stay sharp.
    pub(super) density: f32,
}

impl ImageAsset {
    /// Draws the whole image scaled into `rect`.
    pub(super) fn draw(&self, hdc: HDC, rect: RECT) {
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        unsafe {
            let mem_dc = CreateCompatibleDC(hdc);
            if mem_dc.is_null() {
                return;
            }
            let old = SelectObject(mem_dc, self.bitmap);
            SetStretchBltMode(hdc, HALFTONE);
            StretchBlt(
                hdc,
                rect.left,
                rect.top,
                width,
                height,
                mem_dc,
                0,
                0,
                self.width,
                self.height,
                SRCCOPY,
            );
            SelectObject(mem_dc, old);
            DeleteDC(mem_dc);
        }
    }
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
        let status = GdipCreateHBITMAPFromBitmap(image.cast(), &mut bitmap, image_background());
        GdipDisposeImage(image);
        if status != Ok || bitmap.is_null() || width == 0 || height == 0 {
            return None;
        }
        Some(ImageAsset {
            bitmap,
            width: width as i32,
            height: height as i32,
            density: 1.0,
        })
    }
}

/// Rasterizes an SVG at `density` bitmap pixels per 96-dpi pixel, over the
/// same background color PNG transparency is composited onto. resvg only
/// draws shapes: SVG scripts and links are never run or followed.
pub(super) fn load_svg(bytes: &[u8], density: f32) -> Option<ImageAsset> {
    use resvg::{tiny_skia, usvg};
    let options = usvg::Options {
        fontdb: svg_fonts(),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(bytes, &options).ok()?;
    let size = tree.size();
    let width = (size.width() * density).ceil().clamp(1.0, 4096.0) as u32;
    let height = (size.height() * density).ceil().clamp(1.0, 4096.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    let transform = tiny_skia::Transform::from_scale(
        width as f32 / size.width().max(1.0),
        height as f32 / size.height().max(1.0),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    // Premultiplied RGBA over the background, as the BGRX a DIB expects.
    let backdrop = image_background();
    let background = [
        (backdrop & 0xff) as u16,
        ((backdrop >> 8) & 0xff) as u16,
        ((backdrop >> 16) & 0xff) as u16,
    ];
    let mut bgrx = vec![0u8; (width * height * 4) as usize];
    for (dst, src) in bgrx
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(pixmap.data().as_chunks::<4>().0)
    {
        let uncovered = 255 - u16::from(src[3]);
        dst[0] = (u16::from(src[2]) + background[0] * uncovered / 255).min(255) as u8;
        dst[1] = (u16::from(src[1]) + background[1] * uncovered / 255).min(255) as u8;
        dst[2] = (u16::from(src[0]) + background[2] * uncovered / 255).min(255) as u8;
    }
    unsafe {
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width as i32;
        info.bmiHeader.biHeight = -(height as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut core::ffi::c_void = null_mut();
        let bitmap = CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            return None;
        }
        std::ptr::copy_nonoverlapping(bgrx.as_ptr(), bits as *mut u8, bgrx.len());
        Some(ImageAsset {
            bitmap,
            width: width as i32,
            height: height as i32,
            density,
        })
    }
}

// The fonts SVG text (a badge's label) is drawn with: a few standard Windows
// fonts, loaded once. Without any, resvg silently drops all text; scanning
// every installed font instead would stall the first SVG for a long time.
fn svg_fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: std::sync::OnceLock<Arc<resvg::usvg::fontdb::Database>> =
        std::sync::OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut fonts = resvg::usvg::fontdb::Database::new();
            let folder = std::env::var_os("WINDIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
                .join("Fonts");
            for file in [
                "verdana.ttf",
                "verdanab.ttf",
                "arial.ttf",
                "arialbd.ttf",
                "segoeui.ttf",
                "segoeuib.ttf",
                "tahoma.ttf",
                "times.ttf",
                "consola.ttf",
            ] {
                let _ = fonts.load_font_file(folder.join(file));
            }
            fonts.set_sans_serif_family("Arial");
            fonts.set_serif_family("Times New Roman");
            fonts.set_monospace_family("Consolas");
            Arc::new(fonts)
        })
        .clone()
}

/// Loads a picture file of either kind: SVG by content (badge services
/// serve SVG without an extension), anything else through GDI+.
pub(super) fn load_picture(path: &Path, density: f32) -> Option<ImageAsset> {
    let head = {
        use std::io::Read;
        let mut head = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(1024)
            .read_to_end(&mut head)
            .ok()?;
        head
    };
    if lightline::image_cache::is_svg(&head) {
        load_svg(&std::fs::read(path).ok()?, density)
    } else {
        load_image(path)
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
