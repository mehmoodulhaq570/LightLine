//! Loads a Zed icon-theme extension (e.g. the real "Material Icon Theme"
//! extension from <https://github.com/zed-extensions/material-icon-theme>)
//! from a local extension directory, and resolves file/folder names to the
//! icon SVG they map to.
//!
//! This only understands the pure-data icon-theme extension type — nothing
//! here executes extension code. See `docs/extension_implementation_plan.md`
//! for how this fits into the wider plan.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct ThemeFile {
    themes: Vec<ThemeVariant>,
}

#[derive(Deserialize)]
struct ThemeVariant {
    #[serde(default)]
    file_icons: HashMap<String, IconDef>,
    #[serde(default)]
    file_suffixes: HashMap<String, String>,
    #[serde(default)]
    file_stems: HashMap<String, String>,
    #[serde(default)]
    directory_icons: Option<DirectoryIcons>,
    #[serde(default)]
    named_directory_icons: HashMap<String, DirectoryIcons>,
}

#[derive(Deserialize)]
struct IconDef {
    path: String,
}

#[derive(Deserialize)]
struct DirectoryIcons {
    collapsed: String,
    expanded: String,
}

/// A loaded icon theme, resolved against the extension directory it came
/// from so relative SVG paths (e.g. `"./icons/rust.svg"`) become real paths
/// on disk.
pub struct IconTheme {
    root: PathBuf,
    file_icons: HashMap<String, IconDef>,
    file_suffixes: HashMap<String, String>,
    file_stems: HashMap<String, String>,
    directory_icons: Option<DirectoryIcons>,
    named_directory_icons: HashMap<String, DirectoryIcons>,
}

impl IconTheme {
    /// Loads the icon theme from `extension_dir` (the directory containing
    /// `extension.toml`, `icon_themes/`, and `icons/`). Returns `None` if the
    /// extension isn't installed there or its files can't be parsed — the
    /// caller is expected to fall back to a bundled default rather than
    /// error out, the same way a missing LSP server degrades gracefully
    /// instead of crashing.
    pub fn load(extension_dir: &Path) -> Option<Self> {
        let icon_themes_dir = extension_dir.join("icon_themes");
        let entry = std::fs::read_dir(&icon_themes_dir)
            .ok()?
            .filter_map(|entry| entry.ok())
            .find(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))?;
        let text = std::fs::read_to_string(entry.path()).ok()?;
        let file: ThemeFile = serde_json::from_str(&text).ok()?;
        let variant = file.themes.into_iter().next()?;
        Some(Self {
            root: extension_dir.to_path_buf(),
            file_icons: variant.file_icons,
            file_suffixes: variant.file_suffixes,
            file_stems: variant.file_stems,
            directory_icons: variant.directory_icons,
            named_directory_icons: variant.named_directory_icons,
        })
    }

    /// Resolves a file's name to the absolute path of its SVG icon,
    /// preferring an exact `file_stems` match (handles dotfiles and
    /// extensionless names like `justfile`, `composer.lock`) before falling
    /// back to `file_suffixes`, trying progressively shorter suffixes after
    /// each dot so compound extensions like `prompt.md` resolve too.
    pub fn resolve_file(&self, file_name: &str) -> Option<PathBuf> {
        let lower = file_name.to_ascii_lowercase();
        let id = self
            .file_stems
            .get(file_name)
            .or_else(|| self.file_stems.get(lower.as_str()))
            .or_else(|| {
                let mut rest = lower.as_str();
                while let Some(dot) = rest.find('.') {
                    rest = &rest[dot + 1..];
                    if let Some(id) = self.file_suffixes.get(rest) {
                        return Some(id);
                    }
                }
                None
            })?;
        let relative = &self.file_icons.get(id)?.path;
        Some(self.resolve_path(relative))
    }

    /// Resolves a folder's name (and expanded/collapsed state) to the
    /// absolute path of its SVG icon: `named_directory_icons` first (e.g.
    /// `"src"` gets its own icon), then the theme's generic
    /// `directory_icons` default.
    pub fn resolve_directory(&self, dir_name: &str, expanded: bool) -> Option<PathBuf> {
        let lower = dir_name.to_ascii_lowercase();
        let icons = self
            .named_directory_icons
            .get(dir_name)
            .or_else(|| self.named_directory_icons.get(lower.as_str()))
            .or(self.directory_icons.as_ref())?;
        let relative = if expanded { &icons.expanded } else { &icons.collapsed };
        Some(self.resolve_path(relative))
    }

    /// The theme's generic default file icon (its `file_icons["file"]`
    /// entry), used by chrome UI that needs a plain "file" glyph without
    /// resolving any particular filename.
    pub fn generic_file_icon(&self) -> Option<PathBuf> {
        let relative = &self.file_icons.get("file")?.path;
        Some(self.resolve_path(relative))
    }

    /// The theme's generic default folder icon (collapsed/expanded), used by
    /// chrome UI that needs a plain "folder" glyph without resolving any
    /// particular folder name.
    pub fn generic_folder_icon(&self, expanded: bool) -> Option<PathBuf> {
        let icons = self.directory_icons.as_ref()?;
        let relative = if expanded { &icons.expanded } else { &icons.collapsed };
        Some(self.resolve_path(relative))
    }

    fn resolve_path(&self, relative: &str) -> PathBuf {
        self.root.join(relative.trim_start_matches("./"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Proves LightLine can load and resolve the *real* Zed "Material Icon
    // Theme" extension (github.com/zed-extensions/material-icon-theme), not
    // a hand-written fixture — the whole point of this feature. Skips (does
    // not fail) when the extension hasn't been installed into
    // %APPDATA%\LightLine\extensions\material-icon-theme on this machine,
    // the same way the LSP live tests skip when rust-analyzer/pyright aren't
    // on PATH, since this needs a real local install, not vendored fixtures.
    fn real_theme() -> Option<IconTheme> {
        let dir = lightline_extensions_dir()?.join("material-icon-theme");
        IconTheme::load(&dir)
    }

    fn lightline_extensions_dir() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("APPDATA")?).join("LightLine").join("extensions"))
    }

    #[test]
    fn real_material_icon_theme_resolves_common_files_and_folders() {
        let Some(theme) = real_theme() else {
            eprintln!(
                "skipped: install the real extension into \
                 %APPDATA%\\LightLine\\extensions\\material-icon-theme to run this test"
            );
            return;
        };

        let rust = theme.resolve_file("main.rs").expect("main.rs should resolve");
        assert!(rust.ends_with("icons/rust.svg") || rust.ends_with("icons\\rust.svg"));
        assert!(rust.is_file(), "resolved path {rust:?} should exist on disk");

        let python = theme.resolve_file("main.py").expect("main.py should resolve");
        assert!(python.is_file());

        // The real theme gives package.json its own Node.js icon rather than
        // a generic JSON one -- exactly the kind of real-world mapping a
        // hand-written fixture wouldn't have caught.
        let package_json = theme.resolve_file("package.json").expect("package.json should resolve");
        assert!(package_json.ends_with("icons/nodejs.svg") || package_json.ends_with("icons\\nodejs.svg"));
        assert!(package_json.is_file());

        let readme = theme.resolve_file("README.md").expect("README.md should resolve");
        assert!(readme.is_file());

        let src_collapsed = theme.resolve_directory("src", false).expect("src folder should resolve");
        assert!(src_collapsed.ends_with("icons/folder-src.svg") || src_collapsed.ends_with("icons\\folder-src.svg"));
        assert!(src_collapsed.is_file());

        let src_expanded = theme.resolve_directory("src", true).expect("expanded src folder should resolve");
        assert!(
            src_expanded.ends_with("icons/folder-src-open.svg")
                || src_expanded.ends_with("icons\\folder-src-open.svg")
        );
        assert!(src_expanded.is_file());

        // An unrecognized folder name still gets the theme's generic default
        // rather than resolving to nothing.
        let generic_folder = theme.resolve_directory("some-random-folder-name", false);
        assert!(generic_folder.is_some());
    }
}
