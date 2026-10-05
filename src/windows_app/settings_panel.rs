// The Settings view in the side panel, opened by the gear at the bottom of
// the rail (Ctrl+,). The gear used to only say "Settings are not available
// yet", leaving settings.json as the only way to change anything.
//
// It shows only settings that take effect. Each change applies at once and
// is saved to settings.json, keeping whatever else the file holds. Painting
// and clicks share `settings_rows`, in the side panel's coordinates (y from
// the top of the workbench, as the side panel is painted).

use super::*;
use lightline::terminal::ShellKind;

const TOP: i32 = 48;
const ROW: i32 = 34;
const HEADING: i32 = 30;
const BUTTON: i32 = 34;

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
    FormatOnSave,
    ParseLimit,
    ColorTheme,
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
}

const ROWS: &[Row] = &[
    Row::Heading("EDITOR"),
    Row::Stepper(Setting::FontSize),
    Row::Stepper(Setting::TabSize),
    Row::Toggle(Setting::InsertSpaces),
    Row::Toggle(Setting::WordWrap),
    Row::Toggle(Setting::AutoIndent),
    Row::Toggle(Setting::AutoClosePairs),
    Row::Toggle(Setting::BracketMatching),
    Row::Toggle(Setting::IndentGuides),
    Row::Toggle(Setting::FormatOnSave),
    Row::Choice(Setting::ParseLimit),
    Row::Heading("APPEARANCE"),
    Row::Choice(Setting::ColorTheme),
    Row::Heading("TERMINAL"),
    Row::Choice(Setting::TerminalProfile),
    Row::Heading("MARKDOWN"),
    Row::Toggle(Setting::MarkdownImages),
    Row::Heading("AI ASSISTANT"),
    Row::Info(Setting::AiEndpoint),
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
        Setting::FormatOnSave => "Format on save",
        Setting::ParseLimit => "Syntax parse limit",
        Setting::ColorTheme => "Color theme",
        Setting::TerminalProfile => "Default shell",
        Setting::MarkdownImages => "Load web images in previews",
        Setting::AiEndpoint => "Server",
        Setting::AiModel => "Model",
    }
}

fn row_height(row: Row) -> i32 {
    match row {
        Row::Heading(_) => HEADING,
        Row::OpenJson => BUTTON + 20,
        _ => ROW,
    }
}

fn contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

impl App {
    /// Shows the Settings view (Ctrl+,); unlike the gear, it never closes it.
    pub(super) fn open_settings_panel(&mut self, hwnd: HWND) {
        if self.side_view != SideView::Settings || !self.explorer_visible {
            self.toggle_side_view(hwnd, SideView::Settings);
        }
    }

    fn setting_on(&self, setting: Setting) -> bool {
        let settings = &self.settings;
        match setting {
            Setting::InsertSpaces => settings.insert_spaces,
            Setting::WordWrap => settings.word_wrap,
            Setting::AutoIndent => settings.auto_indent,
            Setting::AutoClosePairs => settings.auto_close_pairs,
            Setting::BracketMatching => settings.bracket_matching,
            Setting::IndentGuides => settings.indent_guides,
            Setting::FormatOnSave => settings.format_on_save,
            Setting::MarkdownImages => settings.markdown_load_remote_images,
            _ => false,
        }
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
            Setting::AiEndpoint => settings.ai_endpoint.clone(),
            Setting::AiModel => settings
                .ai_model
                .clone()
                .unwrap_or_else(|| "None yet".into()),
            _ => String::new(),
        }
    }

    // Each row with its rect, scrolled, from `left` to `right`; and the
    // bottom of the last row before scrolling.
    fn settings_rows(&self, left: i32, right: i32) -> (Vec<(Row, RECT)>, i32) {
        let s = |value: i32| self.scale(value);
        let mut top = s(TOP);
        let rows = ROWS
            .iter()
            .map(|&row| {
                let rect = RECT {
                    left: left + s(8),
                    top: top - self.settings_scroll,
                    right: right - s(8),
                    bottom: top + s(row_height(row)) - self.settings_scroll,
                };
                top += s(row_height(row));
                (row, rect)
            })
            .collect();
        (rows, top)
    }

    // The − and + buttons of a stepper row.
    fn stepper_buttons(&self, row: RECT) -> (RECT, RECT) {
        let s = |value: i32| self.scale(value);
        let size = s(24);
        let top = (row.top + row.bottom - size) / 2;
        let plus = RECT {
            left: row.right - s(8) - size,
            top,
            right: row.right - s(8),
            bottom: top + size,
        };
        let minus = RECT {
            left: plus.left - s(34) - size,
            top,
            right: plus.left - s(34),
            bottom: top + size,
        };
        (minus, plus)
    }

    fn settings_json_button(&self, row: RECT) -> RECT {
        let s = |value: i32| self.scale(value);
        RECT {
            left: row.left + s(8),
            top: row.top + s(12),
            right: row.right - s(8),
            bottom: row.top + s(12) + s(BUTTON),
        }
    }

    pub(super) fn scroll_settings_panel(&mut self, hwnd: HWND, delta: i32) {
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let visible = client.bottom - self.scale(STATUS) - self.chrome_top();
        let (_, content) = self.settings_rows(0, 0);
        let max = (content + self.scale(12) - visible).max(0);
        self.settings_scroll =
            (self.settings_scroll - delta * self.scale(ROW) * 2 / 120).clamp(0, max);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// A click in the Settings view; `y` is in the side panel's coordinates.
    pub(super) fn settings_panel_click(&mut self, hwnd: HWND, x: i32, y: i32) {
        let (rows, _) = self.settings_rows(self.scale(RAIL), self.sidebar_right());
        let Some(&(row, rect)) = rows.iter().find(|(_, rect)| contains(*rect, x, y)) else {
            return;
        };
        match row {
            Row::Toggle(setting) => self.toggle_setting(hwnd, setting),
            Row::Stepper(setting) => {
                let (minus, plus) = self.stepper_buttons(rect);
                if contains(minus, x, y) {
                    self.step_setting(hwnd, setting, -1);
                } else if contains(plus, x, y) {
                    self.step_setting(hwnd, setting, 1);
                }
            }
            Row::Choice(Setting::ColorTheme) => {
                // The theme menu saves the choice itself.
                let top = rect.bottom + self.chrome_top();
                self.show_color_theme_menu(hwnd, rect.left, top);
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
            Row::OpenJson if contains(self.settings_json_button(rect), x, y) => {
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
            Setting::FormatOnSave => &mut settings.format_on_save,
            Setting::MarkdownImages => &mut settings.markdown_load_remote_images,
            _ => return,
        };
        *value = !*value;
        if setting == Setting::WordWrap {
            // Tabs that follow the setting (Alt+Z wasn't used on them)
            // change how their rows are laid out, so they start at the top.
            for tab in self.tabs.iter_mut().filter(|tab| tab.word_wrap.is_none()) {
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

    pub(super) fn paint_settings_panel(&self, hdc: HDC, left: i32, right: i32, bottom: i32) {
        let s = |value: i32| self.scale(value);
        // Below the panel's title row, so scrolled rows slide under it.
        let clip = RECT {
            left,
            top: s(40),
            right,
            bottom,
        };
        let state = unsafe { SaveDC(hdc) };
        unsafe {
            IntersectClipRect(hdc, clip.left, clip.top, clip.right, clip.bottom);
            SelectObject(hdc, self.ui_font);
        }
        let (rows, _) = self.settings_rows(left, right);
        for (row, rect) in rows {
            if rect.bottom < clip.top || rect.top > clip.bottom {
                continue;
            }
            let middle = (rect.top + rect.bottom) / 2;
            let text_left = rect.left + s(8);
            match row {
                Row::Heading(title) => {
                    let y = rect.bottom - s(8);
                    self.label_mid(hdc, title, text_left, y, self.theme.muted, rect);
                }
                Row::Toggle(setting) => {
                    let switch = self.paint_switch(hdc, rect, self.setting_on(setting));
                    let label_clip = RECT {
                        right: switch.left - s(8),
                        ..rect
                    };
                    self.paint_setting_label(hdc, label(setting), text_left, middle, label_clip);
                }
                Row::Stepper(setting) => {
                    let (minus, plus) = self.stepper_buttons(rect);
                    for (button, glyph) in [(minus, "\u{2212}"), (plus, "+")] {
                        self.panel_card(hdc, button, s(6), self.theme.edge, self.theme.editor_bg);
                        let width = self.text_width(hdc, glyph);
                        let x = (button.left + button.right - width) / 2;
                        self.label_mid(hdc, glyph, x, middle, self.theme.text, button);
                    }
                    let value = self.setting_value(setting);
                    let width = self.text_width(hdc, &value);
                    let x = (minus.right + plus.left - width) / 2;
                    self.label_mid(hdc, &value, x, middle, self.theme.text, rect);
                    let label_clip = RECT {
                        right: minus.left - s(8),
                        ..rect
                    };
                    self.paint_setting_label(hdc, label(setting), text_left, middle, label_clip);
                }
                Row::Choice(setting) | Row::Info(setting) => {
                    let choice = matches!(row, Row::Choice(_));
                    let label_text = label(setting);
                    let label_right = text_left + self.text_width(hdc, label_text) + s(16);
                    self.paint_setting_label(hdc, label_text, text_left, middle, rect);
                    let value = if choice {
                        format!("{} \u{25be}", self.setting_value(setting))
                    } else {
                        self.setting_value(setting)
                    };
                    let color = if choice {
                        self.theme.sky
                    } else {
                        self.theme.muted
                    };
                    let value_clip = RECT {
                        left: label_right,
                        right: rect.right - s(8),
                        ..rect
                    };
                    let width = self.text_width(hdc, &value);
                    let x = (value_clip.right - width).max(value_clip.left);
                    self.label_ellipsis(
                        hdc,
                        &value,
                        x,
                        middle - self.text_height(hdc) / 2,
                        color,
                        value_clip,
                    );
                }
                Row::OpenJson => {
                    let button = self.settings_json_button(rect);
                    self.panel_card(hdc, button, s(6), self.theme.edge, self.theme.active_bg);
                    let text = "Open settings.json";
                    let width = self.text_width(hdc, text);
                    let x = (button.left + button.right - width) / 2;
                    let middle = (button.top + button.bottom) / 2;
                    self.label_mid(hdc, text, x, middle, self.theme.text, button);
                }
            }
        }
        unsafe { RestoreDC(hdc, state) };
    }

    fn paint_setting_label(&self, hdc: HDC, text: &str, x: i32, middle: i32, clip: RECT) {
        let top = middle - self.text_height(hdc) / 2;
        self.label_ellipsis(hdc, text, x, top, self.theme.text, clip);
    }

    // An on/off switch at the right of `row`; returns its rect.
    fn paint_switch(&self, hdc: HDC, row: RECT, on: bool) -> RECT {
        let s = |value: i32| self.scale(value);
        let (width, height) = (s(32), s(18));
        let top = (row.top + row.bottom - height) / 2;
        let switch = RECT {
            left: row.right - s(8) - width,
            top,
            right: row.right - s(8),
            bottom: top + height,
        };
        let track = if on { self.theme.sky } else { self.theme.edge };
        Self::rounded_fill(hdc, switch, height, track);
        let knob = height - s(6);
        let knob_left = if on {
            switch.right - s(3) - knob
        } else {
            switch.left + s(3)
        };
        Self::rounded_fill(
            hdc,
            RECT {
                left: knob_left,
                top: top + s(3),
                right: knob_left + knob,
                bottom: top + s(3) + knob,
            },
            knob,
            if on {
                self.theme.editor_bg
            } else {
                self.theme.muted
            },
        );
        switch
    }
}
