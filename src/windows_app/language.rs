use super::app::{CompletionPopup, CompletionRequest, HoverCard, HoverTarget, NavTarget, Tab};
use super::*;
use lightline::lsp::{Command as LspCommand, Position as LspPosition, Range as LspRange};

const LSP_MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
pub(super) const LSP_EVENT_MESSAGE: u32 = WM_APP + 7;

impl App {
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
        let root = self.lsp_root(language, &path);
        if self
            .lsp
            .get(&language)
            .is_none_or(|client| client.root() != root)
        {
            if self
                .lsp_failed_at
                .get(&language)
                .is_some_and(|when| when.elapsed() < Duration::from_secs(3))
            {
                return;
            }
            self.reset_language_client(language);
            if language == LspLanguage::Python && self.python_interpreter.is_none() {
                self.python_interpreter = workflow::detect_python_interpreter(Some(&root));
            }
            let hwnd_value = hwnd as isize;
            let wake = Arc::new(move || unsafe {
                PostMessageW(hwnd_value as HWND, LSP_EVENT_MESSAGE, 0, 0);
            });
            let client = LspClient::start(
                language,
                root,
                (language == LspLanguage::Python)
                    .then(|| self.python_interpreter.clone())
                    .flatten(),
                self.lsp_event_tx.clone(),
                wake,
            );
            self.lsp.insert(language, client);
            self.lsp_failed_at.remove(&language);
        }
        if !self.tab().lsp_opened || self.tab().lsp_language != Some(language) {
            let uri = lsp::file_uri(&path);
            let text = self.doc().text();
            let version = 1;
            if self
                .lsp
                .get(&language)
                .is_some_and(|client| client.send(LspCommand::Open { uri, text, version }))
            {
                let tab = self.tab_mut();
                tab.lsp_opened = true;
                tab.lsp_language = Some(language);
                tab.lsp_version = version;
                tab.lsp_serial = tab.document.change_serial();
            }
        }
    }

    fn lsp_root(&self, language: LspLanguage, path: &Path) -> PathBuf {
        match language {
            LspLanguage::Rust => path
                .ancestors()
                .skip(1)
                .take(10)
                .find(|folder| folder.join("Cargo.toml").is_file())
                .or_else(|| path.parent())
                .unwrap_or(Path::new("."))
                .to_path_buf(),
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
        }
    }

    fn reset_language_client(&mut self, language: LspLanguage) {
        self.lsp.remove(&language);
        for tab in &mut self.tabs {
            if tab.lsp_language == Some(language) {
                tab.lsp_opened = false;
                tab.lsp_language = None;
                tab.diagnostics.clear();
            }
        }
    }

    pub(super) fn sync_lsp_edit(&mut self) {
        let tab = self.tab();
        if !tab.lsp_opened || tab.lsp_serial == tab.document.change_serial() {
            return;
        }
        let Some(language) = tab.lsp_language else {
            return;
        };
        let Some(path) = tab.document.path.as_deref() else {
            return;
        };
        let Some(change) = tab.document.last_change().cloned() else {
            return;
        };
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
        let sent = self.lsp.get(&language).is_some_and(|client| {
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
        let tab = &mut self.tabs[index];
        if tab.lsp_opened {
            if let (Some(language), Some(path)) = (tab.lsp_language, tab.document.path.as_deref()) {
                let uri = lsp::file_uri(path);
                if let Some(client) = self.lsp.get(&language) {
                    client.send(LspCommand::Close { uri });
                }
            }
            tab.lsp_opened = false;
            tab.lsp_language = None;
        }
    }

    pub(super) fn lsp_after_save(&mut self, hwnd: HWND, old_path: Option<&Path>) {
        let changed_path = old_path != self.doc().path.as_deref();
        if changed_path {
            if self.tab().lsp_opened
                && let (Some(language), Some(path)) = (self.tab().lsp_language, old_path)
                && let Some(client) = self.lsp.get(&language)
            {
                client.send(LspCommand::Close {
                    uri: lsp::file_uri(path),
                });
            }
            let tab = self.tab_mut();
            tab.lsp_opened = false;
            tab.lsp_language = None;
            tab.diagnostics.clear();
        }
        self.ensure_lsp(hwnd);
        if self.tab().lsp_opened
            && let (Some(language), Some(path)) =
                (self.tab().lsp_language, self.doc().path.as_deref())
            && let Some(client) = self.lsp.get(&language)
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
                LspEvent::Stopped { language, message } => {
                    self.reset_language_client(language);
                    self.lsp_failed_at.insert(language, Instant::now());
                    self.hover_target = None;
                    self.hover_card = None;
                    self.definition_target = None;
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
                        (line, lsp::utf16_to_byte(doc.line(line), location.range.start.character))
                    };
                    self.move_cursor(Pos { line: byte.0, byte: byte.1 }, false);
                    self.keep_cursor_visible(hwnd);
                    self.status = format!("Jumped to definition (line {})", byte.0 + 1);
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
                    resolved.sort_by(|a, b| {
                        (b.0.line, b.0.byte).cmp(&(a.0.line, a.0.byte))
                    });
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
        self.reset_language_client(LspLanguage::Python);
        self.lsp_failed_at.remove(&LspLanguage::Python);
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
        if y < self.scale(TAB_HEIGHT)
            && Tab::is_runnable(self.doc())
            && x >= rect.right - self.scale(326)
            && x < rect.right - self.scale(296)
        {
            let hint = "Run File (Ctrl+Shift+R)";
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
            || x >= rect.right
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
        if self.lsp.get(&language).is_some_and(|client| {
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
        if self.lsp.get(&language).is_some_and(|client| {
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

    pub(super) fn is_prettier_supported(path: Option<&std::path::Path>) -> bool {
        let Some(path) = path else { return false };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        matches!(
            ext.as_str(),
            "js" | "mjs" | "cjs" | "jsx"
                | "ts" | "mts" | "cts" | "tsx"
                | "json"
                | "css" | "scss" | "less"
                | "html" | "htm"
                | "md" | "markdown"
                | "yaml" | "yml"
                | "graphql" | "gql"
                | "vue"
        )
    }

    pub(super) fn format_with_prettier(&mut self, hwnd: HWND) {
        if self.tab().read_only() {
            return;
        }
        let code = self.doc().text();
        if code.trim().is_empty() {
            self.status = "Nothing to format".into();
            self.refresh(hwnd);
            return;
        }
        let filepath = self
            .doc()
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "document.js".to_string());

        #[cfg(windows)]
        use std::os::windows::process::CommandExt;
        use std::io::Write;
        use std::process::{Command, Stdio};

        let mut child_res = {
            let mut cmd = Command::new("cmd");
            cmd.args(&["/C", "prettier", "--stdin-filepath", &filepath]);
            #[cfg(windows)]
            cmd.creation_flags(0x08000000);
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        };

        if child_res.is_err() {
            let mut cmd = Command::new("cmd");
            cmd.args(&["/C", "npx", "--yes", "prettier", "--stdin-filepath", &filepath]);
            #[cfg(windows)]
            cmd.creation_flags(0x08000000);
            child_res = cmd
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
        }

        let mut child = match child_res {
            Ok(c) => c,
            Err(e) => {
                self.status = format!("Could not launch Prettier: {e}");
                self.refresh(hwnd);
                return;
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(code.as_bytes());
        }

        let output = match child.wait_with_output() {
            Ok(o) => o,
            Err(e) => {
                self.status = format!("Prettier failed: {e}");
                self.refresh(hwnd);
                return;
            }
        };

        if output.status.success() {
            let formatted = String::from_utf8_lossy(&output.stdout).to_string();
            let formatted_clean = formatted.replace("\r\n", "\n").replace('\r', "\n");
            let current_clean = code.replace("\r\n", "\n").replace('\r', "\n");
            if formatted_clean == current_clean {
                self.status = "Already formatted with Prettier".into();
            } else {
                let doc = self.doc();
                let last_line = doc.line_count().saturating_sub(1);
                let end = Pos {
                    line: last_line,
                    byte: doc.line(last_line).len(),
                };
                self.replace_range(Pos::default(), end, &formatted_clean);
                self.status = "Document formatted with Prettier".into();
            }
        } else {
            let err = String::from_utf8_lossy(&output.stderr);
            let first_err = err
                .lines()
                .find(|l| l.contains("Error") || l.contains("SyntaxError"))
                .unwrap_or("Prettier formatting error");
            self.status = first_err.trim().to_string();
        }
        self.refresh(hwnd);
    }

    pub(super) fn format_document(&mut self, hwnd: HWND) {
        let pane = self.focused_pane;
        let index = self.tab_for_pane(pane);
        let path = self.tabs[index].document.path.clone();

        if self.has_extension("prettier") && Self::is_prettier_supported(path.as_deref()) {
            self.format_with_prettier(hwnd);
            return;
        }

        if Self::is_prettier_supported(path.as_deref()) {
            self.status = "Install Prettier extension to format this file (Ctrl+Shift+X)".into();
            self.refresh(hwnd);
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
        if self.lsp.get(&language).is_some_and(|client| {
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
        if self.lsp.get(&language).is_some_and(|client| {
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
                let start_line =
                    (s.line as usize).min(doc.line_count().saturating_sub(1));
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
