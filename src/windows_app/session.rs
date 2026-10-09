use super::*;

type RecoveryTabKey = (Option<PathBuf>, u64, bool, bool, bool, [EditorView; 2]);

#[derive(PartialEq, Eq)]
pub(super) struct RecoveryKey {
    root: Option<PathBuf>,
    active: usize,
    tabs: Vec<RecoveryTabKey>,
}

impl App {
    fn recovery_key(&self) -> RecoveryKey {
        RecoveryKey {
            root: self.workspace_root.clone(),
            active: self.active,
            tabs: self
                .tabs
                .iter()
                .map(|tab| {
                    (
                        tab.document.path.clone(),
                        tab.document.change_serial(),
                        tab.document.is_dirty(),
                        tab.unloaded,
                        tab.read_only(),
                        tab.views.clone(),
                    )
                })
                .collect(),
        }
    }
    // Snapshot tabs and unsaved text; previews and the empty stand-in are excluded.
    fn capture_session(&self) -> workflow::Session {
        let to_view = |view: &EditorView| workflow::SessionView {
            cursor: (view.cursor.line, view.cursor.byte),
            anchor: view.selection_anchor.map(|pos| (pos.line, pos.byte)),
            first_line: view.first_line,
        };
        let mut tabs = Vec::new();
        let mut active = 0;
        for (index, tab) in self.tabs.iter().enumerate() {
            if tab.is_placeholder() || (tab.read_only() && tab.document.path.is_none()) {
                continue;
            }
            let path = tab.document.path.clone();
            if path.is_none()
                && !tab.document.is_dirty()
                && tab.document.line_count() == 1
                && tab.document.line(0).is_empty()
            {
                continue;
            }
            let recovery =
                (!tab.read_only() && !tab.unloaded && (tab.document.is_dirty() || path.is_none()))
                    .then(|| tab.recovery_text());
            if index == self.active {
                active = tabs.len();
            }
            let views = [to_view(&tab.views[0]), to_view(&tab.views[1])];
            tabs.push(workflow::SessionTab {
                path,
                recovery,
                stamp: tab.document.recovery_stamp(),
                views,
            });
        }
        workflow::Session {
            root: self.workspace_root.clone(),
            active,
            tabs,
        }
    }

    // Queues a snapshot if anything it records changed; true if one was.
    pub(super) fn save_session(&self) -> bool {
        if self.restoring {
            return false;
        }
        let key = self.recovery_key();
        if self.recovery_key.borrow().as_ref() == Some(&key) {
            return false;
        }
        if workflow::queue_session(self.capture_session(), false).is_ok() {
            *self.recovery_key.borrow_mut() = Some(key);
            return true;
        }
        false
    }

    // Takes a snapshot five seconds from now unless one is already due.
    // Called on every paint, since whatever a snapshot records (text, caret,
    // scroll, tabs) is drawn when it changes; an idle window then sets no
    // timer at all.
    pub(super) fn arm_recovery(&self) {
        if !self.recovery_armed.replace(true) {
            unsafe { SetTimer(self.hwnd, RECOVERY_TIMER, 5000, None) };
        }
    }

    pub(super) fn flush_session(&mut self, hwnd: HWND) -> bool {
        match workflow::queue_session(self.capture_session(), true) {
            Ok(()) => true,
            Err(error) => {
                self.error(hwnd, &format!("Could not write session recovery: {error}"));
                false
            }
        }
    }

    pub(super) fn recovery_tick(&mut self) {
        if let Some(error) = workflow::take_session_error() {
            self.status = format!("Session recovery failed: {error}");
            *self.recovery_key.borrow_mut() = None;
            unsafe { InvalidateRect(self.hwnd, null(), 0) };
        }
        // Looked at again shortly after a write, to report it if it failed.
        if self.save_session() {
            self.arm_recovery();
        }
    }

    // Reopen the last session: the workspace, and a tab for every file that
    // still exists, in the same order and with each tab's cursor, selection
    // and scroll position. Only the file that was showing is read now; the
    // others are read when first shown (Tab::unloaded). Reading them all
    // first made startup wait for every tab: with 20 tabs the window took
    // twice as long to respond.
    pub(super) fn restore_session(&mut self, hwnd: HWND) {
        let mut session = workflow::load_session();
        if session.tabs.iter().any(|tab| tab.recovery.is_some()) {
            let answer = dialog::show_dialog(
                hwnd,
                "Recover unsaved work",
                "LightLine found unsaved work from your previous session. Restore it? Original files will remain unchanged until you save.",
                dialog::DialogIcon::Question,
                &[
                    dialog::DialogButton {
                        label: "Restore",
                        id: dialog::DLG_YES,
                        is_default: true,
                        is_cancel: true,
                    },
                    dialog::DialogButton {
                        label: "Discard",
                        id: dialog::DLG_NO,
                        is_default: false,
                        is_cancel: false,
                    },
                ],
            );
            if answer == dialog::DLG_NO {
                for tab in &mut session.tabs {
                    tab.recovery = None;
                }
                session
                    .tabs
                    .retain(|tab| tab.path.as_ref().is_some_and(|p| p.is_file()));
            }
        }
        if session.root.is_none() && session.tabs.is_empty() {
            return;
        }
        self.restoring = true;
        if let Some(root) = session.root.clone() {
            self.set_workspace(hwnd, root);
        }
        let view = |saved: &workflow::SessionView| EditorView {
            cursor: Pos {
                line: saved.cursor.0,
                byte: saved.cursor.1,
            },
            selection_anchor: saved.anchor.map(|(line, byte)| Pos { line, byte }),
            first_line: saved.first_line,
            first_row: 0,
        };
        let shown = session.active.min(session.tabs.len().saturating_sub(1));
        let mut active = None;
        for (position, saved) in session.tabs.iter().enumerate() {
            if let Some(text) = &saved.recovery {
                // Keep the original association only if disk still matches the snapshot.
                // Otherwise recover into an untitled buffer so saving cannot overwrite an
                // external edit or a deleted original without a Save As choice.
                let doc = Document::recover(saved.path.as_deref(), text, saved.stamp);
                let views = saved.views.clone().map(|saved| {
                    let mut restored = view(&saved);
                    restored.cursor = doc.grapheme_position(restored.cursor);
                    restored.selection_anchor =
                        restored.selection_anchor.map(|p| doc.grapheme_position(p));
                    restored.first_line =
                        restored.first_line.min(doc.line_count().saturating_sub(1));
                    restored
                });
                let mut tab = Tab::new(doc);
                tab.views = views;
                if self.tabs.len() == 1 && self.tabs[0].is_placeholder() {
                    self.tabs[0] = tab;
                } else {
                    self.tabs.push(tab);
                }
                let index = self.tabs.len() - 1;
                if let (Some(watcher), Some(path)) =
                    (&self.watcher, &self.tabs[index].document.path)
                {
                    watcher.watch_file(path.clone());
                }
                if position == shown {
                    active = Some(index);
                }
                continue;
            }
            let Some(path) = &saved.path else {
                continue;
            };
            if !path.is_file() {
                continue;
            }
            if position == shown {
                self.open(hwnd, Some(path.clone()));
                // `open` activates the tab it just opened, so the active
                // index is the tab to position.
                let index = self.active;
                let doc = &self.tabs[index].document;
                let views = saved.views.clone().map(|saved| {
                    let restored = view(&saved);
                    EditorView {
                        cursor: doc.grapheme_position(restored.cursor),
                        selection_anchor: restored.selection_anchor.map(|pos| doc.clamp(pos)),
                        first_line: restored.first_line.min(doc.line_count().saturating_sub(1)),
                        first_row: 0,
                    }
                });
                self.tabs[index].views = views;
                active = Some(index);
            } else {
                let tab =
                    Tab::unloaded(path.clone(), saved.views.clone().map(|saved| view(&saved)));
                // Until a file opens, the editor's one tab is an empty stand-in.
                if self.tabs.len() == 1 && self.tabs[0].is_placeholder() {
                    self.tabs[0] = tab;
                } else {
                    self.tabs.push(tab);
                }
            }
        }
        if self.tabs.iter().all(Tab::is_placeholder) {
            self.restoring = false;
            return;
        }
        // Normally the file that was showing; if it's gone, the first tab.
        self.welcome = false;
        self.activate_tab(hwnd, active.unwrap_or(0));
        self.keep_cursor_visible(hwnd);
        self.status = if session.tabs.iter().any(|tab| tab.recovery.is_some()) {
            "Restored previous session, including unsaved work".into()
        } else {
            "Restored previous session".into()
        };
        self.restoring = false;
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}
