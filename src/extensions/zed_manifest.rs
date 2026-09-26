//! Parses a downloaded Zed extension's own `extension.toml` -- verified
//! against the real one at
//! <https://github.com/zed-extensions/material-icon-theme/blob/main/extension.toml>:
//! `id`, `name`, `version`, `schema_version`, `authors`, `description`,
//! `repository`. There is no `type` field: Zed extensions declare their
//! capabilities by which directories/files are present, not by a manifest
//! flag, so classification here is done the same way (see `is_icon_theme`).

use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize, Debug, Clone)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
}

impl Manifest {
    pub fn load(extension_dir: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(extension_dir.join("extension.toml")).ok()?;
        toml::from_str(&text).ok()
    }
}

/// A pure-data icon-theme extension has an `icon_themes/` directory of JSON
/// theme files -- no compiled code, matching Zed's own documented structure.
/// See `lightline::icon_theme` for the loader.
pub fn is_icon_theme(extension_dir: &Path) -> bool {
    extension_dir.join("icon_themes").is_dir()
}

/// A pure-data color-theme extension has a `themes/` directory of JSON
/// theme files (verified against the real Dracula extension,
/// github.com/dracula/zed) -- no compiled code, same shape of extension as
/// an icon theme. See `lightline::color_theme` for the loader.
pub fn is_color_theme(extension_dir: &Path) -> bool {
    extension_dir.join("themes").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_real_looking_manifest() {
        let toml = r#"
            id = "material-icon-theme"
            name = "Material Icon Theme"
            version = "1.3.1"
            schema_version = 1
            authors = ["Zed Industries <support@zed.dev>"]
            description = "Material Design icons."
            repository = "https://github.com/zed-extensions/material-icon-theme"
        "#;
        let manifest: Manifest = toml::from_str(toml).unwrap();
        assert_eq!(manifest.id, "material-icon-theme");
        assert_eq!(manifest.version, "1.3.1");
        assert_eq!(
            manifest.repository.as_deref(),
            Some("https://github.com/zed-extensions/material-icon-theme")
        );
    }

    #[test]
    fn real_installed_extension_is_classified_as_an_icon_theme() {
        let Some(dir) = std::env::var_os("APPDATA").map(|appdata| {
            std::path::PathBuf::from(appdata)
                .join("LightLine")
                .join("extensions")
                .join("material-icon-theme")
        }) else {
            return;
        };
        if !dir.is_dir() {
            eprintln!("skipped: real extension not installed at {dir:?}");
            return;
        }
        assert!(is_icon_theme(&dir));
        let manifest = Manifest::load(&dir).expect("extension.toml should parse");
        assert_eq!(manifest.id, "material-icon-theme");
    }
}
