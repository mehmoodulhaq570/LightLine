// Multiple cursors. Alt+Click adds a caret, Ctrl+Alt+Up/Down one on the
// line above or below, and Ctrl+D the next match of the selection (or of the
// word at the caret, which it selects first). Typing, Backspace, Delete,
// Enter, paste, cut and copy then act at every caret, as one undo step;
// arrows, Home and End move them all. Esc, or a click, goes back to one.
//
// The main caret stays in `EditorView::cursor`, so everything that knows
// only one caret keeps working; the others are in `EditorView::extra`. An
// edit made any other way (formatting, undo, a rename) drops the extras.

use super::app::{Caret, remap_position};
use super::input::newline_text;
use super::*;

// What an edit at every caret does there.
enum Op<'a> {
    Insert(&'a str),
    // One text per caret, in document order: pasting as many lines as there
    // are carets puts one at each.
    InsertEach(Vec<String>),
    Newline,
    Backspace { word: bool },
    Delete { word: bool },
}

impl App {
    pub(super) fn has_extra_carets(&self) -> bool {
        !self.view().extra.is_empty()
    }

    /// Back to one caret, in every view of the active file; true when there
    /// were more.
    pub(super) fn drop_extra_carets(&mut self) -> bool {
        let mut had = false;
        for view in &mut self.tabs[self.active].views {
            had |= !view.extra.is_empty();
            view.extra.clear();
        }
        if had {
            unsafe { InvalidateRect(self.hwnd, null(), 0) };
        }
        had
    }

    // Every caret, the main one first.
    fn carets(&self) -> Vec<Caret> {
        let view = self.view();
        std::iter::once(Caret {
            cursor: view.cursor,
            anchor: view.selection_anchor,
        })
        .chain(view.extra.iter().copied())
        .collect()
    }

    // Sets the carets, `carets[main]` the main one. Carets on the same
    // place, or with overlapping selections, become one.
    fn set_carets(&mut self, carets: Vec<Caret>, main: usize) {
        let main_caret = carets[main];
        let mut sorted = carets;
        sorted.sort_by_key(|caret| range_of(*caret).0);
        let mut merged: Vec<Caret> = Vec::new();
        for caret in sorted {
            if let Some(last) = merged.last_mut() {
                let (_, last_end) = range_of(*last);
                let (start, end) = range_of(caret);
                if start < last_end
                    || (start == last_end && caret.anchor.is_none())
                    || start == range_of(*last).0
                {
                    // The union of both, keeping the later one's direction.
                    let (last_start, _) = range_of(*last);
                    let union_end = end.max(last_end);
                    *last = if last.anchor.is_none() && caret.anchor.is_none() {
                        Caret {
                            cursor: union_end,
                            anchor: None,
                        }
                    } else {
                        Caret {
                            cursor: union_end,
                            anchor: Some(last_start),
                        }
                    };
                    continue;
                }
            }
            merged.push(caret);
        }
        let main_index = merged
            .iter()
            .position(|caret| {
                let (start, end) = range_of(*caret);
                let (main_start, main_end) = range_of(main_caret);
                start <= main_start && main_end <= end
            })
            .unwrap_or(0);
        let main = merged.remove(main_index);
        let doc_end = self.doc().end();
        let clamp = |pos: Pos| if pos > doc_end { doc_end } else { pos };
        let view = self.view_mut();
        view.cursor = clamp(main.cursor);
        view.selection_anchor = main
            .anchor
            .map(clamp)
            .filter(|anchor| *anchor != view.cursor);
        view.extra = merged
            .into_iter()
            .map(|caret| Caret {
                cursor: clamp(caret.cursor),
                anchor: caret
                    .anchor
                    .map(clamp)
                    .filter(|anchor| *anchor != caret.cursor),
            })
            .collect();
    }

    /// Alt+Click: a caret at `pos`, or, on an extra caret, removes it.
    pub(super) fn toggle_caret_at(&mut self, hwnd: HWND, pos: Pos) {
        let pos = self.doc().grapheme_position(pos);
        let mut carets = self.carets();
        if let Some(index) = carets
            .iter()
            .position(|caret| caret.cursor == pos && caret.anchor.is_none())
        {
            if carets.len() > 1 {
                carets.remove(index);
                self.set_carets(carets, 0);
            }
        } else {
            carets.push(Caret {
                cursor: pos,
                anchor: None,
            });
            let main = carets.len() - 1;
            self.set_carets(carets, main);
        }
        self.after_caret_change(hwnd);
    }

    /// Ctrl+Alt+Up/Down: a caret on the line above the topmost caret, or
    /// below the bottommost, at the same column where the line is long
    /// enough.
    pub(super) fn add_caret_vertically(&mut self, hwnd: HWND, down: bool) {
        let mut carets = self.carets();
        let edge = if down {
            carets.iter().map(|caret| caret.cursor).max()
        } else {
            carets.iter().map(|caret| caret.cursor).min()
        };
        let Some(edge) = edge else {
            return;
        };
        let line = if down {
            edge.line + 1
        } else {
            match edge.line.checked_sub(1) {
                Some(line) => line,
                None => return,
            }
        };
        if line >= self.doc().line_count() {
            return;
        }
        // The same column in characters, not bytes, so it lines up.
        let doc = self.doc();
        let column = doc.line(edge.line)[..edge.byte].chars().count();
        let text = doc.line(line);
        let byte = text
            .char_indices()
            .nth(column)
            .map_or(text.len(), |(byte, _)| byte);
        carets.push(Caret {
            cursor: doc.grapheme_position(Pos { line, byte }),
            anchor: None,
        });
        let main = carets.len() - 1;
        self.set_carets(carets, main);
        self.after_caret_change(hwnd);
    }

    /// Ctrl+D: selects the word at the caret, or, with a selection, adds
    /// a caret selecting its next match after the last one, wrapping round.
    pub(super) fn add_next_match(&mut self, hwnd: HWND) {
        let doc = self.doc();
        let view = self.view();
        let Some((start, end)) = self.selection_range() else {
            // The word at the caret.
            let line = doc.line(view.cursor.line);
            let is_word = |c: char| c.is_alphanumeric() || c == '_';
            let at = view.cursor.byte.min(line.len());
            let from = line[..at]
                .char_indices()
                .rev()
                .take_while(|(_, c)| is_word(*c))
                .last()
                .map_or(at, |(index, _)| index);
            let to = line[at..]
                .char_indices()
                .find(|(_, c)| !is_word(*c))
                .map_or(line.len(), |(index, _)| at + index);
            if from < to {
                let line_number = view.cursor.line;
                let view = self.view_mut();
                view.selection_anchor = Some(Pos {
                    line: line_number,
                    byte: from,
                });
                view.cursor = Pos {
                    line: line_number,
                    byte: to,
                };
                self.after_caret_change(hwnd);
            }
            return;
        };
        if start.line != end.line {
            self.status = "Ctrl+D adds matches of a selection within one line".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let needle = doc.line(start.line)[start.byte..end.byte].to_owned();
        let mut carets = self.carets();
        let last = carets
            .iter()
            .map(|caret| range_of(*caret).1)
            .max()
            .unwrap_or(end);
        let taken = |at: Pos| carets.iter().any(|caret| range_of(*caret).0 == at);
        let mut found = doc.find_forward(last, &needle);
        // find_forward wraps round; one already selected means all are.
        if found.is_some_and(taken) {
            found = None;
        }
        let Some(at) = found else {
            self.status = format!("No more matches of {needle}");
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        carets.push(Caret {
            anchor: Some(at),
            cursor: Pos {
                line: at.line,
                byte: at.byte + needle.len(),
            },
        });
        let main = carets.len() - 1;
        self.set_carets(carets, main);
        self.status = format!("{} carets", self.carets().len());
        self.after_caret_change(hwnd);
    }

    // Shows the main caret and redraws everything: carets elsewhere on
    // screen aren't covered by the caret-only repaint.
    fn after_caret_change(&mut self, hwnd: HWND) {
        self.caret_on = true;
        self.keep_cursor_visible(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Keys while there are several carets. True when handled here; false
    /// lets the key go on (dropping the extra carets first when it would
    /// only make sense for one).
    pub(super) fn multi_cursor_key(
        &mut self,
        hwnd: HWND,
        key: u32,
        ctrl: bool,
        shift: bool,
    ) -> bool {
        if !self.has_extra_carets() || self.tab().read_only() {
            return false;
        }
        let doc_move = |app: &App, caret: Caret, key: u32| -> Option<Pos> {
            let doc = app.doc();
            let at = caret.cursor;
            let selected = caret.anchor.filter(|anchor| *anchor != at);
            Some(match key {
                // Without Shift, Left/Right first collapse a selection.
                x if x == VK_LEFT as u32 && !shift && !ctrl && selected.is_some() => {
                    range_of(caret).0
                }
                x if x == VK_RIGHT as u32 && !shift && !ctrl && selected.is_some() => {
                    range_of(caret).1
                }
                x if x == VK_LEFT as u32 && ctrl => doc.previous_word(at),
                x if x == VK_RIGHT as u32 && ctrl => doc.next_word(at),
                x if x == VK_LEFT as u32 => doc.previous(at),
                x if x == VK_RIGHT as u32 => doc.next(at),
                x if x == VK_UP as u32 || x == VK_DOWN as u32 => {
                    let line = if x == VK_UP as u32 {
                        at.line.checked_sub(1)?
                    } else if at.line + 1 < doc.line_count() {
                        at.line + 1
                    } else {
                        return Some(at);
                    };
                    let column = doc.line(at.line)[..at.byte].chars().count();
                    let text = doc.line(line);
                    let byte = text
                        .char_indices()
                        .nth(column)
                        .map_or(text.len(), |(byte, _)| byte);
                    doc.grapheme_position(Pos { line, byte })
                }
                x if x == VK_HOME as u32 => {
                    // The first non-blank, or the line start from there.
                    let text = doc.line(at.line);
                    let indent = text.len() - text.trim_start().len();
                    Pos {
                        line: at.line,
                        byte: if at.byte == indent { 0 } else { indent },
                    }
                }
                x if x == VK_END as u32 => Pos {
                    line: at.line,
                    byte: doc.line(at.line).len(),
                },
                _ => return None,
            })
        };
        match key {
            x if x == VK_ESCAPE as u32 => {
                self.drop_extra_carets();
                true
            }
            x if (x == VK_LEFT as u32
                || x == VK_RIGHT as u32
                || x == VK_UP as u32
                || x == VK_DOWN as u32
                || x == VK_HOME as u32
                || x == VK_END as u32)
                && !(ctrl
                    && (x == VK_UP as u32
                        || x == VK_DOWN as u32
                        || x == VK_HOME as u32
                        || x == VK_END as u32)) =>
            {
                let carets: Vec<Caret> = self
                    .carets()
                    .into_iter()
                    .map(|caret| {
                        let moved = doc_move(self, caret, key).unwrap_or(caret.cursor);
                        Caret {
                            cursor: moved,
                            anchor: if shift {
                                Some(caret.anchor.unwrap_or(caret.cursor))
                            } else {
                                None
                            },
                        }
                    })
                    .collect();
                self.set_carets(carets, 0);
                self.after_caret_change(hwnd);
                true
            }
            x if x == VK_BACK as u32 => {
                self.edit_at_carets(hwnd, Op::Backspace { word: ctrl });
                true
            }
            x if x == VK_DELETE as u32 => {
                self.edit_at_carets(hwnd, Op::Delete { word: ctrl });
                true
            }
            0x43 if ctrl => {
                self.copy_carets(hwnd);
                true
            }
            0x58 if ctrl => {
                if self.copy_carets(hwnd) {
                    self.edit_at_carets(hwnd, Op::Delete { word: false });
                }
                true
            }
            0x56 if ctrl => {
                match clipboard::paste(hwnd) {
                    Ok(Some(text)) => {
                        let text = text.replace("\r\n", "\n");
                        let count = self.carets().len();
                        let lines: Vec<String> = text
                            .trim_end_matches('\n')
                            .split('\n')
                            .map(str::to_owned)
                            .collect();
                        if lines.len() == count {
                            self.edit_at_carets(hwnd, Op::InsertEach(lines));
                        } else {
                            self.edit_at_carets(hwnd, Op::Insert(&text));
                        }
                    }
                    Ok(None) => {}
                    Err(error) => self.error(hwnd, &error),
                }
                true
            }
            // Page moves and select-all are for one caret.
            x if x == VK_PRIOR as u32 || x == VK_NEXT as u32 || (ctrl && x == 0x41) => {
                self.drop_extra_carets();
                false
            }
            // Everything else goes on: saving, Ctrl+D and the like keep the
            // carets, and whatever moves the caret or edits the text some
            // other way drops the extra ones itself (move_cursor,
            // replace_range, undo).
            _ => false,
        }
    }

    /// A typed character while there are several carets; true when taken.
    pub(super) fn multi_cursor_char(&mut self, hwnd: HWND, ch: char) -> bool {
        if !self.has_extra_carets() || self.tab().read_only() {
            return false;
        }
        match ch {
            '\r' | '\n' => self.edit_at_carets(hwnd, Op::Newline),
            '\t' => {
                let unit = if self.settings.insert_spaces {
                    " ".repeat(self.settings.tab_size)
                } else {
                    "\t".into()
                };
                self.edit_at_carets(hwnd, Op::Insert(&unit));
            }
            ch if ch.is_control() => return true,
            ch => self.edit_at_carets(hwnd, Op::Insert(ch.encode_utf8(&mut [0; 4]))),
        }
        true
    }

    // Copies each caret's selection, in document order, one per line; with
    // none selected, nothing. True when something was copied.
    fn copy_carets(&mut self, hwnd: HWND) -> bool {
        let mut carets = self.carets();
        carets.sort_by_key(|caret| range_of(*caret).0);
        let parts: Vec<String> = carets
            .iter()
            .filter(|caret| caret.anchor.is_some_and(|anchor| anchor != caret.cursor))
            .map(|caret| {
                let (start, end) = range_of(*caret);
                self.doc().text_range(start, end)
            })
            .collect();
        if parts.is_empty() {
            return false;
        }
        let _ = clipboard::copy(hwnd, &parts.join("\r\n"));
        true
    }

    // Makes `op` at every caret, as one undo step, from the last caret
    // back so earlier ones keep their places; each caret ends after its
    // edit.
    fn edit_at_carets(&mut self, hwnd: HWND, op: Op) {
        let carets = self.carets();
        let mut order: Vec<usize> = (0..carets.len()).collect();
        order.sort_by_key(|&index| range_of(carets[index]).0);
        // Where each caret's edit is, worked out on the text before any.
        let unit = if self.settings.insert_spaces {
            " ".repeat(self.settings.tab_size)
        } else {
            "\t".to_string()
        };
        let doc = self.doc();
        let edits: Vec<(Pos, Pos, String)> = order
            .iter()
            .enumerate()
            .map(|(rank, &index)| {
                let caret = carets[index];
                let (start, end) = range_of(caret);
                let selected = start != end;
                match &op {
                    Op::Insert(text) => (start, end, (*text).to_owned()),
                    Op::InsertEach(texts) => (start, end, texts[rank].clone()),
                    Op::Newline => (
                        start,
                        end,
                        newline_text(
                            &doc.line(start.line)[..start.byte],
                            self.settings.auto_indent,
                            &unit,
                        ),
                    ),
                    Op::Backspace { .. } | Op::Delete { .. } if selected => {
                        (start, end, String::new())
                    }
                    Op::Backspace { word: true } => {
                        (doc.previous_word(start), start, String::new())
                    }
                    Op::Backspace { word: false } => (doc.previous(start), start, String::new()),
                    Op::Delete { word: true } => (start, doc.next_word(start), String::new()),
                    Op::Delete { word: false } => (start, doc.next(start), String::new()),
                }
            })
            .collect();
        let mut placed: Vec<(usize, Pos)> = Vec::new();
        self.multi_editing = true;
        self.doc_mut().begin_group();
        // Later carets first, so earlier carets' places stay valid; the
        // carets already placed (later in the text) move with each edit.
        let mut previous_start: Option<Pos> = None;
        for (rank, &index) in order.iter().enumerate().rev() {
            let (start, mut end, text) = edits[rank].clone();
            // Deleting toward a caret right behind: stop at it, so two
            // carets' edits never overlap.
            if let Some(next) = previous_start
                && end > next
            {
                end = next;
            }
            let start = start.min(end);
            self.replace_range(start, end, &text);
            let inserted = self.view().cursor;
            for (_, pos) in &mut placed {
                *pos = remap_position(*pos, start, end, inserted);
            }
            placed.push((index, inserted));
            previous_start = Some(start);
        }
        self.doc_mut().end_group();
        self.multi_editing = false;
        let mut new_carets = vec![
            Caret {
                cursor: Pos::default(),
                anchor: None,
            };
            carets.len()
        ];
        for (index, pos) in placed {
            new_carets[index] = Caret {
                cursor: pos,
                anchor: None,
            };
        }
        self.set_carets(new_carets, 0);
        self.after_caret_change(hwnd);
    }
}

// A caret's selection as (start, end), or its place twice.
fn range_of(caret: Caret) -> (Pos, Pos) {
    match caret.anchor {
        Some(anchor) if anchor < caret.cursor => (anchor, caret.cursor),
        Some(anchor) => (caret.cursor, anchor),
        None => (caret.cursor, caret.cursor),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_run_start_to_end_whichever_way_selected() {
        let at = |byte| Pos { line: 0, byte };
        let caret = |cursor, anchor| Caret { cursor, anchor };
        assert_eq!(range_of(caret(at(5), Some(at(2)))), (at(2), at(5)));
        assert_eq!(range_of(caret(at(2), Some(at(5)))), (at(2), at(5)));
        assert_eq!(range_of(caret(at(3), None)), (at(3), at(3)));
    }
}
