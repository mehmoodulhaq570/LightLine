//! Merge conflicts: the `<<<<<<<` / `=======` / `>>>>>>>` blocks Git leaves
//! in a file it couldn't merge. The `<<<<<<<` line gets Accept Current,
//! Accept Incoming and Accept Both links (drawn as code lens links, see
//! codelens.rs), and the Command Palette can accept every conflict in the
//! file at once or go to the next one. Each is one edit, so Ctrl+Z undoes it.
//!
//! "Current" is the side above `=======` (your branch, `HEAD`); "incoming"
//! is the side below it (what's being merged in). A diff3-style base section
//! (`|||||||`) is dropped whichever side is kept.

use super::*;

/// The farthest a conflict's markers are looked for below its `<<<<<<<`.
const MAX_LINES: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Current,
    Incoming,
    Both,
}

/// One conflict's marker lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Conflict {
    /// `<<<<<<< HEAD`
    pub(super) start: usize,
    /// `||||||| base`, in diff3 style.
    pub(super) base: Option<usize>,
    /// `=======`
    pub(super) middle: usize,
    /// `>>>>>>> branch`
    pub(super) end: usize,
}

fn is_marker(text: &str, marker: &str) -> bool {
    // Seven characters, then the end of the line or a space and a label.
    text.strip_prefix(marker)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
}

pub(super) fn starts_conflict(text: &str) -> bool {
    is_marker(text, "<<<<<<<")
}

/// The conflict whose `<<<<<<<` is on `line`, when its other markers follow
/// in order.
pub(super) fn conflict_at(doc: &Document, line: usize) -> Option<Conflict> {
    if !starts_conflict(doc.line(line)) {
        return None;
    }
    let (mut base, mut middle) = (None, None);
    let last = doc.line_count().min(line + MAX_LINES);
    for at in line + 1..last {
        let text = doc.line(at);
        if starts_conflict(text) {
            return None;
        } else if middle.is_none() && base.is_none() && is_marker(text, "|||||||") {
            base = Some(at);
        } else if middle.is_none() && text.trim_end() == "=======" {
            middle = Some(at);
        } else if is_marker(text, ">>>>>>>") {
            return Some(Conflict {
                start: line,
                base,
                middle: middle?,
                end: at,
            });
        }
    }
    None
}

/// Every conflict in `doc`, top to bottom.
pub(super) fn conflicts(doc: &Document) -> Vec<Conflict> {
    let mut found = Vec::new();
    let mut line = 0;
    while line < doc.line_count() {
        match conflict_at(doc, line) {
            Some(conflict) => {
                line = conflict.end + 1;
                found.push(conflict);
            }
            None => line += 1,
        }
    }
    found
}

/// The edit that resolves `conflict`: the range from its `<<<<<<<` line to
/// its `>>>>>>>` line, line breaks included, and the lines kept.
pub(super) fn resolution(doc: &Document, conflict: Conflict, choice: Choice) -> (Pos, Pos, String) {
    let lines = |from: usize, to: usize| (from..to).map(|at| doc.line(at)).collect::<Vec<_>>();
    let current = lines(conflict.start + 1, conflict.base.unwrap_or(conflict.middle));
    let incoming = lines(conflict.middle + 1, conflict.end);
    let kept = match choice {
        Choice::Current => current,
        Choice::Incoming => incoming,
        Choice::Both => current.into_iter().chain(incoming).collect(),
    };
    let start = Pos {
        line: conflict.start,
        byte: 0,
    };
    if conflict.end + 1 < doc.line_count() {
        // Up to the start of the line after `>>>>>>>`: each kept line keeps
        // its line break.
        let end = Pos {
            line: conflict.end + 1,
            byte: 0,
        };
        let text = kept.iter().map(|line| format!("{line}\n")).collect();
        (start, end, text)
    } else {
        // The file ends with the conflict: no line break after the last one.
        let end = Pos {
            line: conflict.end,
            byte: doc.line(conflict.end).len(),
        };
        (start, end, kept.join("\n"))
    }
}

impl App {
    /// Resolves the conflict starting on `line` of the active file.
    pub(super) fn accept_conflict(&mut self, hwnd: HWND, line: usize, choice: Choice) {
        let Some(conflict) = conflict_at(self.doc(), line) else {
            return;
        };
        let (start, end, text) = resolution(self.doc(), conflict, choice);
        self.doc_mut().begin_group();
        self.replace_range(start, end, &text);
        self.doc_mut().end_group();
        self.move_cursor(start, false);
        self.keep_cursor_visible(hwnd);
        let left = conflicts(self.doc()).len();
        self.status = match left {
            0 => "No conflicts left in this file: save it, then stage it in Source Control".into(),
            1 => "1 conflict left in this file".into(),
            n => format!("{n} conflicts left in this file"),
        };
        self.refresh(hwnd);
    }

    /// Resolves every conflict in the active file the same way, as one edit.
    pub(super) fn accept_all_conflicts(&mut self, hwnd: HWND, choice: Choice) {
        let found = conflicts(self.doc());
        if found.is_empty() {
            self.status = "No merge conflicts in this file".into();
            self.refresh(hwnd);
            return;
        }
        // Bottom first, so the lines above keep their numbers.
        let edits: Vec<(Pos, Pos, String)> = found
            .iter()
            .rev()
            .map(|conflict| resolution(self.doc(), *conflict, choice))
            .collect();
        self.doc_mut().begin_group();
        for (start, end, text) in edits {
            self.replace_range(start, end, &text);
        }
        self.doc_mut().end_group();
        let first = Pos {
            line: found[0]
                .start
                .min(self.doc().line_count().saturating_sub(1)),
            byte: 0,
        };
        self.move_cursor(first, false);
        self.keep_cursor_visible(hwnd);
        self.status = match found.len() {
            1 => "Resolved 1 conflict: save the file, then stage it".into(),
            n => format!("Resolved {n} conflicts: save the file, then stage it"),
        };
        self.refresh(hwnd);
    }

    /// Moves the caret to the next conflict's `<<<<<<<` line, round to the
    /// first after the last.
    pub(super) fn next_conflict(&mut self, hwnd: HWND) {
        let found = conflicts(self.doc());
        let caret = self.view().cursor.line;
        let Some(next) = found
            .iter()
            .find(|conflict| conflict.start > caret)
            .or(found.first())
        else {
            self.status = "No merge conflicts in this file".into();
            self.refresh(hwnd);
            return;
        };
        let index = found.iter().position(|c| c == next).unwrap_or(0) + 1;
        self.move_cursor(
            Pos {
                line: next.start,
                byte: 0,
            },
            false,
        );
        self.keep_cursor_visible(hwnd);
        self.status = format!("Conflict {index} of {}", found.len());
        self.refresh(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        let mut doc = Document::new();
        doc.replace(Pos::default(), Pos::default(), text);
        doc
    }

    fn resolved(text: &str, choice: Choice) -> String {
        let mut doc = doc(text);
        let conflict = conflict_at(&doc, 1).unwrap();
        let (start, end, kept) = resolution(&doc, conflict, choice);
        doc.replace(start, end, &kept);
        doc.text()
    }

    const TEXT: &str = "a\n<<<<<<< HEAD\nmine\n=======\ntheirs\nmore\n>>>>>>> feature\nz\n";

    #[test]
    fn finds_a_conflicts_markers() {
        let doc = doc(TEXT);
        assert_eq!(
            conflict_at(&doc, 1),
            Some(Conflict {
                start: 1,
                base: None,
                middle: 3,
                end: 6
            })
        );
        assert_eq!(conflict_at(&doc, 0), None);
        assert_eq!(conflicts(&doc).len(), 1);
    }

    #[test]
    fn each_choice_keeps_its_side() {
        assert_eq!(resolved(TEXT, Choice::Current), "a\nmine\nz\n");
        assert_eq!(resolved(TEXT, Choice::Incoming), "a\ntheirs\nmore\nz\n");
        assert_eq!(resolved(TEXT, Choice::Both), "a\nmine\ntheirs\nmore\nz\n");
    }

    #[test]
    fn a_diff3_base_is_dropped() {
        let text = "a\n<<<<<<< HEAD\nmine\n||||||| base\nold\n=======\ntheirs\n>>>>>>> b\n";
        assert_eq!(resolved(text, Choice::Current), "a\nmine\n");
        assert_eq!(resolved(text, Choice::Both), "a\nmine\ntheirs\n");
    }

    #[test]
    fn a_conflict_at_the_end_of_the_file() {
        let text = "a\n<<<<<<< HEAD\nmine\n=======\ntheirs\n>>>>>>> b";
        assert_eq!(resolved(text, Choice::Incoming), "a\ntheirs");
        let text = "a\n<<<<<<< HEAD\n=======\ntheirs\n>>>>>>> b";
        assert_eq!(resolved(text, Choice::Current), "a\n");
    }

    #[test]
    fn unfinished_or_lookalike_markers_are_not_conflicts() {
        assert!(conflicts(&doc("<<<<<<< HEAD\nmine\n=======\n")).is_empty());
        assert!(conflicts(&doc("<<<<<<<< x\n=======\n>>>>>>> y\n")).is_empty());
        assert!(conflicts(&doc("<<<<<<< a\n>>>>>>> b\n")).is_empty());
    }
}
