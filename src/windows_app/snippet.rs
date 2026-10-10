// A completion's snippet once inserted: Tab and Shift+Tab go between its
// places to fill in ($1, $2, ... and the final $0), each selected so typing
// replaces it. The places move with every edit (replace_range). Reaching $0,
// Escape, undo, or a Tab with the caret outside the snippet ends it.

use super::*;

pub(super) struct SnippetSession {
    // The file (Document::id) it was inserted in.
    document: u64,
    // The places in Tab order, $0 last.
    stops: Vec<(Pos, Pos)>,
    current: usize,
}

// Where `offset` bytes into `text`, inserted at `start`, ends up.
fn position_in(start: Pos, text: &str, offset: usize) -> Pos {
    let before = &text[..offset];
    match before.rfind('\n') {
        None => Pos {
            line: start.line,
            byte: start.byte + offset,
        },
        Some(newline) => Pos {
            line: start.line + before.matches('\n').count(),
            byte: offset - newline - 1,
        },
    }
}

// Where `at` goes when `start..end` is replaced by text ending at `new_end`.
// A place's start at the edit's start stays there (typing into it), any
// other point inside the edit goes to its new end.
fn shift(at: Pos, start: Pos, end: Pos, new_end: Pos, is_start: bool) -> Pos {
    if at < start || (is_start && at == start) {
        at
    } else if at > end {
        if at.line == end.line {
            Pos {
                line: new_end.line,
                byte: new_end.byte + (at.byte - end.byte),
            }
        } else {
            Pos {
                line: at.line - end.line + new_end.line,
                byte: at.byte,
            }
        }
    } else {
        new_end
    }
}

impl App {
    /// Inserts a completion's snippet over `start..end` and selects its
    /// first place.
    pub(super) fn insert_snippet(&mut self, hwnd: HWND, start: Pos, end: Pos, snippet: &str) {
        let indent: String = self
            .doc()
            .line(start.line)
            .chars()
            .take_while(|ch| *ch == ' ' || *ch == '\t')
            .collect();
        let tab = if self.settings.insert_spaces {
            " ".repeat(self.settings.tab_size)
        } else {
            "\t".into()
        };
        let snippet = snippet.replace("\r\n", "\n");
        let (text, stops) = lsp::expand_snippet(&snippet, &indent, &tab);
        self.snippet = None;
        self.replace_range(start, end, &text);
        let stops: Vec<(Pos, Pos)> = stops
            .iter()
            .map(|&(_, from, to)| {
                (
                    position_in(start, &text, from),
                    position_in(start, &text, to),
                )
            })
            .collect();
        self.snippet = Some(SnippetSession {
            document: self.doc().id(),
            stops,
            current: 0,
        });
        self.select_snippet_place(hwnd);
    }

    // Selects the current place; at $0 the snippet is done.
    fn select_snippet_place(&mut self, hwnd: HWND) {
        let Some(session) = &self.snippet else {
            return;
        };
        let (from, to) = session.stops[session.current];
        if session.current + 1 == session.stops.len() {
            self.snippet = None;
        }
        self.move_cursor(from, false);
        if to != from {
            self.move_cursor(to, true);
        }
        self.keep_cursor_visible(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Tab and Shift+Tab move between the snippet's places; Escape ends it
    /// (and still does what it otherwise would). True when the key was the
    /// snippet's.
    pub(super) fn snippet_key(&mut self, hwnd: HWND, key: u32, shift: bool) -> bool {
        let Some(session) = &self.snippet else {
            return false;
        };
        if key == VK_ESCAPE as u32 || session.document != self.doc().id() {
            self.snippet = None;
            return false;
        }
        if key != VK_TAB as u32 {
            return false;
        }
        // The caret must still be within the snippet's places.
        let cursor = self.view().cursor;
        let first = session.stops.iter().map(|stop| stop.0).min();
        let last = session.stops.iter().map(|stop| stop.1).max();
        if first.is_none_or(|first| cursor < first) || last.is_none_or(|last| cursor > last) {
            self.snippet = None;
            return false;
        }
        let current = session.current;
        let next = if shift {
            current.saturating_sub(1)
        } else {
            current + 1
        };
        if let Some(session) = &mut self.snippet {
            session.current = next.min(session.stops.len() - 1);
        }
        self.select_snippet_place(hwnd);
        true
    }

    /// After `start..end` became text ending at `new_end` in the active file:
    /// the snippet's places move with it.
    pub(super) fn shift_snippet(&mut self, start: Pos, end: Pos, new_end: Pos) {
        let document = self.doc().id();
        let Some(session) = &mut self.snippet else {
            return;
        };
        if session.document != document {
            return;
        }
        for stop in &mut session.stops {
            let from = shift(stop.0, start, end, new_end, true);
            let to = shift(stop.1, start, end, new_end, false);
            *stop = (from, to.max(from));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: usize, byte: usize) -> Pos {
        Pos { line, byte }
    }

    #[test]
    fn places_are_found_across_lines() {
        let text = "if x {\n    \n}";
        assert_eq!(position_in(at(3, 4), text, 3), at(3, 7));
        assert_eq!(position_in(at(3, 4), text, 11), at(4, 4));
        assert_eq!(position_in(at(3, 4), text, 13), at(5, 1));
    }

    #[test]
    fn places_move_with_typing() {
        // add(left, right): "left" is 4..8, "right" 10..15 on line 0.
        let left = (at(0, 4), at(0, 8));
        let right = (at(0, 10), at(0, 15));
        // Typing "a" over the selected "left".
        let (start, end, new_end) = (at(0, 4), at(0, 8), at(0, 5));
        let moved = |stop: (Pos, Pos)| {
            (
                shift(stop.0, start, end, new_end, true),
                shift(stop.1, start, end, new_end, false),
            )
        };
        assert_eq!(moved(left), (at(0, 4), at(0, 5)));
        assert_eq!(moved(right), (at(0, 7), at(0, 12)));
        // A new line typed before the snippet moves it down.
        let (start, end, new_end) = (at(0, 0), at(0, 0), at(1, 0));
        assert_eq!(shift(at(0, 4), start, end, new_end, true), at(1, 4));
    }
}
