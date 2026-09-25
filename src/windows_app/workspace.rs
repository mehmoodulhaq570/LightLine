use super::*;

impl App {
    pub(super) fn load_directory(&mut self, path: &Path) {
        if self.directory_cache.contains_key(path) {
            return;
        }
        let mut entries: Vec<ExplorerEntry> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .take(400)
            .filter_map(|entry| {
                if entry.file_name().to_str() == Some(".git") {
                    return None;
                }
                let is_dir = entry.file_type().ok()?.is_dir();
                Some(ExplorerEntry {
                    path: entry.path(),
                    is_dir,
                })
            })
            .collect();
        entries.sort_by(|a, b| {
            let rank = |entry: &ExplorerEntry| {
                if entry.is_dir { 0 } else { 1 }
            };
            rank(a).cmp(&rank(b)).then_with(|| {
                a.path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase()
                    .cmp(
                        &b.path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_lowercase(),
                    )
            })
        });
        self.directory_cache.insert(path.to_path_buf(), entries);
    }

    pub(super) fn set_workspace_from_file(&mut self, path: &Path) {
        if self.workspace_root.is_some() {
            return;
        }
        if let Some(parent) = path.parent() {
            let folder = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
            let root = folder
                .ancestors()
                .take(8)
                .find(|dir| dir.join("Cargo.toml").is_file() || dir.join(".git").exists())
                .unwrap_or(&folder)
                .to_path_buf();
            self.workspace_root = Some(root.clone());
            self.workspace_branch = Self::head_branch(&root);
            self.expanded_dirs.insert(root.clone());
            self.load_directory(&root);
            workflow::remember_workspace(&root);
            self.recent = workflow::recent_workspaces();
        }
    }

    /// Which commit is checked out. Reading `.git/HEAD` as text is wrong for
    /// linked worktrees, submodules and detached HEAD, so ask Git instead.
    pub(super) fn head_branch(root: &Path) -> Option<String> {
        let root = workflow::repo_root(root)?;
        if let Ok(branch) =
            workflow::git_output(&root, &["symbolic-ref", "--short", "-q", "HEAD"])
        {
            let branch = branch.trim();
            if !branch.is_empty() {
                return Some(branch.to_owned());
            }
        }
        let sha = workflow::git_output(&root, &["rev-parse", "--short", "HEAD"]).ok()?;
        let sha = sha.trim();
        (!sha.is_empty()).then(|| sha.to_owned())
    }

    pub(super) fn set_workspace(&mut self, hwnd: HWND, root: PathBuf) {
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        if !root.is_dir() {
            return;
        }
        self.workspace_root = Some(root.clone());
        if let Some(watcher) = &self.watcher {
            watcher.watch_directory(root.clone());
        }
        self.workspace_branch = Self::head_branch(&root);
        self.directory_cache.clear();
        self.expanded_dirs.clear();
        self.expanded_dirs.insert(root.clone());
        self.explorer_first_row = 0;
        self.quick_files.clear();
        self.quick_loading = false;
        self.cancel_search();
        self.search_input = false;
        self.search_results.clear();
        self.panel_focus = false;
        self.changes.clear();
        self.review_loading = false;
        // Everything below belongs to the previous repository, including the
        // resolved top level, which can sit above the folder just opened.
        self.git_root = None;
        self.history.clear();
        self.git_ahead = 0;
        self.git_behind = 0;
        self.git_conflicted = false;
        self.git_busy = false;
        self.commit_after_stage = false;
        self.commit_message.clear();
        self.commit_focus = false;
        self.git_diff_cache.clear();
        self.git_head_cache.clear();
        self.git_untracked.clear();
        self.unwatch_git_files();
        self.gutter_done = None;
        self.review_file = None;
        self.reset_terminal_sessions(hwnd);
        self.welcome = false;
        self.explorer_visible = true;
        self.sidebar_width = SIDEBAR;
        self.sidebar_from = SIDEBAR;
        self.sidebar_target = SIDEBAR;
        self.sidebar_started = None;
        unsafe { KillTimer(hwnd, 5) };
        self.side_view = SideView::Files;
        self.panel_focus = false;
        self.lsp.clear();
        self.lsp_failed_at.clear();
        self.hover_target = None;
        self.hover_card = None;
        for tab in &mut self.tabs {
            tab.lsp_opened = false;
            tab.lsp_language = None;
            tab.diagnostics.clear();
        }
        self.load_directory(&root);
        workflow::remember_workspace(&root);
        self.refresh_git(hwnd);
        self.recent = workflow::recent_workspaces();
        self.status = format!(
            "Workspace: {}",
            root.file_name().unwrap_or_default().to_string_lossy()
        );
        self.show_active_tab(hwnd);
        self.save_session();
    }

    pub(super) fn folder_dialog(&self, hwnd: HWND) -> Option<PathBuf> {
        file_dialog::pick_folder(hwnd, "Choose a LightLine workspace folder")
    }

    pub(super) fn open_folder(&mut self, hwnd: HWND) {
        if let Some(root) = self.folder_dialog(hwnd) {
            self.set_workspace(hwnd, root);
        }
    }

    pub(super) fn close_workspace(&mut self, hwnd: HWND) {
        if !self.can_close_window(hwnd) {
            return;
        }
        self.workspace_root = None;
        self.workspace_branch = None;
        self.directory_cache.clear();
        self.expanded_dirs.clear();
        self.explorer_first_row = 0;
        self.quick_files.clear();
        self.quick_loading = false;
        self.cancel_search();
        self.search_input = false;
        self.search_results.clear();
        self.panel_focus = false;
        self.changes.clear();
        self.review_loading = false;
        self.git_root = None;
        self.history.clear();
        self.git_ahead = 0;
        self.git_behind = 0;
        self.git_conflicted = false;
        self.git_busy = false;
        self.commit_message.clear();
        self.git_diff_cache.clear();
        self.git_head_cache.clear();
        self.git_untracked.clear();
        self.unwatch_git_files();
        self.gutter_done = None;
        self.review_file = None;
        self.reset_terminal_sessions(hwnd);
        self.lsp.clear();
        self.lsp_failed_at.clear();
        self.hover_target = None;
        self.hover_card = None;
        self.tabs.clear();
        self.tabs.push(Tab::new(Document::new()));
        self.pane_tabs = [0, 0];
        self.active = 0;
        self.welcome = true;
        self.explorer_input = None;
        self.selected_explorer_path = None;
        self.status = "Workspace closed".into();
        self.update_title(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
        self.save_session();
    }

    pub(super) fn start_explorer_input(
        &mut self,
        target_dir: PathBuf,
        is_folder: bool,
        is_rename: bool,
        old_path: Option<PathBuf>,
        hwnd: HWND,
    ) {
        self.expanded_dirs.insert(target_dir.clone());
        self.explorer_input = Some(ExplorerInputState {
            is_folder,
            is_rename,
            target_dir,
            old_path,
            buffer: String::new(),
        });
        self.refresh(hwnd);
    }

    pub(super) fn create_file_at(&mut self, hwnd: HWND, parent: &Path, name: &str) {
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
            self.status = "Invalid file name".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let target = parent.join(name);
        if target.exists() {
            self.status = format!("File '{}' already exists", name);
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        match std::fs::File::create(&target) {
            Ok(_) => {
                self.directory_cache.remove(parent);
                self.load_directory(parent);
                self.expanded_dirs.insert(parent.to_path_buf());
                self.selected_explorer_path = Some(target.clone());
                self.open(hwnd, Some(target));
                self.status = format!("Created {}", name);
            }
            Err(e) => {
                self.status = format!("Failed to create {}: {}", name, e);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }
    }

    pub(super) fn create_folder_at(&mut self, hwnd: HWND, parent: &Path, name: &str) {
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
            self.status = "Invalid folder name".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let target = parent.join(name);
        if target.exists() {
            self.status = format!("Folder '{}' already exists", name);
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        match std::fs::create_dir_all(&target) {
            Ok(_) => {
                self.directory_cache.remove(parent);
                self.load_directory(parent);
                self.expanded_dirs.insert(parent.to_path_buf());
                self.expanded_dirs.insert(target.clone());
                self.selected_explorer_path = Some(target);
                self.status = format!("Created folder {}", name);
                self.refresh(hwnd);
            }
            Err(e) => {
                self.status = format!("Failed to create folder {}: {}", name, e);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }
    }

    pub(super) fn delete_entry(&mut self, hwnd: HWND, path: &Path) {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let is_dir = path.is_dir();
        let prompt = if is_dir {
            format!(
                "Are you sure you want to permanently delete folder '{}' and all its contents?",
                name
            )
        } else {
            format!("Are you sure you want to permanently delete '{}'?", name)
        };
        let res = dialog::show_dialog(
            hwnd,
            if is_dir { "Delete Folder" } else { "Delete File" },
            &prompt,
            dialog::DialogIcon::Question,
            &[dialog::BTN_DELETE, dialog::BTN_CANCEL],
        );
        if res != dialog::DLG_OK {
            return;
        }

        let mut i = 0;
        while i < self.tabs.len() {
            let tab_path = self.tabs[i].document.path.clone();
            let should_close = tab_path.as_deref().is_some_and(|tp| {
                tp == path || (is_dir && tp.starts_with(path))
            });
            if should_close {
                self.tabs[i].document.mark_clean();
                self.close_tab(hwnd, i);
            } else {
                i += 1;
            }
        }

        let delete_result = if is_dir {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };

        match delete_result {
            Ok(()) => {
                if let Some(parent) = path.parent() {
                    self.directory_cache.remove(parent);
                    self.load_directory(parent);
                }
                if is_dir {
                    self.expanded_dirs.remove(path);
                    self.directory_cache.remove(path);
                }
                if self.selected_explorer_path.as_deref() == Some(path) {
                    self.selected_explorer_path = None;
                }
                self.status = format!("Deleted {}", name);
                self.refresh(hwnd);
            }
            Err(e) => {
                self.status = format!("Failed to delete {}: {}", name, e);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }
    }

    pub(super) fn rename_entry(&mut self, hwnd: HWND, old_path: &Path, new_name: &str) {
        let new_name = new_name.trim();
        if new_name.is_empty() || new_name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
            self.status = "Invalid name".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let Some(parent) = old_path.parent() else {
            return;
        };
        let new_path = parent.join(new_name);
        if new_path.exists() {
            self.status = format!("'{}' already exists", new_name);
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        match std::fs::rename(old_path, &new_path) {
            Ok(_) => {
                for tab in &mut self.tabs {
                    if tab.document.path.as_deref() == Some(old_path) {
                        tab.document.path = Some(new_path.clone());
                    } else if let Some(p) = &tab.document.path
                        && p.starts_with(old_path)
                        && let Ok(rel) = p.strip_prefix(old_path)
                    {
                        tab.document.path = Some(new_path.join(rel));
                    }
                }
                self.directory_cache.remove(parent);
                self.load_directory(parent);
                if old_path.is_dir() {
                    self.expanded_dirs.remove(old_path);
                    self.expanded_dirs.insert(new_path.clone());
                    self.directory_cache.remove(old_path);
                }
                self.selected_explorer_path = Some(new_path.clone());
                self.status = format!("Renamed to {}", new_name);
                self.update_title(hwnd);
                self.refresh(hwnd);
            }
            Err(e) => {
                self.status = format!("Failed to rename: {}", e);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }
    }

    pub(super) fn reveal_file_in_explorer(&mut self, path: &Path) {
        let Some(root) = self.workspace_root.clone() else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        let Ok(relative) = parent.strip_prefix(&root) else {
            return;
        };
        let mut dir = root;
        for part in relative.components().take(8) {
            dir.push(part);
            self.expanded_dirs.insert(dir.clone());
            self.load_directory(&dir);
        }
    }

    pub(super) fn selected_dir_or_root(&self) -> Option<PathBuf> {
        if let Some(selected) = &self.selected_explorer_path {
            if selected.is_dir() {
                return Some(selected.clone());
            } else if let Some(parent) = selected.parent() {
                return Some(parent.to_path_buf());
            }
        }
        self.workspace_root.clone()
    }

    pub(super) fn collapse_all_folders(&mut self, hwnd: HWND) {
        if let Some(root) = &self.workspace_root {
            self.expanded_dirs.retain(|d| d == root);
        } else {
            self.expanded_dirs.clear();
        }
        self.explorer_first_row = 0;
        self.refresh(hwnd);
    }

    pub(super) fn explorer_rows(&self) -> Vec<ExplorerRow> {
        let mut rows = Vec::new();
        if let Some(root) = &self.workspace_root
            && self.expanded_dirs.contains(root)
        {
            self.append_explorer_rows(root, 0, &mut rows);
        }
        rows
    }

    pub(super) fn append_explorer_rows(
        &self,
        dir: &Path,
        depth: usize,
        rows: &mut Vec<ExplorerRow>,
    ) {
        if depth > 8 || rows.len() >= 250 {
            return;
        }
        if let Some(entries) = self.directory_cache.get(dir) {
            for entry in entries {
                if rows.len() >= 250 {
                    break;
                }
                let expanded = entry.is_dir && self.expanded_dirs.contains(&entry.path);
                rows.push(ExplorerRow {
                    entry: entry.clone(),
                    depth,
                    expanded,
                });
                if expanded {
                    self.append_explorer_rows(&entry.path, depth + 1, rows);
                }
            }
        }
    }
}
