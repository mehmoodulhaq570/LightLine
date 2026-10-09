//! Tree-sitter colors for Rust and Python, with bounded lexical fallback for large Rust files.
//! Oversized files (see `parse_limit()`) render as plain text instead.

use crate::document::{Document, Pos, TextChange};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use tree_sitter::{
    InputEdit, Language, Parser, Point, Query, QueryCursor, StreamingIterator, Tree,
};

use std::sync::atomic::{AtomicUsize, Ordering};

pub const DEFAULT_PARSE_LIMIT: usize = 4 * 1024 * 1024;
static PARSE_LIMIT_BYTES: AtomicUsize = AtomicUsize::new(DEFAULT_PARSE_LIMIT);
const RUST_PARSE_LIMIT: usize = 128 * 1024;

/// Configure the maximum file size (in KB) tree-sitter will parse in the background.
pub fn set_parse_limit_kb(kb: usize) {
    PARSE_LIMIT_BYTES.store(kb.saturating_mul(1024), Ordering::Relaxed);
}

/// The current parse limit in bytes.
pub fn parse_limit() -> usize {
    PARSE_LIMIT_BYTES.load(Ordering::Relaxed)
}

fn within_parse_limit(document: &Document) -> bool {
    let limit = parse_limit();
    let mut bytes = 0;
    (0..document.line_count()).all(|line| {
        bytes += document.line(line).len() + 1;
        bytes <= limit
    })
}

fn within_rust_parse_limit(document: &Document) -> bool {
    let mut bytes = 0;
    (0..document.line_count()).all(|line| {
        bytes += document.line(line).len() + 1;
        bytes <= RUST_PARSE_LIMIT
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
    Function,
    Operator,
    Attribute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub color: Color,
}

/// Per-tab syntax highlighter, dispatching to the language matching the open file.
pub enum Syntax {
    Rust(RustSyntax),
    Python(PythonSyntax),
    C(TreeSitterSyntax),
    JavaScript(TreeSitterSyntax),
    TypeScript(TreeSitterSyntax),
    Tsx(TreeSitterSyntax),
    Json(TreeSitterSyntax),
    Markdown(crate::markdown_syntax::MarkdownSyntax),
}

impl Syntax {
    pub fn new_rust() -> Self {
        Syntax::Rust(RustSyntax::new())
    }

    pub fn new_python() -> Self {
        Syntax::Python(PythonSyntax::new())
    }

    pub fn new_c() -> Self {
        Syntax::C(TreeSitterSyntax::new(C))
    }

    pub fn new_javascript() -> Self {
        Syntax::JavaScript(TreeSitterSyntax::new(JAVASCRIPT))
    }

    pub fn new_typescript() -> Self {
        Syntax::TypeScript(TreeSitterSyntax::new(TYPESCRIPT))
    }

    pub fn new_tsx() -> Self {
        Syntax::Tsx(TreeSitterSyntax::new(TSX))
    }

    pub fn new_json() -> Self {
        Syntax::Json(TreeSitterSyntax::new(JSON))
    }

    pub fn new_markdown() -> Self {
        Syntax::Markdown(crate::markdown_syntax::MarkdownSyntax::new())
    }

    pub fn invalidate_from(&mut self, line: usize) {
        match self {
            Syntax::Rust(syntax) => syntax.invalidate_from(line),
            Syntax::Python(syntax) => syntax.invalidate_from(),
            Syntax::C(syntax)
            | Syntax::JavaScript(syntax)
            | Syntax::TypeScript(syntax)
            | Syntax::Tsx(syntax)
            | Syntax::Json(syntax) => syntax.invalidate_from(),
            Syntax::Markdown(syntax) => syntax.invalidate_from(line),
        }
    }

    /// Records an edit. Unlike `invalidate_from`, the last parse's colors stay
    /// on screen (moved along with the text) until the reparse lands, so
    /// typing doesn't flash the viewport uncolored on every keystroke.
    pub fn edited(&mut self, change: &TextChange) {
        match self {
            Syntax::Rust(syntax) => syntax.edited(change),
            Syntax::Python(syntax) => syntax.edited(change),
            Syntax::C(syntax)
            | Syntax::JavaScript(syntax)
            | Syntax::TypeScript(syntax)
            | Syntax::Tsx(syntax)
            | Syntax::Json(syntax) => syntax.edited(change),
            // Colored line by line: only the fence state from here on changes.
            Syntax::Markdown(syntax) => syntax.invalidate_from(change.start.line),
        }
    }

    pub fn advance_to(&mut self, document: &Document, target: usize, budget: usize) -> bool {
        match self {
            Syntax::Rust(syntax) => syntax.advance_to(document, target, budget),
            Syntax::Python(syntax) => syntax.advance_to(document),
            Syntax::C(syntax)
            | Syntax::JavaScript(syntax)
            | Syntax::TypeScript(syntax)
            | Syntax::Tsx(syntax)
            | Syntax::Json(syntax) => syntax.advance_to(document),
            Syntax::Markdown(syntax) => syntax.advance_to(document, target, budget),
        }
    }

    pub fn spans(&self, document: &Document, line: usize) -> Vec<Span> {
        match self {
            Syntax::Rust(syntax) => syntax.spans(document, line),
            Syntax::Python(syntax) => syntax.spans(line),
            Syntax::C(syntax)
            | Syntax::JavaScript(syntax)
            | Syntax::TypeScript(syntax)
            | Syntax::Tsx(syntax)
            | Syntax::Json(syntax) => syntax.spans(line),
            Syntax::Markdown(syntax) => syntax.spans(document, line),
        }
    }
}

// Code longer than this is shown without colors outside the editor.
const SNIPPET_LIMIT: usize = 64 * 1024;

/// Colors for a standalone piece of code, such as a fenced block in
/// Markdown or an AI answer: one span list per line (byte ranges within the
/// line, later spans drawn over earlier ones). None for a language without
/// a grammar here, or for code too long to color quickly.
pub fn highlight_snippet(language: &str, source: &str) -> Option<Vec<Vec<Span>>> {
    if source.len() > SNIPPET_LIMIT {
        return None;
    }
    let grammar = match language.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => RUST,
        "python" | "py" | "python3" => PYTHON,
        "c" | "h" => C,
        "javascript" | "js" | "mjs" | "cjs" | "jsx" => JAVASCRIPT,
        "typescript" | "ts" | "mts" | "cts" => TYPESCRIPT,
        "tsx" => TSX,
        "json" | "jsonc" => JSON,
        _ => return None,
    };
    Parsed::new(grammar, source.to_string()).map(|parsed| parsed.spans)
}

/// A tree-sitter grammar and how its highlight captures map to colors.
#[derive(Clone, Copy)]
pub struct Grammar {
    language: fn() -> Language,
    highlights: fn() -> &'static str,
    color: fn(&str) -> Option<Color>,
}

fn rust_language() -> Language {
    tree_sitter_rust::LANGUAGE.into()
}

fn python_language() -> Language {
    tree_sitter_python::LANGUAGE.into()
}

fn c_language() -> Language {
    tree_sitter_c::LANGUAGE.into()
}

fn javascript_language() -> Language {
    tree_sitter_javascript::LANGUAGE.into()
}

fn typescript_language() -> Language {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}

fn tsx_language() -> Language {
    tree_sitter_typescript::LANGUAGE_TSX.into()
}

fn json_language() -> Language {
    tree_sitter_json::LANGUAGE.into()
}

fn rust_highlights() -> &'static str {
    tree_sitter_rust::HIGHLIGHTS_QUERY
}

fn python_highlights() -> &'static str {
    tree_sitter_python::HIGHLIGHTS_QUERY
}

fn c_highlights() -> &'static str {
    tree_sitter_c::HIGHLIGHT_QUERY
}

fn javascript_highlights() -> &'static str {
    tree_sitter_javascript::HIGHLIGHT_QUERY
}

fn typescript_highlights() -> &'static str {
    static QUERY: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        format!(
            "{}\n{}",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY
        )
    });
    &QUERY
}

fn json_highlights() -> &'static str {
    tree_sitter_json::HIGHLIGHTS_QUERY
}

const RUST: Grammar = Grammar {
    language: rust_language,
    highlights: rust_highlights,
    color: rust_capture_color,
};

const PYTHON: Grammar = Grammar {
    language: python_language,
    highlights: python_highlights,
    color: python_capture_color,
};

const C: Grammar = Grammar {
    language: c_language,
    highlights: c_highlights,
    color: c_capture_color,
};

const JAVASCRIPT: Grammar = Grammar {
    language: javascript_language,
    highlights: javascript_highlights,
    color: js_capture_color,
};

const TYPESCRIPT: Grammar = Grammar {
    language: typescript_language,
    highlights: typescript_highlights,
    color: js_capture_color,
};

const TSX: Grammar = Grammar {
    language: tsx_language,
    highlights: typescript_highlights,
    color: js_capture_color,
};

const JSON: Grammar = Grammar {
    language: json_language,
    highlights: json_highlights,
    color: json_capture_color,
};

/// One document's parse on the worker thread, kept up to date edit by edit:
/// its text, tree and colors. Each keystroke used to send the worker a copy
/// of the whole document, which it diffed against the last copy before
/// reparsing and then recolored every line; now it gets the edit, and
/// recolors only the lines the edit could have changed.
struct Parsed {
    parser: Parser,
    tree: Tree,
    query: Query,
    color: fn(&str) -> Option<Color>,
    source: String,
    // Where each line of `source` starts, in bytes.
    line_starts: Vec<usize>,
    // The colors of each line of `source`.
    spans: Vec<Vec<Span>>,
    // Lines edited since the last parse (first, last); always recolored.
    edited: Option<(usize, usize)>,
}

impl Parsed {
    fn new(grammar: Grammar, source: String) -> Option<Self> {
        let language = (grammar.language)();
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;
        let query = Query::new(&language, (grammar.highlights)()).ok()?;
        let tree = parser.parse(&source, None)?;
        let spans = spans_from_query(&query, &tree, &source, grammar.color);
        let line_starts = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        Some(Self {
            parser,
            tree,
            query,
            color: grammar.color,
            source,
            line_starts,
            spans,
            edited: None,
        })
    }

    fn offset(&self, pos: Pos) -> Option<usize> {
        let byte = self.line_starts.get(pos.line)? + pos.byte;
        (byte <= line_end(&self.source, &self.line_starts, pos.line)
            && self.source.is_char_boundary(byte))
        .then_some(byte)
    }

    /// Applies one edit to the text and the tree and moves the colors along
    /// with it. False when the edit doesn't fit the text, which would mean
    /// this copy no longer matches the document.
    fn apply(&mut self, change: &TextChange) -> bool {
        let (Some(start), Some(old_end)) = (self.offset(change.start), self.offset(change.end))
        else {
            return false;
        };
        if start > old_end || !remap_spans(&mut self.spans, change) {
            return false;
        }
        let new_end_pos = change.new_end();
        let new_end = start + change.text.len();
        self.source.replace_range(start..old_end, &change.text);
        self.tree.edit(&InputEdit {
            start_byte: start,
            old_end_byte: old_end,
            new_end_byte: new_end,
            start_position: Point::new(change.start.line, change.start.byte),
            old_end_position: Point::new(change.end.line, change.end.byte),
            new_end_position: Point::new(new_end_pos.line, new_end_pos.byte),
        });
        // The edited lines' starts are replaced; later lines move with the text.
        let later: Vec<usize> = self.line_starts[change.end.line + 1..]
            .iter()
            .map(|line_start| line_start - old_end + new_end)
            .collect();
        self.line_starts.truncate(change.start.line + 1);
        self.line_starts.extend(
            change
                .text
                .match_indices('\n')
                .map(|(at, _)| start + at + 1),
        );
        self.line_starts.extend(later);
        // Earlier edits' lines, renumbered, joined with this edit's.
        let renumber = |line: usize| {
            if line > change.end.line {
                line - change.end.line + new_end_pos.line
            } else if line > change.start.line {
                new_end_pos.line
            } else {
                line
            }
        };
        let (mut first, mut last) = (change.start.line, new_end_pos.line);
        if let Some((earlier_first, earlier_last)) = self.edited {
            first = first.min(renumber(earlier_first));
            last = last.max(renumber(earlier_last));
        }
        self.edited = Some((first, last));
        self.spans.len() == self.line_starts.len()
    }

    /// Reparses after edits and recolors the lines they could have changed:
    /// the edited lines, and those whose syntax changed (an opened string or
    /// comment recolors everything after it).
    fn reparse(&mut self) -> bool {
        let Some(tree) = self.parser.parse(&self.source, Some(&self.tree)) else {
            return false;
        };
        let mut rows: Vec<(usize, usize)> = self
            .tree
            .changed_ranges(&tree)
            .map(|range| (range.start_point.row, range.end_point.row))
            .collect();
        self.tree = tree;
        rows.extend(self.edited.take());
        rows.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (first, last) in rows {
            match merged.last_mut() {
                Some(previous) if first <= previous.1 + 1 => previous.1 = previous.1.max(last),
                _ => merged.push((first, last)),
            }
        }
        for (first, last) in merged {
            self.recolor(first, last);
        }
        true
    }

    // Recomputes the colors of lines `first..=last` from the tree.
    fn recolor(&mut self, first: usize, last: usize) {
        let Self {
            tree,
            query,
            color,
            source,
            line_starts,
            spans,
            ..
        } = self;
        if first >= spans.len() {
            return;
        }
        let last = last.min(spans.len() - 1);
        for line in &mut spans[first..=last] {
            line.clear();
        }
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(line_starts[first]..line_end(source, line_starts, last));
        let mut captures = cursor.captures(query, tree.root_node(), source.as_bytes());
        while let Some((matched, capture_index)) = captures.next() {
            let capture = matched.captures()[*capture_index];
            let Some(span_color) = color(query.capture_names()[capture.index as usize]) else {
                continue;
            };
            let (start, end) = (capture.node.start_position(), capture.node.end_position());
            for line in start.row.max(first)..=end.row.min(last) {
                let text = &source[line_starts[line]..line_end(source, line_starts, line)];
                push_capture(&mut spans[line], text, line, start, end, span_color);
            }
        }
    }
}

// Where line `line` ends (before its newline).
fn line_end(source: &str, line_starts: &[usize], line: usize) -> usize {
    line_starts
        .get(line + 1)
        .map_or(source.len(), |next| next - 1)
}

// Adds the part of a capture from `start` to `end` that lies on `line`.
fn push_capture(
    spans: &mut Vec<Span>,
    text: &str,
    line: usize,
    start: Point,
    end: Point,
    color: Color,
) {
    let from = if start.row == line { start.column } else { 0 };
    let to = if end.row == line {
        end.column
    } else {
        text.len()
    };
    if from < to && to <= text.len() && text.is_char_boundary(from) && text.is_char_boundary(to) {
        spans.push(Span {
            start: from,
            end: to,
            color,
        });
    }
}

// Moves spans computed for the text before `change` onto the text after it.
// Spans before the edit stay, spans after it shift with the text, and a span
// the edit happened inside of (typing in a string or comment) grows with it.
// The freshly inserted text is uncolored until the next parse. Returns false
// when `spans` doesn't cover the edited lines, so the caller can drop them.
fn remap_spans(spans: &mut Vec<Vec<Span>>, change: &TextChange) -> bool {
    let (start, old_end, new_end) = (change.start, change.end, change.new_end());
    if old_end.line >= spans.len() {
        return false;
    }
    let shift = |byte: usize| byte - old_end.byte + new_end.byte;
    let single_line = start.line == old_end.line && start.line == new_end.line;
    let mut head = Vec::new();
    let mut tail = Vec::new();
    for span in &spans[start.line] {
        if single_line && span.start < start.byte && span.end > old_end.byte {
            head.push(Span {
                end: shift(span.end),
                ..*span
            });
        } else if span.start < start.byte {
            head.push(Span {
                end: span.end.min(start.byte),
                ..*span
            });
        }
    }
    for span in &spans[old_end.line] {
        let contained = single_line && span.start < start.byte && span.end > old_end.byte;
        if span.end > old_end.byte && !contained {
            tail.push(Span {
                start: shift(span.start.max(old_end.byte)),
                end: shift(span.end),
                color: span.color,
            });
        }
    }
    let mut lines = vec![Vec::new(); new_end.line - start.line + 1];
    lines[0] = head;
    lines.last_mut().expect("at least one line").extend(tail);
    spans.splice(start.line..=old_end.line, lines);
    true
}

fn spans_from_query(
    query: &Query,
    tree: &Tree,
    source: &str,
    capture_color: impl Fn(&str) -> Option<Color>,
) -> Vec<Vec<Span>> {
    let lines: Vec<&str> = source.split('\n').collect();
    let mut output = vec![Vec::new(); lines.len()];
    let mut cursor = QueryCursor::new();
    let mut captures = cursor.captures(query, tree.root_node(), source.as_bytes());
    while let Some((matched, capture_index)) = captures.next() {
        let capture = matched.captures()[*capture_index];
        let name = query.capture_names()[capture.index as usize];
        let Some(color) = capture_color(name) else {
            continue;
        };
        let start = capture.node.start_position();
        let end = capture.node.end_position();
        for line in start.row..=end.row.min(lines.len().saturating_sub(1)) {
            push_capture(&mut output[line], lines[line], line, start, end, color);
        }
    }
    output
}

fn rust_capture_color(name: &str) -> Option<Color> {
    if name.starts_with("comment") {
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
    } else if name == "function" || name == "function.method" {
        Some(Color::Function)
    } else if name == "operator" {
        Some(Color::Operator)
    } else if name == "attribute" || name.starts_with("attribute") {
        Some(Color::Attribute)
    } else {
        None
    }
}

fn python_capture_color(name: &str) -> Option<Color> {
    if name.starts_with("comment") {
        Some(Color::Comment)
    } else if name.starts_with("string") || name == "escape" {
        Some(Color::String)
    } else if name.starts_with("keyword") {
        Some(Color::Keyword)
    } else if name.starts_with("type") || name == "constructor" {
        Some(Color::Type)
    } else if name.starts_with("number") || name.starts_with("constant") {
        Some(Color::Number)
    } else if name == "function" || name == "function.method" {
        Some(Color::Function)
    } else if name == "operator" {
        Some(Color::Operator)
    } else if name == "decorator" || name.starts_with("attribute") {
        Some(Color::Attribute)
    } else {
        None
    }
}

fn c_capture_color(name: &str) -> Option<Color> {
    if name.starts_with("comment") {
        Some(Color::Comment)
    } else if name.starts_with("string") || name.starts_with("character") {
        Some(Color::String)
    } else if name.starts_with("keyword") || name == "boolean" {
        Some(Color::Keyword)
    } else if name.starts_with("type") {
        Some(Color::Type)
    } else if name.starts_with("number") || name.starts_with("constant") {
        Some(Color::Number)
    } else if name == "function.macro" || name.starts_with("preproc") {
        Some(Color::Macro)
    } else if name.starts_with("function") {
        Some(Color::Function)
    } else if name == "operator" {
        Some(Color::Operator)
    } else if name.starts_with("attribute") {
        Some(Color::Attribute)
    } else {
        None
    }
}

fn js_capture_color(name: &str) -> Option<Color> {
    if name.starts_with("comment") {
        Some(Color::Comment)
    } else if name.starts_with("string") || name.starts_with("character") || name == "escape" {
        Some(Color::String)
    } else if name.starts_with("keyword") || name == "boolean" {
        Some(Color::Keyword)
    } else if name.starts_with("type") || name == "constructor" {
        Some(Color::Type)
    } else if name.starts_with("number") || name.starts_with("constant") {
        Some(Color::Number)
    } else if name.starts_with("function") || name.starts_with("method") {
        Some(Color::Function)
    } else if name == "operator" {
        Some(Color::Operator)
    } else if name.starts_with("attribute") || name.starts_with("decorator") {
        Some(Color::Attribute)
    } else {
        None
    }
}

fn json_capture_color(name: &str) -> Option<Color> {
    if name.starts_with("comment") {
        Some(Color::Comment)
    } else if name.starts_with("string") {
        Some(Color::String)
    } else if name.starts_with("number") {
        Some(Color::Number)
    } else if name == "boolean" || name == "null" || name.starts_with("constant") {
        Some(Color::Keyword)
    } else {
        None
    }
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
    // The worker needs the whole text again: it has none yet, or a change
    // came through that it wasn't sent as an edit.
    reset: bool,
    // A parse of `revision` is in flight; meanwhile `tree_spans` holds the
    // previous result, remapped through every edit made since.
    pending: bool,
    revision: u64,
}

// Work for the parser thread. Edits must reach it in order, each one after
// the text it applies to.
enum Job {
    // The whole text: the first parse, or after a reload.
    Reset { revision: u64, source: String },
    Edit { revision: u64, change: TextChange },
}

struct Worker {
    jobs: Sender<Job>,
    results: Receiver<ResultSet>,
}

struct ResultSet {
    revision: u64,
    spans: Option<Vec<Vec<Span>>>,
}

impl Worker {
    fn start(grammar: Grammar) -> Self {
        let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
        let (results_tx, results_rx) = mpsc::channel::<ResultSet>();
        thread::spawn(move || {
            let mut parsed: Option<Parsed> = None;
            while let Ok(job) = jobs_rx.recv() {
                // Everything queued is applied, then parsed once.
                let mut jobs = vec![job];
                jobs.extend(jobs_rx.try_iter());
                // Nothing before the last reset matters.
                let from = jobs
                    .iter()
                    .rposition(|job| matches!(job, Job::Reset { .. }))
                    .unwrap_or(0);
                let mut revision = 0;
                let mut edited = false;
                for job in jobs.drain(from..) {
                    match job {
                        Job::Reset {
                            revision: next,
                            source,
                        } => {
                            revision = next;
                            parsed = Parsed::new(grammar, source);
                            edited = false;
                        }
                        Job::Edit {
                            revision: next,
                            change,
                        } => {
                            revision = next;
                            edited = true;
                            // An edit that doesn't fit means this copy no
                            // longer matches the document: stop coloring.
                            if parsed.as_mut().is_some_and(|parsed| !parsed.apply(&change)) {
                                parsed = None;
                            }
                        }
                    }
                }
                let spans = match &mut parsed {
                    // Grown past the limit by editing: stop, as a file that
                    // big wouldn't have been parsed when it was opened.
                    Some(current) if current.source.len() <= parse_limit() => {
                        (!edited || current.reparse()).then(|| current.spans.clone())
                    }
                    _ => None,
                };
                if results_tx.send(ResultSet { revision, spans }).is_err() {
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

// Sends an edit to the parser thread; `reset` means the whole text is about
// to be sent anyway, which includes it.
fn send_edit(
    worker: &Option<Worker>,
    reset: bool,
    pending: &mut bool,
    revision: u64,
    change: &TextChange,
) {
    if let Some(worker) = worker.as_ref().filter(|_| !reset) {
        // A dead worker shows up as a closed channel on the next poll.
        let _ = worker.jobs.send(Job::Edit {
            revision,
            change: change.clone(),
        });
        *pending = true;
    }
}

// Sends the whole document to the parser thread when it needs it, then takes
// the result for the current revision if it has arrived. A failed parse or a
// dead worker stops tree-sitter for this document (Rust then uses its lexical
// fallback; Python shows plain text).
fn poll_worker(
    worker: &mut Option<Worker>,
    tree_spans: &mut Option<Vec<Vec<Span>>>,
    reset: &mut bool,
    pending: &mut bool,
    revision: u64,
    document: &Document,
) {
    let mut failed = false;
    if *reset {
        *reset = false;
        let sent = within_parse_limit(document)
            && worker.as_ref().is_some_and(|worker| {
                let source = document.text_range(Default::default(), document.end());
                worker.jobs.send(Job::Reset { revision, source }).is_ok()
            });
        failed = !sent;
        *pending = sent;
    }
    if let Some(active) = worker.as_ref().filter(|_| !failed) {
        loop {
            match active.results.try_recv() {
                Ok(result) if result.revision == revision => {
                    *pending = false;
                    failed = result.spans.is_none();
                    *tree_spans = result.spans;
                    break;
                }
                Ok(_) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    failed = true;
                    break;
                }
            }
        }
    }
    if failed {
        *worker = None;
        *tree_spans = None;
        *pending = false;
    }
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
            reset: false,
            pending: false,
            revision: 0,
        }
    }

    pub fn invalidate_from(&mut self, line: usize) {
        self.states.truncate((line + 1).max(1));
        self.reset = true;
        self.revision = self.revision.wrapping_add(1);
        self.tree_spans = None;
    }

    pub fn edited(&mut self, change: &TextChange) {
        self.states.truncate(change.start.line + 1);
        self.revision = self.revision.wrapping_add(1);
        send_edit(
            &self.worker,
            self.reset,
            &mut self.pending,
            self.revision,
            change,
        );
        if let Some(spans) = &mut self.tree_spans
            && !remap_spans(spans, change)
        {
            self.tree_spans = None;
        }
    }

    /// Scan at most `budget` fallback lines and poll the background parser.
    /// Returns false while either path has more work for the requested viewport.
    pub fn advance_to(&mut self, document: &Document, target: usize, budget: usize) -> bool {
        if !self.parser_attempted {
            self.parser_attempted = true;
            if within_rust_parse_limit(document) {
                self.worker = Some(Worker::start(RUST));
                self.reset = true;
            }
        }
        if self.worker.is_some() {
            poll_worker(
                &mut self.worker,
                &mut self.tree_spans,
                &mut self.reset,
                &mut self.pending,
                self.revision,
                document,
            );
            if self.tree_spans.is_some() {
                return !self.pending;
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

pub struct TreeSitterSyntax {
    grammar: Grammar,
    worker: Option<Worker>,
    tree_spans: Option<Vec<Vec<Span>>>,
    parser_attempted: bool,
    reset: bool,
    pending: bool,
    revision: u64,
}

impl TreeSitterSyntax {
    pub fn new(grammar: Grammar) -> Self {
        Self {
            grammar,
            worker: None,
            tree_spans: None,
            parser_attempted: false,
            reset: false,
            pending: false,
            revision: 0,
        }
    }

    pub fn invalidate_from(&mut self) {
        self.reset = true;
        self.revision = self.revision.wrapping_add(1);
        self.tree_spans = None;
    }

    pub fn edited(&mut self, change: &TextChange) {
        self.revision = self.revision.wrapping_add(1);
        send_edit(
            &self.worker,
            self.reset,
            &mut self.pending,
            self.revision,
            change,
        );
        if let Some(spans) = &mut self.tree_spans
            && !remap_spans(spans, change)
        {
            self.tree_spans = None;
        }
    }

    /// Polls the background parser. Files over `parse_limit()` never start one, so
    /// they render as plain text; there is no lexical fallback.
    pub fn advance_to(&mut self, document: &Document) -> bool {
        if !self.parser_attempted {
            self.parser_attempted = true;
            if within_parse_limit(document) {
                self.worker = Some(Worker::start(self.grammar));
                self.reset = true;
            }
        }
        if self.worker.is_some() {
            poll_worker(
                &mut self.worker,
                &mut self.tree_spans,
                &mut self.reset,
                &mut self.pending,
                self.revision,
                document,
            );
        }
        self.worker.is_none() || (self.tree_spans.is_some() && !self.pending)
    }

    pub fn spans(&self, line: usize) -> Vec<Span> {
        self.tree_spans
            .as_ref()
            .and_then(|spans| spans.get(line).cloned())
            .unwrap_or_default()
    }
}

pub struct PythonSyntax {
    inner: TreeSitterSyntax,
}

impl Default for PythonSyntax {
    fn default() -> Self {
        Self::new()
    }
}

impl PythonSyntax {
    pub fn new() -> Self {
        Self {
            inner: TreeSitterSyntax::new(PYTHON),
        }
    }

    pub fn invalidate_from(&mut self) {
        self.inner.invalidate_from();
    }

    pub fn edited(&mut self, change: &TextChange) {
        self.inner.edited(change);
    }

    pub fn advance_to(&mut self, document: &Document) -> bool {
        self.inner.advance_to(document)
    }

    pub fn spans(&self, line: usize) -> Vec<Span> {
        self.inner.spans(line)
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
                    } else {
                        let mut k = i;
                        while k < b.len() && b[k] == b' ' {
                            k += 1;
                        }
                        if k < b.len() && b[k] == b'(' {
                            push(&mut spans, start, i, Color::Function);
                        }
                    }
                } else if matches!(
                    b[i],
                    b'+' | b'-' | b'*' | b'/' | b'%' | b'=' | b'<' | b'>' | b'&' | b'|' | b'^'
                ) {
                    let op_start = i;
                    i += 1;
                    if i < b.len() && matches!(b[i], b'=' | b'&' | b'|' | b'<' | b'>') {
                        i += 1;
                    }
                    push(&mut spans, op_start, i, Color::Operator);
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

    #[test]
    fn snippets_are_colored_by_their_fence_language() {
        let colors = |language: &str, source: &str, line: usize| {
            highlight_snippet(language, source).map(|lines| {
                lines[line]
                    .iter()
                    .map(|span| {
                        (
                            source.split('\n').nth(line).unwrap()[span.start..span.end].to_string(),
                            span.color,
                        )
                    })
                    .collect::<Vec<_>>()
            })
        };
        let rust = colors("rust", "fn main() {\n    let x = \"hi\";\n}", 1).unwrap();
        assert!(rust.contains(&("let".into(), Color::Keyword)), "{rust:?}");
        assert!(rust.contains(&("\"hi\"".into(), Color::String)), "{rust:?}");
        let python = colors("py", "def f():\n    return 42", 1).unwrap();
        assert!(
            python.contains(&("return".into(), Color::Keyword)),
            "{python:?}"
        );
        assert!(python.contains(&("42".into(), Color::Number)), "{python:?}");
        assert!(highlight_snippet("cobol", "DISPLAY 'HI'.").is_none());
        assert!(highlight_snippet("rust", &"x".repeat(SNIPPET_LIMIT + 1)).is_none());
    }

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

    #[test]
    fn remapping_splits_and_moves_spans_with_a_line_break() {
        let span = |start, end, color| Span { start, end, color };
        let mut spans = vec![
            vec![span(0, 2, Color::Keyword), span(3, 8, Color::String)],
            vec![span(0, 4, Color::Comment)],
        ];
        // Enter at byte 5, inside the string.
        let change = TextChange {
            serial: 1,
            start: Pos { line: 0, byte: 5 },
            end: Pos { line: 0, byte: 5 },
            start_utf16: 5,
            end_utf16: 5,
            text: "\n".into(),
        };
        assert!(remap_spans(&mut spans, &change));
        assert_eq!(
            spans,
            vec![
                vec![span(0, 2, Color::Keyword), span(3, 5, Color::String)],
                vec![span(0, 3, Color::String)],
                vec![span(0, 4, Color::Comment)],
            ]
        );
    }

    #[test]
    fn edits_keep_the_previous_colors_until_the_reparse_lands() {
        let mut doc = Document::new();
        doc.replace(
            Pos::default(),
            Pos::default(),
            "fn main() {}\nlet s = \"text\";",
        );
        let mut syntax = RustSyntax::new();
        settle(&mut syntax, &doc, 1);
        // A line inserted above moves the string's color down with it at once.
        doc.replace(Pos::default(), Pos::default(), "// note\n");
        syntax.edited(doc.last_change().unwrap());
        assert!(syntax.tree_spans.is_some());
        assert!(
            syntax
                .spans(&doc, 2)
                .iter()
                .any(|span| span.color == Color::String)
        );
        // Typing inside the string keeps the whole string colored.
        let inside = Pos { line: 2, byte: 10 };
        doc.replace(inside, inside, "more ");
        syntax.edited(doc.last_change().unwrap());
        let string = syntax
            .spans(&doc, 2)
            .into_iter()
            .find(|span| span.color == Color::String)
            .unwrap();
        assert_eq!(&doc.line(2)[string.start..string.end], "\"tmore ext\"");
        // The reparse still happens and replaces the remapped colors.
        settle(&mut syntax, &doc, 2);
        assert!(!syntax.pending);
        assert!(
            syntax
                .spans(&doc, 0)
                .iter()
                .any(|span| span.color == Color::Comment)
        );
    }

    // Applies `edits` one at a time (or all before one reparse, with
    // `batched`), checking after each that the colors kept up to date edit
    // by edit are exactly what coloring the whole text from scratch gives.
    fn check_incremental(grammar: Grammar, text: &str, edits: &[(Pos, Pos, &str)], batched: bool) {
        let mut doc = Document::new();
        doc.replace(Pos::default(), Pos::default(), text);
        let mut parsed = Parsed::new(grammar, doc.text()).unwrap();
        for (index, &(start, end, insert)) in edits.iter().enumerate() {
            doc.replace(start, end, insert);
            assert!(
                parsed.apply(doc.last_change().unwrap()),
                "edit {index} didn't fit"
            );
            if batched && index + 1 < edits.len() {
                continue;
            }
            assert!(parsed.reparse());
            assert_eq!(parsed.source, doc.text(), "text after edit {index}");
            let expected = full_colors(&mut parsed, &doc.text());
            assert_eq!(
                parsed.spans, expected,
                "colors after edit {index} ({insert:?})"
            );
        }
    }

    fn at(line: usize, byte: usize) -> Pos {
        Pos { line, byte }
    }

    // Colors for `text` parsed from scratch, with `parsed`'s parser and
    // compiled query (compiling a new one per check made the tests slow).
    // `parsed.tree`, which the next edit builds on, is left as it is.
    fn full_colors(parsed: &mut Parsed, text: &str) -> Vec<Vec<Span>> {
        let tree = parsed.parser.parse(text, None).unwrap();
        spans_from_query(&parsed.query, &tree, text, parsed.color)
    }

    const RUST_SAMPLE: &str =
        "fn main() {\n    let x = 1; // one\n    let s = \"two\";\n}\nfn other() -> u32 { 3 }\n";

    #[test]
    fn rust_incremental_colors_match_a_full_recolor() {
        let edits: &[(Pos, Pos, &str)] = &[
            // Opening a block comment turns everything after it into comment...
            (at(1, 4), at(1, 4), "/* "),
            // ...and closing it gives the code its colors back.
            (at(2, 4), at(2, 4), " */"),
            // A new line, then text typed inside a string.
            (at(1, 24), at(1, 24), "\n    let y = x + 1;"),
            (at(3, 15), at(3, 15), "abc"),
            // An unterminated string, then fixed.
            (at(5, 0), at(5, 0), "\""),
            (at(5, 0), at(5, 1), ""),
            // A whole line deleted, and a replacement across lines.
            (at(2, 0), at(3, 0), ""),
            (at(0, 3), at(2, 5), "start() {\n    return;\n}\nfn b"),
            // Everything deleted, then retyped.
            (at(0, 0), at(5, 0), ""),
            (at(0, 0), at(0, 0), "struct Point { x: f64 }\n"),
        ];
        check_incremental(RUST, RUST_SAMPLE, edits, false);
    }

    #[test]
    fn python_incremental_colors_match_a_full_recolor() {
        let text =
            "def greet(name):\n    # say hello\n    return f\"hi {name}\"\n\nclass A:\n    x = 1\n";
        let edits: &[(Pos, Pos, &str)] = &[
            // A triple-quoted string opened at the top, then closed.
            (at(1, 4), at(1, 4), "\"\"\""),
            (at(3, 0), at(3, 0), "\"\"\""),
            (at(1, 4), at(1, 7), ""),
            (at(3, 0), at(3, 3), ""),
            (at(5, 9), at(5, 9), "\n    y = \"two\"  # note"),
            (at(0, 4), at(0, 9), "wave"),
        ];
        check_incremental(PYTHON, text, edits, false);
    }

    #[test]
    fn edits_batched_before_one_reparse_match_a_full_recolor() {
        // Far-apart edits applied together, as the worker does when it falls
        // behind: the first one's lines move with the later ones.
        let edits: &[(Pos, Pos, &str)] = &[
            (at(4, 0), at(4, 0), "// late\n"),
            (at(0, 0), at(0, 0), "use std::io;\n\n"),
            (at(3, 12), at(3, 12), "/* open"),
        ];
        check_incremental(RUST, RUST_SAMPLE, edits, true);
    }

    #[test]
    fn random_edits_keep_incremental_colors_matching_a_full_recolor() {
        // Fixed-seed pseudo-random edits made of the pieces most likely to
        // change colors far from the edit: quotes, comment markers, newlines.
        let pieces = [
            "\"",
            "/*",
            "*/",
            "//",
            "\n",
            "{",
            "}",
            "fn f() ",
            "let v = 'a';",
            "r#\"",
            "\"#",
            " ",
            "x",
        ];
        let mut seed: u64 = 0x5eed_1234_abcd_ef01;
        let mut next = |below: usize| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            ((seed >> 33) as usize) % below.max(1)
        };
        let mut doc = Document::new();
        doc.replace(Pos::default(), Pos::default(), RUST_SAMPLE);
        let mut parsed = Parsed::new(RUST, doc.text()).unwrap();
        for step in 0..300 {
            let line = next(doc.line_count());
            let text = doc.line(line);
            let mut byte = next(text.len() + 1);
            while !text.is_char_boundary(byte) {
                byte -= 1;
            }
            let start = at(line, byte);
            // Mostly insertions; sometimes delete a few bytes on the line.
            let (end, insert) = if next(4) == 0 {
                let mut end = (byte + next(4)).min(text.len());
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                (at(line, end), "")
            } else {
                (start, pieces[next(pieces.len())])
            };
            let serial = doc.change_serial();
            doc.replace(start, end, insert);
            // As in the editor, a replace that changed nothing isn't sent.
            if doc.change_serial() == serial {
                continue;
            }
            assert!(parsed.apply(doc.last_change().unwrap()), "step {step}");
            assert!(parsed.reparse());
            let expected = full_colors(&mut parsed, &doc.text());
            assert_eq!(parsed.spans, expected, "colors after step {step}");
        }
    }

    #[test]
    fn an_edit_that_does_not_fit_is_refused() {
        let mut parsed = Parsed::new(RUST, "fn a() {}\n".into()).unwrap();
        let change = TextChange {
            serial: 1,
            start: at(5, 0),
            end: at(5, 0),
            start_utf16: 0,
            end_utf16: 0,
            text: "x".into(),
        };
        assert!(!parsed.apply(&change));
    }

    fn settle_python(syntax: &mut PythonSyntax, doc: &Document) {
        for _ in 0..500 {
            if syntax.advance_to(doc) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("python syntax worker did not finish");
    }

    #[test]
    fn python_tree_sitter_colors_keywords_and_strings() {
        let mut doc = Document::new();
        doc.replace(
            Pos::default(),
            Pos::default(),
            "def greet(name):\n    return f\"hi {name}\"  # comment\n",
        );
        let mut syntax = PythonSyntax::new();
        settle_python(&mut syntax, &doc);
        assert!(
            syntax
                .spans(0)
                .iter()
                .any(|span| span.color == Color::Keyword)
        );
        assert!(
            syntax
                .spans(1)
                .iter()
                .any(|span| span.color == Color::String)
        );
        assert!(
            syntax
                .spans(1)
                .iter()
                .any(|span| span.color == Color::Comment)
        );
    }

    #[test]
    fn python_reparses_after_an_edit() {
        let mut doc = Document::new();
        doc.replace(Pos::default(), Pos::default(), "x = 1\n");
        let mut syntax = PythonSyntax::new();
        settle_python(&mut syntax, &doc);
        assert!(
            syntax
                .spans(0)
                .iter()
                .any(|span| span.color == Color::Number)
        );
        doc.replace(Pos { line: 0, byte: 4 }, Pos { line: 0, byte: 5 }, "\"s\"");
        syntax.invalidate_from();
        settle_python(&mut syntax, &doc);
        assert!(
            syntax
                .spans(0)
                .iter()
                .any(|span| span.color == Color::String)
        );
    }

    #[test]
    fn python_oversized_file_falls_back_to_plain_text() {
        set_parse_limit_kb(64);
        let mut doc = Document::new();
        let text = format!("x = 1\n{}", "# padding line\n".repeat(64 * 1024 / 15 + 100));
        doc.replace(Pos::default(), Pos::default(), &text);
        let mut syntax = PythonSyntax::new();
        assert!(syntax.advance_to(&doc));
        assert!(syntax.spans(0).is_empty());
        set_parse_limit_kb(DEFAULT_PARSE_LIMIT / 1024);
    }

    #[test]
    fn dynamic_parse_limit_configuration() {
        set_parse_limit_kb(1024);
        assert_eq!(parse_limit(), 1024 * 1024);
        set_parse_limit_kb(8192);
        assert_eq!(parse_limit(), 8192 * 1024);
        set_parse_limit_kb(DEFAULT_PARSE_LIMIT / 1024);
    }

    #[test]
    fn c_tree_sitter_colors_keywords_and_types() {
        let mut doc = Document::new();
        doc.replace(
            Pos::default(),
            Pos::default(),
            "int main(void) {\n    return 0;\n}\n",
        );
        let mut syntax = TreeSitterSyntax::new(C);
        for _ in 0..500 {
            if syntax.advance_to(&doc) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(syntax.spans(0).iter().any(|span| span.color == Color::Type));
        assert!(
            syntax
                .spans(1)
                .iter()
                .any(|span| span.color == Color::Keyword)
        );
    }

    #[test]
    fn json_tree_sitter_colors() {
        let mut doc = Document::new();
        doc.replace(
            Pos::default(),
            Pos::default(),
            "{\"answer\": 42, \"flag\": true}\n",
        );
        let mut syntax = TreeSitterSyntax::new(JSON);
        for _ in 0..500 {
            if syntax.advance_to(&doc) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            syntax
                .spans(0)
                .iter()
                .any(|span| span.color == Color::String)
        );
        assert!(
            syntax
                .spans(0)
                .iter()
                .any(|span| span.color == Color::Number)
        );
        assert!(
            syntax
                .spans(0)
                .iter()
                .any(|span| span.color == Color::Keyword)
        );
    }

    #[test]
    fn js_ts_snippets_are_highlighted() {
        let js = highlight_snippet("javascript", "const x = 42;\nfunction run() {}\n").unwrap();
        assert!(js[0].iter().any(|s| s.color == Color::Keyword));
        assert!(js[0].iter().any(|s| s.color == Color::Number));
        assert!(js[1].iter().any(|s| s.color == Color::Keyword));

        let ts = highlight_snippet("typescript", "const msg: string = 'hello';\n").unwrap();
        assert!(ts[0].iter().any(|s| s.color == Color::Keyword));
        assert!(ts[0].iter().any(|s| s.color == Color::Type));
        assert!(ts[0].iter().any(|s| s.color == Color::String));

        let c = highlight_snippet("c", "int val = 100;\n").unwrap();
        assert!(c[0].iter().any(|s| s.color == Color::Type));
        assert!(c[0].iter().any(|s| s.color == Color::Number));

        let json = highlight_snippet("json", "{\"num\": 123}\n").unwrap();
        assert!(json[0].iter().any(|s| s.color == Color::String));
        assert!(json[0].iter().any(|s| s.color == Color::Number));
    }

    #[test]
    fn tsx_parses_type_annotations_and_jsx_elements() {
        let source = "const title: string = 'hello';\nconst element = <div>{title}</div>;\n";
        let parsed = Parsed::new(TSX, source.into()).expect("TSX grammar and query");
        assert!(!parsed.tree.root_node().has_error());
        let spans = highlight_snippet("tsx", source).unwrap();
        assert!(spans[0].iter().any(|span| span.color == Color::Type));
        assert!(spans[1].iter().any(|span| span.color == Color::Keyword));
    }
}
