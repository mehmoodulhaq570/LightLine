// The Settings view in the side panel, opened by the gear at the bottom of
// the rail (Ctrl+,). The gear used to only say "Settings are not available
// yet", leaving settings.json as the only way to change anything.
//
// It shows only settings that take effect. Each change applies at once and
// is saved to settings.json, keeping whatever else the file holds. A field
// under the title filters the rows; a violet bar marks a value that isn't
// the default, and hovering such a row offers to reset it. Painting, clicks
// and hover share `settings_rows`, in the side panel's coordinates (y from
// the top of the workbench, as the side panel is painted).

use super::render::blend;
use super::*;
use lightline::settings::Settings;
use lightline::terminal::ShellKind;
use std::sync::OnceLock;

// The search field under the title, then the rows below it.
const FIELD_TOP: i32 = 46;
const FIELD_HEIGHT: i32 = 26;
const LIST_TOP: i32 = 78;
const ROW: i32 = 30;
const HEADING: i32 = 30;
const JSON_ROW: i32 = 40;
// Controls at the right of a row.
const SWITCH_WIDTH: i32 = 28;
const SWITCH_HEIGHT: i32 = 16;
const STEP_PART: i32 = 22;
const STEP_VALUE: i32 = 30;
const CHOICE_WIDTH: i32 = 134;
const SHORT_CHOICE_WIDTH: i32 = 96;
const CONTROL_HEIGHT: i32 = 22;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Setting {
    FontSize,
    TabSize,
    InsertSpaces,
    WordWrap,
    AutoIndent,
    AutoClosePairs,
    BracketMatching,
    IndentGuides,
    BracketColors,
    Minimap,
    GitBlame,
    CodeLens,
    FormatOnSave,
    ParseLimit,
    ColorTheme,
    Animations,
    SmoothScrolling,
    TerminalProfile,
    MarkdownImages,
    AiEndpoint,
    AiModel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Heading(&'static str),
    Toggle(Setting),
    Stepper(Setting),
    // A value that opens a menu or moves to the next choice.
    Choice(Setting),
    // A value shown but changed elsewhere (the AI panel, settings.json).
    Info(Setting),
    OpenJson,
    // Shown in place of the rows when the search matches none.
    NoMatch,
}

/// What the mouse is over in the Settings view (hot.rs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SettingsHot {
    // The { } button in the header.
    Json,
    // A setting's row, by its index in `ROWS`.
    Row(usize),
}

const ROWS: &[Row] = &[
    Row::Heading("EDITOR"),
    Row::Stepper(Setting::FontSize),
    Row::Stepper(Setting::TabSize),
    Row::Toggle(Setting::InsertSpaces),
    Row::Toggle(Setting::WordWrap),
    Row::Toggle(Setting::AutoIndent),
    Row::Toggle(Setting::AutoClosePairs),
    Row::Heading("VISUAL AIDS"),
    Row::Toggle(Setting::BracketMatching),
    Row::Toggle(Setting::BracketColors),
    Row::Toggle(Setting::IndentGuides),
    Row::Toggle(Setting::Minimap),
    Row::Toggle(Setting::GitBlame),
    Row::Toggle(Setting::CodeLens),
    Row::Heading("FILES"),
    Row::Toggle(Setting::FormatOnSave),
    Row::Choice(Setting::ParseLimit),
    Row::Heading("APPEARANCE"),
    Row::Choice(Setting::ColorTheme),
    Row::Toggle(Setting::Animations),
    Row::Toggle(Setting::SmoothScrolling),
    Row::Heading("TERMINAL"),
    Row::Choice(Setting::TerminalProfile),
    Row::Heading("MARKDOWN"),
    Row::Toggle(Setting::MarkdownImages),
    Row::Heading("AI ASSISTANT"),
    Row::Choice(Setting::AiEndpoint),
    Row::Info(Setting::AiModel),
    Row::OpenJson,
];

const SHELLS: [ShellKind; 4] = [
    ShellKind::PowerShell,
    ShellKind::CommandPrompt,
    ShellKind::GitBash,
    ShellKind::Wsl,
];

fn label(setting: Setting) -> &'static str {
    match setting {
        Setting::FontSize => "Font size",
        Setting::TabSize => "Tab size",
        Setting::InsertSpaces => "Indent with spaces",
        Setting::WordWrap => "Word wrap",
        Setting::AutoIndent => "Auto indent",
        Setting::AutoClosePairs => "Close brackets and quotes",
        Setting::BracketMatching => "Highlight matching brackets",
        Setting::IndentGuides => "Indent guides",
        Setting::BracketColors => "Color bracket pairs",
        Setting::Minimap => "Minimap",
        Setting::GitBlame => "Git blame on the current line",
        Setting::CodeLens => "Run and Debug links",
        Setting::FormatOnSave => "Format on save",
        Setting::ParseLimit => "Syntax colors up to",
        Setting::ColorTheme => "Color theme",
        Setting::Animations => "Animations",
        Setting::SmoothScrolling => "Smooth scrolling",
        Setting::TerminalProfile => "Default shell",
        Setting::MarkdownImages => "Load web images in previews",
        Setting::AiEndpoint => "Server",
        Setting::AiModel => "Model",
    }
}

/// The setting's key in settings.json; the search matches it too.
fn json_key(setting: Setting) -> &'static str {
    match setting {
        Setting::FontSize => "fontSize",
        Setting::TabSize => "tabSize",
        Setting::InsertSpaces => "insertSpaces",
        Setting::WordWrap => "wordWrap",
        Setting::AutoIndent => "autoIndent",
        Setting::AutoClosePairs => "autoClosePairs",
        Setting::BracketMatching => "bracketMatching",
        Setting::IndentGuides => "indentGuides",
        Setting::BracketColors => "bracketPairColorization",
        Setting::Minimap => "showMinimap",
        Setting::GitBlame => "gitBlame",
        Setting::CodeLens => "codeLens",
        Setting::FormatOnSave => "formatOnSave",
        Setting::ParseLimit => "parseLimitKb",
        Setting::ColorTheme => "colorTheme",
        Setting::Animations => "animations",
        Setting::SmoothScrolling => "smoothScrolling",
        Setting::TerminalProfile => "terminalDefaultProfile",
        Setting::MarkdownImages => "markdownLoadRemoteImages",
        Setting::AiEndpoint => "aiEndpoint",
        Setting::AiModel => "aiModel",
    }
}

/// The hover tip: what the setting does, and its key in settings.json.
fn tip(setting: Setting) -> &'static str {
    match setting {
        Setting::FontSize => "The editor's text size  \u{b7}  fontSize",
        Setting::TabSize => "Columns per indent level  \u{b7}  tabSize",
        Setting::InsertSpaces => "Tab inserts spaces, not a tab character  \u{b7}  insertSpaces",
        Setting::WordWrap => "Wrap long lines at the edge (Alt+Z)  \u{b7}  wordWrap",
        Setting::AutoIndent => "A new line keeps the indent above  \u{b7}  autoIndent",
        Setting::AutoClosePairs => "Typing ( [ { or a quote adds its pair  \u{b7}  autoClosePairs",
        Setting::BracketMatching => {
            "Mark the bracket that pairs with the caret's  \u{b7}  bracketMatching"
        }
        Setting::IndentGuides => "Thin lines at each indent level  \u{b7}  indentGuides",
        Setting::BracketColors => "Color nested brackets by depth  \u{b7}  bracketPairColorization",
        Setting::Minimap => "A small map of the file at the right  \u{b7}  showMinimap",
        Setting::GitBlame => "Who last changed the caret's line  \u{b7}  gitBlame",
        Setting::CodeLens => "Run, Debug and Run test after main and tests  \u{b7}  codeLens",
        Setting::FormatOnSave => "Format the file after each save  \u{b7}  formatOnSave",
        Setting::ParseLimit => "Larger files open without syntax colors  \u{b7}  parseLimitKb",
        Setting::ColorTheme => "The colors of LightLine  \u{b7}  colorTheme",
        Setting::Animations => "Panels slide and hovers fade  \u{b7}  animations",
        Setting::SmoothScrolling => "The mouse wheel glides  \u{b7}  smoothScrolling",
        Setting::TerminalProfile => "The shell new terminals start  \u{b7}  terminalDefaultProfile",
        Setting::MarkdownImages => {
            "Show images from the web in Markdown previews  \u{b7}  markdownLoadRemoteImages"
        }
        Setting::AiEndpoint => "Where the AI Assistant sends questions  \u{b7}  aiEndpoint",
        Setting::AiModel => "Chosen in the AI Assistant panel  \u{b7}  aiModel",
    }
}

fn row_setting(row: Row) -> Option<Setting> {
    match row {
        Row::Toggle(setting)
        | Row::Stepper(setting)
        | Row::Choice(setting)
        | Row::Info(setting) => Some(setting),
        _ => None,
    }
}

fn row_height(row: Row) -> i32 {
    match row {
        Row::Heading(_) => HEADING,
        Row::OpenJson => JSON_ROW,
        _ => ROW,
    }
}

fn contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

/// The default settings, which the "changed" bar compares with.
fn defaults() -> &'static Settings {
    static DEFAULTS: OnceLock<Settings> = OnceLock::new();
    DEFAULTS.get_or_init(Settings::default)
}

fn setting_on_in(settings: &Settings, setting: Setting) -> bool {
    match setting {
        Setting::InsertSpaces => settings.insert_spaces,
        Setting::WordWrap => settings.word_wrap,
        Setting::AutoIndent => settings.auto_indent,
        Setting::AutoClosePairs => settings.auto_close_pairs,
        Setting::BracketMatching => settings.bracket_matching,
        Setting::IndentGuides => settings.indent_guides,
        Setting::BracketColors => settings.bracket_pair_colors,
        Setting::Minimap => settings.minimap,
        Setting::GitBlame => settings.git_blame,
        Setting::CodeLens => settings.code_lens,
        Setting::FormatOnSave => settings.format_on_save,
        Setting::MarkdownImages => settings.markdown_load_remote_images,
        Setting::Animations => settings.animations,
        Setting::SmoothScrolling => settings.smooth_scrolling,
        _ => false,
    }
}

/// The rows `query` (lowercase) keeps, each with its index in `ROWS`: a
/// heading stays when a row under it matches, and the settings.json link
/// always does.
fn shown_rows(query: &str) -> Vec<(usize, Row)> {
    if query.is_empty() {
        return ROWS.iter().copied().enumerate().collect();
    }
    let mut shown = Vec::new();
    let mut heading = None;
    let mut matched = false;
    for (index, &row) in ROWS.iter().enumerate() {
        if let Row::Heading(_) = row {
            heading = Some((index, row));
        } else if let Some(setting) = row_setting(row)
            && (label(setting).to_ascii_lowercase().contains(query)
                || json_key(setting).to_ascii_lowercase().contains(query))
        {
            shown.extend(heading.take());
            shown.push((index, row));
            matched = true;
        }
    }
    if !matched {
        shown.push((usize::MAX, Row::NoMatch));
    }
    shown.push((ROWS.len() - 1, Row::OpenJson));
    shown
}

fn setting_count() -> usize {
    ROWS.iter()
        .filter(|row| row_setting(**row).is_some())
        .count()
}

/// A row on screen: its index in `ROWS` (usize::MAX for NoMatch).
struct Placed {
    index: usize,
    row: Row,
    rect: RECT,
}

impl App {
    /// Shows the Settings view (Ctrl+,); unlike the gear, it never closes it.
    pub(super) fn open_settings_panel(&mut self, hwnd: HWND) {
        if self.side_view != SideView::Settings || !self.explorer_visible {
            self.toggle_side_view(hwnd, SideView::Settings);
        }
    }

    /// Whether typing goes to the Settings search field.
    pub(super) fn settings_typing(&self) -> bool {
        self.settings_search_active
            && self.side_view == SideView::Settings
            && self.explorer_visible
            && !self.quick_open
    }

    fn setting_on(&self, setting: Setting) -> bool {
        setting_on_in(&self.settings, setting)
    }

    fn setting_value(&self, setting: Setting) -> String {
        let settings = &self.settings;
        match setting {
            Setting::FontSize => settings.font_size.to_string(),
            Setting::TabSize => settings.tab_size.to_string(),
            Setting::ParseLimit => {
                let kb = settings.parse_limit_kb;
                if kb >= 1024 {
                    format!("{} MB", kb / 1024)
                } else {
                    format!("{kb} KB")
                }
            }
            Setting::ColorTheme => settings
                .color_theme
                .clone()
                .unwrap_or_else(|| "LightLine".into()),
            Setting::TerminalProfile => settings.default_terminal_profile.name().into(),
            // The service's name when it's a listed one, else its address.
            Setting::AiEndpoint => {
                let provider = ai_provider::provider_for(&settings.ai_endpoint);
                if provider + 1 < ai_provider::PROVIDERS.len() {
                    ai_provider::PROVIDERS[provider].name.into()
                } else {
                    settings.ai_endpoint.clone()
                }
            }
            Setting::AiModel => settings
                .ai_model
                .clone()
                .unwrap_or_else(|| "None yet".into()),
            _ => String::new(),
        }
    }

    /// Whether `setting` differs from its default, so the row shows the bar
    /// and can be reset. The AI rows are set up in the AI Assistant panel.
    fn setting_changed(&self, setting: Setting) -> bool {
        let (now, default) = (&self.settings, defaults());
        match setting {
            Setting::FontSize => now.font_size != default.font_size,
            Setting::TabSize => now.tab_size != default.tab_size,
            Setting::ParseLimit => now.parse_limit_kb != default.parse_limit_kb,
            Setting::ColorTheme => now.color_theme != default.color_theme,
            Setting::TerminalProfile => {
                now.default_terminal_profile != default.default_terminal_profile
            }
            Setting::AiEndpoint | Setting::AiModel => false,
            toggle => setting_on_in(now, toggle) != setting_on_in(default, toggle),
        }
    }

    fn reset_setting(&mut self, hwnd: HWND, setting: Setting) {
        if !self.setting_changed(setting) {
            return;
        }
        let default = defaults();
        match setting {
            Setting::FontSize => {
                self.step_setting(hwnd, setting, default.font_size - self.settings.font_size)
            }
            Setting::TabSize => self.step_setting(
                hwnd,
                setting,
                default.tab_size as i32 - self.settings.tab_size as i32,
            ),
            Setting::ParseLimit => {
                self.settings.parse_limit_kb = default.parse_limit_kb;
                lightline::syntax::set_parse_limit_kb(default.parse_limit_kb);
                self.save_settings_change(hwnd);
            }
            Setting::ColorTheme => {
                self.set_color_theme(hwnd, None);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            Setting::TerminalProfile => {
                self.set_default_terminal_profile(hwnd, default.default_terminal_profile)
            }
            Setting::AiEndpoint | Setting::AiModel => {}
            toggle => self.toggle_setting(hwnd, toggle),
        }
    }

    fn settings_search(&self) -> String {
        self.settings_query.trim().to_ascii_lowercase()
    }

    fn settings_field(&self, left: i32, right: i32) -> RECT {
        let s = |value: i32| self.scale(value);
        RECT {
            left: left + s(12),
            top: s(FIELD_TOP),
            right: right - s(12),
            bottom: s(FIELD_TOP + FIELD_HEIGHT),
        }
    }

    // The { } button in the header, where other views keep theirs.
    fn settings_json_button(&self, right: i32) -> RECT {
        let s = |value: i32| self.scale(value);
        RECT {
            left: right - s(36),
            top: s(SIDE_SHIFT),
            right: right - s(14),
            bottom: s(40),
        }
    }

    // Each shown row with its rect, scrolled, from `left` to `right`; and
    // the bottom of the last row before scrolling.
    fn settings_rows(&self, left: i32, right: i32) -> (Vec<Placed>, i32) {
        let s = |value: i32| self.scale(value);
        let mut top = s(LIST_TOP);
        let rows = shown_rows(&self.settings_search())
            .into_iter()
            .map(|(index, row)| {
                let rect = RECT {
                    left,
                    top: top - self.settings_scroll,
                    right,
                    bottom: top + s(row_height(row)) - self.settings_scroll,
                };
                top += s(row_height(row));
                Placed { index, row, rect }
            })
            .collect();
        (rows, top)
    }

    // The control at the right of a toggle, stepper or choice row.
    fn setting_control(&self, row: Row, rect: RECT) -> Option<RECT> {
        let s = |value: i32| self.scale(value);
        let (width, height) = match row {
            Row::Toggle(_) => (s(SWITCH_WIDTH), s(SWITCH_HEIGHT)),
            Row::Stepper(_) => (s(STEP_PART * 2 + STEP_VALUE), s(CONTROL_HEIGHT)),
            // The size limit's values are short; its long name needs the room.
            Row::Choice(Setting::ParseLimit) => (s(SHORT_CHOICE_WIDTH), s(CONTROL_HEIGHT)),
            Row::Choice(_) => (
                s(CHOICE_WIDTH).min((rect.right - rect.left) / 2),
                s(CONTROL_HEIGHT),
            ),
            _ => return None,
        };
        let right = rect.right - s(12);
        let top = (rect.top + rect.bottom - height) / 2;
        Some(RECT {
            left: right - width,
            top,
            right,
            bottom: top + height,
        })
    }

    // The − and + ends of a stepper.
    fn stepper_buttons(&self, control: RECT) -> (RECT, RECT) {
        let part = self.scale(STEP_PART);
        (
            RECT {
                right: control.left + part,
                ..control
            },
            RECT {
                left: control.right - part,
                ..control
            },
        )
    }

    // The reset arrow left of a changed row's control.
    fn reset_button(&self, control: RECT) -> RECT {
        let s = |value: i32| self.scale(value);
        let middle = (control.top + control.bottom) / 2;
        RECT {
            left: control.left - s(28),
            top: middle - s(10),
            right: control.left - s(8),
            bottom: middle + s(10),
        }
    }

    // The settings.json link in its row (below a gap).
    fn settings_json_link(&self, row: RECT) -> RECT {
        RECT {
            top: row.top + self.scale(JSON_ROW - ROW),
            ..row
        }
    }

    fn invalidate_settings_panel(&self, hwnd: HWND) {
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let panel = RECT {
            left: self.scale(RAIL),
            top: self.side_origin(),
            right: self.sidebar_right(),
            bottom: client.bottom - self.scale(STATUS),
        };
        unsafe { InvalidateRect(hwnd, &panel, 0) };
    }

    /// The search field's rect in the window, for the caret blink.
    pub(super) fn settings_field_in_window(&self) -> RECT {
        let field = self.settings_field(self.scale(RAIL), self.sidebar_right());
        let origin = self.side_origin();
        RECT {
            top: field.top + origin,
            bottom: field.bottom + origin,
            ..field
        }
    }

    /// The query changed: back to the top of the list.
    pub(super) fn settings_query_changed(&mut self, hwnd: HWND) {
        self.settings_scroll = 0;
        self.invalidate_settings_panel(hwnd);
    }

    /// A key while the search field has the keyboard; false lets it through.
    pub(super) fn settings_search_key(&mut self, hwnd: HWND, key: u32, ctrl: bool) -> bool {
        match key {
            x if x == VK_ESCAPE as u32 => {
                if self.settings_query.is_empty() {
                    self.settings_search_active = false;
                } else {
                    self.settings_query.clear();
                }
            }
            x if x == VK_RETURN as u32 => self.settings_search_active = false,
            x if x == VK_BACK as u32 => {
                if ctrl {
                    self.settings_query.clear();
                } else {
                    self.settings_query.pop();
                }
            }
            _ => return false,
        }
        self.settings_query_changed(hwnd);
        true
    }

    pub(super) fn scroll_settings_panel(&mut self, hwnd: HWND, delta: i32) {
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let visible = client.bottom - self.scale(STATUS) - self.side_origin();
        let (_, content) = self.settings_rows(0, 0);
        let max = (content + self.scale(12) - visible).max(0);
        self.settings_scroll =
            (self.settings_scroll - delta * self.scale(ROW) * 2 / 120).clamp(0, max);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// What the mouse is over at (x, y), in the side panel's coordinates.
    pub(super) fn settings_hot_at(&self, x: i32, y: i32) -> Option<SettingsHot> {
        let (left, right) = (self.scale(RAIL), self.sidebar_right());
        if contains(self.settings_json_button(right), x, y) {
            return Some(SettingsHot::Json);
        }
        if y < self.scale(LIST_TOP - 4) {
            return None;
        }
        let (rows, _) = self.settings_rows(left, right);
        rows.iter()
            .find(|placed| contains(placed.rect, x, y) && row_setting(placed.row).is_some())
            .map(|placed| SettingsHot::Row(placed.index))
    }

    /// Where `hot` is, in the side panel's coordinates.
    pub(super) fn settings_hot_rect(&self, hot: SettingsHot) -> Option<RECT> {
        let (left, right) = (self.scale(RAIL), self.sidebar_right());
        match hot {
            SettingsHot::Json => Some(self.settings_json_button(right)),
            SettingsHot::Row(index) => self
                .settings_rows(left, right)
                .0
                .into_iter()
                .find(|placed| placed.index == index)
                .map(|placed| placed.rect),
        }
    }

    pub(super) fn settings_tip(&self, hot: SettingsHot) -> Option<&'static str> {
        match hot {
            SettingsHot::Json => Some("Open settings.json"),
            SettingsHot::Row(index) => ROWS.get(index).copied().and_then(row_setting).map(tip),
        }
    }

    /// A click in the Settings view; `y` is in the side panel's coordinates.
    pub(super) fn settings_panel_click(&mut self, hwnd: HWND, x: i32, y: i32) {
        let (left, right) = (self.scale(RAIL), self.sidebar_right());
        if contains(self.settings_field(left, right), x, y) {
            self.settings_search_active = true;
            self.search_input = false;
            self.commit_focus = false;
            self.extensions_search_active = false;
            self.panel_focus = false;
            self.invalidate_settings_panel(hwnd);
            return;
        }
        if self.settings_search_active {
            self.settings_search_active = false;
            self.invalidate_settings_panel(hwnd);
        }
        if contains(self.settings_json_button(right), x, y) {
            self.open_settings(hwnd);
            return;
        }
        if y < self.scale(LIST_TOP - 4) {
            return;
        }
        let (rows, _) = self.settings_rows(left, right);
        let Some(&Placed { row, rect, .. }) =
            rows.iter().find(|placed| contains(placed.rect, x, y))
        else {
            return;
        };
        if let (Some(setting), Some(control)) = (row_setting(row), self.setting_control(row, rect))
            && self.setting_changed(setting)
            && contains(self.reset_button(control), x, y)
        {
            self.reset_setting(hwnd, setting);
            return;
        }
        match row {
            Row::Toggle(setting) => self.toggle_setting(hwnd, setting),
            Row::Stepper(setting) => {
                let Some(control) = self.setting_control(row, rect) else {
                    return;
                };
                let (minus, plus) = self.stepper_buttons(control);
                if contains(minus, x, y) {
                    self.step_setting(hwnd, setting, -1);
                } else if contains(plus, x, y) {
                    self.step_setting(hwnd, setting, 1);
                }
            }
            Row::Choice(Setting::ColorTheme) => {
                // The theme menu saves the choice itself.
                let top = rect.bottom + self.side_origin();
                let left = self
                    .setting_control(row, rect)
                    .map_or(rect.left, |c| c.left);
                self.show_color_theme_menu(hwnd, left, top);
            }
            Row::Choice(Setting::TerminalProfile) => {
                let current = SHELLS
                    .iter()
                    .position(|shell| *shell == self.settings.default_terminal_profile)
                    .unwrap_or(0);
                // Saves it too.
                self.set_default_terminal_profile(hwnd, SHELLS[(current + 1) % SHELLS.len()]);
            }
            Row::Choice(Setting::ParseLimit) => {
                const LIMITS: [usize; 6] = [512, 1024, 2048, 4096, 8192, 16384];
                let current = LIMITS
                    .iter()
                    .position(|&kb| kb == self.settings.parse_limit_kb)
                    .unwrap_or(3);
                let next_kb = LIMITS[(current + 1) % LIMITS.len()];
                self.settings.parse_limit_kb = next_kb;
                lightline::syntax::set_parse_limit_kb(next_kb);
                self.save_settings_change(hwnd);
            }
            Row::Choice(Setting::AiEndpoint) => self.show_ai_provider(hwnd, false),
            Row::OpenJson if contains(self.settings_json_link(rect), x, y) => {
                self.open_settings(hwnd);
            }
            _ => {}
        }
    }

    fn toggle_setting(&mut self, hwnd: HWND, setting: Setting) {
        let settings = &mut self.settings;
        let value = match setting {
            Setting::InsertSpaces => &mut settings.insert_spaces,
            Setting::WordWrap => &mut settings.word_wrap,
            Setting::AutoIndent => &mut settings.auto_indent,
            Setting::AutoClosePairs => &mut settings.auto_close_pairs,
            Setting::BracketMatching => &mut settings.bracket_matching,
            Setting::IndentGuides => &mut settings.indent_guides,
            Setting::BracketColors => &mut settings.bracket_pair_colors,
            Setting::Minimap => &mut settings.minimap,
            Setting::GitBlame => &mut settings.git_blame,
            Setting::CodeLens => &mut settings.code_lens,
            Setting::FormatOnSave => &mut settings.format_on_save,
            Setting::MarkdownImages => &mut settings.markdown_load_remote_images,
            Setting::Animations => &mut settings.animations,
            Setting::SmoothScrolling => &mut settings.smooth_scrolling,
            _ => return,
        };
        *value = !*value;
        if setting == Setting::WordWrap || setting == Setting::Minimap {
            // Tabs that follow the setting (Alt+Z wasn't used on them)
            // change how their rows are laid out, so they start at the top;
            // the minimap changes how wide wrapped rows are.
            let wrap_setting = setting == Setting::WordWrap;
            for tab in self
                .tabs
                .iter_mut()
                .filter(|tab| tab.word_wrap.is_none() || !wrap_setting)
            {
                for view in &mut tab.views {
                    view.first_row = 0;
                }
            }
            self.keep_cursor_visible(hwnd);
        }
        self.save_settings_change(hwnd);
    }

    fn step_setting(&mut self, hwnd: HWND, setting: Setting, delta: i32) {
        match setting {
            Setting::FontSize => {
                let size = (self.settings.font_size + delta).clamp(8, 48);
                if size == self.settings.font_size {
                    return;
                }
                self.settings.font_size = size;
                self.set_metrics(self.dpi, self.zoom);
                self.keep_cursor_visible(hwnd);
            }
            Setting::TabSize => {
                let size = (self.settings.tab_size as i32 + delta).clamp(1, 16) as usize;
                if size == self.settings.tab_size {
                    return;
                }
                self.settings.tab_size = size;
            }
            _ => return,
        }
        self.save_settings_change(hwnd);
    }

    fn save_settings_change(&mut self, hwnd: HWND) {
        if let Err(error) = self.settings.save() {
            // Applied for now, but it won't last past a restart.
            self.status = error;
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn paint_settings_panel(&self, hdc: HDC, left: i32, _right: i32, bottom: i32) {
        let s = |value: i32| self.scale(value);
        let right = self.sidebar_right();
        unsafe { SelectObject(hdc, self.ui_font) };
        let json = self.settings_json_button(right);
        let json_hot = self.hot == Some(Hot::Settings(SettingsHot::Json));
        if json_hot {
            Self::rounded_fill(
                hdc,
                json,
                s(4),
                self.hot_color(self.theme.sidebar_bg, hover_fill()),
            );
        }
        let json_color = if json_hot {
            self.hot_color(self.theme.muted, self.theme.text)
        } else {
            self.theme.muted
        };
        self.glyph_in(hdc, DebugGlyph::Braces, json_color, json, s(16));
        self.paint_settings_field(hdc, left, right);

        // Below the field, so scrolled rows slide under it.
        let clip = RECT {
            left,
            top: s(LIST_TOP - 4),
            right,
            bottom,
        };
        let state = unsafe { SaveDC(hdc) };
        unsafe { IntersectClipRect(hdc, clip.left, clip.top, clip.right, clip.bottom) };
        let hot_row = match self.hot {
            Some(Hot::Settings(SettingsHot::Row(index))) => Some(index),
            _ => None,
        };
        let query = self.settings_search();
        let (rows, _) = self.settings_rows(left, right);
        for Placed { index, row, rect } in rows {
            if rect.bottom < clip.top || rect.top > clip.bottom {
                continue;
            }
            let middle = (rect.top + rect.bottom) / 2;
            let text_left = rect.left + s(16);
            let Some(setting) = row_setting(row) else {
                match row {
                    Row::Heading(title) => {
                        unsafe { SelectObject(hdc, self.small_font) };
                        self.label_mid(
                            hdc,
                            title,
                            text_left,
                            rect.bottom - s(9),
                            self.theme.muted,
                            rect,
                        );
                        unsafe { SelectObject(hdc, self.ui_font) };
                    }
                    Row::NoMatch => {
                        self.label_mid(
                            hdc,
                            "No matching settings",
                            text_left,
                            middle,
                            self.theme.muted,
                            rect,
                        );
                    }
                    Row::OpenJson => {
                        self.paint_settings_json_link(hdc, self.settings_json_link(rect))
                    }
                    _ => {}
                }
                continue;
            };
            let hovered = hot_row == Some(index);
            let background = if hovered {
                let fill = self.hot_color(self.theme.sidebar_bg, hover_fill());
                Self::fill(hdc, rect, fill);
                fill
            } else {
                self.theme.sidebar_bg
            };
            let changed = self.setting_changed(setting);
            if changed {
                Self::fill(
                    hdc,
                    RECT {
                        left: rect.left + s(6),
                        top: rect.top + s(7),
                        right: rect.left + s(6) + s(2).max(2),
                        bottom: rect.bottom - s(7),
                    },
                    self.theme.violet,
                );
            }
            let control = self.setting_control(row, rect);
            let mut label_right = match (row, control) {
                (Row::Toggle(_), Some(control)) => {
                    self.paint_switch(hdc, control, self.setting_on(setting), background);
                    control.left
                }
                (Row::Stepper(_), Some(control)) => {
                    self.paint_stepper(hdc, control, &self.setting_value(setting));
                    control.left
                }
                (Row::Choice(_), Some(control)) => {
                    self.paint_choice(hdc, control, &self.setting_value(setting));
                    control.left
                }
                _ => {
                    // Info: the value, dimmed, at the right.
                    let value = self.setting_value(setting);
                    let value_right = rect.right - s(12);
                    let width = self.text_width(hdc, &value).min(s(CHOICE_WIDTH));
                    let value_clip = RECT {
                        left: value_right - width,
                        right: value_right,
                        ..rect
                    };
                    self.label_ellipsis(
                        hdc,
                        &value,
                        value_clip.left,
                        middle - self.text_height(hdc) / 2,
                        self.theme.muted,
                        value_clip,
                    );
                    value_clip.left
                }
            };
            if changed
                && hovered
                && let Some(control) = control
            {
                let reset = self.reset_button(control);
                self.glyph_in(hdc, DebugGlyph::Discard, self.theme.muted, reset, s(14));
                label_right = reset.left;
            }
            self.label_match(
                hdc,
                label(setting),
                (text_left, middle, label_right - s(8)),
                &query,
                self.theme.text,
                self.theme.sky,
            );
        }
        unsafe { RestoreDC(hdc, state) };
    }

    fn paint_settings_field(&self, hdc: HDC, left: i32, right: i32) {
        let s = |value: i32| self.scale(value);
        let field = self.settings_field(left, right);
        let active = self.settings_search_active;
        let edge = if active {
            self.theme.sky
        } else {
            self.theme.edge
        };
        self.panel_card(hdc, field, s(4), edge, self.theme.editor_bg);
        self.glyph_in(
            hdc,
            DebugGlyph::Search,
            self.theme.muted,
            RECT {
                left: field.left + s(4),
                right: field.left + s(24),
                ..field
            },
            s(14),
        );
        let middle = (field.top + field.bottom) / 2;
        let text_left = field.left + s(28);
        let mut text_right = field.right - s(8);
        let query = self.settings_search();
        if !query.is_empty() {
            let shown = shown_rows(&query)
                .iter()
                .filter(|(_, row)| row_setting(*row).is_some())
                .count();
            let count = format!("{shown} of {}", setting_count());
            unsafe { SelectObject(hdc, self.small_font) };
            let width = self.text_width(hdc, &count);
            self.label_mid(
                hdc,
                &count,
                text_right - width,
                middle,
                self.theme.muted,
                field,
            );
            unsafe { SelectObject(hdc, self.ui_font) };
            text_right -= width + s(8);
        }
        let clip = RECT {
            left: text_left,
            right: text_right,
            ..field
        };
        if self.settings_query.is_empty() {
            self.label_mid(
                hdc,
                "Search settings",
                text_left,
                middle,
                self.theme.muted,
                clip,
            );
        } else {
            self.label_mid(
                hdc,
                &self.settings_query,
                text_left,
                middle,
                self.theme.text,
                clip,
            );
        }
        if active && self.focused && self.caret_on {
            let x = text_left + self.text_width(hdc, &self.settings_query);
            Self::fill(
                hdc,
                RECT {
                    left: x.min(text_right),
                    top: middle - s(8),
                    right: x.min(text_right) + s(1).max(1),
                    bottom: middle + s(8),
                },
                self.theme.text,
            );
        }
    }

    fn paint_settings_json_link(&self, hdc: HDC, row: RECT) {
        let s = |value: i32| self.scale(value);
        let middle = (row.top + row.bottom) / 2;
        let icon = RECT {
            left: row.left + s(14),
            right: row.left + s(30),
            ..row
        };
        self.glyph_in(hdc, DebugGlyph::Braces, self.theme.sky, icon, s(14));
        let text = "Open settings.json";
        let text_left = icon.right + s(6);
        self.label_mid(hdc, text, text_left, middle, self.theme.sky, row);
        let note = "for everything else";
        unsafe { SelectObject(hdc, self.small_font) };
        let width = self.text_width(hdc, note);
        let note_left = row.right - s(12) - width;
        if note_left > text_left + self.text_width(hdc, text) + s(16) {
            self.label_mid(hdc, note, note_left, middle, self.theme.muted, row);
        }
        unsafe { SelectObject(hdc, self.ui_font) };
    }

    // An on/off switch filling `switch`; off is an outline on `background`.
    fn paint_switch(&self, hdc: HDC, switch: RECT, on: bool, background: u32) {
        let s = |value: i32| self.scale(value);
        let height = switch.bottom - switch.top;
        let inset = s(1).max(1);
        if on {
            Self::rounded_fill(hdc, switch, height, self.theme.sky);
        } else {
            Self::rounded_fill(
                hdc,
                switch,
                height,
                blend(self.theme.edge, self.theme.muted, 0.4),
            );
            Self::rounded_fill(
                hdc,
                RECT {
                    left: switch.left + inset,
                    top: switch.top + inset,
                    right: switch.right - inset,
                    bottom: switch.bottom - inset,
                },
                height - inset * 2,
                background,
            );
        }
        let knob = height - s(6);
        let knob_left = if on {
            switch.right - s(3) - knob
        } else {
            switch.left + s(3)
        };
        let knob_top = switch.top + s(3);
        Self::rounded_fill(
            hdc,
            RECT {
                left: knob_left,
                top: knob_top,
                right: knob_left + knob,
                bottom: knob_top + knob,
            },
            knob,
            if on {
                self.theme.editor_bg
            } else {
                self.theme.muted
            },
        );
    }

    // − value + in one box.
    fn paint_stepper(&self, hdc: HDC, control: RECT, value: &str) {
        let s = |value: i32| self.scale(value);
        self.panel_card(hdc, control, s(4), self.theme.edge, self.theme.editor_bg);
        let (minus, plus) = self.stepper_buttons(control);
        for (button, glyph) in [(minus, DebugGlyph::Minus), (plus, DebugGlyph::Plus)] {
            self.glyph_in(hdc, glyph, self.theme.text, button, s(12));
        }
        for x in [minus.right, plus.left - s(1).max(1)] {
            Self::fill(
                hdc,
                RECT {
                    left: x,
                    top: control.top + s(1).max(1),
                    right: x + s(1).max(1),
                    bottom: control.bottom - s(1).max(1),
                },
                self.theme.edge,
            );
        }
        let width = self.text_width(hdc, value);
        let middle = (control.top + control.bottom) / 2;
        self.label_mid(
            hdc,
            value,
            (minus.right + plus.left - width) / 2,
            middle,
            self.theme.text,
            control,
        );
    }

    // A drop-down box: the value and a chevron.
    fn paint_choice(&self, hdc: HDC, control: RECT, value: &str) {
        let s = |value: i32| self.scale(value);
        self.panel_card(hdc, control, s(4), self.theme.edge, self.theme.editor_bg);
        let middle = (control.top + control.bottom) / 2;
        let chevron = RECT {
            left: control.right - s(20),
            right: control.right - s(4),
            ..control
        };
        self.glyph_in(
            hdc,
            DebugGlyph::ChevronDown,
            self.theme.muted,
            chevron,
            s(12),
        );
        let clip = RECT {
            left: control.left + s(8),
            right: chevron.left - s(2),
            ..control
        };
        self.label_ellipsis(
            hdc,
            value,
            clip.left,
            middle - self.text_height(hdc) / 2,
            self.theme.text,
            clip,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_keeps_headings_of_matches_and_the_json_link() {
        let rows: Vec<Row> = shown_rows("brack")
            .into_iter()
            .map(|(_, row)| row)
            .collect();
        assert_eq!(
            rows,
            [
                Row::Heading("EDITOR"),
                Row::Toggle(Setting::AutoClosePairs),
                Row::Heading("VISUAL AIDS"),
                Row::Toggle(Setting::BracketMatching),
                Row::Toggle(Setting::BracketColors),
                Row::OpenJson,
            ]
        );
    }

    #[test]
    fn search_matches_json_keys_and_says_when_nothing_does() {
        let rows: Vec<Row> = shown_rows("minimap")
            .into_iter()
            .map(|(_, row)| row)
            .collect();
        assert!(rows.contains(&Row::Toggle(Setting::Minimap)));
        let rows: Vec<Row> = shown_rows("zzz").into_iter().map(|(_, row)| row).collect();
        assert_eq!(rows, [Row::NoMatch, Row::OpenJson]);
    }

    #[test]
    fn every_setting_is_listed_once() {
        assert_eq!(setting_count(), 21);
        let all = shown_rows("");
        assert_eq!(all.len(), ROWS.len());
    }
}
