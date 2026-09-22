use super::super::git::{GitLayout, GitRow, GitSection, ROW_BUTTON, ROW_CHANGE, ROW_COMMIT};
use super::super::*;

// Row action glyphs. `+` stages, `-` unstages, and the undo arrow discards.
const REFRESH_GLYPH: &str = "\u{21bb}";
// Labels for `GitLayout::sync`, left to right.
const SYNC_LABELS: [&str; 3] = ["Push", "Pull", "Fetch"];

impl App {
    pub(in crate::windows_app) fn paint_git_review(
        &self,
        hdc: HDC,
        left: i32,
        right: i32,
        editor_bottom: i32,
        clip: RECT,
    ) {
        let layout = self.git_layout(left, right);
        // The panel header already carries the "SOURCE CONTROL" title, so only
        // the refresh affordance belongs in the strip below it.
        self.git_glyph(hdc, REFRESH_GLYPH, &layout.refresh, self.theme.muted, clip);

        self.paint_commit_box(hdc, layout.commit_box, clip);
        self.paint_commit_button(hdc, &layout, clip);
        for (index, rect) in layout.sync.iter().enumerate() {
            self.paint_git_tool(hdc, rect, SYNC_LABELS[index], clip);
        }

        let rows = self.git_rows();
        let mut top = layout.list_top;
        for (index, row) in rows.iter().enumerate().skip(self.panel_first) {
            let height = self.scale(row.height());
            if top >= editor_bottom {
                break;
            }
            let rect = RECT {
                left,
                top,
                right,
                bottom: top + height,
            };
            let selected = self.panel_focus && index == self.panel_selected;
            match row {
                GitRow::Header {
                    title,
                    count,
                    section,
                } => {
                    Self::label(
                        hdc,
                        &format!("{title}  {count}"),
                        left + self.scale(14),
                        top + self.scale(4),
                        self.theme.muted,
                        clip,
                    );
                    let glyph = match section {
                        GitSection::Staged => "-",
                        GitSection::Changes => "+",
                        GitSection::History => "",
                    };
                    if !glyph.is_empty() {
                        let edge = right - self.scale(8);
                        let size = self.scale(ROW_BUTTON);
                        let button = RECT {
                            left: edge - size,
                            top: rect.top,
                            right: edge,
                            bottom: rect.bottom,
                        };
                        self.git_glyph(hdc, glyph, &button, self.theme.muted, clip);
                    }
                }
                GitRow::Change { change, staged } => {
                    let open = self.review_file.as_ref() == Some(&change.path)
                        && self.review_staged == *staged;
                    if selected || open {
                        Self::fill(
                            hdc,
                            RECT {
                                left: left + self.scale(7),
                                top,
                                right: right - self.scale(7),
                                bottom: top + self.scale(ROW_CHANGE - 2),
                            },
                            self.theme.select_bg,
                        );
                    }
                    let color = if change.unmerged {
                        rgb(240, 150, 90)
                    } else if change.untracked {
                        self.theme.muted
                    } else if *staged {
                        self.theme.violet
                    } else {
                        self.theme.green
                    };
                    // The status word reads better than a two character plumbing
                    // code, and the name stays the primary thing in the row, so
                    // the word sits just left of the action buttons.
                    let word = change.label();
                    let word_width = self.text_width(hdc, word);
                    let buttons_left = right - self.scale(60);
                    Self::label(
                        hdc,
                        word,
                        buttons_left - word_width,
                        top,
                        color,
                        clip,
                    );
                    self.label_ellipsis(
                        hdc,
                        &display_path(&change.path),
                        left + self.scale(14),
                        top,
                        self.theme.text,
                        RECT {
                            left: left + self.scale(14),
                            top,
                            right: buttons_left - word_width - self.scale(10),
                            bottom: top + height,
                        },
                    );
                    let (toggle, discard) = self.git_row_buttons(right, rect, *staged);
                    let busy = if self.git_busy { self.theme.edge } else { self.theme.muted };
                    self.git_glyph(hdc, if *staged { "-" } else { "+" }, &toggle, busy, clip);
                    if let Some(discard) = discard {
                        self.git_glyph(hdc, "\u{21b3}", &discard, busy, clip);
                    }
                }
                GitRow::Commit(entry) => {
                    if selected {
                        Self::fill(
                            hdc,
                            RECT {
                                left: left + self.scale(7),
                                top,
                                right: right - self.scale(7),
                                bottom: top + self.scale(ROW_COMMIT - 2),
                            },
                            self.theme.select_bg,
                        );
                    }
                    self.label_ellipsis(
                        hdc,
                        &entry.subject,
                        left + self.scale(14),
                        top + self.scale(2),
                        self.theme.text,
                        RECT {
                            left: left + self.scale(14),
                            top,
                            right,
                            bottom: top + height,
                        },
                    );
                    Self::label(
                        hdc,
                        &format!("{}  ·  {}  ·  {}", entry.oid, entry.author, entry.date),
                        left + self.scale(14),
                        top + self.scale(17),
                        self.theme.muted,
                        clip,
                    );
                }
                GitRow::Note(text) => {
                    Self::label(hdc, text, left + self.scale(14), top, self.theme.muted, clip);
                }
            }
            top = rect.bottom;
        }
    }

    fn paint_commit_box(&self, hdc: HDC, box_rect: RECT, clip: RECT) {
        Self::fill(hdc, box_rect, self.theme.active_bg);
        let focused = self.commit_focus && self.focused;
        if focused {
            let edge = RECT {
                left: box_rect.left,
                top: box_rect.bottom - 1,
                right: box_rect.right,
                bottom: box_rect.bottom,
            };
            Self::fill(hdc, edge, self.theme.blue);
        }
        let empty = self.commit_message.is_empty();
        let text = if empty {
            "Message".to_owned()
        } else {
            self.commit_message.clone()
        };
        Self::label(
            hdc,
            &text,
            box_rect.left + self.scale(10),
            box_rect.top + self.scale(8),
            if empty { self.theme.muted } else { self.theme.text },
            clip,
        );
        if focused && self.caret_on {
            let caret_x = if empty {
                box_rect.left + self.scale(10)
            } else {
                box_rect.left + self.scale(10) + self.text_width(hdc, &self.commit_message)
            };
            Self::fill(
                hdc,
                RECT {
                    left: caret_x,
                    top: box_rect.top + self.scale(7),
                    right: caret_x + 1,
                    bottom: box_rect.top + self.scale(24),
                },
                self.theme.text,
            );
        }
    }

    fn paint_commit_button(&self, hdc: HDC, layout: &GitLayout, clip: RECT) {
        let ready = !self.commit_message.trim().is_empty() && !self.git_busy;
        Self::fill(
            hdc,
            layout.commit_button,
            if ready {
                rgb(52, 96, 168)
            } else {
                self.theme.edge
            },
        );
        let label = if self.git_busy {
            "Working..."
        } else if self.changes.iter().any(|change| change.staged) {
            "Commit staged"
        } else {
            "Commit all"
        };
        let width = self.text_width(hdc, label);
        Self::label(
            hdc,
            label,
            (layout.commit_button.left + layout.commit_button.right - width) / 2,
            layout.commit_button.top + self.scale(5),
            if ready { self.theme.text } else { self.theme.muted },
            clip,
        );
    }

    /// One of the three sync buttons. They run in the terminal panel, so the
    /// label is the whole affordance and gets no state of its own.
    fn paint_git_tool(&self, hdc: HDC, rect: &RECT, label: &str, clip: RECT) {
        Self::fill(hdc, *rect, self.theme.active_bg);
        let width = self.text_width(hdc, label);
        Self::label(
            hdc,
            label,
            (rect.left + rect.right - width) / 2,
            rect.top + self.scale(4),
            self.theme.muted,
            clip,
        );
    }

    fn git_glyph(&self, hdc: HDC, glyph: &str, rect: &RECT, color: u32, clip: RECT) {
        let width = self.text_width(hdc, glyph).max(1);
        Self::label(
            hdc,
            glyph,
            rect.left + (rect.right - rect.left - width) / 2,
            rect.top + (rect.bottom - rect.top - self.scale(16)) / 2,
            color,
            clip,
        );
    }
}
