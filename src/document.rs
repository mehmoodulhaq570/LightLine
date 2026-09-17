use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: usize,
    pub byte: usize,
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
    pub cursor: Pos,
    eol: &'static str,
    bom: bool,
    last_saved: Option<(SystemTime, u64)>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    revision: u64,
    saved_revision: u64,
    next_revision: u64,
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
            cursor: Pos::default(),
            eol: "\n",
            bom: false,
            last_saved: None,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            saved_revision: 0,
            next_revision: 1,
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
            cursor: Pos::default(),
            eol,
            bom,
            last_saved: Some((metadata.modified()?, metadata.len())),
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            saved_revision: 0,
            next_revision: 1,
        })
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
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

    pub fn replace(&mut self, start: Pos, end: Pos, replacement: &str) {
        let (start, end) = (self.clamp(start), self.clamp(end));
        if start.line > end.line || (start.line == end.line && start.byte > end.byte) {
            return;
        }
        let replacement = replacement.replace("\r\n", "\n").replace('\r', "\n");
        let old = self.slice(start, end);
        if old == replacement {
            return;
        }
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
        self.cursor = new_end;
        self.revision = after;
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

    pub fn undo(&mut self) {
        if let Some(edit) = self.undo.pop() {
            let end = Self::end_of(edit.start, &edit.new);
            self.cursor = self.replace_raw(edit.start, end, &edit.old);
            self.revision = edit.before;
            self.redo.push(edit);
        }
    }

    pub fn redo(&mut self) {
        if let Some(edit) = self.redo.pop() {
            let end = Self::end_of(edit.start, &edit.old);
            self.cursor = self.replace_raw(edit.start, end, &edit.new);
            self.revision = edit.after;
            self.undo.push(edit);
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
            let name = format!(".my-editor-{}-{n}.tmp", std::process::id());
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
    fn multiline_edit_undo_and_redo() {
        let mut doc = Document::new();
        doc.replace(Pos::default(), Pos::default(), "alpha\nbeta\ngamma");
        doc.replace(Pos { line: 0, byte: 2 }, Pos { line: 1, byte: 2 }, "X\nY");
        assert_eq!(doc.lines, ["alX", "Yta", "gamma"]);
        doc.undo();
        assert_eq!(doc.lines, ["alpha", "beta", "gamma"]);
        doc.redo();
        assert_eq!(doc.lines, ["alX", "Yta", "gamma"]);
    }

    #[test]
    fn dirty_state_stays_correct_after_branching_history() {
        let mut doc = Document::new();
        doc.replace(Pos::default(), Pos::default(), "a");
        doc.saved_revision = doc.revision;
        doc.replace(doc.cursor, doc.cursor, "b");
        doc.undo();
        assert!(!doc.is_dirty());
        doc.replace(doc.cursor, doc.cursor, "c");
        assert!(doc.is_dirty());
        assert_eq!(doc.line(0), "ac");
    }

    #[test]
    fn save_round_trip_preserves_crlf_and_bom() {
        let path = std::env::temp_dir().join(format!("my-editor-test-{}.txt", std::process::id()));
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
}
