// Sticky scroll: the first lines of the blocks the top of the view is inside
// (an impl, a function, a loop) stay pinned above the code, as in VS Code. A
// click on one scrolls back to it. The blocks are found from indentation,
// looking at most SCAN lines up, so it costs next to nothing; after each key
// or click `check_sticky` redraws the pinned lines only if they changed.

use super::render::safe_slice_range;
use super::*;

// At most this many pinned lines (and never more than a third of the view).
const MAX_STICKY: usize = 5;
// How far up to look for the start of a block.
const SCAN: usize = 2_000;

// The indentation of `line` in columns, None when it's blank.
fn indent_of(line: &str, tab_size: usize) -> Option<usize> {
    let mut columns = 0;
    for ch in line.chars() {
        match ch {
            ' ' => columns += 1,
            '\t' => columns += tab_size - columns % tab_size,
            _ => return Some(columns),
        }
    }
    None
}

/// The lines starting the blocks that line `probe` is inside, outermost
/// first: going up, each line indented less than the last one found.
/// Comments and lines that only close a block don't count.
fn block_starts(doc: &Document, probe: usize, tab_size: usize, max: usize) -> Vec<usize> {
    let count = doc.line_count();
    // A blank line belongs with the code after it.
    let Some((mut level, start)) = (probe..count.min(probe + 100))
        .find_map(|line| indent_of(doc.line(line), tab_size).map(|indent| (indent, line)))
    else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut line = start;
    let stop = start.saturating_sub(SCAN);
    while line > stop && level > 0 {
        line -= 1;
        if doc.is_line_hidden(line) {
            continue;
        }
        let text = doc.line(line);
        let Some(indent) = indent_of(text, tab_size) else {
            continue;
        };
        if indent >= level {
            continue;
        }
        let code = text.trim();
        let closes_only = code
            .chars()
            .all(|ch| matches!(ch, '}' | ')' | ']' | ';' | ','));
        let comment = ["//", "/*", "*", "#"]
            .iter()
            .any(|start| code.starts_with(start));
        if closes_only || comment {
            continue;
        }
        found.push(line);
        level = indent;
    }
    found.reverse();
    found.truncate(max);
    found
}

impl App {
    /// The lines pinned at the top of `pane`, outermost first.
    pub(super) fn sticky_lines(&self, hwnd: HWND, pane: usize) -> Vec<usize> {
        let tab = &self.tabs[self.tab_for_pane(pane)];
        if !self.settings.sticky_scroll
            || self.welcome
            || tab.image.is_some()
            || tab.markdown.is_some()
            || tab.is_placeholder()
        {
            return Vec::new();
        }
        let doc = &tab.document;
        let (top, _) = self.view_top(hwnd, pane);
        if top == 0 {
            return Vec::new();
        }
        let tab_size = self.settings.tab_size.max(1);
        let max = MAX_STICKY.min(self.visible_lines(hwnd) / 3);
        // The pinned lines cover the first rows, so what decides them is the
        // line showing just below them.
        let first = block_starts(doc, top, tab_size, max);
        let mut probe = top;
        for _ in 0..first.len() {
            probe = (probe + 1..doc.line_count())
                .find(|&line| !doc.is_line_hidden(line))
                .unwrap_or(probe);
        }
        block_starts(doc, probe, tab_size, max)
    }

    /// Draws `pane`'s pinned lines over the top of its code, with their
    /// numbers and colors, and a line under them.
    pub(super) fn paint_sticky(&self, hdc: HDC, hwnd: HWND, pane: usize, left: i32, right: i32) {
        let lines = self.sticky_lines(hwnd, pane);
        if let Some(shown) = self.sticky_shown.borrow_mut().get_mut(pane) {
            shown.clone_from(&lines);
        }
        if lines.is_empty() {
            return;
        }
        let top = self.editor_top();
        let height = lines.len() as i32 * self.line_height;
        let right = right - self.scale(SCROLLBAR);
        let edge = self.scale(1).max(1);
        let strip = RECT {
            left,
            top,
            right,
            bottom: top + height + edge,
        };
        unsafe {
            if RectVisible(hdc, &strip) == 0 {
                return;
            }
            SetBkMode(hdc, TRANSPARENT as i32);
        }
        let tab = &self.tabs[self.tab_for_pane(pane)];
        let doc = &tab.document;
        let code_left = left + self.scale(GUTTER + PAD);
        Self::fill(
            hdc,
            RECT {
                bottom: top + height,
                ..strip
            },
            self.theme.editor_bg,
        );
        Self::fill(
            hdc,
            RECT {
                left: left + self.scale(GUTTER) - edge,
                right: left + self.scale(GUTTER),
                bottom: top + height,
                ..strip
            },
            self.theme.edge,
        );
        let tab_text = " ".repeat(self.settings.tab_size);
        for (slot, &line) in lines.iter().enumerate() {
            let y = top + slot as i32 * self.line_height;
            let number = (line + 1).to_string();
            let number_right = left + self.scale(GUTTER) - self.scale(GUTTER_NUMBER_RIGHT_INSET);
            let wide: Vec<u16> = number.encode_utf16().collect();
            let source = doc.line(line);
            let clip = RECT {
                left: code_left,
                top: y,
                right,
                bottom: y + self.line_height,
            };
            let draw = |from: usize, to: usize, color: u32| {
                let text = safe_slice_range(source, from, to).replace('\t', &tab_text);
                let wide: Vec<u16> = text.encode_utf16().collect();
                let x = code_left + self.text_width(hdc, safe_slice_range(source, 0, from));
                unsafe {
                    SetTextColor(hdc, color);
                    ExtTextOutW(
                        hdc,
                        x,
                        y,
                        ETO_CLIPPED,
                        &clip,
                        wide.as_ptr(),
                        wide.len() as u32,
                        null(),
                    );
                }
            };
            unsafe {
                SetTextColor(hdc, self.theme.line_number);
                ExtTextOutW(
                    hdc,
                    number_right - self.text_width(hdc, &number),
                    y,
                    ETO_CLIPPED,
                    &RECT {
                        left,
                        top: y,
                        right: number_right,
                        bottom: y + self.line_height,
                    },
                    wide.as_ptr(),
                    wide.len() as u32,
                    null(),
                );
            }
            draw(0, source.len(), self.theme.text);
            if let Some(syntax) = tab.syntax.as_ref().filter(|_| source.len() <= 16_384) {
                for span in syntax.spans(doc, line) {
                    if span.start < span.end.min(source.len()) {
                        draw(
                            span.start,
                            span.end.min(source.len()),
                            self.theme.syntax(span.color),
                        );
                    }
                }
            }
        }
        Self::fill(
            hdc,
            RECT {
                top: top + height,
                ..strip
            },
            self.theme.edge,
        );
    }

    /// A click on a pinned line scrolls the view to show it just under the
    /// lines pinned above it, with the caret at its start. True when the
    /// click was on one.
    pub(super) fn sticky_click(&mut self, hwnd: HWND, pane: usize, y: i32) -> bool {
        let lines = self
            .sticky_shown
            .borrow()
            .get(pane)
            .cloned()
            .unwrap_or_default();
        let slot = ((y - self.editor_top()) / self.line_height.max(1)).max(0) as usize;
        let Some(&line) = lines.get(slot) else {
            return false;
        };
        let tab_index = self.tab_for_pane(pane);
        let doc = &self.tabs[tab_index].document;
        // `slot` visible lines above it go to the top.
        let mut first = line;
        for _ in 0..slot {
            first = (0..first)
                .rev()
                .find(|&above| !doc.is_line_hidden(above))
                .unwrap_or(first);
        }
        let text = doc.line(line);
        let byte = text.len() - text.trim_start().len();
        let view = self.view_mut();
        view.first_line = first;
        view.first_row = 0;
        self.move_cursor(Pos { line, byte }, false);
        unsafe { InvalidateRect(hwnd, null(), 0) };
        true
    }

    /// After a key or click: redraws the pinned lines of a pane whose lines
    /// changed (an edit can start or end a block above the view).
    pub(super) fn check_sticky(&self, hwnd: HWND) {
        if self.welcome || self.tabs.is_empty() {
            return;
        }
        let panes = if self.split_visible { 0..2 } else { 0..1 };
        for pane in panes {
            let lines = self.sticky_lines(hwnd, pane);
            let shown = self
                .sticky_shown
                .borrow()
                .get(pane)
                .cloned()
                .unwrap_or_default();
            if lines != shown {
                let rows = lines.len().max(shown.len()) as i32;
                let area = RECT {
                    left: self.pane_left(hwnd, pane),
                    top: self.editor_top(),
                    right: self.pane_right(hwnd, pane),
                    bottom: self.editor_top() + rows * self.line_height + self.scale(1).max(1),
                };
                unsafe { InvalidateRect(hwnd, &area, 0) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        let mut doc = Document::default();
        doc.replace(Pos::default(), Pos::default(), text);
        doc
    }

    #[test]
    fn blocks_are_found_by_indentation() {
        let text = "\
impl Thing {
    // helpers
    fn first() {
        a();
    }

    fn second() {
        if ready {
            b();
        }
        c();
    }
}
";
        let doc = doc(text);
        // Inside `if ready`: impl, fn second, if.
        assert_eq!(block_starts(&doc, 8, 4, 5), [0, 6, 7]);
        // After the if closed: impl, fn second; the `}` doesn't count.
        assert_eq!(block_starts(&doc, 10, 4, 5), [0, 6]);
        // A blank line goes with the code after it.
        assert_eq!(block_starts(&doc, 5, 4, 5), [0]);
        // At the top level, nothing.
        assert_eq!(block_starts(&doc, 0, 4, 5), Vec::<usize>::new());
        // At most `max`, the outermost first.
        assert_eq!(block_starts(&doc, 8, 4, 2), [0, 6]);
    }

    #[test]
    fn else_lines_start_blocks() {
        let doc = doc("if a {\n    x();\n} else {\n    y();\n}\n");
        assert_eq!(block_starts(&doc, 3, 4, 5), [2]);
    }
}
