use super::*;

impl App {
    // Snapshot the current workspace and every path-backed tab (skipping the
    // transient untitled buffer) into a serializable session.
    fn capture_session(&self) -> workflow::Session {
        let to_view = |view: &EditorView| workflow::SessionView {
            cursor: (view.cursor.line, view.cursor.byte),
            anchor: view.selection_anchor.map(|pos| (pos.line, pos.byte)),
            first_line: view.first_line,
        };
        let mut tabs = Vec::new();
        let mut active = 0;
        for (index, tab) in self.tabs.iter().enumerate() {
            let Some(path) = tab.document.path.clone() else {
                continue;
            };
            if index == self.active {
                active = tabs.len();
            }
            let views = [to_view(&tab.views[0]), to_view(&tab.views[1])];
            tabs.push(workflow::SessionTab { path, views });
        }
        workflow::Session {
            root: self.workspace_root.clone(),
            active,
            tabs,
        }
    }

    pub(super) fn save_session(&self) {
        if self.restoring {
            return;
        }
        workflow::save_session(&self.capture_session());
    }

    // Reopen the last session: restore the workspace, then every file that
    // still exists, and finally each tab's cursor, selection, and scroll
    // position before activating the previously focused tab.
    pub(super) fn restore_session(&mut self, hwnd: HWND) {
        let session = workflow::load_session();
        if session.root.is_none() && session.tabs.is_empty() {
            return;
        }
        self.restoring = true;
        if let Some(root) = session.root.clone() {
            self.set_workspace(hwnd, root);
        }
        for saved in &session.tabs {
            if !saved.path.is_file() {
                continue;
            }
            self.open(hwnd, Some(saved.path.clone()));
            // `open` activates the tab it just opened (or focuses the existing
            // one), so the current active index is the tab to position.
            let index = self.active;
            for (pane, view) in saved.views.iter().enumerate().take(2) {
                let (cursor, selection_anchor, first_line) = {
                    let doc = &self.tabs[index].document;
                    (
                        doc.clamp(Pos {
                            line: view.cursor.0,
                            byte: view.cursor.1,
                        }),
                        view.anchor
                            .map(|(line, byte)| doc.clamp(Pos { line, byte })),
                        view.first_line.min(doc.line_count().saturating_sub(1)),
                    )
                };
                self.tabs[index].views[pane] = EditorView {
                    cursor,
                    selection_anchor,
                    first_line,
                };
            }
        }
        if self.tabs.is_empty() {
            self.restoring = false;
            return;
        }
        let active = session.active.min(self.tabs.len() - 1);
        self.activate_tab(hwnd, active);
        self.keep_cursor_visible(hwnd);
        self.status = "Restored previous session".into();
        self.restoring = false;
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}
