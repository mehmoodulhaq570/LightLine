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

/// One resolved Zed color theme (its first `themes[]` variant -- real Zed
/// theme files can define more than one, e.g. a light/dark pair or a high
/// contrast variant; the first is the theme's primary one).
pub struct ZedColorTheme {
    // Flat chrome/editor keys, e.g. "editor.background", "border", "error".
    style: HashMap<String, String>,
    // "syntax.<name>.color", e.g. syntax["keyword"] = "#ff79c6ff".
    syntax: HashMap<String, String>,
}

impl ZedColorTheme {
    pub fn load(extension_dir: &Path) -> Option<Self> {
        let themes_dir = extension_dir.join("themes");
        let entry = std::fs::read_dir(&themes_dir)
            .ok()?
            .filter_map(|entry| entry.ok())
            .find(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))?;
        let text = std::fs::read_to_string(entry.path()).ok()?;
        let file: ThemeFile = serde_json::from_str(&text).ok()?;
        let variant = file.themes.into_iter().next()?;

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
        Some(Self { style, syntax })
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

    fn real_theme() -> Option<ZedColorTheme> {
        let dir = std::env::var_os("APPDATA").map(|appdata| {
            std::path::PathBuf::from(appdata).join("LightLine").join("extensions").join("dracula")
        })?;
        ZedColorTheme::load(&dir)
    }

    #[test]
    fn parses_8_and_6_digit_hex_colors() {
        assert_eq!(parse_rgba("#282a36ff"), Some(Rgba { r: 0x28, g: 0x2a, b: 0x36, a: 0xff }));
        assert_eq!(parse_rgba("#C9A8F933"), Some(Rgba { r: 0xC9, g: 0xA8, b: 0xF9, a: 0x33 }));
        assert_eq!(parse_rgba("282a36"), Some(Rgba { r: 0x28, g: 0x2a, b: 0x36, a: 0xff }));
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
        assert_eq!(theme.style_color("editor.background"), Some(Rgba { r: 0x28, g: 0x2a, b: 0x36, a: 0xff }));
        assert_eq!(theme.style_color("text"), Some(Rgba { r: 0xf8, g: 0xf8, b: 0xf2, a: 0xff }));
        assert_eq!(theme.syntax_color("keyword"), Some(Rgba { r: 0xff, g: 0x79, b: 0xc6, a: 0xff }));
        assert_eq!(theme.syntax_color("string"), Some(Rgba { r: 0xf1, g: 0xfa, b: 0x8c, a: 0xff }));
        assert_eq!(theme.syntax_color("comment"), Some(Rgba { r: 0x62, g: 0x72, b: 0xa4, a: 0xff }));
        // A genuinely translucent value, confirming alpha is preserved
        // rather than silently dropped.
        let active_line = theme.style_color("editor.active_line.background").unwrap();
        assert!(active_line.a < 0xff, "expected a translucent color, got alpha {}", active_line.a);
    }
}
