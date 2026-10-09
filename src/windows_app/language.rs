use super::app::{CompletionPopup, CompletionRequest, HoverCard, HoverTarget, NavTarget, Tab};
use super::*;
use lightline::lsp::{Command as LspCommand, Position as LspPosition, Range as LspRange};

const LSP_MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
pub(super) const LSP_EVENT_MESSAGE: u32 = WM_APP + 7;

impl App {
    // Clicking the status bar's language name (see status_language_control)
    // opens the command palette pre-filtered to that language's run/setup
    // actions, standing in for a dedicated language-picker menu: users
    // reported the label as an unresponsive "dropdown", and this is what
    // there actually is to pick from for the language of the active file.
    pub(super) fn open_language_actions(&mut self, hwnd: HWND) {
        let filter = if Tab::is_python(self.doc()) {
            "python"
        } else if Tab::is_c_family(self.doc()) {
            "c/c++"
        } else if Tab::is_rust(self.doc()) {
            "rust"
        } else {
            ""
        };
        if filter.is_empty() {
            self.status = format!(
                "No run action is available for {} files",
                super::render::language_label(self.doc().path.as_deref())
            );
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        self.show_quick_open(hwnd);
        self.quick_query = format!(">{filter}");
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Errors and warnings reported for every open file, for the status bar.
    pub(super) fn problem_counts(&self) -> (usize, usize) {
        problem_counts(self.tabs.iter().flat_map(|tab| &tab.diagnostics))
    }

    pub(super) fn ensure_lsp(&mut self, hwnd: HWND) {
        let Some(language) = Tab::lsp_language(self.doc()) else {
            return;
        };
        let Some(path) = self.doc().path.as_deref() else {
            return;
        };
        if self.doc().byte_len() > LSP_MAX_FILE_BYTES {
            self.status = format!(
                "{} language support skipped for files over 2 MiB",
                language.name()
            );
            return;
        }
        let path = path.to_path_buf();
        // Opened files carry canonicalize()'s `\\?\` form and restored ones
        // don't; one spelling keeps them on the same server.
        let root = PathBuf::from(display_path(&self.lsp_root(language, &path)));
        // One server per project, kept while other projects' files are
        // edited: switching tabs between two crates or packages used to
        // restart the server, and rust-analyzer then re-indexed everything.
        let running = |client: &LspClient| client.language() == language && client.root() == root;
        if !self.lsp.iter().any(running) {
            let key = (language, root.clone());
            if self
                .lsp_failed_at
                .get(&key)
                .is_some_and(|when| when.elapsed() < Duration::from_secs(3))
            {
                return;
            }
            if language == LspLanguage::Python && self.python_interpreter.is_none() {
                self.python_interpreter = workflow::detect_python_interpreter(Some(&root));
            }
            let hwnd_value = hwnd as isize;
            let wake = Arc::new(move || unsafe {
                PostMessageW(hwnd_value as HWND, LSP_EVENT_MESSAGE, 0, 0);
            });
            let client = LspClient::start(
                language,
                root.clone(),
                (language == LspLanguage::Python)
                    .then(|| self.python_interpreter.clone())
                    .flatten(),
                self.lsp_event_tx.clone(),
                wake,
            );
            self.lsp.push(client);
            self.lsp_failed_at.remove(&key);
        }
        let tab = self.tab();
        if !tab.lsp_opened
            || tab.lsp_language != Some(language)
            || tab.lsp_root.as_deref() != Some(root.as_path())
        {
            self.close_lsp_tab(self.active);
            let uri = lsp::file_uri(&path);
            let text = self.doc().text();
            let version = 1;
            if self
                .lsp
                .iter()
                .find(|client| client.language() == language && client.root() == root)
                .is_some_and(|client| client.send(LspCommand::Open { uri, text, version }))
            {
                let tab = self.tab_mut();
                tab.lsp_opened = true;
                tab.lsp_language = Some(language);
                tab.lsp_root = Some(root);
                tab.lsp_version = version;
                tab.lsp_serial = tab.document.change_serial();
            }
        }
    }

    // The language server the file in tab `index` was opened with.
    pub(super) fn tab_lsp(&self, index: usize) -> Option<&LspClient> {
        let tab = &self.tabs[index];
        let (language, root) = (tab.lsp_language?, tab.lsp_root.as_deref()?);
        self.lsp
            .iter()
            .find(|client| client.language() == language && client.root() == root)
    }

    fn lsp_root(&self, language: LspLanguage, path: &Path) -> PathBuf {
        match language {
            // The Cargo workspace a crate belongs to, so all its crates share
            // one rust-analyzer, which indexes the whole workspace anyway.
            LspLanguage::Rust => cargo_root(path)
                .or_else(|| path.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| PathBuf::from(".")),
            LspLanguage::Python => path
                .ancestors()
                .skip(1)
                .take(10)
                .find(|folder| {
                    folder.join("pyproject.toml").is_file()
                        || folder.join("setup.py").is_file()
                        || folder.join("setup.cfg").is_file()
                        || folder.join("requirements.txt").is_file()
                        || folder.join(".venv").is_dir()
                        || folder.join(".git").exists()
                })
                .or(self.workspace_root.as_deref())
                .or_else(|| path.parent())
                .unwrap_or(Path::new("."))
                .to_path_buf(),
            LspLanguage::C => path
                .ancestors()
                .skip(1)
                .take(10)
                .find(|folder| {
                    folder.join("compile_commands.json").is_file()
                        || folder.join("CMakeLists.txt").is_file()
                        || folder.join("Makefile").is_file()
                        || folder.join(".git").exists()
                })
                .or(self.workspace_root.as_deref())
                .or_else(|| path.parent())
                .unwrap_or(Path::new("."))
                .to_path_buf(),
            LspLanguage::TypeScript => path
                .ancestors()
                .skip(1)
                .take(10)
                .find(|folder| {
                    folder.join("tsconfig.json").is_file()
                        || folder.join("jsconfig.json").is_file()
                        || folder.join("package.json").is_file()
                        || folder.join(".git").exists()
                })
                .or(self.workspace_root.as_deref())
                .or_else(|| path.parent())
                .unwrap_or(Path::new("."))
                .to_path_buf(),
            LspLanguage::Go => path
                .ancestors()
                .skip(1)
                .take(10)
                .find(|folder| {
                    folder.join("go.mod").is_file()
                        || folder.join("go.work").is_file()
                        || folder.join(".git").exists()
                })
                .or(self.workspace_root.as_deref())
                .or_else(|| path.parent())
                .unwrap_or(Path::new("."))
                .to_path_buf(),
        }
    }

    // Drops the `language` server for `root`, or every `language` server
    // when `root` is None; its tabs reopen with a new one when next shown.
    fn reset_language_client(&mut self, language: LspLanguage, root: Option<&Path>) {
        let matches = |other: Option<LspLanguage>, other_root: Option<&Path>| {
            other == Some(language) && root.is_none_or(|root| other_root == Some(root))
        };
        self.lsp
            .retain(|client| !matches(Some(client.language()), Some(client.root())));
        for tab in &mut self.tabs {
            if matches(tab.lsp_language, tab.lsp_root.as_deref()) {
                tab.lsp_opened = false;
                tab.lsp_language = None;
                tab.lsp_root = None;
                tab.diagnostics.clear();
            }
        }
    }

    pub(super) fn sync_lsp_edit(&mut self) {
        let tab = self.tab();
        if !tab.lsp_opened || tab.lsp_serial == tab.document.change_serial() {
            return;
        }
        let Some(path) = tab.document.path.as_deref() else {
            return;
        };
        let Some(change) = tab.document.last_change().cloned() else {
            return;
        };
        // Changes are incremental, so each must follow the last one sent. If
        // one was skipped, the server's text differs from ours: reopen the
        // file with its full text rather than send an edit that won't apply.
        if change.serial != tab.lsp_serial.wrapping_add(1) {
            self.close_lsp_tab(self.active);
            self.tab_mut().diagnostics.clear();
            self.ensure_lsp(self.hwnd);
            return;
        }
        let uri = lsp::file_uri(path);
        let version = tab.lsp_version.saturating_add(1);
        let range = LspRange {
            start: LspPosition {
                line: change.start.line as u32,
                character: change.start_utf16 as u32,
            },
            end: LspPosition {
                line: change.end.line as u32,
                character: change.end_utf16 as u32,
            },
        };
        let sent = self.tab_lsp(self.active).is_some_and(|client| {
            client.send(LspCommand::Change {
                uri,
                version,
                range,
                text: change.text,
            })
        });
        let tab = self.tab_mut();
        tab.diagnostics.clear();
        if sent {
            tab.lsp_version = version;
            tab.lsp_serial = change.serial;
        } else {
            tab.lsp_opened = false;
            tab.lsp_language = None;
        }
    }

    pub(super) fn close_lsp_tab(&mut self, index: usize) {
        if !self.tabs[index].lsp_opened {
            return;
        }
        if let (Some(client), Some(path)) = (
            self.tab_lsp(index),
            self.tabs[index].document.path.as_deref(),
        ) {
            client.send(LspCommand::Close {
                uri: lsp::file_uri(path),
            });
        }
        let tab = &mut self.tabs[index];
        tab.lsp_opened = false;
        tab.lsp_language = None;
        tab.lsp_root = None;
    }

    pub(super) fn lsp_after_save(&mut self, hwnd: HWND, old_path: Option<&Path>) {
        let changed_path = old_path != self.doc().path.as_deref();
        if changed_path {
            if self.tab().lsp_opened
                && let (Some(client), Some(path)) = (self.tab_lsp(self.active), old_path)
            {
                client.send(LspCommand::Close {
                    uri: lsp::file_uri(path),
                });
            }
            let tab = self.tab_mut();
            tab.lsp_opened = false;
            tab.lsp_language = None;
            tab.lsp_root = None;
            tab.diagnostics.clear();
        }
        self.ensure_lsp(hwnd);
        if self.tab().lsp_opened
            && let (Some(client), Some(path)) =
                (self.tab_lsp(self.active), self.doc().path.as_deref())
        {
            client.send(LspCommand::Save {
                uri: lsp::file_uri(path),
            });
        }
    }

    pub(super) fn poll_lsp(&mut self, hwnd: HWND) {
        let events: Vec<LspEvent> = self.lsp_events.try_iter().collect();
        if events.is_empty() {
            return;
        }
        for event in events {
            match event {
                LspEvent::Ready { language } => {
                    if Tab::lsp_language(self.doc()) == Some(language) {
                        self.status = format!("{} language support ready", language.name());
                    }
                }
                LspEvent::Diagnostics {
                    language,
                    uri,
                    version,
                    items,
                } => {
                    if let Some(tab) = self.tabs.iter_mut().find(|tab| {
                        tab.lsp_opened
                            && tab.lsp_language == Some(language)
                            && tab
                                .document
                                .path
                                .as_deref()
                                .is_some_and(|p| lsp::same_file_uri(&lsp::file_uri(p), &uri))
                    }) && version.is_none_or(|number| number == tab.lsp_version)
                    {
                        tab.diagnostics = items;
                    }
                }
                LspEvent::Hover {
                    language,
                    id,
                    uri,
                    version,
                    text,
                } => {
                    if let Some(target) = self.hover_target.take().filter(|target| {
                        target.language == language
                            && target.id == id
                            && target.uri == uri
                            && target.version == version
                    }) && self.tab_for_pane(target.pane) < self.tabs.len()
                        && self.tabs[self.tab_for_pane(target.pane)].lsp_language == Some(language)
                        && self.tabs[self.tab_for_pane(target.pane)].lsp_version == version
                    {
                        self.hover_card = text.map(|text| HoverCard {
                            text,
                            x: target.x,
                            y: target.y,
                        });
                    }
                }
                LspEvent::Stopped {
                    language,
                    root,
                    message,
                } => {
                    self.reset_language_client(language, Some(&root));
                    self.lsp_failed_at.insert((language, root), Instant::now());
                    self.hover_target = None;
                    self.hover_card = None;
                    self.definition_target = None;
                    self.references_target = None;
                    self.format_target = None;
                    self.completion_request = None;
                    self.completion = None;
                    self.status = message;
                }
                LspEvent::Definition {
                    language,
                    id,
                    uri,
                    version,
                    targets,
                } => {
                    let Some(target) = self.definition_target.take().filter(|target| {
                        target.language == language
                            && target.id == id
                            && target.uri == uri
                            && target.version == version
                    }) else {
                        continue;
                    };
                    let index = self.tab_for_pane(target.pane);
                    if self.tabs[index].lsp_version != version {
                        continue;
                    }
                    let Some(location) = targets.first() else {
                        self.status = "No definition found".into();
                        continue;
                    };
                    let Some(path) = lsp::uri_to_path(&location.uri) else {
                        continue;
                    };
                    self.open(hwnd, Some(path));
                    let line = location.range.start.line as usize;
                    let byte = {
                        let doc = self.doc();
                        let line = line.min(doc.line_count().saturating_sub(1));
                        (
                            line,
                            lsp::utf16_to_byte(doc.line(line), location.range.start.character),
                        )
                    };
                    self.move_cursor(
                        Pos {
                            line: byte.0,
                            byte: byte.1,
                        },
                        false,
                    );
                    self.keep_cursor_visible(hwnd);
                    self.status = format!("Jumped to definition (line {})", byte.0 + 1);
                }
                LspEvent::References {
                    language,
                    id,
                    uri,
                    version,
                    locations,
                } => {
                    let Some(target) = self.references_target.take().filter(|target| {
                        target.language == language
                            && target.id == id
                            && target.uri == uri
                            && target.version == version
                    }) else {
                        continue;
                    };
                    let index = self.tab_for_pane(target.pane);
                    if self.tabs[index].lsp_version != version {
                        continue;
                    }
                    if locations.is_empty() {
                        self.status = "No references found".into();
                        continue;
                    }
                    // References can span files that aren't open in any tab,
                    // so their preview text is read straight from disk rather
                    // than reusing document state the way the search panel's
                    // own results do.
                    let hits: Vec<SearchHit> = locations
                        .iter()
                        .filter_map(|location| {
                            let path = lsp::uri_to_path(&location.uri)?;
                            let text = std::fs::read_to_string(&path).ok()?;
                            let line_number = location.range.start.line as usize;
                            let line_text = text.lines().nth(line_number).unwrap_or("");
                            let byte =
                                lsp::utf16_to_byte(line_text, location.range.start.character);
                            Some(SearchHit {
                                path,
                                line: line_number,
                                byte,
                                preview: line_text.trim().chars().take(110).collect(),
                                context: Vec::new(),
                            })
                        })
                        .collect();
                    self.status = format!(
                        "{} reference{} found",
                        hits.len(),
                        if hits.len() == 1 { "" } else { "s" }
                    );
                    self.search_results = hits;
                    self.panel_selected = 0;
                    self.panel_first = 0;
                    self.welcome = false;
                    self.side_view = SideView::Search;
                    self.set_sidebar_visible(hwnd, true);
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
                LspEvent::Format {
                    language,
                    id,
                    uri,
                    version,
                    edits,
                } => {
                    let Some(target) = self.format_target.take().filter(|target| {
                        target.language == language
                            && target.id == id
                            && target.uri == uri
                            && target.version == version
                    }) else {
                        continue;
                    };
                    let index = self.tab_for_pane(target.pane);
                    if self.tabs[index].lsp_version != version || edits.is_empty() {
                        self.status = if edits.is_empty() {
                            "Already formatted".into()
                        } else {
                            String::new()
                        };
                        continue;
                    }
                    if index != self.active {
                        self.activate_tab(hwnd, index);
                    }
                    let cursor = self.view().cursor;
                    // Resolve LSP ranges into document positions against the
                    // unmodified text, then apply from the end backwards so the
                    // earlier edits keep valid line/byte offsets. Each
                    // replace_range re-syncs the server incrementally.
                    let mut resolved: Vec<(Pos, Pos, String)> = edits
                        .iter()
                        .map(|edit| {
                            let doc = self.doc();
                            let start_line = (edit.range.start.line as usize)
                                .min(doc.line_count().saturating_sub(1));
                            let end_line = (edit.range.end.line as usize)
                                .min(doc.line_count().saturating_sub(1));
                            (
                                Pos {
                                    line: start_line,
                                    byte: lsp::utf16_to_byte(
                                        doc.line(start_line),
                                        edit.range.start.character,
                                    ),
                                },
                                Pos {
                                    line: end_line,
                                    byte: lsp::utf16_to_byte(
                                        doc.line(end_line),
                                        edit.range.end.character,
                                    ),
                                },
                                edit.text.clone(),
                            )
                        })
                        .collect();
                    resolved.sort_by_key(|edit| std::cmp::Reverse((edit.0.line, edit.0.byte)));
                    for (start, end, text) in resolved {
                        let text = text.replace("\r\n", "\n").replace('\r', "\n");
                        self.replace_range(start, end, &text);
                    }
                    let restored = {
                        let doc = self.doc();
                        Pos {
                            line: cursor.line.min(doc.line_count().saturating_sub(1)),
                            byte: 0,
                        }
                    };
                    let restored = self.doc().clamp(Pos {
                        line: restored.line,
                        byte: cursor.byte,
                    });
                    self.move_cursor(restored, false);
                    self.keep_cursor_visible(hwnd);
                    self.status = "Formatted document".into();
                }
                LspEvent::Status { message, .. } => {
                    self.status = message;
                }
                LspEvent::Completion {
                    language,
                    id,
                    uri,
                    version,
                    items,
                } => {
                    let Some(request) = self.completion_request.take().filter(|request| {
                        request.language == language
                            && request.id == id
                            && request.uri == uri
                            && request.version == version
                    }) else {
                        continue;
                    };
                    let index = self.tab_for_pane(request.pane);
                    if self.tabs[index].lsp_language != Some(language)
                        || self.tabs[index].lsp_version != version
                    {
                        continue;
                    }
                    // Narrow the list to the identifier prefix typed before the
                    // caret, but fall back to the full set when the prefix would
                    // filter everything away (servers that ignore trigger text).
                    let prefix = {
                        let doc = self.doc();
                        let line = doc.line(request.replace_start.line);
                        let end = request.replace_end.byte.min(line.len());
                        line.get(request.replace_start.byte..end)
                            .unwrap_or("")
                            .to_owned()
                    };
                    let mut items = items;
                    if !prefix.is_empty() {
                        let needle = prefix.to_lowercase();
                        let kept: Vec<_> = items
                            .iter()
                            .filter(|item| item.label.to_lowercase().starts_with(&needle))
                            .cloned()
                            .collect();
                        if !kept.is_empty() {
                            items = kept;
                        }
                    }
                    if items.is_empty() {
                        self.status = "No completions".into();
                        continue;
                    }
                    self.completion = Some(CompletionPopup {
                        items,
                        selected: 0,
                        replace_start: request.replace_start,
                        replace_end: request.replace_end,
                        x: request.x,
                        y: request.y,
                    });
                }
            }
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn select_python_interpreter(&mut self, hwnd: HWND) {
        let mut buffer = [0u16; 32768];
        let filter = wide("Python executable\0python.exe;pythonw.exe\0All files\0*.*\0");
        let mut dialog: OPENFILENAMEW = unsafe { zeroed() };
        dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        dialog.hwndOwner = hwnd;
        dialog.lpstrFilter = filter.as_ptr();
        dialog.lpstrFile = buffer.as_mut_ptr();
        dialog.nMaxFile = buffer.len() as u32;
        dialog.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST;
        let ok = unsafe { GetOpenFileNameW(&mut dialog) };
        if ok != 0
            && let Some(end) = buffer.iter().position(|ch| *ch == 0)
        {
            self.set_python_interpreter(
                hwnd,
                PathBuf::from(String::from_utf16_lossy(&buffer[..end])),
            );
        }
    }

    pub(super) fn select_python_environment(&mut self, hwnd: HWND) {
        if let Some(root) = self.folder_dialog(hwnd) {
            let candidates = [
                root.join("Scripts").join("python.exe"),
                root.join("Scripts").join("pythonw.exe"),
                root.join("bin").join("python"),
            ];
            if let Some(interpreter) = candidates.into_iter().find(|path| path.is_file()) {
                self.set_python_interpreter(hwnd, interpreter);
            } else {
                self.status = "Selected folder is not a Python virtual environment".into();
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }
    }

    fn set_python_interpreter(&mut self, hwnd: HWND, interpreter: PathBuf) {
        self.python_interpreter = Some(interpreter.clone());
        self.reset_language_client(LspLanguage::Python, None);
        self.lsp_failed_at
            .retain(|(language, _), _| *language != LspLanguage::Python);
        self.status = format!("Python interpreter: {}", interpreter.to_string_lossy());
        self.ensure_lsp(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn clear_hover(&mut self, hwnd: HWND) {
        self.hover_mouse = None;
        self.hover_target = None;
        self.hover_card = None;
        unsafe { KillTimer(hwnd, 6) };
    }

    pub(super) fn mouse_hover_move(&mut self, hwnd: HWND, x: i32, y: i32) {
        if self.run_config_panel.is_some() {
            self.run_config_hover(hwnd, x, y);
            return;
        }
        if self.editor_context_hover(hwnd, x, y) || self.more_menu_hover(hwnd, x, y) {
            return;
        }
        let over_scrollbar = self.scrollbar_at(hwnd, x, y);
        self.set_scrollbar_hover(hwnd, over_scrollbar);
        if over_scrollbar.is_some() {
            // No hover card for the code under the scrollbar.
            if self.hover_mouse.is_some() || self.hover_card.is_some() {
                self.clear_hover(hwnd);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            return;
        }
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) };
        let bottom = rect.bottom
            - self.scale(
                STATUS
                    + if self.terminal_visible {
                        self.terminal_height
                    } else {
                        0
                    },
            );
        // The tab strip's action button says what it does; this used to test
        // a stale spot in the title bar, so the Run hint never appeared.
        let button = self.file_action_rect(hwnd);
        if let Some(action) = self.shown_file_action(hwnd)
            && x >= button.left
            && x < button.right
            && y >= button.top
            && y < button.bottom
        {
            let hint = if action == FileAction::PreviewMarkdown && self.preview_beside().is_some() {
                "Close Preview"
            } else {
                action.hint()
            };
            if self.status != hint {
                self.status = hint.into();
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            return;
        }
        if self.welcome
            || self.quick_open
            || self.side_view == SideView::Review
            || x < self.editor_left()
            || x >= self.editor_right(hwnd)
            || y < self.editor_top()
            || y >= bottom
        {
            if self.hover_mouse.is_some() || self.hover_card.is_some() {
                self.clear_hover(hwnd);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            return;
        }
        if self.hover_mouse == Some((x, y)) {
            return;
        }
        self.hover_mouse = Some((x, y));
        self.hover_target = None;
        if self.hover_card.take().is_some() {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
        unsafe {
            KillTimer(hwnd, 6);
            SetTimer(hwnd, 6, 500, None);
        }
    }

    pub(super) fn begin_mouse_hover(&mut self, hwnd: HWND) {
        unsafe { KillTimer(hwnd, 6) };
        let Some((x, y)) = self.hover_mouse.take() else {
            return;
        };
        let pane = if self.split_visible && x >= self.pane_divider(hwnd) {
            1
        } else {
            0
        };
        let pos = self.position_at_pane(hwnd, x, y, pane);
        self.request_hover(hwnd, pane, pos, x + self.scale(14), y + self.scale(18));
    }

    pub(super) fn hover_at_cursor(&mut self, hwnd: HWND) {
        let rect = self.caret_rect(hwnd);
        self.request_hover(
            hwnd,
            self.focused_pane,
            self.view().cursor,
            rect.left,
            rect.bottom + self.scale(5),
        );
    }

    fn request_hover(&mut self, hwnd: HWND, pane: usize, pos: Pos, x: i32, y: i32) {
        let index = self.tab_for_pane(pane);
        if !self.tabs[index].lsp_opened {
            if pane != self.focused_pane {
                return;
            }
            self.ensure_lsp(hwnd);
        }
        let tab = &self.tabs[index];
        if !tab.lsp_opened {
            return;
        }
        let Some(language) = tab.lsp_language else {
            return;
        };
        let Some(path) = tab.document.path.as_deref() else {
            return;
        };
        let uri = lsp::file_uri(path);
        let version = tab.lsp_version;
        let position = LspPosition {
            line: pos.line as u32,
            character: tab.document.utf16_column(pos) as u32,
        };
        self.hover_request_id += 1;
        let id = self.hover_request_id;
        if self.tab_lsp(index).is_some_and(|client| {
            client.send(LspCommand::Hover {
                id,
                uri: uri.clone(),
                version,
                position,
            })
        }) {
            self.hover_target = Some(HoverTarget {
                language,
                id,
                uri,
                version,
                pane,
                x,
                y,
            });
            self.hover_card = None;
        }
    }

    pub(super) fn goto_definition(&mut self, hwnd: HWND) {
        let pane = self.focused_pane;
        let pos = self.view().cursor;
        let index = self.tab_for_pane(pane);
        if !self.tabs[index].lsp_opened {
            self.ensure_lsp(hwnd);
        }
        let tab = &self.tabs[index];
        if !tab.lsp_opened {
            self.status = "Language server not ready yet".into();
            return;
        }
        let Some(language) = tab.lsp_language else {
            return;
        };
        let Some(path) = tab.document.path.as_deref() else {
            return;
        };
        let uri = lsp::file_uri(path);
        let version = tab.lsp_version;
        let position = LspPosition {
            line: pos.line as u32,
            character: tab.document.utf16_column(pos) as u32,
        };
        self.request_id += 1;
        let id = self.request_id;
        if self.tab_lsp(index).is_some_and(|client| {
            client.send(LspCommand::Definition {
                id,
                uri: uri.clone(),
                version,
                position,
            })
        }) {
            self.definition_target = Some(NavTarget {
                language,
                id,
                uri,
                version,
                pane,
            });
            self.status = "Finding definition...".into();
        }
    }

    pub(super) fn find_references(&mut self, hwnd: HWND) {
        let pane = self.focused_pane;
        let pos = self.view().cursor;
        let index = self.tab_for_pane(pane);
        if !self.tabs[index].lsp_opened {
            self.ensure_lsp(hwnd);
        }
        let tab = &self.tabs[index];
        if !tab.lsp_opened {
            self.status = "Language server not ready yet".into();
            return;
        }
        let Some(language) = tab.lsp_language else {
            return;
        };
        let Some(path) = tab.document.path.as_deref() else {
            return;
        };
        let uri = lsp::file_uri(path);
        let version = tab.lsp_version;
        let position = LspPosition {
            line: pos.line as u32,
            character: tab.document.utf16_column(pos) as u32,
        };
        self.request_id += 1;
        let id = self.request_id;
        if self.tab_lsp(index).is_some_and(|client| {
            client.send(LspCommand::References {
                id,
                uri: uri.clone(),
                version,
                position,
            })
        }) {
            self.references_target = Some(NavTarget {
                language,
                id,
                uri,
                version,
                pane,
            });
            self.status = "Finding references...".into();
        }
    }

    // formatter::formatter_for(), minus Prettier once it's turned off in the
    // Extensions panel.
    pub(super) fn formatter_for(
        &self,
        path: &Path,
    ) -> Option<Box<dyn lightline::formatter::Formatter>> {
        use lightline::formatter::{Formatter, PrettierFormatter};
        lightline::formatter::formatter_for(path).filter(|formatter| {
            self.settings.prettier_enabled || formatter.name() != PrettierFormatter.name()
        })
    }

    // Runs whichever formatter::formatter_for() picks for the active file
    // (Prettier today; the one place a second formatter -- rustfmt, black,
    // clang-format -- plugs in later) on a background thread, so a slow or
    // stuck process (bounded by formatter::DEFAULT_TIMEOUT either way) never
    // blocks the UI thread the way the old synchronous implementation did.
    // `then_save` is format-on-save: the file was just saved, and is saved
    // again with the result unless it was edited in the meantime.
    pub(super) fn format_with_external_formatter(&mut self, hwnd: HWND, then_save: bool) {
        if self.tab().read_only() {
            return;
        }
        let code = self.doc().text();
        if code.trim().is_empty() {
            if !then_save {
                self.status = "Nothing to format".into();
                self.refresh(hwnd);
            }
            return;
        }
        let Some(path) = self.doc().path.clone() else {
            self.status = "Save the file before formatting it".into();
            self.refresh(hwnd);
            return;
        };
        let Some(formatter) = self.formatter_for(&path) else {
            return;
        };
        let serial = self.doc().change_serial();
        self.status = if then_save {
            format!("{}; formatting with {}...", self.status, formatter.name())
        } else {
            format!("Formatting with {}...", formatter.name())
        };
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            let result = formatter
                .format(&code, &path)
                .map_err(|error| error.to_string());
            tx.send(WorkerMessage::Formatted {
                path,
                formatter: formatter.name(),
                serial,
                result,
                then_save,
            });
        });
        self.refresh(hwnd);
    }

    // Format-on-save's part before the file is written. Built-in formatters
    // take microseconds and run here; true means an external tool applies,
    // which runs after the write instead (format_with_external_formatter),
    // so a slow or stuck one never holds up the save.
    pub(super) fn format_before_save(&mut self, path: &Path) -> bool {
        if !self.settings.format_on_save {
            return false;
        }
        let Some(formatter) = self.formatter_for(path) else {
            return false;
        };
        if !formatter.is_builtin() {
            return true;
        }
        let code = self.doc().text();
        if !code.trim().is_empty()
            && let Ok(formatted) = formatter.format(&code, path)
        {
            self.apply_formatted(&formatted);
        }
        false
    }

    // Puts a formatter's output into the active document as the one edit
    // spanning everything that changed, so text, folds and breakpoints
    // outside it are untouched. The caret, selection and scroll stay with
    // the code they were on (see `formatted_offset`). Returns false when the
    // output matches the document.
    pub(super) fn apply_formatted(&mut self, formatted: &str) -> bool {
        let formatted = formatted.replace("\r\n", "\n").replace('\r', "\n");
        let current = self.doc().text();
        let Some((from, to, replacement)) = changed_span(&current, &formatted) else {
            return false;
        };
        let start = self.doc().pos_at(from);
        let end = self.doc().pos_at(to);
        let view = self.view().clone();
        let cursor = self.doc().offset_of(view.cursor);
        let anchor = view.selection_anchor.map(|pos| self.doc().offset_of(pos));
        let serial = self.doc().change_serial();
        self.replace_range(start, end, replacement);
        if self.doc().change_serial() == serial {
            return false;
        }
        let doc = self.doc();
        let remap = |offset: usize| {
            doc.grapheme_position(doc.pos_at(formatted_offset(
                &current,
                (from, to, replacement),
                offset,
            )))
        };
        let cursor = remap(cursor);
        let anchor = anchor.map(remap);
        let first_line = view.first_line.min(doc.line_count().saturating_sub(1));
        let restored = self.view_mut();
        restored.cursor = cursor;
        restored.selection_anchor = anchor;
        restored.first_line = first_line;
        true
    }

    pub(super) fn format_document(&mut self, hwnd: HWND) {
        let pane = self.focused_pane;
        let index = self.tab_for_pane(pane);
        let path = self.tabs[index].document.path.clone();

        // formatter::formatter_for() is the single source of truth for "is
        // there a formatter for this file" -- asking it here instead of a
        // second, hand-maintained extension list means a future formatter
        // (rustfmt, black, clang-format) is picked up automatically, with
        // nothing to keep in sync in this file.
        if let Some(path) = path.as_deref()
            && self.formatter_for(path).is_some()
        {
            self.format_with_external_formatter(hwnd, false);
            return;
        }

        if !self.tabs[index].lsp_opened {
            self.ensure_lsp(hwnd);
        }
        let tab = &self.tabs[index];
        if !tab.lsp_opened {
            self.status = "Language server not ready yet".into();
            return;
        }
        let Some(language) = tab.lsp_language else {
            return;
        };
        let Some(path) = tab.document.path.as_deref() else {
            return;
        };
        let uri = lsp::file_uri(path);
        let version = tab.lsp_version;
        self.request_id += 1;
        let id = self.request_id;
        if self.tab_lsp(index).is_some_and(|client| {
            client.send(LspCommand::Format {
                id,
                uri: uri.clone(),
                version,
            })
        }) {
            self.format_target = Some(NavTarget {
                language,
                id,
                uri,
                version,
                pane,
            });
            self.status = "Formatting...".into();
        }
    }

    // Ctrl+Space: ask the language server for completions at the caret and
    // remember the identifier prefix that an accepted item should overwrite.
    pub(super) fn trigger_completion(&mut self, hwnd: HWND) {
        self.completion = None;
        let pane = self.focused_pane;
        let pos = self.view().cursor;
        let index = self.tab_for_pane(pane);
        if !self.tabs[index].lsp_opened {
            self.ensure_lsp(hwnd);
        }
        let tab = &self.tabs[index];
        if !tab.lsp_opened {
            self.status = "Language server not ready yet".into();
            return;
        }
        let Some(language) = tab.lsp_language else {
            return;
        };
        let Some(path) = tab.document.path.as_deref() else {
            return;
        };
        let replace_start = {
            let doc = self.doc();
            let line = doc.line(pos.line);
            let mut start = pos.byte.min(line.len());
            while start > 0 {
                let Some(prev) = line[..start].chars().next_back() else {
                    break;
                };
                if prev.is_alphanumeric() || prev == '_' {
                    start -= prev.len_utf8();
                } else {
                    break;
                }
            }
            Pos {
                line: pos.line,
                byte: start,
            }
        };
        let uri = lsp::file_uri(path);
        let version = tab.lsp_version;
        let character = tab.document.utf16_column(pos) as u32;
        let position = LspPosition {
            line: pos.line as u32,
            character,
        };
        self.request_id += 1;
        let id = self.request_id;
        if self.tab_lsp(index).is_some_and(|client| {
            client.send(LspCommand::Completion {
                id,
                uri: uri.clone(),
                version,
                position,
            })
        }) {
            let rect = self.caret_rect(hwnd);
            self.completion_request = Some(CompletionRequest {
                language,
                id,
                uri,
                version,
                pane,
                replace_start,
                replace_end: pos,
                x: rect.left,
                y: rect.bottom,
            });
        }
    }

    pub(super) fn completion_active(&self) -> bool {
        self.completion.is_some()
    }

    pub(super) fn completion_move(&mut self, hwnd: HWND, delta: i32) {
        let Some(popup) = self.completion.as_mut() else {
            return;
        };
        let count = popup.items.len();
        if count == 0 {
            return;
        }
        popup.selected = if delta < 0 {
            popup.selected.saturating_sub((-delta) as usize)
        } else {
            (popup.selected + delta as usize).min(count - 1)
        };
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn accept_completion(&mut self, hwnd: HWND) {
        let Some(item) = self
            .completion
            .as_ref()
            .and_then(|popup| popup.items.get(popup.selected).cloned())
        else {
            return;
        };
        let (fallback_start, fallback_end) = self
            .completion
            .as_ref()
            .map(|popup| (popup.replace_start, popup.replace_end))
            .unwrap_or((Pos::default(), Pos::default()));
        // Honour the server-provided edit range when present (it may span more
        // than the typed prefix, e.g. for member access after a dot).
        let (start, end) = match (item.edit_start, item.edit_end) {
            (Some(s), Some(e)) => {
                let doc = self.doc();
                let start_line = (s.line as usize).min(doc.line_count().saturating_sub(1));
                let end_line = (e.line as usize).min(doc.line_count().saturating_sub(1));
                (
                    Pos {
                        line: start_line,
                        byte: lsp::utf16_to_byte(doc.line(start_line), s.character),
                    },
                    Pos {
                        line: end_line,
                        byte: lsp::utf16_to_byte(doc.line(end_line), e.character),
                    },
                )
            }
            _ => (fallback_start, fallback_end),
        };
        self.completion = None;
        self.completion_request = None;
        let text = item.insert.replace("\r\n", "\n").replace('\r', "\n");
        self.replace_range(start, end, &text);
        self.keep_cursor_visible(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn dismiss_completion(&mut self, hwnd: HWND) {
        let had = self.completion.take().is_some() || self.completion_request.take().is_some();
        if had {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }
}

// (errors, warnings) among `diagnostics`; hints and information aren't counted.
fn problem_counts<'a>(diagnostics: impl Iterator<Item = &'a LspDiagnostic>) -> (usize, usize) {
    diagnostics.fold((0, 0), |(errors, warnings), diagnostic| {
        match diagnostic.severity {
            1 => (errors + 1, warnings),
            2 => (errors, warnings + 1),
            _ => (errors, warnings),
        }
    })
}

// The folder of the Cargo workspace `file`'s package belongs to, or of the
// package itself when it is in none.
fn cargo_root(file: &Path) -> Option<PathBuf> {
    let package = file
        .ancestors()
        .skip(1)
        .take(10)
        .find(|folder| folder.join("Cargo.toml").is_file())?;
    let workspace = package.ancestors().take(10).find(|folder| {
        std::fs::read_to_string(folder.join("Cargo.toml"))
            .ok()
            .and_then(|text| toml::from_str::<toml::Table>(&text).ok())
            .is_some_and(|manifest| manifest.contains_key("workspace"))
    });
    Some(workspace.unwrap_or(package).to_path_buf())
}

// The single edit turning `old` into `new`: the byte range in `old` past
// their common start and end, and the text of `new` that replaces it. None
// when they are equal.
fn changed_span<'a>(old: &str, new: &'a str) -> Option<(usize, usize, &'a str)> {
    if old == new {
        return None;
    }
    let mut prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(old.len().min(new.len()) - prefix)
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    Some((prefix, old.len() - suffix, &new[prefix..new.len() - suffix]))
}

// Where `offset` in `old` ends up once `span` (from `changed_span`) is
// applied. Formatters move whitespace and add or drop punctuation (`;`, `,`)
// but keep names and numbers, so inside the span it lands after as many
// word characters as preceded it there, then past the punctuation that
// stood between the last of them and `offset` (the `(` in `log(|`).
fn formatted_offset(old: &str, span: (usize, usize, &str), offset: usize) -> usize {
    let (from, to, replacement) = span;
    if offset <= from {
        return offset;
    }
    if offset >= to {
        return offset - to + from + replacement.len();
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let before = &old[from..offset];
    let words = before.chars().filter(|c| is_word(*c)).count();
    let tail_start = before
        .char_indices()
        .rev()
        .find(|(_, c)| is_word(*c))
        .map_or(0, |(index, c)| index + c.len_utf8());
    let mut at = match words {
        0 => 0,
        _ => replacement
            .char_indices()
            .filter(|(_, c)| is_word(*c))
            .nth(words - 1)
            .map_or(replacement.len(), |(index, c)| index + c.len_utf8()),
    };
    for expected in before[tail_start..].chars().filter(|c| !c.is_whitespace()) {
        let rest = &replacement[at..];
        let skipped = rest.len() - rest.trim_start().len();
        if !rest[skipped..].starts_with(expected) {
            break;
        }
        at += skipped + expected.len_utf8();
    }
    from + at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crates_in_a_cargo_workspace_share_its_root() {
        let root =
            std::env::temp_dir().join(format!("lightline-cargo-root-{}", std::process::id()));
        let write = |relative: &str, text: &str| {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write("ws/Cargo.toml", "[workspace]\nmembers = [\"a\", \"b\"]\n");
        write("ws/a/Cargo.toml", "[package]\nname = \"a\"\n");
        write("ws/b/Cargo.toml", "[package]\nname = \"b\"\n");
        write("solo/Cargo.toml", "[package]\nname = \"solo\"\n");
        let ws = root.join("ws");
        assert_eq!(cargo_root(&root.join("ws/a/src/lib.rs")), Some(ws.clone()));
        assert_eq!(cargo_root(&root.join("ws/b/src/lib.rs")), Some(ws));
        assert_eq!(
            cargo_root(&root.join("solo/src/main.rs")),
            Some(root.join("solo"))
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn positions_follow_their_code_through_formatting() {
        let old = "let   y=[1,2 ,3]\nend";
        let new = "let y = [1, 2, 3];\nend";
        let span = changed_span(old, new).unwrap();
        let at = |offset| formatted_offset(old, span, offset);
        // Inside the change: after "y", and after "3".
        assert_eq!(&new[..at(7)], "let y");
        assert_eq!(&new[..at(15)], "let y = [1, 2, 3");
        // The end of the changed line, and positions outside the change.
        assert_eq!(&new[..at(16)], "let y = [1, 2, 3];");
        assert_eq!(at(0), 0);
        assert_eq!(&new[at(17)..], "end");
        assert_eq!(at(old.len()), new.len());

        // Punctuation the formatter adds earlier (the `;`) doesn't shift it,
        // and punctuation just before the caret is kept.
        let old = "function f( ){return x}\nconsole.log(f())";
        let new = "function f() {\n  return x;\n}\nconsole.log(f());";
        let span = changed_span(old, new).unwrap();
        let at = |text: &str| {
            let offset = old.find(text).unwrap() + text.len();
            &new[..formatted_offset(old, span, offset)]
        };
        assert!(at("console.lo").ends_with("\nconsole.lo"));
        assert!(at("console.log(").ends_with("\nconsole.log("));
        assert!(at("return x").ends_with("return x"));
    }

    #[test]
    fn changed_span_covers_only_what_differs() {
        assert_eq!(changed_span("same", "same"), None);
        assert_eq!(
            changed_span("fn a(){x}\nfn b(){}", "fn a() { x }\nfn b(){}"),
            Some((6, 8, " { x "))
        );
        // Growing or shrinking at either end.
        assert_eq!(changed_span("ab", "abc"), Some((2, 2, "c")));
        assert_eq!(changed_span("abc", "bc"), Some((0, 1, "")));
        // Repeated text can't be counted as both prefix and suffix.
        assert_eq!(changed_span("aa", "aaa"), Some((2, 2, "a")));
        // Never splits a character: é and è share their first byte.
        assert_eq!(changed_span("xé", "xè"), Some((1, 3, "è")));
        assert_eq!(changed_span("éy", "èy"), Some((0, 2, "è")));
    }

    fn diagnostic(severity: u8) -> LspDiagnostic {
        let at = LspPosition {
            line: 0,
            character: 0,
        };
        LspDiagnostic {
            range: LspRange { start: at, end: at },
            severity,
            message: String::new(),
        }
    }

    #[test]
    fn problem_counts_count_errors_and_warnings_only() {
        let diagnostics: Vec<_> = [1, 2, 1, 3, 4, 2, 1].into_iter().map(diagnostic).collect();
        assert_eq!(problem_counts(diagnostics.iter()), (3, 2));
        assert_eq!(problem_counts([].iter()), (0, 0));
    }
}
