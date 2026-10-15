//! Go to Line: Ctrl+G, or `:` in Quick Open. "42" goes to line 42, "42:7"
//! to its 7th character; the line is shown a third of the way down when it
//! was off screen.

use super::*;

/// The 0-based line and character typed after `:`, clamped to the file;
/// None while nothing usable is typed.
pub(super) fn parse_target(query: &str, line_count: usize) -> Option<(usize, Option<usize>)> {
    let text = query.trim_start_matches(':').trim();
    let (line, column) = match text.split_once([':', ',']) {
        Some((line, column)) => (line.trim(), Some(column.trim())),
        None => (text, None),
    };
    let line: usize = line.parse().ok()?;
    let column = match column {
        Some("") | None => None,
        Some(column) => Some(column.parse::<usize>().ok()?.saturating_sub(1)),
    };
    Some((line.clamp(1, line_count.max(1)) - 1, column))
}

impl App {
    /// Ctrl+G: Quick Open on the line number prompt.
    pub(super) fn show_goto_line(&mut self, hwnd: HWND) {
        self.show_quick_open(hwnd);
        self.quick_query = ":".into();
    }

    fn goto_target(&self) -> Option<(usize, Option<usize>)> {
        if self.welcome || self.tab().is_placeholder() {
            return None;
        }
        parse_target(&self.quick_query, self.doc().line_count())
    }

    /// The one row the `:` list shows.
    pub(super) fn goto_line_items(&self) -> Vec<String> {
        match self.goto_target() {
            Some((line, Some(column))) => {
                vec![format!("Go to line {}, character {}", line + 1, column + 1)]
            }
            Some((line, None)) => vec![format!("Go to line {}", line + 1)],
            None => Vec::new(),
        }
    }

    /// What the `:` list says when nothing usable is typed.
    pub(super) fn goto_line_message(&self) -> String {
        if self.welcome || self.tab().is_placeholder() {
            return "Open a file to go to a line".into();
        }
        let doc = self.doc();
        let cursor = self.view().cursor;
        format!(
            "Current line: {}, character: {}. Type a line number between 1 and {} (line:character also works)",
            cursor.line + 1,
            doc.line(cursor.line)[..cursor.byte.min(doc.line(cursor.line).len())]
                .chars()
                .count()
                + 1,
            doc.line_count()
        )
    }

    pub(super) fn goto_line(&mut self, hwnd: HWND) {
        let Some((line, column)) = self.goto_target() else {
            return;
        };
        let text = self.doc().line(line);
        let byte = match column {
            // Past the end goes to the end, as in VS Code.
            Some(column) => text
                .char_indices()
                .nth(column)
                .map_or(text.len(), |(byte, _)| byte),
            // The line's first non-blank character.
            None => text.len() - text.trim_start().len(),
        };
        let pane = self.focused_pane;
        let shown = unsafe { IsIconic(hwnd) } == 0
            && self
                .locate(
                    hwnd,
                    pane,
                    Pos { line, byte },
                    self.visible_lines(hwnd).saturating_sub(1),
                )
                .is_some();
        self.move_cursor(Pos { line, byte }, false);
        if !shown && unsafe { IsIconic(hwnd) } == 0 {
            self.doc_mut().unfold_to_reveal(line);
            let visible = self.visible_lines(hwnd) as isize;
            let top = self.step_rows(hwnd, pane, (line, 0), -(visible / 3));
            self.set_view_top(top);
        }
        self.keep_cursor_visible(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_target;

    #[test]
    fn reads_line_and_character() {
        assert_eq!(parse_target(":42", 100), Some((41, None)));
        assert_eq!(parse_target(": 42 ", 100), Some((41, None)));
        assert_eq!(parse_target(":42:7", 100), Some((41, Some(6))));
        assert_eq!(parse_target(":42,7", 100), Some((41, Some(6))));
        assert_eq!(parse_target(":42:", 100), Some((41, None)));
    }

    #[test]
    fn clamps_to_the_file_and_refuses_words() {
        assert_eq!(parse_target(":999", 100), Some((99, None)));
        assert_eq!(parse_target(":0", 100), Some((0, None)));
        assert_eq!(parse_target(":", 100), None);
        assert_eq!(parse_target(":abc", 100), None);
        assert_eq!(parse_target(":4:x", 100), None);
        assert_eq!(parse_target(":-3", 100), None);
    }
}
