// The menu behind the "..." button at the right end of each pane's
// breadcrumb row. The button used to be drawn with nothing behind it, so a
// click did nothing (#57). Like the editor's right-click menu it is painted
// in the backbuffer, so it follows the color theme, and painting and clicks
// share `more_menu_layout`.

use super::*;

const WIDTH: i32 = 250;
const ROW: i32 = 30;
const INSET: i32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MoreAction {
    Split,
    Find,
    Replace,
    WordWrap,
    Format,
    Run,
    RunConfigurations,
    Close,
    Palette,
}

pub(super) struct MoreMenu {
    pane: usize,
    highlighted: Option<usize>,
}

struct MoreLayout {
    menu: RECT,
    rows: Vec<(MoreAction, RECT)>,
}

impl App {
    fn more_actions(&self) -> Vec<MoreAction> {
        let mut actions = vec![MoreAction::Split];
        // Image, binary and Markdown preview tabs have no text to edit.
        if !self.tab().read_only() {
            actions.extend([
                MoreAction::Find,
                MoreAction::Replace,
                MoreAction::WordWrap,
                MoreAction::Format,
                MoreAction::Run,
                MoreAction::RunConfigurations,
            ]);
        }
        actions.extend([MoreAction::Close, MoreAction::Palette]);
        actions
    }

    fn more_label(&self, action: MoreAction) -> (&'static str, &'static str) {
        match action {
            MoreAction::Split if self.split_visible => ("Close Split", "Ctrl+\\"),
            MoreAction::Split => ("Split Editor", "Ctrl+\\"),
            MoreAction::Find => ("Find", "Ctrl+F"),
            MoreAction::Replace => ("Replace", "Ctrl+H"),
            MoreAction::WordWrap => ("Toggle Word Wrap", "Alt+Z"),
            MoreAction::Format => ("Format Document", "Shift+Alt+F"),
            MoreAction::Run => ("Run", "Ctrl+Shift+R"),
            MoreAction::RunConfigurations => ("Run Configurations...", ""),
            MoreAction::Close => ("Close Editor", "Ctrl+W"),
            MoreAction::Palette => ("Command Palette", "Ctrl+Shift+P"),
        }
    }

    /// Opens the menu under `pane`'s "..." button, or closes it if it's open.
    pub(super) fn toggle_more_menu(&mut self, hwnd: HWND, pane: usize) {
        self.more_menu = if self.more_menu.is_some() {
            None
        } else {
            Some(MoreMenu {
                pane,
                highlighted: None,
            })
        };
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn dismiss_more_menu(&mut self, hwnd: HWND) {
        if self.more_menu.take().is_some() {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    fn more_menu_layout(&self, hwnd: HWND) -> Option<MoreLayout> {
        let menu = self.more_menu.as_ref()?;
        let s = |value: i32| self.scale(value);
        let (_, button) = self.pane_actions(self.pane_right(hwnd, menu.pane));
        let actions = self.more_actions();
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let width = s(WIDTH).min(client.right - s(16));
        // Right-aligned under the button, kept inside the window.
        let left = (button.right - width).clamp(s(8), (client.right - width - s(8)).max(s(8)));
        let top = button.bottom + s(2);
        let rows: Vec<(MoreAction, RECT)> = actions
            .into_iter()
            .enumerate()
            .map(|(index, action)| {
                let row_top = top + s(INSET) + s(ROW) * index as i32;
                (
                    action,
                    RECT {
                        left: left + s(INSET),
                        top: row_top,
                        right: left + width - s(INSET),
                        bottom: row_top + s(ROW),
                    },
                )
            })
            .collect();
        let bottom = rows.last().map_or(top, |(_, row)| row.bottom) + s(INSET);
        Some(MoreLayout {
            menu: RECT {
                left,
                top,
                right: left + width,
                bottom,
            },
            rows,
        })
    }

    /// A click while the menu is open: runs the item under it, or closes the
    /// menu when the click is elsewhere. True when the menu took the click.
    pub(super) fn more_menu_click(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some(layout) = self.more_menu_layout(hwnd) else {
            return false;
        };
        let pane = self.more_menu.as_ref().map_or(0, |menu| menu.pane);
        self.dismiss_more_menu(hwnd);
        if !contains(layout.menu, x, y) {
            // A click on the same button only closes the menu.
            let (_, button) = self.pane_actions(self.pane_right(hwnd, pane));
            return contains(button, x, y);
        }
        if let Some(&(action, _)) = layout.rows.iter().find(|(_, row)| contains(*row, x, y)) {
            self.run_more_action(hwnd, pane, action);
        }
        true
    }

    pub(super) fn more_menu_hover(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some(layout) = self.more_menu_layout(hwnd) else {
            return false;
        };
        let next = layout.rows.iter().position(|(_, row)| contains(*row, x, y));
        if let Some(menu) = &mut self.more_menu
            && menu.highlighted != next
        {
            menu.highlighted = next;
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
        true
    }

    /// Up/Down/Enter/Esc while the menu is open. Any other key closes it and
    /// goes on to do what it normally does.
    pub(super) fn more_menu_key(&mut self, hwnd: HWND, key: u32) -> bool {
        let Some(menu) = &self.more_menu else {
            return false;
        };
        let pane = menu.pane;
        let highlighted = menu.highlighted;
        let actions = self.more_actions();
        let count = actions.len();
        let next = match key {
            x if x == VK_DOWN as u32 => highlighted.map_or(0, |index| (index + 1) % count),
            x if x == VK_UP as u32 => {
                highlighted.map_or(count - 1, |index| (index + count - 1) % count)
            }
            x if x == VK_RETURN as u32 => {
                self.dismiss_more_menu(hwnd);
                if let Some(action) = highlighted.and_then(|index| actions.get(index)) {
                    self.run_more_action(hwnd, pane, *action);
                }
                return true;
            }
            x if x == VK_ESCAPE as u32 => {
                self.dismiss_more_menu(hwnd);
                return true;
            }
            _ => {
                self.dismiss_more_menu(hwnd);
                return false;
            }
        };
        if let Some(menu) = &mut self.more_menu {
            menu.highlighted = Some(next);
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
        true
    }

    fn run_more_action(&mut self, hwnd: HWND, pane: usize, action: MoreAction) {
        self.focus_pane(hwnd, pane);
        match action {
            MoreAction::Split => self.toggle_split(hwnd),
            MoreAction::Find => self.open_find(hwnd, false),
            MoreAction::Replace => self.open_find(hwnd, true),
            MoreAction::WordWrap => self.toggle_word_wrap(hwnd),
            MoreAction::Format => self.format_document(hwnd),
            MoreAction::Run => self.run_active_file(hwnd),
            MoreAction::RunConfigurations => self.show_run_configurations(hwnd),
            MoreAction::Close => self.close_tab(hwnd, self.active),
            MoreAction::Palette => {
                self.show_quick_open(hwnd);
                self.quick_query = ">".into();
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }
    }

    pub(super) fn paint_more_menu(&self, hdc: HDC, hwnd: HWND) {
        let Some(layout) = self.more_menu_layout(hwnd) else {
            return;
        };
        let highlighted = self.more_menu.as_ref().and_then(|menu| menu.highlighted);
        let s = |value: i32| self.scale(value);
        let menu = layout.menu;
        let shadow = RECT {
            left: menu.left + s(4),
            top: menu.top + s(5),
            right: menu.right + s(4),
            bottom: menu.bottom + s(5),
        };
        Self::rounded_fill(hdc, shadow, s(9), ui(3, 8, 18));
        self.panel_card(hdc, menu, s(8), self.theme.edge, self.theme.sidebar_bg);
        unsafe { SelectObject(hdc, self.ui_font) };
        for (index, (action, row)) in layout.rows.iter().enumerate() {
            if highlighted == Some(index) {
                Self::rounded_fill(hdc, *row, s(6), self.theme.select_bg);
            }
            let (label, shortcut) = self.more_label(*action);
            let middle = (row.top + row.bottom) / 2;
            let shortcut_left = row.right - s(10) - self.text_width(hdc, shortcut);
            self.label_mid(
                hdc,
                label,
                row.left + s(10),
                middle,
                self.theme.text,
                RECT {
                    right: shortcut_left - s(8),
                    ..*row
                },
            );
            self.label_mid(hdc, shortcut, shortcut_left, middle, self.theme.muted, *row);
        }
    }
}

fn contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}
