// Highlights the other places the name at the caret is used in the file,
// from the language server (textDocument/documentHighlight), as VS Code does.
// The server is asked only once the caret has rested on a name for
// OCCURRENCE_DELAY_MS; per key or click the cost is one comparison, and a
// redraw of the highlighted rows when the caret leaves them.

use super::*;
use lightline::lsp::{Command as LspCommand, Range as LspRange};

pub(super) const OCCURRENCE_TIMER: usize = 22;
const OCCURRENCE_DELAY_MS: u32 = 250;

// What the highlights depend on: the file and its version, and the caret.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct CaretKey {
    document: u64,
    version: i32,
    cursor: Pos,
    selecting: bool,
}

#[derive(Default)]
pub(super) struct Occurrences {
    // The file (Document::id) and LSP version the ranges are for; drawn only
    // while both still match, so an edit hides them at once.
    document: u64,
    version: i32,
    ranges: Vec<(Pos, Pos)>,
    // The caret last looked at, and the request on its way.
    caret: CaretKey,
    request: Option<(u64, CaretKey)>,
}

impl Occurrences {
    /// The server stopped: its answer won't come.
    pub(super) fn forget_request(&mut self) {
        self.request = None;
    }
}

impl App {
    fn caret_key(&self) -> CaretKey {
        let tab = self.tab();
        let view = self.view();
        CaretKey {
            document: tab.document.id(),
            version: tab.lsp_version,
            cursor: view.cursor,
            selecting: self.selection_range().is_some() || !view.extra.is_empty(),
        }
    }

    /// After a key or click: when the caret has moved, forgets highlights
    /// it has left and asks again once it rests.
    pub(super) fn track_caret(&mut self, hwnd: HWND) {
        if self.welcome || self.tabs.is_empty() {
            return;
        }
        let key = self.caret_key();
        if key == self.occurrences.caret {
            return;
        }
        self.occurrences.caret = key;
        let inside = |ranges: &[(Pos, Pos)]| {
            ranges
                .iter()
                .any(|&(start, end)| start <= key.cursor && key.cursor <= end)
        };
        if !self.occurrences.ranges.is_empty()
            && (key.document != self.occurrences.document
                || key.version != self.occurrences.version
                || key.selecting
                || !inside(&self.occurrences.ranges))
        {
            self.set_occurrences(hwnd, key, Vec::new());
        }
        let on_name = !key.selecting && {
            let line = self.doc().line(key.cursor.line);
            let word = |ch: char| ch.is_alphanumeric() || ch == '_';
            let (before, after) = (line.get(..key.cursor.byte), line.get(key.cursor.byte..));
            after.and_then(|text| text.chars().next()).is_some_and(word)
                || before
                    .and_then(|text| text.chars().next_back())
                    .is_some_and(word)
        };
        unsafe {
            if self.settings.occurrences_highlight && on_name && self.tab().lsp_opened {
                SetTimer(hwnd, OCCURRENCE_TIMER, OCCURRENCE_DELAY_MS, None);
            } else {
                KillTimer(hwnd, OCCURRENCE_TIMER);
            }
        }
    }

    /// The caret rested: asks where the name at it is used.
    pub(super) fn request_occurrences(&mut self) {
        let key = self.caret_key();
        if key != self.occurrences.caret || key.selecting {
            return;
        }
        let Some(path) = self.doc().path.as_deref() else {
            return;
        };
        let uri = lsp::file_uri(path);
        let doc = self.doc();
        let position = lsp::Position {
            line: key.cursor.line as u32,
            character: doc.utf16_column(key.cursor) as u32,
        };
        self.request_id += 1;
        let id = self.request_id;
        if self.tab_lsp(self.active).is_some_and(|client| {
            client.send(LspCommand::Highlights {
                id,
                uri,
                version: key.version,
                position,
            })
        }) {
            self.occurrences.request = Some((id, key));
        }
    }

    pub(super) fn finish_occurrences(
        &mut self,
        hwnd: HWND,
        id: u64,
        ranges: Option<Vec<LspRange>>,
    ) {
        let Some((asked, key)) = self.occurrences.request else {
            return;
        };
        if asked != id {
            return;
        }
        self.occurrences.request = None;
        // Moved or edited since: the next rest asks again.
        if key != self.caret_key() {
            return;
        }
        let Some(ranges) = ranges else {
            return;
        };
        let doc = self.doc();
        let to_pos = |position: lsp::Position| {
            let line = (position.line as usize).min(doc.line_count().saturating_sub(1));
            Pos {
                line,
                byte: lsp::utf16_to_byte(doc.line(line), position.character),
            }
        };
        let mut placed: Vec<(Pos, Pos)> = ranges
            .into_iter()
            .map(|range| (to_pos(range.start), to_pos(range.end)))
            .filter(|(start, end)| start < end)
            .collect();
        // A name used once has nothing else to show.
        if placed.len() < 2 {
            placed.clear();
        }
        self.set_occurrences(hwnd, key, placed);
    }

    // Replaces the highlights, redrawing the rows of the old and new ones.
    fn set_occurrences(&mut self, hwnd: HWND, key: CaretKey, ranges: Vec<(Pos, Pos)>) {
        let old = &self.occurrences;
        if old.ranges == ranges && old.document == key.document && old.version == key.version {
            return;
        }
        let lines: Vec<(usize, usize)> = old
            .ranges
            .iter()
            .chain(&ranges)
            .map(|(start, end)| (start.line, end.line))
            .collect();
        let shown_in = self
            .tabs
            .iter()
            .position(|tab| tab.document.id() == old.document);
        self.occurrences.document = key.document;
        self.occurrences.version = key.version;
        self.occurrences.ranges = ranges;
        let panes = if self.split_visible { 0..2 } else { 0..1 };
        for pane in panes {
            let tab = self.tab_for_pane(pane);
            if tab == self.active || Some(tab) == shown_in {
                for area in self.rows_showing(hwnd, pane, &lines) {
                    unsafe { InvalidateRect(hwnd, &area, 0) };
                }
            }
        }
    }

    /// The highlights to draw in tab `index`: none unless they're for its
    /// current text.
    pub(super) fn occurrences_in(&self, index: usize) -> &[(Pos, Pos)] {
        let tab = &self.tabs[index];
        if tab.document.id() == self.occurrences.document
            && tab.lsp_version == self.occurrences.version
        {
            &self.occurrences.ranges
        } else {
            &[]
        }
    }
}
