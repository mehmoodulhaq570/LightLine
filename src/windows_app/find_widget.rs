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

/// The timer that counts a big file's matches a slice at a time. Timer
/// messages come after keyboard input, so the find box keeps up with typing
/// while the count catches up.
pub(super) const FIND_COUNT_TIMER: usize = 24;
/// Files up to this many lines are counted at once: about 3 ms for 50,000
/// lines of code. Counting a 1M-line file took 59 ms per typed character.
const COUNT_AT_ONCE_LINES: usize = 50_000;
/// Lines counted per timer message.
const COUNT_SLICE_LINES: usize = 50_000;
/// In a big file, how many lines from the caret are searched for the first
/// match while typing; past that the count finds it.
const SEEK_AT_ONCE_LINES: usize = 50_000;

/// A count of the find box's matches still going through a big file.
pub(super) struct FindCounting {
    document: u64,
    serial: u64,
    matcher: Matcher,
    current: Option<(Pos, Pos)>,
    next_line: usize,
    counted: (usize, usize),
    // Where the find box's first match is still being looked for, and the
    // first matches (with their numbers) at or after it and before it.
    seek: Option<Pos>,
    before_seek: Option<((Pos, Pos), usize)>,
    at_seek: Option<((Pos, Pos), usize)>,
}

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
    pub(super) fn find_query_changed(&mut self, hwnd: HWND) {
        let origin = self.find_origin;
        let matcher = self.find_matcher().ok();
        // In a big file only the lines near the caret are searched now: a
        // query with no match there would read the whole file on each key.
        // The count, going through it a slice at a time, selects the first
        // match when it gets to it.
        let big = self.doc().line_count() > COUNT_AT_ONCE_LINES;
        let reach = if big { SEEK_AT_ONCE_LINES } else { usize::MAX };
        let found = matcher
            .as_ref()
            .and_then(|matcher| self.doc().find_next_within(origin, matcher, reach));
        let seek = (big && found.is_none() && matcher.is_some()).then_some(origin);
        match found {
            Some((start, end)) => {
                self.view_mut().selection_anchor = Some(start);
                self.view_mut().cursor = end;
            }
            None => {
                self.view_mut().selection_anchor = None;
                self.view_mut().cursor = origin;
            }
        }
        self.start_find_count(seek);
        self.refresh(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn update_find_count(&mut self) {
        self.start_find_count(None);
    }

    /// Counts the find box's matches: at once in a file of up to
    /// COUNT_AT_ONCE_LINES lines, else the first slice now and the rest on
    /// FIND_COUNT_TIMER. With `seek`, the first match from there (wrapping
    /// round) is selected when the count reaches it.
    fn start_find_count(&mut self, seek: Option<Pos>) {
        let matcher = match self.find_matcher() {
            Ok(matcher) => matcher,
            Err(error) => {
                self.find_error = (!self.find_query.is_empty()).then_some(error);
                self.find_count = (0, 0);
                return;
            }
        };
        self.find_error = None;
        // The selection is the current match when it is exactly one match.
        let current = self
            .selection_range()
            .filter(|(start, end)| start.line == end.line);
        let doc = &self.tabs[self.active].document;
        let mut counting = FindCounting {
            seek,
            ..FindCounting::new(doc, matcher, current)
        };
        let slice = if doc.line_count() <= COUNT_AT_ONCE_LINES {
            COUNT_AT_ONCE_LINES
        } else {
            COUNT_SLICE_LINES
        };
        counting.step(doc, slice);
        let done = counting.next_line >= doc.line_count();
        self.select_found(&mut counting, done);
        self.find_count = counting.counted;
        self.find_counting = (!done).then_some(counting);
        unsafe {
            if done {
                KillTimer(self.hwnd, FIND_COUNT_TIMER);
            } else {
                SetTimer(self.hwnd, FIND_COUNT_TIMER, 1, None);
            }
        }
    }

    /// FIND_COUNT_TIMER: counts the next slice of a big file. An edit, a
    /// switch to another file or a closed box starts it over or ends it.
    pub(super) fn find_count_timer(&mut self, hwnd: HWND) {
        let Some(mut counting) = self.find_counting.take() else {
            unsafe { KillTimer(hwnd, FIND_COUNT_TIMER) };
            return;
        };
        let doc = &self.tabs[self.active].document;
        if !self.find_mode
            || doc.id() != counting.document
            || doc.change_serial() != counting.serial
        {
            unsafe { KillTimer(hwnd, FIND_COUNT_TIMER) };
            if self.find_mode {
                self.update_find_count();
                self.invalidate_find_count(hwnd);
            }
            return;
        }
        counting.step(doc, COUNT_SLICE_LINES);
        let done = counting.next_line >= doc.line_count();
        if self.select_found(&mut counting, done) {
            self.refresh(hwnd);
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
        self.find_count = counting.counted;
        if done {
            unsafe { KillTimer(hwnd, FIND_COUNT_TIMER) };
        } else {
            self.find_counting = Some(counting);
        }
        self.invalidate_find_count(hwnd);
    }

    /// Selects the match a count was seeking once it's known: the first at
    /// or after the seek position, or, when the count is `done` without one,
    /// the first before it. Not if the caret has moved meanwhile. Returns
    /// whether the selection changed.
    fn select_found(&mut self, counting: &mut FindCounting, done: bool) -> bool {
        let Some(origin) = counting.seek else {
            return false;
        };
        let found = counting
            .at_seek
            .or(if done { counting.before_seek } else { None });
        let Some(((start, end), ordinal)) = found else {
            if done {
                counting.seek = None;
            }
            return false;
        };
        counting.seek = None;
        let view = self.view();
        if view.cursor != origin || view.selection_anchor.is_some() {
            return false;
        }
        self.view_mut().selection_anchor = Some(start);
        self.view_mut().cursor = end;
        counting.current = Some((start, end));
        counting.counted.0 = ordinal;
        true
    }

    /// Redraws the find box's count only.
    fn invalidate_find_count(&self, hwnd: HWND) {
        if let Some(layout) = self.find_layout(hwnd) {
            unsafe { InvalidateRect(hwnd, &layout.count, 0) };
        }
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
        if let Some(toggle) = self.option_toggle_at(layout.find, x, y) {
            self.flip_find_option(hwnd, toggle);
            return true;
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
            self.option_toggles_left(layout.find),
        );
        self.paint_option_toggles(hdc, layout.find, self.find_options);
        let (current, total) = self.find_count;
        // A big file still being counted shows what's found so far.
        let counting = self.find_counting.is_some();
        let (count, color) = match (current, total) {
            _ if self.find_query.is_empty() => (String::new(), self.theme.muted),
            _ if self.find_error.is_some() => ("Invalid regex".to_string(), self.theme.pink),
            (_, 0) if counting => ("Counting...".to_string(), self.theme.muted),
            (_, 0) => ("No results".to_string(), self.theme.pink),
            (0, total) if counting => (format!("{total}+ results"), self.theme.muted),
            (current, total) if counting => (format!("{current} of {total}+"), self.theme.muted),
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
                field.right - self.scale(9),
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
    // placeholder, and a caret when it has the focus. The text stops at
    // `text_right`, leaving room for buttons inside the field.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_find_field(
        &self,
        hdc: HDC,
        rect: RECT,
        text: &str,
        placeholder: &str,
        focused: bool,
        selected: bool,
        text_right: i32,
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
            right: text_right,
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

impl FindCounting {
    fn new(document: &Document, matcher: Matcher, current: Option<(Pos, Pos)>) -> Self {
        Self {
            document: document.id(),
            serial: document.change_serial(),
            matcher,
            current,
            next_line: 0,
            counted: (0, 0),
            seek: None,
            before_seek: None,
            at_seek: None,
        }
    }

    /// Counts up to `lines` more lines of `doc`.
    fn step(&mut self, doc: &Document, lines: usize) {
        let end = (self.next_line + lines).min(doc.line_count());
        for line in self.next_line..end {
            self.tally(line, doc.line(line));
        }
        self.next_line = end;
    }

    /// Counts line `line`'s matches: `counted` is (current, total), where
    /// current is the 1-based number of the match that is exactly the range
    /// `current`, or 0 when none is.
    fn tally(&mut self, line: usize, text: &str) {
        for (start, end) in self.matcher.matches(text) {
            self.counted.1 += 1;
            let range = (Pos { line, byte: start }, Pos { line, byte: end });
            if self.current == Some(range) {
                self.counted.0 = self.counted.1;
            }
            if let Some(origin) = self.seek
                && self.at_seek.is_none()
            {
                if range.0 >= origin {
                    self.at_seek = Some((range, self.counted.1));
                } else if self.before_seek.is_none()
                    && (line < origin.line || range.1.byte <= origin.byte)
                {
                    // Wrapped round, as find_next does: on the seek line
                    // only what ends before it.
                    self.before_seek = Some((range, self.counted.1));
                }
            }
        }
    }
}

#[cfg(test)]
fn count_matches<'a>(
    lines: impl Iterator<Item = &'a str>,
    matcher: &Matcher,
    current: Option<(Pos, Pos)>,
) -> (usize, usize) {
    let mut counting = FindCounting::new(&Document::new(), matcher.clone(), current);
    for (line, text) in lines.enumerate() {
        counting.tally(line, text);
    }
    counting.counted
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
        let plain = |q| Matcher::new(q, Default::default()).unwrap();
        let at = |line, byte| {
            Some((
                Pos { line, byte },
                Pos {
                    line,
                    byte: byte + 3,
                },
            ))
        };
        let foo = plain("foo");
        assert_eq!(count_matches(lines.into_iter(), &foo, None), (0, 4));
        assert_eq!(count_matches(lines.into_iter(), &foo, at(0, 8)), (2, 4));
        assert_eq!(count_matches(lines.into_iter(), &foo, at(2, 3)), (4, 4));
        assert_eq!(count_matches(lines.into_iter(), &foo, at(0, 1)), (0, 4));
        assert_eq!(
            count_matches(lines.into_iter(), &plain("baz"), None),
            (0, 0)
        );
        // A pattern's matches differ in length.
        let words = Matcher::new(
            r"f\w+",
            lightline::text_search::SearchOptions {
                regex: true,
                ..Default::default()
            },
        )
        .unwrap();
        let range = Some((Pos { line: 2, byte: 0 }, Pos { line: 2, byte: 6 }));
        assert_eq!(count_matches(lines.into_iter(), &words, range), (3, 3));
    }

    #[test]
    fn a_count_in_slices_matches_one_pass() {
        let text: String = (0..1_000)
            .map(|i| format!("let x{i} = foo(); // foo\n"))
            .collect();
        let mut doc = Document::new();
        doc.seed(&text);
        let matcher = Matcher::new("foo", Default::default()).unwrap();
        let line = doc.line(700);
        let byte = line.find("foo").unwrap();
        let current = Some((
            Pos { line: 700, byte },
            Pos {
                line: 700,
                byte: byte + 3,
            },
        ));
        let lines: Vec<&str> = (0..doc.line_count()).map(|line| doc.line(line)).collect();
        let whole = count_matches(lines.into_iter(), &matcher, current);
        let mut counting = FindCounting::new(&doc, matcher.clone(), current);
        while counting.next_line < doc.line_count() {
            counting.step(&doc, 37);
        }
        assert_eq!(counting.counted, whole);
        assert_eq!(whole, (1_401, 2_000));

        // Seeking from line 500 finds that line's first match, number 1,001.
        let seek = Pos { line: 500, byte: 0 };
        let mut counting = FindCounting {
            seek: Some(seek),
            ..FindCounting::new(&doc, matcher.clone(), None)
        };
        counting.step(&doc, 1_000_000);
        let (range, number) = counting.at_seek.unwrap();
        assert_eq!(range.0.line, 500);
        assert_eq!(number, 1_001);
        // From past the last match it wraps round to the first.
        let mut counting = FindCounting {
            seek: Some(Pos {
                line: 999,
                byte: 30,
            }),
            ..FindCounting::new(&doc, matcher, None)
        };
        counting.step(&doc, 1_000_000);
        assert!(counting.at_seek.is_none());
        let (range, number) = counting.before_seek.unwrap();
        assert_eq!((range.0.line, number), (0, 1));
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
