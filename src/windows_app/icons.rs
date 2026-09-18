use super::scaled;
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

macro_rules! icon_bytes {
    ($name:literal) => {
        include_bytes!(concat!("../../assets/material-icon-theme/", $name, ".ico")) as &[u8]
    };
}
const MATERIAL_ICONS: [(&str, &[u8]); 22] = [
    ("file", icon_bytes!("file")),
    ("folder", icon_bytes!("folder")),
    ("folder-open", icon_bytes!("folder-open")),
    ("folder-src", icon_bytes!("folder-src")),
    ("folder-src-open", icon_bytes!("folder-src-open")),
    ("folder-docs", icon_bytes!("folder-docs")),
    ("folder-docs-open", icon_bytes!("folder-docs-open")),
    ("rust", icon_bytes!("rust")),
    ("toml", icon_bytes!("toml")),
    ("markdown", icon_bytes!("markdown")),
    ("json", icon_bytes!("json")),
    ("python", icon_bytes!("python")),
    ("c", icon_bytes!("c")),
    ("cpp", icon_bytes!("cpp")),
    ("html", icon_bytes!("html")),
    ("css", icon_bytes!("css")),
    ("javascript", icon_bytes!("javascript")),
    ("typescript", icon_bytes!("typescript")),
    ("git", icon_bytes!("git")),
    ("lock", icon_bytes!("lock")),
    ("yaml", icon_bytes!("yaml")),
    ("readme", icon_bytes!("readme")),
];
const LIGHTLINE_ICON: &[u8] = include_bytes!("../../assets/lightline.ico");

pub(super) struct AppIcons {
    pub(super) small: HICON,
    pub(super) large: HICON,
}

impl AppIcons {
    pub(super) fn new() -> Option<Self> {
        let small = IconSet::load_ico(LIGHTLINE_ICON, 16);
        let large = IconSet::load_ico(LIGHTLINE_ICON, 32);
        if small.is_null() || large.is_null() {
            if !small.is_null() {
                unsafe { DestroyIcon(small) };
            }
            if !large.is_null() {
                unsafe { DestroyIcon(large) };
            }
            return None;
        }
        Some(Self { small, large })
    }
}

impl Drop for AppIcons {
    fn drop(&mut self) {
        unsafe {
            DestroyIcon(self.small);
            DestroyIcon(self.large);
        }
    }
}

pub(super) struct IconSet {
    handles: Vec<(&'static str, HICON)>,
}

impl IconSet {
    pub(super) fn new(dpi: u32, zoom: i32) -> Self {
        let size = scaled(18, dpi, zoom);
        let handles = MATERIAL_ICONS
            .iter()
            .map(|&(name, data)| (name, Self::load_ico(data, size)))
            .collect();
        Self { handles }
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

    pub(super) fn draw(&self, hdc: HDC, name: &str, x: i32, y: i32, size: i32) -> bool {
        let Some(&(_, icon)) = self.handles.iter().find(|(key, _)| *key == name) else {
            return false;
        };
        if icon.is_null() {
            return false;
        }
        unsafe { DrawIconEx(hdc, x, y, icon, size, size, 0, null_mut(), DI_NORMAL) != 0 }
    }
}

impl Drop for IconSet {
    fn drop(&mut self) {
        for (_, icon) in &self.handles {
            if !icon.is_null() {
                unsafe { DestroyIcon(*icon) };
            }
        }
    }
}

pub(super) fn material_icon_for(path: &Path, is_dir: bool, expanded: bool) -> &'static str {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if is_dir {
        return match (file_name.as_str(), expanded) {
            ("src", true) => "folder-src-open",
            ("src", false) => "folder-src",
            ("docs", true) => "folder-docs-open",
            ("docs", false) => "folder-docs",
            (_, true) => "folder-open",
            _ => "folder",
        };
    }
    if file_name == "readme.md" {
        return "readme";
    }
    if file_name == ".gitignore" || file_name == ".gitattributes" {
        return "git";
    }
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        "json" | "jsonc" => "json",
        "py" | "pyw" => "python",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" => "cpp",
        "html" | "htm" => "html",
        "css" => "css",
        "js" | "jsx" | "mjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "lock" => "lock",
        "yaml" | "yml" => "yaml",
        _ => "file",
    }
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    #[test]
    fn bundled_material_icons_load_as_windows_icons() {
        for (name, data) in MATERIAL_ICONS {
            let icon = IconSet::load_ico(data, 24);
            assert!(!icon.is_null(), "failed to load {name}");
            unsafe { DestroyIcon(icon) };
        }
    }

    #[test]
    fn lightline_app_icon_loads_at_both_window_sizes() {
        assert!(AppIcons::new().is_some());
    }
}
