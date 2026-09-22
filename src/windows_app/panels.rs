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
            ("Kill Active Terminal", 15),
            ("Restart Terminal", 10),
            ("Restart Terminal (No Profile)", 11),
            ("Go to Definition", 12),
            ("Format Document", 13),
            ("Trigger Completion", 14),
            ("Open Settings (JSON)", 16),
            ("Find in File", 17),
            ("Find and Replace", 18),
            ("Run C/C++ File", 19),
            ("Find All References", 20),
        ]
        .into_iter()
        .filter(|(name, _)| name.to_ascii_lowercase().contains(&query))
        .collect()
    }

    // Matches the 7-row render cap in paint_quick_open, so keyboard navigation
    // never selects a row that isn't actually visible.
    pub(super) fn quick_count(&self) -> usize {
        if self.quick_query.starts_with('>') {
            self.quick_commands().len().min(7)
        } else {
            self.quick_matches().len().min(7)
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
                    self.new_terminal(hwnd, false);
                }
                Some(15) => self.close_active_terminal(hwnd),
                Some(10) => self.restart_terminal(hwnd, false),
                Some(11) => self.restart_terminal(hwnd, true),
                Some(12) => self.goto_definition(hwnd),
                Some(20) => self.find_references(hwnd),
                Some(13) => self.format_document(hwnd),
                Some(14) => self.trigger_completion(hwnd),
                Some(16) => self.open_settings(hwnd),
                Some(17) => {
                    self.search_input = false;
                    self.panel_focus = false;
                    self.find_mode = true;
                    self.replace_mode = false;
                    self.find_query.clear();
                    self.status = "Find: ".into();
                }
                Some(18) => {
                    self.search_input = false;
                    self.panel_focus = false;
                    self.find_mode = true;
                    self.replace_mode = true;
                    self.find_query.clear();
                    self.replace_query.clear();
                    self.replace_field = 0;
                    self.update_find_replace_status();
                }
                Some(19) => self.run_c_file(hwnd),
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

    pub(super) fn open_settings(&mut self, hwnd: HWND) {
        if let Some(path) = lightline::settings::Settings::settings_path() {
            if !path.exists() {
                let _ = lightline::settings::Settings::default().save();
            }
            self.settings = lightline::settings::Settings::load();
            self.open(hwnd, Some(path));
        }
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

    // Single entry point for the Run button/shortcut: dispatches on the
    // active file's language instead of always assuming a Cargo workspace,
    // which previously made Run silently no-op (or run the wrong thing) for
    // Python and C/C++ files.
    pub(super) fn run_active_file(&mut self, hwnd: HWND) {
        if Tab::is_python(self.doc()) {
            self.run_python_file(hwnd);
        } else if Tab::is_c_family(self.doc()) {
            self.run_c_file(hwnd);
        } else if Tab::is_rust(self.doc()) || self.workspace_root.is_some() {
            self.run_project(hwnd);
        } else {
            self.status = "Open a Python, C/C++, or Rust file to run it".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    pub(super) fn run_project(&mut self, hwnd: HWND) {
        if self.workspace_root.is_none() {
            self.status = "Open a Rust workspace to run tests".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        if !workflow::command_available("cargo") {
            self.status =
                "Cargo was not found on PATH. Install Rust (rustup.rs) and restart LightLine."
                    .into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        self.run_in_terminal(hwnd, "cargo test --offline");
    }

    pub(super) fn run_c_file(&mut self, hwnd: HWND) {
        if !Tab::is_c_family(self.doc()) {
            self.status = "Open a C/C++ file to run it".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        if self.doc().is_dirty() && !self.save(hwnd, false) {
            self.status = "Save the file before running it".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let Some(file) = self.doc().path.clone() else {
            self.status = "Save the file before running it".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        let is_cpp = Tab::is_cpp(self.doc());
        let Some(compiler) = workflow::detect_c_compiler(is_cpp) else {
            self.status = if is_cpp {
                "No C++ compiler found on PATH (install g++/MinGW or MSVC Build Tools)".into()
            } else {
                "No C compiler found on PATH (install gcc/MinGW or MSVC Build Tools)".into()
            };
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        self.check_c_syntax(hwnd, file.clone(), compiler, is_cpp);
        let source = display_path(&file);
        let mut output = file.clone();
        output.set_extension("exe");
        let output = display_path(&output);
        // Compile then run in one shell command so a compile error is shown
        // in place of a crash from trying to run a binary that was never
        // produced; `&&` short-circuits the run half on a nonzero exit.
        let command = format!(
            "& {compiler} {} -o {} && & {}",
            terminal::powershell_quoted(Path::new(&source)),
            terminal::powershell_quoted(Path::new(&output)),
            terminal::powershell_quoted(Path::new(&output)),
        );
        self.run_in_terminal(hwnd, &command);
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
                        "Select a Python interpreter or virtual environment before running".into();
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    return;
                }
            },
        };
        // PowerShell only executes a quoted-string command when it's prefixed
        // with the call operator; without it "'...exe' -u '...'" parses as a
        // bare string statement followed by an unexpected "-u" token.
        // display_path() drops the \\?\ extended-length prefix canonicalize()
        // leaves on the path, which is noise in a command the user can see.
        let command = format!(
            "& {} -u {}",
            terminal::powershell_quoted(&interpreter),
            terminal::powershell_quoted(Path::new(&display_path(&file)))
        );
        self.run_in_terminal(hwnd, &command);
    }

    // Runs the same syntax check as run_c_file, but on every save instead of
    // only on Run, so a C/C++ error shows up as soon as VS Code-style LSP
    // diagnostics would for Rust/Python, not only once the user tries to run.
    pub(super) fn check_c_syntax_on_save(&mut self, hwnd: HWND, path: &Path) {
        if !is_c_family_path(path) {
            return;
        }
        let is_cpp = is_cpp_path(path);
        if let Some(compiler) = workflow::detect_c_compiler(is_cpp) {
            self.check_c_syntax(hwnd, path.to_path_buf(), compiler, is_cpp);
        }
    }

    // C/C++ has no LSP wired up (see Tab::lsp_language), so this is the only
    // source of the same squiggly-underline error/warning feedback Rust and
    // Python get: a quick `-fsyntax-only` compile, off the UI thread, whose
    // diagnostics land in tab.diagnostics exactly like an LSP response would.
    fn check_c_syntax(&mut self, hwnd: HWND, file: PathBuf, compiler: &'static str, is_cpp: bool) {
        let tx = self.worker_tx.clone();
        self.worker_started(hwnd);
        std::thread::spawn(move || {
            let diagnostics = workflow::c_syntax_diagnostics(&file, compiler, is_cpp);
            let _ = tx.send(WorkerMessage::CDiagnostics(file, diagnostics));
        });
    }

    pub(super) fn show_review(&mut self, hwnd: HWND) {
        if self.workspace_root.is_none() {
            self.status = "Open a Git workspace to review changes".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
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
        self.refresh_git(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn show_diff(&mut self, hwnd: HWND, path: PathBuf, staged: bool) {
        let Some(root) = self.git_root.clone().or_else(|| self.workspace_root.clone()) else {
            return;
        };
        self.review_staged = staged;
        self.review_file = Some(path.clone());
        self.diff_rows.clear();
        self.diff_first = 0;
        self.status = format!("Reviewing {}", path.display());
        let scope = App::git_diff_scope(staged);
        let tx = self.worker_tx.clone();
        self.worker_started(hwnd);
        std::thread::spawn(move || {
            let result = workflow::git_diff(&root, &path, scope);
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
                WorkerMessage::Repo(generation, result) => {
                    if generation == self.git_generation {
                        self.review_loading = false;
                        match result {
                            Ok(state) => self.apply_repo_state(hwnd, state),
                            Err(error) => self.status = error,
                        }
                    }
                }
                WorkerMessage::GitWrite(action, result) => {
                    self.git_write_finished(hwnd, &action, result);
                }
                WorkerMessage::GutterDiff(_root, path, result) => {
                    self.gutter_diff_finished(&path, result);
                }
                WorkerMessage::Diff(root, path, result)
                    if self.git_root.as_ref() == Some(&root)
                        && self.review_file.as_ref() == Some(&path) =>
                {
                    match result {
                        Ok(rows) => self.diff_rows = rows,
                        Err(error) => self.status = error,
                    }
                }
                // Prettier's own detect-only flow (see toggle_extension) --
                // unrelated to the Zed registry.
                WorkerMessage::ExtensionInstalled(id, found) => {
                    if let Some(ext) = self.extensions.iter_mut().find(|e| e.id == id) {
                        ext.installing = false;
                        ext.installed = found;
                        self.status = if found {
                            format!("{} found — ready to format code", ext.name)
                        } else {
                            format!(
                                "{} isn't available. Install it with \"npm install -g prettier\" \
                                 or add it to this project, then try again.",
                                ext.name
                            )
                        };
                    }
                }
                WorkerMessage::ZedRegistryList(result) => {
                    self.zed_registry_loading = false;
                    match result {
                        Ok(entries) => {
                            self.zed_registry_loaded = true;
                            for (id, version) in entries {
                                if self.extensions.iter().any(|ext| ext.id == id) {
                                    continue;
                                }
                                self.extensions.push(Extension {
                                    id: id.clone(),
                                    name: id,
                                    publisher: "zed-industries/extensions".into(),
                                    version,
                                    description: "Zed extension — install to see whether LightLine \
                                                   supports it yet (icon themes only, for now)"
                                        .into(),
                                    downloads: String::new(),
                                    rating: String::new(),
                                    installed: false,
                                    installing: false,
                                });
                            }
                        }
                        Err(error) => {
                            self.status = format!("Could not reach the Zed extension registry: {error}");
                        }
                    }
                }
                WorkerMessage::ZedExtensionInstalled(id, result) => {
                    let registry_id = if id == "material-icons" {
                        "material-icon-theme".to_string()
                    } else {
                        id.clone()
                    };
                    if let Some(ext) = self.extensions.iter_mut().find(|ext| ext.id == id) {
                        ext.installing = false;
                        self.status = match &result {
                            Ok(_) => format!("{} installed", ext.name),
                            Err(error) => format!("Could not install {}: {error}", ext.name),
                        };
                        ext.installed = result.is_ok();
                    }
                    // IconSet only ever reads the "material-icon-theme"
                    // folder -- reloading for any other icon-theme id would
                    // be a no-op, since there's no active-theme selection.
                    if result.is_ok() && registry_id == "material-icon-theme" {
                        self.icons = IconSet::new(self.dpi, self.zoom);
                    }
                }
                WorkerMessage::DebugBuild(result) => {
                    self.debug_build_finished(hwnd, result);
                }
                WorkerMessage::CDiagnostics(file, diagnostics) => {
                    if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.document.path.as_deref() == Some(file.as_path())) {
                        let count = diagnostics.len();
                        tab.diagnostics = diagnostics;
                        if count > 0 {
                            self.status = format!(
                                "{count} issue{} found while checking {}",
                                if count == 1 { "" } else { "s" },
                                file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                            );
                        }
                    }
                }
                WorkerMessage::Formatted(path, formatter_name, serial, result) => {
                    // Only apply to the tab the format request was actually
                    // made against, and only if its content hasn't changed
                    // since -- the user may have kept typing (or switched
                    // tabs entirely) while the formatter ran in the
                    // background. A stale result is silently dropped rather
                    // than clobbering newer text.
                    let still_current = self.doc().path.as_deref() == Some(path.as_path())
                        && self.doc().change_serial() == serial;
                    if !still_current {
                        continue;
                    }
                    match result {
                        Ok(formatted) => {
                            let formatted_clean = formatted.replace("\r\n", "\n").replace('\r', "\n");
                            let current_clean = self.doc().text().replace("\r\n", "\n").replace('\r', "\n");
                            if formatted_clean == current_clean {
                                self.status = format!("Already formatted with {formatter_name}");
                            } else {
                                let doc = self.doc();
                                let last_line = doc.line_count().saturating_sub(1);
                                let end = Pos {
                                    line: last_line,
                                    byte: doc.line(last_line).len(),
                                };
                                self.replace_range(Pos::default(), end, &formatted_clean);
                                self.status = format!("Document formatted with {formatter_name}");
                            }
                        }
                        Err(error) => {
                            self.status = format!("{formatter_name} formatting failed: {error}");
                        }
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
