use std::collections::HashMap;
use std::path::PathBuf;

/// User-configurable settings loaded from `%APPDATA%\LightLine\settings.json`.
/// All fields have sensible defaults matching the current hardcoded values so
/// an absent or partial file still produces a usable editor.
#[derive(Clone)]
pub struct Settings {
    pub font_family: Option<String>,
    pub font_size: i32,
    pub tab_size: usize,
    pub insert_spaces: bool,
    pub word_wrap: bool,
    pub auto_close_pairs: bool,
    pub auto_indent: bool,
    pub bracket_matching: bool,
    pub indent_guides: bool,
    pub minimap: bool,
    pub smooth_scrolling: bool,
    pub parse_limit_kb: usize,
    // Colors can be overridden; values are 0xRRGGBB.
    pub colors: HashMap<String, u32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_family: None, // Auto-detect (Cascadia Mono / Consolas)
            font_size: 15,
            tab_size: 4,
            insert_spaces: true,
            word_wrap: false,
            auto_close_pairs: true,
            auto_indent: true,
            bracket_matching: true,
            indent_guides: true,
            minimap: false,
            smooth_scrolling: false,
            parse_limit_kb: 128,
            colors: HashMap::new(),
        }
    }
}

impl Settings {
    pub fn settings_dir() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|base| PathBuf::from(base).join("LightLine"))
    }

    pub fn settings_path() -> Option<PathBuf> {
        Self::settings_dir().map(|dir| dir.join("settings.json"))
    }

    /// Load settings from the user's settings file, falling back to defaults
    /// for any missing or unparseable fields.
    pub fn load() -> Self {
        let Some(path) = Self::settings_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        Self::from_json(&text)
    }

    /// Save current settings to the settings file, creating the directory
    /// if needed.
    pub fn save(&self) -> Result<(), String> {
        let dir = Self::settings_dir().ok_or("Could not determine settings directory")?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create settings dir: {e}"))?;
        let path = dir.join("settings.json");
        let json = self.to_json();
        std::fs::write(&path, json).map_err(|e| format!("Could not write settings: {e}"))
    }

    fn from_json(text: &str) -> Self {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return Self::default();
        };
        let mut settings = Self::default();
        if let Some(s) = value.get("fontFamily").and_then(|v| v.as_str()) {
            settings.font_family = Some(s.to_owned());
        }
        if let Some(n) = value.get("fontSize").and_then(|v| v.as_i64()) {
            settings.font_size = (n as i32).clamp(8, 48);
        }
        if let Some(n) = value.get("tabSize").and_then(|v| v.as_u64()) {
            settings.tab_size = (n as usize).clamp(1, 16);
        }
        if let Some(b) = value.get("insertSpaces").and_then(|v| v.as_bool()) {
            settings.insert_spaces = b;
        }
        if let Some(b) = value.get("wordWrap").and_then(|v| v.as_bool()) {
            settings.word_wrap = b;
        }
        if let Some(b) = value.get("autoClosePairs").and_then(|v| v.as_bool()) {
            settings.auto_close_pairs = b;
        }
        if let Some(b) = value.get("autoIndent").and_then(|v| v.as_bool()) {
            settings.auto_indent = b;
        }
        if let Some(b) = value.get("bracketMatching").and_then(|v| v.as_bool()) {
            settings.bracket_matching = b;
        }
        if let Some(b) = value.get("indentGuides").and_then(|v| v.as_bool()) {
            settings.indent_guides = b;
        }
        if let Some(b) = value.get("minimap").and_then(|v| v.as_bool()) {
            settings.minimap = b;
        }
        if let Some(b) = value.get("smoothScrolling").and_then(|v| v.as_bool()) {
            settings.smooth_scrolling = b;
        }
        if let Some(n) = value.get("parseLimitKb").and_then(|v| v.as_u64()) {
            settings.parse_limit_kb = (n as usize).clamp(32, 4096);
        }
        if let Some(obj) = value.get("colors").and_then(|v| v.as_object()) {
            for (key, val) in obj {
                if let Some(hex) = val.as_str()
                    && let Some(color) = parse_hex_color(hex)
                {
                    settings.colors.insert(key.clone(), color);
                }
            }
        }
        settings
    }

    fn to_json(&self) -> String {
        let mut obj = serde_json::Map::new();
        if let Some(ref family) = self.font_family {
            obj.insert(
                "fontFamily".into(),
                serde_json::Value::String(family.clone()),
            );
        }
        obj.insert(
            "fontSize".into(),
            serde_json::Value::Number(self.font_size.into()),
        );
        obj.insert(
            "tabSize".into(),
            serde_json::Value::Number(self.tab_size.into()),
        );
        obj.insert(
            "insertSpaces".into(),
            serde_json::Value::Bool(self.insert_spaces),
        );
        obj.insert(
            "wordWrap".into(),
            serde_json::Value::Bool(self.word_wrap),
        );
        obj.insert(
            "autoClosePairs".into(),
            serde_json::Value::Bool(self.auto_close_pairs),
        );
        obj.insert(
            "autoIndent".into(),
            serde_json::Value::Bool(self.auto_indent),
        );
        obj.insert(
            "bracketMatching".into(),
            serde_json::Value::Bool(self.bracket_matching),
        );
        obj.insert(
            "indentGuides".into(),
            serde_json::Value::Bool(self.indent_guides),
        );
        obj.insert(
            "minimap".into(),
            serde_json::Value::Bool(self.minimap),
        );
        obj.insert(
            "smoothScrolling".into(),
            serde_json::Value::Bool(self.smooth_scrolling),
        );
        obj.insert(
            "parseLimitKb".into(),
            serde_json::Value::Number(self.parse_limit_kb.into()),
        );
        serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap_or_default()
    }
}

/// Parse a CSS-style hex color string like "#1c2b3f" or "1c2b3f" into a Win32
/// COLORREF (0x00BBGGRR).
fn parse_hex_color(hex: &str) -> Option<u32> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_have_expected_values() {
        let s = Settings::default();
        assert_eq!(s.font_size, 15);
        assert_eq!(s.tab_size, 4);
        assert!(s.auto_close_pairs);
        assert!(s.auto_indent);
        assert!(s.bracket_matching);
    }

    #[test]
    fn parses_valid_json_settings() {
        let json = r##"{
            "fontFamily": "JetBrains Mono",
            "fontSize": 18,
            "tabSize": 2,
            "autoClosePairs": false,
            "colors": { "editorBg": "#0c1523" }
        }"##;
        let s = Settings::from_json(json);
        assert_eq!(s.font_family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(s.font_size, 18);
        assert_eq!(s.tab_size, 2);
        assert!(!s.auto_close_pairs);
        assert!(s.auto_indent); // default preserved
        assert_eq!(s.colors.get("editorBg"), Some(&0x23150c));
    }

    #[test]
    fn malformed_json_returns_defaults() {
        let s = Settings::from_json("not json {{{");
        assert_eq!(s.font_size, 15);
        assert!(s.auto_close_pairs);
    }

    #[test]
    fn hex_color_parsing() {
        assert_eq!(parse_hex_color("#ff0000"), Some(0x0000ff));
        assert_eq!(parse_hex_color("00ff00"), Some(0x00ff00));
        assert_eq!(parse_hex_color("#0000ff"), Some(0xff0000));
        assert_eq!(parse_hex_color("nope"), None);
        assert_eq!(parse_hex_color("#fff"), None);
    }

    #[test]
    fn round_trip_serialization() {
        let mut s = Settings {
            font_family: Some("Fira Code".into()),
            ..Settings::default()
        };
        s.font_size = 20;
        let json = s.to_json();
        let loaded = Settings::from_json(&json);
        assert_eq!(loaded.font_family.as_deref(), Some("Fira Code"));
        assert_eq!(loaded.font_size, 20);
    }
}
