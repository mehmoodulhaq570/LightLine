use lightline::icon_theme::IconTheme;
use resvg::{tiny_skia, usvg};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const LIGHTLINE_ICON: &[u8] = include_bytes!("../../assets/lightline.ico");

pub(super) struct AppIcons {
    pub(super) small: HICON,
    pub(super) large: HICON,
    // The welcome screen draws the mark far larger than the title bar does;
    // upscaling the 32px icon there is visibly soft, so keep a 64px copy.
    pub(super) hero: HICON,
}

impl AppIcons {
    pub(super) fn new() -> Option<Self> {
        let small = IconSet::load_ico(LIGHTLINE_ICON, 16);
        let large = IconSet::load_ico(LIGHTLINE_ICON, 32);
        let hero = IconSet::load_ico(LIGHTLINE_ICON, 64);
        if small.is_null() || large.is_null() || hero.is_null() {
            for icon in [small, large, hero] {
                if !icon.is_null() {
                    unsafe { DestroyIcon(icon) };
                }
            }
            return None;
        }
        Some(Self { small, large, hero })
    }
}

impl Drop for AppIcons {
    fn drop(&mut self) {
        unsafe {
            DestroyIcon(self.small);
            DestroyIcon(self.large);
            DestroyIcon(self.hero);
        }
    }
}

// Chrome UI (the welcome screen, the Extensions panel, the explorer's
// workspace-root row) needs a plain "file"/"folder"/"folder-open"/"src
// folder" glyph without resolving any particular filename. These used to be
// a second, independent bundled icon set; now they're resolved from the same
// installed Zed icon theme as everything else, via IconSet::draw_generic.
pub(super) enum GenericIcon {
    File,
    Folder,
    FolderOpen,
    FolderSrc,
}

pub(super) struct IconSet {
    // A real Zed icon-theme extension (e.g. Material Icon Theme), loaded
    // from %APPDATA%\LightLine\extensions\material-icon-theme. Installed
    // automatically on first run by extension_installer::
    // ensure_material_icon_theme(); `None` only when that install couldn't
    // happen (no git on PATH, offline on first launch, ...), in which case
    // every draw call below simply draws nothing rather than crashing.
    theme: Option<IconTheme>,
    size: i32,
    // SVGs are rasterized to HICON lazily, on first use, not all ~1000 of
    // them up front: most users will only ever trigger a few dozen distinct
    // icons in a session (whatever file types/folder names they actually
    // have open), so eagerly converting the rest would be pure waste.
    svg_cache: RefCell<HashMap<PathBuf, HICON>>,
}

impl IconSet {
    pub(super) fn new(dpi: u32, zoom: i32) -> Self {
        let size = super::scaled(18, dpi, zoom);
        let theme = lightline::workflow::extensions_dir()
            .map(|dir| dir.join("material-icon-theme"))
            .and_then(|dir| IconTheme::load(&dir));
        Self {
            theme,
            size,
            svg_cache: RefCell::new(HashMap::new()),
        }
    }

    // Draws the icon for `path` (or, for a folder, `is_dir`/`expanded`) at
    // (x, y). When `use_theme` is true and the installed icon theme has a
    // matching entry, that SVG is used; otherwise this draws the theme's own
    // generic file/folder icon, so toggling Material Icons off still shows
    // something rather than nothing.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_for_path(
        &self,
        hdc: HDC,
        path: &Path,
        is_dir: bool,
        expanded: bool,
        use_theme: bool,
        x: i32,
        y: i32,
        size: i32,
    ) -> bool {
        if use_theme
            && let Some(icon) = self.themed_icon(hdc, path, is_dir, expanded)
        {
            return unsafe { DrawIconEx(hdc, x, y, icon, size, size, 0, null_mut(), DI_NORMAL) != 0 };
        }
        let generic = if is_dir {
            if expanded { GenericIcon::FolderOpen } else { GenericIcon::Folder }
        } else {
            GenericIcon::File
        };
        self.draw_generic(hdc, generic, x, y, size)
    }

    // Draws a plain file/folder glyph that isn't resolved from any
    // particular name -- the welcome screen's "Open Folder" icon, the
    // explorer's workspace-root row, the Extensions panel's icon badges.
    pub(super) fn draw_generic(&self, hdc: HDC, kind: GenericIcon, x: i32, y: i32, size: i32) -> bool {
        let Some(theme) = self.theme.as_ref() else {
            return false;
        };
        let svg_path = match kind {
            GenericIcon::File => theme.generic_file_icon(),
            GenericIcon::Folder => theme.generic_folder_icon(false),
            GenericIcon::FolderOpen => theme.generic_folder_icon(true),
            GenericIcon::FolderSrc => theme.resolve_directory("src", false),
        };
        let Some(icon) = svg_path.and_then(|path| self.cached_icon(hdc, path)) else {
            return false;
        };
        unsafe { DrawIconEx(hdc, x, y, icon, size, size, 0, null_mut(), DI_NORMAL) != 0 }
    }

    fn themed_icon(&self, hdc: HDC, path: &Path, is_dir: bool, expanded: bool) -> Option<HICON> {
        let theme = self.theme.as_ref()?;
        let name = path.file_name()?.to_str()?;
        let svg_path = if is_dir {
            theme.resolve_directory(name, expanded)
        } else {
            theme.resolve_file(name)
        }?;
        self.cached_icon(hdc, svg_path)
    }

    fn cached_icon(&self, hdc: HDC, svg_path: PathBuf) -> Option<HICON> {
        if let Some(&icon) = self.svg_cache.borrow().get(&svg_path) {
            return Some(icon);
        }
        let bytes = std::fs::read(&svg_path).ok()?;
        let icon = Self::svg_to_hicon(hdc, &bytes, self.size)?;
        self.svg_cache.borrow_mut().insert(svg_path, icon);
        Some(icon)
    }

    // Rasterizes `svg_bytes` at `size`x`size` via resvg/tiny-skia, then wraps
    // the resulting premultiplied-alpha bitmap as a Win32 HICON through a
    // 32bpp DIB section — the standard technique for building an icon from
    // an in-memory image rather than a pre-baked .ico resource.
    fn svg_to_hicon(hdc: HDC, svg_bytes: &[u8], size: i32) -> Option<HICON> {
        let size = size.max(1) as u32;
        let tree = usvg::Tree::from_data(svg_bytes, &usvg::Options::default()).ok()?;
        let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
        let tree_size = tree.size();
        let scale = (size as f32 / tree_size.width().max(1.0))
            .min(size as f32 / tree_size.height().max(1.0));
        let transform = tiny_skia::Transform::from_scale(scale, scale);
        resvg::render(&tree, transform, &mut pixmap.as_mut());

        // tiny-skia's pixmap is premultiplied RGBA, top-to-bottom; Windows
        // wants premultiplied BGRA for a top-down 32bpp DIB.
        let mut bgra = vec![0u8; (size * size * 4) as usize];
        for (dst, src) in bgra.chunks_exact_mut(4).zip(pixmap.data().chunks_exact(4)) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }

        unsafe {
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = size as i32;
            bmi.bmiHeader.biHeight = -(size as i32);
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let mut bits_ptr: *mut core::ffi::c_void = null_mut();
            let color = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits_ptr, null_mut(), 0);
            if color.is_null() || bits_ptr.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits_ptr as *mut u8, bgra.len());
            let mask = CreateBitmap(size as i32, size as i32, 1, 1, null());
            let icon_info = ICONINFO {
                fIcon: 1,
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: mask,
                hbmColor: color,
            };
            let icon = CreateIconIndirect(&icon_info);
            DeleteObject(color);
            DeleteObject(mask);
            if icon.is_null() { None } else { Some(icon) }
        }
    }

    pub(super) fn load_ico(data: &[u8], size: i32) -> HICON {
        if data.len() < 6 || data[0..4] != [0, 0, 1, 0] {
            return null_mut();
        }
        let count = u16::from_le_bytes([data[4], data[5]]) as usize;
        let mut best: Option<(i32, usize, usize)> = None;
        for index in 0..count {
            let start = 6 + index * 16;
            if start + 16 > data.len() {
                break;
            }
            let width = if data[start] == 0 {
                256
            } else {
                data[start] as i32
            };
            let length =
                u32::from_le_bytes(data[start + 8..start + 12].try_into().unwrap()) as usize;
            let offset =
                u32::from_le_bytes(data[start + 12..start + 16].try_into().unwrap()) as usize;
            if offset
                .checked_add(length)
                .is_none_or(|end| end > data.len())
            {
                continue;
            }
            let score = (width - size).abs();
            if best.is_none_or(|(best_score, _, _)| score < best_score) {
                best = Some((score, offset, length));
            }
        }
        let Some((_, offset, length)) = best else {
            return null_mut();
        };
        unsafe {
            CreateIconFromResourceEx(
                data.as_ptr().add(offset),
                length as u32,
                1,
                0x0003_0000,
                size,
                size,
                0,
            )
        }
    }
}

impl Drop for IconSet {
    fn drop(&mut self) {
        for icon in self.svg_cache.borrow().values() {
            if !icon.is_null() {
                unsafe { DestroyIcon(*icon) };
            }
        }
    }
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    #[test]
    fn lightline_app_icon_loads_at_ui_sizes() {
        for size in [16, 24, 32, 48, 64] {
            let icon = IconSet::load_ico(LIGHTLINE_ICON, size);
            assert!(!icon.is_null(), "failed to load {size}px app icon");
            unsafe { DestroyIcon(icon) };
        }
        assert!(AppIcons::new().is_some());
    }

    // Proves the resvg/tiny-skia -> Win32 HICON pipeline actually works
    // against a real icon from the Zed Material Icon Theme extension, not a
    // hand-crafted SVG fixture. Skips when the extension isn't installed at
    // %APPDATA%\LightLine\extensions\material-icon-theme on this machine.
    #[test]
    fn real_theme_svg_rasterizes_to_a_windows_icon() {
        let Some(appdata) = std::env::var_os("APPDATA") else {
            return;
        };
        let svg_path = PathBuf::from(appdata)
            .join("LightLine")
            .join("extensions")
            .join("material-icon-theme")
            .join("icons")
            .join("rust.svg");
        let Ok(bytes) = std::fs::read(&svg_path) else {
            eprintln!("skipped: real extension not installed at {svg_path:?}");
            return;
        };
        let hdc = unsafe { GetDC(null_mut()) };
        let icon = IconSet::svg_to_hicon(hdc, &bytes, 18);
        unsafe { ReleaseDC(null_mut(), hdc) };
        let icon = icon.expect("rust.svg should rasterize to an icon");
        assert!(!icon.is_null());
        unsafe { DestroyIcon(icon) };
    }
}
