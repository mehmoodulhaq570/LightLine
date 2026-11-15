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
