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
                if entry.is_dir && entry.path.file_name().is_some_and(|name| name == "src") {
                    0
                } else if entry.is_dir {
                    1
                } else {
                    2
                }
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

    pub(super) fn head_branch(root: &Path) -> Option<String> {
        let head = std::fs::read_to_string(root.join(".git").join("HEAD")).ok()?;
        head.trim()
            .strip_prefix("ref: refs/heads/")
            .map(str::to_owned)
    }

    pub(super) fn set_workspace(&mut self, hwnd: HWND, root: PathBuf) {
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        if !root.is_dir() {
            return;
        }
        self.workspace_root = Some(root.clone());
        self.workspace_branch = Self::head_branch(&root);
        self.directory_cache.clear();
        self.expanded_dirs.clear();
        self.expanded_dirs.insert(root.clone());
        self.explorer_first_row = 0;
        self.quick_files.clear();
        self.quick_loading = false;
        self.cancel_search();
        self.search_results.clear();
        self.changes.clear();
        self.review_loading = false;
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
        self.recent = workflow::recent_workspaces();
        self.status = format!(
            "Workspace: {}",
            root.file_name().unwrap_or_default().to_string_lossy()
        );
        self.show_active_tab(hwnd);
    }

    pub(super) fn folder_dialog(&self, hwnd: HWND) -> Option<PathBuf> {
        file_dialog::pick_folder(hwnd, "Choose a LightLine workspace folder")
    }

    pub(super) fn open_folder(&mut self, hwnd: HWND) {
        if let Some(root) = self.folder_dialog(hwnd) {
            self.set_workspace(hwnd, root);
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

    pub(super) fn explorer_rows(&self) -> Vec<ExplorerRow> {
        let mut rows = Vec::new();
        if let Some(root) = &self.workspace_root {
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
