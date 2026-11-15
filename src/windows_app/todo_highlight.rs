// TODO, FIXME, HACK and NOTE inside comments, in their own colours on a
// faint tint (`todoHighlight`), and the Command Palette's "Find TODOs in
// Workspace", which runs the project search with a fixed pattern.
//
// The words are found in the comment spans of the lines being drawn, the
// spans the syntax colours already gave them, so a TODO in a string or a
// variable named `todo` isn't marked and nothing is kept between paints.

use super::render::blend;
use super::*;
use lightline::syntax::{Color, Span};

/// The command's id in `QUICK_COMMANDS`.
pub(super) const FIND_TODOS: u8 = 71;

/// Counting the markers for the scrollbar waits for typing to pause.
pub(super) const TODO_TIMER: usize = 28;
const COUNT_DELAY_MS: u32 = 300;

/// A file's lines with a marker, as last counted: for the document's
/// change `serial`, and whether its colours were all there (`colored`).
pub(super) struct TodoLines {
    serial: u64,
    colored: bool,
    marks: Vec<(usize, TodoKind)>,
}

/// The pattern "Find TODOs in Workspace" searches for.
pub(super) const TODO_PATTERN: &str = r"\b(TODO|FIXME|HACK|NOTE)\b";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TodoKind {
    Todo,
    Fixme,
    Hack,
    Note,
}

const WORDS: [(&str, TodoKind); 4] = [
    ("TODO", TodoKind::Todo),
    ("FIXME", TodoKind::Fixme),
    ("HACK", TodoKind::Hack),
    ("NOTE", TodoKind::Note),
];

/// Each marker word in `source`'s comment spans: (start, end, kind), in
/// order. A word only counts on its own: `TODOS` or `MY_NOTE` don't.
pub(super) fn todo_marks(source: &str, spans: &[Span]) -> Vec<(usize, usize, TodoKind)> {
    let bytes = source.as_bytes();
    let part_of_word = |at: usize| {
        bytes
            .get(at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
    };
    let mut marks = Vec::new();
    for span in spans.iter().filter(|span| span.color == Color::Comment) {
        let Some(text) = source.get(span.start..span.end.min(source.len())) else {
            continue;
        };
        // Every marker starts with an upper-case letter T, F, H or N.
        for (offset, _) in text.match_indices(['T', 'F', 'H', 'N']) {
            let start = span.start + offset;
            if start > 0 && part_of_word(start - 1) {
                continue;
            }
            if let Some(&(word, kind)) = WORDS
                .iter()
                .find(|(word, _)| source[start..].starts_with(word))
                && !part_of_word(start + word.len())
                && start + word.len() <= span.end
            {
                marks.push((start, start + word.len(), kind));
            }
        }
    }
    marks
}

impl App {
    /// The colour a marker word is drawn in.
    pub(super) fn todo_color(&self, kind: TodoKind) -> u32 {
        match kind {
            TodoKind::Todo => self.theme.sky,
            TodoKind::Fixme => self.theme.error,
            TodoKind::Hack => self.theme.orange,
            TodoKind::Note => self.theme.green,
        }
    }

    /// The tint behind a marker word.
    pub(super) fn todo_tint(&self, kind: TodoKind) -> u32 {
        blend(self.theme.editor_bg, self.todo_color(kind), 0.16)
    }

    // What a count of tab `index`'s markers depends on.
    fn todo_key(&self, index: usize) -> (u64, bool) {
        let tab = &self.tabs[index];
        let colored = tab
            .syntax
            .as_ref()
            .is_none_or(|syntax| syntax.colored(&tab.document));
        (tab.document.change_serial(), colored)
    }

    // Every line of tab `index` with a marker in a comment, and its first
    // marker's kind. Only lines holding one of the words are looked at
    // closely.
    fn count_todos(&self, index: usize) -> TodoLines {
        let tab = &self.tabs[index];
        let doc = &tab.document;
        let (serial, colored) = self.todo_key(index);
        let mut marks = Vec::new();
        if let Some(syntax) = &tab.syntax {
            for line in 0..doc.line_count() {
                let text = doc.line(line);
                if !WORDS.iter().any(|(word, _)| text.contains(word)) {
                    continue;
                }
                if let Some(&(_, _, kind)) = todo_marks(text, &syntax.spans(doc, line)).first() {
                    marks.push((line, kind));
                }
            }
        }
        TodoLines {
            serial,
            colored,
            marks,
        }
    }

    /// Draws tab `pane`'s markers on its scrollbar `track`, a short bar in
    /// the marker's colour at its line's height. When the file changed
    /// since they were counted, they're counted again once typing pauses
    /// (TODO_TIMER); until then the last count shows.
    pub(super) fn paint_todo_marks(&self, hdc: HDC, hwnd: HWND, pane: usize, track: RECT) {
        if !self.settings.todo_highlight {
            return;
        }
        let index = self.tab_for_pane(pane);
        let tab = &self.tabs[index];
        let counted = tab.todo_lines.borrow();
        let current = counted
            .as_ref()
            .is_some_and(|lines| (lines.serial, lines.colored) == self.todo_key(index));
        if !current {
            unsafe { SetTimer(hwnd, TODO_TIMER, COUNT_DELAY_MS, None) };
        }
        let Some(lines) = counted.as_ref() else {
            return;
        };
        let total = tab.document.line_count().max(1) as i64;
        let height = i64::from(track.bottom - track.top);
        let (left, right) = (track.right - self.scale(7), track.right - self.scale(2));
        let thick = self.scale(2).max(2);
        for &(line, kind) in &lines.marks {
            let y = track.top + (line as i64 * height / total) as i32;
            Self::fill(
                hdc,
                RECT {
                    left,
                    top: y.min(track.bottom - thick),
                    right,
                    bottom: (y + thick).min(track.bottom),
                },
                self.todo_color(kind),
            );
        }
    }

    /// TODO_TIMER: counts the markers of the files shown again, and redraws
    /// a scrollbar whose marks changed.
    pub(super) fn todo_timer(&mut self, hwnd: HWND) {
        unsafe { KillTimer(hwnd, TODO_TIMER) };
        let panes = if self.split_visible { 0..2 } else { 0..1 };
        for pane in panes {
            let index = self.tab_for_pane(pane);
            let fresh = self.count_todos(index);
            let changed = self.tabs[index]
                .todo_lines
                .borrow()
                .as_ref()
                .is_none_or(|old| old.marks != fresh.marks);
            *self.tabs[index].todo_lines.borrow_mut() = Some(fresh);
            if changed && let Some((track, _)) = self.scrollbar(hwnd, pane) {
                unsafe { InvalidateRect(hwnd, &track, 0) };
            }
        }
    }

    /// Command Palette: Find TODOs in Workspace. The project search, set to
    /// a regular expression matching case, with the markers' pattern.
    pub(super) fn find_todos(&mut self, hwnd: HWND) {
        if self.workspace_root.is_none() {
            self.status = "Open a workspace to find its TODOs".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        self.open_project_search(hwnd);
        self.project_query = TODO_PATTERN.into();
        self.search_options.regex = true;
        self.search_options.match_case = true;
        self.search_options.whole_word = false;
        self.search_project();
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(start: usize, end: usize) -> Span {
        Span {
            start,
            end,
            color: Color::Comment,
        }
    }

    #[test]
    fn markers_in_comments_are_found() {
        let line = "x = 1  # TODO: fix, FIXME(me) and NOTE";
        let marks = todo_marks(line, &[comment(7, line.len())]);
        let words: Vec<(&str, TodoKind)> = marks
            .iter()
            .map(|&(start, end, kind)| (&line[start..end], kind))
            .collect();
        assert_eq!(
            words,
            [
                ("TODO", TodoKind::Todo),
                ("FIXME", TodoKind::Fixme),
                ("NOTE", TodoKind::Note)
            ]
        );
    }

    #[test]
    fn only_whole_words_inside_comments_count() {
        let line = "let todo = \"TODO\"; // TODOS MY_NOTE HACKy XHACK HACK";
        let string = Span {
            start: 11,
            end: 17,
            color: Color::String,
        };
        let marks = todo_marks(line, &[string, comment(19, line.len())]);
        assert_eq!(marks.len(), 1);
        let (start, end, kind) = marks[0];
        assert_eq!((&line[start..end], kind), ("HACK", TodoKind::Hack));
        // A word cut by the span's end isn't one.
        assert!(todo_marks("// TODO", &[comment(0, 5)]).is_empty());
        assert!(todo_marks("// TODO", &[]).is_empty());
    }

    #[test]
    fn the_search_pattern_finds_the_same_words() {
        let options = lightline::text_search::SearchOptions {
            regex: true,
            match_case: true,
            whole_word: false,
        };
        let matcher = Matcher::new(TODO_PATTERN, options).unwrap();
        assert!(matcher.find_at("// TODO: x", 0).is_some());
        assert!(matcher.find_at("# FIXME", 0).is_some());
        assert!(!matcher.find_at("// todo", 0).is_some());
        assert!(!matcher.find_at("TODOS", 0).is_some());
    }
}
