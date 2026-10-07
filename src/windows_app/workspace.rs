use super::*;

impl App {
    pub(super) fn load_directory(&mut self, path: &Path) {
        if self.directory_cache.contains_key(path) || self.directory_requests.contains_key(path) {
            return;
        }
        // Watched before it is read, so a file created in between still
        // shows up on the watcher's next pass instead of being missed.
        if let Some(watcher) = &self.watcher {
            watcher.watch_directory(path.to_path_buf());
        }
        self.directory_request_serial += 1;
        let request = self.directory_request_serial;
        self.directory_requests.insert(path.to_owned(), request);
        let generation = self.workspace_generation;
        let path = path.to_owned();
        let tx = self.worker_tx.clone();
        self.worker_started(self.hwnd);
        std::thread::spawn(move || {
            let result = (|| {
                // Every entry: taking the first N in read_dir order (before sorting)
                // used to show an arbitrary subset of a large folder.
                let mut entries: Vec<ExplorerEntry> = std::fs::read_dir(&path)
                    .map_err(|e| e.to_string())?
                    .filter_map(Result::ok)
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
                // Folders first, then case-insensitive by name; the key is computed
                // once per entry rather than twice per comparison.
                entries.sort_by_cached_key(|entry| {
                    (
                        !entry.is_dir,
                        entry
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_lowercase(),
                    )
                });
                Ok(entries)
            })();
            let _ = tx.send(WorkerMessage::Directory(generation, request, path, result));
        });
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
            self.workspace_generation += 1;
            self.workspace_branch = None;
            self.refresh_git(self.hwnd);
            self.expanded_dirs.insert(root.clone());
            self.load_directory(&root);
            workflow::remember_workspace(&root);
            self.recent = workflow::recent_workspaces();
        }
    }

    pub(super) fn set_workspace(&mut self, hwnd: HWND, root: PathBuf) {
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        if !root.is_dir() {
            return;
        }
        self.workspace_root = Some(root.clone());
        self.workspace_generation += 1;
        // The previous workspace's folders stop being watched; load_directory
        // below starts watching the new root and each folder as it is shown.
        if let Some(watcher) = &self.watcher {
            watcher.unwatch_directories();
        }
        self.workspace_branch = None;
        self.directory_cache.clear();
        self.directory_requests.clear();
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
        self.workspace_generation += 1;
        self.workspace_branch = None;
        if let Some(watcher) = &self.watcher {
            watcher.unwatch_directories();
        }
        self.directory_cache.clear();
        self.directory_requests.clear();
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
        self.tabs.push(Tab::placeholder());
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

    pub(super) fn add_file_to_project(&mut self, hwnd: HWND, parent: &Path) {
        let Some(source) = self.pick_any_file(hwnd) else {
            return;
        };
        let Some(file_name) = source.file_name() else {
            self.status = "Invalid source file".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        let file_name_str = file_name.to_string_lossy().into_owned();
        let target = parent.join(file_name);

        if Self::same_path(&source, &target) {
            self.status = "Source and destination files are identical".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }

        // The copy runs on a worker so a large file or a slow drive doesn't
        // freeze the window; add_file_finished picks up the result.
        self.status = format!("Adding {}...", file_name_str);
        let parent = parent.to_path_buf();
        let generation = self.workspace_generation;
        let tx = self.worker_tx.clone();
        self.worker_started(hwnd);
        std::thread::spawn(move || {
            let result = copy_new_file(&source, &target).map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    format!("File '{}' already exists", file_name_str)
                } else {
                    format!("Failed to add {}: {}", file_name_str, error)
                }
            });
            let _ = tx.send(WorkerMessage::FileAdded(generation, parent, target, result));
        });
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn add_file_finished(
        &mut self,
        hwnd: HWND,
        generation: u64,
        parent: PathBuf,
        target: PathBuf,
        result: Result<(), String>,
    ) {
        // If the workspace was switched, reopened or closed during the copy,
        // the user has moved on: the result mustn't change the status or the
        // Explorer selection.
        let same_session = generation == self.workspace_generation;
        if let Err(error) = result {
            if same_session {
                self.status = error;
            }
            return;
        }
        // The new file is on disk either way. While its folder is in the open
        // workspace (e.g. the same folder was reopened), the folder's
        // listing and Git status still need refreshing so the file shows up.
        if self
            .workspace_root
            .as_ref()
            .is_some_and(|root| parent.starts_with(root))
        {
            self.directory_cache.remove(&parent);
            self.directory_requests.remove(&parent);
            if same_session {
                self.expanded_dirs.insert(parent.clone());
            }
            if self.workspace_root.as_ref() == Some(&parent) || self.expanded_dirs.contains(&parent)
            {
                self.load_directory(&parent);
            }
            self.refresh_git(hwnd);
        }
        if same_session {
            let name = target
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.status = format!("Added {}", name);
            self.selected_explorer_path = Some(target);
        }
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
                self.directory_requests.remove(parent);
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
                self.directory_requests.remove(parent);
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
            if is_dir {
                "Delete Folder"
            } else {
                "Delete File"
            },
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
            let should_close = tab_path
                .as_deref()
                .is_some_and(|tp| tp == path || (is_dir && tp.starts_with(path)));
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
                    self.directory_requests.remove(parent);
                    self.load_directory(parent);
                }
                if is_dir {
                    self.expanded_dirs.remove(path);
                    self.directory_cache.remove(path);
                    self.directory_requests.remove(path);
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
        if new_name.is_empty() || new_name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        {
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
                self.directory_requests.remove(parent);
                self.load_directory(parent);
                if old_path.is_dir() {
                    self.expanded_dirs.remove(old_path);
                    self.expanded_dirs.insert(new_path.clone());
                    self.directory_cache.remove(old_path);
                    self.directory_requests.remove(old_path);
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
        for part in relative.components() {
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

    // Every row of the expanded tree. There is deliberately no row or depth
    // cap: rows past a cap were silently unreachable. Painting only draws the
    // rows that fit on screen, starting at explorer_first_row.
    pub(super) fn explorer_rows(&self) -> Vec<ExplorerRow<'_>> {
        let mut rows = Vec::new();
        if let Some(root) = &self.workspace_root
            && self.expanded_dirs.contains(root)
        {
            self.append_explorer_rows(root, 0, &mut rows);
        }
        rows
    }

    pub(super) fn append_explorer_rows<'a>(
        &'a self,
        dir: &Path,
        depth: usize,
        rows: &mut Vec<ExplorerRow<'a>>,
    ) {
        if let Some(entries) = self.directory_cache.get(dir) {
            for entry in entries {
                let expanded = entry.is_dir && self.expanded_dirs.contains(&entry.path);
                rows.push(ExplorerRow {
                    entry,
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

// Copies `source` to a new file `target`. The existence check is made by the
// file system as `target` is created (create_new), so a file that appears
// after the user picked the source is never overwritten. A copy that fails
// part-way removes what it wrote rather than leaving a truncated file behind.
fn copy_new_file(source: &Path, target: &Path) -> io::Result<()> {
    let mut from = std::fs::File::open(source)?;
    let mut to = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    if let Err(error) = io::copy(&mut from, &mut to) {
        drop(to);
        let _ = std::fs::remove_file(target);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lightline-add-file-{}-{}",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copy_new_file_copies_contents() {
        let dir = scratch_dir("copy");
        let source = dir.join("source.bin");
        let target = dir.join("target.bin");
        std::fs::write(&source, b"\x00binary\xffdata").unwrap();
        copy_new_file(&source, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"\x00binary\xffdata");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_new_file_never_overwrites_an_existing_file() {
        let dir = scratch_dir("exists");
        let source = dir.join("source.txt");
        let target = dir.join("target.txt");
        std::fs::write(&source, "new").unwrap();
        std::fs::write(&target, "keep me").unwrap();
        let error = copy_new_file(&source, &target).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_new_file_leaves_nothing_when_the_source_is_missing() {
        let dir = scratch_dir("missing");
        let target = dir.join("target.txt");
        assert!(copy_new_file(&dir.join("missing.txt"), &target).is_err());
        assert!(!target.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
