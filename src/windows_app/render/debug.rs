use super::super::debug_panel::{
    DEBUG_INDENT, DEBUG_NEST, DebugButton, DebugHot, DebugLayout, DebugRow, DebugSection, DebugTone,
};
use super::super::*;
use super::blend;

// The editor's paused-line yellow and breakpoint red, so the panel's marks
// match the gutter's.
const PAUSED: (u8, u8, u8) = (255, 204, 0);
const BREAKPOINT: (u8, u8, u8) = (246, 110, 120);
const GO: (u8, u8, u8) = (34, 197, 94);

fn rgb((r, g, b): (u8, u8, u8)) -> u32 {
    ui(r, g, b)
}

impl App {
    /// The Run & Debug panel below its header title, laid out as
    /// design/run-debug-design.html: the launch bar (the session toolbar
    /// while debugging), one status line, then one list of variables, call
    /// stack and breakpoints, every row 22 px.
    pub(in crate::windows_app) fn paint_debug_panel(
        &self,
        hdc: HDC,
        left: i32,
        right: i32,
        bottom: i32,
        clip: RECT,
    ) {
        let layout = self.debug_panel_layout(left, right);
        // Hover repaints one row; rows outside the update region are skipped.
        let mut update = RECT::default();
        unsafe { GetClipBox(hdc, &mut update) };
        let shows = |rect: &RECT| rect.bottom > update.top && rect.top < update.bottom;

        self.paint_debug_button(hdc, layout.more, DebugButton::More, DebugGlyph::More, None);
        if shows(&layout.bar) {
            if self.debug_session_shown() {
                self.paint_debug_toolbar(hdc, &layout);
            } else {
                self.paint_launch_bar(hdc, &layout);
            }
        }
        if shows(&layout.state) {
            self.paint_debug_status(hdc, &layout, clip);
        }
        Self::fill(
            hdc,
            RECT {
                left,
                top: layout.divider,
                right,
                bottom: layout.divider + 1,
            },
            self.theme.edge,
        );

        let rows = self.debug_rows();
        let hot_row = self.debug_hot_row();
        for (index, rect) in self.debug_rows_on_screen(rows.len(), left, right, layout.list_top) {
            if rect.top >= bottom {
                break;
            }
            if !shows(&rect) {
                continue;
            }
            self.paint_debug_row(hdc, &rows[index], rect, hot_row == Some(index), clip);
        }
        self.paint_debug_scrollbar(hdc, &layout, rows.len(), right, bottom);
    }

    fn debug_hover_fill(&self) -> u32 {
        self.hot_color(self.theme.sidebar_bg, self.theme.active_bg)
    }

    fn debug_hot(&self, button: DebugButton) -> bool {
        self.hot == Some(Hot::Debug(DebugHot::Button(button)))
    }

    /// An icon button: lit under the mouse.
    fn paint_debug_button(
        &self,
        hdc: HDC,
        rect: RECT,
        button: DebugButton,
        glyph: DebugGlyph,
        color: Option<u32>,
    ) {
        let hot = self.debug_hot(button);
        if hot {
            Self::rounded_fill(hdc, rect, self.scale(4), self.debug_hover_fill());
        }
        let color = color.unwrap_or(if hot {
            self.hot_color(self.theme.muted, self.theme.text)
        } else {
            self.theme.muted
        });
        self.glyph_in(hdc, glyph, color, rect, self.scale(16));
    }

    /// ▶ joined to the configuration picker.
    fn paint_launch_bar(&self, hdc: HDC, layout: &DebugLayout) {
        let radius = self.scale(4);
        self.panel_card(
            hdc,
            layout.bar,
            radius,
            self.theme.edge,
            self.theme.active_bg,
        );
        let lit = blend(self.theme.active_bg, self.theme.text, 0.08);
        for (button, rect) in [
            (DebugButton::Go, layout.go),
            (DebugButton::Pick, layout.pick),
        ] {
            if self.debug_hot(button) {
                Self::rounded_fill(
                    hdc,
                    RECT {
                        left: rect.left + 1,
                        top: rect.top + 1,
                        right: rect.right - 1,
                        bottom: rect.bottom - 1,
                    },
                    radius - 1,
                    self.hot_color(self.theme.active_bg, lit),
                );
            }
        }
        Self::fill(
            hdc,
            RECT {
                left: layout.go.right,
                top: layout.go.top + 1,
                right: layout.go.right + 1,
                bottom: layout.go.bottom - 1,
            },
            self.theme.edge,
        );
        let config = self.effective_debug_config();
        let go = if config.is_some() {
            rgb(GO)
        } else {
            blend(self.theme.edge, self.theme.muted, 0.3)
        };
        self.glyph_in(hdc, DebugGlyph::Start, go, layout.go, self.scale(14));

        let pick = layout.pick;
        let middle = (pick.top + pick.bottom) / 2;
        let size = self.scale(16);
        let chevron = RECT {
            left: pick.right - self.scale(26),
            ..pick
        };
        self.glyph_in(
            hdc,
            DebugGlyph::ChevronDown,
            self.theme.muted,
            chevron,
            size,
        );
        let mut text_right = chevron.left;
        // "Auto" when Automatic picked it.
        if self.debug_config.is_none() && config.is_some() {
            let width = self.text_width(hdc, "Auto");
            text_right -= width;
            self.label_mid(hdc, "Auto", text_right, middle, self.theme.muted, pick);
            text_right -= self.scale(8);
        }
        let (label, color) = match config {
            Some(config) => (config.label(), self.theme.text),
            None => ("No configuration", self.theme.muted),
        };
        let text_left = pick.left + self.scale(10);
        self.label_ellipsis(
            hdc,
            label,
            text_left,
            middle - self.text_height(hdc) / 2,
            color,
            RECT {
                right: text_right,
                ..pick
            },
        );
    }

    /// Continue/Pause, Step Over, Into, Out | Restart, Stop.
    fn paint_debug_toolbar(&self, hdc: HDC, layout: &DebugLayout) {
        self.panel_card(
            hdc,
            layout.bar,
            self.scale(4),
            self.theme.edge,
            self.theme.active_bg,
        );
        let off = blend(self.theme.edge, self.theme.muted, 0.3);
        let step = ui(170, 202, 250);
        let running = self.debug_state.running;
        for (index, rect) in layout.tools.iter().enumerate() {
            let enabled = self.debug_tool_enabled(index);
            let (glyph, color) = match index {
                0 if running => (DebugGlyph::Pause, step),
                0 => (DebugGlyph::Continue, rgb(GO)),
                1 => (DebugGlyph::StepOver, step),
                2 => (DebugGlyph::StepInto, step),
                3 => (DebugGlyph::StepOut, step),
                4 => (DebugGlyph::Restart, step),
                _ => (DebugGlyph::Stop, self.theme.error),
            };
            let color = if enabled { color } else { off };
            if enabled && self.debug_hot(DebugButton::Tool(index)) {
                Self::rounded_fill(
                    hdc,
                    *rect,
                    self.scale(4),
                    self.hot_color(self.theme.active_bg, super::super::hot::hover_fill()),
                );
            }
            self.glyph_in(hdc, glyph, color, *rect, self.scale(16));
        }
        let gap = layout.tools[4].left - self.scale(4);
        let middle = (layout.bar.top + layout.bar.bottom) / 2;
        Self::fill(
            hdc,
            RECT {
                left: gap,
                top: middle - self.scale(8),
                right: gap + 1,
                bottom: middle + self.scale(8),
            },
            self.theme.edge,
        );
        // What's being debugged, at the right: the language and adapter, or
        // just the adapter when the panel is too narrow for both.
        let (full, short) = match self.debug_state.config {
            Some(DebugConfig::RustWorkspace) => ("Rust \u{00b7} LLDB", "LLDB"),
            Some(DebugConfig::PythonFile) => ("Python \u{00b7} debugpy", "debugpy"),
            None => ("", ""),
        };
        let room = layout.bar.right - self.scale(8) - (layout.tools[5].right + self.scale(8));
        if let Some(label) = [full, short]
            .into_iter()
            .find(|label| self.text_width(hdc, label) <= room)
        {
            let x = layout.bar.right - self.scale(8) - self.text_width(hdc, label);
            self.label_mid(hdc, label, x, middle, self.theme.muted, layout.bar);
        }
    }

    /// One line: a dot, what's happening, where, and maybe a link.
    fn paint_debug_status(&self, hdc: HDC, layout: &DebugLayout, clip: RECT) {
        let status = self.debug_status();
        let rect = layout.state;
        let middle = (rect.top + rect.bottom) / 2;
        let dot_color = match status.tone {
            DebugTone::Idle => self.theme.muted,
            DebugTone::Busy => self.theme.blue,
            DebugTone::Running => self.theme.green,
            DebugTone::Paused => self.theme.warning,
            DebugTone::Failed => self.theme.error,
        };
        let dot = self.scale(8);
        self.icons.draw_glyph(
            hdc,
            DebugGlyph::GutterDot,
            dot_color,
            rect.left,
            middle - dot / 2,
            dot,
        );
        let mut text_right = rect.right;
        if let (Some(link), Some(link_rect)) =
            (status.link, self.debug_link_rect(layout, Some(hdc)))
        {
            let color = if self.debug_hot(DebugButton::Link) {
                self.hot_color(self.theme.blue, self.theme.text)
            } else {
                self.theme.blue
            };
            self.label_mid(hdc, link, link_rect.left, middle, color, rect);
            text_right = link_rect.left - self.scale(8);
        }
        let top = middle - self.text_height(hdc) / 2;
        let limit = RECT {
            right: text_right,
            ..clip
        };
        let title_left = rect.left + dot + self.scale(8);
        let title_width = self.text_width(hdc, &status.title);
        self.label_ellipsis(hdc, &status.title, title_left, top, self.theme.text, limit);
        if !status.detail.is_empty() {
            let detail_left = title_left + title_width + self.scale(6);
            if detail_left < text_right - self.scale(16) {
                self.label_ellipsis(
                    hdc,
                    &format!("\u{00b7} {}", status.detail),
                    detail_left,
                    top,
                    self.theme.muted,
                    limit,
                );
            }
        }
    }

    fn paint_debug_row(&self, hdc: HDC, row: &DebugRow, rect: RECT, hot: bool, clip: RECT) {
        let s = |value: i32| self.scale(value);
        let middle = (rect.top + rect.bottom) / 2;
        let top = middle - self.text_height(hdc) / 2;
        let size = s(16);
        let limit = RECT {
            right: rect.right - s(8),
            ..rect
        };
        match row {
            DebugRow::Header {
                section,
                title,
                count,
            } => {
                let expanded = self.debug_sections_expanded[match section {
                    DebugSection::Variables => 0,
                    DebugSection::CallStack => 1,
                    DebugSection::Breakpoints => 2,
                }];
                self.chevron(hdc, rect.left + s(12), middle, expanded);
                let title_left = rect.left + s(22);
                self.label_mid(
                    hdc,
                    title,
                    title_left,
                    middle,
                    blend(self.theme.muted, self.theme.text, 0.35),
                    clip,
                );
                let count_text = count.to_string();
                let count_width = self.text_width(hdc, &count_text);
                let pill_left = title_left + self.text_width(hdc, title) + s(6);
                let pill = RECT {
                    left: pill_left,
                    top: middle - s(8),
                    right: pill_left + (count_width + s(10)).max(s(18)),
                    bottom: middle + s(8),
                };
                Self::rounded_fill(hdc, pill, s(16), self.theme.edge);
                self.label_mid(
                    hdc,
                    &count_text,
                    (pill.left + pill.right - count_width) / 2,
                    middle,
                    self.theme.text,
                    pill,
                );
                let action = match section {
                    DebugSection::Variables => Some(DebugGlyph::CollapseAll),
                    DebugSection::Breakpoints => Some(DebugGlyph::Close),
                    DebugSection::CallStack => None,
                };
                if hot
                    && *count > 0
                    && let Some(glyph) = action
                {
                    let button = self.debug_row_button(rect, 0);
                    self.glyph_in(hdc, glyph, self.theme.muted, button, size);
                }
            }
            DebugRow::Note(text) => {
                self.label_ellipsis(
                    hdc,
                    text,
                    rect.left + s(DEBUG_INDENT),
                    top,
                    self.theme.muted,
                    limit,
                );
            }
            DebugRow::Scope(name) => {
                self.chevron(hdc, rect.left + s(12), middle, true);
                self.label_ellipsis(hdc, name, rect.left + s(26), top, self.theme.text, limit);
            }
            DebugRow::Variable {
                depth,
                reference,
                expanded,
                name,
                value,
            } => {
                if hot {
                    Self::fill(hdc, rect, self.debug_hover_fill());
                }
                let x = rect.left + s(DEBUG_INDENT + DEBUG_NEST * *depth as i32);
                if *reference != 0 {
                    self.chevron(hdc, x + size / 2, middle, *expanded);
                }
                let name_left = x + size + s(6);
                let name_color = blend(self.theme.violet, self.theme.text, 0.55);
                let name_width = self.text_width(hdc, name).min(limit.right - name_left);
                self.label_ellipsis(hdc, name, name_left, top, name_color, limit);
                let colon_left = name_left + name_width;
                self.label_mid(hdc, ":", colon_left, middle, self.theme.muted, limit);
                let value_left = colon_left + self.text_width(hdc, ":") + s(6);
                if value_left < limit.right - s(12) {
                    self.label_ellipsis(
                        hdc,
                        value,
                        value_left,
                        top,
                        self.debug_value_color(value),
                        limit,
                    );
                }
            }
            DebugRow::Loading(depth) => {
                let x = rect.left + s(DEBUG_INDENT + DEBUG_NEST * *depth as i32) + size + s(6);
                self.label_mid(hdc, "Loading\u{2026}", x, middle, self.theme.muted, limit);
            }
            DebugRow::Frame(index) => {
                let Some(frame) = self.debug_state.frames.get(*index) else {
                    return;
                };
                let current = *index == 0 && self.debug_paused();
                if current {
                    Self::fill(hdc, rect, blend(self.theme.sidebar_bg, rgb(PAUSED), 0.14));
                    self.icons.draw_glyph(
                        hdc,
                        DebugGlyph::ExecutionArrow,
                        rgb(PAUSED),
                        rect.left + s(3),
                        middle - s(7),
                        s(14),
                    );
                } else if hot {
                    Self::fill(hdc, rect, self.debug_hover_fill());
                }
                let location = frame
                    .path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| format!("{}:{}", name.to_string_lossy(), frame.line))
                    .unwrap_or_default();
                let location_width = self.text_width(hdc, &location);
                let location_left = limit.right - location_width;
                self.label_mid(
                    hdc,
                    &location,
                    location_left,
                    middle,
                    self.theme.muted,
                    rect,
                );
                self.label_ellipsis(
                    hdc,
                    &frame.name,
                    rect.left + s(DEBUG_INDENT),
                    top,
                    if current {
                        blend(rgb(PAUSED), self.theme.text, 0.6)
                    } else {
                        self.theme.text
                    },
                    RECT {
                        right: location_left - s(8),
                        ..rect
                    },
                );
            }
            DebugRow::Breakpoint { tab, line } => {
                let Some(path) = self
                    .tabs
                    .get(*tab)
                    .and_then(|tab| tab.document.path.as_ref())
                else {
                    return;
                };
                if hot {
                    Self::fill(hdc, rect, self.debug_hover_fill());
                }
                let dot = s(10);
                self.icons.draw_glyph(
                    hdc,
                    DebugGlyph::GutterDot,
                    rgb(BREAKPOINT),
                    rect.left + s(DEBUG_INDENT) + s(3),
                    middle - dot / 2,
                    dot,
                );
                let number = (line + 1).to_string();
                let number_left = limit.right - self.text_width(hdc, &number);
                self.label_mid(hdc, &number, number_left, middle, self.theme.muted, rect);
                let remove = self.debug_breakpoint_remove(rect, hdc, *line);
                let text_right = if hot {
                    self.glyph_in(hdc, DebugGlyph::Close, self.theme.muted, remove, size);
                    remove.left - s(4)
                } else {
                    number_left - s(8)
                };
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // The folder, from the workspace when the file is inside it.
                let folder = path
                    .parent()
                    .map(|parent| {
                        match self
                            .workspace_root
                            .as_deref()
                            .and_then(|root| parent.strip_prefix(root).ok())
                        {
                            Some(relative) => relative.to_string_lossy().replace('\\', "/"),
                            None => parent
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        }
                    })
                    .unwrap_or_default();
                let name_left = rect.left + s(DEBUG_INDENT) + s(22);
                let text_limit = RECT {
                    right: text_right,
                    ..rect
                };
                let name_width = self.text_width(hdc, &name).min(text_right - name_left);
                self.label_ellipsis(hdc, &name, name_left, top, self.theme.text, text_limit);
                let folder_left = name_left + name_width + s(6);
                if !folder.is_empty() && folder_left < text_right - s(16) {
                    self.label_ellipsis(
                        hdc,
                        &folder,
                        folder_left,
                        top,
                        self.theme.muted,
                        text_limit,
                    );
                }
            }
        }
    }

    /// A variable's value in the editor's syntax colours: strings, numbers,
    /// and true/false/None; anything else (a struct's summary) dimmed.
    fn debug_value_color(&self, value: &str) -> u32 {
        let value = value.trim();
        if value.starts_with('"') || value.starts_with('\'') {
            return self.theme.string;
        }
        if matches!(
            value,
            "true" | "false" | "True" | "False" | "None" | "null" | "nil"
        ) {
            return self.theme.keyword;
        }
        let number = value.strip_prefix('-').unwrap_or(value);
        if number.starts_with(|ch: char| ch.is_ascii_digit())
            && !number.contains(char::is_whitespace)
        {
            return self.theme.number;
        }
        self.theme.muted
    }

    fn paint_debug_scrollbar(
        &self,
        hdc: HDC,
        layout: &DebugLayout,
        count: usize,
        right: i32,
        bottom: i32,
    ) {
        let row = self.scale(EXPLORER_ROW).max(1);
        let total = row * count as i32;
        let viewport = (bottom - layout.list_top).max(1);
        if total <= viewport {
            return;
        }
        let track_top = layout.list_top + self.scale(2);
        let track_height = (bottom - self.scale(2) - track_top).max(1);
        let thumb = ((track_height as i64 * viewport as i64) / total as i64)
            .max(self.scale(24) as i64) as i32;
        let max_first = count.saturating_sub((viewport / row) as usize).max(1);
        let offset = (track_height - thumb).max(0) * self.debug_first.min(max_first) as i32
            / max_first as i32;
        Self::rounded_fill(
            hdc,
            RECT {
                left: right - self.scale(5),
                top: track_top + offset,
                right: right - self.scale(1),
                bottom: track_top + offset + thumb,
            },
            self.scale(2),
            blend(self.theme.edge, self.theme.muted, 0.3),
        );
    }
}
