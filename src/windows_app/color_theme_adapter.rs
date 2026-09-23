// Maps a parsed Zed color theme (lightline::color_theme::ZedColorTheme) onto
// LightLine's own Theme. This is where Zed's color names meet LightLine's --
// the parser (src/color_theme.rs) knows nothing about Theme, and Theme knows
// nothing about Zed; only this file does the translation, the same split
// icon_theme.rs (parsing) / icons.rs (GDI rendering) already established.
//
// Scope, matching the task: only the core editor/chrome/syntax fields that
// map cleanly onto a single Zed key are touched. LightLine's four general
// accent fields (blue/violet/teal/green) and macro_color have no single
// clean Zed equivalent -- they're reused for several unrelated purposes
// each (git markers, debug toolbar, ...) -- so a Zed theme never touches
// them; they keep `base`'s value, per "preserve existing defaults when a
// Zed theme does not provide an equivalent color."

use super::theme::Theme;
use lightline::color_theme::{Rgba, ZedColorTheme};

// Alpha-composites `color` over `backdrop` (both already-resolved LightLine
// COLORREFs). Many of Zed's own chrome colors are intentionally translucent
// (e.g. an active-line highlight at ~20% opacity, meant to tint whatever's
// beneath it) -- LightLine's renderer has no alpha blending, so an opaque
// fill needs the blend done once, here, instead of showing the raw
// (much too vivid) foreground color at full strength.
fn composite(color: Rgba, backdrop: u32) -> u32 {
    let a = u32::from(color.a);
    let br = backdrop & 0xff;
    let bg = (backdrop >> 8) & 0xff;
    let bb = (backdrop >> 16) & 0xff;
    let blend = |fg: u8, bd: u32| -> u32 { (u32::from(fg) * a + bd * (255 - a)) / 255 };
    blend(color.r, br) | (blend(color.g, bg) << 8) | (blend(color.b, bb) << 16)
}

impl Theme {
    // Builds a new Theme from `base` (typically the current theme, so an
    // unmapped field -- and there are several by design, see above --
    // simply keeps whatever it already held) plus whatever `zed` provides
    // for the fields this adapter knows how to translate.
    pub(super) fn from_zed_color_theme(base: &Theme, zed: &ZedColorTheme) -> Theme {
        let mut theme = Theme {
            shell_bg: base.shell_bg,
            card_edge: base.card_edge,
            editor_bg: base.editor_bg,
            rail_bg: base.rail_bg,
            sidebar_bg: base.sidebar_bg,
            tab_bg: base.tab_bg,
            active_bg: base.active_bg,
            status_bg: base.status_bg,
            line_bg: base.line_bg,
            select_bg: base.select_bg,
            edge: base.edge,
            text: base.text,
            muted: base.muted,
            blue: base.blue,
            violet: base.violet,
            teal: base.teal,
            green: base.green,
            comment: base.comment,
            string: base.string,
            keyword: base.keyword,
            type_color: base.type_color,
            number: base.number,
            macro_color: base.macro_color,
            function: base.function,
            operator: base.operator,
            attribute: base.attribute,
            punctuation: base.punctuation,
            cursor: base.cursor,
            line_number: base.line_number,
            line_number_active: base.line_number_active,
            error: base.error,
            warning: base.warning,
            info: base.info,
        };

        // Editor background first: every translucent chrome color below
        // composites against it, so it must be resolved before them.
        if let Some(color) = zed.style_color("editor.background") {
            theme.editor_bg = composite(color, theme.editor_bg);
        }
        let backdrop = theme.editor_bg;

        let apply_style = |key: &str, field: &mut u32| {
            if let Some(color) = zed.style_color(key) {
                *field = composite(color, backdrop);
            }
        };
        apply_style("text", &mut theme.text);
        apply_style("text.muted", &mut theme.muted);
        apply_style("border", &mut theme.edge);
        apply_style("border.variant", &mut theme.card_edge);
        apply_style("status_bar.background", &mut theme.status_bg);
        apply_style("tab.inactive_background", &mut theme.tab_bg);
        apply_style("element.hover", &mut theme.active_bg);
        apply_style("panel.background", &mut theme.sidebar_bg);
        apply_style("panel.background", &mut theme.rail_bg);
        apply_style("elevated_surface.background", &mut theme.shell_bg);
        apply_style("editor.active_line.background", &mut theme.line_bg);
        apply_style("editor.line_number", &mut theme.line_number);
        apply_style("editor.active_line_number", &mut theme.line_number_active);
        apply_style("players.0.cursor", &mut theme.cursor);
        apply_style("players.0.selection", &mut theme.select_bg);
        apply_style("error", &mut theme.error);
        apply_style("warning", &mut theme.warning);
        apply_style("info", &mut theme.info);

        let apply_syntax = |key: &str, field: &mut u32| {
            if let Some(color) = zed.syntax_color(key) {
                *field = composite(color, backdrop);
            }
        };
        apply_syntax("comment", &mut theme.comment);
        apply_syntax("string", &mut theme.string);
        apply_syntax("keyword", &mut theme.keyword);
        apply_syntax("type", &mut theme.type_color);
        apply_syntax("number", &mut theme.number);
        apply_syntax("function", &mut theme.function);
        apply_syntax("operator", &mut theme.operator);
        apply_syntax("attribute", &mut theme.attribute);
        apply_syntax("punctuation", &mut theme.punctuation);

        theme
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_color_ignores_backdrop() {
        let color = Rgba { r: 0x28, g: 0x2a, b: 0x36, a: 0xff };
        assert_eq!(composite(color, 0x00ffffff), 0x00362a28); // COLORREF: r|g<<8|b<<16
    }

    #[test]
    fn fully_transparent_color_keeps_backdrop_unchanged() {
        let color = Rgba { r: 0xff, g: 0, b: 0, a: 0x00 };
        let backdrop = 0x00362a28;
        assert_eq!(composite(color, backdrop), backdrop);
    }

    #[test]
    fn real_dracula_theme_maps_onto_a_theme_without_touching_unmapped_accents() {
        let Some(dir) = std::env::var_os("APPDATA").map(|appdata| {
            std::path::PathBuf::from(appdata).join("LightLine").join("extensions").join("dracula")
        }) else {
            return;
        };
        let Some(zed) = lightline::color_theme::ZedColorTheme::load(&dir) else {
            eprintln!("skipped: real dracula extension not installed");
            return;
        };
        let base = Theme::default_dark();
        let theme = Theme::from_zed_color_theme(&base, &zed);
        // Editor background should have genuinely changed to Dracula's.
        assert_ne!(theme.editor_bg, base.editor_bg);
        // Fields this adapter intentionally never touches must be preserved.
        assert_eq!(theme.blue, base.blue);
        assert_eq!(theme.violet, base.violet);
        assert_eq!(theme.teal, base.teal);
        assert_eq!(theme.green, base.green);
        assert_eq!(theme.macro_color, base.macro_color);
    }
}
