//! Git blame on the caret's line: who last changed it, when, and the
//! commit's summary, faint after the line's end. Setting `gitBlame`.
//!
//! `git blame` runs once per file and HEAD, on a worker. The buffer's lines
//! are mapped onto HEAD's through the gutter diff's hunks, so typing never
//! runs Git again: lines in a hunk are "Uncommitted changes".

use super::*;
use lightline::workflow::Blame;

impl App {
    /// Starts a blame of the active file if it has none and could have one.
    pub(super) fn ensure_blame(&mut self) {
        if !self.settings.git_blame || self.welcome || self.tabs.is_empty() {
            return;
        }
        let tab = self.tab();
        if tab.read_only() || tab.unloaded || tab.markdown.is_some() || tab.image.is_some() {
            return;
        }
        let Some(path) = tab.document.path.clone() else {
            return;
        };
        let Some(root) = self.git_root.clone() else {
            return;
        };
        if self.blame_cache.contains_key(&path)
            || self.blame_requests.contains(&path)
            || self.git_untracked.contains(&path)
        {
            return;
        }
        let Some(relative) = repo_relative(&path, &root) else {
            return;
        };
        self.blame_requests.insert(path.clone());
        let generation = self.blame_generation;
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            let blame = workflow::git_blame(&root, &relative).unwrap_or_default();
            tx.send(WorkerMessage::Blame(generation, path, Arc::new(blame)));
        });
    }

    pub(super) fn blame_finished(
        &mut self,
        hwnd: HWND,
        generation: u64,
        path: PathBuf,
        blame: Arc<Blame>,
    ) {
        if generation != self.blame_generation {
            return;
        }
        self.blame_requests.remove(&path);
        self.blame_cache.insert(path.clone(), blame);
        // The caret's line in each pane showing the file.
        let panes = if self.split_visible { 0..2 } else { 0..1 };
        for pane in panes {
            let tab = &self.tabs[self.tab_for_pane(pane)];
            if tab.document.path.as_ref() != Some(&path) {
                continue;
            }
            let line = self.view_for_pane(pane).cursor.line;
            for area in self.rows_showing(hwnd, pane, &[(line, line)]) {
                unsafe { InvalidateRect(hwnd, &area, 0) };
            }
        }
    }

    /// HEAD moved or the repository changed: every blame is out of date.
    pub(super) fn forget_blame(&mut self) {
        self.blame_cache.clear();
        self.blame_requests.clear();
        self.blame_generation += 1;
    }

    /// What's shown after line `line` of tab `index`, if anything.
    pub(super) fn blame_label(&self, index: usize, line: usize) -> Option<String> {
        if !self.settings.git_blame {
            return None;
        }
        let tab = &self.tabs[index];
        let path = tab.document.path.as_ref()?;
        let blame = self.blame_cache.get(path)?;
        if blame.lines.is_empty() {
            return None;
        }
        // Through the hunks between the buffer and HEAD, when the file
        // differs from it.
        let mut head_line = line as isize;
        if let Some(diff) = self.git_diff_cache.get(path) {
            for hunk in &diff.hunks {
                if line >= hunk.buffer_start + hunk.buffer_len {
                    head_line += hunk.head_len as isize - hunk.buffer_len as isize;
                } else if line >= hunk.buffer_start {
                    return Some("You  \u{2022}  Uncommitted changes".into());
                }
            }
        } else if tab.document.is_dirty() {
            // Not compared with HEAD yet: no telling which line is which.
            return None;
        }
        let commit = blame.line(usize::try_from(head_line).ok()?)?;
        if commit.uncommitted() {
            return Some("You  \u{2022}  Uncommitted changes".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs() as i64);
        Some(format!(
            "{}, {}  \u{2022}  {}",
            commit.author,
            ago(now - commit.time),
            commit.summary
        ))
    }
}

/// "just now", "5 minutes ago", "3 days ago", "2 years ago".
pub(super) fn ago(seconds: i64) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    const MONTH: i64 = 30 * DAY;
    const YEAR: i64 = 365 * DAY;
    let (count, unit) = match seconds.max(0) {
        s if s < MINUTE => return "just now".into(),
        s if s < HOUR => (s / MINUTE, "minute"),
        s if s < DAY => (s / HOUR, "hour"),
        s if s < MONTH => (s / DAY, "day"),
        s if s < YEAR => (s / MONTH, "month"),
        s => (s / YEAR, "year"),
    };
    if count == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{count} {unit}s ago")
    }
}

#[cfg(test)]
mod tests {
    use super::ago;

    #[test]
    fn times_read_as_how_long_ago() {
        assert_eq!(ago(5), "just now");
        assert_eq!(ago(-30), "just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(5 * 60), "5 minutes ago");
        assert_eq!(ago(3 * 3600 + 5), "3 hours ago");
        assert_eq!(ago(2 * 86_400), "2 days ago");
        assert_eq!(ago(65 * 86_400), "2 months ago");
        assert_eq!(ago(800 * 86_400), "2 years ago");
    }
}
