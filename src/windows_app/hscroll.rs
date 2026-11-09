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

impl App {
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

    /// Where to start measuring and drawing `text[row_start..row_end]` when
    /// it is scrolled `scroll_x` pixels left: a byte at or before the first
    /// one that shows, and its x from the row's start. Short rows start at
    /// their start. Long ones start near the left edge, found by columns and
    /// checked by measuring once, so a half-megabyte line isn't measured
    /// from its start for every bracket.
    pub(super) fn row_anchor(
        &self,
        hdc: HDC,
        text: &str,
        (row_start, row_end): (usize, usize),
        scroll_x: i32,
    ) -> (usize, i32) {
        if scroll_x <= 0 || row_end.saturating_sub(row_start) <= DRAW_WHOLE_ROW_BYTES {
            return (row_start, 0);
        }
        let Some(row) = text.get(row_start..row_end) else {
            return (row_start, 0);
        };
        // A few columns short of the edge, so a glyph wider than its
        // columns suggest still starts left of it.
        let target = (scroll_x / self.char_width.max(1)) as usize;
        let mut columns = target.saturating_sub(8);
        loop {
            let byte = row_start + byte_at_column(row, columns, self.settings.tab_size);
            let x = self.text_width(hdc, &text[row_start..byte]);
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
        let line = doc.line(cursor.line);
        let x = unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.font);
            let x = self.text_width(hdc, safe_slice_range(line, 0, cursor.byte))
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
