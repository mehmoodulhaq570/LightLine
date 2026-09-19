use super::*;

impl App {
    pub(super) fn worker_started(&mut self, hwnd: HWND) {
        self.pending_workers += 1;
        unsafe { SetTimer(hwnd, 4, 60, None) };
    }

    pub(super) fn show_quick_open(&mut self, hwnd: HWND) {
        self.quick_open = true;
        self.panel_focus = false;
        self.terminal_focus = false;
        self.quick_query.clear();
        self.quick_selected = 0;
        self.quick_loading = false;
        if let Some(root) = self.workspace_root.clone() {
            self.quick_files.clear();
            self.quick_loading = true;
            let tx = self.worker_tx.clone();
            self.worker_started(hwnd);
            std::thread::spawn(move || {
                let _ = tx.send(WorkerMessage::Files(
                    root.clone(),
                    workflow::workspace_files(&root),
                ));
            });
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn quick_commands(&self) -> Vec<(&'static str, u8)> {
        let query = self
            .quick_query
            .trim_start_matches('>')
            .trim()
            .to_ascii_lowercase();
        [
            ("Search in files", 0),
            ("Run Rust tests", 1),
            ("Run Python File", 8),
            ("Review Git changes", 2),
            ("Open folder", 3),
            ("New file", 4),
            ("Select Python interpreter", 5),
            ("Select Python virtual environment", 6),
            (
                if self.split_visible {
                    "Close editor split"
                } else {
                    "Split editor"
                },
                7,
            ),
            ("New Terminal", 9),
        ]
        .into_iter()
        .filter(|(name, _)| name.to_ascii_lowercase().contains(&query))
        .collect()
    }

    pub(super) fn quick_count(&self) -> usize {
        if self.quick_query.starts_with('>') {
            self.quick_commands().len()
        } else {
            self.quick_matches().len()
        }
    }

    pub(super) fn activate_quick_item(&mut self, hwnd: HWND, index: usize) {
        if self.quick_query.starts_with('>') {
            let action = self.quick_commands().get(index).map(|(_, action)| *action);
            self.quick_open = false;
            self.backbuffer = None;
            match action {
                Some(0) => self.open_project_search(hwnd),
                Some(1) => self.run_project(hwnd),
                Some(8) => self.run_python_file(hwnd),
                Some(2) => self.show_review(hwnd),
                Some(3) => self.open_folder(hwnd),
                Some(4) => self.new_file(hwnd),
                Some(5) => self.select_python_interpreter(hwnd),
                Some(6) => self.select_python_environment(hwnd),
                Some(7) => self.toggle_split(hwnd),
                Some(9) => {
                    self.open_terminal(hwnd);
                }
                _ => {}
            }
        } else {
            let path = self.quick_matches().get(index).cloned();
            self.quick_open = false;
            if let Some(path) = path {
                self.backbuffer = None;
                self.open(hwnd, Some(path));
            }
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn open_project_search(&mut self, hwnd: HWND) {
        if self.workspace_root.is_none() {
            self.status = "Open a workspace to search files".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        self.welcome = false;
        self.set_sidebar_visible(hwnd, true);
        self.side_view = SideView::Search;
        self.review_file = None;
        self.search_input = true;
        self.panel_focus = true;
        self.terminal_focus = false;
        self.panel_first = 0;
        self.panel_selected = 0;
        self.update_title(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn quick_matches(&self) -> Vec<PathBuf> {
        let query = self.quick_query.to_ascii_lowercase();
        self.quick_files
            .iter()
            .filter(|path| {
                path.strip_prefix(self.workspace_root.as_deref().unwrap_or(Path::new("")))
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .contains(&query)
            })
            .take(8)
            .cloned()
            .collect()
    }

    pub(super) fn search_project(&mut self, hwnd: HWND) {
        let Some(root) = self.workspace_root.clone() else {
            self.status = "Open a workspace to search files".into();
            return;
        };
        let query = self.project_query.clone();
        if query.is_empty() {
            self.cancel_search();
            self.search_results.clear();
            return;
        }
        self.cancel_search();
        let cancel = Arc::new(AtomicBool::new(false));
        self.search_cancel = Some(cancel.clone());
        self.status = format!("Searching for {query}...");
        self.panel_selected = 0;
        self.panel_first = 0;
        let tx = self.worker_tx.clone();
        self.worker_started(hwnd);
        std::thread::spawn(move || {
            let hits = workflow::search_workspace_with_cancel(&root, &query, &cancel);
            let _ = tx.send(WorkerMessage::Search(root, query, cancel, hits));
        });
    }

    pub(super) fn cancel_search(&mut self) {
        if let Some(cancel) = self.search_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn run_project(&mut self, hwnd: HWND) {
        if self.workspace_root.is_none() {
            self.status = "Open a Rust workspace to run tests".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        self.run_in_terminal(hwnd, "cargo test --offline");
    }

    pub(super) fn run_python_file(&mut self, hwnd: HWND) {
        if !Tab::is_python(self.doc()) {
            self.status = "Open a Python file to run it".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        if self.doc().is_dirty() && !self.save(hwnd, false) {
            self.status = "Save the Python file before running it".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let Some(file) = self.doc().path.clone() else {
            self.status = "Save the Python file before running it".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        let root = workflow::python_project_root(&file, self.workspace_root.as_deref());
        let interpreter = match self.python_interpreter.clone() {
            Some(interpreter) => interpreter,
            None => match workflow::detect_python_interpreter(Some(&root)) {
                Some(detected) => {
                    self.python_interpreter = Some(detected.clone());
                    self.status =
                        format!("Using Python interpreter {}", detected.to_string_lossy());
                    detected
                }
                None => {
                    self.status =
                        "Select a Python interpreter or virtual environment before running"
                            .into();
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    return;
                }
            },
        };
        let command = format!(
            "{} -u {}",
            terminal::powershell_quoted(&interpreter),
            terminal::powershell_quoted(&file)
        );
        self.run_in_terminal(hwnd, &command);
    }

    pub(super) fn show_review(&mut self, hwnd: HWND) {
        let Some(root) = self.workspace_root.clone() else {
            self.status = "Open a Git workspace to review changes".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        self.welcome = false;
        self.cancel_search();
        self.search_input = false;
        self.terminal_focus = false;
        self.update_title(hwnd);
        self.set_sidebar_visible(hwnd, true);
        self.side_view = SideView::Review;
        self.panel_focus = true;
        self.panel_selected = 0;
        self.panel_first = 0;
        self.status = "Loading Git changes...".into();
        self.review_loading = true;
        let tx = self.worker_tx.clone();
        self.worker_started(hwnd);
        std::thread::spawn(move || {
            let result = workflow::git_changes(&root);
            let _ = tx.send(WorkerMessage::Changes(root, result));
        });
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn show_diff(&mut self, hwnd: HWND, path: PathBuf) {
        let Some(root) = self.workspace_root.clone() else {
            return;
        };
        self.review_file = Some(path.clone());
        self.diff_rows.clear();
        self.diff_first = 0;
        self.status = format!("Reviewing {}", path.display());
        let tx = self.worker_tx.clone();
        self.worker_started(hwnd);
        std::thread::spawn(move || {
            let result = workflow::git_diff(&root, &path);
            let _ = tx.send(WorkerMessage::Diff(root, path, result));
        });
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn poll_workers(&mut self, hwnd: HWND) {
        let mut received = false;
        while let Ok(message) = self.worker_rx.try_recv() {
            received = true;
            self.pending_workers = self.pending_workers.saturating_sub(1);
            match message {
                WorkerMessage::Files(root, files)
                    if self.workspace_root.as_ref() == Some(&root) =>
                {
                    self.quick_loading = false;
                    self.quick_files = files;
                }
                WorkerMessage::Search(root, query, cancel, hits)
                    if self.workspace_root.as_ref() == Some(&root)
                        && self.project_query == query
                        && self
                            .search_cancel
                            .as_ref()
                            .is_some_and(|current| Arc::ptr_eq(current, &cancel)) =>
                {
                    self.search_cancel = None;
                    self.status = format!("{} results for {}", hits.len(), query);
                    self.search_results = hits;
                }
                WorkerMessage::Changes(root, result)
                    if self.workspace_root.as_ref() == Some(&root) =>
                {
                    self.review_loading = false;
                    match result {
                        Ok(changes) => {
                            self.status = format!("{} changed files", changes.len());
                            self.changes = changes;
                        }
                        Err(error) => self.status = error,
                    }
                }
                WorkerMessage::Diff(root, path, result)
                    if self.workspace_root.as_ref() == Some(&root)
                        && self.review_file.as_ref() == Some(&path) =>
                {
                    match result {
                        Ok(rows) => self.diff_rows = rows,
                        Err(error) => self.status = error,
                    }
                }
                _ => {}
            }
        }
        if self.pending_workers == 0 {
            unsafe { KillTimer(hwnd, 4) };
        }
        if received {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }
}
