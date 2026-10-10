use super::super::terminal::cell_colors;
use super::super::*;

// CURSOR uses a soft block color distinct from the palette so it stays visible on
// both default and colored cells.
const CURSOR_BG: u32 = rgb(158, 178, 214);

impl App {
    pub(in crate::windows_app) fn paint_terminal(
        &mut self,
        hdc: HDC,
        left: i32,
        right: i32,
        bottom: i32,
    ) {
        let top = bottom - self.scale(self.terminal_height);
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
                bottom: top + self.scale(1),
            },
            self.theme.edge,
        );
        self.refresh_terminal_cell_width(hdc);
        // The tab strip (OUTPUT, one tab per shell, add/kill buttons, panel
        // close) is laid out by the same helper input.rs hit-tests, so painted
        // rectangles and click targets can never drift apart.
        let layout = self.terminal_header_layout(left, right, top);
        let header_bottom = layout.header_bottom;

        let (errors, warnings) = self.problem_counts();
        Self::label(
            hdc,
            &format!("PROBLEMS  {}", errors + warnings),
            layout.problems.left,
            top + self.scale(9),
            if self.problems_shown {
                self.theme.text
            } else {
                self.theme.muted
            },
            layout.problems,
        );
        if self.problems_shown {
            Self::fill(
                hdc,
                RECT {
                    left: layout.problems.left,
                    top: header_bottom - self.scale(2),
                    right: layout.problems.right - self.scale(14),
                    bottom: header_bottom,
                },
                self.theme.violet,
            );
        }

        let output_active = self.terminal_tab == TerminalTab::Output && !self.problems_shown;
        Self::label(
            hdc,
            "OUTPUT",
            layout.output.left,
            top + self.scale(9),
            if output_active {
                self.theme.text
            } else {
                self.theme.muted
            },
            layout.output,
        );
        if output_active {
            Self::fill(
                hdc,
                RECT {
                    left: layout.output.left,
                    top: header_bottom - self.scale(2),
                    right: layout.output.right - self.scale(14),
                    bottom: header_bottom,
                },
                self.theme.violet,
            );
        }

        // One tab per interactive shell session, marked with a leading bullet.
        for (index, rect) in layout.terminals.iter().enumerate() {
            let pane = self.terminals.get(index);
            let shell_tag = pane
                .map(|p| p.shell_kind.tag().to_uppercase())
                .unwrap_or_else(|| "TERMINAL".into());
            let title = pane.map(|p| p.title.as_str()).unwrap_or("?");
            let label = if pane.is_some_and(|pane| pane.custom_title) {
                title.to_string()
            } else if self.terminals.len() == 1 {
                shell_tag
            } else {
                format!("{shell_tag} {title}")
            };
            let active = self.terminal_tab == TerminalTab::Terminal
                && index == self.terminal_active
                && !self.problems_shown;
            let clip = RECT {
                left: rect.left,
                top: rect.top,
                right: rect.right - self.scale(4),
                bottom: rect.bottom,
            };
            self.label_ellipsis(
                hdc,
                &label,
                rect.left + self.scale(6),
                top + self.scale(9),
                if active {
                    self.theme.text
                } else {
                    self.theme.muted
                },
                clip,
            );
            if active {
                Self::fill(
                    hdc,
                    RECT {
                        left: rect.left,
                        top: header_bottom - self.scale(2),
                        right: rect.right,
                        bottom: header_bottom,
                    },
                    self.theme.violet,
                );
            }
        }

        let shell_open = self.terminal_tab == TerminalTab::Terminal
            && self.terminal_active < self.terminals.len();
        self.panel_card(
            hdc,
            RECT {
                left: layout.plus.left,
                top: top + self.scale(5),
                right: layout.chevron.right,
                bottom: header_bottom - self.scale(5),
            },
            self.scale(4),
            self.theme.card_edge,
            ui(15, 28, 49),
        );
        Self::label(
            hdc,
            "+",
            layout.plus.left + self.scale(6),
            top + self.scale(8),
            self.theme.muted,
            layout.plus,
        );
        Self::label(
            hdc,
            "\u{25be}",
            layout.chevron.left + self.scale(3),
            top + self.scale(8),
            self.theme.muted,
            layout.chevron,
        );
        Self::label(
            hdc,
            "\u{2715}",
            layout.kill.left + self.scale(4),
            top + self.scale(9),
            if match self.terminal_tab {
                TerminalTab::Output => self.run_session.is_some(),
                TerminalTab::Terminal => shell_open,
            } {
                self.theme.muted
            } else {
                ui(74, 80, 94)
            },
            layout.kill,
        );

        if self.problems_shown {
            Self::label(
                hdc,
                "\u{d7}",
                layout.hide.left + self.scale(6),
                top + self.scale(6),
                self.theme.muted,
                layout.hide,
            );
            self.paint_problems(hdc, self.hwnd);
            if self.terminal_profile_menu_open {
                self.paint_terminal_profile_menu(hdc, left, right, top, bottom);
            }
            return;
        }
        let active_snapshot: Option<Arc<Snapshot>> = match self.terminal_tab {
            TerminalTab::Output => self.run_snapshot.clone(),
            TerminalTab::Terminal => self
                .terminals
                .get(self.terminal_active)
                .and_then(|pane| pane.snapshot.clone()),
        };
        let status = match &active_snapshot {
            Some(snapshot) => match &snapshot.status {
                SessionStatus::Exited { code } => format!("process exited ({code})"),
                SessionStatus::Failed(message) => format!("error: {message}"),
                SessionStatus::Stopped => "stopped".to_string(),
                SessionStatus::Starting => "starting\u{2026}".to_string(),
                SessionStatus::Stopping => "stopping\u{2026}".to_string(),
                SessionStatus::Running => String::new(),
            },
            None => String::new(),
        };
        if !status.is_empty() {
            let width = self.text_width(hdc, &status);
            let x = (layout.kill.right + self.scale(18))
                .min(right - self.scale(48) - width)
                .max(layout.kill.right);
            Self::label(
                hdc,
                &status,
                x,
                top + self.scale(10),
                self.theme.muted,
                RECT {
                    left,
                    top,
                    right: layout.hide.left,
                    bottom: header_bottom,
                },
            );
        }
        Self::label(
            hdc,
            "\u{d7}",
            layout.hide.left + self.scale(6),
            top + self.scale(6),
            self.theme.muted,
            layout.hide,
        );
        let Some(snapshot) = active_snapshot else {
            if self.terminal_profile_menu_open {
                self.paint_terminal_profile_menu(hdc, left, right, top, bottom);
            }
            return;
        };
        let cell_width = self.cell_width.max(1);
        let cell_height = self.line_height.max(1);
        let content_left = left + self.scale(8);
        let content_right = right - self.scale(8);
        let old_font = unsafe { SelectObject(hdc, self.font) };
        let clip = RECT {
            left,
            top: header_bottom,
            right,
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
        for (row_index, row) in snapshot.rows.iter().enumerate() {
            let y = header_bottom + row_index as i32 * cell_height;
            if y >= bottom {
                break;
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
                let mut run = String::new();
                for cell in &row.cells[column..end] {
                    if cell.wide_continuation {
                        continue;
                    }
                    if cell.text.is_empty() {
                        run.push(' ');
                    } else {
                        run.push_str(&cell.text);
                    }
                }
                if run.chars().any(|ch| ch != ' ') {
                    Self::label(hdc, &run, x, y, foreground, clip);
                }
                column = end;
            }
        }
        if self.terminal_focus && self.caret_on && snapshot.cursor.visible {
            let x = content_left + i32::from(snapshot.cursor.column) * cell_width;
            let y = header_bottom + i32::from(snapshot.cursor.row) * cell_height;
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
            Self::rounded_fill(hdc, menu, self.scale(4), self.theme.card_edge);
            Self::fill(
                hdc,
                RECT {
                    left: menu.left + self.scale(1),
                    top: menu.top + self.scale(1),
                    right: menu.right - self.scale(1),
                    bottom: menu.bottom - self.scale(1),
                },
                self.theme.sidebar_bg,
            );
            Self::label(
                hdc,
                "Rename Terminal",
                menu.left + self.scale(10),
                menu.top + self.scale(7),
                self.theme.text,
                menu,
            );
        }
        if let Some((session_id, name)) = &self.terminal_rename_input
            && let Some(index) = self
                .terminals
                .iter()
                .position(|pane| pane.id == *session_id)
            && let Some(tab) = layout.terminals.get(index)
        {
            let field = RECT {
                left: tab.left,
                top: header_bottom + self.scale(3),
                right: (tab.left + self.scale(230)).min(right - self.scale(8)),
                bottom: header_bottom + self.scale(33),
            };
            self.panel_card(
                hdc,
                field,
                self.scale(4),
                self.theme.card_edge,
                self.theme.sidebar_bg,
            );
            Self::label(
                hdc,
                name,
                field.left + self.scale(8),
                field.top + self.scale(7),
                self.theme.text,
                field,
            );
        }
        if self.terminal_profile_menu_open {
            self.paint_terminal_profile_menu(hdc, left, right, top, bottom);
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
        self.panel_card(hdc, menu, s(7), ui(47, 73, 112), ui(13, 25, 45));
        let title = if self.terminal_profile_defaults_open {
            "SELECT DEFAULT PROFILE"
        } else {
            "NEW TERMINAL"
        };
        Self::label(
            hdc,
            title,
            menu.left + s(12),
            menu.top + s(3),
            ui(129, 154, 194),
            menu,
        );

        let default_shell = self.settings.default_terminal_profile;
        for (shell, row) in &layout.shell_rows {
            let available = self.terminal_profile_shell_available(*shell);
            let is_default = *shell == default_shell;
            if is_default {
                Self::rounded_fill(hdc, *row, s(4), ui(24, 53, 94));
                Self::fill(
                    hdc,
                    RECT {
                        left: row.left,
                        top: row.top + s(2),
                        right: row.left + s(3),
                        bottom: row.bottom - s(2),
                    },
                    ui(40, 205, 225),
                );
            }
            let icon = RECT {
                left: row.left + s(9),
                top: row.top + s(3),
                right: row.left + s(29),
                bottom: row.bottom - s(3),
            };
            let text_color = if available {
                ui(226, 235, 249)
            } else {
                ui(91, 107, 132)
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
                let check = RECT {
                    left: row.right - s(76),
                    top: row.top + s(5),
                    right: row.right - s(62),
                    bottom: row.top + s(19),
                };
                Self::rounded_fill(hdc, check, s(4), ui(24, 104, 105));
                unsafe {
                    let pen = CreatePen(PS_SOLID, s(1).max(1), ui(172, 246, 229));
                    let old_pen = SelectObject(hdc, pen);
                    MoveToEx(hdc, check.left + s(3), check.top + s(7), null_mut());
                    LineTo(hdc, check.left + s(6), check.top + s(10));
                    LineTo(hdc, check.left + s(11), check.top + s(4));
                    SelectObject(hdc, old_pen);
                    DeleteObject(pen);
                }
                Self::label(
                    hdc,
                    "Default",
                    check.right + s(6),
                    row.top + s(3),
                    ui(158, 179, 213),
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
            ui(40, 57, 84),
        );
        if let Some(settings) = layout.settings {
            Self::rounded_fill(hdc, settings, s(4), ui(17, 34, 59));
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
                "Select Default Profile",
                settings.left + s(38),
                settings.top + s(4),
                ui(226, 235, 249),
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
