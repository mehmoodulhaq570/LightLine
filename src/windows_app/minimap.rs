//! The minimap: a strip left of each code pane's scrollbar showing the file
//! in miniature, one thin row per line and a pixel per character, in the
//! syntax colors, with the lines on screen shaded. Clicking or dragging on it
//! scrolls there. Setting `minimap`.
//!
//! It's drawn from a pixel buffer kept per pane and rebuilt only when the
//! text, its colors or the lines it shows change; after that, drawing it is a
//! single SetDIBitsToDevice. `check_minimap` (message-loop hook, with
//! check_sticky) redraws the strip when what it shows has changed.

use super::*;

/// Debug builds: a live test's question about where things are (see
/// `geometry_for_test`).
#[cfg(debug_assertions)]
pub(super) const GEOMETRY_TEST_MESSAGE: u32 = WM_APP + 96;

/// Fires once typing has paused, to redraw the strips with the new text.
pub(super) const MINIMAP_TIMER: usize = 23;
const MINIMAP_DELAY_MS: u32 = 250;

/// Its width in logical pixels.
pub(super) const MINIMAP_WIDTH: i32 = 84;

/// What the strip showed, to tell when it's stale.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct MinimapShown {
    rect: (i32, i32, i32, i32),
    document: u64,
    serial: u64,
    syntax: u64,
    start: usize,
    // The shaded lines: the view's top and how many lines show.
    top: usize,
    page: usize,
}

/// One pane's pixels, and what they were built from.
pub(super) struct MinimapPixels {
    key: (u64, u64, u64, usize, i32, i32),
    pixels: Vec<u32>,
}

/// Where everything goes in one pane's strip.
struct Layout {
    rect: RECT,
    // Device pixels per line and per character.
    row: i32,
    column: i32,
    // The first line shown (a visual index: folded lines don't count) and
    // how many fit.
    start: usize,
    rows: usize,
    // The view's top line and how many lines it shows, as visual indexes.
    top: usize,
    page: usize,
    total: usize,
}

impl Layout {
    /// Where the shading of the lines on screen goes, top and bottom.
    fn shade(&self) -> (i32, i32) {
        let top = self.rect.top + self.top.saturating_sub(self.start) as i32 * self.row;
        (top, top + self.page as i32 * self.row)
    }

    /// The view's top line that puts the shading's top at `y`. The shading
    /// moves `(rows - 1) / (total - 1)` rows per line scrolled, as `start`
    /// moves with the view.
    fn top_for_shade(&self, y: i32) -> usize {
        let rows_down = ((y - self.rect.top).max(0) / self.row) as usize;
        let top = if self.total <= self.rows {
            rows_down
        } else {
            rows_down * (self.total - 1) / (self.rows - 1).max(1)
        };
        top.min(self.total - 1)
    }
}

// A COLORREF (0x00BBGGRR) as a DIB pixel (0x00RRGGBB).
fn pixel(color: u32) -> u32 {
    ((color & 0xff) << 16) | (color & 0xff00) | ((color >> 16) & 0xff)
}

// `amount` (0..=255) of `over` mixed into `base`, both DIB pixels.
fn mix(base: u32, over: u32, amount: u32) -> u32 {
    let channel = |shift: u32| {
        let from = (base >> shift) & 0xff;
        let to = (over >> shift) & 0xff;
        ((from * (255 - amount) + to * amount) / 255) << shift
    };
    channel(0) | channel(8) | channel(16)
}

impl App {
    /// `pane`'s minimap strip, when it has one: the setting is on, the pane
    /// shows code, and it's wide enough to keep room for the text.
    pub(super) fn minimap_rect(&self, hwnd: HWND, pane: usize) -> Option<RECT> {
        if !self.settings.minimap || self.welcome {
            return None;
        }
        let tab = &self.tabs[self.tab_for_pane(pane)];
        if tab.image.is_some()
            || tab.markdown.is_some()
            || tab.is_placeholder()
            || self.showing_diff()
        {
            return None;
        }
        let (left, right) = (self.pane_left(hwnd, pane), self.pane_right(hwnd, pane));
        let strip = self.scale(MINIMAP_WIDTH);
        let room = right - left - self.scale(GUTTER + PAD + SCROLLBAR) - strip;
        if room < self.scale(360) {
            return None;
        }
        let track_left = right - self.scale(SCROLLBAR);
        let bottom = self.editor_area(hwnd, false).bottom - self.chrome_gap();
        (bottom > self.editor_top()).then_some(RECT {
            left: track_left - strip,
            top: self.editor_top(),
            right: track_left,
            bottom,
        })
    }

    /// How much narrower the minimap makes `pane`'s text.
    pub(super) fn minimap_width(&self, hwnd: HWND, pane: usize) -> i32 {
        self.minimap_rect(hwnd, pane)
            .map_or(0, |rect| rect.right - rect.left)
    }

    fn minimap_layout(&self, hwnd: HWND, pane: usize) -> Option<Layout> {
        let rect = self.minimap_rect(hwnd, pane)?;
        let doc = &self.tabs[self.tab_for_pane(pane)].document;
        let row = self.scale(2).max(2);
        let column = self.scale(1).max(1);
        let rows = ((rect.bottom - rect.top) / row).max(1) as usize;
        let total = doc.visible_line_count().max(1);
        let top = doc.visual_index(self.view_top(hwnd, pane).0).min(total - 1);
        let page = self.visible_lines(hwnd).min(total);
        // A file taller than the strip scrolls in it, in step with the view:
        // its first line at the top, its last at the bottom.
        let start = if total <= rows {
            0
        } else {
            top * (total - rows) / (total - 1).max(1)
        };
        Some(Layout {
            rect,
            row,
            column,
            start,
            rows,
            top,
            page,
            total,
        })
    }

    fn minimap_shown_now(&self, hwnd: HWND, pane: usize) -> Option<MinimapShown> {
        let layout = self.minimap_layout(hwnd, pane)?;
        let doc = &self.tabs[self.tab_for_pane(pane)].document;
        let RECT {
            left,
            top,
            right,
            bottom,
        } = layout.rect;
        Some(MinimapShown {
            rect: (left, top, right, bottom),
            document: doc.id(),
            serial: doc.change_serial(),
            syntax: self.syntax_epoch,
            start: layout.start,
            top: layout.top,
            page: layout.page,
        })
    }

    /// Message-loop hook: redraws a strip whose lines or shading changed
    /// since it was drawn (scrolling, another file) at once. One whose text
    /// or colors changed waits until typing pauses (MINIMAP_TIMER):
    /// rebuilding it after every keystroke cost about a third more work
    /// per key.
    pub(super) fn check_minimap(&self, hwnd: HWND) {
        if !self.settings.minimap || self.welcome || self.tabs.is_empty() {
            return;
        }
        let panes = if self.split_visible { 0..2 } else { 0..1 };
        for pane in panes {
            let Some(now) = self.minimap_shown_now(hwnd, pane) else {
                continue;
            };
            let before = self.minimap_shown.borrow()[pane];
            if before == Some(now) {
                continue;
            }
            let edited = before.is_some_and(|before| {
                MinimapShown {
                    serial: now.serial,
                    syntax: now.syntax,
                    ..before
                } == now
            });
            if edited {
                // Each edit puts the redraw off again; a syntax pass alone
                // doesn't.
                let waiting = self.minimap_waiting.get();
                if waiting != Some(now.serial) {
                    self.minimap_waiting.set(Some(now.serial));
                    unsafe { SetTimer(hwnd, MINIMAP_TIMER, MINIMAP_DELAY_MS, None) };
                }
            } else if let Some(rect) = self.minimap_rect(hwnd, pane) {
                unsafe { InvalidateRect(hwnd, &rect, 0) };
            }
        }
    }

    /// MINIMAP_TIMER: typing paused, the strips are drawn anew.
    pub(super) fn minimap_timer(&self, hwnd: HWND) {
        unsafe { KillTimer(hwnd, MINIMAP_TIMER) };
        self.minimap_waiting.set(None);
        let panes = if self.split_visible { 0..2 } else { 0..1 };
        for pane in panes {
            if let Some(rect) = self.minimap_rect(hwnd, pane) {
                unsafe { InvalidateRect(hwnd, &rect, 0) };
            }
        }
    }

    /// Draws `pane`'s strip, over the text under it.
    pub(super) fn paint_minimap(&self, hdc: HDC, hwnd: HWND, pane: usize) {
        let Some(layout) = self.minimap_layout(hwnd, pane) else {
            self.minimap_shown.borrow_mut()[pane] = None;
            return;
        };
        let rect = layout.rect;
        if unsafe { RectVisible(hdc, &rect) } == 0 {
            return;
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let doc = &self.tabs[self.tab_for_pane(pane)].document;
        let key = (
            doc.id(),
            doc.change_serial(),
            self.syntax_epoch,
            layout.start,
            width,
            height,
        );
        let mut cache = self.minimap_pixels.borrow_mut();
        // Part of the strip drawn with a row of text while its new picture
        // waits for typing to pause: the picture it already shows.
        let same_place = |built: &MinimapPixels| {
            (built.key.0, built.key.3, built.key.4, built.key.5) == (key.0, key.3, key.4, key.5)
        };
        let fully = self.minimap_fully_drawn(hdc, rect);
        let keep = cache[pane]
            .as_ref()
            .is_some_and(|built| built.key == key || (same_place(built) && !fully));
        if !keep {
            cache[pane] = Some(MinimapPixels {
                key,
                pixels: self.minimap_build(pane, &layout, width, height),
            });
        }
        let Some(built) = cache[pane].as_ref() else {
            return;
        };
        // Only the rows this paint reaches are copied out (a keystroke's
        // row redraw passes through the strip), with the lines on screen
        // shaded.
        let mut clip = RECT::default();
        unsafe { GetClipBox(hdc, &mut clip) };
        let from = (clip.top - rect.top).clamp(0, height);
        let to = (clip.bottom - rect.top).clamp(from, height);
        if from == to {
            return;
        }
        let mut pixels = built.pixels[(from * width) as usize..(to * width) as usize].to_vec();
        let shade = pixel(self.theme.text);
        let first = (layout.top.saturating_sub(layout.start) as i32 * layout.row).clamp(from, to);
        let last = ((layout.top + layout.page).saturating_sub(layout.start) as i32 * layout.row)
            .clamp(first, to);
        for value in
            &mut pixels[((first - from) * width) as usize..((last - from) * width) as usize]
        {
            *value = mix(*value, shade, 22);
        }
        let rows = to - from;
        unsafe {
            let mut info: BITMAPINFO = zeroed();
            info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = width;
            info.bmiHeader.biHeight = -rows;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB;
            SetDIBitsToDevice(
                hdc,
                rect.left,
                rect.top + from,
                width as u32,
                rows as u32,
                0,
                0,
                0,
                rows as u32,
                pixels.as_ptr().cast(),
                &info,
                DIB_RGB_COLORS,
            );
        }
        // Only a strip drawn whole is up to date; a part drawn with a row of
        // text is redrawn whole by check_minimap if it changed.
        if fully {
            self.minimap_shown.borrow_mut()[pane] = self.minimap_shown_now(hwnd, pane);
        }
    }

    // Whether the whole of `rect` is inside what this paint draws.
    fn minimap_fully_drawn(&self, hdc: HDC, rect: RECT) -> bool {
        unsafe {
            let clip = CreateRectRgn(0, 0, 0, 0);
            let wanted = CreateRectRgnIndirect(&rect);
            let whole = match GetClipRgn(hdc, clip) {
                // No clip region: everything is drawn.
                0 => true,
                1 => CombineRgn(wanted, wanted, clip, RGN_DIFF) == NULLREGION,
                _ => false,
            };
            DeleteObject(clip);
            DeleteObject(wanted);
            whole
        }
    }

    // The strip's pixels without the shading: each line a row of blocks in
    // the colors its text is drawn in, a little faded.
    fn minimap_build(&self, pane: usize, layout: &Layout, width: i32, height: i32) -> Vec<u32> {
        let background = pixel(self.theme.editor_bg);
        let mut pixels = vec![background; (width * height) as usize];
        let tab = &self.tabs[self.tab_for_pane(pane)];
        let doc = &tab.document;
        let columns = (width / layout.column).max(1) as usize;
        let tab_size = self.settings.tab_size.max(1);
        let text = mix(background, pixel(self.theme.text), 150);
        let mut colors: Vec<u32> = Vec::new();
        let mut line = doc.line_at_visual_index(layout.start);
        for row in 0..layout.rows.min(layout.total - layout.start) {
            let source = doc.line(line);
            // Only the start of a long line shows.
            let shown_bytes = source
                .char_indices()
                .nth(columns * 2)
                .map_or(source.len(), |(byte, _)| byte);
            colors.clear();
            colors.resize(shown_bytes, text);
            if let Some(syntax) = &tab.syntax
                && source.len() <= 16_384
            {
                for span in syntax.spans(doc, line) {
                    let color = mix(background, pixel(self.theme.syntax(span.color)), 170);
                    let end = span.end.min(shown_bytes);
                    if span.start < end {
                        colors[span.start..end].fill(color);
                    }
                }
            }
            let y = row as i32 * layout.row;
            // The last pixel row is left empty between lines.
            let rows_drawn = (layout.row - 1).max(1).min(height - y);
            let mut column = 0usize;
            for (byte, ch) in source[..shown_bytes].char_indices() {
                if column >= columns {
                    break;
                }
                if ch == '\t' {
                    column += tab_size - column % tab_size;
                    continue;
                }
                if !ch.is_whitespace() {
                    let x = column as i32 * layout.column;
                    for dy in 0..rows_drawn {
                        let at = ((y + dy) * width + x) as usize;
                        pixels[at..at + layout.column as usize].fill(colors[byte]);
                    }
                }
                column += 1;
            }
            let next = doc.next_visible_line(line);
            if next == line {
                break;
            }
            line = next;
        }
        pixels
    }

    /// A press on `pane`'s strip, as in VS Code: on the shaded lines it
    /// takes hold of them; elsewhere it scrolls so the line under it is in
    /// the middle of the view, then takes hold. Held, the shading follows
    /// the mouse.
    pub(super) fn minimap_press(&mut self, hwnd: HWND, pane: usize, y: i32) -> bool {
        let Some(layout) = self.minimap_layout(hwnd, pane) else {
            return false;
        };
        let (shade_top, shade_bottom) = layout.shade();
        let mut offset = y - shade_top;
        if y < shade_top || y >= shade_bottom {
            let row = ((y - layout.rect.top).max(0) / layout.row) as usize;
            let line = (layout.start + row).min(layout.total - 1);
            self.scroll_to_index(hwnd, line.saturating_sub(layout.page / 2));
            offset = self
                .minimap_layout(hwnd, pane)
                .map_or(0, |layout| y - layout.shade().0);
        }
        self.minimap_grab = Some((pane, offset));
        unsafe { SetCapture(hwnd) };
        true
    }

    pub(super) fn minimap_drag(&mut self, hwnd: HWND, y: i32) {
        let Some((pane, offset)) = self.minimap_grab else {
            return;
        };
        if let Some(layout) = self.minimap_layout(hwnd, pane) {
            let top = layout.top_for_shade(y - offset);
            self.scroll_to_index(hwnd, top);
        }
    }

    /// Answers GEOMETRY_TEST_MESSAGE for the focused pane, two numbers as
    /// `a << 16 | b`: 0 the minimap's left and top, 1 its right and bottom
    /// (-1 without one); 2 the code's top and the line height; 3 where the
    /// code's text starts and the character width; 4 the caret's line and
    /// byte; 5 the first line shown and the document's line count; 6 the
    /// minimap's pixels per line and how many lines it has room for.
    #[cfg(debug_assertions)]
    pub(super) fn geometry_for_test(&self, hwnd: HWND, what: usize) -> LRESULT {
        let pane = self.focused_pane;
        let pack = |a: i64, b: i64| ((a << 16) | (b & 0xffff)) as LRESULT;
        let cursor = self.view().cursor;
        match (what, self.minimap_rect(hwnd, pane)) {
            (0, Some(rect)) => pack(rect.left.into(), rect.top.into()),
            (1, Some(rect)) => pack(rect.right.into(), rect.bottom.into()),
            (0 | 1, None) => -1,
            (2, _) => pack(self.editor_top().into(), self.line_height.into()),
            (3, _) => pack(
                (self.pane_left(hwnd, pane) + self.scale(GUTTER + PAD)).into(),
                self.char_width.into(),
            ),
            (4, _) => pack(cursor.line as i64, cursor.byte as i64),
            (5, _) => pack(
                self.view_top(hwnd, pane).0 as i64,
                self.doc().line_count() as i64,
            ),
            (6, Some(_)) => self
                .minimap_layout(hwnd, pane)
                .map_or(-1, |layout| pack(layout.row.into(), layout.rows as i64)),
            _ => -1,
        }
    }

    /// Where a click goes: the pane whose strip is at (`x`, `y`).
    pub(super) fn minimap_at(&self, hwnd: HWND, x: i32, y: i32) -> Option<usize> {
        if self.quick_open {
            return None;
        }
        (0..if self.split_visible { 2 } else { 1 }).find(|&pane| {
            self.minimap_rect(hwnd, pane).is_some_and(|rect| {
                x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{mix, pixel};

    #[test]
    fn colors_become_dib_pixels_and_mix() {
        assert_eq!(pixel(0x00_33_22_11), 0x00_11_22_33);
        assert_eq!(mix(0x000000, 0xffffff, 255), 0xffffff);
        assert_eq!(mix(0x000000, 0xffffff, 0), 0x000000);
        assert_eq!(mix(0x00_00_00, 0x00_ff_00_00, 51), 0x00_33_00_00);
    }
}
