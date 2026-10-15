//! Bracket pair colors and the active indent guide.
//!
//! Brackets are colored by how deeply they're nested, cycling through three
//! colors; those in strings and comments don't count. The depth at the start
//! of each line is kept per tab and worked out only as far down as the view
//! reaches, again only from the first line an edit (or a syntax pass) changed.
//! Setting `bracketPairColorization`.
//!
//! The indent guide of the block holding the caret is drawn brighter. Both
//! are kept up to date on screen by `check_brackets` (message-loop hook),
//! which redraws only the rows whose bracket depth changed and the guide's
//! old and new columns.

use super::*;

/// How many lines one paint may work through to find a depth; past that a
/// view far down a huge file starts counting at its first line.
const SCAN_LIMIT: usize = 50_000;

/// Depth at the start of each line, for the lines `0..depths.len()`.
#[derive(Default)]
pub(super) struct BracketDepths {
    document: u64,
    serial: u64,
    depths: Vec<u32>,
}

impl BracketDepths {
    /// A syntax pass recolored lines from `line` on: brackets there may have
    /// moved into or out of a string.
    pub(super) fn invalidate_from(&mut self, line: usize) {
        self.depths.truncate(line + 1);
    }
}

/// What the hook last saw in a pane: the depth at each shown line's start,
/// and the active guide.
#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct BracketsShown {
    document: u64,
    first: usize,
    depths: Vec<u32>,
    guide: Option<ActiveGuide>,
}

/// The caret's block: the guide at `level`, down lines `start..=end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ActiveGuide {
    pub(super) level: usize,
    pub(super) start: usize,
    pub(super) end: usize,
}

fn in_string_or_comment(spans: &[lightline::syntax::Span], byte: usize) -> bool {
    spans.iter().any(|span| {
        span.start <= byte
            && byte < span.end
            && matches!(span.color, Color::String | Color::Comment)
    })
}

// The brackets of `text` that count, with whether each opens.
fn brackets<'a>(
    text: &'a str,
    spans: &'a [lightline::syntax::Span],
) -> impl Iterator<Item = (usize, bool)> + 'a {
    text.bytes().enumerate().filter_map(move |(byte, ch)| {
        let opens = match ch {
            b'(' | b'[' | b'{' => true,
            b')' | b']' | b'}' => false,
            _ => return None,
        };
        (!in_string_or_comment(spans, byte)).then_some((byte, opens))
    })
}

/// Columns of indentation at the start of `text`, tabs to the next stop.
pub(super) fn indent_columns(text: &str, tab_size: usize) -> usize {
    let mut columns = 0;
    for ch in text.chars() {
        match ch {
            ' ' => columns += 1,
            '\t' => columns += tab_size - columns % tab_size,
            _ => break,
        }
    }
    columns
}

impl App {
    fn colors_brackets(&self, tab: &Tab) -> bool {
        self.settings.bracket_pair_colors
            && tab.markdown.is_none()
            && !tab.binary_preview
            && matches!(&tab.syntax, Some(syntax) if !matches!(syntax, Syntax::Markdown(_)))
    }

    /// The three colors nested brackets cycle through.
    pub(super) fn bracket_palette(&self) -> [u32; 3] {
        if self.theme.is_light() {
            [ui(4, 49, 250), ui(49, 147, 49), ui(123, 56, 20)]
        } else {
            [ui(255, 215, 0), ui(218, 112, 214), ui(23, 159, 255)]
        }
    }

    /// The bracket depth at the start of `line` of tab `index`, working out
    /// what's missing; None when brackets aren't colored there.
    fn depth_at(&self, index: usize, line: usize) -> Option<u32> {
        let tab = &self.tabs[index];
        if !self.colors_brackets(tab) {
            return None;
        }
        let doc = &tab.document;
        let syntax = tab.syntax.as_ref()?;
        let mut cache = tab.bracket_depths.borrow_mut();
        // Since it was last used: one edit is redone from its line; anything
        // else (another file, several edits) from the top.
        if cache.document != doc.id() {
            *cache = BracketDepths {
                document: doc.id(),
                ..BracketDepths::default()
            };
        } else if cache.serial != doc.change_serial() {
            match doc.last_change() {
                Some(change)
                    if change.serial == doc.change_serial()
                        && cache.serial + 1 == doc.change_serial() =>
                {
                    cache.invalidate_from(change.start.line);
                }
                _ => cache.depths.clear(),
            }
        }
        cache.serial = doc.change_serial();
        let line = line.min(doc.line_count().saturating_sub(1));
        if cache.depths.is_empty() {
            cache.depths.push(0);
        }
        if line + 1 > cache.depths.len() + SCAN_LIMIT {
            return Some(0);
        }
        while cache.depths.len() <= line {
            let at = cache.depths.len() - 1;
            let text = doc.line(at);
            let spans = if text.len() <= 16_384 {
                syntax.spans(doc, at)
            } else {
                Vec::new()
            };
            let depth = brackets(text, &spans).fold(cache.depths[at], |depth, (_, opens)| {
                if opens {
                    depth + 1
                } else {
                    depth.saturating_sub(1)
                }
            });
            cache.depths.push(depth);
        }
        Some(cache.depths[line])
    }

    /// The colored brackets of `line` of tab `index`: byte and color.
    pub(super) fn line_bracket_colors(
        &self,
        index: usize,
        line: usize,
        spans: &[lightline::syntax::Span],
    ) -> Vec<(usize, u32)> {
        let Some(mut depth) = self.depth_at(index, line) else {
            return Vec::new();
        };
        let palette = self.bracket_palette();
        let text = self.tabs[index].document.line(line);
        brackets(text, spans)
            .map(|(byte, opens)| {
                let level = if opens {
                    depth += 1;
                    depth - 1
                } else {
                    depth = depth.saturating_sub(1);
                    depth
                };
                (byte, palette[level as usize % palette.len()])
            })
            .collect()
    }

    /// The color of the bracket at `at` in tab `index`, if it's colored.
    pub(super) fn bracket_color_at(&self, index: usize, at: Pos) -> Option<u32> {
        let tab = &self.tabs[index];
        let spans = match &tab.syntax {
            Some(syntax) if tab.document.line(at.line).len() <= 16_384 => {
                syntax.spans(&tab.document, at.line)
            }
            _ => Vec::new(),
        };
        self.line_bracket_colors(index, at.line, &spans)
            .into_iter()
            .find(|(byte, _)| *byte == at.byte)
            .map(|(_, color)| color)
    }

    /// The guide of the block holding `pane`'s caret: the caret line's own
    /// level, down the lines around it indented at least as far (blank
    /// lines included).
    pub(super) fn active_guide(&self, pane: usize) -> Option<ActiveGuide> {
        if !self.settings.indent_guides {
            return None;
        }
        let doc = &self.tabs[self.tab_for_pane(pane)].document;
        let tab_size = self.settings.tab_size.max(1);
        let line = doc.clamp(self.view_for_pane(pane).cursor).line;
        let level = indent_columns(doc.line(line), tab_size) / tab_size;
        if level == 0 || doc.line(line).trim().is_empty() {
            return None;
        }
        let inside = |at: usize| {
            let text = doc.line(at);
            text.trim().is_empty() || indent_columns(text, tab_size) >= level * tab_size
        };
        let mut start = line;
        while start > 0 && line - start < 2_000 && inside(start - 1) {
            start -= 1;
        }
        let mut end = line;
        let last = doc.line_count() - 1;
        while end < last && end - line < 2_000 && inside(end + 1) {
            end += 1;
        }
        // Blank lines at the block's edges belong outside it.
        while start < line && doc.line(start).trim().is_empty() {
            start += 1;
        }
        while end > line && doc.line(end).trim().is_empty() {
            end -= 1;
        }
        Some(ActiveGuide { level, start, end })
    }

    /// Indentation levels with a guide on `line`: its own, or for a blank
    /// line, the lesser of the lines around it (looking at most 100 away).
    pub(super) fn guide_levels(&self, doc: &Document, line: usize) -> usize {
        let tab_size = self.settings.tab_size.max(1);
        let text = doc.line(line);
        if !text.trim().is_empty() {
            return indent_columns(text, tab_size) / tab_size;
        }
        let around = |lines: &mut dyn Iterator<Item = usize>| {
            lines
                .take(100)
                .find(|&at| !doc.line(at).trim().is_empty())
                .map_or(0, |at| indent_columns(doc.line(at), tab_size) / tab_size)
        };
        let above = around(&mut (0..line).rev());
        let below = around(&mut (line + 1..doc.line_count()));
        above.min(below)
    }

    // The x of the guide at `level` in `pane`.
    pub(super) fn guide_x(&self, hwnd: HWND, pane: usize, level: usize, space_width: i32) -> i32 {
        self.pane_left(hwnd, pane)
            + self.scale(GUTTER + PAD)
            + (level * self.settings.tab_size.max(1)) as i32 * space_width
            - self.scale(4)
    }

    /// Message-loop hook: redraws the rows whose bracket depth changed (an
    /// edit above them, or a string closed) and the active guide's columns
    /// when the caret moved to another block.
    pub(super) fn check_brackets(&self, hwnd: HWND) {
        if self.welcome || self.tabs.is_empty() {
            return;
        }
        let panes = if self.split_visible { 0..2 } else { 0..1 };
        for pane in panes {
            let index = self.tab_for_pane(pane);
            let doc = &self.tabs[index].document;
            let first = self.view_top(hwnd, pane).0;
            let mut depths = Vec::new();
            let mut line = first;
            if self.colors_brackets(&self.tabs[index]) {
                for _ in 0..=self.visible_lines(hwnd) {
                    let Some(depth) = self.depth_at(index, line) else {
                        break;
                    };
                    depths.push(depth);
                    let next = doc.next_visible_line(line);
                    if next == line {
                        break;
                    }
                    line = next;
                }
            }
            let now = BracketsShown {
                document: doc.id(),
                first,
                depths,
                guide: self.active_guide(pane),
            };
            let before =
                std::mem::replace(&mut self.brackets_shown.borrow_mut()[pane], now.clone());
            if before == now {
                continue;
            }
            // Rows whose depth changed, when the same lines are shown; when
            // the view or file changed, it's being redrawn anyway.
            if before.document == now.document && before.first == now.first {
                let mut line = first;
                let mut changed = Vec::new();
                for (at, depth) in now.depths.iter().enumerate() {
                    if before.depths.get(at) != Some(depth) {
                        changed.push((line, line));
                    }
                    line = doc.next_visible_line(line);
                }
                for area in self.rows_showing(hwnd, pane, &changed) {
                    unsafe { InvalidateRect(hwnd, &area, 0) };
                }
            }
            if before.guide != now.guide {
                self.invalidate_guides(hwnd, pane, [before.guide, now.guide]);
            }
        }
    }

    // The columns of `guides`, down the rows of their lines.
    fn invalidate_guides(&self, hwnd: HWND, pane: usize, guides: [Option<ActiveGuide>; 2]) {
        let space_width = self.char_width.max(1);
        for guide in guides.into_iter().flatten() {
            let x = self.guide_x(hwnd, pane, guide.level, space_width);
            for area in self.rows_showing(hwnd, pane, &[(guide.start, guide.end)]) {
                let column = RECT {
                    left: x - 1,
                    right: x + 2,
                    ..area
                };
                unsafe { InvalidateRect(hwnd, &column, 0) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightline::syntax::Span;

    #[test]
    fn brackets_in_strings_and_comments_dont_count() {
        let text = r#"f("(", x) // )"#;
        let spans = [
            Span {
                start: 2,
                end: 5,
                color: Color::String,
            },
            Span {
                start: 10,
                end: 14,
                color: Color::Comment,
            },
        ];
        let found: Vec<(usize, bool)> = brackets(text, &spans).collect();
        assert_eq!(found, vec![(1, true), (8, false)]);
    }

    #[test]
    fn indentation_counts_tabs_to_the_next_stop() {
        assert_eq!(indent_columns("    x", 4), 4);
        assert_eq!(indent_columns("\tx", 4), 4);
        assert_eq!(indent_columns("  \tx", 4), 4);
        assert_eq!(indent_columns("x", 4), 0);
        assert_eq!(indent_columns("      ", 4), 6);
    }
}
