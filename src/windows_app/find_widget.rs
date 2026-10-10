// The find / replace box floating at the top-right of the editor (Ctrl+F,
// Ctrl+H). Painting and clicks both use `find_layout`, so the box can't be
// drawn somewhere clicks don't follow.
//
// It used to be a "Find: ..." prompt in the status bar, which faded after a
// few seconds while typing still went into it: pressing Ctrl+F looked like
// nothing happened (#55).

use super::*;

const WIDTH: i32 = 400;
const MIN_WIDTH: i32 = 260;
const ROW: i32 = 30;
const INSET: i32 = 6;
const ROW_GAP: i32 = 4;
const SIDE_WIDTH: i32 = 96;

pub(super) struct FindLayout {
    card: RECT,
    find: RECT,
    replace: Option<RECT>,
    // Right of each field: the match count, then the Replace All hint.
    count: RECT,
    hint: Option<RECT>,
}

impl App {
    /// Opens the find box, with the replace row when `replace` is set. The
    /// query starts from the selected text when it's on one line, and shows
    /// as selected so typing replaces it.
    pub(super) fn open_find(&mut self, hwnd: HWND, replace: bool) {
        if self.welcome || self.tab().is_placeholder() || self.tab().read_only() {
            return;
        }
        let selected = self
            .selection_range()
            .filter(|(start, end)| start.line == end.line)
            .map(|(start, end)| self.doc().text_range(start, end));
        match selected {
            Some(text) => self.find_query = text,
            None if !self.find_mode => self.find_query.clear(),
            None => {}
        }
        if replace && !self.replace_mode {
            self.replace_query.clear();
        }
        self.search_input = false;
        self.panel_focus = false;
        self.find_mode = true;
        self.replace_mode = replace;
        self.replace_field = 0;
        self.find_text_selected = !self.find_query.is_empty();
        self.find_origin = self
            .selection_range()
            .map_or(self.view().cursor, |(start, _)| start);
        self.update_find_count();
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn close_find(&mut self, hwnd: HWND) {
        self.find_mode = false;
        self.replace_mode = false;
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Types `text` into the focused field of the find box; `None` is
    /// Backspace. Selected find text is replaced as a whole.
    pub(super) fn find_box_input(&mut self, hwnd: HWND, text: Option<&str>) {
        if self.replace_mode && self.replace_field == 1 {
            match text {
                Some(text) => self.replace_query.push_str(text),
                None => {
                    self.replace_query.pop();
                }
            }
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        if std::mem::take(&mut self.find_text_selected) {
            self.find_query.clear();
        } else if text.is_none() {
            self.find_query.pop();
        }
        if let Some(text) = text {
            self.find_query.push_str(text);
        }
        self.find_query_changed(hwnd);
    }

    /// After the query changes, selects its first match from where the search
    /// started, so matches show while typing.
    fn find_query_changed(&mut self, hwnd: HWND) {
        let origin = self.find_origin;
        match self.doc().find_forward(origin, &self.find_query) {
            Some(start) => {
                let end = Pos {
                    line: start.line,
                    byte: start.byte + self.find_query.len(),
                };
                self.view_mut().selection_anchor = Some(start);
                self.view_mut().cursor = end;
            }
            None => {
                self.view_mut().selection_anchor = None;
                self.view_mut().cursor = origin;
            }
        }
        self.update_find_count();
        self.refresh(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn update_find_count(&mut self) {
        let query = self.find_query.as_str();
        // The selection is the current match when it is exactly one match long.
        let current = self
            .selection_range()
            .filter(|(start, end)| start.line == end.line && end.byte - start.byte == query.len())
            .map(|(start, _)| start);
        let doc = self.doc();
        let lines = (0..doc.line_count()).map(|line| doc.line(line));
        self.find_count = count_matches(lines, query, current);
    }

    pub(super) fn find_layout(&self, hwnd: HWND) -> Option<FindLayout> {
        if !self.find_mode || self.welcome || self.tab().is_placeholder() || self.tab().read_only()
        {
            return None;
        }
        let s = |value: i32| self.scale(value);
        let pane = self.focused_pane;
        let left = self.pane_left(hwnd, pane) + s(GUTTER);
        let right = self.pane_right(hwnd, pane) - s(20);
        // A bit over half the text area, so matches to the left of the box
        // stay visible even in a narrow pane.
        let width = ((right - left) * 11 / 20)
            .clamp(s(MIN_WIDTH), s(WIDTH))
            .min(right - left);
        if width < s(MIN_WIDTH) {
            return None;
        }
        let rows = if self.replace_mode { 2 } else { 1 };
        let top = self.editor_top();
        let card = RECT {
            left: right - width,
            top,
            right,
            bottom: top + s(INSET) * 2 + s(ROW) * rows + s(ROW_GAP) * (rows - 1),
        };
        let field_right = card.right - s(INSET) - s(SIDE_WIDTH);
        let row = |index: i32| {
            let top = card.top + s(INSET) + (s(ROW) + s(ROW_GAP)) * index;
            (
                RECT {
                    left: card.left + s(INSET),
                    top,
                    right: field_right,
                    bottom: top + s(ROW),
                },
                RECT {
                    left: field_right + s(10),
                    top,
                    right: card.right - s(INSET),
                    bottom: top + s(ROW),
                },
            )
        };
        let (find, count) = row(0);
        let (replace, hint) = if self.replace_mode {
            let (field, hint) = row(1);
            (Some(field), Some(hint))
        } else {
            (None, None)
        };
        Some(FindLayout {
            card,
            find,
            replace,
            count,
            hint,
        })
    }

    /// Focuses the field under a click on the find box. True when the click
    /// was on the box, so the editor underneath doesn't take it too.
    pub(super) fn find_click(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some(layout) = self.find_layout(hwnd) else {
            return false;
        };
        if !contains(layout.card, x, y) {
            return false;
        }
        if layout.replace.is_some_and(|field| contains(field, x, y)) {
            self.replace_field = 1;
        } else if contains(layout.find, x, y) {
            self.replace_field = 0;
            self.find_text_selected = false;
        }
        self.panel_focus = false;
        self.terminal_focus = false;
        self.search_input = false;
        unsafe { InvalidateRect(hwnd, null(), 0) };
        true
    }

    pub(super) fn paint_find_widget(&self, hdc: HDC, hwnd: HWND) {
        let Some(layout) = self.find_layout(hwnd) else {
            return;
        };
        let s = |value: i32| self.scale(value);
        let card = layout.card;
        let shadow = RECT {
            left: card.left + s(4),
            top: card.top + s(5),
            right: card.right + s(4),
            bottom: card.bottom + s(5),
        };
        Self::rounded_fill(hdc, shadow, s(9), ui(3, 8, 18));
        self.panel_card(hdc, card, s(8), self.theme.edge, self.theme.sidebar_bg);
        unsafe { SelectObject(hdc, self.ui_font) };

        self.paint_find_field(
            hdc,
            layout.find,
            &self.find_query,
            "Find",
            self.replace_field == 0,
            self.find_text_selected,
        );
        let (current, total) = self.find_count;
        let (count, color) = match (current, total) {
            _ if self.find_query.is_empty() => (String::new(), self.theme.muted),
            (_, 0) => ("No results".to_string(), self.theme.pink),
            (0, 1) => ("1 result".to_string(), self.theme.muted),
            (0, total) => (format!("{total} results"), self.theme.muted),
            (current, total) => (format!("{current} of {total}"), self.theme.muted),
        };
        let middle = (layout.count.top + layout.count.bottom) / 2;
        self.label_mid(hdc, &count, layout.count.left, middle, color, layout.count);

        if let (Some(field), Some(hint)) = (layout.replace, layout.hint) {
            self.paint_find_field(
                hdc,
                field,
                &self.replace_query,
                "Replace",
                self.replace_field == 1,
                false,
            );
            let middle = (hint.top + hint.bottom) / 2;
            self.label_mid(
                hdc,
                "Alt+Enter: all",
                hint.left,
                middle,
                self.theme.muted,
                hint,
            );
        }
    }

    // One input: its text (the end of it, when too long to fit) or a
    // placeholder, and a caret when it has the focus.
    pub(super) fn paint_find_field(
        &self,
        hdc: HDC,
        rect: RECT,
        text: &str,
        placeholder: &str,
        focused: bool,
        selected: bool,
    ) {
        let s = |value: i32| self.scale(value);
        let edge = if focused {
            self.theme.sky
        } else {
            self.theme.edge
        };
        self.panel_card(hdc, rect, s(6), edge, self.theme.editor_bg);
        let inner = RECT {
            left: rect.left + s(9),
            top: rect.top,
            right: rect.right - s(9),
            bottom: rect.bottom,
        };
        let middle = (rect.top + rect.bottom) / 2;
        let shown = fitting_tail(text, (inner.right - inner.left - s(3)).max(0), |part| {
            self.text_width(hdc, part)
        });
        if selected && !shown.is_empty() {
            let half = self.text_height(hdc) / 2 + s(1);
            Self::fill(
                hdc,
                RECT {
                    left: inner.left - s(1),
                    top: middle - half,
                    right: inner.left + self.text_width(hdc, shown) + s(1),
                    bottom: middle + half,
                },
                self.theme.select_bg,
            );
        }
        if shown.is_empty() {
            // After the caret, so the caret doesn't cover its first letter.
            let left = inner.left + if focused { s(5) } else { 0 };
            self.label_mid(hdc, placeholder, left, middle, self.theme.muted, inner);
        } else {
            self.label_mid(hdc, shown, inner.left, middle, self.theme.text, inner);
        }
        if focused {
            let x = inner.left + self.text_width(hdc, shown);
            let half = self.text_height(hdc) / 2;
            Self::fill(
                hdc,
                RECT {
                    left: x,
                    top: middle - half,
                    right: x + s(2).max(1),
                    bottom: middle + half,
                },
                self.theme.text,
            );
        }
    }
}

fn contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

// (current, total): every non-overlapping match of `query` in `lines`, and
// which of them (1-based) starts at `current`, or 0 when none does.
fn count_matches<'a>(
    lines: impl Iterator<Item = &'a str>,
    query: &str,
    current: Option<Pos>,
) -> (usize, usize) {
    if query.is_empty() {
        return (0, 0);
    }
    let (mut index, mut total) = (0, 0);
    for (line, text) in lines.enumerate() {
        for (byte, _) in text.match_indices(query) {
            total += 1;
            if current == Some(Pos { line, byte }) {
                index = total;
            }
        }
    }
    (index, total)
}

// The longest end of `text` no wider than `width`, so the part being typed
// stays in view.
fn fitting_tail(text: &str, width: i32, measure: impl Fn(&str) -> i32) -> &str {
    let mut start = 0;
    while start < text.len() && measure(&text[start..]) > width {
        start += 1;
        while !text.is_char_boundary(start) {
            start += 1;
        }
    }
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_matches_finds_the_current_match() {
        let lines = ["foo bar foo", "", "foofoo"];
        let at = |line, byte| Some(Pos { line, byte });
        assert_eq!(count_matches(lines.into_iter(), "foo", None), (0, 4));
        assert_eq!(count_matches(lines.into_iter(), "foo", at(0, 8)), (2, 4));
        assert_eq!(count_matches(lines.into_iter(), "foo", at(2, 3)), (4, 4));
        assert_eq!(count_matches(lines.into_iter(), "foo", at(0, 1)), (0, 4));
        assert_eq!(count_matches(lines.into_iter(), "", at(0, 0)), (0, 0));
        assert_eq!(count_matches(lines.into_iter(), "baz", None), (0, 0));
    }

    #[test]
    fn fitting_tail_keeps_the_end_and_char_boundaries() {
        // One unit per byte, so the width is a byte budget.
        let measure = |part: &str| part.len() as i32;
        assert_eq!(fitting_tail("hello", 10, measure), "hello");
        assert_eq!(fitting_tail("hello", 3, measure), "llo");
        assert_eq!(fitting_tail("héllo", 4, measure), "llo");
        assert_eq!(fitting_tail("hello", 0, measure), "");
    }
}
