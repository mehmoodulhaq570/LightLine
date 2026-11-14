use super::*;

// Source control sidebar geometry, in logical pixels from the top of the
// panel's 40 px header (design/source-control-design.html). The message box,
// the button and the branch row stay pinned while the list below scrolls, so
// their rectangles come from the same layout as the rows.
const COMMIT_TOP: i32 = 48;
const COMMIT_HEIGHT: i32 = 30;
const COMMIT_BUTTON_HEIGHT: i32 = 28;
// The ▾ at the commit button's right end.
const DROP_WIDTH: i32 = 28;
// The branch, with Pull, Push and Fetch as icon buttons at its right.
const BRANCH_HEIGHT: i32 = 26;
const SYNC_BUTTON: i32 = 24;
// Every list row is one 22 px line, like the Explorer's.
pub(super) const ROW_HEADER: i32 = EXPLORER_ROW;
pub(super) const ROW_CHANGE: i32 = EXPLORER_ROW;
pub(super) const ROW_COMMIT: i32 = EXPLORER_ROW;
pub(super) const ROW_CLEAN: i32 = EXPLORER_ROW;
// Row action buttons are square and right aligned, like the rest of the panel.
pub(super) const ROW_BUTTON: i32 = 22;
// Where a change row's text starts, and the status letter's column.
pub(super) const ROW_INDENT: i32 = 20;
pub(super) const LETTER_WIDTH: i32 = 14;

// One line in the source-control list: a section title, a changed file, a
// commit, or a plain message such as "No changes".
pub(super) enum GitRow {
    Header {
        title: &'static str,
        count: usize,
        section: GitSection,
    },
    Change {
        change: Change,
        staged: bool,
    },
    Commit(CommitEntry),
    Clean,
    Note(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GitSection {
    Staged,
    Changes,
    History,
}

impl GitRow {
    pub(super) fn height(&self) -> i32 {
        match self {
            Self::Header { .. } => ROW_HEADER,
            Self::Change { .. } | Self::Note(_) => ROW_CHANGE,
            Self::Commit(_) => ROW_COMMIT,
            Self::Clean => ROW_CLEAN,
        }
    }

    /// Whether Up/Down and Enter act on this row.
    fn selectable(&self) -> bool {
        matches!(self, Self::Change { .. } | Self::Commit(_))
    }
}

/// The panel's buttons outside the list: three in the header, the commit
/// button and its ▾, and three on the branch row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GitButton {
    Commit,
    Refresh,
    More,
    Primary,
    Drop,
    Pull,
    Push,
    Fetch,
}

/// What the mouse is over in the panel, for hover: a button, or a row of
/// the list by its index into `git_rows`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GitHot {
    Button(GitButton),
    Row(usize),
}

/// What the commit button does now: the next thing there is to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GitPrimary {
    Busy,
    /// Commit; `ready` once there's a message.
    Commit {
        ready: bool,
    },
    /// Nothing to commit and the branch is ahead / behind its upstream.
    Push(usize),
    Pull(usize),
}

// Where a click landed. Rows are addressed by index into `git_rows`.
pub(super) enum GitHit {
    CommitBox,
    Button(GitButton),
    StageAll,
    UnstageAll,
    DiscardAll,
    ToggleSection(GitSection),
    Row(usize),
    Toggle(usize),
    Discard(usize),
    Open(usize),
    Nothing,
}

pub(super) struct GitLayout {
    pub commit_box: RECT,
    /// The commit button without its ▾, and the ▾.
    pub primary: RECT,
    pub drop: RECT,
    /// The branch row; Pull, Push and Fetch sit at its right.
    pub branch: RECT,
    /// Commit, Refresh and More in the header.
    pub header: [RECT; 3],
    /// Pull, Push and Fetch.
    pub sync: [RECT; 3],
    /// The line between the branch row and the list.
    pub divider: i32,
    pub list_top: i32,
}

/// A change row's buttons, right to left: the status letter's column, then
/// Stage or Unstage, Discard (unstaged rows only) and Open File (not for a
/// deleted file).
pub(super) struct RowActions {
    pub letter: RECT,
    pub toggle: RECT,
    pub discard: Option<RECT>,
    pub open: Option<RECT>,
}

impl GitLayout {
    pub(super) fn button(&self, button: GitButton) -> RECT {
        match button {
            GitButton::Commit => self.header[0],
            GitButton::Refresh => self.header[1],
            GitButton::More => self.header[2],
            GitButton::Primary => self.primary,
            GitButton::Drop => self.drop,
            GitButton::Pull => self.sync[0],
            GitButton::Push => self.sync[1],
            GitButton::Fetch => self.sync[2],
        }
    }

    const BUTTONS: [GitButton; 8] = [
        GitButton::Commit,
        GitButton::Refresh,
        GitButton::More,
        GitButton::Primary,
        GitButton::Drop,
        GitButton::Pull,
        GitButton::Push,
        GitButton::Fetch,
    ];

    fn button_at(&self, x: i32, y: i32) -> Option<GitButton> {
        Self::BUTTONS
            .into_iter()
            .find(|button| contains(&self.button(*button), x, y))
    }
}

/// How many digits a count adds to a sync button's width.
fn count_digits(count: usize) -> i32 {
    match count {
        0 => 0,
        1..=9 => 1,
        10..=99 => 2,
        _ => 3,
    }
}

impl App {
    fn git_paths(&self) -> Option<(PathBuf, Vec<PathBuf>)> {
        let root = self.git_root.clone().or(self.workspace_root.clone())?;
        Some((root, Vec::new()))
    }

    /// Ask Git for branch, changes and history without touching the UI thread.
    /// Called on open, on activation, after a save and after every write.
    pub(super) fn refresh_git(&mut self) {
        let Some(root) = self.git_root.clone().or(self.workspace_root.clone()) else {
            return;
        };
        self.git_generation += 1;
        let generation = self.git_generation;
        let workspace_generation = self.workspace_generation;
        self.review_loading = true;
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            let result = workflow::repo_state(&root);
            tx.send(WorkerMessage::Repo(
                generation,
                workspace_generation,
                result,
            ));
        });
    }

    pub(super) fn unwatch_git_files(&mut self) {
        if let Some(watcher) = &self.watcher {
            for path in self.git_watch_files.drain(..) {
                watcher.unwatch_file(path);
            }
        }
    }

    pub(super) fn apply_repo_state(&mut self, hwnd: HWND, state: RepoState) {
        // A commit, checkout or reset -- from LightLine or a terminal -- moves
        // HEAD, and the gutter compares against HEAD's text, so drop the
        // cached copy and read it again.
        let head_moved = self.git_root.as_ref() != Some(&state.root)
            || self.history.first().map(|c| &c.oid) != state.history.first().map(|c| &c.oid);
        if head_moved {
            self.git_head_cache.clear();
            self.git_diff_cache.clear();
            self.git_untracked.clear();
            self.gutter_done = None;
            self.forget_blame();
        }
        // index and HEAD move with staging, commits and checkouts. Push,
        // Pull and Fetch run in the terminal and finish there, so the panel
        // hears of them from FETCH_HEAD (fetch, pull) and the upstream's
        // remote-tracking ref (push), and its ahead/behind counts and the
        // commit button follow without a manual refresh.
        let mut watch: Vec<PathBuf> = state
            .git_dir
            .iter()
            .flat_map(|dir| [dir.join("index"), dir.join("HEAD"), dir.join("FETCH_HEAD")])
            .collect();
        if let (Some(dir), Some(upstream)) = (&state.git_dir, &state.upstream) {
            // "origin/main" -> refs\remotes\origin\main, in the separators
            // the watcher reports paths in.
            let mut path = dir.join("refs").join("remotes");
            path.extend(upstream.split('/'));
            watch.push(path);
        }
        if watch != self.git_watch_files
            && let Some(watcher) = &self.watcher
        {
            for path in &self.git_watch_files {
                watcher.unwatch_file(path.clone());
            }
            for path in &watch {
                watcher.watch_file(path.clone());
            }
            self.git_watch_files = watch;
        }
        self.git_root = Some(state.root.clone());
        self.workspace_branch = Some(state.head_label());
        self.git_ahead = state.ahead;
        self.git_behind = state.behind;
        self.git_conflicted = state.conflicted;
        self.history = state.history;
        self.changes = state.changes;
        self.forget_git_hot_row(hwnd);
        // Keep the selection inside the list after a refresh shrinks it, and
        // keep it on screen: the row count changes under the pointer whenever
        // a file is saved or a write lands.
        let rows = self.git_rows().len();
        self.panel_selected = self.panel_selected.min(rows.saturating_sub(1));
        self.git_scroll_into_view(hwnd);
        if head_moved {
            self.refresh_active_git_diff();
        }
        self.ensure_blame();
        // The status line belongs to whatever the user last asked for, so the
        // counts here stay in the section headers instead of overwriting it.
        self.refresh(hwnd);
    }

    /// The list model, built fresh so paint and hit-testing always agree.
    pub(super) fn git_rows(&self) -> Vec<GitRow> {
        let mut rows = Vec::new();
        // Only before the first answer: a refresh (one runs whenever the
        // window gets focus back) keeps showing the last list until the new
        // one arrives, instead of blanking a clean repository's history.
        if self.review_loading && self.changes.is_empty() && self.history.is_empty() {
            rows.push(GitRow::Note("Reading Git status..."));
            return rows;
        }
        let staged: Vec<&Change> = self.changes.iter().filter(|change| change.staged).collect();
        let unstaged: Vec<&Change> = self
            .changes
            .iter()
            .filter(|change| change.unstaged)
            .collect();
        let has_staged = !staged.is_empty();
        if has_staged {
            rows.push(GitRow::Header {
                title: "STAGED CHANGES",
                count: staged.len(),
                section: GitSection::Staged,
            });
            if !self.git_staged_collapsed {
                rows.extend(staged.into_iter().map(|change| GitRow::Change {
                    change: change.clone(),
                    staged: true,
                }));
            }
        }
        rows.push(GitRow::Header {
            title: "CHANGES",
            count: unstaged.len(),
            section: GitSection::Changes,
        });
        if !self.git_changes_collapsed {
            if !has_staged && unstaged.is_empty() {
                rows.push(GitRow::Clean);
            } else if unstaged.is_empty() {
                rows.push(GitRow::Note("No unstaged changes."));
            } else {
                rows.extend(unstaged.into_iter().map(|change| GitRow::Change {
                    change: change.clone(),
                    staged: false,
                }));
            }
        }
        if !self.history.is_empty() {
            rows.push(GitRow::Header {
                title: "HISTORY",
                count: self.history.len(),
                section: GitSection::History,
            });
            if !self.git_history_collapsed {
                rows.extend(self.history.iter().cloned().map(GitRow::Commit));
            }
        }
        rows
    }

    pub(super) fn git_layout(&self, left: i32, right: i32) -> GitLayout {
        let s = |value: i32| self.scale(value);
        let box_left = left + s(12);
        let box_right = right - s(12);
        let top = s(COMMIT_TOP);
        let button_top = top + s(COMMIT_HEIGHT) + s(8);
        let button_bottom = button_top + s(COMMIT_BUTTON_HEIGHT);
        let branch_top = button_bottom + s(6);
        let branch_bottom = branch_top + s(BRANCH_HEIGHT);
        // Header buttons, centred on the header's visible part (its top
        // SIDE_SHIFT is hidden), at the Run & Debug panel's spacing.
        let header = [94, 65, 36].map(|offset| RECT {
            left: right - s(offset),
            top: s(SIDE_SHIFT),
            right: right - s(offset) + s(22),
            bottom: s(40),
        });
        // Fetch, Push, Pull from the right; a count widens its button.
        let mut edge = right - s(8);
        let mut sync = [RECT::default(); 3];
        for (index, count) in [(2, 0), (1, self.git_ahead), (0, self.git_behind)] {
            let width = s(SYNC_BUTTON) + s(7) * count_digits(count);
            sync[index] = RECT {
                left: edge - width,
                top: branch_top + s(2),
                right: edge,
                bottom: branch_bottom - s(2),
            };
            edge -= width + s(2);
        }
        GitLayout {
            commit_box: RECT {
                left: box_left,
                top,
                right: box_right,
                bottom: top + s(COMMIT_HEIGHT),
            },
            primary: RECT {
                left: box_left,
                top: button_top,
                right: box_right - s(DROP_WIDTH),
                bottom: button_bottom,
            },
            drop: RECT {
                left: box_right - s(DROP_WIDTH),
                top: button_top,
                right: box_right,
                bottom: button_bottom,
            },
            branch: RECT {
                left: box_left,
                top: branch_top,
                right: right - s(8),
                bottom: branch_bottom,
            },
            header,
            sync,
            divider: branch_bottom + s(4),
            list_top: branch_bottom + s(7),
        }
    }

    /// The buttons of a change row: see `RowActions`.
    pub(super) fn git_row_actions(
        &self,
        right: i32,
        rect: RECT,
        change: &Change,
        staged: bool,
    ) -> RowActions {
        let size = self.scale(ROW_BUTTON);
        let letter_right = right - self.scale(8);
        let letter = RECT {
            left: letter_right - self.scale(LETTER_WIDTH),
            top: rect.top,
            right: letter_right,
            bottom: rect.bottom,
        };
        let at = |right: i32| RECT {
            left: right - size,
            top: rect.top,
            right,
            bottom: rect.top + size,
        };
        let toggle = at(letter.left - self.scale(4));
        let discard = (!staged).then(|| at(toggle.left));
        let open_right = discard.map_or(toggle.left, |rect| rect.left);
        let deleted = change.label() == "Deleted";
        let open = (!deleted).then(|| at(open_right));
        RowActions {
            letter,
            toggle,
            discard,
            open,
        }
    }

    /// A section header's buttons, right to left: Unstage All on Staged
    /// Changes; Stage All, then Discard All on Changes.
    pub(super) fn git_header_actions(
        &self,
        right: i32,
        rect: RECT,
        section: GitSection,
    ) -> Vec<(GitHit, DebugGlyph, RECT)> {
        let size = self.scale(ROW_BUTTON);
        let at = |index: i32| {
            let right = right - self.scale(8) - size * index;
            RECT {
                left: right - size,
                top: rect.top,
                right,
                bottom: rect.top + size,
            }
        };
        match section {
            GitSection::Staged => vec![(GitHit::UnstageAll, DebugGlyph::Minus, at(0))],
            GitSection::Changes if self.changes.iter().any(|change| change.unstaged) => vec![
                (GitHit::StageAll, DebugGlyph::Plus, at(0)),
                (GitHit::DiscardAll, DebugGlyph::Discard, at(1)),
            ],
            _ => Vec::new(),
        }
    }

    /// Each row on screen with its rectangle, in the panel's coordinates.
    fn git_rows_on_screen(
        &self,
        rows: &[GitRow],
        left: i32,
        right: i32,
        list_top: i32,
    ) -> Vec<(usize, RECT)> {
        let mut top = list_top;
        rows.iter()
            .enumerate()
            .skip(self.panel_first)
            .map(|(index, row)| {
                let rect = RECT {
                    left,
                    top,
                    right,
                    bottom: top + self.scale(row.height()),
                };
                top = rect.bottom;
                (index, rect)
            })
            .collect()
    }

    pub(super) fn git_hit(&self, x: i32, y: i32, left: i32, right: i32) -> GitHit {
        let layout = self.git_layout(left, right);
        if contains(&layout.commit_box, x, y) {
            return GitHit::CommitBox;
        }
        if let Some(button) = layout.button_at(x, y) {
            return GitHit::Button(button);
        }
        if y < layout.list_top {
            return GitHit::Nothing;
        }
        let rows = self.git_rows();
        let Some((index, rect)) = self
            .git_rows_on_screen(&rows, left, right, layout.list_top)
            .into_iter()
            .find(|(_, rect)| y >= rect.top && y < rect.bottom)
        else {
            return GitHit::Nothing;
        };
        match &rows[index] {
            GitRow::Change { change, staged } => {
                let actions = self.git_row_actions(right, rect, change, *staged);
                if contains(&actions.toggle, x, y) {
                    return GitHit::Toggle(index);
                }
                if actions.discard.is_some_and(|rect| contains(&rect, x, y)) {
                    return GitHit::Discard(index);
                }
                if actions.open.is_some_and(|rect| contains(&rect, x, y)) {
                    return GitHit::Open(index);
                }
                GitHit::Row(index)
            }
            GitRow::Header { section, .. } => self
                .git_header_actions(right, rect, *section)
                .into_iter()
                .find(|(_, _, button)| contains(button, x, y))
                .map_or(GitHit::ToggleSection(*section), |(hit, _, _)| hit),
            GitRow::Commit(_) => GitHit::Row(index),
            GitRow::Clean | GitRow::Note(_) => GitHit::Nothing,
        }
    }

    /// What's under the mouse at (x, y) in the panel's coordinates.
    pub(super) fn git_hot_at(&self, x: i32, y: i32) -> Option<GitHot> {
        let (left, right) = (self.scale(RAIL), self.sidebar_right());
        let layout = self.git_layout(left, right);
        if let Some(button) = layout.button_at(x, y) {
            return Some(GitHot::Button(button));
        }
        if y < layout.list_top {
            return None;
        }
        let rows = self.git_rows();
        self.git_rows_on_screen(&rows, left, right, layout.list_top)
            .into_iter()
            .find(|(_, rect)| y >= rect.top && y < rect.bottom)
            .filter(|(index, _)| !matches!(rows[*index], GitRow::Clean | GitRow::Note(_)))
            .map(|(index, _)| GitHot::Row(index))
    }

    /// Where `hot` is, in the panel's coordinates.
    pub(super) fn git_hot_rect(&self, hot: GitHot) -> Option<RECT> {
        let (left, right) = (self.scale(RAIL), self.sidebar_right());
        let layout = self.git_layout(left, right);
        match hot {
            GitHot::Button(button) => Some(layout.button(button)),
            GitHot::Row(index) => {
                let rows = self.git_rows();
                self.git_rows_on_screen(&rows, left, right, layout.list_top)
                    .into_iter()
                    .find(|(row, _)| *row == index)
                    .map(|(_, rect)| rect)
            }
        }
    }

    /// The row under the mouse, if any.
    pub(super) fn git_hot_row(&self) -> Option<usize> {
        match self.hot {
            Some(Hot::Git(GitHot::Row(index))) => Some(index),
            _ => None,
        }
    }

    /// The list scrolled or changed under a still mouse: the row it lit is
    /// no longer the one under it.
    pub(super) fn forget_git_hot_row(&mut self, hwnd: HWND) {
        if self.git_hot_row().is_some() {
            self.clear_hot(hwnd);
        }
    }

    pub(super) fn git_primary(&self) -> GitPrimary {
        if self.git_busy {
            GitPrimary::Busy
        } else if !self.commit_message.trim().is_empty() || !self.changes.is_empty() {
            GitPrimary::Commit {
                ready: !self.commit_message.trim().is_empty(),
            }
        } else if self.git_behind > 0 {
            GitPrimary::Pull(self.git_behind)
        } else if self.git_ahead > 0 {
            GitPrimary::Push(self.git_ahead)
        } else {
            GitPrimary::Commit { ready: false }
        }
    }

    /// A click on one of the panel's buttons outside the list.
    pub(super) fn git_button_pressed(&mut self, hwnd: HWND, button: GitButton, layout: &GitLayout) {
        match button {
            GitButton::Commit => self.git_commit_pressed(hwnd),
            GitButton::Refresh => self.refresh_git(),
            GitButton::Primary => match self.git_primary() {
                GitPrimary::Busy => {}
                GitPrimary::Commit { .. } => self.git_commit_pressed(hwnd),
                GitPrimary::Push(_) => self.git_remote(hwnd, workflow::RemoteAction::Push),
                GitPrimary::Pull(_) => self.git_remote(hwnd, workflow::RemoteAction::Pull),
            },
            GitButton::Pull => self.git_remote(hwnd, workflow::RemoteAction::Pull),
            GitButton::Push => self.git_remote(hwnd, workflow::RemoteAction::Push),
            GitButton::Fetch => self.git_remote(hwnd, workflow::RemoteAction::Fetch),
            GitButton::More | GitButton::Drop => {
                let menu = if button == GitButton::More {
                    GitMenu::Header
                } else {
                    GitMenu::Commit
                };
                // The menu is placed in the window's coordinates.
                let rect = layout.button(button);
                let origin = self.side_origin();
                self.toggle_git_menu(
                    hwnd,
                    menu,
                    RECT {
                        top: rect.top + origin,
                        bottom: rect.bottom + origin,
                        ..rect
                    },
                );
            }
        }
    }

    /// Open a changed file in the editor.
    pub(super) fn git_open_row(&mut self, hwnd: HWND, index: usize) {
        let Some(GitRow::Change { change, .. }) = self.git_rows().into_iter().nth(index) else {
            return;
        };
        let path = match self.git_root.clone().or(self.workspace_root.clone()) {
            Some(root) if !change.path.is_absolute() => root.join(&change.path),
            _ => change.path,
        };
        self.panel_focus = false;
        self.open(hwnd, Some(path));
    }

    /// Discard every unstaged change, after asking.
    pub(super) fn git_discard_all(&mut self, hwnd: HWND) {
        let unstaged: Vec<Change> = self
            .changes
            .iter()
            .filter(|change| change.unstaged)
            .cloned()
            .collect();
        if unstaged.is_empty() {
            self.status = "No changes to discard".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let new_files = unstaged.iter().filter(|change| change.untracked).count();
        let mut message = match unstaged.len() {
            1 => "Discard the changes in 1 file?".to_owned(),
            count => format!("Discard the changes in {count} files?"),
        };
        match new_files {
            0 => {}
            1 => message.push_str(" 1 new file will be deleted."),
            count => message.push_str(&format!(" {count} new files will be deleted.")),
        }
        message.push_str(" Git cannot recover them once discarded.");
        if !self.confirm_git(hwnd, "Discard all changes", &message) {
            return;
        }
        let (untracked, tracked): (Vec<Change>, Vec<Change>) =
            unstaged.into_iter().partition(|change| change.untracked);
        self.git_write(
            hwnd,
            GitAction::Discard {
                paths: tracked.into_iter().map(|change| change.path).collect(),
                untracked: untracked.into_iter().map(|change| change.path).collect(),
            },
        );
    }

    pub(super) fn git_section_collapsed(&self, section: GitSection) -> bool {
        match section {
            GitSection::Staged => self.git_staged_collapsed,
            GitSection::Changes => self.git_changes_collapsed,
            GitSection::History => self.git_history_collapsed,
        }
    }

    pub(super) fn git_toggle_section(&mut self, hwnd: HWND, section: GitSection) {
        let collapsed = match section {
            GitSection::Staged => &mut self.git_staged_collapsed,
            GitSection::Changes => &mut self.git_changes_collapsed,
            GitSection::History => &mut self.git_history_collapsed,
        };
        *collapsed = !*collapsed;

        let rows = self.git_rows();
        self.panel_first = self.panel_first.min(rows.len().saturating_sub(1));
        self.panel_selected = self.panel_selected.min(rows.len().saturating_sub(1));
        if !rows
            .get(self.panel_selected)
            .is_some_and(GitRow::selectable)
            && let Some(index) = rows.iter().position(GitRow::selectable)
        {
            self.panel_selected = index;
        }
        self.git_scroll_into_view(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Move the selection to the next row Up/Down can act on.
    pub(super) fn git_move_selection(&mut self, hwnd: HWND, delta: i32) {
        let rows = self.git_rows();
        if rows.is_empty() {
            return;
        }
        let mut index = self.panel_selected.min(rows.len() - 1);
        for _ in 0..rows.len() {
            if delta < 0 {
                index = index.wrapping_sub(1);
                if index > rows.len() {
                    index = rows.len() - 1;
                }
            } else {
                index = (index + 1) % rows.len();
            }
            if rows.get(index).is_some_and(GitRow::selectable) {
                break;
            }
        }
        self.panel_selected = index;
        self.git_scroll_into_view(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    fn git_scroll_into_view(&mut self, hwnd: HWND) {
        let rows = self.git_rows().len();
        let visible = self.git_visible_rows(hwnd).max(1);
        if self.panel_selected < self.panel_first {
            self.panel_first = self.panel_selected;
        }
        if self.panel_selected >= self.panel_first + visible {
            self.panel_first = self.panel_selected + 1 - visible;
        }
        self.panel_first = self.panel_first.min(rows);
    }

    pub(super) fn git_visible_rows(&self, hwnd: HWND) -> usize {
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) };
        let layout = self.git_layout(self.scale(RAIL), self.sidebar_right());
        // The layout is in the panel's coordinates, which start at
        // side_origin in the window.
        let available =
            (rect.bottom - self.scale(STATUS) - self.side_origin() - layout.list_top).max(0);
        (available / self.scale(ROW_CHANGE).max(1)).max(3) as usize
    }

    pub(super) fn git_activate_row(&mut self, hwnd: HWND, index: usize) {
        match self.git_rows().into_iter().nth(index) {
            Some(GitRow::Change { change, staged }) => {
                self.show_diff(hwnd, change.path, staged);
            }
            Some(GitRow::Commit(entry)) => {
                self.status = format!("{}  ·  {}", entry.oid, entry.subject);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            _ => {}
        }
    }

    /// Leave the diff and open the reviewed file at the line under `y`. Diff row
    /// numbers are 1-based, and the after side is the file as it stands now.
    pub(super) fn open_diff_line(&mut self, hwnd: HWND, y: i32) {
        let top = self.editor_top() + self.scale(43);
        if y < top || self.line_height <= 0 {
            return;
        }
        let index = self.diff_first + ((y - top) / self.line_height) as usize;
        let Some(number) = self
            .diff_rows
            .get(index)
            .and_then(|row| row.after_number.or(row.before_number))
        else {
            return;
        };
        let Some(relative) = self.review_file.clone() else {
            return;
        };
        let path = match self.git_root.clone().or(self.workspace_root.clone()) {
            Some(root) if !relative.is_absolute() => root.join(relative),
            _ => relative,
        };
        self.close_diff();
        self.diff_rows.clear();
        self.diff_first = 0;
        self.panel_focus = true;
        let label = display_path(&path);
        self.open(hwnd, Some(path));
        let line = {
            let doc = self.doc();
            number
                .saturating_sub(1)
                .min(doc.line_count().saturating_sub(1))
        };
        self.move_cursor(Pos { line, byte: 0 }, false);
        self.keep_cursor_visible(hwnd);
        self.status = format!("Line {number} of {label}");
    }

    pub(super) fn git_toggle_row(&mut self, hwnd: HWND, index: usize) {
        match self.git_rows().into_iter().nth(index) {
            Some(GitRow::Change { staged, change }) if staged => {
                self.git_write(hwnd, GitAction::Unstage(vec![change.path]));
            }
            Some(GitRow::Change { change, .. }) => {
                self.git_write(hwnd, GitAction::Stage(vec![change.path]));
            }
            _ => {}
        }
    }

    pub(super) fn git_discard_row(&mut self, hwnd: HWND, index: usize) {
        let Some(GitRow::Change { change, staged }) = self.git_rows().into_iter().nth(index) else {
            return;
        };
        if staged {
            return;
        }
        // Losing work is the one thing here that cannot be undone, so the
        // dialog names the file and says so.
        let message = if change.untracked {
            format!(
                "Delete {}? It was never committed, so Git cannot recover it.",
                display_path(&change.path)
            )
        } else {
            format!(
                "Discard changes in {}? Git cannot recover them once discarded.",
                display_path(&change.path)
            )
        };
        if !self.confirm_git(hwnd, "Discard changes", &message) {
            return;
        }
        let (paths, untracked) = if change.untracked {
            (Vec::new(), vec![change.path])
        } else {
            (vec![change.path], Vec::new())
        };
        self.git_write(hwnd, GitAction::Discard { paths, untracked });
    }

    pub(super) fn git_stage_all(&mut self, hwnd: HWND) {
        self.git_write(hwnd, GitAction::Stage(Vec::new()));
    }

    pub(super) fn git_unstage_all(&mut self, hwnd: HWND) {
        self.git_write(hwnd, GitAction::Unstage(Vec::new()));
    }

    pub(super) fn git_commit_pressed(&mut self, hwnd: HWND) {
        if self.commit_message.trim().is_empty() {
            self.status = "Type a commit message first".into();
            self.commit_focus = true;
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        if !self.changes.iter().any(|change| change.staged) {
            let message = "Nothing is staged. Stage every change and commit?";
            if !self.confirm_git(hwnd, "Commit", message) {
                return;
            }
            let action = GitAction::Stage(Vec::new());
            if !self.git_write(hwnd, action) {
                return;
            }
            // The commit itself waits for the stage to land; a stale index
            // would otherwise commit an empty tree.
            self.commit_after_stage = true;
            return;
        }
        self.git_write(hwnd, GitAction::Commit);
    }

    fn confirm_git(&mut self, hwnd: HWND, title: &str, message: &str) -> bool {
        self.panel_focus = true;
        unsafe { InvalidateRect(hwnd, null(), 0) };
        dialog::show_dialog(
            hwnd,
            title,
            message,
            dialog::DialogIcon::Question,
            &[
                dialog::DialogButton {
                    label: "Yes",
                    id: dialog::DLG_YES,
                    is_default: false,
                    is_cancel: false,
                },
                dialog::DialogButton {
                    label: "Cancel",
                    id: dialog::DLG_CANCEL,
                    is_default: true,
                    is_cancel: true,
                },
            ],
        ) == dialog::DLG_YES
    }

    /// Run one write on the worker channel. Only one may be in flight: Git
    /// takes an index lock, and two of our own commands would fight over it.
    /// The answer says whether the action was accepted, because a caller that
    /// queued a follow-up must not wait for a completion that never comes.
    pub(super) fn git_write(&mut self, hwnd: HWND, action: GitAction) -> bool {
        let Some((root, _)) = self.git_paths() else {
            self.status = "Open a Git workspace to change source control".into();
            return false;
        };
        if self.git_busy {
            self.status = "Waiting for the current Git command".into();
            return false;
        }
        // Git reads (stage, commit) or rewrites (discard) the files on disk.
        self.finish_background_save(hwnd);
        let message = self.commit_message.clone();
        self.status = match &action {
            GitAction::Stage(paths) => stage_text("Staging", paths),
            GitAction::Unstage(paths) => stage_text("Unstaging", paths),
            GitAction::Discard { .. } => "Discarding changes...".into(),
            GitAction::Commit => "Committing...".into(),
        };
        self.git_busy = true;
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            let result = match &action {
                GitAction::Stage(paths) => workflow::stage(&root, paths),
                GitAction::Unstage(paths) => workflow::unstage(&root, paths),
                GitAction::Discard { paths, untracked } => {
                    workflow::discard(&root, paths, untracked)
                }
                GitAction::Commit => workflow::commit(&root, &message),
            };
            tx.send(WorkerMessage::GitWrite(action, result));
        });
        unsafe { InvalidateRect(hwnd, null(), 0) };
        true
    }

    pub(super) fn git_write_finished(
        &mut self,
        hwnd: HWND,
        action: &GitAction,
        result: Result<(), String>,
    ) {
        self.git_busy = false;
        match result {
            Ok(()) => {
                let status = match action {
                    GitAction::Stage(paths) => stage_text("Staged", paths),
                    GitAction::Unstage(paths) => stage_text("Unstaged", paths),
                    GitAction::Discard { .. } => "Changes discarded".into(),
                    GitAction::Commit => "Committed".into(),
                };
                if matches!(action, GitAction::Commit) {
                    self.commit_message.clear();
                }
                self.status = status;
                if matches!(action, GitAction::Stage(_)) && self.commit_after_stage {
                    self.commit_after_stage = false;
                    self.git_write(hwnd, GitAction::Commit);
                    return;
                }
            }
            Err(error) => {
                self.commit_after_stage = false;
                self.error(hwnd, &error);
            }
        }
        self.refresh_git();
    }

    /// Push, pull and fetch run in the terminal panel, where Git's own output
    /// and its credential prompt stay visible instead of a hidden child.
    pub(super) fn git_remote(&mut self, hwnd: HWND, action: workflow::RemoteAction) {
        if self.workspace_root.is_none() {
            self.status = "Open a folder first".into();
            return;
        }
        let command = workflow::remote_command(action);
        self.run_in_terminal(hwnd, command);
        self.status = format!("Running {command} in the terminal");
    }

    /// Diff rows for `path` in the section the row came from.
    pub(super) fn git_diff_scope(staged: bool) -> DiffScope {
        if staged {
            DiffScope::Staged
        } else {
            DiffScope::Unstaged
        }
    }

    pub(super) fn git_head_label(&self) -> String {
        let branch = self.workspace_branch.clone().unwrap_or_default();
        if self.git_ahead == 0 && self.git_behind == 0 {
            return branch;
        }
        let mut parts = Vec::new();
        if self.git_ahead > 0 {
            parts.push(format!("↑{}", self.git_ahead));
        }
        if self.git_behind > 0 {
            parts.push(format!("↓{}", self.git_behind));
        }
        format!("{branch}  {}", parts.join(" "))
    }
}

fn stage_text(verb: &str, paths: &[PathBuf]) -> String {
    match paths.len() {
        0 => format!("{verb} everything..."),
        1 => format!("{verb} {}", display_path(&paths[0])),
        count => format!("{verb} {count} files"),
    }
}

fn contains(rect: &RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}
