//! Horizontal scrolling. With word wrap off a line can be wider than its
//! pane: the view then scrolls sideways to keep the caret in sight (typing,
//! End, clicking near the edge), and Shift+wheel or a tilt wheel / touchpad
//! swipe scrolls it by hand. Each pane keeps its own offset, in pixels, in
//! `EditorView::scroll_x`; a tab that wraps never scrolls sideways.

use super::render::safe_slice_range;
use super::wrap::char_columns;
use super::*;

/// Rows at most this long are measured and drawn from their start, as
/// before horizontal scrolling: cheaper than finding where they come into
/// view. Longer rows (minified files) start drawing near the view's left
/// edge instead (`row_anchor`).
const DRAW_WHOLE_ROW_BYTES: usize = 4_096;

/// How many measured line starts `line_prefix_width` keeps.
const PREFIX_WIDTHS_KEPT: usize = 8;

/// Lines longer than this are drawn without syntax colors, so which of their
/// brackets count doesn't depend on a parse (render/code_pane.rs).
const UNCOLORED_LINE_BYTES: usize = 16_384;

/// The width of the first `end` bytes of `line` of a document, measured in
/// `font` with tabs `tab_size` wide, as the document was at `serial`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PrefixWidth {
    document: u64,
    line: usize,
    end: usize,
    serial: u64,
    font: usize,
    tab_size: usize,
    x: i32,
    // When `end` was found as the byte this many columns in (`row_anchor`).
    columns: Option<usize>,
    // The bracket depth at `end` (`walk_brackets_to`), counting the bracket
    // set and starting from the depth at the line's start given.
    brackets: Option<BracketsAt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BracketsAt {
    set: u8,
    line_start: u32,
    depth: u32,
}

impl PrefixWidth {
    /// Whether the measured text is still the same in `doc`: nothing has
    /// changed since, or only the latest change, which came after it (typing
    /// at or past `end` on the line, or on a line below). Kept up to date by
    /// being asked after each change.
    fn still_holds(&mut self, doc: &Document, font: usize, tab_size: usize) -> bool {
        if self.document != doc.id() || self.font != font || self.tab_size != tab_size {
            return false;
        }
        let serial = doc.change_serial();
        if self.serial == serial {
            return true;
        }
        let after = doc.last_change().is_some_and(|change| {
            self.serial + 1 == serial
                && change.serial == serial
                && change.start
                    >= Pos {
                        line: self.line,
                        byte: self.end,
                    }
        });
        if after {
            self.serial = serial;
        }
        after
    }
}

impl App {
    /// The width of `doc.line(line)[..end]` in the editor font, which must be
    /// selected into `hdc`. A long line's start is measured from the end of a
    /// part of it measured before that hasn't changed since: typing in the
    /// middle of a half-megabyte line then measures the few characters past
    /// the last measurement, not everything before the caret.
    pub(super) fn line_prefix_width(
        &self,
        hdc: HDC,
        doc: &Document,
        line: usize,
        end: usize,
    ) -> i32 {
        let text = doc.line(line);
        if end <= DRAW_WHOLE_ROW_BYTES {
            return self.text_width(hdc, safe_slice_range(text, 0, end));
        }
        let mut kept = self.fresh_prefix_widths(doc);
        if let Some(at) = kept
            .iter()
            .position(|known| known.document == doc.id() && known.line == line && known.end == end)
        {
            let known = kept.remove(at);
            kept.insert(0, known);
            return known.x;
        }
        let from = kept
            .iter()
            .filter(|known| known.document == doc.id() && known.line == line && known.end <= end)
            .max_by_key(|known| known.end)
            .map_or((0, 0), |known| (known.end, known.x));
        let x = from.1 + self.text_width(hdc, safe_slice_range(text, from.0, end));
        kept.insert(
            0,
            PrefixWidth {
                document: doc.id(),
                line,
                end,
                serial: doc.change_serial(),
                font: self.font as usize,
                tab_size: self.settings.tab_size,
                x,
                columns: None,
                brackets: None,
            },
        );
        kept.truncate(PREFIX_WIDTHS_KEPT);
        x
    }

    // The measured line starts, with those of `doc` that changed dropped.
    // Another file's are checked when it's asked about.
    fn fresh_prefix_widths(&self, doc: &Document) -> std::cell::RefMut<'_, Vec<PrefixWidth>> {
        let (font, tab_size) = (self.font as usize, self.settings.tab_size);
        let mut kept = self.prefix_widths.borrow_mut();
        kept.retain_mut(|known| {
            known.document != doc.id() || known.still_holds(doc, font, tab_size)
        });
        kept
    }

    // The kept measurement of `doc.line(line)[..end]`, to note more on.
    fn prefix_entry<'a>(
        kept: &'a mut [PrefixWidth],
        doc: &Document,
        line: usize,
        end: usize,
    ) -> Option<&'a mut PrefixWidth> {
        kept.iter_mut()
            .find(|known| known.document == doc.id() && known.line == line && known.end == end)
    }

    /// The byte `columns` columns into a long `line` of `doc`, and the width
    /// before it: what `row_anchor` asked for last time, while the line's
    /// start hasn't changed (typing to the right of it), is known.
    fn line_byte_at_column(
        &self,
        hdc: HDC,
        doc: &Document,
        line: usize,
        columns: usize,
    ) -> (usize, i32) {
        let known = self
            .fresh_prefix_widths(doc)
            .iter()
            .find(|known| {
                known.document == doc.id() && known.line == line && known.columns == Some(columns)
            })
            .map(|known| known.end);
        let byte = known
            .unwrap_or_else(|| byte_at_column(doc.line(line), columns, self.settings.tab_size));
        // Measured or found, which also keeps it at the front.
        let x = self.line_prefix_width(hdc, doc, line, byte);
        if let Some(entry) =
            Self::prefix_entry(&mut self.prefix_widths.borrow_mut(), doc, line, byte)
        {
            entry.columns = Some(columns);
        }
        (byte, x)
    }

    /// Moves `walker` through `doc.line(line)` up to `byte`, where a row's
    /// drawing starts. On a long line scrolled sideways the depth there is
    /// known from the last paint while the line's start hasn't changed, so
    /// the brackets before it aren't counted again on every keystroke.
    pub(super) fn walk_brackets_to(
        &self,
        walker: &mut super::bracket_colors::BracketWalker,
        doc: &Document,
        line: usize,
        spans: &[lightline::syntax::Span],
        byte: usize,
    ) {
        let text = doc.line(line);
        if text.len() <= UNCOLORED_LINE_BYTES || byte <= DRAW_WHOLE_ROW_BYTES || walker.at() != 0 {
            walker.advance(text, spans, byte);
            return;
        }
        let wanted =
            |known: &BracketsAt| known.set == walker.set() && known.line_start == walker.depth();
        let known = Self::prefix_entry(&mut self.fresh_prefix_widths(doc), doc, line, byte)
            .and_then(|entry| entry.brackets)
            .filter(|known| wanted(known));
        if let Some(known) = known {
            walker.jump(byte, known.depth);
            return;
        }
        let line_start = walker.depth();
        walker.advance(text, spans, byte);
        if let Some(entry) =
            Self::prefix_entry(&mut self.prefix_widths.borrow_mut(), doc, line, byte)
        {
            entry.brackets = Some(BracketsAt {
                set: walker.set(),
                line_start,
                depth: walker.depth(),
            });
        }
    }

    /// How far `pane`'s text is scrolled to the left, in pixels.
    pub(super) fn scroll_x(&self, pane: usize) -> i32 {
        if self.wraps(self.tab_for_pane(pane)) {
            0
        } else {
            self.view_for_pane(pane).scroll_x.max(0)
        }
    }

    /// The width of `pane`'s text area: from where text starts to the
    /// minimap or scrollbar.
    pub(super) fn text_view_width(&self, hwnd: HWND, pane: usize) -> i32 {
        let text_left = self.pane_left(hwnd, pane) + self.scale(GUTTER + PAD);
        (self.pane_right(hwnd, pane)
            - text_left
            - self.scale(PAD + SCROLLBAR)
            - self.minimap_width(hwnd, pane))
        .max(self.char_width.max(1))
    }

    /// The width of `doc.line(line)[row_start..byte]` in the editor font,
    /// selected into `hdc`: through `line_prefix_width` for a row that starts
    /// the line (the only row of a line scrolled sideways, which can be long).
    pub(super) fn row_prefix_width(
        &self,
        hdc: HDC,
        doc: &Document,
        line: usize,
        row_start: usize,
        byte: usize,
    ) -> i32 {
        if row_start == 0 {
            self.line_prefix_width(hdc, doc, line, byte)
        } else {
            self.text_width(hdc, safe_slice_range(doc.line(line), row_start, byte))
        }
    }

    /// Where to start measuring and drawing `text[row_start..row_end]` when
    /// it is scrolled `scroll_x` pixels left: a byte at or before the first
    /// one that shows, and its x from the row's start. Short rows start at
    /// their start. Long ones start near the left edge, found by columns and
    /// checked by measuring once, so a half-megabyte line isn't measured
    /// from its start for every bracket.
    pub(super) fn row_anchor(
        &self,
        hdc: HDC,
        doc: &Document,
        line: usize,
        (row_start, row_end): (usize, usize),
        scroll_x: i32,
    ) -> (usize, i32) {
        if scroll_x <= 0 || row_end.saturating_sub(row_start) <= DRAW_WHOLE_ROW_BYTES {
            return (row_start, 0);
        }
        let text = doc.line(line);
        let Some(row) = text.get(row_start..row_end) else {
            return (row_start, 0);
        };
        // A few columns short of the edge, so a glyph wider than its
        // columns suggest still starts left of it.
        let target = (scroll_x / self.char_width.max(1)) as usize;
        let mut columns = target.saturating_sub(8);
        loop {
            // Scrolled sideways, a line has one row: it starts the line, and
            // where the anchor is and the width before it are usually known
            // (typing to the right of it).
            let (byte, x) = if row_start == 0 {
                self.line_byte_at_column(hdc, doc, line, columns)
            } else {
                let byte = row_start + byte_at_column(row, columns, self.settings.tab_size);
                (byte, self.text_width(hdc, &text[row_start..byte]))
            };
            if x <= scroll_x || columns == 0 {
                return (byte, x);
            }
            // Wider than the columns said: start further left.
            columns /= 2;
        }
    }

    /// Scrolls `pane` sideways so its caret shows, a few characters in from
    /// the edge it went past. Returns whether the offset changed.
    pub(super) fn reveal_caret_x(&mut self, hwnd: HWND, pane: usize) -> bool {
        let tab = self.tab_for_pane(pane);
        if self.wraps(tab) {
            return false;
        }
        let doc = &self.tabs[tab].document;
        let cursor = doc.clamp(self.view_for_pane(pane).cursor);
        if doc.is_line_hidden(cursor.line) {
            return false;
        }
        let x = unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.font);
            let x = self.line_prefix_width(hdc, doc, cursor.line, cursor.byte)
                + self.hint_shift(
                    hdc,
                    self.hints_on_line(tab, cursor.line),
                    0,
                    cursor.byte,
                    false,
                );
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            x
        };
        let width = self.text_view_width(hwnd, pane);
        let margin = (self.char_width * 4).min(width / 4).max(0);
        let old = self.view_for_pane(pane).scroll_x.max(0);
        // Past an edge, the view jumps a quarter of its width (or as far as
        // the caret needs, if more): scrolling redraws the whole pane, so
        // typing at the edge does that every few dozen characters rather
        // than on every key.
        let jump = width / 4;
        let scroll = if x < old + margin {
            (x - margin).min(old - jump).max(0)
        } else if x > old + width - margin {
            (x - width + margin).max(old + jump)
        } else {
            old
        };
        self.set_scroll_x(pane, scroll)
    }

    /// A sideways wheel turn at `point` (client coordinates): a tilt wheel,
    /// a touchpad swipe, or Shift with the ordinary wheel. Scrolls the code
    /// pane under the mouse; `delta` is in wheel units, positive to the
    /// right, 120 a notch.
    pub(super) fn wheel_sideways(&mut self, hwnd: HWND, point: POINT, delta: i32) {
        if self.welcome
            || self.quick_open
            || self.run_choice.is_some()
            || self.run_config_panel.is_some()
            || self.ai_provider.is_some()
            || self.editor_context.is_some()
            || self.showing_diff()
        {
            return;
        }
        let area = self.editor_area(hwnd, false);
        if point.x < area.left
            || point.x >= area.right
            || point.y < area.top
            || point.y >= area.bottom
        {
            return;
        }
        let pane = if self.split_visible {
            usize::from(point.x >= self.pane_divider(hwnd))
        } else {
            self.focused_pane
        };
        let tab = &self.tabs[self.tab_for_pane(pane)];
        if tab.markdown.is_some() || tab.image.is_some() || tab.is_placeholder() {
            return;
        }
        // Popups placed at the caret would be left behind.
        self.cancel_rename(hwnd);
        self.dismiss_code_actions(hwnd);
        self.hide_signature(hwnd);
        // Six characters a notch; a touchpad sends smaller steps.
        self.scroll_pane_x(hwnd, pane, delta * self.char_width.max(1) * 6 / 120);
    }

    /// Scrolls `pane` sideways by `pixels` (positive: towards the lines'
    /// ends), no further than the widest line in view needs.
    pub(super) fn scroll_pane_x(&mut self, hwnd: HWND, pane: usize, pixels: i32) {
        if self.wraps(self.tab_for_pane(pane)) || pixels == 0 {
            return;
        }
        let max = self.max_scroll_x(hwnd, pane);
        let old = self.view_for_pane(pane).scroll_x.max(0);
        // Past the end already (the caret took it there), it only goes back.
        let scroll = if pixels > 0 {
            (old + pixels).min(max.max(old))
        } else {
            (old + pixels).max(0)
        };
        if self.set_scroll_x(pane, scroll) {
            let area = RECT {
                left: self.pane_left(hwnd, pane),
                right: self.pane_right(hwnd, pane),
                ..self.editor_area(hwnd, false)
            };
            unsafe { InvalidateRect(hwnd, &area, 0) };
        }
    }

    /// How far `pane` can scroll: until the end of the widest line in view
    /// is a few characters in from the right edge. Counted in columns, so a
    /// long line isn't measured.
    fn max_scroll_x(&self, hwnd: HWND, pane: usize) -> i32 {
        let doc = &self.tabs[self.tab_for_pane(pane)].document;
        let (mut line, _) = self.view_top(hwnd, pane);
        let mut widest = 0usize;
        for _ in 0..self.visible_lines(hwnd) + 1 {
            let columns: usize = doc
                .line(line)
                .chars()
                .map(|ch| char_columns(ch, self.settings.tab_size))
                .sum();
            widest = widest.max(columns);
            let next = doc.next_visible_line(line);
            if next == line {
                break;
            }
            line = next;
        }
        let width = self.text_view_width(hwnd, pane);
        ((widest as i32 + 4).saturating_mul(self.char_width) - width).max(0)
    }

    fn set_scroll_x(&mut self, pane: usize, scroll: i32) -> bool {
        let tab = self.tab_for_pane(pane);
        let view = &mut self.tabs[tab].views[pane];
        if view.scroll_x == scroll {
            return false;
        }
        view.scroll_x = scroll;
        true
    }
}

/// The byte offset in `row` where display column `columns` starts (or the
/// row's length when it's shorter).
fn byte_at_column(row: &str, columns: usize, tab_size: usize) -> usize {
    let mut used = 0;
    for (byte, ch) in row.char_indices() {
        if used >= columns {
            return byte;
        }
        used += char_columns(ch, tab_size);
    }
    row.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_measured_line_start_holds_until_something_before_its_end_changes() {
        let at = |line, byte| Pos { line, byte };
        let mut doc = Document::new();
        doc.replace(at(0, 0), at(0, 0), "first line\nsecond line here\nthird");
        let measured = |doc: &Document| PrefixWidth {
            document: doc.id(),
            line: 1,
            end: 6,
            serial: doc.change_serial(),
            font: 1,
            tab_size: 4,
            x: 60,
            columns: None,
            brackets: None,
        };
        let mut known = measured(&doc);
        assert!(known.still_holds(&doc, 1, 4));
        // Another font or tab width: measured differently.
        assert!(!known.clone().still_holds(&doc, 2, 4));
        assert!(!known.clone().still_holds(&doc, 1, 8));
        // Typing at its end, past it, and on a line below, one change at a
        // time: still the same text.
        doc.replace(at(1, 6), at(1, 6), "x");
        assert!(known.still_holds(&doc, 1, 4));
        doc.replace(at(1, 9), at(1, 10), "");
        assert!(known.still_holds(&doc, 1, 4));
        doc.replace(at(2, 0), at(2, 0), "y");
        assert!(known.still_holds(&doc, 1, 4));
        // Inside it, or on a line above (which renumbers the lines): gone.
        let mut inside = measured(&doc);
        doc.replace(at(1, 5), at(1, 5), "z");
        assert!(!inside.still_holds(&doc, 1, 4));
        let mut below_an_edit = measured(&doc);
        doc.replace(at(0, 0), at(0, 0), "new\n");
        assert!(!below_an_edit.still_holds(&doc, 1, 4));
        // Two changes since it was last asked about: not known to be safe.
        let mut skipped = measured(&doc);
        doc.replace(at(3, 0), at(3, 0), "a");
        doc.replace(at(3, 1), at(3, 1), "b");
        assert!(!skipped.still_holds(&doc, 1, 4));
        // Another document with the same lines isn't this one.
        let mut other = Document::new();
        other.replace(at(0, 0), at(0, 0), "first line\nsecond line here\nthird");
        assert!(!measured(&doc).still_holds(&other, 1, 4));
    }

    #[test]
    fn columns_map_to_bytes() {
        assert_eq!(byte_at_column("abcdef", 0, 4), 0);
        assert_eq!(byte_at_column("abcdef", 3, 4), 3);
        assert_eq!(byte_at_column("abc", 10, 4), 3);
        // A tab takes tab_size columns; é is two bytes and one column.
        assert_eq!(byte_at_column("\tx", 4, 4), 1);
        assert_eq!(byte_at_column("ééx", 2, 4), 4);
        // A wide character takes two columns.
        assert_eq!(byte_at_column("日x", 2, 4), 3);
    }
}
