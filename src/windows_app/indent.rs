// Tab and Shift+Tab in the editor, as in VS Code: Tab types spaces up to the
// next tab stop (a tab character with insertSpaces off), and with several
// lines selected indents them; Shift+Tab takes one indent off the selected
// lines, or the caret's line. Each is one undo step. With several carets the
// typed tab character (multi_cursor.rs) does the work instead.

use super::*;

// The screen column `byte` of `line` starts at, tabs counting to their stop.
fn visual_column(line: &str, byte: usize, tab_size: usize) -> usize {
    line.get(..byte)
        .unwrap_or(line)
        .chars()
        .fold(0, |column, ch| {
            if ch == '\t' {
                column + tab_size - column % tab_size
            } else {
                column + 1
            }
        })
}

// How many bytes of indent Shift+Tab takes off `line`: a tab, or up to a tab
// stop's worth of spaces.
fn outdent_width(line: &str, tab_size: usize) -> usize {
    if line.starts_with('\t') {
        1
    } else {
        line.bytes()
            .take(tab_size)
            .take_while(|b| *b == b' ')
            .count()
    }
}

impl App {
    /// True when Tab (Shift+Tab with `shift`) was the editor's.
    pub(super) fn tab_key(&mut self, hwnd: HWND, shift: bool) -> bool {
        if !self.view().extra.is_empty() || self.tab().read_only() || self.tab().is_placeholder() {
            return false;
        }
        let tab_size = self.settings.tab_size.max(1);
        let selection = self.selection_range();
        let several_lines = selection.is_some_and(|(start, end)| start.line != end.line);
        if !shift && !several_lines {
            let at = selection.map_or(self.view().cursor, |(start, _)| start);
            let text = if self.settings.insert_spaces {
                let column = visual_column(self.doc().line(at.line), at.byte, tab_size);
                " ".repeat(tab_size - column % tab_size)
            } else {
                "\t".into()
            };
            let stays_in_editor = self.keystroke_stays_in_editor();
            let before = self.caret_frame(hwnd);
            self.replace_selection(&text);
            self.refresh_after_editor_input(hwnd, stays_in_editor, &before);
            return true;
        }
        let cursor = self.view().cursor;
        let anchor = self.view().selection_anchor;
        let (start, end) = selection.unwrap_or((cursor, cursor));
        // A selection ending at a line's start doesn't take that line.
        let last = if end.line > start.line && end.byte == 0 {
            end.line - 1
        } else {
            end.line
        };
        let unit = if self.settings.insert_spaces {
            " ".repeat(tab_size)
        } else {
            "\t".into()
        };
        // Per line, the bytes added (or taken off) at its start.
        let mut changed: Vec<(usize, isize)> = Vec::new();
        self.doc_mut().begin_group();
        for line in start.line..=last {
            let text = self.doc().line(line);
            let line_start = Pos { line, byte: 0 };
            if shift {
                let width = outdent_width(text, tab_size);
                if width > 0 {
                    self.replace_range(line_start, Pos { line, byte: width }, "");
                    changed.push((line, -(width as isize)));
                }
            } else if !text.is_empty() {
                self.replace_range(line_start, line_start, &unit);
                changed.push((line, unit.len() as isize));
            }
        }
        self.doc_mut().end_group();
        // The selection (or caret) stays on the same text; a point at a
        // line's start stays there, so whole lines stay selected.
        let moved = |at: Pos| match changed.iter().find(|(line, _)| *line == at.line) {
            Some(&(_, delta)) if at.byte > 0 => Pos {
                line: at.line,
                byte: (at.byte as isize + delta).max(0) as usize,
            },
            _ => at,
        };
        let view = self.view_mut();
        view.cursor = moved(cursor);
        view.selection_anchor = anchor.map(moved);
        self.keep_cursor_visible(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_stops_count_tabs_and_characters() {
        assert_eq!(visual_column("ab", 2, 4), 2);
        assert_eq!(visual_column("\tx", 2, 4), 5);
        assert_eq!(visual_column("a\tb", 2, 4), 4);
    }

    #[test]
    fn outdent_takes_one_indent_at_most() {
        assert_eq!(outdent_width("        x", 4), 4);
        assert_eq!(outdent_width("  x", 4), 2);
        assert_eq!(outdent_width("\t\tx", 4), 1);
        assert_eq!(outdent_width("x", 4), 0);
    }
}
