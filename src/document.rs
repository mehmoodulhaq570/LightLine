use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: usize,
    pub byte: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextChange {
    pub serial: u64,
    pub start: Pos,
    pub end: Pos,
    pub start_utf16: usize,
    pub end_utf16: usize,
    pub text: String,
}

#[derive(Clone, Debug)]
struct Edit {
    start: Pos,
    old: String,
    new: String,
    before: u64,
    after: u64,
}

pub struct Document {
    lines: Vec<String>,
    pub path: Option<PathBuf>,
    eol: &'static str,
    bom: bool,
    last_saved: Option<(SystemTime, u64)>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    revision: u64,
    saved_revision: u64,
    next_revision: u64,
    change_serial: u64,
    last_change: Option<TextChange>,
    // Zero-based line numbers with a breakpoint set from the gutter.
    breakpoints: BTreeSet<usize>,
    // Collapsed line ranges: (start_line, end_line), inclusive.
    // Lines (start_line + 1)..=end_line are hidden from view.
    pub folded_ranges: BTreeSet<(usize, usize)>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            path: None,
            eol: "\n",
            bom: false,
            last_saved: None,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            saved_revision: 0,
            next_revision: 1,
            change_serial: 0,
            last_change: None,
            breakpoints: BTreeSet::new(),
            folded_ranges: BTreeSet::new(),
        }
    }

    pub fn open(path: PathBuf) -> io::Result<Self> {
        let bytes = fs::read(&path)?;
        let bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
        let text = std::str::from_utf8(if bom { &bytes[3..] } else { &bytes }).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "Only UTF-8 files are supported")
        })?;
        let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let normalized = text.replace("\r\n", "\n");
        let metadata = fs::metadata(&path)?;
        Ok(Self {
            lines: normalized.split('\n').map(str::to_owned).collect(),
            path: Some(path),
            eol,
            bom,
            last_saved: Some((metadata.modified()?, metadata.len())),
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            saved_revision: 0,
            next_revision: 1,
            change_serial: 0,
            last_change: None,
            breakpoints: BTreeSet::new(),
            folded_ranges: BTreeSet::new(),
        })
    }

    // Sets the initial content of a fresh, non-file-backed document (e.g. a
    // generated read-only preview) directly, bypassing undo tracking. Only
    // meaningful before any real edit has happened.
    pub fn seed(&mut self, text: &str) {
        self.lines = text.split('\n').map(str::to_owned).collect();
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
    pub fn lines(&self) -> &[String] {
        &self.lines
    }
    pub fn line(&self, index: usize) -> &str {
        &self.lines[index]
    }
    pub fn end(&self) -> Pos {
        let line = self.lines.len() - 1;
        Pos {
            line,
            byte: self.lines[line].len(),
        }
    }
    pub fn text_range(&self, start: Pos, end: Pos) -> String {
        let start = self.clamp(start);
        let end = self.clamp(end);
        if start > end {
            return String::new();
        }
        self.slice(start, end)
    }
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn mark_clean(&mut self) {
        self.saved_revision = self.revision;
    }

    pub fn breakpoints(&self) -> &BTreeSet<usize> {
        &self.breakpoints
    }

    pub fn has_breakpoint(&self, line: usize) -> bool {
        self.breakpoints.contains(&line)
    }

    // Returns true if the breakpoint is now set, false if it was cleared.
    pub fn toggle_breakpoint(&mut self, line: usize) -> bool {
        if line >= self.lines.len() {
            return false;
        }
        if !self.breakpoints.insert(line) {
            self.breakpoints.remove(&line);
            false
        } else {
            true
        }
    }

    /// Checks if a line is the start of a foldable block and returns the end line (inclusive).
    pub fn foldable_range(&self, line: usize) -> Option<usize> {
        let total_lines = self.lines.len();
        if line >= total_lines {
            return None;
        }
        let line_text = &self.lines[line];
        let trimmed = line_text.trim();
        if trimmed.is_empty() {
            return None;
        }

        // 1. Bracket-based folding: Count open vs close braces { [ (
        let mut brace_depth: i32 = 0;
        for ch in line_text.chars() {
            match ch {
                '{' | '[' | '(' => brace_depth += 1,
                '}' | ']' | ')' => brace_depth -= 1,
                _ => {}
            }
        }

        if brace_depth > 0 {
            let mut depth = brace_depth;
            for next in (line + 1)..total_lines {
                for ch in self.lines[next].chars() {
                    match ch {
                        '{' | '[' | '(' => depth += 1,
                        '}' | ']' | ')' => {
                            depth -= 1;
                            if depth <= 0 {
                                return Some(next);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // 2. Indentation-based folding (Python, YAML, and general multi-line blocks):
        let get_indent = |s: &str| -> usize {
            let mut indent = 0;
            for ch in s.chars() {
                if ch == ' ' {
                    indent += 1;
                } else if ch == '\t' {
                    indent += 4;
                } else {
                    break;
                }
            }
            indent
        };

        let base_indent = get_indent(line_text);
        let mut last_indented_line = None;
        for next in (line + 1)..total_lines {
            let next_text = &self.lines[next];
            if next_text.trim().is_empty() {
                continue;
            }
            let next_indent = get_indent(next_text);
            if next_indent > base_indent {
                last_indented_line = Some(next);
            } else {
                break;
            }
        }

        if let Some(end) = last_indented_line {
            if end > line {
                return Some(end);
            }
        }

        None
    }

    /// Returns the end line if `line` is currently the start of a folded range.
    pub fn is_folded_start(&self, line: usize) -> Option<usize> {
        self.folded_ranges
            .iter()
            .find(|(start, _)| *start == line)
            .map(|(_, end)| *end)
    }

    /// Checks if a line is hidden inside any currently folded range.
    pub fn is_line_hidden(&self, line: usize) -> bool {
        self.folded_ranges
            .iter()
            .any(|(start, end)| line > *start && line <= *end)
    }

    /// Toggles the fold state for `line`.
    /// Returns true if toggled.
    pub fn toggle_fold(&mut self, line: usize) -> bool {
        if let Some(range) = self.folded_ranges.iter().copied().find(|(s, _)| *s == line) {
            self.folded_ranges.remove(&range);
            return true;
        }
        if let Some(end) = self.foldable_range(line) {
            if end > line {
                self.folded_ranges.insert((line, end));
                return true;
            }
        }
        false
    }

    /// Skips past any folded ranges to the next visible line after `line`.
    pub fn next_visible_line(&self, line: usize) -> usize {
        let count = self.line_count();
        if count == 0 {
            return 0;
        }
        let mut cur = if let Some(end) = self.is_folded_start(line) {
            end.saturating_add(1)
        } else {
            line.saturating_add(1)
        };
        while cur < count && self.is_line_hidden(cur) {
            cur = cur.saturating_add(1);
        }
        cur.min(count.saturating_sub(1))
    }

    /// Skips backward past any folded ranges to the previous visible line before `line`.
    pub fn prev_visible_line(&self, line: usize) -> usize {
        let mut cur = line.saturating_sub(1);
        while cur > 0 && self.is_line_hidden(cur) {
            cur = cur.saturating_sub(1);
        }
        cur
    }

    /// Maps a visual row offset (from `first_line`) to the actual document line index,
    /// skipping collapsed lines. Returns None if row goes past the document end.
    pub fn visual_row_to_doc_line(&self, first_line: usize, row: usize) -> Option<usize> {
        let count = self.line_count();
        if count == 0 || first_line >= count {
            return None;
        }
        let mut current = first_line;
        for (s, e) in &self.folded_ranges {
            if current > *s && current <= *e {
                current = *s;
                break;
            }
        }
        for _ in 0..row {
            if let Some(end) = self.is_folded_start(current) {
                current = end + 1;
            } else {
                current += 1;
            }
            if current >= count {
                return None;
            }
        }
        Some(current)
    }

    pub fn change_serial(&self) -> u64 {
        self.change_serial
    }

    pub fn last_change(&self) -> Option<&TextChange> {
        self.last_change.as_ref()
    }

    pub fn byte_len(&self) -> usize {
        self.lines.iter().map(String::len).sum::<usize>() + self.lines.len() - 1
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn utf16_column(&self, pos: Pos) -> usize {
        let pos = self.clamp(pos);
        self.lines[pos.line][..pos.byte].encode_utf16().count()
    }

    fn record_change(&mut self, start: Pos, end: Pos, text: String) {
        let start_utf16 = self.utf16_column(start);
        let end_utf16 = self.utf16_column(end);
        self.change_serial += 1;
        self.last_change = Some(TextChange {
            serial: self.change_serial,
            start,
            end,
            start_utf16,
            end_utf16,
            text,
        });
    }

    pub fn clamp(&self, mut pos: Pos) -> Pos {
        pos.line = pos.line.min(self.lines.len() - 1);
        pos.byte = pos.byte.min(self.lines[pos.line].len());
        while !self.lines[pos.line].is_char_boundary(pos.byte) {
            pos.byte -= 1;
        }
        pos
    }

    pub fn previous(&self, pos: Pos) -> Pos {
        let pos = self.clamp(pos);
        if pos.byte > 0 {
            let byte = self.lines[pos.line][..pos.byte]
                .char_indices()
                .last()
                .unwrap()
                .0;
            Pos {
                line: pos.line,
                byte,
            }
        } else if pos.line > 0 {
            Pos {
                line: pos.line - 1,
                byte: self.lines[pos.line - 1].len(),
            }
        } else {
            pos
        }
    }

    pub fn next(&self, pos: Pos) -> Pos {
        let pos = self.clamp(pos);
        if pos.byte < self.lines[pos.line].len() {
            let ch = self.lines[pos.line][pos.byte..].chars().next().unwrap();
            Pos {
                line: pos.line,
                byte: pos.byte + ch.len_utf8(),
            }
        } else if pos.line + 1 < self.lines.len() {
            Pos {
                line: pos.line + 1,
                byte: 0,
            }
        } else {
            pos
        }
    }

    fn char_before(&self, pos: Pos) -> Option<char> {
        let pos = self.clamp(pos);
        if pos.byte > 0 {
            self.lines[pos.line][..pos.byte].chars().next_back()
        } else if pos.line > 0 {
            Some('\n')
        } else {
            None
        }
    }

    fn char_at(&self, pos: Pos) -> Option<char> {
        let pos = self.clamp(pos);
        self.lines[pos.line][pos.byte..]
            .chars()
            .next()
            .or_else(|| (pos.line + 1 < self.lines.len()).then_some('\n'))
    }

    pub fn previous_word(&self, pos: Pos) -> Pos {
        let mut pos = self.clamp(pos);
        while self.char_before(pos).is_some_and(char::is_whitespace) {
            pos = self.previous(pos);
        }
        let Some(first) = self.char_before(pos) else {
            return pos;
        };
        let word = first.is_alphanumeric() || first == '_';
        while self
            .char_before(pos)
            .is_some_and(|ch| !ch.is_whitespace() && (ch.is_alphanumeric() || ch == '_') == word)
        {
            pos = self.previous(pos);
        }
        pos
    }

    pub fn next_word(&self, pos: Pos) -> Pos {
        let mut pos = self.clamp(pos);
        if let Some(first) = self.char_at(pos)
            && !first.is_whitespace()
        {
            let word = first.is_alphanumeric() || first == '_';
            while self.char_at(pos).is_some_and(|ch| {
                !ch.is_whitespace() && (ch.is_alphanumeric() || ch == '_') == word
            }) {
                pos = self.next(pos);
            }
        }
        while self.char_at(pos).is_some_and(char::is_whitespace) {
            pos = self.next(pos);
        }
        pos
    }

    pub fn find_forward(&self, origin: Pos, query: &str) -> Option<Pos> {
        if query.is_empty() {
            return None;
        }
        let origin = self.clamp(origin);
        let count = self.line_count();
        (0..=count).find_map(|step| {
            let line = (origin.line + step) % count;
            let text = self.line(line);
            let from = if step == 0 { origin.byte } else { 0 };
            let to = if step == count {
                origin.byte
            } else {
                text.len()
            };
            text.get(from..to)?.find(query).map(|offset| Pos {
                line,
                byte: from + offset,
            })
        })
    }

    pub fn find_backward(&self, origin: Pos, query: &str) -> Option<Pos> {
        if query.is_empty() {
            return None;
        }
        let origin = self.clamp(origin);
        let count = self.line_count();
        (0..=count).find_map(|step| {
            let line = (origin.line + count - step % count) % count;
            let text = self.line(line);
            let from = if step == count { origin.byte } else { 0 };
            let to = if step == 0 { origin.byte } else { text.len() };
            text.get(from..to)?.rfind(query).map(|offset| Pos {
                line,
                byte: from + offset,
            })
        })
    }

    fn slice(&self, start: Pos, end: Pos) -> String {
        if start.line == end.line {
            return self.lines[start.line][start.byte..end.byte].to_owned();
        }
        let mut text = self.lines[start.line][start.byte..].to_owned();
        for line in start.line + 1..end.line {
            text.push('\n');
            text.push_str(&self.lines[line]);
        }
        text.push('\n');
        text.push_str(&self.lines[end.line][..end.byte]);
        text
    }

    fn replace_raw(&mut self, start: Pos, end: Pos, replacement: &str) -> Pos {
        let prefix = self.lines[start.line][..start.byte].to_owned();
        let suffix = self.lines[end.line][end.byte..].to_owned();
        let parts: Vec<&str> = replacement.split('\n').collect();
        let cursor = if parts.len() == 1 {
            Pos {
                line: start.line,
                byte: prefix.len() + parts[0].len(),
            }
        } else {
            Pos {
                line: start.line + parts.len() - 1,
                byte: parts.last().unwrap().len(),
            }
        };
        let mut new_lines: Vec<String> = parts.iter().map(|s| (*s).to_owned()).collect();
        new_lines[0].insert_str(0, &prefix);
        new_lines.last_mut().unwrap().push_str(&suffix);
        self.lines.splice(start.line..=end.line, new_lines);
        cursor
    }

    pub fn replace(&mut self, start: Pos, end: Pos, replacement: &str) -> Pos {
        let (start, end) = (self.clamp(start), self.clamp(end));
        if start.line > end.line || (start.line == end.line && start.byte > end.byte) {
            return start;
        }
        let replacement = replacement.replace("\r\n", "\n").replace('\r', "\n");
        let old = self.slice(start, end);
        if old == replacement {
            return end;
        }
        self.record_change(start, end, replacement.clone());
        let new_end = self.replace_raw(start, end, &replacement);
        let after = self.next_revision;
        self.next_revision += 1;
        self.undo.push(Edit {
            start,
            old,
            new: replacement,
            before: self.revision,
            after,
        });
        self.redo.clear();
        self.revision = after;
        new_end
    }

    fn end_of(start: Pos, text: &str) -> Pos {
        let count = text.bytes().filter(|b| *b == b'\n').count();
        if count == 0 {
            Pos {
                line: start.line,
                byte: start.byte + text.len(),
            }
        } else {
            Pos {
                line: start.line + count,
                byte: text.rsplit('\n').next().unwrap().len(),
            }
        }
    }

    pub fn undo(&mut self) -> Option<(Pos, usize)> {
        if let Some(edit) = self.undo.pop() {
            let line = edit.start.line;
            let end = Self::end_of(edit.start, &edit.new);
            self.record_change(edit.start, end, edit.old.clone());
            let cursor = self.replace_raw(edit.start, end, &edit.old);
            self.revision = edit.before;
            self.redo.push(edit);
            Some((cursor, line))
        } else {
            None
        }
    }

    pub fn redo(&mut self) -> Option<(Pos, usize)> {
        if let Some(edit) = self.redo.pop() {
            let line = edit.start.line;
            let end = Self::end_of(edit.start, &edit.old);
            self.record_change(edit.start, end, edit.new.clone());
            let cursor = self.replace_raw(edit.start, end, &edit.new);
            self.revision = edit.after;
            self.undo.push(edit);
            Some((cursor, line))
        } else {
            None
        }
    }

    pub fn save(&mut self, path: &Path) -> io::Result<()> {
        if self.path.as_deref() == Some(path)
            && let Some((modified, len)) = self.last_saved
        {
            let current = fs::metadata(path)?;
            if current.modified()? != modified || current.len() != len {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "File changed on disk; save to a different path",
                ));
            }
        }
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut temporary = None;
        for n in 0..100 {
            let name = format!(".lightline-{}-{n}.tmp", std::process::id());
            let candidate = parent.join(name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(file) => {
                    temporary = Some((candidate, file));
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        let (temp_path, mut file) = temporary.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Could not create temporary save file",
            )
        })?;
        let write_result = (|| -> io::Result<()> {
            if self.bom {
                file.write_all(&[0xef, 0xbb, 0xbf])?;
            }
            for (i, line) in self.lines.iter().enumerate() {
                if i > 0 {
                    file.write_all(self.eol.as_bytes())?;
                }
                file.write_all(line.as_bytes())?;
            }
            file.sync_all()?;
            Ok(())
        })();
        drop(file);
        let result = write_result.and_then(|_| replace_file(&temp_path, path));
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result?;
        let metadata = fs::metadata(path)?;
        self.path = Some(path.to_owned());
        self.last_saved = Some((metadata.modified()?, metadata.len()));
        self.saved_revision = self.revision;
        Ok(())
    }
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let (from, to) = (wide(from), wide(to));
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edit_records_utf16_ranges_for_replace_undo_and_redo() {
        let mut doc = Document::new();
        doc.replace(Pos::default(), Pos::default(), "a🦀z");
        let start = Pos { line: 0, byte: 5 };
        let end = Pos { line: 0, byte: 6 };
        doc.replace(start, end, "é");
        let change = doc.last_change().unwrap();
        assert_eq!((change.start_utf16, change.end_utf16), (3, 4));
        assert_eq!(change.text, "é");
        let serial = change.serial;
        doc.undo();
        let undone = doc.last_change().unwrap();
        assert_eq!((undone.start_utf16, undone.end_utf16), (3, 4));
        assert_eq!(undone.text, "z");
        assert!(undone.serial > serial);
        doc.redo();
        assert_eq!(doc.last_change().unwrap().text, "é");
    }

    #[test]
    fn multiline_edit_undo_and_redo() {
        let mut doc = Document::new();
        doc.replace(Pos::default(), Pos::default(), "alpha\nbeta\ngamma");
        assert_eq!(
            doc.replace(Pos { line: 0, byte: 2 }, Pos { line: 1, byte: 2 }, "X\nY"),
            Pos { line: 1, byte: 1 }
        );
        assert_eq!(doc.lines, ["alX", "Yta", "gamma"]);
        assert_eq!(doc.undo(), Some((Pos { line: 1, byte: 2 }, 0)));
        assert_eq!(doc.lines, ["alpha", "beta", "gamma"]);
        assert_eq!(doc.redo(), Some((Pos { line: 1, byte: 1 }, 0)));
        assert_eq!(doc.lines, ["alX", "Yta", "gamma"]);
    }

    #[test]
    fn dirty_state_stays_correct_after_branching_history() {
        let mut doc = Document::new();
        doc.replace(Pos::default(), Pos::default(), "a");
        doc.saved_revision = doc.revision;
        doc.replace(doc.end(), doc.end(), "b");
        let cursor = doc.undo().unwrap().0;
        assert!(!doc.is_dirty());
        doc.replace(cursor, cursor, "c");
        assert!(doc.is_dirty());
        assert_eq!(doc.line(0), "ac");
    }

    #[test]
    fn save_round_trip_preserves_crlf_and_bom() {
        let path = std::env::temp_dir().join(format!("lightline-test-{}.txt", std::process::id()));
        std::fs::write(&path, b"\xef\xbb\xbfhello\r\nworld").unwrap();
        let mut doc = Document::open(path.clone()).unwrap();
        doc.replace(Pos { line: 1, byte: 5 }, Pos { line: 1, byte: 5 }, "!");
        doc.save(&path).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"\xef\xbb\xbfhello\r\nworld!"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn word_navigation_crosses_punctuation_and_lines() {
        let mut doc = Document::new();
        doc.replace(Pos::default(), Pos::default(), "one.two\nthree four");
        assert_eq!(
            doc.previous_word(Pos { line: 1, byte: 5 }),
            Pos { line: 1, byte: 0 }
        );
        assert_eq!(
            doc.previous_word(Pos { line: 1, byte: 0 }),
            Pos { line: 0, byte: 4 }
        );
        assert_eq!(
            doc.next_word(Pos { line: 0, byte: 0 }),
            Pos { line: 0, byte: 3 }
        );
        assert_eq!(
            doc.next_word(Pos { line: 0, byte: 7 }),
            Pos { line: 1, byte: 0 }
        );
    }

    #[test]
    fn search_wraps_and_keeps_utf8_byte_positions() {
        let mut doc = Document::new();
        doc.replace(Pos::default(), Pos::default(), "αbc\nsecond αbc");
        assert_eq!(
            doc.find_forward(Pos { line: 0, byte: 2 }, "bc"),
            Some(Pos { line: 0, byte: 2 })
        );
        assert_eq!(doc.find_forward(doc.end(), "αbc"), Some(Pos::default()));
        assert_eq!(
            doc.find_backward(Pos::default(), "αbc"),
            Some(Pos { line: 1, byte: 7 })
        );
        assert_eq!(doc.find_forward(Pos::default(), "ABC"), None);
    }

    #[test]
    fn test_code_folding_bracket_and_indentation() {
        let mut doc = Document::new();
        let code = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n\ndef python_func():\n    a = 10\n    b = 20\n\nlet single = 5;\n";
        doc.replace(Pos::default(), Pos::default(), code);

        // 1. Rust function bracket block
        assert_eq!(doc.foldable_range(0), Some(3));
        assert!(doc.toggle_fold(0));
        assert!(doc.is_folded_start(0).is_some());
        assert!(doc.is_line_hidden(1));
        assert!(doc.is_line_hidden(2));
        assert!(doc.is_line_hidden(3));
        assert!(!doc.is_line_hidden(0));
        assert!(!doc.is_line_hidden(4));

        // Visual mapping: row 0 is line 0, row 1 is line 4 (lines 1..3 hidden)
        assert_eq!(doc.visual_row_to_doc_line(0, 0), Some(0));
        assert_eq!(doc.visual_row_to_doc_line(0, 1), Some(4));

        // Navigation
        assert_eq!(doc.next_visible_line(0), 4);
        assert_eq!(doc.prev_visible_line(4), 0);

        // Toggle back to unfold
        assert!(doc.toggle_fold(0));
        assert!(doc.is_folded_start(0).is_none());
        assert!(!doc.is_line_hidden(1));

        // 2. Python indentation block
        assert_eq!(doc.foldable_range(5), Some(7));

        // 3. Single line statement (no fold)
        assert_eq!(doc.foldable_range(9), None);
    }
}
