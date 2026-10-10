// Hover feedback for buttons (#62): the one under the mouse lights up, and
// an icon-only one says what it does in a small tip beside it. Moving onto
// or off a button redraws only it and its tip; moving within one redraws
// nothing, and nothing runs while the mouse is still.
//
// Hover and clicks share the same geometry: `hot_at` is what the rail and
// title-bar clicks use too, and the Welcome screen's targets are the ones
// its clicks use.

use super::render::welcome_rail_tip;
use super::*;

/// Debug builds: a live test's mouse move to (LOWORD, HIWORD of lParam);
/// see `App::hover_for_test`.
#[cfg(debug_assertions)]
pub(super) const HOVER_TEST_MESSAGE: u32 = WM_APP + 98;

/// The fill behind a button under the mouse, in the active theme.
pub(super) fn hover_fill() -> u32 {
    ui(28, 42, 68)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hot {
    // The editor's left rail: ☰, the six views, and settings.
    RailMenu,
    Rail(usize),
    RailSettings,
    // Minimize, maximize/restore, close.
    TitleButton(usize),
    CommandCenter,
    // A Welcome screen target, by its index in `WelcomeLayout::targets`.
    Welcome(usize),
}

impl App {
    /// The button at (x, y), if any. None while a popup has the mouse.
    pub(super) fn hot_at(&self, hwnd: HWND, x: i32, y: i32) -> Option<Hot> {
        if self.run_choice.is_some()
            || self.run_config_panel.is_some()
            || self.editor_context.is_some()
            || self.more_menu.is_some()
            || self.quick_open
            || self.debug_config_menu_open
        {
            return None;
        }
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let chrome_top = self.chrome_top();
        if y < chrome_top {
            let button = self.scale(46);
            let controls_left = client.right - button * 3;
            if x >= controls_left {
                return Some(Hot::TitleButton(
                    ((x - controls_left) / button.max(1)).clamp(0, 2) as usize,
                ));
            }
            let command = self.command_center_rect(hwnd);
            return (x >= command.left
                && x < command.right
                && y >= command.top
                && y < command.bottom)
                .then_some(Hot::CommandCenter);
        }
        if self.welcome {
            let layout = self.welcome_layout(client);
            return layout
                .targets
                .iter()
                .position(|(rect, _)| {
                    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
                })
                .map(Hot::Welcome);
        }
        if x < self.scale(RAIL) && y < client.bottom - self.scale(STATUS) {
            return self.rail_item_at(y - chrome_top, client.bottom - chrome_top);
        }
        None
    }

    /// The rail item at `y` in the rail's own coordinates (from the top of
    /// the window's chrome), the rail being `bottom` tall to the window's
    /// bottom edge.
    pub(super) fn rail_item_at(&self, y: i32, bottom: i32) -> Option<Hot> {
        let s = |value: i32| self.scale(value);
        if y >= s(RAIL_MENU_ROW) && y < s(RAIL_FIRST_ROW) {
            Some(Hot::RailMenu)
        } else if y >= s(RAIL_FIRST_ROW) && y < s(RAIL_FIRST_ROW + RAIL_ROW * 6) {
            Some(Hot::Rail(
                ((y - s(RAIL_FIRST_ROW)) / s(RAIL_ROW).max(1)) as usize,
            ))
        } else if y >= bottom - s(STATUS + 40) {
            Some(Hot::RailSettings)
        } else {
            None
        }
    }

    /// Where rail item `hot` is drawn, in the rail's own coordinates, with
    /// the rail's editor area ending at `editor_bottom`.
    pub(super) fn rail_item_rect(&self, hot: Hot, editor_bottom: i32) -> Option<RECT> {
        let s = |value: i32| self.scale(value);
        let top = match hot {
            Hot::RailMenu => s(RAIL_MENU_ROW),
            Hot::Rail(index) => s(RAIL_FIRST_ROW + index as i32 * RAIL_ROW),
            Hot::RailSettings => editor_bottom - s(40),
            _ => return None,
        };
        let bottom = match hot {
            Hot::RailSettings => editor_bottom - s(4),
            _ => top + s(42),
        };
        Some(RECT {
            left: s(7),
            top,
            right: s(RAIL - 7),
            bottom,
        })
    }

    // Where `hot` is in the window.
    fn hot_rect(&self, hwnd: HWND, hot: Hot) -> Option<RECT> {
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let chrome_top = self.chrome_top();
        match hot {
            Hot::TitleButton(index) => {
                let button = self.scale(46);
                let left = client.right - button * 3 + button * index as i32;
                Some(RECT {
                    left,
                    top: 0,
                    right: left + button,
                    bottom: chrome_top,
                })
            }
            Hot::CommandCenter => Some(self.command_center_rect(hwnd)),
            Hot::Welcome(index) => self
                .welcome_layout(client)
                .targets
                .get(index)
                .map(|(rect, _)| *rect),
            rail => {
                let editor_bottom = client.bottom - self.scale(STATUS) - chrome_top;
                self.rail_item_rect(rail, editor_bottom).map(|rect| RECT {
                    top: rect.top + chrome_top,
                    bottom: rect.bottom + chrome_top,
                    ..rect
                })
            }
        }
    }

    // What an icon-only button does.
    fn hot_tip(&self, hwnd: HWND, hot: Hot) -> Option<&'static str> {
        Some(match hot {
            Hot::RailMenu => "Show or hide the side panel (Ctrl+B)",
            Hot::Rail(0) => "Explorer",
            Hot::Rail(1) => "Search in files (Ctrl+Shift+F)",
            Hot::Rail(2) => "Source control (Ctrl+Shift+G)",
            Hot::Rail(3) => "Run and debug (Ctrl+Shift+D)",
            Hot::Rail(4) => "Extensions (Ctrl+Shift+X)",
            Hot::Rail(5) => "AI Assistant",
            Hot::RailSettings => "Settings (Ctrl+,)",
            Hot::TitleButton(0) => "Minimize",
            Hot::TitleButton(1) if unsafe { IsZoomed(hwnd) } != 0 => "Restore down",
            Hot::TitleButton(1) => "Maximize",
            Hot::TitleButton(_) => "Close",
            Hot::Welcome(index) => {
                let mut client = RECT::default();
                unsafe { GetClientRect(hwnd, &mut client) };
                return welcome_rail_tip(&self.welcome_layout(client).targets, index);
            }
            _ => return None,
        })
    }

    // The tip's card: right of a rail button, or under a title-bar button.
    fn tip_layout(&self, hwnd: HWND, hot: Hot) -> Option<(RECT, &'static str)> {
        let text = self.hot_tip(hwnd, hot)?;
        let item = self.hot_rect(hwnd, hot)?;
        let s = |value: i32| self.scale(value);
        let width = unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.ui_font);
            let width = self.text_width(hdc, text);
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            width
        } + s(20);
        let height = s(28);
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let card = if let Hot::TitleButton(_) = hot {
            let right = item.right.min(client.right - s(6));
            RECT {
                left: right - width,
                top: item.bottom + s(4),
                right,
                bottom: item.bottom + s(4) + height,
            }
        } else {
            let middle = (item.top + item.bottom) / 2;
            let top = (middle - height / 2).min(client.bottom - s(STATUS) - height);
            RECT {
                left: s(RAIL) + s(6),
                top,
                right: s(RAIL) + s(6) + width,
                bottom: top + height,
            }
        };
        Some((card, text))
    }

    /// After the mouse moved to (x, y): lights up what's under it.
    pub(super) fn update_hot(&mut self, hwnd: HWND, x: i32, y: i32) {
        self.set_hot(hwnd, x, y, true);
    }

    // `track`: ask to hear when the mouse leaves the window. A test (see
    // HOVER_TEST_MESSAGE) drives the window with the real mouse elsewhere,
    // and Windows would then report the leave at once.
    fn set_hot(&mut self, hwnd: HWND, x: i32, y: i32, track: bool) {
        let hot = self.hot_at(hwnd, x, y);
        if hot == self.hot {
            return;
        }
        self.invalidate_hot(hwnd);
        self.hot = hot;
        self.invalidate_hot(hwnd);
        if hot.is_some() && track {
            // To hear when the mouse leaves the window, which would leave
            // the button lit.
            let mut leave = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe { TrackMouseEvent(&mut leave) };
        }
    }

    /// Debug builds: hover at (x, y) as a mouse move would, without
    /// watching for the mouse leaving (see `set_hot`).
    #[cfg(debug_assertions)]
    pub(super) fn hover_for_test(&mut self, hwnd: HWND, x: i32, y: i32) {
        self.set_hot(hwnd, x, y, false);
    }

    pub(super) fn clear_hot(&mut self, hwnd: HWND) {
        if self.hot.is_some() {
            self.invalidate_hot(hwnd);
            self.hot = None;
        }
    }

    fn invalidate_hot(&self, hwnd: HWND) {
        let Some(hot) = self.hot else {
            return;
        };
        if let Some(rect) = self.hot_rect(hwnd, hot) {
            unsafe { InvalidateRect(hwnd, &rect, 0) };
        }
        if let Some((tip, _)) = self.tip_layout(hwnd, hot) {
            unsafe { InvalidateRect(hwnd, &tip, 0) };
        }
    }

    /// The command center's border: brighter under the mouse.
    pub(super) fn command_center_edge(&self) -> u32 {
        if self.hot == Some(Hot::CommandCenter) {
            ui(86, 130, 210)
        } else {
            ui(43, 76, 132)
        }
    }

    /// The fill behind a hovered title-bar button: red for close, as in
    /// Windows. Drawn before the button's glyph.
    pub(super) fn paint_hot_title_button(&self, hdc: HDC, hwnd: HWND) {
        if let Some(hot @ Hot::TitleButton(index)) = self.hot
            && let Some(rect) = self.hot_rect(hwnd, hot)
        {
            Self::fill(
                hdc,
                rect,
                if index == 2 {
                    ui(196, 43, 28)
                } else {
                    hover_fill()
                },
            );
        }
    }

    /// An outline around a hovered Welcome target. Drawn after the page.
    pub(super) fn paint_hot_welcome(&self, hdc: HDC, hwnd: HWND) {
        if let Some(hot @ Hot::Welcome(_)) = self.hot
            && let Some(rect) = self.hot_rect(hwnd, hot)
        {
            self.card_outline(hdc, rect, self.scale(8), self.theme.sky);
        }
    }

    /// The tip of the button under the mouse. Drawn over everything.
    pub(super) fn paint_hot_tip(&self, hdc: HDC, hwnd: HWND) {
        let Some((card, text)) = self.hot.and_then(|hot| self.tip_layout(hwnd, hot)) else {
            return;
        };
        let s = |value: i32| self.scale(value);
        self.panel_card(hdc, card, s(6), self.theme.edge, self.theme.sidebar_bg);
        unsafe {
            let old = SelectObject(hdc, self.ui_font);
            self.label_mid(
                hdc,
                text,
                card.left + s(10),
                (card.top + card.bottom) / 2,
                self.theme.text,
                card,
            );
            SelectObject(hdc, old);
        }
    }
}
