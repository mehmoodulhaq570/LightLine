// A problem's message written after its line, like VS Code's Error Lens:
// the line's worst problem (`inlineDiagnostics` says which severities), its
// first line of text, cut with an ellipsis at the window's edge, and "+N"
// when the line has more. Drawn by code_pane.rs after the Run links, only on
// a line's last row and only where it fits; it reuses the diagnostics the
// squiggles come from, so nothing new is asked of the language server.

use super::render::blend;
use super::*;

/// The problem starting on `line` to show after it, and how many more start
/// there: the worst severity no worse than `max_severity` (1 error, 2
/// warning), the first one on the line among equals. None when none shows.
pub(super) fn line_problem(
    diagnostics: &[LspDiagnostic],
    line: usize,
    max_severity: u8,
) -> Option<(&LspDiagnostic, usize)> {
    let mut shown = diagnostics.iter().filter(|problem| {
        problem.range.start.line as usize == line
            && problem.severity >= 1
            && problem.severity <= max_severity
    });
    let first = shown.next()?;
    let (worst, count) = shown.fold((first, 1), |(worst, count), problem| {
        let better = (problem.severity, problem.range.start.character)
            < (worst.severity, worst.range.start.character);
        (if better { problem } else { worst }, count + 1)
    });
    Some((worst, count - 1))
}

/// The message's first non-empty line, with tabs as spaces: servers (rustc)
/// put notes on the lines after it.
pub(super) fn first_line(message: &str) -> String {
    message
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .replace('\t', " ")
}

impl App {
    /// Draws `problem`'s message from `after` (where the row's text and
    /// links end) up to `right`, on the row at `y`; `more` is how many other
    /// problems the line has, and `current` whether the caret is on it.
    /// Returns where the drawing ends, or `after` when nothing fitted.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_line_problem(
        &self,
        hdc: HDC,
        problem: &LspDiagnostic,
        more: usize,
        current: bool,
        (after, y, right): (i32, i32, i32),
        clip: RECT,
    ) -> i32 {
        let s = |value: i32| self.scale(value);
        let x = after + s(28);
        let dot = s(7);
        let text_x = x + dot + s(6);
        // Less room than a few letters says nothing useful.
        if text_x + s(40) > right {
            return after;
        }
        let tone = if problem.severity == 1 {
            self.theme.error
        } else {
            self.theme.warning
        };
        // Scrolled sideways, the dot can start left of the code, over the
        // gutter: the text is clipped there, the dot is left out.
        if x >= clip.left {
            let dot_top = y + (self.line_height - dot) / 2;
            Self::rounded_fill(
                hdc,
                RECT {
                    left: x,
                    top: dot_top,
                    right: x + dot,
                    bottom: dot_top + dot,
                },
                dot,
                tone,
            );
        }
        unsafe { SelectObject(hdc, self.small_font) };
        let text_top = y + (self.line_height - self.text_height(hdc)) / 2;
        let extra = (more > 0).then(|| format!("  +{more}"));
        let extra_width = extra
            .as_deref()
            .map_or(0, |text| self.text_width(hdc, text));
        let message = first_line(&problem.message);
        let message_right = (right - extra_width).max(text_x);
        let color = if current {
            self.theme.text
        } else {
            blend(self.theme.editor_bg, tone, 0.8)
        };
        let row_clip = RECT { right, ..clip };
        self.label_ellipsis(
            hdc,
            &message,
            text_x,
            text_top,
            color,
            RECT {
                right: message_right,
                ..row_clip
            },
        );
        let mut end = (text_x + self.text_width(hdc, &message)).min(message_right);
        if let Some(extra) = &extra {
            Self::label(hdc, extra, end, text_top, self.theme.muted, row_clip);
            end += extra_width;
        }
        unsafe { SelectObject(hdc, self.font) };
        end.min(right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightline::lsp::{Position, Range};

    fn problem(line: u32, character: u32, severity: u8, message: &str) -> LspDiagnostic {
        let at = Position { line, character };
        LspDiagnostic {
            range: Range { start: at, end: at },
            severity,
            message: message.into(),
            raw: serde_json::Value::Null,
            origin: None,
        }
    }

    #[test]
    fn the_worst_problem_on_the_line_shows_and_the_rest_are_counted() {
        let problems = [
            problem(3, 20, 2, "unused"),
            problem(3, 9, 1, "late error"),
            problem(3, 4, 1, "early error"),
            problem(4, 0, 1, "next line"),
            problem(3, 0, 3, "info, never counted"),
        ];
        let (shown, more) = line_problem(&problems, 3, 2).unwrap();
        assert_eq!((shown.message.as_str(), more), ("early error", 2));
        // Errors only: the warning neither shows nor counts.
        let (shown, more) = line_problem(&problems, 3, 1).unwrap();
        assert_eq!((shown.message.as_str(), more), ("early error", 1));
        assert!(line_problem(&problems, 3, 0).is_none());
        assert!(line_problem(&problems, 7, 2).is_none());
    }

    #[test]
    fn a_warning_shows_only_when_warnings_do() {
        let problems = [problem(0, 0, 2, "unused import")];
        assert!(line_problem(&problems, 0, 1).is_none());
        assert_eq!(line_problem(&problems, 0, 2).unwrap().1, 0);
    }

    #[test]
    fn only_the_first_line_of_a_message_is_written() {
        assert_eq!(
            first_line("\n  mismatched types\nexpected `u32`, found `usize`"),
            "mismatched types"
        );
        assert_eq!(first_line("a\tb"), "a b");
        assert_eq!(first_line(""), "");
    }
}
