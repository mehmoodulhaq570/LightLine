//! The menu bar in the title bar, as in VS Code: File, Edit, Selection,
//! View, Go, Run, Terminal and Help (design/menu-bar-design.html).
//!
//! Every item runs something LightLine already has: what its shortcut does
//! (replayed through `key`, so a menu item and its shortcut can't drift
//! apart) or a Command Palette command (`run_command`). The titles sit
//! between the brand and the search box; when the window is too narrow for
//! them and a usable search box, they fold into one button whose list opens
//! each menu beside it.
//!
//! Painted in the backbuffer like the "..." and right-click menus, so it
//! follows the color theme. Nothing runs until a menu opens: the titles are
//! part of the title bar, drawn only on full-frame paints, and their widths
//! are measured once per font.
//!
//! Keyboard: Alt+F, Alt+E... open a menu; holding Alt underlines the access
//! letters; tapping Alt alone puts the keyboard on the titles (Left/Right,
//! Down or Enter opens one). In a menu: Up/Down, Left/Right (submenus and
//! the next menu), Home/End, Enter, Esc, and the underlined letters.

use super::*;

// Sizes, unscaled.
const BAR_LEFT: i32 = 160;
const TITLE_HEIGHT: i32 = 26;
const TITLE_PAD: i32 = 10;
const TITLE_GAP: i32 = 2;
const COMPACT_WIDTH: i32 = 36;
/// The narrowest the search box gets before the titles fold into one button.
const MIN_SEARCH: i32 = 240;
/// Between the menu bar and the search box.
pub(super) const SEARCH_GAP: i32 = 16;
const MENU_MIN_WIDTH: i32 = 300;
const LIST_WIDTH: i32 = 200;
const ROW: i32 = 30;
const MIN_ROW: i32 = 22;
const INSET: i32 = 6;
const SEPARATOR: i32 = 11;
const SHORTCUT_GAP: i32 = 32;
const CHEVRON_ROOM: i32 = 18;
/// Kept between a menu and the window's bottom edge: rows shrink first,
/// then the menu moves up.
const WINDOW_MARGIN: i32 = 8;

const VERSION: &str = match option_env!("LIGHTLINE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
const PROJECT: &str = "https://github.com/mehmoodulhaq570/LightLine";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    /// What this shortcut does, replayed through `key` (`replay_keys`).
    Keys {
        key: u32,
        ctrl: bool,
        shift: bool,
        alt: bool,
    },
    /// Command Palette command `id` (`run_command`).
    Command(u8),
    /// Shows the Explorer side panel.
    Explorer,
    /// Opens recent workspace folder `n` (`App::recent`).
    Recent(usize),
    Exit,
    /// Opens an address in the browser, and says what was opened.
    Link(&'static str, &'static str),
    About,
    /// Opens a submenu beside the menu.
    Open(Submenu),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Submenu {
    Recent,
    Preferences,
    TerminalProfile,
    DefaultProfile,
}

/// When an item can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Needs {
    Nothing,
    /// A file open for editing (not the Welcome screen or a preview).
    Editor,
    Workspace,
    /// At least one recent folder.
    Recent,
}

#[derive(Clone, Copy, Debug)]
enum Entry {
    /// Label, access letter, shortcut shown, what it does, when it can.
    Item(&'static str, char, &'static str, Action, Needs),
    Separator,
}
use Entry::{Item, Separator};

const fn keys(key: u32, ctrl: bool, shift: bool, alt: bool) -> Action {
    Action::Keys {
        key,
        ctrl,
        shift,
        alt,
    }
}
const fn ctrl(key: u32) -> Action {
    keys(key, true, false, false)
}
const fn ctrl_shift(key: u32) -> Action {
    keys(key, true, true, false)
}
const fn plain(key: u32) -> Action {
    keys(key, false, false, false)
}
const fn shifted(key: u32) -> Action {
    keys(key, false, true, false)
}

const N: Needs = Needs::Nothing;
const ED: Needs = Needs::Editor;

const FILE: &[Entry] = &[
    Item("New Tab", 'N', "Ctrl+N", ctrl(0x4E), N),
    Item(
        "New File in Workspace...",
        'W',
        "",
        Action::Command(22),
        Needs::Workspace,
    ),
    Separator,
    Item("Open File...", 'O', "Ctrl+O", ctrl(0x4F), N),
    Item("Open Folder...", 'F', "Ctrl+Shift+O", ctrl_shift(0x4F), N),
    Item(
        "Open Recent",
        'R',
        "",
        Action::Open(Submenu::Recent),
        Needs::Recent,
    ),
    Separator,
    Item("Save", 'S', "Ctrl+S", ctrl(0x53), ED),
    Item("Local History...", 'H', "", Action::Command(65), ED),
    Separator,
    Item(
        "Preferences",
        'P',
        "",
        Action::Open(Submenu::Preferences),
        N,
    ),
    Separator,
    Item("Close Editor", 'C', "Ctrl+W", ctrl(0x57), ED),
    Item(
        "Close Workspace",
        'k',
        "Ctrl+Shift+W",
        ctrl_shift(0x57),
        Needs::Workspace,
    ),
    Separator,
    Item("Exit", 'x', "Alt+F4", Action::Exit, N),
];

const EDIT: &[Entry] = &[
    Item("Undo", 'U', "Ctrl+Z", ctrl(0x5A), ED),
    Item("Redo", 'R', "Ctrl+Y", ctrl(0x59), ED),
    Separator,
    Item("Cut", 't', "Ctrl+X", ctrl(0x58), ED),
    Item("Copy", 'C', "Ctrl+C", ctrl(0x43), ED),
    Item("Paste", 'P', "Ctrl+V", ctrl(0x56), ED),
    Separator,
    Item("Find", 'F', "Ctrl+F", ctrl(0x46), ED),
    Item("Replace", 'e', "Ctrl+H", ctrl(0x48), ED),
    Separator,
    Item("Find in Files", 'i', "Ctrl+Shift+F", ctrl_shift(0x46), N),
    Item("Replace in Files", 'a', "", Action::Command(63), N),
    Separator,
    Item(
        "Toggle Line Comment",
        'L',
        "Ctrl+/",
        ctrl(VK_OEM_2 as u32),
        ED,
    ),
    Item(
        "Format Document",
        'D',
        "Shift+Alt+F",
        keys(0x46, false, true, true),
        ED,
    ),
];

const SELECTION: &[Entry] = &[
    Item("Select All", 'A', "Ctrl+A", ctrl(0x41), ED),
    Separator,
    Item(
        "Add Cursor Above",
        'b',
        "Ctrl+Alt+Up",
        keys(VK_UP as u32, true, false, true),
        ED,
    ),
    Item(
        "Add Cursor Below",
        'l',
        "Ctrl+Alt+Down",
        keys(VK_DOWN as u32, true, false, true),
        ED,
    ),
    Item("Add Next Occurrence", 'N', "Ctrl+D", ctrl(0x44), ED),
];

const VIEW: &[Entry] = &[
    Item(
        "Command Palette...",
        'C',
        "Ctrl+Shift+P",
        ctrl_shift(0x50),
        N,
    ),
    Item("Quick Open...", 'Q', "Ctrl+P", ctrl(0x50), N),
    Separator,
    Item("Explorer", 'E', "", Action::Explorer, N),
    Item("Search", 'S', "Ctrl+Shift+F", ctrl_shift(0x46), N),
    Item("Source Control", 'u', "Ctrl+Shift+G", ctrl_shift(0x47), N),
    Item("Run & Debug", 'D', "Ctrl+Shift+D", ctrl_shift(0x44), N),
    Item("Extensions", 'x', "Ctrl+Shift+X", ctrl_shift(0x58), N),
    Separator,
    Item("Problems", 'P', "Ctrl+Shift+M", ctrl_shift(0x4D), N),
    Item("Debug Console", 'b', "Ctrl+Shift+Y", ctrl_shift(0x59), N),
    Item("Terminal", 'T', "Ctrl+`", ctrl(VK_OEM_3 as u32), N),
    Separator,
    Item("Side Panel", 'i', "Ctrl+B", ctrl(0x42), N),
    Item("Split Editor", 'l', "Ctrl+\\", ctrl(VK_OEM_5 as u32), ED),
    Item(
        "Word Wrap",
        'W',
        "Alt+Z",
        keys(0x5A, false, false, true),
        ED,
    ),
    Separator,
    Item("Zoom In", 'o', "Ctrl++", ctrl(VK_OEM_PLUS as u32), N),
    Item("Zoom Out", 'm', "Ctrl+-", ctrl(VK_OEM_MINUS as u32), N),
    Item("Reset Zoom", 'R', "Ctrl+0", ctrl(0x30), N),
    Separator,
    Item(
        "Markdown Preview",
        'k',
        "Ctrl+Shift+V",
        ctrl_shift(0x56),
        ED,
    ),
];

const GO: &[Entry] = &[
    Item("Go to File...", 'F', "Ctrl+P", ctrl(0x50), N),
    Item(
        "Go to Symbol in File...",
        'S',
        "Ctrl+P, @",
        Action::Command(53),
        ED,
    ),
    Item("Go to Symbol in Workspace...", 'W', "Ctrl+T", ctrl(0x54), N),
    Item("Go to Line...", 'L', "Ctrl+G", ctrl(0x47), ED),
    Separator,
    Item("Go to Definition", 'D', "F12", plain(VK_F12 as u32), ED),
    Item(
        "Find All References",
        'R',
        "Shift+F12",
        shifted(VK_F12 as u32),
        ED,
    ),
    Separator,
    Item("Next Merge Conflict", 'N', "", Action::Command(66), ED),
];

const RUN: &[Entry] = &[
    Item("Run File", 'R', "Ctrl+Shift+R", ctrl_shift(0x52), N),
    Item("Stop Running Program", 'S', "", Action::Command(49), N),
    Item("Run Rust Tests", 'T', "Ctrl+Shift+B", ctrl_shift(0x42), N),
    Separator,
    Item("Start Debugging", 'D', "F5", plain(VK_F5 as u32), N),
    Item("Stop Debugging", 'g', "Shift+F5", shifted(VK_F5 as u32), N),
    Item("Step Over", 'O', "F10", plain(VK_F10 as u32), N),
    Item("Step Into", 'I', "F11", plain(VK_F11 as u32), N),
    Item("Step Out", 'u', "Shift+F11", shifted(VK_F11 as u32), N),
    Item("Toggle Breakpoint", 'B', "F9", plain(VK_F9 as u32), ED),
    Separator,
    Item("Run Configurations...", 'C', "", Action::Command(50), N),
    Item(
        "Choose the File's Language...",
        'L',
        "",
        Action::Command(59),
        ED,
    ),
    Item("Python: Install debugpy", 'P', "", Action::Command(35), N),
];

const TERMINAL: &[Entry] = &[
    Item(
        "New Terminal",
        'N',
        "Ctrl+Shift+`",
        ctrl_shift(VK_OEM_3 as u32),
        N,
    ),
    Item(
        "New Terminal With Profile",
        'W',
        "",
        Action::Open(Submenu::TerminalProfile),
        N,
    ),
    Separator,
    Item(
        "Show or Hide Terminal",
        'S',
        "Ctrl+`",
        ctrl(VK_OEM_3 as u32),
        N,
    ),
    Item("Kill Terminal", 'K', "", Action::Command(15), N),
    Item("Restart Terminal", 'R', "", Action::Command(10), N),
    Separator,
    Item(
        "Default Profile",
        'D',
        "",
        Action::Open(Submenu::DefaultProfile),
        N,
    ),
];

const HELP: &[Entry] = &[
    Item(
        "Keyboard Shortcuts",
        'K',
        "",
        Action::Link(
            "https://github.com/mehmoodulhaq570/LightLine/blob/main/docs/USER_GUIDE.md#keyboard-shortcuts",
            "the keyboard shortcuts",
        ),
        N,
    ),
    Item(
        "User Guide",
        'U',
        "",
        Action::Link(
            "https://github.com/mehmoodulhaq570/LightLine/blob/main/docs/USER_GUIDE.md",
            "the User Guide",
        ),
        N,
    ),
    Separator,
    Item(
        "Release Notes",
        'N',
        "",
        Action::Link(
            "https://github.com/mehmoodulhaq570/LightLine/releases",
            "the release notes",
        ),
        N,
    ),
    Item(
        "Report an Issue",
        'I',
        "",
        Action::Link(
            "https://github.com/mehmoodulhaq570/LightLine/issues/new",
            "a new issue",
        ),
        N,
    ),
    Separator,
    Item("About LightLine", 'A', "", Action::About, N),
];

const PREFERENCES: &[Entry] = &[
    Item("Settings", 'S', "Ctrl+,", ctrl(VK_OEM_COMMA as u32), N),
    Item("Settings (JSON)", 'J', "", Action::Command(16), N),
    Item("Color Theme...", 'T', "", Action::Command(39), N),
];

const TERMINAL_PROFILE: &[Entry] = &[
    Item("PowerShell", 'P', "", Action::Command(25), N),
    Item("Command Prompt", 'C', "", Action::Command(26), N),
    Item("Git Bash", 'G', "", Action::Command(27), N),
    Item("WSL", 'W', "", Action::Command(28), N),
];

const DEFAULT_PROFILE: &[Entry] = &[
    Item("PowerShell", 'P', "", Action::Command(29), N),
    Item("Command Prompt", 'C', "", Action::Command(30), N),
    Item("Git Bash", 'G', "", Action::Command(31), N),
    Item("WSL", 'W', "", Action::Command(32), N),
];

/// The menus: title, access letter, items.
const MENUS: [(&str, char, &[Entry]); 8] = [
    ("File", 'F', FILE),
    ("Edit", 'E', EDIT),
    ("Selection", 'S', SELECTION),
    ("View", 'V', VIEW),
    ("Go", 'G', GO),
    ("Run", 'R', RUN),
    ("Terminal", 'T', TERMINAL),
    ("Help", 'H', HELP),
];

/// The titles' text widths, and the font and scale they were measured at.
pub(super) type TitleWidths = Option<((usize, i32), Vec<i32>)>;

/// How many recent folders Open Recent lists.
const RECENT_SHOWN: usize = 10;

/// An open menu.
pub(super) struct MenuBar {
    open: usize,
    // The highlighted row of the menu, by its index in `lines`.
    highlighted: Option<usize>,
    sub: Option<OpenSub>,
    // Opened or moved with the keyboard: access letters are underlined.
    keyboard: bool,
}

struct OpenSub {
    // The menu row it opened from.
    from: usize,
    which: Submenu,
    highlighted: Option<usize>,
    // Up/Down move in the submenu rather than the menu.
    has_keys: bool,
}

/// One row of a menu as it shows now; None is a separator.
struct Line {
    label: String,
    access: Option<char>,
    shortcut: &'static str,
    action: Action,
    enabled: bool,
}

/// Where the titles are: one each, or folded into one button.
pub(super) enum Titles {
    Full(Vec<RECT>),
    Compact(RECT),
}

struct Panel {
    rect: RECT,
    // Each row: its index in the lines, and where it is.
    rows: Vec<(usize, RECT)>,
    separators: Vec<RECT>,
}

struct Drop {
    // Narrow window: the list of the eight menus.
    list: Option<Panel>,
    menu: Panel,
    sub: Option<Panel>,
}

fn contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

// `base` with `amount` of `over` mixed in (COLORREF values).
fn mix(base: u32, over: u32, amount: f32) -> u32 {
    let channel = |shift: u32| {
        let a = ((base >> shift) & 0xff) as f32;
        let b = ((over >> shift) & 0xff) as f32;
        ((a + (b - a) * amount).round() as u32) << shift
    };
    channel(0) | channel(8) | channel(16)
}

fn is_typing_key(key: u32) -> bool {
    (0x30..=0x39).contains(&key) || (0x41..=0x5A).contains(&key)
}

impl App {
    /// Whether Ctrl, Shift or Alt (`vk`) is held for the key `key` is
    /// handling: what a menu item replaying its shortcut says, else the
    /// keyboard. A replayed shortcut never touches the thread's key state,
    /// which a dialog it opens would otherwise inherit.
    pub(super) fn modifier_down(&self, vk: u16) -> bool {
        match self.replayed_modifiers {
            Some((ctrl, shift, alt)) => {
                (vk == VK_CONTROL && ctrl) || (vk == VK_SHIFT && shift) || (vk == VK_MENU && alt)
            }
            None => (unsafe { GetKeyState(vk as i32) }) < 0,
        }
    }

    // A card that takes the whole window's input: the menu bar waits.
    fn menu_blocked(&self) -> bool {
        self.run_choice.is_some() || self.run_config_panel.is_some() || self.ai_provider.is_some()
    }

    fn needs_met(&self, needs: Needs) -> bool {
        match needs {
            Needs::Nothing => true,
            Needs::Editor => {
                !self.welcome
                    && !self.tabs.is_empty()
                    && !self.tab().is_placeholder()
                    && !self.tab().read_only()
            }
            Needs::Workspace => self.workspace_root.is_some(),
            Needs::Recent => !self.recent.is_empty(),
        }
    }

    fn lines_of(&self, entries: &[Entry]) -> Vec<Option<Line>> {
        entries
            .iter()
            .map(|entry| match *entry {
                Item(label, access, shortcut, action, needs) => Some(Line {
                    label: label.to_owned(),
                    access: Some(access.to_ascii_lowercase()),
                    shortcut,
                    action,
                    enabled: self.needs_met(needs),
                }),
                Separator => None,
            })
            .collect()
    }

    fn menu_lines(&self, menu: usize) -> Vec<Option<Line>> {
        self.lines_of(MENUS[menu.min(MENUS.len() - 1)].2)
    }

    fn submenu_lines(&self, which: Submenu) -> Vec<Option<Line>> {
        match which {
            Submenu::Preferences => self.lines_of(PREFERENCES),
            Submenu::TerminalProfile => self.lines_of(TERMINAL_PROFILE),
            Submenu::DefaultProfile => self.lines_of(DEFAULT_PROFILE),
            Submenu::Recent => {
                // A folder listed twice (once as \\?\D:\..., once as
                // D:\...) shows once.
                let mut shown: Vec<String> = Vec::new();
                let mut lines = Vec::new();
                for (index, path) in self.recent.iter().enumerate() {
                    let name = path.display().to_string();
                    let name = name.trim_start_matches(r"\\?\").to_owned();
                    if shown.iter().any(|seen| seen.eq_ignore_ascii_case(&name)) {
                        continue;
                    }
                    // 1 to 9, then 0, as access keys.
                    let access = char::from_digit(((shown.len() + 1) % 10) as u32, 10);
                    lines.push(Some(Line {
                        label: format!("{}  {name}", access.unwrap_or(' ')),
                        access,
                        shortcut: "",
                        action: Action::Recent(index),
                        enabled: true,
                    }));
                    shown.push(name);
                    if shown.len() == RECENT_SHOWN {
                        break;
                    }
                }
                lines
            }
        }
    }

    // The list of the eight menus, in a narrow window.
    fn title_lines() -> Vec<Option<Line>> {
        MENUS
            .iter()
            .map(|(title, access, _)| {
                Some(Line {
                    label: (*title).to_owned(),
                    access: Some(access.to_ascii_lowercase()),
                    shortcut: "",
                    action: Action::Open(Submenu::Preferences),
                    enabled: true,
                })
            })
            .collect()
    }

    // The width of each title's text, measured once per font and DPI.
    fn title_widths(&self, hwnd: HWND) -> Vec<i32> {
        let key = (self.ui_font as usize, self.scale(1000));
        if let Some((known, widths)) = &*self.menu_title_widths.borrow()
            && *known == key
        {
            return widths.clone();
        }
        let widths: Vec<i32> = unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.ui_font);
            let widths = MENUS
                .iter()
                .map(|(title, _, _)| self.text_width(hdc, title))
                .collect();
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            widths
        };
        *self.menu_title_widths.borrow_mut() = Some((key, widths.clone()));
        widths
    }

    /// Where the menu titles are in the title bar.
    pub(super) fn menu_titles(&self, hwnd: HWND) -> Titles {
        let s = |value: i32| self.scale(value);
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let top = (self.chrome_top() - s(TITLE_HEIGHT)) / 2;
        let mut left = s(BAR_LEFT);
        let rects: Vec<RECT> = self
            .title_widths(hwnd)
            .into_iter()
            .map(|width| {
                let rect = RECT {
                    left,
                    top,
                    right: left + width + s(TITLE_PAD) * 2,
                    bottom: top + s(TITLE_HEIGHT),
                };
                left = rect.right + s(TITLE_GAP);
                rect
            })
            .collect();
        let titles_right = rects.last().map_or(s(BAR_LEFT), |rect| rect.right);
        // The same right edge the search box is centred against.
        let usable_right = client.right - s(46 * 3 + 16);
        if usable_right - titles_right - s(SEARCH_GAP) >= s(MIN_SEARCH) {
            Titles::Full(rects)
        } else {
            Titles::Compact(RECT {
                left: s(BAR_LEFT),
                top,
                right: s(BAR_LEFT + COMPACT_WIDTH),
                bottom: top + s(TITLE_HEIGHT),
            })
        }
    }

    /// The right edge of the menu bar: the search box starts past it, and
    /// the title bar drags the window only right of it.
    pub(super) fn menu_bar_right(&self, hwnd: HWND) -> i32 {
        match self.menu_titles(hwnd) {
            Titles::Full(rects) => rects.last().map_or(self.scale(BAR_LEFT), |rect| rect.right),
            Titles::Compact(button) => button.right,
        }
    }

    /// The menu title (or, folded, the menu button) at (x, y), for hover.
    pub(super) fn menu_title_at(&self, hwnd: HWND, x: i32, y: i32) -> Option<Hot> {
        match self.menu_titles(hwnd) {
            Titles::Full(rects) => rects
                .iter()
                .position(|rect| contains(*rect, x, y))
                .map(Hot::MenuTitle),
            Titles::Compact(button) => contains(button, x, y).then_some(Hot::MenuButton),
        }
    }

    /// Where hover target `hot` (a menu title or the menu button) is.
    pub(super) fn menu_title_rect(&self, hwnd: HWND, hot: Hot) -> Option<RECT> {
        match (self.menu_titles(hwnd), hot) {
            (Titles::Full(rects), Hot::MenuTitle(index)) => rects.get(index).copied(),
            (Titles::Compact(button), Hot::MenuButton) => Some(button),
            _ => None,
        }
    }

    // A menu panel for `lines`, its top left at (left, top), kept inside the
    // window: shifted left at the right edge, rows made shorter at the
    // bottom.
    fn menu_panel(
        &self,
        hdc: HDC,
        lines: &[Option<Line>],
        left: i32,
        top: i32,
        min_width: i32,
        client: RECT,
    ) -> Panel {
        let s = |value: i32| self.scale(value);
        let widest = lines
            .iter()
            .flatten()
            .map(|line| {
                let shortcut = if line.shortcut.is_empty() {
                    0
                } else {
                    s(SHORTCUT_GAP) + self.text_width(hdc, line.shortcut)
                };
                let chevron = if matches!(line.action, Action::Open(_)) {
                    s(CHEVRON_ROOM)
                } else {
                    0
                };
                self.text_width(hdc, &line.label) + shortcut + chevron + s(20)
            })
            .max()
            .unwrap_or(0);
        let width = (widest + s(INSET) * 2).max(min_width);
        let left = left
            .min(client.right - s(4) - width)
            .max(s(4).min(client.right - width));
        let rows = lines.iter().filter(|line| line.is_some()).count().max(1) as i32;
        let separators = lines.iter().filter(|line| line.is_none()).count() as i32;
        let room =
            client.bottom - s(WINDOW_MARGIN) - top - s(INSET) * 2 - s(SEPARATOR) * separators;
        let row = s(ROW).min(room / rows).max(s(MIN_ROW));
        // Still too tall with the shortest rows: up, as far as it must.
        let height = s(INSET) * 2 + row * rows + s(SEPARATOR) * separators;
        let limit = client.bottom - s(WINDOW_MARGIN);
        let top = if top + height > limit {
            (limit - height).max(s(4))
        } else {
            top
        };
        let mut y = top + s(INSET);
        let mut panel = Panel {
            rect: RECT::default(),
            rows: Vec::new(),
            separators: Vec::new(),
        };
        for (index, line) in lines.iter().enumerate() {
            if line.is_some() {
                panel.rows.push((
                    index,
                    RECT {
                        left: left + s(INSET),
                        top: y,
                        right: left + width - s(INSET),
                        bottom: y + row,
                    },
                ));
                y += row;
            } else {
                let middle = y + s(SEPARATOR) / 2;
                panel.separators.push(RECT {
                    left: left + s(INSET + 8),
                    top: middle,
                    right: left + width - s(INSET + 8),
                    bottom: middle + s(1).max(1),
                });
                y += s(SEPARATOR);
            }
        }
        panel.rect = RECT {
            left,
            top,
            right: left + width,
            bottom: y + s(INSET),
        };
        panel
    }

    // Where the open menu, its submenu and (narrow window) the list of
    // menus are.
    fn menu_drop(&self, hwnd: HWND) -> Option<Drop> {
        let state = self.menu_bar.as_ref()?;
        let s = |value: i32| self.scale(value);
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let top = self.chrome_top() - s(2);
        unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.ui_font);
            let (list, menu_left, menu_top) = match self.menu_titles(hwnd) {
                Titles::Full(rects) => {
                    let title = rects.get(state.open).copied().unwrap_or_default();
                    (None, title.left, top)
                }
                Titles::Compact(button) => {
                    let list = self.menu_panel(
                        hdc,
                        &Self::title_lines(),
                        button.left - s(INSET),
                        top,
                        s(LIST_WIDTH),
                        client,
                    );
                    let row_top = list
                        .rows
                        .get(state.open)
                        .map_or(top, |(_, row)| row.top - s(INSET));
                    let left = list.rect.right + s(2);
                    (Some(list), left, row_top)
                }
            };
            let lines = self.menu_lines(state.open);
            let menu = self.menu_panel(hdc, &lines, menu_left, menu_top, s(MENU_MIN_WIDTH), client);
            let sub = state.sub.as_ref().and_then(|open| {
                let row = menu
                    .rows
                    .iter()
                    .find(|(index, _)| *index == open.from)
                    .map(|(_, row)| *row)?;
                let lines = self.submenu_lines(open.which);
                let mut panel = self.menu_panel(
                    hdc,
                    &lines,
                    menu.rect.right + s(2),
                    row.top - s(INSET),
                    s(220),
                    client,
                );
                // No room to the right: open to the left of the menu.
                if panel.rect.left < menu.rect.right {
                    let width = panel.rect.right - panel.rect.left;
                    panel = self.menu_panel(
                        hdc,
                        &lines,
                        (menu.rect.left - s(2) - width).max(0),
                        row.top - s(INSET),
                        s(220),
                        client,
                    );
                }
                Some(panel)
            });
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            Some(Drop { list, menu, sub })
        }
    }

    // What changes on screen when the menu bar does: the title bar and the
    // open panels with their shadows.
    fn menu_damage(&self, hwnd: HWND) -> Vec<RECT> {
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let mut areas = vec![RECT {
            left: 0,
            top: 0,
            right: client.right,
            bottom: self.chrome_top(),
        }];
        if let Some(drop) = self.menu_drop(hwnd) {
            let margin = self.scale(8);
            for panel in drop.list.iter().chain([&drop.menu]).chain(drop.sub.iter()) {
                areas.push(RECT {
                    left: panel.rect.left - margin,
                    top: panel.rect.top - margin,
                    right: panel.rect.right + margin,
                    bottom: panel.rect.bottom + margin,
                });
            }
        }
        areas
    }

    // Changes the menu bar's state with `change`, and redraws what that
    // moved: before and after.
    fn change_menu_bar(&mut self, hwnd: HWND, change: impl FnOnce(&mut Self)) {
        let before = self.menu_damage(hwnd);
        change(self);
        for area in before.into_iter().chain(self.menu_damage(hwnd)) {
            unsafe { InvalidateRect(hwnd, &area, 0) };
        }
    }

    /// Opens menu `menu`; from the keyboard, with its first item
    /// highlighted.
    fn open_menu(&mut self, hwnd: HWND, menu: usize, keyboard: bool) {
        self.dismiss_more_menu(hwnd);
        self.dismiss_editor_context(hwnd);
        self.change_menu_bar(hwnd, |app| {
            app.menu_bar_focus = None;
            app.menu_bar = Some(MenuBar {
                open: menu,
                highlighted: None,
                sub: None,
                keyboard,
            });
            if keyboard {
                app.move_menu_highlight(1);
            }
        });
    }

    /// Closes any open menu and takes the keyboard off the titles.
    pub(super) fn close_menu_bar(&mut self, hwnd: HWND) {
        if self.menu_bar.is_some() || self.menu_bar_focus.is_some() {
            self.change_menu_bar(hwnd, |app| {
                app.menu_bar = None;
                app.menu_bar_focus = None;
            });
        }
    }

    // The rows Up/Down move through: the submenu's when it has the keys.
    fn active_lines(&self) -> Vec<Option<Line>> {
        match &self.menu_bar {
            Some(MenuBar { sub: Some(sub), .. }) if sub.has_keys => self.submenu_lines(sub.which),
            Some(state) => self.menu_lines(state.open),
            None => Vec::new(),
        }
    }

    // Moves the highlight `step` enabled rows (+1 down, -1 up), wrapping;
    // `step` 0 with nothing highlighted highlights nothing.
    fn move_menu_highlight(&mut self, step: isize) {
        let lines = self.active_lines();
        let enabled: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.as_ref().is_some_and(|line| line.enabled))
            .map(|(index, _)| index)
            .collect();
        let Some(state) = self.menu_bar.as_mut() else {
            return;
        };
        let current = match &state.sub {
            Some(sub) if sub.has_keys => sub.highlighted,
            _ => state.highlighted,
        };
        let next = if enabled.is_empty() {
            None
        } else {
            let at = current.and_then(|current| enabled.iter().position(|&index| index == current));
            let count = enabled.len() as isize;
            let position = match (at, step) {
                (Some(at), step) => (at as isize + step).rem_euclid(count),
                (None, step) if step < 0 => count - 1,
                (None, _) => 0,
            };
            Some(enabled[position as usize])
        };
        match &mut state.sub {
            Some(sub) if sub.has_keys => sub.highlighted = next,
            _ => state.highlighted = next,
        }
    }

    // Highlights the first (`last` false) or last enabled row.
    fn menu_highlight_end(&mut self, last: bool) {
        if let Some(state) = self.menu_bar.as_mut() {
            match &mut state.sub {
                Some(sub) if sub.has_keys => sub.highlighted = None,
                _ => state.highlighted = None,
            }
        }
        self.move_menu_highlight(if last { -1 } else { 1 });
    }

    // Opens the submenu row `from` of the menu leads to.
    fn open_submenu(&mut self, from: usize, which: Submenu, keyboard: bool) {
        let Some(state) = self.menu_bar.as_mut() else {
            return;
        };
        state.highlighted = Some(from);
        state.sub = Some(OpenSub {
            from,
            which,
            highlighted: None,
            has_keys: keyboard,
        });
        if keyboard {
            self.move_menu_highlight(1);
        }
    }

    // Runs row `index` of the active list (or opens its submenu).
    fn activate_menu_line(&mut self, hwnd: HWND, index: usize, in_sub: bool) {
        let lines = if in_sub {
            match self.menu_bar.as_ref().and_then(|state| state.sub.as_ref()) {
                Some(sub) => self.submenu_lines(sub.which),
                None => return,
            }
        } else {
            match &self.menu_bar {
                Some(state) => self.menu_lines(state.open),
                None => return,
            }
        };
        let Some(Some(line)) = lines.get(index) else {
            return;
        };
        if !line.enabled {
            return;
        }
        let keyboard = self.menu_bar.as_ref().is_some_and(|state| state.keyboard);
        // An Edit or Selection item (one that needs an editor) acts on it.
        let for_editor = !in_sub
            && self.menu_bar.as_ref().is_some_and(|state| {
                matches!(
                    MENUS[state.open].2.get(index),
                    Some(Item(_, _, _, _, Needs::Editor))
                )
            });
        match line.action {
            Action::Open(which) if !in_sub => {
                self.change_menu_bar(hwnd, |app| app.open_submenu(index, which, keyboard));
            }
            Action::Open(_) => {}
            action => {
                self.close_menu_bar(hwnd);
                self.run_menu_action(hwnd, action, for_editor);
            }
        }
    }

    fn run_menu_action(&mut self, hwnd: HWND, action: Action, for_editor: bool) {
        // Edit and Selection act on the file being edited, as their
        // shortcuts do there, even when the terminal or a side panel has the
        // keyboard: Copy must not send Ctrl+C to a shell.
        if for_editor {
            self.terminal_focus = false;
            self.panel_focus = false;
            self.problem_focus = false;
            self.debug_console.focus = false;
        }
        match action {
            Action::Keys {
                key,
                ctrl,
                shift,
                alt,
            } => {
                self.replayed_modifiers = Some((ctrl, shift, alt));
                self.key(hwnd, key);
                self.replayed_modifiers = None;
            }
            Action::Command(id) => self.run_command(hwnd, id),
            Action::Explorer => {
                if !(self.side_view == SideView::Files && self.explorer_visible) {
                    self.toggle_side_view(hwnd, SideView::Files);
                }
            }
            Action::Recent(index) => {
                if let Some(path) = self.recent.get(index).cloned() {
                    self.set_workspace(hwnd, path);
                }
            }
            Action::Exit => unsafe {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            },
            Action::Link(address, what) => {
                super::ai_provider::open_in_browser(hwnd, address);
                self.status = format!("Opened {what} in your browser");
            }
            Action::About => {
                dialog::show_dialog(
                    hwnd,
                    "About LightLine",
                    &format!(
                        "LightLine {VERSION}\n\nA fast, light code editor for Windows, \
                         written in Rust.\n\n{PROJECT}"
                    ),
                    dialog::DialogIcon::Info,
                    &[dialog::BTN_OK],
                );
            }
            Action::Open(_) => {}
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// A click: on a title it opens (or closes) that menu; in an open menu
    /// it runs the row under it. True when the menu bar took the click; a
    /// click elsewhere closes the menu and goes on to what was clicked.
    pub(super) fn menu_bar_click(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        self.menu_alt_tap = false;
        if let Some(drop) = self.menu_drop(hwnd) {
            if let Some(sub) = &drop.sub
                && contains(sub.rect, x, y)
            {
                if let Some(&(index, _)) = sub.rows.iter().find(|(_, row)| contains(*row, x, y)) {
                    self.activate_menu_line(hwnd, index, true);
                }
                return true;
            }
            if contains(drop.menu.rect, x, y) {
                if let Some(&(index, _)) =
                    drop.menu.rows.iter().find(|(_, row)| contains(*row, x, y))
                {
                    if let Some(state) = self.menu_bar.as_mut() {
                        state.keyboard = false;
                    }
                    self.activate_menu_line(hwnd, index, false);
                }
                return true;
            }
            if let Some(list) = &drop.list
                && contains(list.rect, x, y)
            {
                if let Some(&(index, _)) = list.rows.iter().find(|(_, row)| contains(*row, x, y)) {
                    self.open_menu(hwnd, index, false);
                }
                return true;
            }
        }
        let open = self.menu_bar.as_ref().map(|state| state.open);
        match self.menu_title_at(hwnd, x, y) {
            Some(Hot::MenuTitle(index)) => {
                if open == Some(index) {
                    self.close_menu_bar(hwnd);
                } else {
                    self.open_menu(hwnd, index, false);
                }
                true
            }
            Some(Hot::MenuButton) => {
                if open.is_some() {
                    self.close_menu_bar(hwnd);
                } else {
                    self.open_menu(hwnd, 0, false);
                }
                true
            }
            _ => {
                self.close_menu_bar(hwnd);
                false
            }
        }
    }

    /// The mouse moving while a menu is open: onto another title (or, in a
    /// narrow window, another menu in the list) switches menus; over a row
    /// highlights it, and one with a submenu opens it. True while a menu is
    /// open: nothing under the menus lights up.
    pub(super) fn menu_bar_hover(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some(drop) = self.menu_drop(hwnd) else {
            return false;
        };
        let Some(state) = self.menu_bar.as_ref() else {
            return false;
        };
        let open = state.open;
        if let Some(Hot::MenuTitle(index)) = self.menu_title_at(hwnd, x, y) {
            if index != open {
                self.open_menu(hwnd, index, false);
            }
            return true;
        }
        if let Some(list) = &drop.list
            && let Some(&(index, _)) = list.rows.iter().find(|(_, row)| contains(*row, x, y))
        {
            if index != open {
                self.open_menu(hwnd, index, false);
            }
            return true;
        }
        if let Some(sub) = &drop.sub
            && contains(sub.rect, x, y)
        {
            let row = sub
                .rows
                .iter()
                .find(|(_, row)| contains(*row, x, y))
                .map(|(index, _)| *index);
            let lines = self.active_sub_lines();
            let row = row.filter(|&index| {
                lines
                    .get(index)
                    .is_some_and(|line| line.as_ref().is_some_and(|line| line.enabled))
            });
            let unchanged = state
                .sub
                .as_ref()
                .is_some_and(|sub| sub.highlighted == row && sub.has_keys);
            if !unchanged {
                self.change_menu_bar(hwnd, |app| {
                    if let Some(sub) = app.menu_bar.as_mut().and_then(|state| state.sub.as_mut()) {
                        sub.highlighted = row;
                        sub.has_keys = true;
                    }
                });
            }
            return true;
        }
        if let Some(&(index, _)) = drop.menu.rows.iter().find(|(_, row)| contains(*row, x, y)) {
            let lines = self.menu_lines(open);
            let line = lines.get(index).and_then(|line| line.as_ref());
            let enabled = line.is_some_and(|line| line.enabled);
            let opens = line.and_then(|line| match line.action {
                Action::Open(which) if enabled => Some(which),
                _ => None,
            });
            let same = state.highlighted == Some(index)
                && state.sub.as_ref().map(|sub| sub.from) == opens.map(|_| index);
            if !same {
                self.change_menu_bar(hwnd, |app| {
                    if let Some(state) = app.menu_bar.as_mut() {
                        state.highlighted = enabled.then_some(index);
                        state.sub = None;
                    }
                    if let Some(which) = opens {
                        app.open_submenu(index, which, false);
                    }
                });
            }
            return true;
        }
        true
    }

    // The open submenu's rows.
    fn active_sub_lines(&self) -> Vec<Option<Line>> {
        self.menu_bar
            .as_ref()
            .and_then(|state| state.sub.as_ref())
            .map(|sub| self.submenu_lines(sub.which))
            .unwrap_or_default()
    }

    /// Keys while a menu is open or the titles have the keyboard. True when
    /// the menu bar took the key. Letters and digits are taken here and
    /// acted on as characters (`menu_bar_char`); Ctrl chords and F-keys
    /// close the menu and do what they always do.
    pub(super) fn menu_bar_key(&mut self, hwnd: HWND, key: u32, ctrl: bool) -> bool {
        let is = |vk: u16| key == vk as u32;
        if self.menu_bar.is_none() {
            let Some(focus) = self.menu_bar_focus else {
                return false;
            };
            let count = MENUS.len();
            if is(VK_LEFT) || is(VK_RIGHT) {
                let next = if is(VK_LEFT) {
                    (focus + count - 1) % count
                } else {
                    (focus + 1) % count
                };
                self.change_menu_bar(hwnd, |app| app.menu_bar_focus = Some(next));
            } else if is(VK_DOWN) || is(VK_RETURN) || is(VK_SPACE) {
                self.open_menu(hwnd, focus, true);
            } else if is(VK_UP) {
                self.open_menu(hwnd, focus, true);
                self.change_menu_bar(hwnd, |app| app.menu_highlight_end(true));
            } else if is(VK_ESCAPE) {
                self.close_menu_bar(hwnd);
            } else if !(is_typing_key(key) && !ctrl) {
                self.close_menu_bar(hwnd);
                return false;
            }
            return true;
        }
        if ctrl || (VK_F1 as u32..=VK_F24 as u32).contains(&key) {
            self.close_menu_bar(hwnd);
            return false;
        }
        let Some(state) = self.menu_bar.as_ref() else {
            return false;
        };
        let open = state.open;
        let in_sub = state.sub.as_ref().is_some_and(|sub| sub.has_keys);
        let highlighted = if in_sub {
            state.sub.as_ref().and_then(|sub| sub.highlighted)
        } else {
            state.highlighted
        };
        let count = MENUS.len();
        if is(VK_ESCAPE) {
            if state.sub.is_some() {
                self.change_menu_bar(hwnd, |app| {
                    if let Some(state) = app.menu_bar.as_mut() {
                        state.sub = None;
                    }
                });
            } else {
                self.close_menu_bar(hwnd);
            }
        } else if is(VK_DOWN) || is(VK_UP) {
            self.change_menu_bar(hwnd, |app| {
                if let Some(state) = app.menu_bar.as_mut() {
                    state.keyboard = true;
                }
                app.move_menu_highlight(if is(VK_DOWN) { 1 } else { -1 });
            });
        } else if is(VK_HOME) || is(VK_END) {
            self.change_menu_bar(hwnd, |app| app.menu_highlight_end(is(VK_END)));
        } else if is(VK_RIGHT) {
            let opens = (!in_sub)
                .then(|| {
                    let lines = self.menu_lines(open);
                    highlighted.and_then(|index| match lines.get(index) {
                        Some(Some(Line {
                            action: Action::Open(which),
                            enabled: true,
                            ..
                        })) => Some((index, *which)),
                        _ => None,
                    })
                })
                .flatten();
            match opens {
                Some((index, which)) => {
                    self.change_menu_bar(hwnd, |app| app.open_submenu(index, which, true));
                }
                None => self.open_menu(hwnd, (open + 1) % count, true),
            }
        } else if is(VK_LEFT) {
            if in_sub {
                self.change_menu_bar(hwnd, |app| {
                    if let Some(state) = app.menu_bar.as_mut() {
                        state.sub = None;
                    }
                });
            } else {
                self.open_menu(hwnd, (open + count - 1) % count, true);
            }
        } else if is(VK_RETURN) || is(VK_SPACE) {
            if let Some(index) = highlighted {
                self.activate_menu_line(hwnd, index, in_sub);
            }
        } else if is(VK_TAB) || is_typing_key(key) || is(VK_SHIFT) || is(VK_CONTROL) {
            // Taken; letters act as characters.
        } else {
            self.close_menu_bar(hwnd);
            return false;
        }
        true
    }

    /// Characters while a menu is open or the titles have the keyboard: an
    /// access letter runs its item or opens its menu. True when taken.
    pub(super) fn menu_bar_char(&mut self, hwnd: HWND, unit: u16) -> bool {
        if self.menu_bar.is_none() && self.menu_bar_focus.is_none() {
            return false;
        }
        let Some(typed) = char::from_u32(unit as u32)
            .filter(|ch| ch.is_ascii_alphanumeric())
            .map(|ch| ch.to_ascii_lowercase())
        else {
            return true;
        };
        if self.menu_bar.is_none() {
            if let Some(index) = MENUS
                .iter()
                .position(|(_, access, _)| access.to_ascii_lowercase() == typed)
            {
                self.open_menu(hwnd, index, true);
            }
            return true;
        }
        let in_sub = self
            .menu_bar
            .as_ref()
            .and_then(|state| state.sub.as_ref())
            .is_some_and(|sub| sub.has_keys);
        let lines = self.active_lines();
        if let Some(index) = lines.iter().position(|line| {
            line.as_ref()
                .is_some_and(|line| line.enabled && line.access == Some(typed))
        }) {
            if let Some(state) = self.menu_bar.as_mut() {
                state.keyboard = true;
            }
            self.activate_menu_line(hwnd, index, in_sub);
        }
        true
    }

    /// Alt with a letter: opens that menu (Alt+F, Alt+E...). Not from the
    /// terminal, which gets its Alt keys, nor Alt+R while the find box or
    /// the Search panel has the keyboard (Regular Expression there).
    pub(super) fn menu_bar_alt_key(&mut self, hwnd: HWND, key: u32, shift: bool) -> bool {
        self.menu_alt_tap = false;
        if shift || self.terminal_focus || self.menu_blocked() {
            return false;
        }
        if (self.find_mode || self.search_input) && matches!(key, 0x43 | 0x57 | 0x52) {
            return false;
        }
        let Some(index) = MENUS
            .iter()
            .position(|(_, access, _)| access.to_ascii_uppercase() as u32 == key)
        else {
            return false;
        };
        self.open_menu(hwnd, index, true);
        true
    }

    /// Alt went down: the access letters are underlined while it's held.
    pub(super) fn menu_alt_down(&mut self, hwnd: HWND) {
        if !self.menu_alt_held {
            self.menu_alt_tap = true;
            self.change_menu_bar(hwnd, |app| app.menu_alt_held = true);
        }
    }

    /// Something other than Alt alone happened while Alt was down.
    pub(super) fn menu_alt_interrupted(&mut self) {
        self.menu_alt_tap = false;
    }

    /// Alt came up. Pressed and released alone, it puts the keyboard on the
    /// titles (or takes it off them, or closes an open menu); true then, and
    /// Windows' own handling (its invisible system-menu mode) is skipped.
    pub(super) fn menu_alt_up(&mut self, hwnd: HWND) -> bool {
        let tap = std::mem::take(&mut self.menu_alt_tap);
        if self.menu_alt_held {
            self.change_menu_bar(hwnd, |app| app.menu_alt_held = false);
        }
        if !tap || self.terminal_focus || self.menu_blocked() {
            return false;
        }
        if self.menu_bar.is_some() || self.menu_bar_focus.is_some() {
            self.close_menu_bar(hwnd);
        } else {
            self.change_menu_bar(hwnd, |app| app.menu_bar_focus = Some(0));
        }
        true
    }

    /// The window lost the keyboard: menus close, Alt is up.
    pub(super) fn menu_bar_focus_lost(&mut self, hwnd: HWND) {
        self.menu_alt_tap = false;
        if self.menu_alt_held {
            self.change_menu_bar(hwnd, |app| app.menu_alt_held = false);
        }
        self.close_menu_bar(hwnd);
    }

    fn menu_underlines(&self) -> bool {
        self.menu_alt_held
            || self.menu_bar_focus.is_some()
            || self.menu_bar.as_ref().is_some_and(|state| state.keyboard)
    }

    // Draws `text` at x, centred on `middle`, underlining `access` (its
    // first match, case-insensitive) while access letters show.
    fn menu_text(
        &self,
        hdc: HDC,
        text: &str,
        access: Option<char>,
        (x, middle): (i32, i32),
        color: u32,
        clip: RECT,
    ) {
        self.label_mid(hdc, text, x, middle, color, clip);
        let Some(access) = access.filter(|_| self.menu_underlines()) else {
            return;
        };
        let Some((at, ch)) = text
            .char_indices()
            .find(|(_, ch)| ch.to_ascii_lowercase() == access)
        else {
            return;
        };
        let left = x + self.text_width(hdc, &text[..at]);
        let width = self.text_width(hdc, &text[at..at + ch.len_utf8()]);
        let y = middle + self.text_height(hdc) / 2 - self.scale(2).max(1);
        Self::fill(
            hdc,
            RECT {
                left,
                top: y,
                right: left + width,
                bottom: y + self.scale(1).max(1),
            },
            color,
        );
    }

    /// The titles (or the folded menu button), in the title bar.
    pub(super) fn paint_menu_titles(&self, hdc: HDC, hwnd: HWND) {
        let s = |value: i32| self.scale(value);
        let open = self.menu_bar.as_ref().map(|state| state.open);
        let label = mix(self.theme.muted, self.theme.text, 0.35);
        unsafe { SelectObject(hdc, self.ui_font) };
        match self.menu_titles(hwnd) {
            Titles::Full(rects) => {
                for (index, rect) in rects.iter().enumerate() {
                    let (title, access, _) = MENUS[index];
                    let focused = self.menu_bar_focus == Some(index);
                    let color = if open == Some(index) {
                        Self::rounded_fill(hdc, *rect, s(6), self.theme.select_bg);
                        self.theme.text
                    } else if focused {
                        self.panel_card(hdc, *rect, s(6), self.theme.blue, self.theme.select_bg);
                        self.theme.text
                    } else if self.hot == Some(Hot::MenuTitle(index)) {
                        Self::rounded_fill(
                            hdc,
                            *rect,
                            s(6),
                            self.hot_color(self.theme.tab_bg, hover_fill()),
                        );
                        self.theme.text
                    } else {
                        label
                    };
                    self.menu_text(
                        hdc,
                        title,
                        Some(access.to_ascii_lowercase()),
                        (rect.left + s(TITLE_PAD), (rect.top + rect.bottom) / 2),
                        color,
                        *rect,
                    );
                }
            }
            Titles::Compact(button) => {
                let lit = open.is_some() || self.menu_bar_focus.is_some();
                if self.menu_bar_focus.is_some() {
                    self.panel_card(hdc, button, s(6), self.theme.blue, self.theme.select_bg);
                } else if lit {
                    Self::rounded_fill(hdc, button, s(6), self.theme.select_bg);
                } else if self.hot == Some(Hot::MenuButton) {
                    Self::rounded_fill(
                        hdc,
                        button,
                        s(6),
                        self.hot_color(self.theme.tab_bg, hover_fill()),
                    );
                }
                let color = if lit || self.hot == Some(Hot::MenuButton) {
                    self.theme.text
                } else {
                    label
                };
                // Three lines, as the rail's ☰.
                let middle = (button.top + button.bottom) / 2;
                let half = s(7);
                let thick = s(2).max(1);
                let center = (button.left + button.right) / 2;
                for offset in [-s(5), 0, s(5)] {
                    let top = middle + offset - thick / 2;
                    Self::fill(
                        hdc,
                        RECT {
                            left: center - half,
                            top,
                            right: center + half,
                            bottom: top + thick,
                        },
                        color,
                    );
                }
            }
        }
    }

    // One panel: card, rows, separators, the highlight.
    fn paint_menu_panel(
        &self,
        hdc: HDC,
        panel: &Panel,
        lines: &[Option<Line>],
        highlighted: Option<usize>,
    ) {
        let s = |value: i32| self.scale(value);
        let rect = panel.rect;
        let shadow = RECT {
            left: rect.left + s(4),
            top: rect.top + s(5),
            right: rect.right + s(4),
            bottom: rect.bottom + s(5),
        };
        Self::rounded_fill(hdc, shadow, s(9), ui(3, 8, 18));
        self.panel_card(hdc, rect, s(8), self.theme.edge, self.theme.sidebar_bg);
        for separator in &panel.separators {
            Self::fill(hdc, *separator, self.theme.edge);
        }
        let disabled = mix(self.theme.muted, self.theme.sidebar_bg, 0.4);
        for &(index, row) in &panel.rows {
            let Some(Some(line)) = lines.get(index) else {
                continue;
            };
            if highlighted == Some(index) && line.enabled {
                Self::rounded_fill(hdc, row, s(6), self.theme.select_bg);
            }
            let middle = (row.top + row.bottom) / 2;
            let (text, muted) = if line.enabled {
                (self.theme.text, self.theme.muted)
            } else {
                (disabled, disabled)
            };
            let opens = matches!(line.action, Action::Open(_));
            let mut right = row.right - s(10);
            if opens {
                // A right-pointing chevron, drawn like the Explorer's.
                self.chevron(hdc, row.right - s(12), middle, false);
                right -= s(CHEVRON_ROOM - 6);
            }
            if !line.shortcut.is_empty() {
                let shortcut_left = right - self.text_width(hdc, line.shortcut);
                self.label_mid(hdc, line.shortcut, shortcut_left, middle, muted, row);
                right = shortcut_left - s(16);
            }
            self.menu_text(
                hdc,
                &line.label,
                line.access,
                (row.left + s(10), middle),
                text,
                RECT { right, ..row },
            );
        }
    }

    /// The open menu, its submenu and (narrow window) the list of menus,
    /// over everything else.
    pub(super) fn paint_menu_bar(&self, hdc: HDC, hwnd: HWND) {
        let Some(state) = self.menu_bar.as_ref() else {
            return;
        };
        let Some(drop) = self.menu_drop(hwnd) else {
            return;
        };
        unsafe { SelectObject(hdc, self.ui_font) };
        if let Some(list) = &drop.list {
            self.paint_menu_panel(hdc, list, &Self::title_lines(), Some(state.open));
        }
        self.paint_menu_panel(
            hdc,
            &drop.menu,
            &self.menu_lines(state.open),
            state.highlighted,
        );
        if let (Some(panel), Some(sub)) = (&drop.sub, &state.sub) {
            self.paint_menu_panel(hdc, panel, &self.submenu_lines(sub.which), sub.highlighted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_lists() -> Vec<(&'static str, &'static [Entry])> {
        let mut lists: Vec<(&str, &[Entry])> = MENUS
            .iter()
            .map(|(title, _, entries)| (*title, *entries))
            .collect();
        lists.extend([
            ("Preferences", PREFERENCES),
            ("Terminal profile", TERMINAL_PROFILE),
            ("Default profile", DEFAULT_PROFILE),
        ]);
        lists
    }

    #[test]
    fn every_access_letter_is_in_its_label_and_used_once_per_menu() {
        for (menu, entries) in all_lists() {
            let mut seen = Vec::new();
            for entry in entries {
                if let Item(label, access, ..) = entry {
                    let access = access.to_ascii_lowercase();
                    assert!(
                        label.to_ascii_lowercase().contains(access),
                        "{menu}: {label} has no {access}"
                    );
                    assert!(!seen.contains(&access), "{menu}: {access} twice");
                    seen.push(access);
                }
            }
        }
        let mut titles: Vec<char> = MENUS
            .iter()
            .map(|(title, access, _)| {
                assert!(title.contains(*access), "{title}");
                access.to_ascii_lowercase()
            })
            .collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(
            titles.len(),
            MENUS.len(),
            "menu titles share an access letter"
        );
    }

    #[test]
    fn every_command_is_one_the_palette_runs() {
        for (menu, entries) in all_lists() {
            for entry in entries {
                if let Item(label, _, _, Action::Command(id), _) = entry {
                    assert!(
                        super::super::panels::QUICK_COMMANDS
                            .iter()
                            .any(|(_, known)| known == id),
                        "{menu}: {label} runs unknown command {id}"
                    );
                }
            }
        }
    }

    #[test]
    fn menus_have_no_leading_trailing_or_double_separators() {
        for (menu, entries) in all_lists() {
            assert!(
                matches!(entries.first(), Some(Item(..))),
                "{menu} starts with a separator"
            );
            assert!(
                matches!(entries.last(), Some(Item(..))),
                "{menu} ends with a separator"
            );
            for pair in entries.windows(2) {
                assert!(
                    !matches!(pair, [Separator, Separator]),
                    "{menu} has two separators in a row"
                );
            }
        }
    }

    #[test]
    fn shown_shortcuts_match_the_keys_replayed() {
        // The label next to an item must be the chord it replays.
        let name = |key: u32| -> String {
            match key {
                0x30..=0x39 | 0x41..=0x5A => char::from_u32(key).unwrap().to_string(),
                k if k == VK_OEM_2 as u32 => "/".into(),
                k if k == VK_OEM_3 as u32 => "`".into(),
                k if k == VK_OEM_5 as u32 => "\\".into(),
                k if k == VK_OEM_COMMA as u32 => ",".into(),
                k if k == VK_OEM_PLUS as u32 => "+".into(),
                k if k == VK_OEM_MINUS as u32 => "-".into(),
                k if k == VK_UP as u32 => "Up".into(),
                k if k == VK_DOWN as u32 => "Down".into(),
                k if k == VK_F5 as u32 => "F5".into(),
                k if k == VK_F9 as u32 => "F9".into(),
                k if k == VK_F10 as u32 => "F10".into(),
                k if k == VK_F11 as u32 => "F11".into(),
                k if k == VK_F12 as u32 => "F12".into(),
                other => panic!("no name for key {other:#x}"),
            }
        };
        for (menu, entries) in all_lists() {
            for entry in entries {
                if let Item(
                    label,
                    _,
                    shown,
                    Action::Keys {
                        key,
                        ctrl,
                        shift,
                        alt,
                    },
                    _,
                ) = entry
                {
                    // Format Document is written Shift+Alt+F, as in the
                    // User Guide; the others Ctrl, Shift, Alt.
                    let mut parts = Vec::new();
                    if *ctrl {
                        parts.push("Ctrl".to_string());
                    }
                    if *shift && *alt && !*ctrl {
                        parts.push("Shift".into());
                        parts.push("Alt".into());
                    } else {
                        if *alt {
                            parts.push("Alt".into());
                        }
                        if *shift {
                            parts.push("Shift".into());
                        }
                    }
                    parts.push(name(*key));
                    let mut expected = parts.join("+");
                    // Ctrl+Alt+Up is written in that order.
                    if *ctrl && *alt && !*shift {
                        expected = format!("Ctrl+Alt+{}", name(*key));
                    }
                    assert_eq!(*shown, expected, "{menu}: {label}");
                }
            }
        }
    }

    #[test]
    fn colors_mix_per_channel() {
        assert_eq!(mix(0x000000, 0xffffff, 0.5), 0x808080);
        assert_eq!(mix(0x0000ff, 0x00ff00, 0.0), 0x0000ff);
        assert_eq!(mix(0x0000ff, 0x00ff00, 1.0), 0x00ff00);
    }
}
