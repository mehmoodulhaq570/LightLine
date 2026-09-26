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
    folded_ranges: BTreeSet<(usize, usize)>,
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

    /// True while the file on disk still has the modification time and size
    /// this document last read or wrote, so a watcher event for it is this
    /// document's own save rather than an outside edit.
    pub fn disk_matches_last_save(&self) -> bool {
        let (Some(path), Some((modified, len))) = (&self.path, self.last_saved) else {
            return false;
        };
        fs::metadata(path).is_ok_and(|current| {
            current.len() == len && current.modified().is_ok_and(|time| time == modified)
        })
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

    /// If `line` opens a foldable block, returns the block's last line
    /// (inclusive). Brackets are tried first, then indentation.
    pub fn foldable_range(&self, line: usize) -> Option<usize> {
        if line >= self.lines.len() || self.lines[line].trim().is_empty() {
            return None;
        }
        let bracket = self.bracket_fold_end(line);
        // In bracket languages blocks are delimited by brackets; indentation
        // there is layout (a wrapped argument list, a JSON value), so only
        // indentation-structured files fall back to indentation folding.
        if bracket.is_some() || self.is_bracket_language() {
            return bracket.filter(|end| *end > line);
        }
        self.indent_fold_end(line).filter(|end| *end > line)
    }

    fn is_bracket_language(&self) -> bool {
        let extension = self
            .path
            .as_deref()
            .and_then(Path::extension)
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        matches!(
            extension.as_str(),
            "rs" | "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "cs" | "java" | "kt"
                | "kts" | "go" | "swift" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "mts" | "cts"
                | "tsx" | "json" | "jsonc" | "css" | "scss" | "less" | "php" | "dart" | "scala"
        )
    }

    // Comment syntax is picked from the file extension, so a `#` in CSS or a
    // `//` (floor division) in Python isn't mistaken for a comment.
    fn comment_style(&self) -> (bool, bool) {
        let extension = self
            .path
            .as_deref()
            .and_then(Path::extension)
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let hash = matches!(
            extension.as_str(),
            "py" | "pyw" | "toml" | "yaml" | "yml" | "sh" | "bash" | "rb" | "ps1" | "r" | "pl"
        );
        (!hash, hash)
    }

    // Follows the outermost bracket left open on `line` to the line that
    // closes it. Brackets inside strings and comments are ignored, and a
    // closer of the wrong type (a sign of broken code) gives up instead of
    // guessing a range.
    fn bracket_fold_end(&self, line: usize) -> Option<usize> {
        const MAX_SCAN_LINES: usize = 5_000;
        let (slash_comments, hash_comments) = self.comment_style();
        // Rust lifetimes ('a) would open a never-closed "string".
        let rust = self
            .path
            .as_deref()
            .and_then(Path::extension)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"));
        let mut stack: Vec<u8> = Vec::new();
        let mut in_block_comment = false;
        let last = self.lines.len().min(line + MAX_SCAN_LINES);
        for (index, text) in self.lines[line..last].iter().enumerate() {
            let current = line + index;
            let bytes = text.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if in_block_comment {
                    if bytes[i..].starts_with(b"*/") {
                        in_block_comment = false;
                        i += 2;
                    } else {
                        i += 1;
                    }
                    continue;
                }
                match bytes[i] {
                    b'/' if slash_comments && bytes.get(i + 1) == Some(&b'/') => break,
                    b'/' if slash_comments && bytes.get(i + 1) == Some(&b'*') => {
                        in_block_comment = true;
                        i += 2;
                        continue;
                    }
                    b'#' if hash_comments => break,
                    quote @ (b'"' | b'`' | b'\'') if quote != b'\'' || !rust => {
                        i += 1;
                        while i < bytes.len() && bytes[i] != quote {
                            if bytes[i] == b'\\' {
                                i += 1;
                            }
                            i += 1;
                        }
                    }
                    open @ (b'{' | b'[' | b'(') => stack.push(open),
                    close @ (b'}' | b']' | b')') => {
                        let open = match close {
                            b'}' => b'{',
                            b']' => b'[',
                            _ => b'(',
                        };
                        match stack.last() {
                            // A closer on the first line with nothing open
                            // belongs to an earlier block (`} else {`).
                            None if current == line => {}
                            Some(top) if *top == open => {
                                stack.pop();
                                if stack.is_empty() && current > line {
                                    return Some(current);
                                }
                            }
                            _ => return None,
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            if current == line && stack.is_empty() {
                return None;
            }
        }
        None
    }

    // Python/YAML-style blocks: the run of following lines indented deeper
    // than `line` (blank lines inside the run are included).
    fn indent_fold_end(&self, line: usize) -> Option<usize> {
        let indent = |s: &str| -> usize {
            s.chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .map(|c| if c == '\t' { 4 } else { 1 })
                .sum()
        };
        let base = indent(&self.lines[line]);
        let mut end = None;
        for (offset, text) in self.lines[line + 1..].iter().enumerate() {
            if text.trim().is_empty() {
                continue;
            }
            if indent(text) <= base {
                break;
            }
            end = Some(line + 1 + offset);
        }
        end
    }

    /// Returns the end line if `line` is currently the start of a folded range.
    pub fn is_folded_start(&self, line: usize) -> Option<usize> {
        self.folded_ranges
            .range((line, 0)..=(line, usize::MAX))
            .next()
            .map(|(_, end)| *end)
    }

    /// Checks if a line is hidden inside any currently folded range.
    pub fn is_line_hidden(&self, line: usize) -> bool {
        self.folded_ranges
            .iter()
            .any(|(start, end)| line > *start && line <= *end)
    }

    /// The line that is shown in place of `line`: `line` itself if visible,
    /// otherwise the start line of the outermost fold hiding it.
    pub fn visible_line_for(&self, line: usize) -> usize {
        self.folded_ranges
            .iter()
            .filter(|(start, end)| line > *start && line <= *end)
            .map(|(start, _)| *start)
            .min()
            .unwrap_or(line)
    }

    pub fn has_folds(&self) -> bool {
        !self.folded_ranges.is_empty()
    }

    // The hidden line runs of all folds, merged so nested and overlapping
    // folds count each hidden line once: (first hidden, last hidden).
    fn hidden_runs(&self) -> Vec<(usize, usize)> {
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for &(start, end) in &self.folded_ranges {
            let (first, last) = (start + 1, end);
            match runs.last_mut() {
                Some(run) if first <= run.1 + 1 => run.1 = run.1.max(last),
                _ => runs.push((first, last)),
            }
        }
        runs
    }

    /// Number of rows the document takes on screen: lines minus hidden ones.
    pub fn visible_line_count(&self) -> usize {
        let hidden: usize = self.hidden_runs().iter().map(|(first, last)| last + 1 - first).sum();
        self.lines.len() - hidden.min(self.lines.len())
    }

    /// The screen row of `line` counted from the top of the document, for
    /// the scrollbar. A hidden line reports its fold's row.
    pub fn visual_index(&self, line: usize) -> usize {
        let line = self.visible_line_for(line);
        let hidden_before: usize = self
            .hidden_runs()
            .iter()
            .filter(|(first, _)| *first <= line)
            .map(|(first, last)| (*last).min(line) + 1 - first)
            .sum();
        line - hidden_before
    }

    /// The document line shown on screen row `row` (counted from the top of
    /// the document); the inverse of `visual_index`.
    pub fn line_at_visual_index(&self, row: usize) -> usize {
        let mut line = row;
        for (first, last) in self.hidden_runs() {
            if first <= line {
                line += last + 1 - first;
            } else {
                break;
            }
        }
        line.min(self.lines.len().saturating_sub(1))
    }

    /// Toggles the fold state for `line`.
    /// Returns true if toggled.
    pub fn toggle_fold(&mut self, line: usize) -> bool {
        if let Some(end) = self.is_folded_start(line) {
            self.folded_ranges.remove(&(line, end));
            return true;
        }
        if let Some(end) = self.foldable_range(line) {
            self.folded_ranges.insert((line, end));
            return true;
        }
        false
    }

    /// Unfolds every fold that hides `line`, so a cursor that lands there
    /// (search, go to definition, undo) is never inside collapsed text.
    /// Returns true if anything was unfolded.
    pub fn unfold_to_reveal(&mut self, line: usize) -> bool {
        let before = self.folded_ranges.len();
        self.folded_ranges.retain(|(start, end)| !(line > *start && line <= *end));
        self.folded_ranges.len() != before
    }

    // Keeps folds attached to their text across an edit that replaced lines
    // `start..=old_end` with lines `start..=new_end`: folds after the edit
    // move with it, folds before it stay, and a fold the edit reached into
    // is dropped rather than left covering the wrong lines. Typing on a
    // fold's own (visible) first line keeps the fold.
    fn shift_folds(&mut self, start: usize, old_end: usize, new_end: usize) {
        if self.folded_ranges.is_empty() {
            return;
        }
        let single_line_edit = start == old_end && old_end == new_end;
        self.folded_ranges = std::mem::take(&mut self.folded_ranges)
            .into_iter()
            .filter_map(|(s, e)| {
                if e < start || (single_line_edit && s == start) {
                    Some((s, e))
                } else if s > old_end {
                    Some((s - old_end + new_end, e - old_end + new_end))
                } else {
                    None
                }
            })
            .collect();
    }

    /// Skips past any folded ranges to the next visible line after `line`.
    pub fn next_visible_line(&self, line: usize) -> usize {
        let count = self.line_count();
        if count == 0 {
            return 0;
        }
        let line = self.visible_line_for(line);
        let mut cur = match self.is_folded_start(line) {
            Some(end) => end + 1,
            None => line + 1,
        };
        while cur < count && self.is_line_hidden(cur) {
            cur += 1;
        }
        if cur >= count { line } else { cur }
    }

    /// Skips backward past any folded ranges to the previous visible line before `line`.
    pub fn prev_visible_line(&self, line: usize) -> usize {
        let line = self.visible_line_for(line);
        self.visible_line_for(line.saturating_sub(1))
    }

    /// Moves `rows` visible lines down (positive) or up (negative) from
    /// `line`, stopping at the first or last line.
    pub fn step_visible_lines(&self, line: usize, rows: isize) -> usize {
        let mut current = self.visible_line_for(line);
        for _ in 0..rows.unsigned_abs() {
            let next = if rows > 0 {
                self.next_visible_line(current)
            } else {
                self.prev_visible_line(current)
            };
            if next == current {
                break;
            }
            current = next;
        }
        current
    }

    /// The screen row of `line` when the view starts at `first_line`, or
    /// None if it is above the view or more than `max_rows` rows below it.
    /// A hidden line reports the row of the fold that hides it.
    pub fn visual_row_of(&self, first_line: usize, line: usize, max_rows: usize) -> Option<usize> {
        let first = self.visible_line_for(first_line);
        let target = self.visible_line_for(line);
        if target < first {
            return None;
        }
        if !self.has_folds() {
            return (target - first <= max_rows).then_some(target - first);
        }
        let mut current = first;
        for row in 0..=max_rows {
            if current == target {
                return Some(row);
            }
            let next = self.next_visible_line(current);
            if next == current {
                break;
            }
            current = next;
        }
        None
    }

    /// Maps a visual row offset (from `first_line`) to the actual document line index,
    /// skipping collapsed lines. Returns None if row goes past the document end.
    pub fn visual_row_to_doc_line(&self, first_line: usize, row: usize) -> Option<usize> {
        let count = self.line_count();
        if count == 0 || first_line >= count {
            return None;
        }
        if !self.has_folds() {
            return (first_line + row < count).then_some(first_line + row);
        }
        let mut current = self.visible_line_for(first_line);
        for _ in 0..row {
            let next = self.next_visible_line(current);
            if next == current {
                return None;
            }
            current = next;
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
        let new_end = start.line + new_lines.len() - 1;
        self.lines.splice(start.line..=end.line, new_lines);
        self.shift_folds(start.line, end.line, new_end);
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

    fn doc_with(path: &str, text: &str) -> Document {
        let mut doc = Document::new();
        doc.path = Some(PathBuf::from(path));
        doc.replace(Pos::default(), Pos::default(), text);
        doc
    }

    #[test]
    fn folding_ignores_brackets_in_strings_and_comments_and_matches_types() {
        let doc = doc_with(
            "main.js",
            "function f() {\n  const s = \"}\";\n  // } not a close\n  /* ) */ call(')');\n}\nafter();\n",
        );
        assert_eq!(doc.foldable_range(0), Some(4));

        // Rust lifetimes are not strings.
        let rust = doc_with("lib.rs", "fn get<'a>(x: &'a str) -> &'a str {\n    x\n}\n");
        assert_eq!(rust.foldable_range(0), Some(2));

        // `} else {` folds the else block, and a wrong-type closer gives up.
        let branches = doc_with("a.c", "if (a) {\n  x();\n} else {\n  y();\n}\n");
        assert_eq!(branches.foldable_range(2), Some(4));
        let broken = doc_with("a.c", "f({\n  x\n)}\n");
        assert_eq!(broken.bracket_fold_end(0), None);

        // In Python `#` starts a comment, so its bracket doesn't count.
        let python = doc_with("a.py", "x = 1  # {\ny = 2\n");
        assert_eq!(python.foldable_range(0), None);

        // Non-ASCII text inside comments and strings must not panic.
        let unicode = doc_with("a.js", "f({ /* ü ñ */ s: \"é\",\n  /* ß\n ö */ x: 1\n})\n");
        assert_eq!(unicode.foldable_range(0), Some(3));
    }

    #[test]
    fn nested_folds_count_hidden_lines_once() {
        let mut doc = doc_with("a.rs", "a {\n  b {\n    1\n  }\n}\nz\n");
        assert!(doc.toggle_fold(1));
        assert!(doc.toggle_fold(0));
        assert_eq!(doc.visible_line_count(), 3); // "a {", "z", ""
        assert_eq!(doc.visual_index(5), 1);
        assert_eq!(doc.line_at_visual_index(1), 5);
    }

    #[test]
    fn indentation_folds_only_outside_bracket_languages() {
        // JSON: an indented continuation is layout, not a block.
        let json = doc_with("a.json", "{\"a\": 1,\n\"b\": [1, 2],\n  \"c\": 3\n}\n");
        assert_eq!(json.foldable_range(1), None);
        assert_eq!(json.foldable_range(0), Some(3));
        // YAML and Python fold by indentation.
        let yaml = doc_with("a.yml", "key:\n  child: 1\n  other: 2\nnext: 3\n");
        assert_eq!(yaml.foldable_range(0), Some(2));
        let python = doc_with("a.py", "def f():\n    return 1\n");
        assert_eq!(python.foldable_range(0), Some(1));
    }

    #[test]
    fn folds_move_with_edits_above_and_drop_when_edited_inside() {
        let mut doc = doc_with("a.rs", "fn a() {\n    1\n}\nfn b() {\n    2\n}\n");
        assert!(doc.toggle_fold(3));
        // Insert two lines at the top: the fold moves down with its text.
        doc.replace(Pos::default(), Pos::default(), "// x\n// y\n");
        assert_eq!(doc.is_folded_start(5), Some(7));
        assert!(doc.is_line_hidden(6));
        // Typing on the fold's own first line keeps it.
        doc.replace(Pos { line: 5, byte: 0 }, Pos { line: 5, byte: 0 }, "pub ");
        assert_eq!(doc.is_folded_start(5), Some(7));
        // Undo the typing and the two inserted lines: the fold follows back.
        doc.undo();
        doc.undo();
        assert_eq!(doc.is_folded_start(3), Some(5));
        // An edit that reaches into the folded block drops the fold.
        doc.replace(Pos { line: 2, byte: 0 }, Pos { line: 4, byte: 0 }, "");
        assert!(!doc.has_folds());
    }

    #[test]
    fn visual_rows_skip_folded_lines() {
        let mut doc = doc_with("a.rs", "a {\n1\n2\n}\nb\nc {\n3\n}\nd\n");
        assert!(doc.toggle_fold(0));
        assert!(doc.toggle_fold(5));
        // Rows: a{ b c{ d ""
        assert_eq!(doc.visual_row_of(0, 4, 50), Some(1));
        assert_eq!(doc.visual_row_of(0, 8, 50), Some(3));
        assert_eq!(doc.visual_row_of(0, 8, 2), None);
        assert_eq!(doc.visual_row_to_doc_line(0, 2), Some(5));
        // A hidden line reports its fold's row, and scrolling to it snaps.
        assert_eq!(doc.visible_line_for(2), 0);
        assert_eq!(doc.step_visible_lines(0, 2), 5);
        assert_eq!(doc.step_visible_lines(8, -2), 4);
        assert_eq!(doc.step_visible_lines(0, -5), 0);
        // Scrollbar math: 10 lines, 3 + 2 hidden.
        assert_eq!(doc.visible_line_count(), 5);
        assert_eq!(doc.visual_index(0), 0);
        assert_eq!(doc.visual_index(4), 1);
        assert_eq!(doc.visual_index(8), 3);
        assert_eq!(doc.visual_index(2), 0);
        for row in 0..doc.visible_line_count() {
            assert_eq!(doc.visual_index(doc.line_at_visual_index(row)), row);
        }
        assert_eq!(doc.line_at_visual_index(3), 8);
        // Revealing a hidden line opens only the fold around it.
        assert!(doc.unfold_to_reveal(6));
        assert!(doc.is_folded_start(0).is_some());
        assert!(doc.is_folded_start(5).is_none());
    }
}
