use std::collections::HashMap;
use std::path::PathBuf;

use crate::terminal::ShellKind;

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
    pub format_on_save: bool,
    /// False once the user turns Prettier off in the Extensions panel; its
    /// file types are then left to the language server.
    pub prettier_enabled: bool,
    /// Show the language server's inlay hints (types, parameter names).
    pub inlay_hints: bool,
    /// Highlight the other uses of the name at the caret.
    pub occurrences_highlight: bool,
    /// Pin the first lines of the blocks the view is inside above the code.
    pub sticky_scroll: bool,
    pub bracket_matching: bool,
    pub indent_guides: bool,
    pub minimap: bool,
    pub smooth_scrolling: bool,
    pub parse_limit_kb: usize,
    pub default_terminal_profile: ShellKind,
    /// Load web images in Markdown previews without asking first.
    pub markdown_load_remote_images: bool,
    /// The installed color-theme variant in use, by name (e.g. "Dracula");
    /// None is LightLine's own theme.
    pub color_theme: Option<String>,
    /// The AI Assistant's server: an OpenAI-compatible endpoint such as
    /// Ollama's (the default) or LM Studio's.
    pub ai_endpoint: String,
    /// The model the AI Assistant chats with; None until one is chosen,
    /// which is what leaves the assistant off.
    pub ai_model: Option<String>,
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
            format_on_save: false,
            prettier_enabled: true,
            inlay_hints: true,
            occurrences_highlight: true,
            sticky_scroll: true,
            bracket_matching: true,
            indent_guides: true,
            minimap: true,
            smooth_scrolling: false,
            parse_limit_kb: 4096,
            default_terminal_profile: ShellKind::PowerShell,
            markdown_load_remote_images: false,
            color_theme: None,
            ai_endpoint: crate::ai::DEFAULT_ENDPOINT.to_string(),
            ai_model: None,
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
        Self::try_load().unwrap_or_default()
    }

    /// Like `load`, but reports a settings file that exists and can't be
    /// used instead of silently falling back to defaults. A missing file is
    /// not an error: it just means nothing has been customized yet.
    pub fn try_load() -> Result<Self, String> {
        let Some(path) = Self::settings_path() else {
            return Ok(Self::default());
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Could not read settings.json: {e}")),
        }
    }

    /// Save current settings to the settings file, creating the directory
    /// if needed.
    pub fn save(&self) -> Result<(), String> {
        let dir = Self::settings_dir().ok_or("Could not determine settings directory")?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create settings dir: {e}"))?;
        let path = dir.join("settings.json");
        let existing = match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("Could not read settings.json: {e}")),
        };
        let json = self.merged_json(existing.as_deref())?;
        std::fs::write(&path, json).map_err(|e| format!("Could not write settings: {e}"))
    }

    /// The settings file's text `existing` with these settings written over
    /// LightLine's own keys. Every other key, such as "colors", is kept as it
    /// was: saving used to rewrite the file from scratch and drop them. A file
    /// that isn't valid JSON is left alone rather than replaced, so a hand
    /// edit with a typo in it isn't lost.
    fn merged_json(&self, existing: Option<&str>) -> Result<String, String> {
        let existing = existing.filter(|text| !text.trim().is_empty());
        let mut obj = match existing.map(serde_json::from_str::<serde_json::Value>) {
            None => serde_json::Map::new(),
            Some(Ok(serde_json::Value::Object(obj))) => obj,
            Some(_) => {
                return Err(
                    "settings.json has an error, so it wasn't changed. Fix it with Open Settings (JSON)"
                        .into(),
                );
            }
        };
        // Keys left unset (no color theme, no model) must go, not linger.
        for key in OWN_KEYS {
            obj.remove(*key);
        }
        obj.extend(self.to_object());
        Ok(serde_json::to_string_pretty(&serde_json::Value::Object(obj)).unwrap_or_default())
    }

    fn parse(text: &str) -> Result<Self, String> {
        let value = serde_json::from_str::<serde_json::Value>(text)
            .map_err(|e| format!("settings.json is invalid JSON ({e}); settings not applied"))?;
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
        if let Some(b) = value.get("formatOnSave").and_then(|v| v.as_bool()) {
            settings.format_on_save = b;
        }
        if let Some(b) = value.get("prettierEnabled").and_then(|v| v.as_bool()) {
            settings.prettier_enabled = b;
        }
        if let Some(b) = value.get("inlayHints").and_then(|v| v.as_bool()) {
            settings.inlay_hints = b;
        }
        if let Some(b) = value.get("occurrencesHighlight").and_then(|v| v.as_bool()) {
            settings.occurrences_highlight = b;
        }
        if let Some(b) = value.get("stickyScroll").and_then(|v| v.as_bool()) {
            settings.sticky_scroll = b;
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
            settings.parse_limit_kb = (n as usize).clamp(32, 16384);
        }
        if let Some(name) = value.get("colorTheme").and_then(|v| v.as_str()) {
            let name = name.trim();
            if !name.is_empty() {
                settings.color_theme = Some(name.to_owned());
            }
        }
        if let Some(endpoint) = value.get("aiEndpoint").and_then(|v| v.as_str()) {
            let endpoint = endpoint.trim();
            if !endpoint.is_empty() {
                settings.ai_endpoint = endpoint.to_owned();
            }
        }
        if let Some(model) = value.get("aiModel").and_then(|v| v.as_str()) {
            let model = model.trim();
            if !model.is_empty() {
                settings.ai_model = Some(model.to_owned());
            }
        }
        if let Some(b) = value
            .get("markdownLoadRemoteImages")
            .and_then(|v| v.as_bool())
        {
            settings.markdown_load_remote_images = b;
        }
        if let Some(s) = value
            .get("terminalDefaultProfile")
            .or_else(|| value.get("terminalShell"))
            .and_then(|v| v.as_str())
        {
            let lower = s.trim().to_ascii_lowercase();
            settings.default_terminal_profile = match lower.as_str() {
                "cmd" | "command prompt" | "commandprompt" => ShellKind::CommandPrompt,
                "bash" | "gitbash" | "git bash" => ShellKind::GitBash,
                "wsl" => ShellKind::Wsl,
                _ => ShellKind::PowerShell,
            };
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
        Ok(settings)
    }

    #[cfg(test)]
    fn to_json(&self) -> String {
        serde_json::to_string_pretty(&serde_json::Value::Object(self.to_object()))
            .unwrap_or_default()
    }

    fn to_object(&self) -> serde_json::Map<String, serde_json::Value> {
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
        obj.insert("wordWrap".into(), serde_json::Value::Bool(self.word_wrap));
        obj.insert(
            "autoClosePairs".into(),
            serde_json::Value::Bool(self.auto_close_pairs),
        );
        obj.insert(
            "autoIndent".into(),
            serde_json::Value::Bool(self.auto_indent),
        );
        obj.insert(
            "formatOnSave".into(),
            serde_json::Value::Bool(self.format_on_save),
        );
        obj.insert(
            "prettierEnabled".into(),
            serde_json::Value::Bool(self.prettier_enabled),
        );
        obj.insert(
            "inlayHints".into(),
            serde_json::Value::Bool(self.inlay_hints),
        );
        obj.insert(
            "occurrencesHighlight".into(),
            serde_json::Value::Bool(self.occurrences_highlight),
        );
        obj.insert(
            "stickyScroll".into(),
            serde_json::Value::Bool(self.sticky_scroll),
        );
        obj.insert(
            "bracketMatching".into(),
            serde_json::Value::Bool(self.bracket_matching),
        );
        obj.insert(
            "indentGuides".into(),
            serde_json::Value::Bool(self.indent_guides),
        );
        obj.insert("minimap".into(), serde_json::Value::Bool(self.minimap));
        obj.insert(
            "smoothScrolling".into(),
            serde_json::Value::Bool(self.smooth_scrolling),
        );
        obj.insert(
            "parseLimitKb".into(),
            serde_json::Value::Number(self.parse_limit_kb.into()),
        );
        if let Some(name) = &self.color_theme {
            obj.insert("colorTheme".into(), serde_json::Value::String(name.clone()));
        }
        obj.insert(
            "aiEndpoint".into(),
            serde_json::Value::String(self.ai_endpoint.clone()),
        );
        if let Some(model) = &self.ai_model {
            obj.insert("aiModel".into(), serde_json::Value::String(model.clone()));
        }
        obj.insert(
            "markdownLoadRemoteImages".into(),
            serde_json::Value::Bool(self.markdown_load_remote_images),
        );
        obj.insert(
            "terminalDefaultProfile".into(),
            serde_json::Value::String(self.default_terminal_profile.name().to_string()),
        );
        obj
    }
}

// Every key `to_object` can write.
const OWN_KEYS: &[&str] = &[
    "fontFamily",
    "fontSize",
    "tabSize",
    "insertSpaces",
    "wordWrap",
    "autoClosePairs",
    "autoIndent",
    "formatOnSave",
    "prettierEnabled",
    "inlayHints",
    "occurrencesHighlight",
    "stickyScroll",
    "bracketMatching",
    "indentGuides",
    "minimap",
    "smoothScrolling",
    "parseLimitKb",
    "colorTheme",
    "aiEndpoint",
    "aiModel",
    "markdownLoadRemoteImages",
    "terminalDefaultProfile",
];

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
        assert!(!s.format_on_save);
        assert_eq!(s.parse_limit_kb, 4096);
        assert_eq!(s.default_terminal_profile, ShellKind::PowerShell);
    }

    #[test]
    fn parse_limit_round_trips() {
        let s = Settings {
            parse_limit_kb: 8192,
            ..Settings::default()
        };
        let loaded = Settings::parse(&s.to_json()).unwrap();
        assert_eq!(loaded.parse_limit_kb, 8192);
    }

    #[test]
    fn format_on_save_round_trips() {
        let s = Settings {
            format_on_save: true,
            ..Settings::default()
        };
        let loaded = Settings::parse(&s.to_json()).unwrap();
        assert!(loaded.format_on_save);
    }

    #[test]
    fn inlay_hints_are_on_until_turned_off() {
        assert!(Settings::default().inlay_hints);
        let s = Settings {
            inlay_hints: false,
            ..Settings::default()
        };
        assert!(!Settings::parse(&s.to_json()).unwrap().inlay_hints);
    }

    #[test]
    fn occurrence_highlights_are_on_until_turned_off() {
        assert!(Settings::default().occurrences_highlight);
        let s = Settings {
            occurrences_highlight: false,
            ..Settings::default()
        };
        assert!(!Settings::parse(&s.to_json()).unwrap().occurrences_highlight);
    }

    #[test]
    fn sticky_scroll_is_on_until_turned_off() {
        assert!(Settings::default().sticky_scroll);
        let s = Settings {
            sticky_scroll: false,
            ..Settings::default()
        };
        assert!(!Settings::parse(&s.to_json()).unwrap().sticky_scroll);
    }

    #[test]
    fn prettier_is_on_until_turned_off() {
        assert!(Settings::default().prettier_enabled);
        let s = Settings {
            prettier_enabled: false,
            ..Settings::default()
        };
        assert!(!Settings::parse(&s.to_json()).unwrap().prettier_enabled);
    }

    #[test]
    fn saving_keeps_colors_and_unknown_keys() {
        let existing = r##"{"colors": {"text": "#d8dee9"}, "myKey": 1, "fontSize": 20}"##;
        let s = Settings {
            font_size: 16,
            ..Settings::default()
        };
        let saved: serde_json::Value =
            serde_json::from_str(&s.merged_json(Some(existing)).unwrap()).unwrap();
        assert_eq!(saved["colors"]["text"], "#d8dee9");
        assert_eq!(saved["myKey"], 1);
        assert_eq!(saved["fontSize"], 16);
        assert_eq!(Settings::parse(&saved.to_string()).unwrap().colors.len(), 1);
    }

    #[test]
    fn saving_removes_settings_that_were_unset() {
        let existing = r#"{"colorTheme": "Dracula", "aiModel": "llama3.2:1b"}"#;
        let saved: serde_json::Value =
            serde_json::from_str(&Settings::default().merged_json(Some(existing)).unwrap())
                .unwrap();
        assert!(saved.get("colorTheme").is_none());
        assert!(saved.get("aiModel").is_none());
    }

    #[test]
    fn saving_leaves_a_broken_file_alone() {
        let settings = Settings::default();
        assert!(
            settings
                .merged_json(Some("{ \"fontSize\": 14,, }"))
                .is_err()
        );
        assert!(settings.merged_json(Some("[1, 2]")).is_err());
        assert!(
            settings
                .merged_json(Some(
                    "  
"
                ))
                .is_ok()
        );
        assert!(settings.merged_json(None).is_ok());
    }

    #[test]
    fn color_theme_round_trips() {
        assert!(Settings::default().color_theme.is_none());
        let s = Settings {
            color_theme: Some("Catppuccin Mocha".into()),
            ..Settings::default()
        };
        let loaded = Settings::parse(&s.to_json()).unwrap();
        assert_eq!(loaded.color_theme.as_deref(), Some("Catppuccin Mocha"));
    }

    #[test]
    fn ai_settings_round_trip() {
        let defaults = Settings::default();
        assert_eq!(defaults.ai_endpoint, "http://localhost:11434");
        assert!(defaults.ai_model.is_none(), "the assistant starts off");
        let s = Settings {
            ai_endpoint: "http://localhost:1234/v1".into(),
            ai_model: Some("qwen2.5-coder:7b".into()),
            ..Settings::default()
        };
        let loaded = Settings::parse(&s.to_json()).unwrap();
        assert_eq!(loaded.ai_endpoint, "http://localhost:1234/v1");
        assert_eq!(loaded.ai_model.as_deref(), Some("qwen2.5-coder:7b"));
    }

    #[test]
    fn markdown_remote_images_setting_round_trips() {
        assert!(!Settings::default().markdown_load_remote_images);
        let s = Settings {
            markdown_load_remote_images: true,
            ..Settings::default()
        };
        let loaded = Settings::parse(&s.to_json()).unwrap();
        assert!(loaded.markdown_load_remote_images);
    }

    #[test]
    fn terminal_profile_round_trips() {
        let s = Settings {
            default_terminal_profile: ShellKind::GitBash,
            ..Settings::default()
        };
        let loaded = Settings::parse(&s.to_json()).unwrap();
        assert_eq!(loaded.default_terminal_profile, ShellKind::GitBash);
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
        let s = Settings::parse(json).unwrap();
        assert_eq!(s.font_family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(s.font_size, 18);
        assert_eq!(s.tab_size, 2);
        assert!(!s.auto_close_pairs);
        assert!(s.auto_indent); // default preserved
        assert_eq!(s.colors.get("editorBg"), Some(&0x23150c));
    }

    #[test]
    fn malformed_json_is_reported() {
        let error = Settings::parse("not json {{{").err().unwrap();
        assert!(error.contains("invalid"));
        // A trailing comma is the usual hand-editing mistake.
        assert!(Settings::parse("{\"formatOnSave\": true,}").is_err());
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
        let loaded = Settings::parse(&json).unwrap();
        assert_eq!(loaded.font_family.as_deref(), Some("Fira Code"));
        assert_eq!(loaded.font_size, 20);
    }
}
