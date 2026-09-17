//! Rust colors from Tree-sitter for ordinary files, with bounded lexical fallback for large files.

use crate::document::Document;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use tree_sitter::{InputEdit, Parser, Point, Query, QueryCursor, StreamingIterator, Tree};

const PARSE_LIMIT: usize = 128 * 1024;

fn within_parse_limit(document: &Document) -> bool {
    let mut bytes = 0;
    (0..document.line_count()).all(|line| {
        bytes += document.line(line).len() + 1;
        bytes <= PARSE_LIMIT
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Comment,
    String,
    Keyword,
    Type,
    Number,
    Macro,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub color: Color,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Normal,
    BlockComment(u32),
    String,
    RawString(usize),
}

pub struct RustSyntax {
    // states[i] is the lexer state at the beginning of line i.
    states: Vec<State>,
    worker: Option<Worker>,
    tree_spans: Option<Vec<Vec<Span>>>,
    parser_attempted: bool,
    dirty: bool,
    revision: u64,
}

struct Worker {
    jobs: Sender<Job>,
    results: Receiver<ResultSet>,
}

struct Job {
    revision: u64,
    source: String,
}

struct ResultSet {
    revision: u64,
    spans: Option<Vec<Vec<Span>>>,
}

struct ParsedRust {
    parser: Parser,
    tree: Tree,
    query: Query,
    source: String,
}

impl ParsedRust {
    fn new(source: String) -> Option<Self> {
        let language = tree_sitter_rust::LANGUAGE.into();
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;
        let query = Query::new(&language, tree_sitter_rust::HIGHLIGHTS_QUERY).ok()?;
        let tree = parser.parse(&source, None)?;
        Some(Self {
            parser,
            tree,
            query,
            source,
        })
    }

    fn refresh(&mut self, next: String) -> bool {
        if next == self.source {
            return true;
        }
        let before = self.source.as_bytes();
        let after = next.as_bytes();
        let mut start = before.iter().zip(after).take_while(|(a, b)| a == b).count();
        while !self.source.is_char_boundary(start) || !next.is_char_boundary(start) {
            start -= 1;
        }
        let mut suffix = before[start..]
            .iter()
            .rev()
            .zip(after[start..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        while !self.source.is_char_boundary(before.len() - suffix)
            || !next.is_char_boundary(after.len() - suffix)
        {
            suffix -= 1;
        }
        let old_end = before.len() - suffix;
        let new_end = after.len() - suffix;
        let edit = InputEdit {
            start_byte: start,
            old_end_byte: old_end,
            new_end_byte: new_end,
            start_position: point_at(&self.source, start),
            old_end_position: point_at(&self.source, old_end),
            new_end_position: point_at(&next, new_end),
        };
        self.tree.edit(&edit);
        let Some(tree) = self.parser.parse(&next, Some(&self.tree)) else {
            return false;
        };
        self.tree = tree;
        self.source = next;
        true
    }

    fn all_spans(&self) -> Vec<Vec<Span>> {
        let lines: Vec<&str> = self.source.split('\n').collect();
        let mut output = vec![Vec::new(); lines.len()];
        let mut cursor = QueryCursor::new();
        let mut captures =
            cursor.captures(&self.query, self.tree.root_node(), self.source.as_bytes());
        while let Some((matched, capture_index)) = captures.next() {
            let capture = matched.captures()[*capture_index];
            let name = self.query.capture_names()[capture.index as usize];
            let color = if name.starts_with("comment") {
                Some(Color::Comment)
            } else if name.starts_with("string") || name.starts_with("character") {
                Some(Color::String)
            } else if name.starts_with("keyword") || name == "boolean" {
                Some(Color::Keyword)
            } else if name.starts_with("type") || name == "constructor" {
                Some(Color::Type)
            } else if name.starts_with("number") || name.starts_with("constant") {
                Some(Color::Number)
            } else if name == "function.macro" {
                Some(Color::Macro)
            } else {
                None
            };
            let Some(color) = color else {
                continue;
            };
            let start = capture.node.start_position();
            let end = capture.node.end_position();
            for line in start.row..=end.row.min(lines.len().saturating_sub(1)) {
                let source = lines[line];
                let from = if start.row == line { start.column } else { 0 };
                let to = if end.row == line {
                    end.column
                } else {
                    source.len()
                };
                if from < to
                    && to <= source.len()
                    && source.is_char_boundary(from)
                    && source.is_char_boundary(to)
                {
                    output[line].push(Span {
                        start: from,
                        end: to,
                        color,
                    });
                }
            }
        }
        output
    }
}

impl Worker {
    fn start() -> Self {
        let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
        let (results_tx, results_rx) = mpsc::channel::<ResultSet>();
        thread::spawn(move || {
            let mut parsed: Option<ParsedRust> = None;
            while let Ok(mut job) = jobs_rx.recv() {
                while let Ok(newer) = jobs_rx.try_recv() {
                    job = newer;
                }
                let okay = if let Some(parser) = &mut parsed {
                    parser.refresh(job.source)
                } else {
                    parsed = ParsedRust::new(job.source);
                    parsed.is_some()
                };
                let spans = if okay {
                    parsed.as_ref().map(ParsedRust::all_spans)
                } else {
                    None
                };
                if results_tx
                    .send(ResultSet {
                        revision: job.revision,
                        spans,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            jobs: jobs_tx,
            results: results_rx,
        }
    }
}

fn point_at(source: &str, byte: usize) -> Point {
    let prefix = &source.as_bytes()[..byte];
    let row = prefix.iter().filter(|b| **b == b'\n').count();
    let column = prefix
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(prefix.len(), |index| prefix.len() - index - 1);
    Point::new(row, column)
}

impl Default for RustSyntax {
    fn default() -> Self {
        Self::new()
    }
}

impl RustSyntax {
    pub fn new() -> Self {
        Self {
            states: vec![State::Normal],
            worker: None,
            tree_spans: None,
            parser_attempted: false,
            dirty: false,
            revision: 0,
        }
    }

    pub fn invalidate_from(&mut self, line: usize) {
        self.states.truncate((line + 1).max(1));
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
        self.tree_spans = None;
    }

    /// Scan at most `budget` fallback lines and poll the background parser.
    /// Returns false while either path has more work for the requested viewport.
    pub fn advance_to(&mut self, document: &Document, target: usize, budget: usize) -> bool {
        if !self.parser_attempted {
            self.parser_attempted = true;
            if within_parse_limit(document) {
                self.worker = Some(Worker::start());
                self.dirty = true;
            }
        }
        if self.worker.is_some() {
            if self.dirty {
                if !within_parse_limit(document) {
                    self.worker = None;
                } else {
                    let job = Job {
                        revision: self.revision,
                        source: document.text_range(Default::default(), document.end()),
                    };
                    if self.worker.as_ref().unwrap().jobs.send(job).is_err() {
                        self.worker = None;
                    }
                }
                self.dirty = false;
            }
            if let Some(worker) = &self.worker {
                loop {
                    match worker.results.try_recv() {
                        Ok(result) if result.revision == self.revision => {
                            self.tree_spans = result.spans;
                            if self.tree_spans.is_none() {
                                self.worker = None;
                            }
                            break;
                        }
                        Ok(_) => continue,
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            self.worker = None;
                            break;
                        }
                    }
                }
            }
            if self.tree_spans.is_some() {
                return true;
            }
        }
        let target = target.min(document.line_count().saturating_sub(1));
        let mut scanned_bytes = 0;
        for _ in 0..budget {
            if self.states.len() > target {
                break;
            }
            if scanned_bytes >= 256 * 1024 {
                break;
            }
            let line = self.states.len() - 1;
            let source = document.line(line);
            scanned_bytes += source.len();
            let next = scan(source, self.states[line], None);
            self.states.push(next);
        }
        self.states.len() > target && self.worker.is_none()
    }

    pub fn spans(&self, document: &Document, line: usize) -> Vec<Span> {
        if let Some(spans) = &self.tree_spans {
            return spans.get(line).cloned().unwrap_or_default();
        }
        let Some(&state) = self.states.get(line) else {
            return Vec::new();
        };
        let mut spans = Vec::new();
        scan(document.line(line), state, Some(&mut spans));
        spans
    }
}

fn push(spans: &mut Option<&mut Vec<Span>>, start: usize, end: usize, color: Color) {
    if start < end
        && let Some(spans) = spans.as_deref_mut()
    {
        spans.push(Span { start, end, color });
    }
}

fn ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn keyword(word: &str) -> Option<Color> {
    match word {
        "as" | "async" | "await" | "break" | "const" | "continue" | "crate" | "dyn" | "else"
        | "enum" | "extern" | "false" | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop"
        | "match" | "mod" | "move" | "mut" | "pub" | "ref" | "return" | "self" | "Self"
        | "static" | "struct" | "super" | "trait" | "true" | "type" | "unsafe" | "use"
        | "where" | "while" | "yield" => Some(Color::Keyword),
        "bool" | "char" | "str" | "isize" | "usize" | "i8" | "i16" | "i32" | "i64" | "i128"
        | "u8" | "u16" | "u32" | "u64" | "u128" | "f32" | "f64" => Some(Color::Type),
        _ => None,
    }
}

fn raw_start(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    let r = if bytes.get(at) == Some(&b'r') {
        at
    } else if bytes.get(at..at + 2) == Some(b"br") {
        at + 1
    } else {
        return None;
    };
    let mut p = r + 1;
    while bytes.get(p) == Some(&b'#') {
        p += 1;
    }
    (bytes.get(p) == Some(&b'"')).then_some((p + 1, p - r - 1))
}

fn scan(source: &str, mut state: State, mut spans: Option<&mut Vec<Span>>) -> State {
    let b = source.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        match state {
            State::BlockComment(mut depth) => {
                while i < b.len() {
                    if b.get(i..i + 2) == Some(b"/*") {
                        depth += 1;
                        i += 2;
                    } else if b.get(i..i + 2) == Some(b"*/") {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                push(&mut spans, start, i, Color::Comment);
                state = if depth == 0 {
                    State::Normal
                } else {
                    State::BlockComment(depth)
                };
            }
            State::String => {
                while i < b.len() {
                    if b[i] == b'\\' {
                        i = (i + 2).min(b.len());
                    } else if b[i] == b'"' {
                        i += 1;
                        state = State::Normal;
                        break;
                    } else {
                        i += 1;
                    }
                }
                push(&mut spans, start, i, Color::String);
            }
            State::RawString(hashes) => {
                while i < b.len() {
                    if b[i] == b'"'
                        && b.get(i + 1..i + 1 + hashes)
                            .is_some_and(|s| s.iter().all(|c| *c == b'#'))
                    {
                        i += 1 + hashes;
                        state = State::Normal;
                        break;
                    }
                    i += 1;
                }
                push(&mut spans, start, i, Color::String);
            }
            State::Normal => {
                if b.get(i..i + 2) == Some(b"//") {
                    push(&mut spans, i, b.len(), Color::Comment);
                    break;
                } else if b.get(i..i + 2) == Some(b"/*") {
                    state = State::BlockComment(0);
                } else if let Some((after_quote, hashes)) = raw_start(b, i) {
                    state = State::RawString(hashes);
                    i = after_quote;
                    // Include the raw-string prefix in the colored span.
                    let mut j = i;
                    while j < b.len() {
                        if b[j] == b'"'
                            && b.get(j + 1..j + 1 + hashes)
                                .is_some_and(|s| s.iter().all(|c| *c == b'#'))
                        {
                            j += 1 + hashes;
                            state = State::Normal;
                            break;
                        }
                        j += 1;
                    }
                    i = j;
                    push(&mut spans, start, i, Color::String);
                } else if b[i] == b'"' || b.get(i..i + 2) == Some(b"b\"") {
                    if b[i] == b'b' {
                        i += 1;
                    }
                    i += 1;
                    state = State::String;
                    while i < b.len() {
                        if b[i] == b'\\' {
                            i = (i + 2).min(b.len());
                        } else if b[i] == b'"' {
                            i += 1;
                            state = State::Normal;
                            break;
                        } else {
                            i += 1;
                        }
                    }
                    push(&mut spans, start, i, Color::String);
                } else if b[i] == b'\'' {
                    i += 1;
                    let mut j = i;
                    while j < b.len() && j - i < 8 {
                        if b[j] == b'\\' {
                            j = (j + 2).min(b.len());
                        } else if b[j] == b'\'' {
                            j += 1;
                            push(&mut spans, start, j, Color::String);
                            i = j;
                            break;
                        } else if b[j] == b' ' {
                            break;
                        } else {
                            j += 1;
                        }
                    }
                } else if b[i].is_ascii_digit() {
                    i += 1;
                    while i < b.len()
                        && (ident_byte(b[i])
                            || (b[i] == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)))
                    {
                        i += 1;
                    }
                    push(&mut spans, start, i, Color::Number);
                } else if b[i].is_ascii_alphabetic() || b[i] == b'_' {
                    i += 1;
                    while i < b.len() && ident_byte(b[i]) {
                        i += 1;
                    }
                    let word = &source[start..i];
                    if let Some(color) = keyword(word) {
                        push(&mut spans, start, i, color);
                    } else if b.get(i) == Some(&b'!') {
                        i += 1;
                        push(&mut spans, start, i, Color::Macro);
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Pos;

    fn settle(syntax: &mut RustSyntax, doc: &Document, line: usize) {
        for _ in 0..500 {
            if syntax.advance_to(doc, line, 100) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("syntax worker did not finish");
    }

    #[test]
    fn multiline_comments_and_strings_keep_state() {
        let mut doc = Document::new();
        doc.replace(Pos::default(), Pos::default(), "fn main() { /* outer\n/* inner */ still */ let s = r##\"x\n// not a comment\"##; // real\n}");
        let mut syntax = RustSyntax::new();
        settle(&mut syntax, &doc, 3);
        assert_eq!(syntax.spans(&doc, 1)[0].color, Color::Comment);
        assert_eq!(syntax.spans(&doc, 2)[0].color, Color::String);
        assert_eq!(syntax.spans(&doc, 2).last().unwrap().color, Color::Comment);
        doc.replace(Pos { line: 0, byte: 12 }, Pos { line: 0, byte: 12 }, "x");
        syntax.invalidate_from(0);
        assert_eq!(syntax.spans(&doc, 2), Vec::new());
        settle(&mut syntax, &doc, 2);
    }

    #[test]
    fn scanning_is_bounded_and_spans_respect_utf8() {
        let mut doc = Document::new();
        let text = format!(
            "{}let π = 42; println!(\"ok\"); // done",
            "// a deliberately long line of Rust comments for the lexical fallback test\n"
                .repeat(3000)
        );
        doc.replace(Pos::default(), Pos::default(), &text);
        let mut syntax = RustSyntax::new();
        assert!(!syntax.advance_to(&doc, 3000, 64));
        assert!(syntax.spans(&doc, 3000).is_empty());
        assert!(syntax.advance_to(&doc, 3000, 3000));
        let source = doc.line(3000);
        let spans = syntax.spans(&doc, 3000);
        assert!(
            spans
                .iter()
                .any(|s| s.color == Color::Keyword && &source[s.start..s.end] == "let")
        );
        assert!(
            spans
                .iter()
                .any(|s| s.color == Color::Macro && &source[s.start..s.end] == "println!")
        );
        assert!(
            spans
                .iter()
                .all(|s| source.is_char_boundary(s.start) && source.is_char_boundary(s.end))
        );
    }

    #[test]
    fn tree_sitter_reparses_after_an_edit() {
        let mut doc = Document::new();
        doc.replace(
            Pos::default(),
            Pos::default(),
            "fn main() { let count = 1; }",
        );
        let mut syntax = RustSyntax::new();
        settle(&mut syntax, &doc, 0);
        assert!(syntax.tree_spans.is_some());
        assert!(
            syntax
                .spans(&doc, 0)
                .iter()
                .any(|span| span.color == Color::Keyword)
        );
        doc.replace(
            Pos { line: 0, byte: 12 },
            Pos { line: 0, byte: 12 },
            "/* note */ ",
        );
        syntax.invalidate_from(0);
        settle(&mut syntax, &doc, 0);
        assert!(syntax.tree_spans.is_some());
        assert!(
            syntax
                .spans(&doc, 0)
                .iter()
                .any(|span| span.color == Color::Comment)
        );
    }
}
