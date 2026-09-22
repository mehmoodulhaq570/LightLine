// A centralized, runtime-swappable color palette. This replaces what used
// to be ~17 top-level `const u32` values plus a handful of syntax colors
// inlined in code_pane.rs's Color match -- values that were already a
// coherent theme in practice (reused consistently across every render file),
// just not represented as one struct an adapter could replace at runtime.
//
// Colors are Win32 COLORREF (0x00BBGGRR), matching the existing `rgb()`
// helper -- this stays a windows_app type, not a library-crate one, since
// that byte order is a GDI detail, not portable data.
//
// This is only the foundation: one hardcoded default (today's exact
// palette, moved as-is so appearance is unchanged), swappable via
// App::set_theme(), with no theme-switching UI and no parser for any
// external format yet. A future Zed color-theme adapter populates a `Theme`
// value the same way icon themes already populate `IconTheme`.
pub(super) struct Theme {
    // Chrome: window backdrop, cards, panels.
    pub(super) shell_bg: u32,
    pub(super) card_edge: u32,
    pub(super) editor_bg: u32,
    pub(super) rail_bg: u32,
    pub(super) sidebar_bg: u32,
    pub(super) tab_bg: u32,
    pub(super) active_bg: u32,
    pub(super) status_bg: u32,
    pub(super) line_bg: u32,
    pub(super) select_bg: u32,
    pub(super) edge: u32,

    // Text.
    pub(super) text: u32,
    pub(super) muted: u32,

    // General accent colors, reused across chrome (git status dots, debug
    // toolbar, icons, ...) as well as syntax highlighting below.
    pub(super) blue: u32,
    pub(super) violet: u32,
    pub(super) teal: u32,
    pub(super) green: u32,

    // Syntax highlighting (see lightline::syntax::Color). Several of these
    // already reused one of the accents above before this struct existed;
    // that's preserved here, just named explicitly instead of accidentally.
    pub(super) comment: u32,
    pub(super) string: u32,
    pub(super) keyword: u32,
    pub(super) type_color: u32,
    pub(super) number: u32,
    pub(super) macro_color: u32,
    pub(super) function: u32,
    pub(super) operator: u32,
    pub(super) attribute: u32,
    // No distinct rendering exists for punctuation yet (it falls through as
    // plain `text`); this slot exists so a future theme can claim it
    // without another renderer change.
    pub(super) punctuation: u32,

    // Editor semantics that existed as *behavior* (reusing another color)
    // rather than a named slot -- broken out so a theme can genuinely
    // change them, while defaulting to exactly what they do today.
    pub(super) cursor: u32,
    pub(super) line_number: u32,
    pub(super) line_number_active: u32,
    pub(super) error: u32,
    pub(super) warning: u32,
    // Not distinguished from `warning` in any current render code (LSP
    // "information"/"hint" severities render the same as a warning today);
    // reserved for when that distinction is worth making.
    pub(super) info: u32,
}

impl Theme {
    // Every literal below is copied unchanged from the constants/inline
    // colors it replaces -- this function must not change LightLine's
    // default appearance.
    pub(super) fn default_dark() -> Self {
        use super::rgb;
        let text = rgb(226, 234, 248);
        let muted = rgb(136, 156, 188);
        let blue = rgb(56, 189, 248);
        let violet = rgb(147, 112, 255);
        let teal = rgb(45, 212, 191);
        let green = rgb(74, 222, 128);
        let warning = rgb(245, 184, 95);
        Self {
            shell_bg: rgb(8, 12, 22),
            card_edge: rgb(32, 48, 80),
            editor_bg: rgb(10, 16, 30),
            rail_bg: rgb(10, 15, 28),
            sidebar_bg: rgb(11, 18, 34),
            tab_bg: rgb(11, 18, 34),
            active_bg: rgb(20, 32, 60),
            status_bg: rgb(10, 16, 28),
            line_bg: rgb(18, 28, 50),
            select_bg: rgb(28, 48, 96),
            edge: rgb(28, 42, 70),
            text,
            muted,
            blue,
            violet,
            teal,
            green,
            comment: muted,
            string: green,
            keyword: blue,
            type_color: teal,
            number: rgb(248, 180, 130),
            macro_color: violet,
            function: rgb(220, 210, 130),
            operator: rgb(200, 200, 220),
            attribute: rgb(180, 140, 230),
            punctuation: text,
            cursor: blue,
            line_number: muted,
            line_number_active: text,
            error: rgb(246, 110, 120),
            warning,
            info: warning,
        }
    }

    // Applies settings.json's `colors` overrides (already loaded/parsed by
    // src/settings.rs, previously only consulted for the 4 syntax colors
    // that had a key; now the single place every themeable key is resolved).
    pub(super) fn with_overrides(mut self, overrides: &std::collections::HashMap<String, u32>) -> Self {
        let apply = |key: &str, field: &mut u32| {
            if let Some(&color) = overrides.get(key) {
                *field = color;
            }
        };
        apply("shellBg", &mut self.shell_bg);
        apply("cardEdge", &mut self.card_edge);
        apply("editorBg", &mut self.editor_bg);
        apply("railBg", &mut self.rail_bg);
        apply("sidebarBg", &mut self.sidebar_bg);
        apply("tabBg", &mut self.tab_bg);
        apply("activeBg", &mut self.active_bg);
        apply("statusBg", &mut self.status_bg);
        apply("lineBg", &mut self.line_bg);
        apply("selectBg", &mut self.select_bg);
        apply("edge", &mut self.edge);
        apply("text", &mut self.text);
        apply("muted", &mut self.muted);
        apply("blue", &mut self.blue);
        apply("violet", &mut self.violet);
        apply("teal", &mut self.teal);
        apply("green", &mut self.green);
        apply("comment", &mut self.comment);
        apply("string", &mut self.string);
        apply("keyword", &mut self.keyword);
        apply("type", &mut self.type_color);
        apply("number", &mut self.number);
        apply("macro", &mut self.macro_color);
        apply("function", &mut self.function);
        apply("operator", &mut self.operator);
        apply("attribute", &mut self.attribute);
        apply("punctuation", &mut self.punctuation);
        apply("cursor", &mut self.cursor);
        apply("lineNumber", &mut self.line_number);
        apply("lineNumberActive", &mut self.line_number_active);
        apply("error", &mut self.error);
        apply("warning", &mut self.warning);
        apply("info", &mut self.info);
        self
    }
}
