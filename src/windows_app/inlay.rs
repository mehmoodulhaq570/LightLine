// Inlay hints: text the language server shows inside lines without it being
// in the file, such as a variable's type after its name. They're asked for
// when a file's server has looked at it and after typing pauses
// (INLAY_TIMER), never per keystroke; until new ones come, those an edit
// moved go with their text.
//
// A hint sits before the character at its byte: the caret at that byte is
// drawn before the hint, the text from that byte after it. `hint_shift` says
// how far hints push a byte along its screen row, and everything that turns
// a byte into an x (drawing, carets, clicks) adds it.

use super::*;
use lightline::document::TextChange;
use lightline::lsp::{
    Command as LspCommand, InlayHint, Position as LspPosition, Range as LspRange,
};

pub(super) const INLAY_TIMER: usize = 21;
// How long typing must pause before hints are asked for again.
const INLAY_DELAY_MS: u32 = 400;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct InlayHintAt {
    pub(super) line: usize,
    pub(super) byte: usize,
    pub(super) label: String,
}

pub(super) struct InlayRequest {
    id: u64,
    uri: String,
    version: i32,
}

impl App {
    /// Asks for the active file's hints, unless those shown are for its
    /// current text or a request for it is on its way.
    pub(super) fn request_inlay_hints(&mut self) {
        if !self.settings.inlay_hints || self.welcome {
            return;
        }
        let tab = self.tab();
        let (true, Some(path)) = (tab.lsp_opened, tab.document.path.as_deref()) else {
            return;
        };
        if tab.inlay_version == Some(tab.lsp_version) {
            return;
        }
        let uri = lsp::file_uri(path);
        let version = tab.lsp_version;
        if self
            .inlay_request
            .as_ref()
            .is_some_and(|request| request.uri == uri && request.version == version)
        {
            return;
        }
        // Up to the real end: a line past it is an error to rust-analyzer.
        let end = tab.document.end();
        let range = LspRange {
            start: LspPosition {
                line: 0,
                character: 0,
            },
            end: LspPosition {
                line: end.line as u32,
                character: tab.document.utf16_column(end) as u32,
            },
        };
        self.request_id += 1;
        let id = self.request_id;
        if self.tab_lsp(self.active).is_some_and(|client| {
            client.send(LspCommand::InlayHints {
                id,
                uri: uri.clone(),
                version,
                range,
            })
        }) {
            self.inlay_request = Some(InlayRequest { id, uri, version });
        }
    }

    /// After an edit: asks again once typing pauses.
    pub(super) fn schedule_inlay_hints(&self) {
        if self.settings.inlay_hints {
            unsafe { SetTimer(self.hwnd, INLAY_TIMER, INLAY_DELAY_MS, None) };
        }
    }

    pub(super) fn finish_inlay_hints(
        &mut self,
        hwnd: HWND,
        id: u64,
        uri: &str,
        version: i32,
        hints: Option<Vec<InlayHint>>,
    ) {
        if self
            .inlay_request
            .as_ref()
            .is_none_or(|request| request.id != id)
        {
            return;
        }
        self.inlay_request = None;
        // An error (often "content modified" while the server loads): the
        // hints shown stay until it sends its refresh.
        let Some(hints) = hints else {
            return;
        };
        let Some(index) = self.open_tab_for_uri(uri) else {
            return;
        };
        let tab = &mut self.tabs[index];
        // Edited since: the request after the pause brings newer ones.
        if tab.lsp_version != version {
            return;
        }
        let doc = &tab.document;
        let placed: Vec<InlayHintAt> = hints
            .into_iter()
            .filter(|hint| (hint.position.line as usize) < doc.line_count())
            .map(|hint| {
                let line = hint.position.line as usize;
                InlayHintAt {
                    line,
                    byte: lsp::utf16_to_byte(doc.line(line), hint.position.character),
                    label: hint.label,
                }
            })
            .collect();
        // None while there are none: a server still loading answers with
        // nothing, so the next look asks again.
        tab.inlay_version = (!placed.is_empty()).then_some(version);
        if tab.inlay_hints == placed {
            return;
        }
        tab.inlay_hints = placed;
        let panes = if self.split_visible { 0..2 } else { 0..1 };
        for pane in panes.filter(|&pane| self.tab_for_pane(pane) == index) {
            let area = RECT {
                left: self.pane_left(hwnd, pane),
                right: self.pane_right(hwnd, pane),
                ..self.editor_area(hwnd, false)
            };
            unsafe { InvalidateRect(hwnd, &area, 0) };
        }
    }

    /// The hints on `line` of tab `tab`; none when they're turned off.
    pub(super) fn hints_on_line(&self, tab: usize, line: usize) -> &[InlayHintAt] {
        if !self.settings.inlay_hints {
            return &[];
        }
        let hints = &self.tabs[tab].inlay_hints;
        let from = hints.partition_point(|hint| hint.line < line);
        let to = hints.partition_point(|hint| hint.line <= line);
        &hints[from..to]
    }

    /// How far the hints of a screen row starting at `row_start` push `byte`
    /// along it: the widths of those before it, and with `at` those at it
    /// too (for drawing the text from `byte`, which goes after them).
    pub(super) fn hint_shift(
        &self,
        hdc: HDC,
        hints: &[InlayHintAt],
        row_start: usize,
        byte: usize,
        at: bool,
    ) -> i32 {
        hints
            .iter()
            .filter(|hint| {
                hint.byte >= row_start && (hint.byte < byte || (at && hint.byte == byte))
            })
            .map(|hint| self.text_width(hdc, &hint.label))
            .sum()
    }
}

/// Moves hints along with an edit: those after it go with their text, those
/// inside what it replaced go.
pub(super) fn shift_inlay_hints(hints: &mut Vec<InlayHintAt>, change: &TextChange) {
    if hints.is_empty() {
        return;
    }
    let (start, end, new_end) = (change.start, change.end, change.new_end());
    hints.retain_mut(|hint| {
        let at = Pos {
            line: hint.line,
            byte: hint.byte,
        };
        if at < start || (at == start && start != end) {
            true
        } else if at >= end {
            if hint.line == end.line {
                hint.line = new_end.line;
                hint.byte = new_end.byte + (at.byte - end.byte);
            } else {
                hint.line = hint.line - end.line + new_end.line;
            }
            true
        } else {
            false
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(line: usize, byte: usize) -> InlayHintAt {
        InlayHintAt {
            line,
            byte,
            label: ": i32".into(),
        }
    }

    fn change(start: (usize, usize), end: (usize, usize), text: &str) -> TextChange {
        TextChange {
            serial: 1,
            start: Pos {
                line: start.0,
                byte: start.1,
            },
            end: Pos {
                line: end.0,
                byte: end.1,
            },
            start_utf16: start.1,
            end_utf16: end.1,
            text: text.into(),
        }
    }

    #[test]
    fn hints_move_with_their_text() {
        let places = |hints: &[InlayHintAt]| -> Vec<(usize, usize)> {
            hints.iter().map(|hint| (hint.line, hint.byte)).collect()
        };
        // Typing before a hint on its line moves it; one at the typing
        // place goes after what's typed; earlier ones stay.
        let mut hints = vec![hint(0, 2), hint(1, 5), hint(1, 9), hint(4, 1)];
        shift_inlay_hints(&mut hints, &change((1, 5), (1, 5), "ab"));
        assert_eq!(places(&hints), [(0, 2), (1, 7), (1, 11), (4, 1)]);
        // A new line moves later lines down, and the rest of its line.
        shift_inlay_hints(&mut hints, &change((1, 3), (1, 3), "\n"));
        assert_eq!(places(&hints), [(0, 2), (2, 4), (2, 8), (5, 1)]);
        // Deleting text with a hint inside drops it; one at the deletion's
        // start stays.
        let mut hints = vec![hint(0, 2), hint(0, 4), hint(0, 8)];
        shift_inlay_hints(&mut hints, &change((0, 2), (0, 6), ""));
        assert_eq!(places(&hints), [(0, 2), (0, 4)]);
    }
}
