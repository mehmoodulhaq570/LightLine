//! Markdown source colors for the editor, in the spirit of VS Code's:
//! headings, emphasis, code, links and URLs, list and quote markers.
//!
//! Lines are colored one at a time as they come on screen. Only fenced code
//! blocks carry over from one line to the next, so an edit only rescans the
//! fence state from its own line onward, and lazily at that.

use crate::document::Document;
use crate::syntax::{Color, Span};

// Colors are theme roles, so a color theme restyles Markdown too.
const HEADING: Color = Color::Keyword;
const MARKER: Color = Color::Keyword;
const STRONG: Color = Color::Function;
const EMPHASIS: Color = Color::Attribute;
const CODE: Color = Color::Number;
const LINK_TEXT: Color = Color::Type;
const URL: Color = Color::Macro;
const MUTED: Color = Color::Comment;
const PUNCTUATION: Color = Color::Operator;

// How far ahead a closing delimiter is looked for, so a line full of
// unmatched brackets or asterisks costs linear time, not quadratic.
const SEARCH_WINDOW: usize = 2048;
const MAX_NESTING: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fence {
    marker: u8,
    len: usize,
}

pub struct MarkdownSyntax {
    // states[i] is the code fence open at the start of line i, if any.
    states: Vec<Option<Fence>>,
    pub(crate) recolored: crate::syntax::Recolored,
}

impl Default for MarkdownSyntax {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownSyntax {
    pub fn new() -> Self {
        Self {
            states: vec![None],
            recolored: crate::syntax::Recolored::Nothing,
        }
    }

    pub fn invalidate_from(&mut self, line: usize) {
        self.states.truncate(line + 1);
    }

    /// Scans fence state up to `target`, at most `budget` lines per call.
    /// Returns false while lines up to `target` are still unscanned.
    pub fn advance_to(&mut self, document: &Document, target: usize, budget: usize) -> bool {
        let target = target.min(document.line_count().saturating_sub(1));
        let scanned_before = self.states.len();
        for _ in 0..budget {
            if self.states.len() > target {
                break;
            }
            let line = self.states.len() - 1;
            let next = fence_after(document.line(line), self.states[line]);
            self.states.push(next);
        }
        if self.states.len() != scanned_before {
            self.recolored = crate::syntax::Recolored::Unknown;
        }
        self.states.len() > target
    }

    pub fn spans(&self, document: &Document, line: usize) -> Vec<Span> {
        match self.states.get(line) {
            Some(&fence) => line_spans(document.line(line), fence),
            None => Vec::new(),
        }
    }
}

// A code fence (``` or ~~~, three or more) indented at most three spaces.
fn fence_marker(text: &str) -> Option<Fence> {
    let trimmed = text.trim_start_matches(' ');
    if text.len() - trimmed.len() > 3 {
        return None;
    }
    let marker = *trimmed.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let len = trimmed.bytes().take_while(|byte| *byte == marker).count();
    (len >= 3).then_some(Fence { marker, len })
}

// The fence open after `text`, given the one open before it.
fn fence_after(text: &str, open: Option<Fence>) -> Option<Fence> {
    match open {
        Some(fence) => {
            let closes = fence_marker(text).is_some_and(|close| {
                close.marker == fence.marker
                    && close.len >= fence.len
                    && text
                        .trim()
                        .trim_start_matches(fence.marker as char)
                        .trim()
                        .is_empty()
            });
            (!closes).then_some(fence)
        }
        None => {
            let fence = fence_marker(text)?;
            // A backtick fence's info string can't contain backticks.
            let info = &text.trim_start()[fence.len..];
            (fence.marker != b'`' || !info.contains('`')).then_some(fence)
        }
    }
}

// The length of a list marker (`-`, `*`, `+`, or `1.`/`1)`) at the start
// of `text`, when followed by whitespace or the end of the line.
fn list_marker_len(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let len = match bytes.first()? {
        b'-' | b'*' | b'+' => 1,
        byte if byte.is_ascii_digit() => {
            let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
            if digits > 9 || !matches!(bytes.get(digits), Some(b'.' | b')')) {
                return None;
            }
            digits + 1
        }
        _ => return None,
    };
    matches!(bytes.get(len), None | Some(b' ' | b'\t')).then_some(len)
}

fn paint(colors: &mut [Option<Color>], from: usize, to: usize, color: Color) {
    let to = to.min(colors.len());
    for slot in &mut colors[from..to] {
        if slot.is_none() {
            *slot = Some(color);
        }
    }
}

fn char_len(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(1, char::len_utf8)
}

/// The colored spans of one Markdown line, given the code fence (if any)
/// open when the line starts.
fn line_spans(text: &str, open: Option<Fence>) -> Vec<Span> {
    let len = text.len();
    if len == 0 {
        return Vec::new();
    }
    let whole = |color| {
        vec![Span {
            start: 0,
            end: len,
            color,
        }]
    };
    // Inside a code block, or one of its fence lines.
    if open.is_some() || fence_marker(text).is_some() {
        return whole(CODE);
    }
    let bytes = text.as_bytes();
    let trimmed = text.trim_start();
    let indent = len - trimmed.len();
    if indent <= 3 {
        let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
        if (1..=6).contains(&hashes)
            && matches!(trimmed.as_bytes().get(hashes), None | Some(b' ' | b'\t'))
        {
            return whole(HEADING);
        }
        // A thematic break (---, ***, ___) or a setext underline (===).
        let marks: Vec<u8> = trimmed
            .bytes()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect();
        if marks.len() >= 3
            && b"-*_="
                .iter()
                .any(|mark| marks.iter().all(|byte| byte == mark))
        {
            return whole(PUNCTUATION);
        }
    }
    let mut colors = vec![None; len];
    let mut at = indent;
    let mut quoted = false;
    while at < len && bytes[at] == b'>' {
        paint(&mut colors, at, at + 1, MARKER);
        quoted = true;
        at += 1;
        while at < len && bytes[at] == b' ' {
            at += 1;
        }
    }
    if let Some(marker) = list_marker_len(&text[at..]) {
        paint(&mut colors, at, at + marker, MARKER);
        at += marker;
        while at < len && bytes[at] == b' ' {
            at += 1;
        }
        let rest = &text[at..];
        if ["[ ]", "[x]", "[X]"]
            .iter()
            .any(|task| rest.starts_with(task))
        {
            paint(&mut colors, at, at + 3, MARKER);
            at += 3;
        }
    }
    if trimmed.starts_with('|') {
        let separator = trimmed
            .bytes()
            .all(|byte| matches!(byte, b'|' | b'-' | b':' | b' ' | b'\t'));
        if separator {
            return whole(PUNCTUATION);
        }
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'|' && (index == 0 || bytes[index - 1] != b'\\') {
                paint(&mut colors, index, index + 1, PUNCTUATION);
            }
        }
    }
    inline(text, at, len, &mut colors, None, 0);
    if quoted {
        paint(&mut colors, indent, len, MUTED);
    }
    let mut spans = Vec::new();
    let mut start = 0;
    while start < len {
        let color = colors[start];
        let mut end = start + 1;
        while end < len && colors[end] == color {
            end += 1;
        }
        if let Some(color) = color {
            spans.push(Span { start, end, color });
        }
        start = end;
    }
    spans
}

// The index just past the `close` byte matching the `open` at `from`,
// counting nesting, within the search window.
fn matching(bytes: &[u8], from: usize, to: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0usize;
    let end = to.min(from + SEARCH_WINDOW);
    let mut at = from;
    while at < end {
        match bytes[at] {
            b'\\' => at += 1,
            byte if byte == open => depth += 1,
            byte if byte == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(at + 1);
                }
            }
            _ => {}
        }
        at += 1;
    }
    None
}

// Colors the inline Markdown in `text[from..to]`: code spans, links and
// images, autolinks and URLs, HTML, emphasis and strikethrough. With
// `base`, everything left uncolored in the range takes that color (the
// text inside bold, or a link's text).
fn inline(
    text: &str,
    from: usize,
    to: usize,
    colors: &mut [Option<Color>],
    base: Option<Color>,
    depth: u8,
) {
    let bytes = text.as_bytes();
    let window = |at: usize| to.min(at + SEARCH_WINDOW);
    let mut at = from;
    while at < to {
        let byte = bytes[at];
        match byte {
            b'\\' if at + 1 < to => at += 1 + char_len(text, at + 1),
            b'`' => {
                let run = bytes[at..to].iter().take_while(|b| **b == b'`').count();
                let mut close = None;
                let mut scan = at + run;
                while scan < window(at) {
                    if bytes[scan] == b'`' {
                        let length = bytes[scan..to].iter().take_while(|b| **b == b'`').count();
                        if length == run {
                            close = Some(scan + run);
                            break;
                        }
                        scan += length;
                    } else {
                        scan += 1;
                    }
                }
                match close {
                    Some(end) => {
                        paint(colors, at, end, CODE);
                        at = end;
                    }
                    None => at += run,
                }
            }
            b'<' => {
                let rest = &text[at..to];
                let end = if rest.starts_with("<!--") {
                    Some(rest.find("-->").map_or(to, |close| at + close + 3))
                } else if rest.starts_with("<http://")
                    || rest.starts_with("<https://")
                    || rest.starts_with("<mailto:")
                {
                    rest.find('>')
                        .filter(|close| !rest[..*close].contains(char::is_whitespace))
                        .map(|close| {
                            paint(colors, at, at + close + 1, URL);
                            at + close + 1
                        })
                } else if rest[1..].starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '/') {
                    rest.find('>').map(|close| at + close + 1)
                } else {
                    None
                };
                match end {
                    Some(end) => {
                        let color = if rest.starts_with("<!--") {
                            MUTED
                        } else {
                            PUNCTUATION
                        };
                        paint(colors, at, end, color);
                        at = end;
                    }
                    None => at += 1,
                }
            }
            b'!' | b'[' if depth < MAX_NESTING => {
                let open = if byte == b'!' { at + 1 } else { at };
                if bytes.get(open) != Some(&b'[') {
                    at += 1;
                    continue;
                }
                let Some(text_end) = matching(bytes, open, to, b'[', b']') else {
                    at += 1;
                    continue;
                };
                match bytes.get(text_end) {
                    Some(b'(') => {
                        let Some(url_end) = matching(bytes, text_end, to, b'(', b')') else {
                            at += 1;
                            continue;
                        };
                        inline(
                            text,
                            open + 1,
                            text_end - 1,
                            colors,
                            Some(LINK_TEXT),
                            depth + 1,
                        );
                        paint(colors, at, open + 1, LINK_TEXT);
                        paint(colors, text_end - 1, text_end, LINK_TEXT);
                        paint(colors, text_end, url_end, URL);
                        at = url_end;
                    }
                    Some(b'[') => {
                        let Some(label_end) = matching(bytes, text_end, to, b'[', b']') else {
                            at += 1;
                            continue;
                        };
                        inline(
                            text,
                            open + 1,
                            text_end - 1,
                            colors,
                            Some(LINK_TEXT),
                            depth + 1,
                        );
                        paint(colors, at, open + 1, LINK_TEXT);
                        paint(colors, text_end - 1, text_end, LINK_TEXT);
                        paint(colors, text_end, label_end, URL);
                        at = label_end;
                    }
                    // A reference definition: `[name]: url`.
                    Some(b':') if at == 0 || text[..at].trim().is_empty() => {
                        paint(colors, at, text_end, LINK_TEXT);
                        paint(colors, text_end + 1, to, URL);
                        at = to;
                    }
                    _ => at += 1,
                }
            }
            b'*' | b'_' if depth < MAX_NESTING => {
                let run = bytes[at..to].iter().take_while(|b| **b == byte).count();
                let intraword = byte == b'_' && at > 0 && bytes[at - 1].is_ascii_alphanumeric();
                let opens = bytes
                    .get(at + run)
                    .is_some_and(|next| !next.is_ascii_whitespace());
                if intraword || !opens {
                    at += run;
                    continue;
                }
                let size = run.min(2);
                let mut close = None;
                let mut scan = at + size + 1;
                while scan + size <= window(at) {
                    let candidate = bytes[scan..scan + size].iter().all(|b| *b == byte)
                        && !bytes[scan - 1].is_ascii_whitespace()
                        && (size == 2 || bytes.get(scan + 1) != Some(&byte))
                        && !(byte == b'_'
                            && bytes
                                .get(scan + size)
                                .is_some_and(u8::is_ascii_alphanumeric));
                    if candidate {
                        close = Some(scan);
                        break;
                    }
                    scan += 1;
                }
                match close {
                    Some(close) => {
                        let color = if size == 2 { STRONG } else { EMPHASIS };
                        inline(text, at + size, close, colors, Some(color), depth + 1);
                        paint(colors, at, at + size, color);
                        paint(colors, close, close + size, color);
                        at = close + size;
                    }
                    None => at += run,
                }
            }
            b'~' if bytes.get(at + 1) == Some(&b'~') => {
                // Searched as bytes: the window's edge can fall inside a
                // multi-byte character, where slicing the str would panic.
                let limit = window(at);
                match (at + 2..limit.saturating_sub(1))
                    .find(|&scan| bytes[scan] == b'~' && bytes[scan + 1] == b'~')
                {
                    Some(close) => {
                        paint(colors, at, close + 2, MUTED);
                        at = close + 2;
                    }
                    None => at += 2,
                }
            }
            b'h' if (at == 0 || !bytes[at - 1].is_ascii_alphanumeric())
                && (text[at..].starts_with("https://") || text[at..].starts_with("http://")) =>
            {
                let mut end = at;
                while end < to
                    && !matches!(bytes[end], b' ' | b'\t' | b')' | b'>' | b'<' | b'"' | b']')
                {
                    end += char_len(text, end);
                }
                while end > at && matches!(bytes[end - 1], b'.' | b',' | b';' | b':') {
                    end -= 1;
                }
                paint(colors, at, end, URL);
                at = end.max(at + 1);
            }
            _ => at += char_len(text, at),
        }
    }
    if let Some(base) = base {
        paint(colors, from, to, base);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Pos;

    fn colored(text: &str) -> Vec<(&str, Color)> {
        line_spans(text, None)
            .into_iter()
            .map(|span| (&text[span.start..span.end], span.color))
            .collect()
    }

    #[test]
    fn headings_rules_and_markers() {
        assert_eq!(
            colored("## [Unreleased]"),
            vec![("## [Unreleased]", HEADING)]
        );
        assert_eq!(colored("#hashtag"), vec![]);
        assert_eq!(colored("---"), vec![("---", PUNCTUATION)]);
        assert_eq!(colored("- [x] done"), vec![("-", MARKER), ("[x]", MARKER)]);
        assert_eq!(colored("12. step"), vec![("12.", MARKER)]);
        assert_eq!(colored("> quoted"), vec![(">", MARKER), (" quoted", MUTED)]);
    }

    #[test]
    fn emphasis_code_and_links() {
        let spans = colored("Some **bold**, *italic*, `code` and [a link](https://x.y).");
        assert!(spans.contains(&("**bold**", STRONG)));
        assert!(spans.contains(&("*italic*", EMPHASIS)));
        assert!(spans.contains(&("`code`", CODE)));
        assert!(spans.contains(&("[a link]", LINK_TEXT)));
        assert!(spans.contains(&("(https://x.y)", URL)));
    }

    #[test]
    fn nested_links_and_bold_keep_their_parts() {
        let spans = colored("**[#45](https://github.com/x)**: text");
        assert!(spans.contains(&("**", STRONG)));
        assert!(spans.contains(&("[#45]", LINK_TEXT)));
        assert!(spans.contains(&("(https://github.com/x)", URL)));
    }

    #[test]
    fn plain_text_is_left_alone() {
        // snake_case, a lone asterisk and a bare bracket are not markup.
        assert_eq!(colored("call some_fn_name now"), vec![]);
        assert_eq!(colored("a * b and [x] later"), vec![]);
        assert!(colored("see https://example.com.").contains(&("https://example.com", URL)));
    }

    #[test]
    fn fenced_code_carries_across_lines() {
        let mut document = Document::new();
        document.replace(
            Pos::default(),
            Pos::default(),
            "text\n```rust\nlet x = **1**;\n```\n# after",
        );
        let mut syntax = MarkdownSyntax::new();
        assert!(syntax.advance_to(&document, 4, 100));
        let color_of = |line| syntax.spans(&document, line).first().map(|span| span.color);
        assert_eq!(color_of(0), None);
        assert_eq!(color_of(1), Some(CODE));
        assert_eq!(color_of(2), Some(CODE));
        assert_eq!(color_of(3), Some(CODE));
        assert_eq!(color_of(4), Some(HEADING));
    }

    #[test]
    fn pathological_lines_stay_fast_and_safe() {
        let brackets = "[".repeat(20_000);
        let stars = "*a ".repeat(7_000);
        let mixed = "é*ü`ñ[ü](é)**ß**~~";
        for text in [
            brackets.as_str(),
            stars.as_str(),
            mixed,
            "<",
            "`",
            "**",
            "[x](",
            "\\",
        ] {
            line_spans(text, None);
        }
    }
}
