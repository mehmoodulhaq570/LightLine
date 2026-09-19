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
        let top = bottom - self.scale(210);
        Self::fill(
            hdc,
            RECT {
                left,
                top,
                right,
                bottom,
            },
            SIDEBAR_BG,
        );
        Self::fill(
            hdc,
            RECT {
                left,
                top,
                right,
                bottom: top + self.scale(1),
            },
            EDGE,
        );
        self.refresh_terminal_cell_width(hdc);
        let header_bottom = top + self.scale(34);
        let (title, status) = match &self.terminal_snapshot {
            Some(snapshot) => {
                let title = if snapshot.title.is_empty() {
                    "TERMINAL".to_string()
                } else {
                    snapshot.title.clone()
                };
                let status = match &snapshot.status {
                    SessionStatus::Exited { code } => format!("process exited ({code})"),
                    SessionStatus::Failed(message) => format!("error: {message}"),
                    SessionStatus::Stopped => "stopped".to_string(),
                    SessionStatus::Starting => "starting\u{2026}".to_string(),
                    SessionStatus::Stopping => "stopping\u{2026}".to_string(),
                    SessionStatus::Running => String::new(),
                };
                (title, status)
            }
            None => ("TERMINAL".to_string(), String::new()),
        };
        Self::label(
            hdc,
            &title,
            left + self.scale(16),
            top + self.scale(9),
            TEXT,
            RECT {
                left,
                top,
                right: right - self.scale(44),
                bottom: header_bottom,
            },
        );
        if !status.is_empty() {
            let width = self.text_width(hdc, &status);
            Self::label(
                hdc,
                &status,
                right - self.scale(48) - width,
                top + self.scale(10),
                MUTED,
                RECT {
                    left,
                    top,
                    right: right - self.scale(44),
                    bottom: header_bottom,
                },
            );
        }
        Self::label(
            hdc,
            "\u{d7}",
            right - self.scale(28),
            top + self.scale(6),
            MUTED,
            RECT {
                left: right - self.scale(30),
                top,
                right,
                bottom: header_bottom,
            },
        );
        let Some(snapshot) = self.terminal_snapshot.clone() else {
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
        for (row_index, row) in snapshot.rows.iter().enumerate() {
            let y = header_bottom + row_index as i32 * cell_height;
            if y >= bottom {
                break;
            }
            let colors: Vec<(u32, u32)> = row.cells.iter().map(cell_colors).collect();
            let mut column = 0usize;
            while column < row.cells.len() {
                let (foreground, background) = colors[column];
                let mut end = column + 1;
                while end < row.cells.len() && colors[end] == (foreground, background) {
                    end += 1;
                }
                let x = content_left + column as i32 * cell_width;
                if background != SIDEBAR_BG {
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
