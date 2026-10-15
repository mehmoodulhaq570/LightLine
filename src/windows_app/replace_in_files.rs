//! Replace across files from the Search panel: a second field under the
//! query, and Replace All (its button, or Enter in the field). The files are
//! found on a worker; after a confirmation, open files change as one undo step
//! each (saved if they had no unsaved work) and the rest are written to disk.

use super::dialog::{self, BTN_CANCEL, DLG_OK, DialogButton, DialogIcon};
use super::workspace_edit::{Edits, count};
use super::*;

const BTN_REPLACE: DialogButton = DialogButton {
    label: "Replace",
    id: DLG_OK,
    is_default: true,
    is_cancel: false,
};

/// A Replace All whose files are being found.
pub(super) struct ReplaceRequest {
    pub(super) root: PathBuf,
    pub(super) query: String,
    pub(super) with: String,
    pub(super) cancel: Arc<AtomicBool>,
}

impl App {
    // Rows of the Search panel, top to bottom: query, replacement (with the
    // Replace All button at its right), the count, the results.
    pub(super) fn search_query_box(&self, left: i32, right: i32) -> RECT {
        RECT {
            left: left + self.scale(8),
            top: self.scale(47),
            right: right - self.scale(8),
            bottom: self.scale(78),
        }
    }

    pub(super) fn replace_box(&self, left: i32, right: i32) -> RECT {
        RECT {
            left: left + self.scale(8),
            top: self.scale(84),
            right: self.replace_all_button(right).left - self.scale(6),
            bottom: self.scale(115),
        }
    }

    pub(super) fn replace_all_button(&self, right: i32) -> RECT {
        RECT {
            left: right - self.scale(8 + 92),
            top: self.scale(84),
            right: right - self.scale(8),
            bottom: self.scale(115),
        }
    }

    pub(super) fn search_count_top(&self) -> i32 {
        self.scale(124)
    }

    pub(super) fn search_list_top(&self) -> i32 {
        self.scale(150)
    }

    /// Replace All: finds every workspace file with the query.
    pub(super) fn start_replace_all(&mut self, hwnd: HWND) {
        let Some(root) = self.workspace_root.clone() else {
            return;
        };
        let query = self.project_query.clone();
        let with = self.search_replacement.clone();
        if query.is_empty() {
            self.status = "Type what to find first".into();
        } else if query == with {
            self.status = "The replacement is the same as the text".into();
        } else {
            if let Some(request) = self.replace_request.take() {
                request.cancel.store(true, Ordering::Relaxed);
            }
            let cancel = Arc::new(AtomicBool::new(false));
            let open: Vec<PathBuf> = self
                .tabs
                .iter()
                .filter(|tab| !tab.unloaded && !tab.is_placeholder() && tab.markdown.is_none())
                .filter_map(|tab| tab.document.path.clone())
                .collect();
            self.replace_request = Some(ReplaceRequest {
                root: root.clone(),
                query: query.clone(),
                with,
                cancel: cancel.clone(),
            });
            self.status = format!("Finding \u{201c}{query}\u{201d} in the workspace...");
            let tx = self.worker_tx.clone();
            std::thread::spawn(move || {
                let targets = workflow::replace_targets(&root, &query, &open, &cancel);
                tx.send(WorkerMessage::ReplaceTargets(cancel, targets));
            });
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// The worker's files for the current Replace All: asks, then replaces.
    pub(super) fn finish_replace_all(
        &mut self,
        hwnd: HWND,
        cancel: Arc<AtomicBool>,
        targets: Vec<(PathBuf, usize)>,
    ) {
        let current = self
            .replace_request
            .as_ref()
            .is_some_and(|request| Arc::ptr_eq(&request.cancel, &cancel));
        if !current {
            return;
        }
        let Some(request) = self.replace_request.take() else {
            return;
        };
        if self.workspace_root.as_ref() != Some(&request.root) {
            return;
        }
        // Open files count by their text in memory, which is what changes.
        let mut places = 0;
        let mut files = Vec::new();
        for (path, on_disk) in targets {
            let uri = lsp::file_uri(&path);
            let found = match self.open_tab_for_uri(&uri) {
                Some(index) => self.tabs[index].document.find_all(&request.query).len(),
                None => on_disk,
            };
            if found > 0 {
                places += found;
                files.push(uri);
            }
        }
        if files.is_empty() {
            self.status = format!("No \u{201c}{}\u{201d} in the workspace", request.query);
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let message = format!(
            "Replace {} of \u{201c}{}\u{201d} with \u{201c}{}\u{201d} in {}?\n\n\
             Open files can be undone with Ctrl+Z. Files that aren't open are saved at once.",
            count(places, "occurrence"),
            request.query,
            request.with,
            count(files.len(), "file"),
        );
        let answer = dialog::show_dialog(
            hwnd,
            "Replace All",
            &message,
            DialogIcon::Question,
            &[BTN_REPLACE, BTN_CANCEL],
        );
        if answer != DLG_OK {
            self.status = "Replace All cancelled".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let edits = files
            .into_iter()
            .map(|uri| {
                (
                    uri,
                    Edits::ReplaceAll {
                        query: request.query.clone(),
                        with: request.with.clone(),
                    },
                )
            })
            .collect();
        self.status = match self.apply_edits(edits, None, true) {
            Ok(applied) if applied.failed.is_empty() => format!(
                "Replaced {} in {}",
                count(applied.places, "occurrence"),
                count(applied.files, "file")
            ),
            Ok(applied) => format!(
                "Replaced {} in {}; not saved: {}",
                count(applied.places, "occurrence"),
                count(applied.files, "file"),
                applied.failed.join(", ")
            ),
            Err(error) => format!("Nothing was replaced: {error}"),
        };
        // The results showed the old text, as VS Code does they go.
        self.cancel_search();
        self.search_results.clear();
        self.panel_selected = 0;
        self.panel_first = 0;
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}
