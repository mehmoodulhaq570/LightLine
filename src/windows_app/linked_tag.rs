// Renaming the matching tag (`linkedTagRename`), like VS Code's linked
// editing: with the caret in a tag's name in HTML, XML, JSX/TSX, Vue or
// Svelte, both names get a thin outline, and typing a name character or
// Backspace/Delete inside one makes the same change to the other, as one
// undo step.
//
// The pair is found from the text: the matching tag is looked for at most
// MAX_LINES away, counting nested tags of the same name, and only when the
// caret is in a tag's name (`check_tag_pair`, after each key or click, and
// once per caret position). Self-closing tags, HTML's void elements and
// names that differ are left alone, so typing a space or any other
// character simply ends the link.

use super::*;

const MAX_LINES: usize = 2000;
// A tag's attributes may run over this many lines before its `>`.
const MAX_TAG_LINES: usize = 50;

const VOID_ELEMENTS: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// What the pair at the caret was found for: the document, its change
/// serial, the caret, whether text was selected, and the setting.
pub(super) type TagPairKey = (u64, u64, Pos, bool, bool);

/// The pair at the caret (with its document) and what it was found for.
pub(super) type TagPairCache = (Option<TagPairKey>, Option<(u64, TagPair)>);

/// An opening tag's name and its closing tag's name, as byte ranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TagPair {
    pub(super) open: (Pos, Pos),
    pub(super) close: (Pos, Pos),
}

impl TagPair {
    /// The name the caret at `at` is in, and the other one.
    fn split(self, at: Pos) -> Option<((Pos, Pos), (Pos, Pos))> {
        let inside = |(start, end): (Pos, Pos)| start <= at && at <= end;
        if inside(self.open) {
            Some((self.open, self.close))
        } else if inside(self.close) {
            Some((self.close, self.open))
        } else {
            None
        }
    }
}

/// Files whose tags are linked: markup, and JavaScript that may hold JSX.
pub(super) fn links_tags(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "html"
                    | "htm"
                    | "xhtml"
                    | "xml"
                    | "svg"
                    | "vue"
                    | "svelte"
                    | "astro"
                    | "jsx"
                    | "tsx"
                    | "js"
                    | "mjs"
                    | "php"
            )
        })
}

fn name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':')
}

/// The name of the tag starting with `<` at `lt` on `text`: its range and
/// whether it's a closing tag. None when `<` doesn't start a tag.
fn tag_name_at(text: &str, lt: usize) -> Option<(usize, usize, bool)> {
    let bytes = text.as_bytes();
    let closing = bytes.get(lt + 1) == Some(&b'/');
    let start = lt + 1 + usize::from(closing);
    if !bytes.get(start).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    let end = start + bytes[start..].iter().take_while(|&&b| name_char(b)).count();
    Some((start, end, closing))
}

/// Whether the opening tag whose name ends at `from` closes itself (`/>`),
/// looking past its attributes (quoted values skipped) up to its `>`.
fn self_closing(doc: &Document, from: Pos) -> bool {
    let mut quote: Option<u8> = None;
    let last = (from.line + MAX_TAG_LINES).min(doc.line_count() - 1);
    for line in from.line..=last {
        let bytes = doc.line(line).as_bytes();
        let start = if line == from.line { from.byte } else { 0 };
        for (i, &b) in bytes.iter().enumerate().skip(start) {
            match quote {
                Some(q) if b == q => quote = None,
                Some(_) => {}
                None if b == b'"' || b == b'\'' => quote = Some(b),
                None if b == b'>' => return i > 0 && bytes[i - 1] == b'/',
                None => {}
            }
        }
    }
    true
}

/// Every tag on `line`, left to right: (name start, name end, closing).
fn tags_on(text: &str) -> Vec<(usize, usize, bool)> {
    text.match_indices('<')
        .filter_map(|(lt, _)| tag_name_at(text, lt))
        .collect()
}

/// The pair whose name the caret at `at` is in or touches, when the other
/// tag of the pair has the same name.
pub(super) fn tag_pair_at(doc: &Document, at: Pos) -> Option<TagPair> {
    let text = doc.line(at.line);
    let bytes = text.as_bytes();
    if at.byte > bytes.len() {
        return None;
    }
    // The name around the caret.
    let start = at.byte
        - bytes[..at.byte]
            .iter()
            .rev()
            .take_while(|&&b| name_char(b))
            .count();
    let (name_start, name_end, closing) = [start.checked_sub(1), start.checked_sub(2)]
        .into_iter()
        .flatten()
        .find_map(|lt| {
            tag_name_at(text, lt).filter(|&(s, _, _)| s == start && bytes[lt] == b'<')
        })?;
    if at.byte > name_end {
        return None;
    }
    let name = &text[name_start..name_end];
    let here = (
        Pos {
            line: at.line,
            byte: name_start,
        },
        Pos {
            line: at.line,
            byte: name_end,
        },
    );
    if closing {
        let open = find_opening(doc, here.0, name)?;
        Some(TagPair { open, close: here })
    } else {
        if VOID_ELEMENTS
            .iter()
            .any(|void| void.eq_ignore_ascii_case(name))
            || self_closing(doc, here.1)
        {
            return None;
        }
        let close = find_closing(doc, here.1, name)?;
        Some(TagPair { open: here, close })
    }
}

fn range(line: usize, start: usize, end: usize) -> (Pos, Pos) {
    (Pos { line, byte: start }, Pos { line, byte: end })
}

// The closing tag of the `name` opened before `after`, counting nested
// tags of the same name.
fn find_closing(doc: &Document, after: Pos, name: &str) -> Option<(Pos, Pos)> {
    let mut depth = 0usize;
    let last = (after.line + MAX_LINES).min(doc.line_count() - 1);
    for line in after.line..=last {
        let text = doc.line(line);
        for (start, end, closing) in tags_on(text) {
            if (line == after.line && start < after.byte) || &text[start..end] != name {
                continue;
            }
            if closing {
                if depth == 0 {
                    return Some(range(line, start, end));
                }
                depth -= 1;
            } else if !self_closing(doc, Pos { line, byte: end }) {
                depth += 1;
            }
        }
    }
    None
}

// The opening tag of the `name` closed at `before`, counting nested tags of
// the same name.
fn find_opening(doc: &Document, before: Pos, name: &str) -> Option<(Pos, Pos)> {
    let mut depth = 0usize;
    let first = before.line.saturating_sub(MAX_LINES);
    for line in (first..=before.line).rev() {
        let text = doc.line(line);
        for (start, end, closing) in tags_on(text).into_iter().rev() {
            if (line == before.line && start >= before.byte) || &text[start..end] != name {
                continue;
            }
            if closing {
                depth += 1;
            } else if !self_closing(doc, Pos { line, byte: end }) {
                if depth == 0 {
                    return Some(range(line, start, end));
                }
                depth -= 1;
            }
        }
    }
    None
}

impl App {
    /// Whether the active file links tags at all: a cheap check before
    /// typing looks for a pair.
    pub(super) fn links_tags_here(&self) -> bool {
        self.settings.linked_tag_rename && self.doc().path.as_deref().is_some_and(links_tags)
    }

    /// The pair to outline and edit together in the active tab, if any.
    fn tag_pair_here(&self) -> Option<TagPair> {
        if !self.settings.linked_tag_rename
            || self.selection_range().is_some()
            || !self.view().extra.is_empty()
            || self.tab().read_only()
            || !self.doc().path.as_deref().is_some_and(links_tags)
        {
            return None;
        }
        tag_pair_at(self.doc(), self.view().cursor)
    }

    /// After a key or click: finds the pair at the caret when the caret or
    /// the text changed, and redraws the rows whose outline changed.
    pub(super) fn check_tag_pair(&mut self, hwnd: HWND) {
        if self.welcome || self.tabs.is_empty() {
            return;
        }
        let key = (
            self.doc().id(),
            self.doc().change_serial(),
            self.view().cursor,
            self.selection_range().is_some(),
            self.settings.linked_tag_rename,
        );
        if self.tag_pair.0 == Some(key) {
            return;
        }
        let pair = self.tag_pair_here().map(|pair| (self.doc().id(), pair));
        let old = std::mem::replace(&mut self.tag_pair, (Some(key), pair)).1;
        if old != pair {
            let lines: Vec<(usize, usize)> = [old, pair]
                .into_iter()
                .flatten()
                .flat_map(|(_, pair)| [pair.open, pair.close])
                .map(|(start, end)| (start.line, end.line))
                .collect();
            self.invalidate_lines(hwnd, &lines);
        }
    }

    // Redraws `lines` of the active tab in each pane showing it.
    fn invalidate_lines(&self, hwnd: HWND, lines: &[(usize, usize)]) {
        let panes = if self.split_visible { 0..2 } else { 0..1 };
        for pane in panes {
            if self.tab_for_pane(pane) == self.active {
                for area in self.rows_showing(hwnd, pane, lines) {
                    unsafe { InvalidateRect(hwnd, &area, 0) };
                }
            }
        }
    }

    /// The names to outline in tab `index`: the pair at the caret, when it
    /// is for that tab's document as it is now.
    pub(super) fn tag_outline(&self, index: usize) -> Option<TagPair> {
        let (key, pair) = &self.tag_pair;
        let (document, pair) = (*pair)?;
        let tab = &self.tabs[index];
        (index == self.active
            && document == tab.document.id()
            && key.is_some_and(|key| key.1 == tab.document.change_serial()))
        .then_some(pair)
    }

    /// Typing `text` (name characters) at the caret, in a tag name with a
    /// matching tag: both names get it, as one undo step. False when the
    /// caret isn't in such a name, for the usual typing to happen.
    pub(super) fn type_in_linked_tag(&mut self, hwnd: HWND, text: &str) -> bool {
        if !text.bytes().all(name_char) {
            return false;
        }
        let cursor = self.view().cursor;
        let Some((here, other)) = self.tag_pair_here().and_then(|pair| pair.split(cursor)) else {
            return false;
        };
        let offset = cursor.byte - here.0.byte;
        let mirror = Pos {
            line: other.0.line,
            byte: other.0.byte + offset,
        };
        self.linked_edit(hwnd, (cursor, cursor), (mirror, mirror), text);
        true
    }

    /// Backspace (`forward` false) or Delete at the caret in a linked tag
    /// name: the same character goes from both names. False when it isn't
    /// such a name, or the name would be left empty.
    pub(super) fn delete_in_linked_tag(&mut self, hwnd: HWND, forward: bool) -> bool {
        let cursor = self.view().cursor;
        let Some(pair) = self.tag_pair_here() else {
            return false;
        };
        let Some((here, other)) = pair.split(cursor) else {
            return false;
        };
        let length = here.1.byte - here.0.byte;
        let offset = cursor.byte - here.0.byte;
        if length < 2 || (forward && offset == length) || (!forward && offset == 0) {
            return false;
        }
        // Names are ASCII-only (name_char), so a character is a byte.
        let (from, to) = if forward {
            (offset, offset + 1)
        } else {
            (offset - 1, offset)
        };
        let at = |start: Pos, byte: usize| Pos {
            line: start.line,
            byte: start.byte + byte,
        };
        self.linked_edit(
            hwnd,
            (at(here.0, from), at(here.0, to)),
            (at(other.0, from), at(other.0, to)),
            "",
        );
        true
    }

    // Replaces `here` (at the caret) and `mirror` (in the other name) with
    // `text` in one undo group, the later one first so the earlier one's
    // positions hold, and leaves the caret after `here`'s new text.
    fn linked_edit(&mut self, hwnd: HWND, here: (Pos, Pos), mirror: (Pos, Pos), text: &str) {
        let mirror_first = mirror.0 > here.0;
        self.doc_mut().begin_group();
        if mirror_first {
            self.replace_range(mirror.0, mirror.1, text);
            self.replace_range(here.0, here.1, text);
        } else {
            self.replace_range(here.0, here.1, text);
            self.replace_range(mirror.0, mirror.1, text);
        }
        self.doc_mut().end_group();
        // The mirror's change shifts a caret after it on the same line.
        let shift = if !mirror_first && mirror.0.line == here.0.line {
            text.len() as isize - (mirror.1.byte - mirror.0.byte) as isize
        } else {
            0
        };
        let caret = Pos {
            line: here.0.line,
            byte: (here.0.byte + text.len()).saturating_add_signed(shift),
        };
        self.view_mut().cursor = self.doc().clamp(caret);
        self.view_mut().selection_anchor = None;
        self.invalidate_lines(hwnd, &[(mirror.0.line, mirror.0.line)]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        let mut doc = Document::new();
        doc.seed(text);
        doc
    }

    fn names(doc: &Document, pair: TagPair) -> (String, String) {
        let get = |(start, end): (Pos, Pos)| doc.line(start.line)[start.byte..end.byte].to_owned();
        (get(pair.open), get(pair.close))
    }

    #[test]
    fn finds_the_matching_tag_across_lines_and_nesting() {
        let text = "<div class=\"a\">\n  <div>\n    <p>Hi</p>\n  </div>\n</div>";
        let d = doc(text);
        let pair = tag_pair_at(&d, Pos { line: 0, byte: 2 }).unwrap();
        assert_eq!(pair.close.0, Pos { line: 4, byte: 2 });
        assert_eq!(names(&d, pair), ("div".into(), "div".into()));
        // From the closing tag back, past the nested div.
        let pair = tag_pair_at(&d, Pos { line: 4, byte: 5 }).unwrap();
        assert_eq!(pair.open.0, Pos { line: 0, byte: 1 });
        // The inner pair, with the caret right after the name.
        let pair = tag_pair_at(&d, Pos { line: 1, byte: 6 }).unwrap();
        assert_eq!(pair.close.0, Pos { line: 3, byte: 4 });
        // Not in a name.
        assert!(tag_pair_at(&d, Pos { line: 2, byte: 8 }).is_none());
        assert!(tag_pair_at(&d, Pos { line: 0, byte: 0 }).is_none());
    }

    #[test]
    fn self_closing_void_and_unmatched_tags_are_left_alone() {
        let d = doc("<Card title=\"a/b\" />\n<br>\n<img src=x>\n<span>\n<b>x</i>");
        assert!(tag_pair_at(&d, Pos { line: 0, byte: 2 }).is_none());
        assert!(tag_pair_at(&d, Pos { line: 1, byte: 2 }).is_none());
        assert!(tag_pair_at(&d, Pos { line: 2, byte: 2 }).is_none());
        assert!(tag_pair_at(&d, Pos { line: 3, byte: 2 }).is_none());
        assert!(tag_pair_at(&d, Pos { line: 4, byte: 2 }).is_none());
        // A self-closing tag of the same name inside doesn't count.
        let d = doc("<Row>\n  <Row />\n</Row>");
        let pair = tag_pair_at(&d, Pos { line: 0, byte: 1 }).unwrap();
        assert_eq!(pair.close.0, Pos { line: 2, byte: 2 });
    }

    #[test]
    fn jsx_on_one_line_and_comparisons() {
        let d = doc("return a < b ? <Item.Row key={x}>{x}</Item.Row> : null;");
        let pair = tag_pair_at(&d, Pos { line: 0, byte: 18 }).unwrap();
        assert_eq!(names(&d, pair), ("Item.Row".into(), "Item.Row".into()));
        assert!(tag_pair_at(&d, Pos { line: 0, byte: 10 }).is_none());
    }

    #[test]
    fn which_files_link_tags() {
        assert!(links_tags(Path::new("a/index.HTML")));
        assert!(links_tags(Path::new("App.tsx")));
        assert!(!links_tags(Path::new("main.rs")));
        assert!(!links_tags(Path::new("types.ts")));
    }
}
