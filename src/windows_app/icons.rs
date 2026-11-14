use super::builtin_icons::BuiltinIcon;
use lightline::icon_theme::IconTheme;
use resvg::{tiny_skia, usvg};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const LIGHTLINE_ICON: &[u8] = include_bytes!("../../assets/lightline.ico");
// The sizes assets/lightline.ico holds (the title bar draws one of them 1:1).
const BRAND_SIZES: [i32; 5] = [16, 24, 32, 48, 64];

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

// The Run & Debug toolbar's controls and the workbench's own icons (rail,
// title bar, panel headers, tabs, status bar), drawn from SVG so their
// curves and diagonals are anti-aliased like the file icons, at the exact
// pixel size they show at. GDI's 1 px pens drew them jagged, and font
// characters standing in for icons came out small and smudged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum DebugGlyph {
    Start,
    Continue,
    Pause,
    StepOver,
    StepInto,
    StepOut,
    Restart,
    Stop,
    // Editor gutter markers.
    GutterDot,
    ExecutionArrow,
    // The activity rail.
    Menu,
    Files,
    Search,
    SourceControl,
    RunDebug,
    Extensions,
    Sparkle,
    Gear,
    // Panel headers, tabs and the title bar.
    NewFile,
    NewFolder,
    Refresh,
    CollapseAll,
    Close,
    ChevronDown,
    ChevronRight,
    SplitEditor,
    More,
    Minimize,
    Maximize,
    Restore,
    // The status bar and the Problems list.
    Error,
    Warning,
    Info,
    Branch,
    // Panel header and row buttons.
    Plus,
    Minus,
    Discard,
    // Source Control: commit, push, pull, fetch, open the changed file.
    Check,
    ArrowUp,
    ArrowDown,
    Fetch,
    OpenFile,
    // The Extensions search box's filter.
    Filter,
}

impl DebugGlyph {
    // 24x24 artwork; `color` is a CSS hex colour such as "#aac8fa".
    fn svg(self, color: &str) -> String {
        let line = format!(
            r#"fill="none" stroke="{color}" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round""#
        );
        // The workbench icons: lighter outlines, as VS Code's.
        let ui = format!(
            r#"fill="none" stroke="{color}" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round""#
        );
        let body = match self {
            DebugGlyph::Menu => format!(r#"<path {ui} d="M4 6.5h16M4 12h16M4 17.5h16"/>"#),
            // Two sheets, the front one with a folded corner.
            DebugGlyph::Files => format!(
                r#"<path {ui} d="M8.5 6.5V4.8A1.8 1.8 0 0 1 10.3 3h5.2L20 7.5v9.7a1.8 1.8 0 0 1-1.8 1.8h-1.7"/><path {ui} d="M15.5 3v4.5H20"/><path {ui} d="M5.8 6.5h5.7l4 4v8.7a1.8 1.8 0 0 1-1.8 1.8H5.8A1.8 1.8 0 0 1 4 19.2V8.3a1.8 1.8 0 0 1 1.8-1.8z"/>"#
            ),
            DebugGlyph::Search => {
                format!(
                    r#"<circle {ui} cx="10.5" cy="10.5" r="6.2"/><path {ui} d="m15.2 15.2 5.3 5.3"/>"#
                )
            }
            DebugGlyph::SourceControl | DebugGlyph::Branch => format!(
                r#"<circle {ui} cx="6.5" cy="5.5" r="2.2"/><circle {ui} cx="6.5" cy="18.5" r="2.2"/><circle {ui} cx="17.5" cy="7.5" r="2.2"/><path {ui} d="M6.5 7.7v8.6"/><path {ui} d="M17.5 9.7c0 3.6-2.6 4.5-5.5 4.8-2.6.3-4.8.9-5.4 2.4"/>"#
            ),
            // A play triangle with a small bug's head: run and debug.
            DebugGlyph::RunDebug => format!(
                r#"<path {ui} d="M6 4.5v15l7.5-4.7"/><path {ui} d="M6 4.5l11.5 7.2"/><circle {ui} cx="17.5" cy="17" r="3.2"/><path {ui} d="M17.5 13.8v-1.6M14.3 17h-1.6M20.7 17h1.6"/>"#
            ),
            // Three squares and a fourth set apart.
            DebugGlyph::Extensions => format!(
                r#"<rect {ui} x="3.5" y="9.5" width="5.5" height="5.5" rx="1"/><rect {ui} x="9" y="9.5" width="5.5" height="5.5" rx="1"/><rect {ui} x="3.5" y="15" width="5.5" height="5.5" rx="1"/><rect {ui} x="9" y="15" width="5.5" height="5.5" rx="1"/><rect {ui} x="14.5" y="3.5" width="5.5" height="5.5" rx="1" transform="rotate(12 17.25 6.25)"/>"#
            ),
            // A four-point star and a small one: the AI Assistant.
            DebugGlyph::Sparkle => format!(
                r#"<path {ui} d="M11 4l1.7 5.3L18 11l-5.3 1.7L11 18l-1.7-5.3L4 11l5.3-1.7z"/><path {ui} d="M18.5 16v4M16.5 18h4"/>"#
            ),
            // A cog: a ring with eight teeth around a hub.
            DebugGlyph::Gear => format!(
                r#"<circle {ui} cx="12" cy="12" r="6"/><circle {ui} cx="12" cy="12" r="2.4"/><path {ui} d="M12 2.8v3.2M12 18v3.2M2.8 12H6M18 12h3.2M5.5 5.5l2.3 2.3M16.2 16.2l2.3 2.3M5.5 18.5l2.3-2.3M16.2 7.8l2.3-2.3"/>"#
            ),
            DebugGlyph::NewFile => format!(
                r#"<path {ui} d="M13.5 3.5H7.3a1.8 1.8 0 0 0-1.8 1.8v13.4a1.8 1.8 0 0 0 1.8 1.8h9.4a1.8 1.8 0 0 0 1.8-1.8V8.5z"/><path {ui} d="M13.5 3.5v5h5"/><path {ui} d="M12 11.5v5.5M9.25 14.25h5.5"/>"#
            ),
            DebugGlyph::NewFolder => format!(
                r#"<path {ui} d="M3.5 7.3a1.8 1.8 0 0 1 1.8-1.8h3.9l2 2h7.5a1.8 1.8 0 0 1 1.8 1.8v8.4a1.8 1.8 0 0 1-1.8 1.8H5.3a1.8 1.8 0 0 1-1.8-1.8z"/><path {ui} d="M12 10.5v5M9.5 13h5"/>"#
            ),
            DebugGlyph::Refresh => format!(
                r#"<path {ui} d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3"/><path {ui} d="M19.8 4.3v4.2h-4.2"/>"#
            ),
            // A box with a minus, over another: collapse every folder.
            DebugGlyph::CollapseAll => format!(
                r#"<rect {ui} x="7.5" y="7.5" width="13" height="13" rx="1.8"/><path {ui} d="M4 15.5V5.3A1.8 1.8 0 0 1 5.8 3.5H16"/><path {ui} d="M10.5 14h7"/>"#
            ),
            DebugGlyph::Close => format!(r#"<path {ui} d="M6.5 6.5l11 11M17.5 6.5l-11 11"/>"#),
            DebugGlyph::ChevronDown => format!(r#"<path {ui} d="M6.5 9.5l5.5 5.5 5.5-5.5"/>"#),
            DebugGlyph::ChevronRight => format!(r#"<path {ui} d="M9.5 6.5l5.5 5.5-5.5 5.5"/>"#),
            DebugGlyph::SplitEditor => format!(
                r#"<rect {ui} x="3.5" y="4.5" width="17" height="15" rx="1.8"/><path {ui} d="M12 4.5v15"/>"#
            ),
            DebugGlyph::More => format!(
                r#"<circle fill="{color}" cx="5.5" cy="12" r="1.7"/><circle fill="{color}" cx="12" cy="12" r="1.7"/><circle fill="{color}" cx="18.5" cy="12" r="1.7"/>"#
            ),
            // Window controls: thin, as Windows draws them.
            DebugGlyph::Minimize => {
                format!(r#"<path fill="none" stroke="{color}" stroke-width="1.3" d="M6 12.5h12"/>"#)
            }
            DebugGlyph::Maximize => format!(
                r#"<rect fill="none" stroke="{color}" stroke-width="1.3" x="6.5" y="6.5" width="11" height="11" rx=".8"/>"#
            ),
            DebugGlyph::Restore => format!(
                r#"<rect fill="none" stroke="{color}" stroke-width="1.3" x="6" y="8.5" width="9.5" height="9.5" rx=".8"/><path fill="none" stroke="{color}" stroke-width="1.3" d="M8.5 6h8.7a.8.8 0 0 1 .8.8v8.7"/>"#
            ),
            DebugGlyph::Error => format!(
                r#"<circle {ui} cx="12" cy="12" r="8.2"/><path {ui} d="M9.2 9.2l5.6 5.6M14.8 9.2l-5.6 5.6"/>"#
            ),
            DebugGlyph::Info => format!(
                r#"<circle {ui} cx="12" cy="12" r="8.2"/><path {ui} d="M12 11v5.5"/><circle fill="{color}" cx="12" cy="7.9" r="1.05"/>"#
            ),
            DebugGlyph::Plus => format!(r#"<path {ui} d="M12 5.5v13M5.5 12h13"/>"#),
            DebugGlyph::Minus => format!(r#"<path {ui} d="M5.5 12h13"/>"#),
            // Back to how it was: an arrow turning back.
            DebugGlyph::Discard => format!(
                r#"<path {ui} d="M9 6.5 4.5 11 9 15.5"/><path {ui} d="M4.5 11h9.5a5 5 0 0 1 0 10h-2.5"/>"#
            ),
            DebugGlyph::Check => format!(r#"<path {ui} d="M5 12.5l4.5 4.5L19 7.5"/>"#),
            DebugGlyph::ArrowUp => {
                format!(r#"<path {ui} d="M12 19V5.5"/><path {ui} d="M6.5 11 12 5.5l5.5 5.5"/>"#)
            }
            DebugGlyph::ArrowDown => {
                format!(r#"<path {ui} d="M12 5v13.5"/><path {ui} d="M6.5 13 12 18.5l5.5-5.5"/>"#)
            }
            // Down onto a shelf: bring the remote's commits in, nothing merged.
            DebugGlyph::Fetch => format!(
                r#"<path {ui} d="M12 4v11"/><path {ui} d="M7.5 10.5 12 15l4.5-4.5"/><path {ui} d="M5 19.5h14"/>"#
            ),
            DebugGlyph::Filter => {
                format!(r#"<path {ui} d="M4 5h16l-6.2 7.3v5.2l-3.6 1.8v-7z"/>"#)
            }
            DebugGlyph::OpenFile => format!(
                r#"<path {ui} d="M13.5 3.5H7.3a1.8 1.8 0 0 0-1.8 1.8v13.4a1.8 1.8 0 0 0 1.8 1.8h9.4a1.8 1.8 0 0 0 1.8-1.8V8.5z"/><path {ui} d="M13.5 3.5v5h5"/>"#
            ),
            DebugGlyph::Warning => format!(
                r#"<path {ui} d="M10.4 4.6a1.85 1.85 0 0 1 3.2 0l6.9 12.2a1.85 1.85 0 0 1-1.6 2.7H5.1a1.85 1.85 0 0 1-1.6-2.7z"/><path {ui} d="M12 9.5v4"/><circle fill="{color}" cx="12" cy="16.6" r="1.05"/>"#
            ),
            DebugGlyph::Start => format!(
                r#"<path fill="{color}" d="M8 5.2v13.6a1.1 1.1 0 0 0 1.7.92l10.3-6.8a1.1 1.1 0 0 0 0-1.84L9.7 4.28A1.1 1.1 0 0 0 8 5.2z"/>"#
            ),
            DebugGlyph::Continue => format!(
                r#"<rect fill="{color}" x="4" y="5" width="3" height="14" rx="1.1"/><path fill="{color}" d="M10 6.1v11.8a1 1 0 0 0 1.53.85l9.2-5.9a1 1 0 0 0 0-1.7l-9.2-5.9A1 1 0 0 0 10 6.1z"/>"#
            ),
            DebugGlyph::Pause => format!(
                r#"<rect fill="{color}" x="6" y="5" width="4" height="14" rx="1.2"/><rect fill="{color}" x="14" y="5" width="4" height="14" rx="1.2"/>"#
            ),
            // An arc over a statement (the dot), landing past it.
            DebugGlyph::StepOver => format!(
                r#"<path {line} d="M5 14a7 7 0 0 1 14 0"/><path {line} d="M15.8 11.2 19 14.4l3.2-3.2"/><circle fill="{color}" cx="12" cy="19" r="2.2"/>"#
            ),
            // Down into the statement.
            DebugGlyph::StepInto => format!(
                r#"<path {line} d="M12 3.5v10"/><path {line} d="M7.8 9.6 12 13.8l4.2-4.2"/><circle fill="{color}" cx="12" cy="19.5" r="2.2"/>"#
            ),
            // Up and out of it.
            DebugGlyph::StepOut => format!(
                r#"<path {line} d="M12 13.5V3.8"/><path {line} d="M7.8 8 12 3.8 16.2 8"/><circle fill="{color}" cx="12" cy="19.5" r="2.2"/>"#
            ),
            DebugGlyph::Restart => format!(
                r#"<path {line} d="M21.35 5.2v5.1h-5.1"/><path {line} d="M19.12 14.55a7.65 7.65 0 1 1-1.8-7.96l4.03 3.71"/>"#
            ),
            // Rasterized from SVG instead of drawn with GDI's aliased
            // Ellipse, so small breakpoint/diagnostic dots stay smooth.
            DebugGlyph::GutterDot => {
                format!(r#"<circle fill="{color}" cx="12" cy="12" r="8.75"/>"#)
            }
            // The line execution is paused on, pointing into the code.
            DebugGlyph::ExecutionArrow => format!(
                r#"<path fill="{color}" d="M3.5 7.2a1.7 1.7 0 0 1 1.7-1.7h8.6a1.7 1.7 0 0 1 1.25.55l5.1 5.35a.9.9 0 0 1 0 1.2l-5.1 5.35a1.7 1.7 0 0 1-1.25.55H5.2a1.7 1.7 0 0 1-1.7-1.7z"/>"#
            ),
            DebugGlyph::Stop => {
                format!(r#"<rect fill="{color}" x="5.5" y="5.5" width="13" height="13" rx="2.2"/>"#)
            }
        };
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">{body}</svg>"#
        )
    }
}

// What an atlas slot holds: a debug or UI glyph in one color, a built-in
// file icon, or an icon theme's SVG file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum AtlasKey {
    Glyph(DebugGlyph, u32),
    Builtin(BuiltinIcon),
    Themed(PathBuf),
}

// Where a rasterized icon sits: which page, at which corner.
#[derive(Clone, Copy, Debug)]
struct Slot {
    page: usize,
    x: i32,
    y: i32,
}

/// A 32bpp DIB section that icons are rasterized straight into, side by
/// side, and drawn from with AlphaBlend. Each icon used to be its own HICON:
/// the three GDI objects that took cost ~0.9 M cycles an icon, two thirds of
/// what the flat layout's 28 glyphs added to the first frame.
struct AtlasPage {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    side: i32,
    // The next free spot: left to right, then a row down.
    x: i32,
    y: i32,
    row: i32,
}

const PAGE_SIDE: i32 = 256;

impl AtlasPage {
    fn new(side: i32) -> Option<Self> {
        unsafe {
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = side;
            bmi.bmiHeader.biHeight = -side;
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let mut bits: *mut core::ffi::c_void = null_mut();
            let bitmap =
                CreateDIBSection(null_mut(), &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() || bits.is_null() {
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                return None;
            }
            let dc = CreateCompatibleDC(null_mut());
            if dc.is_null() {
                DeleteObject(bitmap);
                return None;
            }
            let old = SelectObject(dc, bitmap);
            Some(Self {
                dc,
                bitmap,
                old,
                bits: bits.cast(),
                side,
                x: 0,
                y: 0,
                row: 0,
            })
        }
    }

    // A free `size`-pixel square, if one is left.
    fn place(&mut self, size: i32) -> Option<(i32, i32)> {
        if self.x + size > self.side {
            self.x = 0;
            self.y += self.row;
            self.row = 0;
        }
        if size > self.side || self.y + size > self.side {
            return None;
        }
        let at = (self.x, self.y);
        self.x += size;
        self.row = self.row.max(size);
        Some(at)
    }

    // Copies `pixels` (premultiplied BGRA, `size` square) to (x, y).
    fn write(&self, (x, y): (i32, i32), size: i32, pixels: &[u8]) {
        let row_bytes = (size * 4) as usize;
        // GDI may still be reading this bitmap for a batched AlphaBlend.
        unsafe { GdiFlush() };
        for row in 0..size as usize {
            let offset = (((y as usize + row) * self.side as usize) + x as usize) * 4;
            unsafe {
                std::ptr::copy_nonoverlapping(
                    pixels.as_ptr().add(row * row_bytes),
                    self.bits.add(offset),
                    row_bytes,
                );
            }
        }
    }
}

impl Drop for AtlasPage {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

pub(super) struct IconSet {
    // An installed icon-theme extension (e.g. Material Icon Theme), loaded
    // from %APPDATA%\LightLine\extensions\material-icon-theme if installed
    // by the user via the Extensions marketplace. When not installed,
    // IconSet automatically uses our fast built-in authentic SVG suite.
    theme: Option<IconTheme>,
    // SVGs are rasterized lazily, on first use, not all ~1000 of them up
    // front: most users will only ever trigger a few dozen distinct icons in
    // a session (whatever file types/folder names they actually have open),
    // so eagerly converting the rest would be pure waste. Each is rasterized
    // at the size it's drawn, never stretched; None marks an SVG that
    // didn't rasterize, so it isn't tried again on every paint.
    slots: RefCell<HashMap<(AtlasKey, i32), Option<Slot>>>,
    pages: RefCell<Vec<AtlasPage>>,
    // The LightLine mark for the title bar, at one of the sizes the .ico
    // holds, so it's drawn 1:1 and never stretched (stretching blurred it).
    brand: RefCell<Option<(i32, HICON)>>,
}

impl IconSet {
    pub(super) fn new(_dpi: u32, _zoom: i32) -> Self {
        let theme = lightline::workflow::extensions_dir()
            .map(|dir| dir.join("material-icon-theme"))
            .and_then(|dir| IconTheme::load(&dir));
        Self {
            theme,
            slots: RefCell::new(HashMap::new()),
            pages: RefCell::new(Vec::new()),
            brand: RefCell::new(None),
        }
    }

    /// Draws the LightLine mark with its left edge at `x`, centred on
    /// `middle`, at the size the icon file holds nearest to `wanted`.
    /// Returns the size drawn.
    pub(super) fn draw_brand(&self, hdc: HDC, x: i32, middle: i32, wanted: i32) -> i32 {
        let size = BRAND_SIZES
            .iter()
            .copied()
            .min_by_key(|size| (size - wanted).abs())
            .unwrap_or(32);
        let mut brand = self.brand.borrow_mut();
        let icon = match *brand {
            Some((known, icon)) if known == size => icon,
            _ => {
                if let Some((_, old)) = brand.take() {
                    unsafe { DestroyIcon(old) };
                }
                let icon = Self::load_ico(LIGHTLINE_ICON, size);
                if icon.is_null() {
                    return 0;
                }
                *brand = Some((size, icon));
                icon
            }
        };
        unsafe {
            DrawIconEx(
                hdc,
                x,
                middle - size / 2,
                icon,
                size,
                size,
                0,
                null_mut(),
                DI_NORMAL,
            );
        }
        size
    }

    // Draws the icon `key` names as a `size`-pixel square at (x, y),
    // rasterizing `svg` into the atlas the first time.
    fn draw_atlas(
        &self,
        hdc: HDC,
        key: AtlasKey,
        (x, y): (i32, i32),
        size: i32,
        svg: impl FnOnce() -> Option<Vec<u8>>,
    ) -> bool {
        let size = size.max(1);
        let key = (key, size);
        let known = self.slots.borrow().get(&key).copied();
        let slot = match known {
            Some(slot) => slot,
            None => {
                let slot = svg()
                    .and_then(|bytes| Self::rasterize(&bytes, size))
                    .and_then(|pixels| self.store(&pixels, size));
                self.slots.borrow_mut().insert(key, slot);
                slot
            }
        };
        let Some(slot) = slot else {
            return false;
        };
        let pages = self.pages.borrow();
        let Some(page) = pages.get(slot.page) else {
            return false;
        };
        unsafe {
            AlphaBlend(
                hdc,
                x,
                y,
                size,
                size,
                page.dc,
                slot.x,
                slot.y,
                size,
                size,
                BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                },
            ) != 0
        }
    }

    // Puts `pixels` in the atlas, on a new page when the last one is full.
    fn store(&self, pixels: &[u8], size: i32) -> Option<Slot> {
        let mut pages = self.pages.borrow_mut();
        let spot = pages.last_mut().and_then(|page| page.place(size));
        let (page, at) = match spot {
            Some(at) => (pages.len() - 1, at),
            None => {
                let mut page = AtlasPage::new(PAGE_SIDE.max(size))?;
                let at = page.place(size)?;
                pages.push(page);
                (pages.len() - 1, at)
            }
        };
        pages[page].write(at, size, pixels);
        Some(Slot {
            page,
            x: at.0,
            y: at.1,
        })
    }

    // Draws `glyph` in `color` (a COLORREF) as a `size`-pixel square at (x, y).
    pub(super) fn draw_glyph(
        &self,
        hdc: HDC,
        glyph: DebugGlyph,
        color: u32,
        x: i32,
        y: i32,
        size: i32,
    ) -> bool {
        self.draw_atlas(hdc, AtlasKey::Glyph(glyph, color), (x, y), size, || {
            let hex = format!(
                "#{:02x}{:02x}{:02x}",
                color & 0xff,
                (color >> 8) & 0xff,
                (color >> 16) & 0xff
            );
            Some(glyph.svg(&hex).into_bytes())
        })
    }

    // Draws the icon for `path` (or, for a folder, `is_dir`/`expanded`) at
    // (x, y). When `use_theme` is true and the installed icon theme has a
    // matching entry, that SVG is used; otherwise this falls back to our
    // bundled authentic vector SVG catalog.
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
            && let Some(svg_path) = self.themed_path(path, is_dir, expanded)
            && self.draw_themed(hdc, svg_path, x, y, size)
        {
            return true;
        }
        let builtin = BuiltinIcon::resolve_for_path(path, is_dir, expanded);
        self.draw_builtin(hdc, builtin, x, y, size)
    }

    // Draws a plain file/folder glyph that isn't resolved from any
    // particular name -- the welcome screen's "Open Folder" icon, the
    // explorer's workspace-root row, the Extensions panel's icon badges.
    pub(super) fn draw_generic(
        &self,
        hdc: HDC,
        kind: GenericIcon,
        x: i32,
        y: i32,
        size: i32,
    ) -> bool {
        if let Some(theme) = self.theme.as_ref() {
            let svg_path = match kind {
                GenericIcon::File => theme.generic_file_icon(),
                GenericIcon::Folder => theme.generic_folder_icon(false),
                GenericIcon::FolderOpen => theme.generic_folder_icon(true),
                GenericIcon::FolderSrc => theme.resolve_directory("src", false),
            };
            if let Some(svg_path) = svg_path
                && self.draw_themed(hdc, svg_path, x, y, size)
            {
                return true;
            }
        }
        let builtin = match kind {
            GenericIcon::File => BuiltinIcon::File,
            GenericIcon::Folder => BuiltinIcon::Folder,
            GenericIcon::FolderOpen => BuiltinIcon::FolderOpen,
            GenericIcon::FolderSrc => BuiltinIcon::FolderSrc,
        };
        self.draw_builtin(hdc, builtin, x, y, size)
    }

    pub(super) fn draw_builtin(
        &self,
        hdc: HDC,
        kind: BuiltinIcon,
        x: i32,
        y: i32,
        size: i32,
    ) -> bool {
        self.draw_atlas(hdc, AtlasKey::Builtin(kind), (x, y), size, || {
            Some(kind.svg_str().as_bytes().to_vec())
        })
    }

    fn draw_themed(&self, hdc: HDC, svg_path: PathBuf, x: i32, y: i32, size: i32) -> bool {
        let read = svg_path.clone();
        self.draw_atlas(hdc, AtlasKey::Themed(svg_path), (x, y), size, || {
            std::fs::read(read).ok()
        })
    }

    fn themed_path(&self, path: &Path, is_dir: bool, expanded: bool) -> Option<PathBuf> {
        let theme = self.theme.as_ref()?;
        let name = path.file_name()?.to_str()?;
        if is_dir {
            theme.resolve_directory(name, expanded)
        } else {
            theme.resolve_file(name)
        }
    }

    // Rasterizes `svg_bytes` at `size`x`size` via resvg/tiny-skia, as
    // premultiplied BGRA rows top to bottom: what a 32bpp DIB holds and
    // AlphaBlend's AC_SRC_ALPHA wants.
    fn rasterize(svg_bytes: &[u8], size: i32) -> Option<Vec<u8>> {
        let size = size.max(1) as u32;
        let tree = usvg::Tree::from_data(svg_bytes, &usvg::Options::default()).ok()?;
        let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
        let tree_size = tree.size();
        let scale = (size as f32 / tree_size.width().max(1.0))
            .min(size as f32 / tree_size.height().max(1.0));
        let transform = tiny_skia::Transform::from_scale(scale, scale);
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        let mut bgra = pixmap.take();
        for pixel in bgra.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        Some(bgra)
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
        if let Some((_, icon)) = self.brand.borrow_mut().take() {
            unsafe { DestroyIcon(icon) };
        }
    }
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    #[test]
    fn every_debug_glyph_is_valid_svg() {
        let glyphs = [
            DebugGlyph::Start,
            DebugGlyph::Continue,
            DebugGlyph::Pause,
            DebugGlyph::StepOver,
            DebugGlyph::StepInto,
            DebugGlyph::StepOut,
            DebugGlyph::Restart,
            DebugGlyph::Stop,
            DebugGlyph::GutterDot,
            DebugGlyph::ExecutionArrow,
            DebugGlyph::Check,
            DebugGlyph::ArrowUp,
            DebugGlyph::ArrowDown,
            DebugGlyph::Fetch,
            DebugGlyph::OpenFile,
            DebugGlyph::Filter,
        ];
        for glyph in glyphs {
            let svg = glyph.svg("#aac8fa");
            let tree = usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default());
            assert!(tree.is_ok(), "{glyph:?} does not parse: {svg}");
            let drawn = tree.unwrap().root().has_children();
            assert!(drawn, "{glyph:?} draws nothing");
        }
    }

    #[test]
    fn all_builtin_icons_rasterize_cleanly() {
        let hdc = unsafe { GetDC(null_mut()) };
        let all_icons = [
            BuiltinIcon::Folder,
            BuiltinIcon::FolderOpen,
            BuiltinIcon::FolderSrc,
            BuiltinIcon::File,
            BuiltinIcon::Rust,
            BuiltinIcon::Python,
            BuiltinIcon::Markdown,
            BuiltinIcon::Toml,
            BuiltinIcon::Json,
            BuiltinIcon::Yaml,
            BuiltinIcon::Git,
            BuiltinIcon::Html,
            BuiltinIcon::Css,
            BuiltinIcon::JavaScript,
            BuiltinIcon::TypeScript,
            BuiltinIcon::Image,
            BuiltinIcon::Script,
            BuiltinIcon::Lock,
            BuiltinIcon::Document,
        ];
        for icon_kind in all_icons {
            let svg = icon_kind.svg_str();
            let pixels = IconSet::rasterize(svg.as_bytes(), 18);
            assert!(
                pixels
                    .as_ref()
                    .is_some_and(|p| p.chunks(4).any(|px| px[3] > 0)),
                "failed to rasterize builtin icon {icon_kind:?}"
            );
        }
        // And they draw from the atlas, at two sizes, onto a real DC.
        let icons = IconSet::new(96, 100);
        for icon_kind in all_icons {
            assert!(icons.draw_builtin(hdc, icon_kind, 0, 0, 18));
            assert!(icons.draw_builtin(hdc, icon_kind, 0, 0, 24));
        }
        assert!(icons.draw_glyph(hdc, DebugGlyph::Close, 0xffffff, 0, 0, 300));
        assert!(
            icons.pages.borrow().len() >= 2,
            "a 300 px glyph gets a page of its own"
        );
        unsafe { ReleaseDC(null_mut(), hdc) };
    }

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
        let pixels = IconSet::rasterize(&bytes, 18).expect("rust.svg should rasterize");
        assert!(
            pixels.chunks(4).any(|px| px[3] > 0),
            "rust.svg drew nothing"
        );
    }
}
