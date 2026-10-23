// Mouse-wheel scrolling without redrawing the whole pane. After a notch most
// of the pane's rows are still on screen, only higher or lower, so they are
// moved inside the backbuffer and only what that leaves out is drawn: the
// rows scrolled into view, the pinned sticky lines, the scrollbar and the
// strip along the card's rounded bottom. Redrawing the whole pane took
// ~14.5 ms a notch, more than half of it drawing ~40 rows of text.

use super::*;

/// Debug builds: answers how many backbuffer pixels differ from a full
/// redraw, a live test's check that scrolling left nothing stale.
#[cfg(debug_assertions)]
pub(super) const SCROLL_CHECK_MESSAGE: u32 = WM_APP + 97;

/// Handles SCROLL_CHECK_MESSAGE. Not borrowing `cell` while it paints.
#[cfg(debug_assertions)]
pub(super) fn check_for_test(hwnd: HWND, cell: &RefCell<App>) -> LRESULT {
    let pixels = || {
        let app = cell.borrow();
        let buffer = app.backbuffer.as_ref()?;
        let mut bits = vec![0u32; (buffer.width * buffer.height) as usize];
        let size = (bits.len() * 4) as i32;
        let read = unsafe { GetBitmapBits(buffer.bitmap, size, bits.as_mut_ptr().cast()) };
        (read == size).then_some(bits)
    };
    unsafe { UpdateWindow(hwnd) };
    let Some(shown) = pixels() else {
        return -1;
    };
    unsafe {
        InvalidateRect(hwnd, null(), 0);
        UpdateWindow(hwnd);
    }
    let Some(redrawn) = pixels() else {
        return -1;
    };
    if shown.len() != redrawn.len() {
        return -1;
    }
    let width = cell
        .borrow()
        .backbuffer
        .as_ref()
        .map_or(1, |b| b.width.max(1)) as usize;
    let differing: Vec<usize> = shown
        .iter()
        .zip(&redrawn)
        .enumerate()
        .filter(|(_, (a, b))| (*a ^ *b) & 0x00ff_ffff != 0)
        .map(|(index, _)| index)
        .collect();
    // Where they are, for the test to report (LIGHTLINE_SCROLL_CHECK_LOG).
    if let (Some(log), Some(first)) = (
        std::env::var_os("LIGHTLINE_SCROLL_CHECK_LOG"),
        differing.first(),
    ) {
        let (mut left, mut top, mut right, mut bottom) =
            (first % width, first / width, first % width, first / width);
        for index in &differing {
            let (x, y) = (index % width, index / width);
            left = left.min(x);
            right = right.max(x);
            top = top.min(y);
            bottom = bottom.max(y);
        }
        let raw = |pixels: &[u32]| {
            pixels
                .iter()
                .flat_map(|p| p.to_le_bytes())
                .collect::<Vec<u8>>()
        };
        let _ = std::fs::write(format!("{}.shown", log.to_string_lossy()), raw(&shown));
        let _ = std::fs::write(format!("{}.redrawn", log.to_string_lossy()), raw(&redrawn));
        let _ = std::fs::write(
            log,
            format!(
                "{left} {top} {right} {bottom} shown {:06x} redrawn {:06x}",
                shown[*first] & 0xffffff,
                redrawn[*first] & 0xffffff
            ),
        );
    }
    differing.len() as LRESULT
}

impl App {
    /// Scrolls `pane` by `rows` screen rows and asks for the redraw.
    pub(super) fn wheel_scroll(&mut self, hwnd: HWND, pane: usize, rows: isize) {
        let sticky_before = self.sticky_shown.borrow()[pane].len();
        let moved = self.scroll_pane_rows(hwnd, pane, rows);
        if moved == 0 || self.blit_scroll(hwnd, pane, moved, sticky_before) {
            return;
        }
        // A popup or menu may reach past the panes.
        if self.keystroke_stays_in_editor() {
            let area = self.editor_area(hwnd, false);
            unsafe { InvalidateRect(hwnd, &area, 0) };
        } else {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    /// Whether nothing is drawn over the code panes that would move with
    /// their rows: a popup, card, menu or tip, the search preview or a
    /// crossfade.
    fn nothing_over_panes(&self, hwnd: HWND) -> bool {
        self.keystroke_stays_in_editor()
            && !self.quick_open
            && self.find_layout(hwnd).is_none()
            && self.rename_box.is_none()
            && self.signature.is_none()
            && self.run_choice.is_none()
            && self.run_config_panel.is_none()
            && self.ai_provider.is_none()
            && self.hot.is_none()
            && self.transition.is_none()
            && !(self.side_view == SideView::Search && self.panel_focus && !self.search_input)
            && !self.showing_diff()
    }

    /// Moves what `pane` shows by `moved` screen rows (positive: the view
    /// went down, so the rows go up) inside the backbuffer and invalidates
    /// only what has to be drawn. False, having changed nothing, when the
    /// pane must be redrawn whole.
    fn blit_scroll(&mut self, hwnd: HWND, pane: usize, moved: isize, sticky_before: usize) -> bool {
        if !self.nothing_over_panes(hwnd) {
            return false;
        }
        let Some((track, _)) = self.scrollbar(hwnd, pane) else {
            return false;
        };
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        // Paint replaces a backbuffer of another size, drawing it whole.
        let Some(buffer) = self
            .backbuffer
            .as_ref()
            .filter(|buffer| buffer.width == client.right && buffer.height == client.bottom)
        else {
            return false;
        };
        // Where the code pane stops drawing: a gap above the status bar or
        // the terminal.
        let bottom = self.editor_area(hwnd, false).bottom - self.chrome_gap();
        // From the card's left edge (or the split divider), which are the
        // same on every row, to the scrollbar, above the card's rounded
        // bottom corners.
        // The minimap doesn't move with the rows: it's drawn again.
        let minimap = self.minimap_rect(hwnd, pane);
        // It ends on a row boundary: rows scrolled in at the bottom then fill
        // whole rows, and only one row (cut by the corners) is drawn below.
        let top = self.editor_top();
        let rows = (bottom - self.scale(CARD_RADIUS) - top) / self.line_height.max(1);
        let area = RECT {
            left: self.pane_left(hwnd, pane),
            top,
            right: minimap.map_or(track.left, |strip| strip.left),
            bottom: top + rows * self.line_height,
        };
        let shift = -(moved as i32) * self.line_height;
        // Sticky scroll's pinned lines, before and after, are drawn anew.
        // Only rows that were below the old ones and land below the new ones
        // are moved: moving the old pinned lines along with the rows meant
        // redrawing the rows they landed on, about as many again as the
        // notch brought into view.
        let strip = |count: usize| {
            if count == 0 {
                0
            } else {
                count as i32 * self.line_height + self.scale(1).max(1)
            }
        };
        let sticky_after = self.sticky_lines(hwnd, pane).len();
        let to_top = (area.top + strip(sticky_after)).max(area.top + strip(sticky_before) + shift);
        let to_bottom = area.bottom.min(area.bottom + shift);
        let kept = to_bottom - to_top;
        if kept <= 0 || area.right <= area.left {
            return false;
        }
        let moved_rows = RECT {
            top: to_top,
            bottom: to_bottom,
            ..area
        };
        unsafe {
            // Parts waiting to be redrawn move with the rows they're on.
            let pending = CreateRectRgn(0, 0, 0, 0);
            let within = CreateRectRgnIndirect(&area);
            if pending.is_null() || within.is_null() {
                DeleteObject(pending);
                DeleteObject(within);
                return false;
            }
            if GetUpdateRgn(hwnd, pending, 0) > NULLREGION {
                CombineRgn(pending, pending, within, RGN_AND);
                OffsetRgn(pending, 0, shift);
                CombineRgn(pending, pending, within, RGN_AND);
                InvalidateRgn(hwnd, pending, 0);
            }
            DeleteObject(pending);
            DeleteObject(within);
            BitBlt(
                buffer.dc,
                area.left,
                to_top,
                area.right - area.left,
                kept,
                buffer.dc,
                area.left,
                to_top - shift,
                SRCCOPY,
            );
        }
        let pane_right = self.pane_right(hwnd, pane);
        let mut redraw = vec![
            // The pinned lines and the rows scrolled into view under them...
            RECT {
                bottom: to_top,
                ..area
            },
            // ...or at the bottom...
            RECT {
                top: to_bottom,
                ..area
            },
            // ...the strip along the bottom corners, which shows part of a
            // row...
            RECT {
                top: area.bottom,
                bottom,
                right: pane_right,
                ..area
            },
            // ...and the scrollbar.
            RECT {
                bottom: track.bottom.min(bottom),
                ..track
            },
        ];
        redraw.extend(minimap);
        for rect in redraw.iter().filter(|rect| rect.bottom > rect.top) {
            unsafe { InvalidateRect(hwnd, rect, 0) };
        }
        // The moved rows are in the backbuffer but not yet on screen: paint
        // copies them out with what it draws.
        self.blitted = Some(match self.blitted {
            Some(earlier) => RECT {
                left: earlier.left.min(moved_rows.left),
                top: earlier.top.min(moved_rows.top),
                right: earlier.right.max(moved_rows.right),
                bottom: earlier.bottom.max(moved_rows.bottom),
            },
            None => moved_rows,
        });
        true
    }
}
