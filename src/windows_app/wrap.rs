//! Word wrap: how document lines break into screen rows, and the row-based
//! view model the editor paints, scrolls and hit-tests with.
//!
//! Only lines that are on screen (or between the view and the caret) are
//! ever wrapped, so wrapping costs nothing for the rest of a large file. The
//! scrollbar keeps counting document lines for the same reason.

use super::*;
use unicode_width::UnicodeWidthChar;

/// Where each screen row of one document line starts.
pub(super) struct LineRows {
    starts: Vec<usize>,
    // Columns that rows after the first are indented by, so a wrapped list
    // item or indented paragraph keeps its left edge (VS Code's default).
    indent_cols: usize,
}

impl LineRows {
    pub(super) fn count(&self) -> usize {
        self.starts.len()
    }

    pub(super) fn start(&self, row: usize) -> usize {
        self.starts[row.min(self.starts.len() - 1)]
    }

    /// Where `row` ends: the next row's start, or the end of the line.
    pub(super) fn end(&self, row: usize, line_len: usize) -> usize {
        self.starts.get(row + 1).copied().unwrap_or(line_len)
    }

    /// The row `byte` is on. A byte where a row starts belongs to that row.
    pub(super) fn row_of(&self, byte: usize) -> usize {
        self.starts
            .partition_point(|&start| start <= byte)
            .saturating_sub(1)
    }

    pub(super) fn is_last(&self, row: usize) -> bool {
        row + 1 >= self.starts.len()
    }

    /// The pixel indent of `row` (0 for the first row).
    pub(super) fn indent(&self, row: usize, char_width: i32) -> i32 {
        if row == 0 {
            0
        } else {
            self.indent_cols as i32 * char_width
        }
    }
}

fn char_columns(ch: char, tab_size: usize) -> usize {
    if ch == '\t' {
        tab_size
    } else {
        ch.width().unwrap_or(0).max(1)
    }
}

/// Breaks `text` into rows at most `columns` wide, preferring to break
/// after whitespace. With `columns` None (wrap off) the line is one row.
pub(super) fn layout_line(text: &str, columns: Option<usize>, tab_size: usize) -> LineRows {
    let single = LineRows {
        starts: vec![0],
        indent_cols: 0,
    };
    let Some(columns) = columns.filter(|columns| *columns > 0) else {
        return single;
    };
    let indent_cols: usize = text
        .chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .map(|ch| char_columns(ch, tab_size))
        .sum();
    // Too deep an indent would leave continuation rows no room.
    let indent_cols = if indent_cols * 2 > columns {
        0
    } else {
        indent_cols
    };
    let mut starts = vec![0];
    let mut row_start = 0;
    let mut capacity = columns;
    let mut used = 0;
    // The latest place a row may break (just after whitespace), and how many
    // columns of the row came before it.
    let mut break_at: Option<(usize, usize)> = None;
    for (byte, ch) in text.char_indices() {
        let width = char_columns(ch, tab_size);
        let space = ch == ' ' || ch == '\t';
        // Whitespace may hang past the edge; the next word moves instead.
        if !space && used + width > capacity && byte > row_start {
            match break_at.filter(|(at, _)| *at > row_start) {
                Some((at, before)) => {
                    row_start = at;
                    used -= before;
                }
                None => {
                    row_start = byte;
                    used = 0;
                }
            }
            starts.push(row_start);
            capacity = columns - indent_cols;
            break_at = None;
            // A word longer than a whole row is broken where it overflows.
            if used + width > capacity && byte > row_start {
                row_start = byte;
                used = 0;
                starts.push(row_start);
            }
        }
        used += width;
        if space {
            break_at = Some((byte + ch.len_utf8(), used));
        }
    }
    LineRows {
        starts,
        indent_cols,
    }
}

impl App {
    /// Whether the tab at `index` wraps long lines: its own choice (Alt+Z),
    /// else the `wordWrap` setting, on by default for Markdown.
    pub(super) fn wraps(&self, index: usize) -> bool {
        let tab = &self.tabs[index];
        if tab.read_only() {
            return false;
        }
        tab.word_wrap
            .unwrap_or_else(|| self.settings.word_wrap || Tab::is_markdown(&tab.document))
    }

    /// How many character columns fit in `pane`'s text area when its tab
    /// wraps; None when it doesn't.
    pub(super) fn wrap_columns(&self, hwnd: HWND, pane: usize) -> Option<usize> {
        if !self.wraps(self.tab_for_pane(pane)) {
            return None;
        }
        let text_left = self.pane_left(hwnd, pane) + self.scale(GUTTER + PAD);
        // Wrapped text stops short of the minimap and the scrollbar.
        let width = self.pane_right(hwnd, pane)
            - text_left
            - self.scale(PAD + SCROLLBAR)
            - self.minimap_width(hwnd, pane);
        Some((width / self.char_width.max(1)).max(8) as usize)
    }

    pub(super) fn line_rows(&self, hwnd: HWND, pane: usize, line: usize) -> LineRows {
        let doc = &self.tabs[self.tab_for_pane(pane)].document;
        layout_line(
            doc.line(line.min(doc.line_count() - 1)),
            self.wrap_columns(hwnd, pane),
            self.settings.tab_size,
        )
    }

    /// The view's top: the first document line shown and which of its rows
    /// is at the top, kept inside the document and outside folded text.
    pub(super) fn view_top(&self, hwnd: HWND, pane: usize) -> (usize, usize) {
        let view = self.view_for_pane(pane);
        let doc = &self.tabs[self.tab_for_pane(pane)].document;
        let line = doc
            .visible_line_for(view.first_line)
            .min(doc.line_count() - 1);
        let rows = self.line_rows(hwnd, pane, line).count();
        (line, view.first_row.min(rows - 1))
    }

    /// Moves `rows` screen rows down (positive) or up from `(line, row)`,
    /// through wrapped rows and over folded lines, stopping at either end.
    pub(super) fn step_rows(
        &self,
        hwnd: HWND,
        pane: usize,
        from: (usize, usize),
        rows: isize,
    ) -> (usize, usize) {
        let doc = &self.tabs[self.tab_for_pane(pane)].document;
        let columns = self.wrap_columns(hwnd, pane);
        let count =
            |line: usize| layout_line(doc.line(line), columns, self.settings.tab_size).count();
        let (mut line, mut row) = from;
        line = doc.visible_line_for(line);
        row = row.min(count(line) - 1);
        for _ in 0..rows.unsigned_abs() {
            if rows > 0 {
                if row + 1 < count(line) {
                    row += 1;
                } else {
                    let next = doc.next_visible_line(line);
                    if next == line {
                        break;
                    }
                    line = next;
                    row = 0;
                }
            } else if row > 0 {
                row -= 1;
            } else {
                let previous = doc.prev_visible_line(line);
                if previous == line {
                    break;
                }
                line = previous;
                row = count(line) - 1;
            }
        }
        (line, row)
    }

    /// Where `pos` appears in `pane`: its screen row counted from the top of
    /// the view, the byte that row starts at, and the row's pixel indent.
    /// None when it is above the view or more than `max_rows` rows below.
    pub(super) fn locate(
        &self,
        hwnd: HWND,
        pane: usize,
        pos: Pos,
        max_rows: usize,
    ) -> Option<(usize, usize, i32)> {
        let doc = &self.tabs[self.tab_for_pane(pane)].document;
        let columns = self.wrap_columns(hwnd, pane);
        let rows_of = |line: usize| layout_line(doc.line(line), columns, self.settings.tab_size);
        let (top_line, top_row) = self.view_top(hwnd, pane);
        let target = doc.visible_line_for(pos.line.min(doc.line_count() - 1));
        if target < top_line {
            return None;
        }
        // Inside a fold: shown on the fold's first row.
        let byte = if target == pos.line { pos.byte } else { 0 };
        let mut line = top_line;
        let mut screen_row = 0usize;
        loop {
            let rows = rows_of(line);
            let first = if line == top_line { top_row } else { 0 };
            if line == target {
                let row = rows.row_of(byte);
                if row < first {
                    return None;
                }
                let screen_row = screen_row + row - first;
                return (screen_row <= max_rows).then(|| {
                    (
                        screen_row,
                        rows.start(row),
                        rows.indent(row, self.char_width),
                    )
                });
            }
            screen_row += rows.count() - first;
            if screen_row > max_rows {
                return None;
            }
            let next = doc.next_visible_line(line);
            if next == line {
                return None;
            }
            line = next;
        }
    }

    /// The document line and wrapped row shown `screen_row` rows below the
    /// top of `pane`'s view; None past the end of the document.
    pub(super) fn at_screen_row(
        &self,
        hwnd: HWND,
        pane: usize,
        screen_row: usize,
    ) -> Option<(usize, usize)> {
        let doc = &self.tabs[self.tab_for_pane(pane)].document;
        let columns = self.wrap_columns(hwnd, pane);
        let count =
            |line: usize| layout_line(doc.line(line), columns, self.settings.tab_size).count();
        let (mut line, mut row) = self.view_top(hwnd, pane);
        let mut remaining = screen_row;
        loop {
            let rows = count(line);
            if row + remaining < rows {
                return Some((line, row + remaining));
            }
            remaining -= rows - row;
            let next = doc.next_visible_line(line);
            if next == line {
                return None;
            }
            line = next;
            row = 0;
        }
    }

    /// Puts `(line, row)` at the top of the focused pane's view.
    pub(super) fn set_view_top(&mut self, top: (usize, usize)) {
        let view = self.view_mut();
        view.first_line = top.0;
        view.first_row = top.1;
    }

    /// Scrolls `pane` by `rows` screen rows. The focus stays where it is.
    /// Returns how many rows it moved, less than `rows` at either end.
    pub(super) fn scroll_pane_rows(&mut self, hwnd: HWND, pane: usize, rows: isize) -> isize {
        let mut top = self.view_top(hwnd, pane);
        let mut moved = 0;
        while moved != rows {
            let next = self.step_rows(hwnd, pane, top, rows.signum());
            if next == top {
                break;
            }
            top = next;
            moved += rows.signum();
        }
        let tab = self.tab_for_pane(pane);
        let view = &mut self.tabs[tab].views[pane];
        view.first_line = top.0;
        view.first_row = top.1;
        moved
    }

    /// The byte in `text[start..end]` whose left edge is nearest `x` pixels
    /// from where that range is drawn. `end` itself is a candidate only on a
    /// line's last row: elsewhere it is the next row's first byte.
    pub(super) fn nearest_byte(
        &self,
        hdc: HDC,
        text: &str,
        (start, end): (usize, usize),
        end_is_candidate: bool,
        x: i32,
    ) -> usize {
        let mut bounds: Vec<usize> = text[start..end]
            .char_indices()
            .map(|(offset, _)| start + offset)
            .collect();
        if end_is_candidate || bounds.is_empty() {
            bounds.push(end);
        }
        let width = |byte: usize| self.text_width(hdc, &text[start..byte]);
        let (mut low, mut high) = (0, bounds.len());
        while low < high {
            let mid = (low + high) / 2;
            if width(bounds[mid]) < x {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        let right = low.min(bounds.len() - 1);
        let left = right.saturating_sub(1);
        if x - width(bounds[left]) <= width(bounds[right]) - x {
            bounds[left]
        } else {
            bounds[right]
        }
    }

    /// Where Up/Down (`rows` = ±1) or Page Up/Down lands the caret: the same
    /// horizontal position, `rows` screen rows away, so moving through a
    /// wrapped paragraph goes row by row as it does in VS Code.
    pub(super) fn cursor_moved_by_rows(&self, hwnd: HWND, rows: isize) -> Pos {
        let pane = self.focused_pane;
        let doc = self.doc();
        let cursor = doc.clamp(self.view().cursor);
        let line = doc.visible_line_for(cursor.line);
        let byte = if line == cursor.line { cursor.byte } else { 0 };
        let layout = self.line_rows(hwnd, pane, line);
        let row = layout.row_of(byte);
        let (target_line, target_row) = self.step_rows(hwnd, pane, (line, row), rows);
        if (target_line, target_row) == (line, row) {
            return cursor;
        }
        unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.font);
            let text = doc.line(line);
            let x = layout.indent(row, self.char_width)
                + self.text_width(hdc, &text[layout.start(row)..byte]);
            let target = self.line_rows(hwnd, pane, target_line);
            let target_text = doc.line(target_line);
            let range = (
                target.start(target_row),
                target.end(target_row, target_text.len()),
            );
            let byte = self.nearest_byte(
                hdc,
                target_text,
                range,
                target.is_last(target_row),
                x - target.indent(target_row, self.char_width),
            );
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            Pos {
                line: target_line,
                byte,
            }
        }
    }

    /// Alt+Z: turns word wrap on or off for the active tab.
    pub(super) fn toggle_word_wrap(&mut self, hwnd: HWND) {
        if self.tab().read_only() {
            return;
        }
        let wraps = !self.wraps(self.active);
        self.tab_mut().word_wrap = Some(wraps);
        for view in &mut self.tabs[self.active].views {
            view.first_row = 0;
        }
        self.status = if wraps {
            "Word wrap on (Alt+Z)".into()
        } else {
            "Word wrap off (Alt+Z)".into()
        };
        self.keep_cursor_visible(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str, columns: usize) -> Vec<&str> {
        let layout = layout_line(text, Some(columns), 4);
        (0..layout.count())
            .map(|row| &text[layout.start(row)..layout.end(row, text.len())])
            .collect()
    }

    #[test]
    fn short_lines_and_wrap_off_are_one_row() {
        assert_eq!(rows("short", 20), vec!["short"]);
        assert_eq!(rows("", 20), vec![""]);
        assert_eq!(layout_line("a b c d e f g", None, 4).count(), 1);
    }

    #[test]
    fn breaks_after_spaces() {
        assert_eq!(
            rows("the quick brown fox jumps", 10),
            vec!["the quick ", "brown fox ", "jumps"]
        );
    }

    #[test]
    fn long_words_break_where_they_overflow() {
        assert_eq!(
            rows("abcdefghijklmnop", 6),
            vec!["abcdef", "ghijkl", "mnop"]
        );
        assert_eq!(rows("go abcdefghij", 6), vec!["go ", "abcdef", "ghij"]);
    }

    #[test]
    fn continuation_rows_keep_the_indent() {
        let text = "  - one two three four five";
        let layout = layout_line(text, Some(12), 4);
        let parts: Vec<&str> = (0..layout.count())
            .map(|row| &text[layout.start(row)..layout.end(row, text.len())])
            .collect();
        // Rows after the first have two columns less, for the indent.
        assert_eq!(parts, vec!["  - one two ", "three four ", "five"]);
        assert_eq!(layout.indent(1, 8), 16);
        assert_eq!(layout.indent(0, 8), 0);
    }

    #[test]
    fn wide_characters_count_double_and_bytes_stay_on_boundaries() {
        // Each of these CJK characters takes two columns.
        let text = "漢字漢字漢字";
        let parts = rows(text, 5);
        assert_eq!(parts, vec!["漢字", "漢字", "漢字"]);
        let layout = layout_line(text, Some(5), 4);
        assert_eq!(layout.row_of(0), 0);
        assert_eq!(layout.row_of(6), 1);
        assert_eq!(layout.row_of(text.len()), 2);
    }
}
