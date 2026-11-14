use super::super::git::{
    GitButton, GitHot, GitLayout, GitPrimary, GitRow, GitSection, LETTER_WIDTH, ROW_INDENT,
};
use super::super::*;
use super::blend;

// The commit button's fill and border once it has something to do.
const READY_FILL: (u8, u8, u8) = (79, 70, 210);
const READY_EDGE: (u8, u8, u8) = (108, 92, 246);

impl App {
    /// The Source Control panel below its header title, laid out as
    /// design/source-control-design.html: the message box, the commit button
    /// with its ▾, the branch with Pull, Push and Fetch, then one list of
    /// staged changes, changes and history, every row 22 px.
    pub(in crate::windows_app) fn paint_git_review(
        &self,
        hdc: HDC,
        left: i32,
        right: i32,
        editor_bottom: i32,
        clip: RECT,
    ) {
        let layout = self.git_layout(left, right);
        // Hovering a row repaints only that row, so rows outside what's
        // being painted are skipped instead of drawn and clipped away.
        let mut update = RECT::default();
        unsafe { GetClipBox(hdc, &mut update) };
        let shows = |rect: &RECT| rect.bottom > update.top && rect.top < update.bottom;

        for (button, glyph) in [
            (GitButton::Commit, DebugGlyph::Check),
            (GitButton::Refresh, DebugGlyph::Refresh),
            (GitButton::More, DebugGlyph::More),
        ] {
            self.paint_git_icon_button(hdc, layout.button(button), button, glyph, 0);
        }
        if shows(&layout.commit_box) {
            self.paint_commit_box(hdc, layout.commit_box);
        }
        if shows(&layout.primary) {
            self.paint_commit_button(hdc, &layout);
        }
        if shows(&layout.branch) {
            self.paint_git_branch(hdc, &layout, clip);
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

        let rows = self.git_rows();
        let hot_row = self.git_hot_row();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs() as i64);
        let first_commit = rows.iter().position(|row| matches!(row, GitRow::Commit(_)));
        let mut top = layout.list_top;
        for (index, row) in rows.iter().enumerate().skip(self.panel_first) {
            if top >= editor_bottom {
                break;
            }
            let rect = RECT {
                left,
                top,
                right,
                bottom: top + self.scale(row.height()),
            };
            top = rect.bottom;
            if !shows(&rect) {
                continue;
            }
            let selected = self.panel_focus && index == self.panel_selected;
            let hot = hot_row == Some(index);
            match row {
                GitRow::Header {
                    title,
                    count,
                    section,
                } => self.paint_git_header(hdc, rect, title, *count, *section, hot, clip),
                GitRow::Change { change, staged } => {
                    let open = self.review_file.as_ref() == Some(&change.path)
                        && self.review_staged == *staged;
                    self.paint_git_change(
                        hdc,
                        rect,
                        change,
                        *staged,
                        selected || open,
                        hot,
                        selected,
                    );
                }
                GitRow::Commit(entry) => self.paint_git_commit(
                    hdc,
                    rect,
                    entry,
                    first_commit == Some(index),
                    selected,
                    hot,
                    now,
                    clip,
                ),
                GitRow::Clean => {
                    let middle = (rect.top + rect.bottom) / 2;
                    let size = self.scale(16);
                    self.icons.draw_glyph(
                        hdc,
                        DebugGlyph::Check,
                        self.theme.green,
                        left + self.scale(ROW_INDENT),
                        middle - size / 2,
                        size,
                    );
                    self.label_mid(
                        hdc,
                        "No changes",
                        left + self.scale(ROW_INDENT + 22),
                        middle,
                        self.theme.muted,
                        clip,
                    );
                }
                GitRow::Note(text) => {
                    self.label_mid(
                        hdc,
                        text,
                        left + self.scale(ROW_INDENT),
                        (rect.top + rect.bottom) / 2,
                        self.theme.muted,
                        clip,
                    );
                }
            }
        }
        self.paint_git_scrollbar(hdc, &layout, &rows, right, editor_bottom);
    }

    /// The fill behind a row or button under the mouse, easing in.
    fn git_hover_fill(&self) -> u32 {
        self.hot_color(self.theme.sidebar_bg, self.theme.active_bg)
    }

    /// An icon button of the header or the branch row, with `count` beside
    /// its icon when there's one.
    fn paint_git_icon_button(
        &self,
        hdc: HDC,
        rect: RECT,
        button: GitButton,
        glyph: DebugGlyph,
        count: usize,
    ) {
        let hot = self.hot == Some(Hot::Git(GitHot::Button(button)));
        if hot {
            Self::rounded_fill(hdc, rect, self.scale(4), self.git_hover_fill());
        }
        let color = if hot {
            self.hot_color(self.theme.muted, self.theme.text)
        } else {
            self.theme.muted
        };
        let size = self.scale(16);
        if count == 0 {
            self.glyph_in(hdc, glyph, color, rect, size);
            return;
        }
        let middle = (rect.top + rect.bottom) / 2;
        let x = rect.left + self.scale(4);
        self.icons
            .draw_glyph(hdc, glyph, color, x, middle - size / 2, size);
        self.label_mid(
            hdc,
            &count.to_string(),
            x + size + self.scale(1),
            middle,
            self.theme.blue,
            rect,
        );
    }

    fn paint_commit_box(&self, hdc: HDC, box_rect: RECT) {
        let focused = self.commit_focus && self.focused;
        self.panel_card(
            hdc,
            box_rect,
            self.scale(4),
            if focused {
                self.theme.blue
            } else {
                self.theme.edge
            },
            self.theme.active_bg,
        );
        let empty = self.commit_message.is_empty();
        let text = if !empty {
            self.commit_message.clone()
        } else if let Some(branch) = self.workspace_branch.as_deref() {
            format!("Message (Enter to commit on \"{branch}\")")
        } else {
            "Message".to_owned()
        };
        let middle = (box_rect.top + box_rect.bottom) / 2;
        let text_left = box_rect.left + self.scale(8);
        let inside = RECT {
            left: text_left,
            top: box_rect.top,
            right: box_rect.right - self.scale(8),
            bottom: box_rect.bottom,
        };
        self.label_ellipsis(
            hdc,
            &text,
            text_left,
            middle - self.text_height(hdc) / 2,
            if empty {
                self.theme.muted
            } else {
                self.theme.text
            },
            inside,
        );
        if focused && self.caret_on {
            let caret_x = if empty {
                text_left
            } else {
                (text_left + self.text_width(hdc, &self.commit_message)).min(inside.right)
            };
            let half = self.scale(8);
            Self::fill(
                hdc,
                RECT {
                    left: caret_x,
                    top: middle - half,
                    right: caret_x + 1,
                    bottom: middle + half,
                },
                self.theme.text,
            );
        }
    }

    fn paint_commit_button(&self, hdc: HDC, layout: &GitLayout) {
        let primary = self.git_primary();
        let ready = match primary {
            GitPrimary::Busy => false,
            GitPrimary::Commit { ready } => ready,
            GitPrimary::Push(_) | GitPrimary::Pull(_) => true,
        };
        let (fill, edge) = if ready {
            (
                ui(READY_FILL.0, READY_FILL.1, READY_FILL.2),
                ui(READY_EDGE.0, READY_EDGE.1, READY_EDGE.2),
            )
        } else {
            (self.theme.active_bg, self.theme.edge)
        };
        let whole = RECT {
            right: layout.drop.right,
            ..layout.primary
        };
        let radius = self.scale(4);
        self.panel_card(hdc, whole, radius, edge, fill);
        // The half under the mouse lights up a little.
        for button in [GitButton::Primary, GitButton::Drop] {
            if self.hot == Some(Hot::Git(GitHot::Button(button))) {
                let rect = layout.button(button);
                let lit = blend(fill, self.theme.text, 0.1);
                Self::rounded_fill(
                    hdc,
                    RECT {
                        left: rect.left + 1,
                        top: rect.top + 1,
                        right: rect.right - 1,
                        bottom: rect.bottom - 1,
                    },
                    radius - 1,
                    self.hot_color(fill, lit),
                );
            }
        }
        Self::fill(
            hdc,
            RECT {
                left: layout.drop.left,
                top: layout.drop.top + 1,
                right: layout.drop.left + 1,
                bottom: layout.drop.bottom - 1,
            },
            edge,
        );
        let color = if ready {
            self.theme.text
        } else {
            self.theme.muted
        };
        let commits = |count: usize| if count == 1 { "commit" } else { "commits" };
        let (glyph, label) = match primary {
            GitPrimary::Busy => (None, "Working...".to_owned()),
            GitPrimary::Commit { .. } => (Some(DebugGlyph::Check), "Commit".to_owned()),
            GitPrimary::Push(count) => (
                Some(DebugGlyph::ArrowUp),
                format!("Push {count} {}", commits(count)),
            ),
            GitPrimary::Pull(count) => (
                Some(DebugGlyph::ArrowDown),
                format!("Pull {count} {}", commits(count)),
            ),
        };
        let size = self.scale(16);
        let gap = self.scale(6);
        let text_width = self.text_width(hdc, &label);
        let width = text_width + glyph.map_or(0, |_| size + gap);
        let rect = layout.primary;
        let middle = (rect.top + rect.bottom) / 2;
        let mut x = (rect.left + rect.right - width) / 2;
        if let Some(glyph) = glyph {
            self.icons
                .draw_glyph(hdc, glyph, color, x, middle - size / 2, size);
            x += size + gap;
        }
        self.label_mid(hdc, &label, x, middle, color, rect);
        self.glyph_in(hdc, DebugGlyph::ChevronDown, color, layout.drop, size);
    }

    fn paint_git_branch(&self, hdc: HDC, layout: &GitLayout, clip: RECT) {
        let rect = layout.branch;
        let middle = (rect.top + rect.bottom) / 2;
        let size = self.scale(16);
        self.icons.draw_glyph(
            hdc,
            DebugGlyph::Branch,
            self.theme.blue,
            rect.left,
            middle - size / 2,
            size,
        );
        let text_right = layout.sync[0].left - self.scale(6);
        let name_left = rect.left + size + self.scale(6);
        let limit = RECT {
            right: text_right,
            ..clip
        };
        let Some(branch) = self.workspace_branch.as_deref() else {
            self.label_mid(
                hdc,
                "No repository",
                name_left,
                middle,
                self.theme.muted,
                limit,
            );
            return;
        };
        let name_width = self.text_width(hdc, branch);
        self.label_ellipsis(
            hdc,
            branch,
            name_left,
            middle - self.text_height(hdc) / 2,
            self.theme.text,
            limit,
        );
        let commits = |count: usize| if count == 1 { "commit" } else { "commits" };
        let (state, color) = if self.git_conflicted {
            ("conflicts".to_owned(), self.theme.error)
        } else if self.git_ahead > 0 && self.git_behind > 0 {
            (
                format!("{} to push, {} to pull", self.git_ahead, self.git_behind),
                self.theme.blue,
            )
        } else if self.git_ahead > 0 {
            (
                format!("{} {} to push", self.git_ahead, commits(self.git_ahead)),
                self.theme.blue,
            )
        } else if self.git_behind > 0 {
            (
                format!("{} {} to pull", self.git_behind, commits(self.git_behind)),
                self.theme.blue,
            )
        } else {
            ("up to date".to_owned(), self.theme.muted)
        };
        let state_left = name_left + name_width + self.scale(8);
        if state_left < text_right - self.scale(16) {
            self.label_ellipsis(
                hdc,
                &state,
                state_left,
                middle - self.text_height(hdc) / 2,
                color,
                limit,
            );
        }
        for (button, glyph, count) in [
            (GitButton::Pull, DebugGlyph::ArrowDown, self.git_behind),
            (GitButton::Push, DebugGlyph::ArrowUp, self.git_ahead),
            (GitButton::Fetch, DebugGlyph::Fetch, 0),
        ] {
            self.paint_git_icon_button(hdc, layout.button(button), button, glyph, count);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_git_header(
        &self,
        hdc: HDC,
        rect: RECT,
        title: &str,
        count: usize,
        section: GitSection,
        hot: bool,
        clip: RECT,
    ) {
        let middle = (rect.top + rect.bottom) / 2;
        let collapsed = self.git_section_collapsed(section);
        self.chevron(hdc, rect.left + self.scale(12), middle, !collapsed);
        let title_left = rect.left + self.scale(22);
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
        let pill_left = title_left + self.text_width(hdc, title) + self.scale(6);
        let pill_height = self.scale(16);
        let pill = RECT {
            left: pill_left,
            top: middle - pill_height / 2,
            right: pill_left + (count_width + self.scale(10)).max(self.scale(18)),
            bottom: middle - pill_height / 2 + pill_height,
        };
        Self::rounded_fill(hdc, pill, pill_height, self.theme.edge);
        self.label_mid(
            hdc,
            &count_text,
            (pill.left + pill.right - count_width) / 2,
            middle,
            self.theme.text,
            pill,
        );
        if hot {
            for (_, glyph, button) in self.git_header_actions(rect.right, rect, section) {
                self.glyph_in(hdc, glyph, self.theme.muted, button, self.scale(16));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_git_change(
        &self,
        hdc: HDC,
        rect: RECT,
        change: &Change,
        staged: bool,
        highlighted: bool,
        hot: bool,
        selected: bool,
    ) {
        if highlighted {
            Self::fill(hdc, rect, self.theme.select_bg);
        } else if hot {
            Self::fill(hdc, rect, self.git_hover_fill());
        }
        let middle = (rect.top + rect.bottom) / 2;
        let actions = self.git_row_actions(rect.right, rect, change, staged);

        // The status as one letter, coloured by the kind of change.
        let (letter, color) = if change.unmerged {
            ("!", self.theme.orange)
        } else if change.untracked {
            ("U", self.theme.green)
        } else {
            match change.label() {
                "Added" => ("A", self.theme.green),
                "Deleted" => ("D", self.theme.error),
                "Renamed" => ("R", self.theme.blue),
                "Copied" => ("C", self.theme.blue),
                "Type change" => ("T", self.theme.blue),
                "Conflicted" => ("!", self.theme.orange),
                _ => ("M", self.theme.warning),
            }
        };
        let letter_width = self.text_width(hdc, letter);
        self.label_mid(
            hdc,
            letter,
            actions.letter.left + (self.scale(LETTER_WIDTH) - letter_width) / 2,
            middle,
            color,
            actions.letter,
        );

        // Buttons on the row under the mouse and on the keyboard's row.
        let mut text_right = actions.letter.left - self.scale(6);
        if hot || selected {
            let busy = if self.git_busy {
                self.theme.edge
            } else {
                self.theme.muted
            };
            let size = self.scale(16);
            let toggle = if staged {
                DebugGlyph::Minus
            } else {
                DebugGlyph::Plus
            };
            self.glyph_in(hdc, toggle, busy, actions.toggle, size);
            text_right = actions.toggle.left;
            if let Some(discard) = actions.discard {
                self.glyph_in(hdc, DebugGlyph::Discard, busy, discard, size);
                text_right = discard.left;
            }
            if let Some(open) = actions.open {
                self.glyph_in(hdc, DebugGlyph::OpenFile, self.theme.muted, open, size);
                text_right = open.left;
            }
            text_right -= self.scale(4);
        }

        let icon_left = rect.left + self.scale(ROW_INDENT);
        let size = self.scale(16);
        let use_theme = self.has_extension("material-icons");
        if !self.icons.draw_for_path(
            hdc,
            &change.path,
            false,
            false,
            use_theme,
            icon_left,
            middle - size / 2,
            size,
        ) {
            self.draw_vector_file(hdc, &change.path, icon_left, middle - size / 2, size);
        }

        // The file's name first, then its folder, dimmed.
        let full = change.path.to_string_lossy().replace('\\', "/");
        let (folder, name) = match full.rsplit_once('/') {
            Some((folder, name)) => (folder, name),
            None => ("", full.as_str()),
        };
        let name_left = icon_left + size + self.scale(6);
        let top = middle - self.text_height(hdc) / 2;
        let deleted = letter == "D";
        let name_color = if deleted {
            self.theme.muted
        } else {
            self.theme.text
        };
        let limit = RECT {
            right: text_right,
            ..rect
        };
        let name_width = self.text_width(hdc, name).min(text_right - name_left);
        self.label_ellipsis(hdc, name, name_left, top, name_color, limit);
        if deleted && name_width > 0 {
            Self::fill(
                hdc,
                RECT {
                    left: name_left,
                    top: middle,
                    right: name_left + name_width,
                    bottom: middle + 1,
                },
                self.theme.muted,
            );
        }
        let folder_left = name_left + name_width + self.scale(6);
        if !folder.is_empty() && folder_left < text_right - self.scale(16) {
            self.label_ellipsis(hdc, folder, folder_left, top, self.theme.muted, limit);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_git_commit(
        &self,
        hdc: HDC,
        rect: RECT,
        entry: &CommitEntry,
        newest: bool,
        selected: bool,
        hot: bool,
        now: i64,
        clip: RECT,
    ) {
        let background = if selected {
            self.theme.select_bg
        } else if hot {
            self.git_hover_fill()
        } else {
            self.theme.sidebar_bg
        };
        if selected || hot {
            Self::fill(hdc, rect, background);
        }
        let middle = (rect.top + rect.bottom) / 2;
        // The graph: one line through every commit, a dot on each, and a
        // ring on the newest.
        let lane = rect.left + self.scale(ROW_INDENT);
        Self::fill(
            hdc,
            RECT {
                left: lane,
                top: if newest { middle } else { rect.top },
                right: lane + 1,
                bottom: rect.bottom,
            },
            blend(self.theme.edge, self.theme.muted, 0.3),
        );
        let dot = |size: i32, color: u32| {
            let size = self.scale(size);
            self.icons.draw_glyph(
                hdc,
                DebugGlyph::GutterDot,
                color,
                lane - size / 2,
                middle - size / 2,
                size,
            );
        };
        if newest {
            dot(11, self.theme.violet);
            dot(6, background);
        } else {
            dot(8, self.theme.blue);
        }

        let mut x = rect.left + self.scale(34);
        if newest && let Some(branch) = self.workspace_branch.as_deref() {
            let width = self.text_width(hdc, branch) + self.scale(12);
            let height = self.scale(16);
            let pill = RECT {
                left: x,
                top: middle - height / 2,
                right: (x + width).min(rect.right - self.scale(60)),
                bottom: middle - height / 2 + height,
            };
            if pill.right > pill.left + self.scale(16) {
                Self::rounded_fill(
                    hdc,
                    pill,
                    height,
                    blend(self.theme.sidebar_bg, self.theme.violet, 0.28),
                );
                self.label_ellipsis(
                    hdc,
                    branch,
                    pill.left + self.scale(6),
                    middle - self.text_height(hdc) / 2,
                    blend(self.theme.violet, self.theme.text, 0.5),
                    RECT {
                        right: pill.right - self.scale(4),
                        ..pill
                    },
                );
                x = pill.right + self.scale(6);
            }
        }
        // Its age at the right; its hash instead under the mouse.
        let side = if hot {
            entry.oid.clone()
        } else {
            workflow::short_age(entry.time, now)
        };
        let side_width = self.text_width(hdc, &side);
        let side_left = rect.right - self.scale(8) - side_width;
        self.label_mid(hdc, &side, side_left, middle, self.theme.muted, clip);
        self.label_ellipsis(
            hdc,
            &entry.subject,
            x,
            middle - self.text_height(hdc) / 2,
            self.theme.text,
            RECT {
                right: side_left - self.scale(8),
                ..rect
            },
        );
    }

    fn paint_git_scrollbar(
        &self,
        hdc: HDC,
        layout: &GitLayout,
        rows: &[GitRow],
        right: i32,
        editor_bottom: i32,
    ) {
        let total: i32 = rows.iter().map(|row| self.scale(row.height())).sum();
        let viewport = (editor_bottom - layout.list_top).max(1);
        if total <= viewport || rows.is_empty() {
            return;
        }
        let track = RECT {
            left: right - self.scale(5),
            top: layout.list_top + self.scale(2),
            right: right - self.scale(1),
            bottom: editor_bottom - self.scale(2),
        };
        let track_height = (track.bottom - track.top).max(1);
        let thumb_height = ((track_height as i64 * viewport as i64) / total as i64)
            .max(self.scale(24) as i64) as i32;
        let max_first = rows
            .len()
            .saturating_sub(self.git_visible_rows_from_height(viewport));
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
            self.scale(2),
            blend(self.theme.edge, self.theme.muted, 0.3),
        );
    }

    fn git_visible_rows_from_height(&self, height: i32) -> usize {
        (height / self.scale(EXPLORER_ROW).max(1)).max(1) as usize
    }
}
