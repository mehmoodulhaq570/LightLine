// Short motion (design/layout-smooth-design.html): the side panel and the
// terminal ease open and closed, wheel and Page Up/Down scrolls glide, and a
// button under the mouse fades in. Each runs on one-shot 16 ms frames that
// stop when it ends, so nothing wakes while idle, and typing is never
// animated. The `animations` setting and Windows' "Show animations" turn all
// of it off.

use super::*;

pub(super) const TERMINAL_TIMER: usize = 25;
pub(super) const SCROLL_TIMER: usize = 26;
pub(super) const HOVER_TIMER: usize = 27;
const FRAME_MS: u32 = 16;
/// How long the side panel and the terminal take to open or close.
pub(super) const PANEL_MS: f32 = 120.0;
const HOVER_MS: f32 = 90.0;

/// Fast at first, settling at the end.
pub(super) fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Windows' "Show animations in Windows" (Accessibility > Visual effects).
pub(super) fn system_animations() -> bool {
    let mut on: i32 = 1;
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut on as *mut i32).cast(),
            0,
        )
    };
    read == 0 || on != 0
}

impl App {
    /// Whether anything may animate.
    pub(super) fn motion(&self) -> bool {
        self.settings.animations && self.system_animations
    }

    /// The terminal panel's height on screen, in pixels: while it eases,
    /// what the animation has reached.
    pub(super) fn terminal_panel(&self) -> i32 {
        if self.terminal_anim.is_some() {
            self.scale(self.terminal_shown)
        } else if self.terminal_visible {
            self.scale(self.terminal_height)
        } else {
            0
        }
    }

    /// Called when a frame starts: the terminal was shown or hidden since
    /// the last one, so it starts easing there. Every place that shows or
    /// hides it only sets `terminal_visible`.
    pub(super) fn sync_terminal_motion(&mut self, hwnd: HWND) {
        if self.terminal_visible == self.terminal_was_visible {
            return;
        }
        self.terminal_was_visible = self.terminal_visible;
        if !self.motion() || self.welcome {
            self.terminal_anim = None;
            unsafe { KillTimer(hwnd, TERMINAL_TIMER) };
            return;
        }
        let from = if self.terminal_anim.is_some() {
            self.terminal_shown
        } else if self.terminal_visible {
            0
        } else {
            self.terminal_height
        };
        self.terminal_shown = from;
        self.terminal_anim = Some((Instant::now(), from));
        unsafe { SetTimer(hwnd, TERMINAL_TIMER, FRAME_MS, None) };
    }

    pub(super) fn advance_terminal(&mut self, hwnd: HWND) {
        let Some((started, from)) = self.terminal_anim else {
            unsafe { KillTimer(hwnd, TERMINAL_TIMER) };
            return;
        };
        let target = if self.terminal_visible {
            self.terminal_height
        } else {
            0
        };
        let t = started.elapsed().as_secs_f32() * 1000.0 / PANEL_MS;
        self.terminal_shown = from + ((target - from) as f32 * ease_out(t)).round() as i32;
        if t >= 1.0 {
            self.terminal_anim = None;
            unsafe { KillTimer(hwnd, TERMINAL_TIMER) };
        }
        // The code panes and the panel share what moved: everything between
        // the tabs and the status bar.
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let area = RECT {
            left: self.editor_left(),
            top: self.editor_top(),
            right: client.right,
            bottom: client.bottom - self.scale(STATUS),
        };
        unsafe { InvalidateRect(hwnd, &area, 0) };
    }

    /// A wheel notch or a page: scrolls `pane` by `rows` screen rows,
    /// gliding there when smooth scrolling is on. The first row moves at
    /// once, so it never feels later than a jump.
    pub(super) fn glide_scroll(&mut self, hwnd: HWND, pane: usize, rows: isize) {
        if rows == 0 {
            return;
        }
        if !(self.settings.smooth_scrolling && self.motion()) {
            self.stop_glide(hwnd);
            self.wheel_scroll(hwnd, pane, rows);
            return;
        }
        // Notches the same way add up; turning back starts over.
        let pending = match self.scroll_glide {
            Some((glide_pane, left)) if glide_pane == pane && left.signum() == rows.signum() => {
                left + rows
            }
            _ => rows,
        };
        self.scroll_glide = Some((pane, pending));
        self.advance_glide(hwnd);
    }

    /// One frame of a glide: a third of what's left, at least a row.
    pub(super) fn advance_glide(&mut self, hwnd: HWND) {
        let Some((pane, pending)) = self.scroll_glide else {
            unsafe { KillTimer(hwnd, SCROLL_TIMER) };
            return;
        };
        let step = (pending.abs() + 2) / 3 * pending.signum();
        let moved = self.wheel_scroll(hwnd, pane, step);
        let left = pending - step;
        if left == 0 || moved != step {
            self.stop_glide(hwnd);
        } else {
            self.scroll_glide = Some((pane, left));
            unsafe { SetTimer(hwnd, SCROLL_TIMER, FRAME_MS, None) };
        }
    }

    /// Page Up/Down moved the focused pane's view from `before`: it glides
    /// there instead of jumping.
    pub(super) fn glide_from(&mut self, hwnd: HWND, before: (usize, usize)) {
        let pane = self.focused_pane;
        let after = self.view_top(hwnd, pane);
        if after == before || !(self.settings.smooth_scrolling && self.motion()) {
            return;
        }
        let direction = if after > before { 1 } else { -1 };
        let limit = self.visible_lines(hwnd) as isize * 2 + 2;
        let (mut top, mut rows) = (before, 0isize);
        while top != after && rows.abs() < limit {
            let next = self.step_rows(hwnd, pane, top, direction);
            if next == top {
                break;
            }
            top = next;
            rows += direction;
        }
        // Not reached in rows (it shouldn't happen): keep the jump.
        if top != after {
            return;
        }
        self.set_view_top(before);
        self.glide_scroll(hwnd, pane, rows);
    }

    /// Ends a glide where it is: a key, a click or a jump takes over.
    pub(super) fn stop_glide(&mut self, hwnd: HWND) {
        if self.scroll_glide.take().is_some() {
            unsafe { KillTimer(hwnd, SCROLL_TIMER) };
        }
    }

    /// How far the hovered button has faded in, 0 to 1.
    pub(super) fn hot_level(&self) -> f32 {
        if !self.motion() {
            return 1.0;
        }
        (self.hot_since.elapsed().as_secs_f32() * 1000.0 / HOVER_MS).min(1.0)
    }

    /// The mouse moved onto a button: its highlight fades in.
    pub(super) fn start_hot_fade(&mut self, hwnd: HWND) {
        self.hot_since = Instant::now();
        if self.hot.is_some() && self.motion() {
            unsafe { SetTimer(hwnd, HOVER_TIMER, FRAME_MS, None) };
        }
    }

    pub(super) fn advance_hot_fade(&mut self, hwnd: HWND) {
        self.invalidate_hot(hwnd);
        if self.hot.is_none() || self.hot_level() >= 1.0 {
            unsafe { KillTimer(hwnd, HOVER_TIMER) };
        }
    }

    /// `color` faded in from `base` as far as the hover has gone.
    pub(super) fn hot_color(&self, base: u32, color: u32) -> u32 {
        super::render::blend(base, color, self.hot_level())
    }
}

#[cfg(test)]
mod tests {
    use super::ease_out;

    #[test]
    fn easing_starts_and_ends_in_place() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_out(2.0), 1.0);
        assert!(ease_out(0.5) > 0.5);
    }
}
