use super::*;

// Source control sidebar geometry, in logical pixels below the panel header.
// The commit message box and button stay pinned while the change list scrolls,
// so their rectangles come from the same layout as the rows.
const COMMIT_TOP: i32 = 50;
const COMMIT_HEIGHT: i32 = 54;
const COMMIT_BUTTON_HEIGHT: i32 = 34;
// Push, pull and fetch sit in a row of their own below the commit button.
const SYNC_HEIGHT: i32 = 34;
const BRANCH_HEIGHT: i32 = 38;
pub(super) const ROW_HEADER: i32 = 34;
pub(super) const ROW_CHANGE: i32 = EXPLORER_ROW;
pub(super) const ROW_COMMIT: i32 = 48;
pub(super) const ROW_CLEAN: i32 = 82;
// Row action buttons are square and right aligned, like the rest of the panel.
pub(super) const ROW_BUTTON: i32 = 22;

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

// Where a click landed. Rows are addressed by index into `git_rows`.
pub(super) enum GitHit {
    CommitBox,
    CommitButton,
    StageAll,
    UnstageAll,
    Refresh,
    Push,
    Pull,
    Fetch,
    ToggleSection(GitSection),
    Row(usize),
    Toggle(usize),
    Discard(usize),
    Nothing,
}

pub(super) struct GitLayout {
    pub commit_box: RECT,
    pub commit_button: RECT,
    pub sync: [RECT; 3],
    pub branch: RECT,
    pub refresh: RECT,
    pub list_top: i32,
}

impl App {
    fn git_paths(&self) -> Option<(PathBuf, Vec<PathBuf>)> {
        let root = self.git_root.clone().or(self.workspace_root.clone())?;
        Some((root, Vec::new()))
    }

    /// Ask Git for branch, changes and history without touching the UI thread.
    /// Called on open, on activation, after a save and after every write.
    pub(super) fn refresh_git(&mut self, hwnd: HWND) {
        let Some(root) = self.git_root.clone().or(self.workspace_root.clone()) else {
            return;
        };
        self.git_generation += 1;
        let generation = self.git_generation;
        self.review_loading = true;
        let tx = self.worker_tx.clone();
        self.worker_started(hwnd);
        std::thread::spawn(move || {
            let result = workflow::repo_state(&root);
            let _ = tx.send(WorkerMessage::Repo(generation, result));
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
        }
        let watch: Vec<PathBuf> = state
            .git_dir
            .iter()
            .flat_map(|dir| [dir.join("index"), dir.join("HEAD")])
            .collect();
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
        // Keep the selection inside the list after a refresh shrinks it, and
        // keep it on screen: the row count changes under the pointer whenever
        // a file is saved or a write lands.
        let rows = self.git_rows().len();
        self.panel_selected = self.panel_selected.min(rows.saturating_sub(1));
        self.git_scroll_into_view(hwnd);
        if head_moved {
            self.refresh_active_git_diff(hwnd);
        }
        // The status line belongs to whatever the user last asked for, so the
        // counts here stay in the section headers instead of overwriting it.
        self.refresh(hwnd);
    }

    /// The list model, built fresh so paint and hit-testing always agree.
    pub(super) fn git_rows(&self) -> Vec<GitRow> {
        let mut rows = Vec::new();
        if self.review_loading && self.changes.is_empty() {
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
                title: "STAGED",
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
        let box_left = left + self.scale(12);
        let box_right = right - self.scale(12);
        let top = self.scale(COMMIT_TOP);
        let height = self.scale(COMMIT_HEIGHT);
        let button_top = top + height + self.scale(8);
        let button_bottom = button_top + self.scale(COMMIT_BUTTON_HEIGHT);
        let sync_top = button_bottom + self.scale(8);
        let sync_bottom = sync_top + self.scale(SYNC_HEIGHT);
        let branch_top = sync_bottom + self.scale(10);
        let branch_bottom = branch_top + self.scale(BRANCH_HEIGHT);
        let gap = self.scale(6);
        let third = ((box_right - box_left - gap * 2) / 3).max(1);
        GitLayout {
            commit_box: RECT {
                left: box_left,
                top,
                right: box_right,
                bottom: top + height,
            },
            commit_button: RECT {
                left: box_left,
                top: button_top,
                right: box_right,
                bottom: button_bottom,
            },
            // Push, pull, fetch - left to right, each labelled with its own name
            // so the three download arrows do not have to be told apart.
            sync: [0, 1, 2].map(|index| {
                let left = box_left + (third + gap) * index;
                RECT {
                    left,
                    top: sync_top,
                    right: left + third,
                    bottom: sync_bottom,
                }
            }),
            branch: RECT {
                left: box_left,
                top: branch_top,
                right: box_right,
                bottom: branch_bottom,
            },
            refresh: RECT {
                left: right - self.scale(34),
                top: self.scale(12),
                right: right - self.scale(10),
                bottom: self.scale(34),
            },
            list_top: branch_bottom + self.scale(8),
        }
    }

    /// The two square buttons at the right edge of a change row.
    pub(super) fn git_row_buttons(
        &self,
        right: i32,
        rect: RECT,
        staged: bool,
    ) -> (RECT, Option<RECT>) {
        let size = self.scale(ROW_BUTTON);
        let middle = (rect.top + rect.bottom) / 2;
        let edge = right - self.scale(8);
        let toggle = RECT {
            left: edge - size,
            top: middle - size / 2,
            right: edge,
            bottom: middle + size / 2,
        };
        let discard = (!staged).then(|| RECT {
            left: toggle.left - size - self.scale(2),
            top: toggle.top,
            right: toggle.right - size - self.scale(2),
            bottom: toggle.bottom,
        });
        (toggle, discard)
    }

    pub(super) fn git_hit(&self, x: i32, y: i32, left: i32, right: i32) -> GitHit {
        let layout = self.git_layout(left, right);
        if contains(&layout.commit_box, x, y) {
            return GitHit::CommitBox;
        }
        if contains(&layout.commit_button, x, y) {
            return GitHit::CommitButton;
        }
        match layout.sync.iter().position(|rect| contains(rect, x, y)) {
            Some(0) => return GitHit::Push,
            Some(1) => return GitHit::Pull,
            Some(2) => return GitHit::Fetch,
            _ => {}
        }
        if contains(&layout.refresh, x, y) {
            return GitHit::Refresh;
        }
        // The section headers carry their own all-rows buttons.
        if y < layout.list_top {
            return GitHit::Nothing;
        }
        let rows = self.git_rows();
        let mut top = layout.list_top;
        for (index, row) in rows.iter().enumerate().skip(self.panel_first) {
            let height = self.scale(row.height());
            let rect = RECT {
                left,
                top,
                right,
                bottom: top + height,
            };
            if y >= rect.bottom {
                top = rect.bottom;
                continue;
            }
            match row {
                GitRow::Change { staged, .. } => {
                    let (toggle, discard) = self.git_row_buttons(right, rect, *staged);
                    if contains(&toggle, x, y) {
                        return GitHit::Toggle(index);
                    }
                    if discard.is_some_and(|rect| contains(&rect, x, y)) {
                        return GitHit::Discard(index);
                    }
                    return GitHit::Row(index);
                }
                GitRow::Header { section, .. } => {
                    let action = match section {
                        GitSection::Changes => Some(GitHit::StageAll),
                        GitSection::Staged => Some(GitHit::UnstageAll),
                        GitSection::History => None,
                    };
                    if let Some(action) = action {
                        let size = self.scale(ROW_BUTTON);
                        let edge = right - self.scale(8);
                        let rect = RECT {
                            left: edge - size,
                            top: rect.top,
                            right: edge,
                            bottom: rect.bottom,
                        };
                        if contains(&rect, x, y) {
                            return action;
                        }
                    }
                    return GitHit::ToggleSection(*section);
                }
                GitRow::Commit(_) => return GitHit::Row(index),
                GitRow::Clean | GitRow::Note(_) => {}
            }
            return GitHit::Nothing;
        }
        GitHit::Nothing
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
        let layout = self.git_layout(self.scale(RAIL), self.editor_left());
        let available = (rect.bottom - self.scale(STATUS) - layout.list_top).max(0);
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
        self.review_file = None;
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
        let message = self.commit_message.clone();
        self.status = match &action {
            GitAction::Stage(paths) => stage_text("Staging", paths),
            GitAction::Unstage(paths) => stage_text("Unstaging", paths),
            GitAction::Discard { .. } => "Discarding changes...".into(),
            GitAction::Commit => "Committing...".into(),
        };
        self.git_busy = true;
        let tx = self.worker_tx.clone();
        self.worker_started(hwnd);
        std::thread::spawn(move || {
            let result = match &action {
                GitAction::Stage(paths) => workflow::stage(&root, paths),
                GitAction::Unstage(paths) => workflow::unstage(&root, paths),
                GitAction::Discard { paths, untracked } => {
                    workflow::discard(&root, paths, untracked)
                }
                GitAction::Commit => workflow::commit(&root, &message),
            };
            let _ = tx.send(WorkerMessage::GitWrite(action, result));
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
        self.refresh_git(hwnd);
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
