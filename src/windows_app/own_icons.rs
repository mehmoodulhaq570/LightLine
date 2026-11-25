// Icons for the extensions LightLine publishes itself
// (design/own-extension-icons-design.html): the LightLine mark's indigo
// tile, its violet-to-blue edge and its bolt, with a motif for each kind.
// Each is a whole 36 x 36 tile, drawn through the icon atlas at the size
// shown, before and after the extension is installed.

use super::app::{Extension, ExtensionCategory};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum OwnIcon {
    /// Split by the bolt: a Daylight half and a Contrast half.
    ColorTheme,
    /// Braces around the bolt.
    Snippets,
    IconTheme,
    Language,
    Checker,
}

// Shared by every tile: its fill, its edge and the bolt's colors.
const DEFS: &str = r##"<linearGradient id="tile" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#2c1ba6"/><stop offset="1" stop-color="#0a136b"/></linearGradient><linearGradient id="edge" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#c084fc"/><stop offset="1" stop-color="#4f8dff"/></linearGradient><linearGradient id="bolt" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fbe2ff"/><stop offset=".55" stop-color="#c9b8ff"/><stop offset="1" stop-color="#7fd4ff"/></linearGradient><linearGradient id="neon" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#e3c4ff"/><stop offset="1" stop-color="#8cc8ff"/></linearGradient><clipPath id="clip"><rect x="1" y="1" width="34" height="34" rx="8"/></clipPath>"##;
const TILE: &str = r#"<rect x="1" y="1" width="34" height="34" rx="8" fill="url(#tile)"/>"#;
const EDGE: &str = r#"<rect x="1" y="1" width="34" height="34" rx="8" fill="none" stroke="url(#edge)" stroke-width="1.6"/>"#;

const COLOR_THEME: &str = r##"<g clip-path="url(#clip)"><rect width="36" height="36" fill="#000"/><path d="M0 0H22L13 36H0Z" fill="#f3f6fb"/><rect x="4.5" y="8" width="9" height="2.2" rx="1.1" fill="#1e293b"/><rect x="4.5" y="12.5" width="6" height="2.2" rx="1.1" fill="#0a7fc2"/><rect x="4.5" y="17" width="4" height="2.2" rx="1.1" fill="#7c3aed"/><rect x="23" y="21.5" width="8.5" height="2.2" rx="1.1" fill="#4cc9ff"/><rect x="23" y="26" width="5.5" height="2.2" rx="1.1" fill="#ffffff"/></g><path d="M21 4L10 19.2H17L14 32L26 15.8H19Z" fill="none" stroke="#a855f7" stroke-opacity=".55" stroke-width="3.4" stroke-linejoin="round"/><path d="M21 4L10 19.2H17L14 32L26 15.8H19Z" fill="url(#bolt)" stroke="#24127e" stroke-width="1.2" stroke-linejoin="round"/>"##;

const SNIPPETS: &str = r##"<g fill="none" stroke="url(#neon)" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12.5 8.5C9.8 8.5 10 10.5 10 13V15.4C10 17 9 18 7.6 18C9 18 10 19 10 20.6V23C10 25.5 9.8 27.5 12.5 27.5"/><path d="M23.5 8.5C26.2 8.5 26 10.5 26 13V15.4C26 17 27 18 28.4 18C27 18 26 19 26 20.6V23C26 25.5 26.2 27.5 23.5 27.5"/></g><path d="M19.8 10.5L14.2 18.9H17.7L16.3 25.6L21.9 17H18.4Z" fill="none" stroke="#a855f7" stroke-opacity=".6" stroke-width="3" stroke-linejoin="round"/><path d="M19.8 10.5L14.2 18.9H17.7L16.3 25.6L21.9 17H18.4Z" fill="url(#bolt)"/>"##;

const ICON_THEME: &str = r##"<path d="M7 12.5C7 11.1 8.1 10 9.5 10H14.4L16.8 12.6H26.5C27.9 12.6 29 13.7 29 15.1V24.5C29 25.9 27.9 27 26.5 27H9.5C8.1 27 7 25.9 7 24.5Z" fill="none" stroke="url(#neon)" stroke-width="2.2" stroke-linejoin="round"/><path d="M19.4 14.6L15.4 20.6H17.9L16.9 25L20.9 19H18.4Z" fill="url(#bolt)"/>"##;

const LANGUAGE: &str = r##"<g fill="none" stroke="url(#neon)" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12 11.5L6.5 18L12 24.5"/><path d="M24 11.5L29.5 18L24 24.5"/></g><path d="M20.3 9.5L14.4 19H18.1L16.4 27L22.4 17.2H18.6Z" fill="url(#bolt)"/>"##;

const CHECKER: &str = r##"<path d="M8.5 18.5L14.5 24.5L27.5 11.5" fill="none" stroke="url(#neon)" stroke-width="2.8" stroke-linecap="round" stroke-linejoin="round"/><path d="M28.6 18.5L25 24H27.3L26.4 28L30 22.6H27.7Z" fill="url(#bolt)"/>"##;

impl OwnIcon {
    #[cfg(test)]
    pub(super) const ALL: [OwnIcon; 5] = [
        OwnIcon::ColorTheme,
        OwnIcon::Snippets,
        OwnIcon::IconTheme,
        OwnIcon::Language,
        OwnIcon::Checker,
    ];

    /// The icon for an extension LightLine publishes, by its kind: one on
    /// LightLine's list with LightLine as its publisher. None for any other,
    /// so a registry extension can't take the mark by its name.
    pub(super) fn for_extension(ext: &Extension) -> Option<OwnIcon> {
        if ext.listed.is_none() || ext.publisher != "LightLine" {
            return None;
        }
        match ext.category() {
            ExtensionCategory::ColorTheme => Some(OwnIcon::ColorTheme),
            ExtensionCategory::Snippets => Some(OwnIcon::Snippets),
            ExtensionCategory::IconTheme => Some(OwnIcon::IconTheme),
            ExtensionCategory::Language => Some(OwnIcon::Language),
            ExtensionCategory::Tool => Some(OwnIcon::Checker),
            ExtensionCategory::Formatter | ExtensionCategory::Unknown => None,
        }
    }

    /// The whole tile as an SVG document.
    pub(super) fn svg(self) -> String {
        // The theme tile has no indigo: its halves fill the tile.
        let (tile, motif) = match self {
            OwnIcon::ColorTheme => ("", COLOR_THEME),
            OwnIcon::Snippets => (TILE, SNIPPETS),
            OwnIcon::IconTheme => (TILE, ICON_THEME),
            OwnIcon::Language => (TILE, LANGUAGE),
            OwnIcon::Checker => (TILE, CHECKER),
        };
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="36" height="36" viewBox="0 0 36 36"><defs>{DEFS}</defs>{tile}{motif}{EDGE}</svg>"#
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::app::{Extension, ExtensionCategory};
    use super::OwnIcon;

    #[test]
    fn only_extensions_lightline_publishes_get_the_mark() {
        let own = |kind| Extension {
            id: "lightline-themes".into(),
            publisher: "LightLine".into(),
            listed: Some(0),
            kind: Some(kind),
            ..Extension::default()
        };
        assert_eq!(
            OwnIcon::for_extension(&own(ExtensionCategory::ColorTheme)),
            Some(OwnIcon::ColorTheme)
        );
        assert_eq!(
            OwnIcon::for_extension(&own(ExtensionCategory::Snippets)),
            Some(OwnIcon::Snippets)
        );
        assert_eq!(
            OwnIcon::for_extension(&own(ExtensionCategory::Tool)),
            Some(OwnIcon::Checker)
        );
        // A registry extension that calls itself lightline-something, or one
        // a later list no longer has, keeps the usual icon.
        let unlisted = Extension {
            listed: None,
            ..own(ExtensionCategory::ColorTheme)
        };
        assert_eq!(OwnIcon::for_extension(&unlisted), None);
        let someone_else = Extension {
            publisher: "someone".into(),
            ..own(ExtensionCategory::ColorTheme)
        };
        assert_eq!(OwnIcon::for_extension(&someone_else), None);
    }
}
