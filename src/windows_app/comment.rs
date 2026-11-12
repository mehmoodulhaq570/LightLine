// Ctrl+/ toggles line comments on the caret's line or the selected lines,
// with the comment tokens of the file's language: an extension language's
// `line_comments` (or its `block_comment` when it has no line comment), and
// LightLine's own languages' usual ones. The same file holds the other
// typing rules an extension language brings: which pairs typing closes, and
// outdenting a line that becomes `end` or `else`.

use super::*;
use lightline::extensions::editing::EditingRules;

/// How a file's language writes a comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CommentStyle {
    /// `// ` before each line.
    Line(String),
    /// `<!-- ` before the first line and ` -->` after the last.
    Block(String, String),
}

/// One edit on line `line` of the lines toggled: bytes `start..end`
/// become `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LineEdit {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

fn indent_len(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// The edits that comment out `lines`, or uncomment them when every line
/// with text already starts with `prefix`. Blank lines are left alone;
/// the prefix goes at the shallowest indentation, as in VS Code.
pub(super) fn toggle_line_comments(lines: &[&str], prefix: &str) -> Vec<LineEdit> {
    let token = prefix.trim_end();
    let filled: Vec<usize> = (0..lines.len())
        .filter(|&index| !lines[index].trim().is_empty())
        .collect();
    if token.is_empty() || filled.is_empty() {
        return Vec::new();
    }
    let commented = filled
        .iter()
        .all(|&index| lines[index].trim_start().starts_with(token));
    if commented {
        return filled
            .into_iter()
            .map(|index| {
                let line = lines[index];
                let start = indent_len(line);
                let mut end = start + token.len();
                // The space the prefix put after the token goes too.
                if prefix.len() > token.len() && line[end..].starts_with(' ') {
                    end += 1;
                }
                LineEdit {
                    line: index,
                    start,
                    end,
                    text: String::new(),
                }
            })
            .collect();
    }
    let column = filled
        .iter()
        .map(|&index| indent_len(lines[index]))
        .min()
        .unwrap_or(0);
    filled
        .into_iter()
        .map(|index| LineEdit {
            line: index,
            start: column,
            end: column,
            text: prefix.to_string(),
        })
        .collect()
}

/// The edits that wrap `lines` in `open`..`close`, or unwrap them when
/// they already start and end with them.
pub(super) fn toggle_block_comment(lines: &[&str], open: &str, close: &str) -> Vec<LineEdit> {
    let (open_token, close_token) = (open.trim_end(), close.trim_start());
    let Some(first) = lines.iter().position(|line| !line.trim().is_empty()) else {
        return Vec::new();
    };
    let last = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .unwrap_or(first);
    let (first_text, last_text) = (lines[first], lines[last]);
    let start = indent_len(first_text);
    let end = last_text.trim_end().len();
    let wrapped = first_text[start..].starts_with(open_token)
        && last_text[..end].ends_with(close_token)
        && (first != last || end - start >= open_token.len() + close_token.len());
    if wrapped {
        let mut close_start = end - close_token.len();
        if close.len() > close_token.len() && last_text[..close_start].ends_with(' ') {
            close_start -= 1;
        }
        let mut open_end = start + open_token.len();
        if open.len() > open_token.len() && first_text[open_end..].starts_with(' ') {
            open_end += 1;
        }
        // On one line the two mustn't overlap.
        let close_start = if first == last {
            close_start.max(open_end)
        } else {
            close_start
        };
        return vec![
            LineEdit {
                line: last,
                start: close_start,
                end,
                text: String::new(),
            },
            LineEdit {
                line: first,
                start,
                end: open_end,
                text: String::new(),
            },
        ];
    }
    vec![
        LineEdit {
            line: last,
            start: end,
            end,
            text: close.to_string(),
        },
        LineEdit {
            line: first,
            start,
            end: start,
            text: open.to_string(),
        },
    ]
}

/// Where `byte` of a line is after `edits` on that line.
fn moved_byte(byte: usize, edits: &[&LineEdit]) -> usize {
    let mut moved = byte as isize;
    for edit in edits.iter().filter(|edit| edit.start < byte) {
        moved -= (edit.end.min(byte) - edit.start) as isize;
        moved += edit.text.len() as isize;
    }
    moved.max(0) as usize
}

/// The comments of LightLine's own languages, and of a few common files
/// it shows without colors, by the file's name.
fn builtin_comment(path: Option<&Path>) -> Option<CommentStyle> {
    let extension = path?.extension()?.to_str()?.to_ascii_lowercase();
    let line = |prefix: &str| Some(CommentStyle::Line(prefix.to_string()));
    let block = |open: &str, close: &str| Some(CommentStyle::Block(open.into(), close.into()));
    match extension.as_str() {
        "rs" | "c" | "h" | "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" | "ino" | "js" | "jsx"
        | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" | "go" | "jsonc" | "java" | "kt" | "cs"
        | "swift" | "dart" | "scala" | "zig" => line("// "),
        "py" | "pyw" | "toml" | "yaml" | "yml" | "sh" | "bash" | "ps1" | "psm1" | "rb" | "r"
        | "pl" | "cmake" | "dockerfile" | "gitignore" => line("# "),
        "lua" | "sql" | "hs" => line("-- "),
        "ini" | "cfg" => line("; "),
        "bat" | "cmd" => line("REM "),
        "md" | "markdown" | "html" | "htm" | "xml" | "svg" => block("<!-- ", " -->"),
        "css" => block("/* ", " */"),
        _ => None,
    }
}

impl App {
    /// How the active file's language writes a comment, if it's known.
    pub(super) fn comment_style(&self) -> Option<CommentStyle> {
        match self.tab().editing_rules() {
            Some(rules) => match (rules.line_comments.first(), &rules.block_comment) {
                (Some(prefix), _) => Some(CommentStyle::Line(prefix.clone())),
                (None, Some((open, close))) => {
                    Some(CommentStyle::Block(open.clone(), close.clone()))
                }
                (None, None) => None,
            },
            None => builtin_comment(self.doc().path.as_deref()),
        }
    }

    /// Ctrl+/: comments out the caret's line or the selected lines, or
    /// uncomments them. One Ctrl+Z undoes it.
    pub(super) fn toggle_comment(&mut self, hwnd: HWND) {
        if self.tab().read_only() || self.tab().is_placeholder() || self.showing_diff() {
            return;
        }
        let Some(style) = self.comment_style() else {
            self.status = "This file's language has no comments LightLine knows".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        self.drop_extra_carets();
        let cursor = self.view().cursor;
        let anchor = self.view().selection_anchor;
        let (start, end) = self.selection_range().unwrap_or((cursor, cursor));
        // A selection ending at a line's start doesn't take that line.
        let last = if end.line > start.line && end.byte == 0 {
            end.line - 1
        } else {
            end.line
        };
        let first = start.line;
        let edits = {
            let doc = self.doc();
            let lines: Vec<&str> = (first..=last).map(|line| doc.line(line)).collect();
            let mut edits = match &style {
                CommentStyle::Line(prefix) => toggle_line_comments(&lines, prefix),
                CommentStyle::Block(open, close) => toggle_block_comment(&lines, open, close),
            };
            for edit in &mut edits {
                edit.line += first;
            }
            edits
        };
        if edits.is_empty() {
            return;
        }
        // Later places first, so earlier ones stay where they were.
        let mut ordered: Vec<&LineEdit> = edits.iter().collect();
        ordered.sort_by_key(|edit| std::cmp::Reverse((edit.line, edit.start)));
        self.doc_mut().begin_group();
        for edit in &ordered {
            self.replace_range(
                Pos {
                    line: edit.line,
                    byte: edit.start,
                },
                Pos {
                    line: edit.line,
                    byte: edit.end,
                },
                &edit.text,
            );
        }
        self.doc_mut().end_group();
        let moved = |at: Pos| {
            let on_line: Vec<&LineEdit> =
                edits.iter().filter(|edit| edit.line == at.line).collect();
            Pos {
                line: at.line,
                byte: moved_byte(at.byte, &on_line),
            }
        };
        let view = self.view_mut();
        view.cursor = moved(cursor);
        view.selection_anchor = anchor.map(moved);
        self.keep_cursor_visible(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// The character typing `ch` closes it with, in an extension language
    /// that lists its brackets: only before whitespace, the end of the line
    /// or one of its `autoclose_before` characters, not inside a string or
    /// comment when the pair says so, and a quote not right after a word.
    pub(super) fn extension_auto_close(&self, ch: char) -> Option<char> {
        if !self.settings.auto_close_pairs {
            return None;
        }
        let rules = self.tab().editing_rules()?;
        let pair = *rules.auto_pair(ch)?;
        let brackets = rules.brackets.as_ref()?;
        let cursor = self.view().cursor;
        let line = self.doc().line(cursor.line);
        let after = line.get(cursor.byte..).unwrap_or_default();
        if !after
            .chars()
            .next()
            .is_none_or(|next| next.is_whitespace() || brackets.autoclose_before.contains(next))
        {
            return None;
        }
        if pair.open == pair.close
            && line
                .get(..cursor.byte)
                .and_then(|before| before.chars().next_back())
                .is_some_and(|previous| previous.is_alphanumeric() || previous == '_')
        {
            return None;
        }
        if (pair.not_in_string || pair.not_in_comment)
            && let Some(syntax) = &self.tab().syntax
        {
            let byte = cursor.byte;
            let inside = syntax.spans(self.doc(), cursor.line).iter().any(|span| {
                match span.color {
                    Color::String => pair.not_in_string && span.start < byte && byte < span.end,
                    // A line comment runs to the end of the line.
                    Color::Comment => pair.not_in_comment && span.start < byte && byte <= span.end,
                    _ => false,
                }
            });
            if inside {
                return None;
            }
        }
        Some(pair.close)
    }

    fn outdent_rules(&self) -> Option<&EditingRules> {
        self.tab()
            .editing_rules()
            .filter(|rules| self.settings.auto_indent && rules.decrease_indent.is_some())
    }

    /// True when the caret's line is one an extension language outdents
    /// (`end`, `else`...), checked before a keystroke.
    pub(super) fn caret_line_outdents(&self) -> bool {
        self.outdent_rules()
            .is_some_and(|rules| rules.outdents(self.doc().line(self.view().cursor.line)))
    }

    /// After a typed character: a line that just became `end` or `else`
    /// moves one level out, unless it's already shallower than the line
    /// above; and one typing outdented that has become something else
    /// (`endpoint`) moves back. `before` is `caret_line_outdents` from
    /// before the keystroke.
    pub(super) fn outdent_after_typing(&mut self, before: bool) {
        let Some(rules) = self.outdent_rules() else {
            self.auto_outdent = None;
            return;
        };
        let cursor = self.view().cursor;
        let document = self.doc().id();
        let text = self.doc().line(cursor.line);
        let now = rules.outdents(text);
        if now && !before {
            let (_, size) = self.indent_style();
            let indent = indent_len(text);
            if indent == 0 || cursor.byte < indent {
                return;
            }
            let above = (0..cursor.line)
                .rev()
                .map(|line| self.doc().line(line))
                .find(|line| !line.trim().is_empty())
                .map_or(0, |line| super::bracket_colors::indent_columns(line, size));
            if super::bracket_colors::indent_columns(text, size) < above {
                return;
            }
            let width = if text.starts_with('\t') {
                1
            } else {
                text.bytes().take(size).take_while(|b| *b == b' ').count()
            };
            let removed = text[..width].to_string();
            let line = cursor.line;
            self.replace_range(Pos { line, byte: 0 }, Pos { line, byte: width }, "");
            self.view_mut().cursor = Pos {
                line,
                byte: cursor.byte - width,
            };
            self.auto_outdent = Some((document, line, removed));
        } else if !now && before {
            if let Some((outdented, line, removed)) = self.auto_outdent.take()
                && outdented == document
                && line == cursor.line
            {
                self.replace_range(Pos { line, byte: 0 }, Pos { line, byte: 0 }, &removed);
                self.view_mut().cursor = Pos {
                    line,
                    byte: cursor.byte + removed.len(),
                };
            }
        } else if self
            .auto_outdent
            .as_ref()
            .is_some_and(|(outdented, line, _)| *outdented != document || *line != cursor.line)
        {
            self.auto_outdent = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(lines: &[&str], edits: &[LineEdit]) -> Vec<String> {
        let mut out: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
        let mut ordered: Vec<&LineEdit> = edits.iter().collect();
        ordered.sort_by_key(|edit| std::cmp::Reverse((edit.line, edit.start)));
        for edit in ordered {
            out[edit.line].replace_range(edit.start..edit.end, &edit.text);
        }
        out
    }

    #[test]
    fn line_comments_go_at_the_shallowest_indent_and_come_off_again() {
        let lines = ["if x then", "    print(x)", "", "  end"];
        let commented = apply(&lines, &toggle_line_comments(&lines, "-- "));
        assert_eq!(
            commented,
            ["-- if x then", "--     print(x)", "", "--   end"].map(String::from)
        );
        let refs: Vec<&str> = commented.iter().map(String::as_str).collect();
        let back = apply(&refs, &toggle_line_comments(&refs, "-- "));
        assert_eq!(back, lines.map(String::from));
    }

    #[test]
    fn a_partly_commented_selection_is_commented_and_tokens_without_space_come_off() {
        let lines = ["    # done", "    todo()"];
        let out = apply(&lines, &toggle_line_comments(&lines, "# "));
        assert_eq!(out, ["    # # done", "    # todo()"]);
        // `#x` (no space after the token) loses only the token.
        let lines = ["#x", "# y"];
        let out = apply(&lines, &toggle_line_comments(&lines, "# "));
        assert_eq!(out, ["x", "y"]);
        assert!(toggle_line_comments(&["", "  "], "# ").is_empty());
    }

    #[test]
    fn block_comments_wrap_the_lines_and_unwrap_them() {
        let lines = ["  <p>", "  </p>"];
        let wrapped = apply(&lines, &toggle_block_comment(&lines, "<!-- ", " -->"));
        assert_eq!(wrapped, ["  <!-- <p>", "  </p> -->"]);
        let refs: Vec<&str> = wrapped.iter().map(String::as_str).collect();
        let back = apply(&refs, &toggle_block_comment(&refs, "<!-- ", " -->"));
        assert_eq!(back, lines.map(String::from));
        let one = ["a { b }"];
        let wrapped = apply(&one, &toggle_block_comment(&one, "/* ", " */"));
        assert_eq!(wrapped, ["/* a { b } */"]);
        let refs: Vec<&str> = wrapped.iter().map(String::as_str).collect();
        assert_eq!(
            apply(&refs, &toggle_block_comment(&refs, "/* ", " */")),
            ["a { b }"]
        );
    }

    #[test]
    fn carets_follow_the_text_they_were_on() {
        let insert = LineEdit {
            line: 0,
            start: 4,
            end: 4,
            text: "-- ".into(),
        };
        // At the line's start: stays, so whole lines stay selected.
        assert_eq!(moved_byte(0, &[&insert]), 0);
        assert_eq!(moved_byte(4, &[&insert]), 4);
        assert_eq!(moved_byte(9, &[&insert]), 12);
        let remove = LineEdit {
            line: 0,
            start: 4,
            end: 7,
            text: String::new(),
        };
        assert_eq!(moved_byte(5, &[&remove]), 4);
        assert_eq!(moved_byte(10, &[&remove]), 7);
    }

    #[test]
    fn builtin_languages_have_their_usual_comments() {
        let style = |name: &str| builtin_comment(Some(Path::new(name)));
        assert_eq!(style("main.rs"), Some(CommentStyle::Line("// ".into())));
        assert_eq!(style("app.py"), Some(CommentStyle::Line("# ".into())));
        assert_eq!(
            style("README.md"),
            Some(CommentStyle::Block("<!-- ".into(), " -->".into()))
        );
        assert_eq!(style("data.json"), None);
        assert_eq!(style("notes.txt"), None);
        assert_eq!(builtin_comment(None), None);
    }
}
