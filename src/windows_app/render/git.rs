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
        self.paint_git_branch(hdc, layout.branch, clip);

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
                    if top > layout.list_top {
                        Self::fill(
                            hdc,
                            RECT {
                                left: left + self.scale(10),
                                top,
                                right: right - self.scale(10),
                                bottom: top + 1,
                            },
                            self.theme.edge,
                        );
                    }
                    let collapsed = self.git_section_collapsed(*section);
                    Self::label(
                        hdc,
                        if collapsed { ">" } else { "v" },
                        left + self.scale(13),
                        top + self.scale(9),
                        self.theme.muted,
                        clip,
                    );
                    Self::label(
                        hdc,
                        title,
                        left + self.scale(31),
                        top + self.scale(9),
                        self.theme.muted,
                        clip,
                    );
                    let count_text = count.to_string();
                    let count_width = self.text_width(hdc, &count_text);
                    let title_width = self.text_width(hdc, title);
                    let badge = RECT {
                        left: left + self.scale(37) + title_width,
                        top: top + self.scale(7),
                        right: left + self.scale(49) + title_width + count_width,
                        bottom: top + self.scale(27),
                    };
                    self.panel_card(
                        hdc,
                        badge,
                        self.scale(5),
                        self.theme.edge,
                        self.theme.active_bg,
                    );
                    Self::label(
                        hdc,
                        &count_text,
                        badge.left + (badge.right - badge.left - count_width) / 2,
                        badge.top + self.scale(2),
                        self.theme.text,
                        badge,
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
                    let newest = self.history.first().is_some_and(|item| item.oid == entry.oid);
                    if selected || newest {
                        self.panel_card(
                            hdc,
                            RECT {
                                left: left + self.scale(9),
                                top: top + self.scale(2),
                                right: right - self.scale(11),
                                bottom: top + self.scale(ROW_COMMIT - 3),
                            },
                            self.scale(5),
                            if selected { self.theme.blue } else { self.theme.edge },
                            if selected { self.theme.select_bg } else { self.theme.active_bg },
                        );
                    }
                    let node_x = left + self.scale(25);
                    Self::fill(
                        hdc,
                        RECT {
                            left: node_x,
                            top,
                            right: node_x + 1,
                            bottom: top + height,
                        },
                        self.theme.edge,
                    );
                    Self::rounded_fill(
                        hdc,
                        RECT {
                            left: node_x - self.scale(4),
                            top: top + self.scale(10),
                            right: node_x + self.scale(5),
                            bottom: top + self.scale(19),
                        },
                        self.scale(9),
                        if newest { self.theme.violet } else { self.theme.blue },
                    );
                    self.label_ellipsis(
                        hdc,
                        &entry.subject,
                        left + self.scale(40),
                        top + self.scale(5),
                        self.theme.text,
                        RECT {
                            left: left + self.scale(40),
                            top,
                            right: right - self.scale(16),
                            bottom: top + height,
                        },
                    );
                    Self::label(
                        hdc,
                        &format!("{}  ·  {}  ·  {}", entry.oid, entry.author, entry.date),
                        left + self.scale(40),
                        top + self.scale(25),
                        self.theme.muted,
                        clip,
                    );
                }
                GitRow::Clean => {
                    let center = (left + right) / 2;
                    let icon = RECT {
                        left: center - self.scale(14),
                        top: top + self.scale(8),
                        right: center + self.scale(14),
                        bottom: top + self.scale(36),
                    };
                    self.panel_card(
                        hdc,
                        icon,
                        self.scale(14),
                        self.theme.green,
                        self.theme.sidebar_bg,
                    );
                    let check = "✓";
                    let check_width = self.text_width(hdc, check);
                    Self::label(
                        hdc,
                        check,
                        center - check_width / 2,
                        icon.top + self.scale(5),
                        self.theme.green,
                        icon,
                    );
                    let title = "Working tree clean";
                    let title_width = self.text_width(hdc, title);
                    Self::label(
                        hdc,
                        title,
                        center - title_width / 2,
                        top + self.scale(43),
                        self.theme.text,
                        clip,
                    );
                    let detail = "No pending changes";
                    let detail_width = self.text_width(hdc, detail);
                    Self::label(
                        hdc,
                        detail,
                        center - detail_width / 2,
                        top + self.scale(62),
                        self.theme.muted,
                        clip,
                    );
                }
                GitRow::Note(text) => {
                    Self::label(
                        hdc,
                        text,
                        left + self.scale(14),
                        top + self.scale(4),
                        self.theme.muted,
                        clip,
                    );
                }
            }
            top = rect.bottom;
        }
        self.paint_git_scrollbar(hdc, &layout, &rows, editor_bottom);
    }

    fn paint_commit_box(&self, hdc: HDC, box_rect: RECT, _clip: RECT) {
        let focused = self.commit_focus && self.focused;
        self.panel_card(
            hdc,
            box_rect,
            self.scale(6),
            if focused { self.theme.blue } else { self.theme.edge },
            self.theme.active_bg,
        );
        let empty = self.commit_message.is_empty();
        let text = if empty {
            "Commit message".to_owned()
        } else {
            self.commit_message.clone()
        };
        self.label_ellipsis(
            hdc,
            &text,
            box_rect.left + self.scale(10),
            box_rect.top + self.scale(12),
            if empty { self.theme.muted } else { self.theme.text },
            RECT {
                left: box_rect.left + self.scale(10),
                top: box_rect.top,
                right: box_rect.right - self.scale(8),
                bottom: box_rect.bottom,
            },
        );
        if focused && self.caret_on {
            let caret_x = if empty {
                box_rect.left + self.scale(10)
            } else {
                (box_rect.left + self.scale(10) + self.text_width(hdc, &self.commit_message))
                    .min(box_rect.right - self.scale(6))
            };
            Self::fill(
                hdc,
                RECT {
                    left: caret_x,
                    top: box_rect.top + self.scale(11),
                    right: caret_x + 1,
                    bottom: box_rect.top + self.scale(30),
                },
                self.theme.text,
            );
        }
    }

    fn paint_commit_button(&self, hdc: HDC, layout: &GitLayout, clip: RECT) {
        let ready = !self.commit_message.trim().is_empty() && !self.git_busy;
        self.panel_card(
            hdc,
            layout.commit_button,
            self.scale(6),
            if ready { rgb(108, 92, 246) } else { self.theme.edge },
            if ready {
                rgb(79, 70, 210)
            } else {
                self.theme.active_bg
            },
        );
        let label = if self.git_busy {
            "Working..."
        } else {
            "Commit"
        };
        let width = self.text_width(hdc, label);
        Self::label(
            hdc,
            label,
            (layout.commit_button.left + layout.commit_button.right - width) / 2,
            layout.commit_button.top + self.scale(8),
            if ready { self.theme.text } else { self.theme.muted },
            clip,
        );
    }

    /// One of the three sync buttons. They run in the terminal panel, so the
    /// label is the whole affordance and gets no state of its own.
    fn paint_git_tool(&self, hdc: HDC, rect: &RECT, label: &str, clip: RECT) {
        self.panel_card(
            hdc,
            *rect,
            self.scale(5),
            self.theme.edge,
            self.theme.sidebar_bg,
        );
        let decorated = match label {
            "Push" => "↑  Push",
            "Pull" => "↓  Pull",
            _ => "↻  Fetch",
        };
        let width = self.text_width(hdc, decorated);
        Self::label(
            hdc,
            decorated,
            (rect.left + rect.right - width) / 2,
            rect.top + self.scale(8),
            self.theme.text,
            clip,
        );
    }

    fn paint_git_branch(&self, hdc: HDC, rect: RECT, clip: RECT) {
        Self::fill(
            hdc,
            RECT {
                left: rect.left,
                top: rect.bottom - 1,
                right: rect.right,
                bottom: rect.bottom,
            },
            self.theme.edge,
        );
        let branch = self.workspace_branch.as_deref().unwrap_or("No repository");
        Self::label(
            hdc,
            "⑂",
            rect.left + self.scale(4),
            rect.top + self.scale(9),
            self.theme.blue,
            clip,
        );
        self.label_ellipsis(
            hdc,
            branch,
            rect.left + self.scale(26),
            rect.top + self.scale(9),
            self.theme.text,
            RECT {
                left: rect.left + self.scale(26),
                top: rect.top,
                right: rect.right - self.scale(104),
                bottom: rect.bottom,
            },
        );
        let sync = if self.git_conflicted {
            "Conflicts".to_owned()
        } else if self.git_ahead > 0 || self.git_behind > 0 {
            format!("↑{}  ↓{}", self.git_ahead, self.git_behind)
        } else {
            "✓  Up to date".to_owned()
        };
        let sync_width = self.text_width(hdc, &sync);
        Self::label(
            hdc,
            &sync,
            rect.right - sync_width - self.scale(4),
            rect.top + self.scale(9),
            if self.git_conflicted { self.theme.error } else { self.theme.green },
            clip,
        );
    }

    fn paint_git_scrollbar(
        &self,
        hdc: HDC,
        layout: &GitLayout,
        rows: &[GitRow],
        editor_bottom: i32,
    ) {
        let total: i32 = rows.iter().map(|row| self.scale(row.height())).sum();
        let viewport = (editor_bottom - layout.list_top).max(1);
        if total <= viewport || rows.is_empty() {
            return;
        }
        let track = RECT {
            left: layout.branch.right + self.scale(5),
            top: layout.list_top + self.scale(4),
            right: layout.branch.right + self.scale(8),
            bottom: editor_bottom - self.scale(5),
        };
        Self::rounded_fill(hdc, track, self.scale(3), self.theme.edge);
        let track_height = (track.bottom - track.top).max(1);
        let thumb_height = ((track_height as i64 * viewport as i64) / total as i64)
            .max(self.scale(24) as i64) as i32;
        let max_first = rows.len().saturating_sub(self.git_visible_rows_from_height(viewport));
        let travel = (track_height - thumb_height).max(0);
        let offset = if max_first == 0 {
            0
        } else {
            travel * self.panel_first.min(max_first) as i32 / max_first as i32
        };
        Self::rounded_fill(
            hdc,
            RECT {
                left: track.left,
                top: track.top + offset,
                right: track.right,
                bottom: track.top + offset + thumb_height,
            },
            self.scale(3),
            self.theme.muted,
        );
    }

    fn git_visible_rows_from_height(&self, height: i32) -> usize {
        (height / self.scale(ROW_CHANGE).max(1)).max(1) as usize
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
