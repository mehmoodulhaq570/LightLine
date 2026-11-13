// Saving a large file off the UI thread. Ctrl+S on a file over
// BACKGROUND_SAVE_BYTES copies its text into one buffer (as fast as
// `Document::text`), and a worker writes, flushes and moves that into place
// the way a synchronous save does. Until it finishes the tab stays unsaved, so
// the recovery snapshot keeps the text; once it has, the document is marked
// saved at the revision that was copied, so edits typed meanwhile stay unsaved.
//
// Only one runs at a time. Anything that needs the file on disk, or a true
// answer to "is it saved?", calls `finish_background_save` first, which waits
// for it on the UI thread: another save, closing a tab or the window, Run and
// Debug, workspace edits, Git writes, Explorer delete and rename.

use super::*;
use lightline::document::SaveStamp;
use std::thread::JoinHandle;

/// Files at least this large (in bytes, not counting CRs) save in the
/// background on Ctrl+S; smaller ones save on the spot, as before. Measured
/// on a nearly full SSD (two runs, median of seven saves), a synchronous save
/// took 12-18 ms at 2 MiB and 18-29 ms at 4 MiB, then jumped to 83-96 ms at
/// 8 MiB and 310-340 ms at 36 MiB, mostly waiting for the flush to disk. The
/// background path still copies the text on the UI thread (6-12 ms at
/// 4 MiB), so below this it would save little.
pub(super) const BACKGROUND_SAVE_BYTES: usize = 4 * 1024 * 1024;

pub(super) struct BackgroundSave {
    // Matches the worker's WorkerMessage::Saved.
    id: u64,
    // The saved document (`Document::id`) and the revision copied.
    doc_id: u64,
    revision: u64,
    path: PathBuf,
    worker: JoinHandle<io::Result<SaveStamp>>,
}

impl App {
    /// Starts writing the active document to `path` on a worker. False if the
    /// thread couldn't be started; nothing was written then.
    pub(super) fn start_background_save(&mut self, hwnd: HWND, path: &Path) -> bool {
        debug_assert!(self.background_save.is_none());
        let snapshot = self.doc().snapshot_for_save(path);
        let (doc_id, revision) = (self.doc().id(), snapshot.revision);
        self.background_save_serial += 1;
        let id = self.background_save_serial;
        let tx = self.worker_tx.clone();
        let worker = std::thread::Builder::new()
            .name("save".into())
            .spawn(move || {
                let result = snapshot.write();
                drop(snapshot);
                tx.send(WorkerMessage::Saved(id));
                result
            });
        let Ok(worker) = worker else {
            return false;
        };
        self.background_save = Some(BackgroundSave {
            id,
            doc_id,
            revision,
            path: path.to_owned(),
            worker,
        });
        self.status = format!(
            "Saving {}...",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        self.update_title(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
        true
    }

    /// The worker said save `id` is done.
    pub(super) fn background_save_done(&mut self, hwnd: HWND, id: u64) {
        if self
            .background_save
            .as_ref()
            .is_some_and(|save| save.id == id)
        {
            self.finish_background_save(hwnd);
        }
    }

    /// True while document `doc_id` is being saved in the background.
    pub(super) fn saving_in_background(&self, doc_id: u64) -> bool {
        self.background_save
            .as_ref()
            .is_some_and(|save| save.doc_id == doc_id)
    }

    /// Waits for the background save, if one is running, and applies its
    /// result as a synchronous save would: the document marked saved and the
    /// after-save steps, or the error shown and the document left unsaved.
    /// Returns the saved document's id when it succeeded.
    pub(super) fn finish_background_save(&mut self, hwnd: HWND) -> Option<u64> {
        let save = self.background_save.take()?;
        let result = save.worker.join().unwrap_or_else(|_| {
            Err(io::Error::other(
                "The save stopped unexpectedly; the file was not changed",
            ))
        });
        let index = self.tabs.iter().position(|tab| {
            tab.document.id() == save.doc_id
                && tab
                    .document
                    .path
                    .as_deref()
                    .is_some_and(|path| path == save.path)
        });
        match (result, index) {
            (Ok(stamp), Some(index)) => {
                self.tabs[index]
                    .document
                    .finish_save(&save.path, stamp, save.revision);
                self.after_save(hwnd, index, Some(&save.path), &save.path);
                self.update_title(hwnd);
                unsafe { InvalidateRect(hwnd, null(), 0) };
                Some(save.doc_id)
            }
            // Written, but its tab was closed or now holds another file:
            // there is nothing to mark saved.
            (Ok(_), None) => {
                self.status = format!(
                    "Saved {}",
                    save.path.file_name().unwrap_or_default().to_string_lossy()
                );
                unsafe { InvalidateRect(hwnd, null(), 0) };
                None
            }
            (Err(error), _) => {
                self.error(hwnd, &error);
                None
            }
        }
    }
}
