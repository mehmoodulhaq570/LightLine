use super::app::{HoverCard, HoverTarget, Tab};
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
                    self.status = message;
                }
                LspEvent::Status { message, .. } => {
                    self.status = message;
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
            && Tab::is_python(self.doc())
            && x >= rect.right - self.scale(326)
            && x < rect.right - self.scale(296)
        {
            let hint = "Run Python File (Ctrl+Shift+R)";
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
}
