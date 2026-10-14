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
    shown
        .iter()
        .zip(&redrawn)
        .filter(|(a, b)| (*a ^ *b) & 0x00ff_ffff != 0)
        .count() as LRESULT
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
            && self.hot.is_none()
            && self.transition.is_none()
            && !(self.side_view == SideView::Search && self.panel_focus && !self.search_input)
            && !(self.side_view == SideView::Review && self.review_file.is_some())
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
        let bottom = self.editor_area(hwnd, false).bottom;
        // From the card's left edge (or the split divider), which are the
        // same on every row, to the scrollbar, above the card's rounded
        // bottom corners.
        let area = RECT {
            left: self.pane_left(hwnd, pane),
            top: self.editor_top(),
            right: track.left,
            bottom: bottom - self.scale(CARD_RADIUS) - self.chrome_gap(),
        };
        let shift = -(moved as i32) * self.line_height;
        let kept = area.bottom - area.top - shift.abs();
        if kept <= 0 || area.right <= area.left {
            return false;
        }
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
                area.top + shift.max(0),
                area.right - area.left,
                kept,
                buffer.dc,
                area.left,
                area.top - shift.min(0),
                SRCCOPY,
            );
        }
        let pane_right = self.pane_right(hwnd, pane);
        let mut redraw = vec![
            // The rows scrolled into view...
            if shift < 0 {
                RECT {
                    top: area.top + kept,
                    ..area
                }
            } else {
                RECT {
                    bottom: area.top + shift,
                    ..area
                }
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
            track,
        ];
        // The pinned lines, before and after: the old ones moved with the
        // rows.
        let sticky_after = self.sticky_lines(hwnd, pane).len();
        if sticky_before > 0 || sticky_after > 0 {
            let edge = self.scale(1).max(1);
            let height = (sticky_before as i32 * self.line_height + edge + shift.max(0))
                .max(sticky_after as i32 * self.line_height + edge);
            redraw.push(RECT {
                bottom: (area.top + height).min(area.bottom),
                ..area
            });
        }
        for rect in &redraw {
            unsafe { InvalidateRect(hwnd, rect, 0) };
        }
        // The moved rows are in the backbuffer but not yet on screen: paint
        // copies them out with what it draws.
        self.blitted = Some(match self.blitted {
            Some(earlier) => RECT {
                left: earlier.left.min(area.left),
                top: earlier.top.min(area.top),
                right: earlier.right.max(area.right),
                bottom: earlier.bottom.max(area.bottom),
            },
            None => area,
        });
        true
    }
}
