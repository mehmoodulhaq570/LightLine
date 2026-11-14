use super::super::terminal::{
    TERMINAL_PAD, TERMINAL_PAD_RIGHT, TerminalHeaderLayout, TerminalHot, cell_colors,
    is_powerline_separator, is_private_use, shell_exit_text,
};
use super::super::*;
use super::code_pane::blend;

// CURSOR uses a soft block color distinct from the palette so it stays visible on
// both default and colored cells.
const CURSOR_BG: u32 = rgb(158, 178, 214);

impl App {
    pub(in crate::windows_app) fn paint_terminal(
        &mut self,
        hwnd: HWND,
        hdc: HDC,
        left: i32,
        right: i32,
        bottom: i32,
    ) {
        // While it eases open or closed (motion.rs) the panel is drawn at
        // its full height from its eased top, and cut off at the status bar.
        let top = bottom - self.terminal_panel();
        let full_bottom = top + self.scale(self.terminal_height);
        unsafe {
            let saved = SaveDC(hdc);
            IntersectClipRect(hdc, left, top, right, bottom);
            self.paint_terminal_panel(hwnd, hdc, left, right, top, full_bottom);
            RestoreDC(hdc, saved);
        }
    }

    fn paint_terminal_panel(
        &mut self,
        hwnd: HWND,
        hdc: HDC,
        left: i32,
        right: i32,
        top: i32,
        bottom: i32,
    ) {
        let (dpi, zoom) = (self.dpi, self.zoom);
        let s = move |v: i32| scaled(v, dpi, zoom);
        Self::fill(
            hdc,
            RECT {
                left,
                top,
                right,
                bottom,
            },
            self.theme.sidebar_bg,
        );
        Self::fill(
            hdc,
            RECT {
                left,
                top,
                right,
                bottom: top + s(1),
            },
            self.theme.edge,
        );
        self.refresh_terminal_cell_width(hdc);
        // The header is laid out by the same helper input.rs hit-tests, so
        // painted rectangles and click targets can never drift apart.
        let layout = self.terminal_header_layout(left, right, top);
        let header_bottom = layout.header_bottom;
        self.paint_terminal_header(hdc, &layout);

        let problems_active = self.terminal_tab == TerminalTab::Problems;
        let debug_active = self.terminal_tab == TerminalTab::DebugConsole;
        if problems_active {
            self.paint_problems(hdc, hwnd);
            if let Some((track, slider)) = self.problems_scrollbar(hwnd) {
                let edge = s(1).max(1);
                Self::fill(hdc, track, self.theme.editor_bg);
                Self::fill(
                    hdc,
                    RECT {
                        right: track.left + edge,
                        ..track
                    },
                    self.theme.edge,
                );
                let strength = if self.problem_scrollbar_grab.is_some() {
                    0.4
                } else if self.problem_scrollbar_hover {
                    0.3
                } else {
                    0.2
                };
                Self::fill(
                    hdc,
                    RECT {
                        left: track.left + edge,
                        ..slider
                    },
                    blend(self.theme.editor_bg, self.theme.text, strength),
                );
            }
            if self.terminal_profile_menu_open {
                self.paint_terminal_profile_menu(hdc, left, right, top, bottom);
            }
            return;
        }
        if debug_active {
            self.paint_debug_console(hdc, hwnd);
            if self.terminal_profile_menu_open {
                self.paint_terminal_profile_menu(hdc, left, right, top, bottom);
            }
            return;
        }

        // The shells' list at the side, from two shells on.
        let list = self.shell_list_rect(right, top, bottom);
        if let Some(list) = list {
            self.paint_shell_list(hdc, list);
        }

        let active_snapshot: Option<Arc<Snapshot>> = match self.terminal_tab {
            TerminalTab::Output => self.run_snapshot.clone(),
            TerminalTab::Terminal => self
                .terminals
                .get(self.terminal_active)
                .and_then(|pane| pane.snapshot.clone()),
            TerminalTab::Problems | TerminalTab::DebugConsole => None,
        };
        let Some(snapshot) = active_snapshot else {
            if self.terminal_profile_menu_open {
                self.paint_terminal_profile_menu(hdc, left, right, top, bottom);
            }
            return;
        };
        let cell_width = self.cell_width.max(1);
        let cell_height = self.line_height.max(1);
        let content_left = left + s(TERMINAL_PAD);
        let content_right = list.map_or(right, |list| list.left) - s(TERMINAL_PAD_RIGHT);
        let text_top = header_bottom + s(2);
        let symbol_font = if snapshot_has_private_use(&snapshot) {
            self.terminal_symbol_font()
        } else {
            None
        };
        let old_font = unsafe { SelectObject(hdc, self.font) };
        let clip = RECT {
            left,
            top: header_bottom,
            right: content_right + s(TERMINAL_PAD_RIGHT) / 2,
            bottom,
        };
        let selection = self.terminal_selection_range();
        let cell_selected = |row_index: usize, column: usize| {
            let Some((start, finish)) = selection else {
                return false;
            };
            // Tuple comparison is lexicographic (row, then column), which is
            // exactly reading-order: "at or after start, at or before end."
            let point = (row_index as u16, column as u16);
            point >= (start.1, start.0) && point <= (finish.1, finish.0)
        };
        let mut last_text_row = 0usize;
        for (row_index, row) in snapshot.rows.iter().enumerate() {
            let y = text_top + row_index as i32 * cell_height;
            if y >= bottom {
                break;
            }
            if row.cells.iter().any(|cell| !cell.text.trim().is_empty()) {
                last_text_row = row_index;
            }
            let colors: Vec<(u32, u32)> = row
                .cells
                .iter()
                .enumerate()
                .map(|(column, cell)| {
                    let (foreground, background) = cell_colors(
                        cell,
                        self.theme.text,
                        self.theme.sidebar_bg,
                        &self.theme.ansi,
                    );
                    if cell_selected(row_index, column) {
                        (foreground, self.theme.select_bg)
                    } else {
                        (foreground, background)
                    }
                })
                .collect();
            let mut column = 0usize;
            while column < row.cells.len() {
                let (foreground, background) = colors[column];
                let mut end = column + 1;
                while end < row.cells.len() && colors[end] == (foreground, background) {
                    end += 1;
                }
                let x = content_left + column as i32 * cell_width;
                if background != self.theme.sidebar_bg {
                    Self::fill(
                        hdc,
                        RECT {
                            left: x,
                            top: y,
                            right: (content_left + end as i32 * cell_width).min(content_right),
                            bottom: (y + cell_height).min(bottom),
                        },
                        background,
                    );
                }
                // Text in runs; Powerline separators drawn as shapes and
                // other private-use symbols in the symbol font, one cell each.
                let mut run = String::new();
                let mut run_start = column;
                for (offset, cell) in row.cells[column..end].iter().enumerate() {
                    let at = column + offset;
                    if cell.wide_continuation {
                        continue;
                    }
                    let ch = cell.text.chars().next().unwrap_or(' ');
                    let special =
                        is_powerline_separator(ch) || (symbol_font.is_some() && is_private_use(ch));
                    if !special {
                        if run.is_empty() {
                            run_start = at;
                        }
                        if cell.text.is_empty() {
                            run.push(' ');
                        } else {
                            run.push_str(&cell.text);
                        }
                        continue;
                    }
                    if run.chars().any(|ch| ch != ' ') {
                        let run_x = content_left + run_start as i32 * cell_width;
                        Self::label(hdc, &run, run_x, y, foreground, clip);
                    }
                    run.clear();
                    let cell_rect = RECT {
                        left: content_left + at as i32 * cell_width,
                        top: y,
                        right: content_left + (at as i32 + 1) * cell_width,
                        bottom: y + cell_height,
                    };
                    if is_powerline_separator(ch) {
                        Self::paint_powerline(hdc, ch, cell_rect, foreground);
                    } else if let Some(font) = symbol_font {
                        unsafe { SelectObject(hdc, font) };
                        Self::label(hdc, &cell.text, cell_rect.left, y, foreground, clip);
                        unsafe { SelectObject(hdc, self.font) };
                    }
                }
                if run.chars().any(|ch| ch != ' ') {
                    let run_x = content_left + run_start as i32 * cell_width;
                    Self::label(hdc, &run, run_x, y, foreground, clip);
                }
                column = end;
            }
        }
        // An exited shell says so under its last line, as in VS Code.
        if self.terminal_tab == TerminalTab::Terminal
            && let Some(exit) = self.active_shell_exit()
            && let Some(pane) = self.terminals.get(self.terminal_active)
        {
            let y = text_top + (last_text_row as i32 + 2) * cell_height;
            if y + cell_height <= bottom {
                let note = format!(
                    "{} exited with {exit}. Press Enter to close this terminal.",
                    pane.shell_kind.tag()
                );
                Self::label(hdc, &note, content_left, y, self.theme.muted, clip);
            }
        }
        if self.terminal_focus
            && self.caret_on
            && snapshot.cursor.visible
            && !snapshot.status.is_final()
        {
            let x = content_left + i32::from(snapshot.cursor.column) * cell_width;
            let y = text_top + i32::from(snapshot.cursor.row) * cell_height;
            if y + cell_height <= bottom && x + cell_width <= content_right {
                Self::fill(
                    hdc,
                    RECT {
                        left: x,
                        top: y,
                        right: x + cell_width,
                        bottom: y + cell_height,
                    },
                    themed(CURSOR_BG),
                );
            }
        }
        unsafe {
            SelectObject(hdc, old_font);
        }
        if let Some((_, menu)) = self.terminal_context_menu {
            Self::rounded_fill(hdc, menu, s(4), self.theme.card_edge);
            Self::fill(
                hdc,
                RECT {
                    left: menu.left + s(1),
                    top: menu.top + s(1),
                    right: menu.right - s(1),
                    bottom: menu.bottom - s(1),
                },
                self.theme.sidebar_bg,
            );
            Self::label(
                hdc,
                "Rename Terminal",
                menu.left + s(10),
                menu.top + s(7),
                self.theme.text,
                menu,
            );
        }
        if let Some((_, name)) = &self.terminal_rename_input
            && let Some(field) = self.terminal_rename_field_rect(hwnd)
        {
            self.panel_card(hdc, field, s(4), self.theme.blue, self.theme.sidebar_bg);
            self.label_mid(
                hdc,
                name,
                field.left + s(8),
                (field.top + field.bottom) / 2,
                self.theme.text,
                field,
            );
        }
        if self.terminal_profile_menu_open {
            self.paint_terminal_profile_menu(hdc, left, right, top, bottom);
        }
    }

    /// The header: four tabs in the small font (PROBLEMS with its count
    /// badge), a line under the open one, and the toolbar at the right.
    fn paint_terminal_header(&self, hdc: HDC, layout: &TerminalHeaderLayout) {
        let s = |v: i32| self.scale(v);
        let header_bottom = layout.header_bottom;
        let middle = (layout.problems.top + header_bottom) / 2;
        let old = unsafe { SelectObject(hdc, self.small_font) };
        let (errors, warnings) = self.problem_counts();
        let tabs = [
            (layout.problems, "PROBLEMS", TerminalTab::Problems),
            (layout.output, "OUTPUT", TerminalTab::Output),
            (layout.debug, "DEBUG CONSOLE", TerminalTab::DebugConsole),
            (layout.terminal, "TERMINAL", TerminalTab::Terminal),
        ];
        for (index, (rect, label, tab)) in tabs.into_iter().enumerate() {
            let active = self.terminal_tab == tab;
            let hot = self.hot == Some(Hot::Terminal(TerminalHot::Tab(index)));
            let color = if active || hot {
                self.theme.text
            } else {
                self.theme.muted
            };
            self.label_mid(hdc, label, rect.left + s(10), middle, color, rect);
            if tab == TerminalTab::Problems {
                let count = (errors + warnings).to_string();
                let width = self.text_width(hdc, &count);
                let badge_left = rect.left + s(10) + self.text_width(hdc, label) + s(6);
                let badge = RECT {
                    left: badge_left,
                    top: middle - s(8),
                    right: badge_left + (width + s(8)).max(s(16)),
                    bottom: middle + s(8),
                };
                Self::rounded_fill(hdc, badge, s(16), self.theme.edge);
                self.label_mid(
                    hdc,
                    &count,
                    (badge.left + badge.right - width) / 2,
                    middle,
                    self.theme.text,
                    badge,
                );
            }
            if active {
                Self::fill(
                    hdc,
                    RECT {
                        left: rect.left + s(10),
                        top: header_bottom - s(6),
                        right: rect.right - s(10),
                        bottom: header_bottom - s(5),
                    },
                    self.theme.violet,
                );
            }
        }
        unsafe { SelectObject(hdc, old) };

        // Output: what's running, beside Stop.
        if self.terminal_tab == TerminalTab::Output && self.run_session.is_some() {
            let label = match &self.run_label {
                Some(name) => format!("Running {name}"),
                None => "Running".to_owned(),
            };
            let width = self.text_width(hdc, &label);
            let x = layout.kill.left - s(8) - width;
            if x > layout.terminal.right + s(16) {
                let dot = s(7);
                self.icons.draw_glyph(
                    hdc,
                    DebugGlyph::GutterDot,
                    self.theme.green,
                    x - s(13),
                    middle - dot / 2,
                    dot,
                );
                self.label_mid(
                    hdc,
                    &label,
                    x,
                    middle,
                    self.theme.muted,
                    RECT {
                        left: x,
                        right: layout.kill.left,
                        ..layout.kill
                    },
                );
            }
        }

        let running = self.terminal_tab == TerminalTab::Output && self.run_session.is_some();
        let shell_open = self.terminal_tab == TerminalTab::Terminal
            && self.terminal_active < self.terminals.len();
        let buttons = [
            (layout.plus, TerminalHot::New, DebugGlyph::Plus, true),
            (
                layout.chevron,
                TerminalHot::ShellPicker,
                DebugGlyph::ChevronDown,
                true,
            ),
            (
                layout.kill,
                TerminalHot::Kill,
                if self.terminal_tab == TerminalTab::Output {
                    DebugGlyph::Stop
                } else {
                    DebugGlyph::Trash
                },
                running || shell_open,
            ),
            (
                layout.maximize,
                TerminalHot::Maximize,
                if self.terminal_restore_height.is_some() {
                    DebugGlyph::ChevronDown
                } else {
                    DebugGlyph::ChevronUp
                },
                true,
            ),
            (layout.hide, TerminalHot::Hide, DebugGlyph::Close, true),
        ];
        for (rect, hot, glyph, enabled) in buttons {
            if rect.right <= rect.left {
                continue;
            }
            let lit = enabled && self.hot == Some(Hot::Terminal(hot));
            if lit {
                Self::rounded_fill(
                    hdc,
                    rect,
                    s(4),
                    self.hot_color(self.theme.sidebar_bg, self.theme.active_bg),
                );
            }
            let color = if !enabled {
                blend(self.theme.edge, self.theme.muted, 0.3)
            } else if glyph == DebugGlyph::Stop {
                self.theme.error
            } else if lit {
                self.theme.text
            } else {
                self.theme.muted
            };
            let size = if glyph == DebugGlyph::Stop {
                s(13)
            } else {
                s(16)
            };
            self.glyph_in(hdc, glyph, color, rect, size);
        }
        // The line between the tab's own buttons and maximize/close.
        if layout.kill.right > layout.kill.left {
            Self::fill(
                hdc,
                RECT {
                    left: layout.separator,
                    top: middle - s(8),
                    right: layout.separator + 1,
                    bottom: middle + s(8),
                },
                self.theme.edge,
            );
        }
    }

    /// The shells, one 22 px row each: its shell's icon, its name, how it
    /// ended if it has, and a 🗑 under the mouse.
    fn paint_shell_list(&self, hdc: HDC, list: RECT) {
        let s = |v: i32| self.scale(v);
        Self::fill(
            hdc,
            RECT {
                right: list.left + 1,
                ..list
            },
            self.theme.edge,
        );
        for (index, pane) in self.terminals.iter().enumerate() {
            let row = self.shell_row_rect(list, index);
            if row.bottom > list.bottom {
                break;
            }
            let active = index == self.terminal_active;
            let hot = self.hot == Some(Hot::Terminal(TerminalHot::Shell(index)));
            if active {
                Self::fill(hdc, row, self.theme.select_bg);
                Self::fill(
                    hdc,
                    RECT {
                        right: row.left + s(2),
                        ..row
                    },
                    self.theme.violet,
                );
            } else if hot {
                Self::fill(
                    hdc,
                    row,
                    self.hot_color(self.theme.sidebar_bg, self.theme.active_bg),
                );
            }
            let middle = (row.top + row.bottom) / 2;
            let icon = RECT {
                left: row.left + s(10),
                top: middle - s(8),
                right: row.left + s(26),
                bottom: middle + s(8),
            };
            self.paint_terminal_profile_shell_icon(hdc, pane.shell_kind, icon, true);
            let exit = shell_exit_text(pane);
            let name = if pane.custom_title {
                pane.title.clone()
            } else {
                // "pwsh", then "pwsh 2" for a second of the same kind.
                let same_before = self.terminals[..index]
                    .iter()
                    .filter(|other| other.shell_kind == pane.shell_kind && !other.custom_title)
                    .count();
                if same_before == 0 {
                    pane.shell_kind.tag().to_owned()
                } else {
                    format!("{} {}", pane.shell_kind.tag(), same_before + 1)
                }
            };
            let mut text_right = row.right - s(8);
            if hot {
                let kill = self.shell_row_kill(row);
                self.glyph_in(hdc, DebugGlyph::Trash, self.theme.muted, kill, s(16));
                text_right = kill.left - s(4);
            } else if let Some(exit) = &exit {
                let short = match exit.strip_prefix("code ") {
                    Some(code) => format!("exited ({code})"),
                    None => "exited".to_owned(),
                };
                let width = self.text_width(hdc, &short);
                self.label_mid(
                    hdc,
                    &short,
                    text_right - width,
                    middle,
                    self.theme.error,
                    row,
                );
                text_right -= width + s(6);
            }
            let color = if exit.is_some() {
                self.theme.muted
            } else if active || hot {
                self.theme.text
            } else {
                self.theme.muted
            };
            self.label_ellipsis(
                hdc,
                &name,
                icon.right + s(7),
                middle - self.text_height(hdc) / 2,
                color,
                RECT {
                    right: text_right,
                    ..row
                },
            );
        }
    }

    /// A Powerline separator drawn as a shape filling its cell, so the
    /// prompt's segments join without a Nerd Font: solid and thin arrows
    /// (U+E0B0–E0B3) and half circles (U+E0B4–E0B7).
    fn paint_powerline(hdc: HDC, ch: char, cell: RECT, color: u32) {
        let (l, t, r, b) = (cell.left, cell.top, cell.right, cell.bottom);
        let mid = (t + b) / 2;
        unsafe {
            let brush = CreateSolidBrush(color);
            let pen = CreatePen(PS_SOLID, 1, color);
            let old_brush = SelectObject(hdc, brush);
            let old_pen = SelectObject(hdc, pen);
            match ch {
                '\u{E0B0}' => {
                    let points = [
                        POINT { x: l, y: t },
                        POINT { x: r, y: mid },
                        POINT { x: l, y: b },
                    ];
                    Polygon(hdc, points.as_ptr(), 3);
                }
                '\u{E0B2}' => {
                    let points = [
                        POINT { x: r, y: t },
                        POINT { x: l, y: mid },
                        POINT { x: r, y: b },
                    ];
                    Polygon(hdc, points.as_ptr(), 3);
                }
                '\u{E0B1}' => {
                    MoveToEx(hdc, l, t, null_mut());
                    LineTo(hdc, r - 1, mid);
                    LineTo(hdc, l - 1, b);
                }
                '\u{E0B3}' => {
                    MoveToEx(hdc, r - 1, t, null_mut());
                    LineTo(hdc, l, mid);
                    LineTo(hdc, r, b);
                }
                // Half circles: the right half of an ellipse twice the cell's
                // width, and the left half.
                '\u{E0B4}' => {
                    Pie(hdc, l - (r - l), t, r, b, l, b, l, t);
                }
                '\u{E0B6}' => {
                    Pie(hdc, l, t, r + (r - l), b, r, t, r, b);
                }
                '\u{E0B5}' => {
                    Arc(hdc, l - (r - l), t, r, b, l, b, l, t);
                }
                _ => {
                    Arc(hdc, l, t, r + (r - l), b, r, t, r, b);
                }
            }
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old_pen);
            DeleteObject(brush);
            DeleteObject(pen);
        }
    }

    fn paint_terminal_profile_menu(&self, hdc: HDC, left: i32, right: i32, top: i32, bottom: i32) {
        let s = |v: i32| self.scale(v);
        let layout = self.terminal_profile_menu_layout(left, right, top, bottom);
        let menu = layout.rect;
        Self::rounded_fill(
            hdc,
            RECT {
                left: menu.left + s(4),
                top: menu.top + s(5),
                right: menu.right + s(4),
                bottom: menu.bottom + s(5),
            },
            s(7),
            ui(4, 9, 18),
        );
        self.panel_card(hdc, menu, s(6), self.theme.edge, self.theme.sidebar_bg);
        let title = if self.terminal_profile_defaults_open {
            "SELECT DEFAULT PROFILE"
        } else {
            "NEW TERMINAL"
        };
        let old_font = unsafe { SelectObject(hdc, self.small_font) };
        Self::label(
            hdc,
            title,
            menu.left + s(12),
            menu.top + s(5),
            self.theme.muted,
            menu,
        );
        unsafe { SelectObject(hdc, old_font) };

        let default_shell = self.settings.default_terminal_profile;
        for (shell, row) in &layout.shell_rows {
            let available = self.terminal_profile_shell_available(*shell);
            let is_default = *shell == default_shell;
            if is_default {
                Self::rounded_fill(hdc, *row, s(4), self.theme.select_bg);
            }
            let icon = RECT {
                left: row.left + s(9),
                top: row.top + s(3),
                right: row.left + s(29),
                bottom: row.bottom - s(3),
            };
            let text_color = if available {
                self.theme.text
            } else {
                self.theme.muted
            };
            self.paint_terminal_profile_shell_icon(hdc, *shell, icon, available);
            Self::label(
                hdc,
                shell.name(),
                row.left + s(38),
                row.top + s(3),
                text_color,
                *row,
            );
            if !available {
                let label = "Not installed";
                let width = self.text_width(hdc, label);
                Self::label(
                    hdc,
                    label,
                    row.right - width - s(10),
                    row.top + s(3),
                    ui(91, 107, 132),
                    *row,
                );
            } else if is_default {
                let label = "Default";
                let width = self.text_width(hdc, label);
                Self::label(
                    hdc,
                    label,
                    row.right - width - s(10),
                    row.top + s(3),
                    self.theme.muted,
                    *row,
                );
            }
        }

        let after_shells = layout
            .shell_rows
            .last()
            .map(|(_, row)| row.bottom)
            .unwrap_or(menu.top + s(24));
        Self::fill(
            hdc,
            RECT {
                left: menu.left + s(10),
                top: after_shells + s(1),
                right: menu.right - s(10),
                bottom: after_shells + s(2),
            },
            self.theme.edge,
        );
        if let Some(settings) = layout.settings {
            Self::label(
                hdc,
                "\u{2699}",
                settings.left + s(8),
                settings.top + s(3),
                ui(205, 220, 242),
                settings,
            );
            Self::label(
                hdc,
                "Select Default Profile…",
                settings.left + s(38),
                settings.top + s(4),
                self.theme.text,
                settings,
            );
            Self::label(
                hdc,
                "\u{203a}",
                settings.right - s(16),
                settings.top + s(3),
                ui(158, 179, 213),
                settings,
            );
        } else {
            Self::rounded_fill(hdc, layout.footer, s(4), ui(17, 34, 59));
            Self::label(
                hdc,
                "\u{2039}",
                layout.footer.left + s(8),
                layout.footer.top + s(3),
                ui(158, 179, 213),
                layout.footer,
            );
            Self::label(
                hdc,
                "Back",
                layout.footer.left + s(28),
                layout.footer.top + s(4),
                ui(205, 220, 242),
                layout.footer,
            );
        }
    }

    fn paint_terminal_profile_shell_icon(
        &self,
        hdc: HDC,
        shell: ShellKind,
        icon: RECT,
        available: bool,
    ) {
        let s = |v: i32| self.scale(v);
        let muted = ui(75, 91, 116);
        if shell == ShellKind::GitBash {
            self.rail_icon(
                hdc,
                2,
                icon.left + s(2),
                icon.top + s(1),
                if available { ui(244, 91, 57) } else { muted },
            );
            return;
        }

        let (border, background, glyph) = match shell {
            ShellKind::PowerShell if available => (
                ui(70, 161, 232),
                ui(27, 103, 175),
                label_on(ui(27, 103, 175), 247, 251, 255),
            ),
            ShellKind::CommandPrompt if available => (
                ui(116, 137, 168),
                ui(15, 23, 36),
                label_on(ui(15, 23, 36), 230, 237, 247),
            ),
            ShellKind::Wsl if available => (ui(77, 184, 137), ui(15, 52, 44), ui(193, 242, 216)),
            _ => (ui(65, 79, 101), ui(18, 27, 41), muted),
        };
        self.panel_card(hdc, icon, s(3), border, background);

        unsafe {
            let pen = CreatePen(PS_SOLID, s(2).max(1), glyph);
            let old_pen = SelectObject(hdc, pen);
            let left = icon.left;
            let top = icon.top;
            match shell {
                ShellKind::PowerShell => {
                    MoveToEx(hdc, left + s(5), top + s(5), null_mut());
                    LineTo(hdc, left + s(9), top + s(9));
                    LineTo(hdc, left + s(5), top + s(13));
                    MoveToEx(hdc, left + s(10), top + s(13), null_mut());
                    LineTo(hdc, left + s(15), top + s(13));
                }
                ShellKind::CommandPrompt => {
                    MoveToEx(hdc, left + s(4), top + s(6), null_mut());
                    LineTo(hdc, left + s(8), top + s(9));
                    LineTo(hdc, left + s(4), top + s(12));
                    MoveToEx(hdc, left + s(10), top + s(12), null_mut());
                    LineTo(hdc, left + s(15), top + s(12));
                }
                ShellKind::Wsl => {
                    MoveToEx(hdc, left + s(4), top + s(5), null_mut());
                    LineTo(hdc, left + s(8), top + s(9));
                    LineTo(hdc, left + s(4), top + s(13));
                    MoveToEx(hdc, left + s(10), top + s(13), null_mut());
                    LineTo(hdc, left + s(15), top + s(13));
                    MoveToEx(hdc, left + s(4), top + s(3), null_mut());
                    LineTo(hdc, left + s(15), top + s(3));
                }
                ShellKind::GitBash => {}
            }
            SelectObject(hdc, old_pen);
            DeleteObject(pen);
        }
    }
}

/// Whether any cell shows a private-use character, so the symbol font is
/// looked up only for a terminal that needs it.
fn snapshot_has_private_use(snapshot: &Snapshot) -> bool {
    snapshot.rows.iter().any(|row| {
        row.cells
            .iter()
            .any(|cell| cell.text.chars().next().is_some_and(is_private_use))
    })
}
