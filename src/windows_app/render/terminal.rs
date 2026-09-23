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

        Self::label(
            hdc,
            "PROBLEMS  0",
            layout.problems.left,
            top + self.scale(9),
            self.theme.muted,
            layout.problems,
        );

        let output_active = self.terminal_tab == TerminalTab::Output;
        Self::label(
            hdc,
            "OUTPUT",
            layout.output.left,
            top + self.scale(9),
            if output_active { self.theme.text } else { self.theme.muted },
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
            let title = self
                .terminals
                .get(index)
                .map(|pane| pane.title.as_str())
                .unwrap_or("?");
            let label = if self.terminals.len() == 1 {
                "TERMINAL".to_string()
            } else {
                format!("TERMINAL {title}")
            };
            let active = self.terminal_tab == TerminalTab::Terminal && index == self.terminal_active;
            Self::label(
                hdc,
                &label,
                rect.left + self.scale(6),
                top + self.scale(9),
                if active { self.theme.text } else { self.theme.muted },
                *rect,
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
        Self::label(
            hdc,
            "+",
            layout.plus.left + self.scale(8),
            top + self.scale(8),
            self.theme.muted,
            layout.plus,
        );
        Self::label(
            hdc,
            "\u{2715}",
            layout.kill.left + self.scale(4),
            top + self.scale(9),
            if shell_open { self.theme.muted } else { rgb(74, 80, 94) },
            layout.kill,
        );

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
                    let (foreground, background) =
                        cell_colors(cell, self.theme.text, self.theme.sidebar_bg);
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
                    CURSOR_BG,
                );
            }
        }
        unsafe {
            SelectObject(hdc, old_font);
        }
    }
}
