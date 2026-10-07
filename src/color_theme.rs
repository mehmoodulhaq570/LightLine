//! Parses a Zed color-theme extension's real JSON schema (verified against
//! the actual Dracula theme, github.com/dracula/zed: `{name, author, themes:
//! [{name, appearance, style: {...}}]}`, `style` holding both flat chrome
//! keys like `"editor.background"` and a nested `"syntax"` object of
//! `{color, font_style, font_weight}` entries).
//!
//! This module only parses -- it has no opinion on LightLine's own `Theme`
//! type (a windows_app/GDI concept); the mapping from Zed's color names to
//! LightLine's fields lives in `windows_app::color_theme_adapter`, the same
//! split `icon_theme.rs` (parsing) / `icons.rs` (GDI rendering) already uses.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize)]
struct ThemeFile {
    themes: Vec<ThemeVariant>,
}

#[derive(Deserialize)]
struct ThemeVariant {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    appearance: Option<String>,
    #[serde(default)]
    style: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct SyntaxEntry {
    color: Option<String>,
}

#[derive(Deserialize)]
struct Player {
    cursor: Option<String>,
    selection: Option<String>,
}

/// One variant of a Zed color theme. A theme file can define several (a
/// light/dark pair, or e.g. Catppuccin's Latte, Frappé, Macchiato and Mocha).
pub struct ZedColorTheme {
    /// The variant's display name, e.g. "Dracula" or "Catppuccin Mocha".
    pub name: String,
    /// False only for a variant marked `"appearance": "light"`.
    pub dark: bool,
    // Flat chrome/editor keys, e.g. "editor.background", "border", "error".
    style: HashMap<String, String>,
    // "syntax.<name>.color", e.g. syntax["keyword"] = "#ff79c6ff".
    syntax: HashMap<String, String>,
}

impl ZedColorTheme {
    /// The variant to use when the extension is installed: its first dark
    /// one (LightLine's own palette is dark), else its first.
    pub fn load(extension_dir: &Path) -> Option<Self> {
        let mut variants = Self::load_all(extension_dir);
        let index = variants.iter().position(|theme| theme.dark).unwrap_or(0);
        (index < variants.len()).then(|| variants.swap_remove(index))
    }

    /// Every variant of every theme file in the extension's `themes`
    /// folder, in file-name order.
    pub fn load_all(extension_dir: &Path) -> Vec<Self> {
        let Ok(entries) = std::fs::read_dir(extension_dir.join("themes")) else {
            return Vec::new();
        };
        let mut files: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect();
        files.sort();
        files
            .iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .flat_map(|text| Self::parse(&text))
            .collect()
    }

    /// The variants in one theme file's text.
    pub fn parse(text: &str) -> Vec<Self> {
        let Ok(file) = serde_json::from_str::<ThemeFile>(text) else {
            return Vec::new();
        };
        file.themes.into_iter().map(Self::from_variant).collect()
    }

    fn from_variant(variant: ThemeVariant) -> Self {
        let dark = !variant
            .appearance
            .as_deref()
            .is_some_and(|appearance| appearance.eq_ignore_ascii_case("light"));
        let mut style = HashMap::new();
        let mut syntax = HashMap::new();
        for (key, value) in variant.style {
            match key.as_str() {
                "syntax" => {
                    if let Ok(entries) =
                        serde_json::from_value::<HashMap<String, SyntaxEntry>>(value)
                    {
                        for (name, entry) in entries {
                            if let Some(color) = entry.color {
                                syntax.insert(name, color);
                            }
                        }
                    }
                }
                "players" => {
                    if let Ok(players) = serde_json::from_value::<Vec<Player>>(value)
                        && let Some(first) = players.into_iter().next()
                    {
                        if let Some(cursor) = first.cursor {
                            style.insert("players.0.cursor".to_string(), cursor);
                        }
                        if let Some(selection) = first.selection {
                            style.insert("players.0.selection".to_string(), selection);
                        }
                    }
                }
                _ => {
                    if let Some(text) = value.as_str() {
                        style.insert(key, text.to_string());
                    }
                }
            }
        }
        Self {
            name: variant.name.unwrap_or_else(|| "Unnamed theme".to_string()),
            dark,
            style,
            syntax,
        }
    }

    /// A flat chrome/editor color by its Zed key (e.g. `"editor.background"`,
    /// `"border"`, `"error"`, or the synthetic `"players.0.cursor"` /
    /// `"players.0.selection"`), parsed to RGBA.
    pub fn style_color(&self, key: &str) -> Option<Rgba> {
        self.style.get(key).and_then(|hex| parse_rgba(hex))
    }

    /// A syntax-highlighting color by its Zed name (e.g. `"keyword"`,
    /// `"string"`, `"comment.doc"`), parsed to RGBA.
    pub fn syntax_color(&self, name: &str) -> Option<Rgba> {
        self.syntax.get(name).and_then(|hex| parse_rgba(hex))
    }
}

/// Every color-theme variant installed under `extensions_dir`, with the
/// id of the extension it comes from.
pub fn installed(extensions_dir: &Path) -> Vec<(String, ZedColorTheme)> {
    let Ok(entries) = std::fs::read_dir(extensions_dir) else {
        return Vec::new();
    };
    let mut folders: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        // Staging and rollback copies are never selectable installed themes.
        .filter(|path| {
            !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        })
        .filter(|path| crate::extensions::zed_manifest::is_color_theme(path))
        .collect();
    folders.sort();
    folders
        .into_iter()
        .flat_map(|folder| {
            let id = folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            ZedColorTheme::load_all(&folder)
                .into_iter()
                .map(move |theme| (id.clone(), theme))
        })
        .collect()
}

/// A parsed Zed color: 8-hex-digit `#RRGGBBAA` (alpha last, matching every
/// key observed in the real Dracula theme -- Zed's own schema uses this
/// consistently), falling back to opaque if only 6 digits are given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

fn parse_rgba(hex: &str) -> Option<Rgba> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    match hex.len() {
        8 => Some(Rgba {
            r: u8::from_str_radix(&hex[0..2], 16).ok()?,
            g: u8::from_str_radix(&hex[2..4], 16).ok()?,
            b: u8::from_str_radix(&hex[4..6], 16).ok()?,
            a: u8::from_str_radix(&hex[6..8], 16).ok()?,
        }),
        6 => Some(Rgba {
            r: u8::from_str_radix(&hex[0..2], 16).ok()?,
            g: u8::from_str_radix(&hex[2..4], 16).ok()?,
            b: u8::from_str_radix(&hex[4..6], 16).ok()?,
            a: 255,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_themes_exclude_staging_and_backup_copies() {
        let root =
            std::env::temp_dir().join(format!("lightline-theme-staging-{}", std::process::id()));
        for name in ["theme", ".staging-theme", ".backup-theme"] {
            let dir = root.join(name).join("themes");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("theme.json"), r##"{"name":"Theme","themes":[{"name":"Theme","appearance":"dark","style":{"editor.background":"#101010"}}]}"##).unwrap();
        }
        let themes = installed(&root);
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].0, "theme");
        std::fs::remove_dir_all(root).unwrap();
    }

    fn real_theme() -> Option<ZedColorTheme> {
        let dir = std::env::var_os("APPDATA").map(|appdata| {
            std::path::PathBuf::from(appdata)
                .join("LightLine")
                .join("extensions")
                .join("dracula")
        })?;
        ZedColorTheme::load(&dir)
    }

    #[test]
    fn reads_every_variant_and_prefers_a_dark_one() {
        let text = r##"{"name": "Pair", "themes": [
            {"name": "Pair Light", "appearance": "light", "style": {"editor.background": "#ffffff"}},
            {"name": "Pair Dark", "appearance": "dark", "style": {"editor.background": "#101010",
              "syntax": {"keyword": {"color": "#ff0000"}}}}
        ]}"##;
        let variants = ZedColorTheme::parse(text);
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].name, "Pair Light");
        assert!(!variants[0].dark);
        assert!(variants[1].dark);
        assert_eq!(
            variants[1].syntax_color("keyword"),
            Some(Rgba {
                r: 0xff,
                g: 0,
                b: 0,
                a: 0xff
            })
        );
        let dir = std::env::temp_dir().join(format!("lightline-theme-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("themes")).unwrap();
        std::fs::write(dir.join("themes").join("pair.json"), text).unwrap();
        assert_eq!(ZedColorTheme::load(&dir).unwrap().name, "Pair Dark");
        assert_eq!(ZedColorTheme::load_all(&dir).len(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parses_8_and_6_digit_hex_colors() {
        assert_eq!(
            parse_rgba("#282a36ff"),
            Some(Rgba {
                r: 0x28,
                g: 0x2a,
                b: 0x36,
                a: 0xff
            })
        );
        assert_eq!(
            parse_rgba("#C9A8F933"),
            Some(Rgba {
                r: 0xC9,
                g: 0xA8,
                b: 0xF9,
                a: 0x33
            })
        );
        assert_eq!(
            parse_rgba("282a36"),
            Some(Rgba {
                r: 0x28,
                g: 0x2a,
                b: 0x36,
                a: 0xff
            })
        );
        assert!(parse_rgba("nope").is_none());
    }

    // Proves this parses the *real* Dracula theme, not a hand-written
    // fixture. Skips if it hasn't been installed into
    // %APPDATA%\LightLine\extensions\dracula on this machine.
    #[test]
    fn real_dracula_theme_has_expected_colors() {
        let Some(theme) = real_theme() else {
            eprintln!("skipped: install the real dracula extension to run this test");
            return;
        };
        // Verified against the actual theme JSON fetched from
        // github.com/dracula/zed during development.
        assert_eq!(
            theme.style_color("editor.background"),
            Some(Rgba {
                r: 0x28,
                g: 0x2a,
                b: 0x36,
                a: 0xff
            })
        );
        assert_eq!(
            theme.style_color("text"),
            Some(Rgba {
                r: 0xf8,
                g: 0xf8,
                b: 0xf2,
                a: 0xff
            })
        );
        assert_eq!(
            theme.syntax_color("keyword"),
            Some(Rgba {
                r: 0xff,
                g: 0x79,
                b: 0xc6,
                a: 0xff
            })
        );
        assert_eq!(
            theme.syntax_color("string"),
            Some(Rgba {
                r: 0xf1,
                g: 0xfa,
                b: 0x8c,
                a: 0xff
            })
        );
        assert_eq!(
            theme.syntax_color("comment"),
            Some(Rgba {
                r: 0x62,
                g: 0x72,
                b: 0xa4,
                a: 0xff
            })
        );
        // A genuinely translucent value, confirming alpha is preserved
        // rather than silently dropped.
        let active_line = theme.style_color("editor.active_line.background").unwrap();
        assert!(
            active_line.a < 0xff,
            "expected a translucent color, got alpha {}",
            active_line.a
        );
    }
}
