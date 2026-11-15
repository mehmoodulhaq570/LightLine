//! The Welcome page, flat like the rest of the workbench (approved design:
//! design/welcome-design.html). One centred column in two halves: Start and
//! Recent on the left, Get started and Learn on the right; in a narrow
//! window they stack. Every row runs something LightLine already has, and
//! each action appears once.

use super::super::*;
use super::blend;
use crate::windows_app::menu_bar::{
    GUIDE_URL, NEW_ISSUE_URL, RELEASES_URL, SHORTCUTS_URL, VERSION,
};

// Geometry, in logical pixels (everything goes through scale()).
// The two halves need this much width right of the rail; below it they stack.
const WIDE_MIN: i32 = 1120;
const INNER_WIDTH: i32 = 1040;
const LEFT_WIDTH: i32 = 500;
const HALF_GAP: i32 = 64;
const NARROW_PAD: i32 = 40;
const TOP_PAD: i32 = 64;
const HEAD_H: i32 = 56;
const HEAD_GAP: i32 = 36;
const SECTION_H: i32 = 26;
const ROW_H: i32 = 28;
const STEP_H: i32 = 50;
const BLOCK_GAP: i32 = 30;
const LINK_H: i32 = 24;
const LINK_CELL: i32 = 158;
const FOOT_H: i32 = 22;
const PILL_W: i32 = 150;
const PILL_H: i32 = 24;
const RECENT_WIDE: usize = 8;
const RECENT_NARROW: usize = 5;

// Everything on this page maps to a real command; nothing is decorative.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::windows_app) enum WelcomeAction {
    Explorer,
    Search,
    SourceControl,
    RunDebug,
    Extensions,
    AiAssistant,
    Settings,
    NewFile,
    OpenFile,
    OpenFolder,
    Recent(usize),
    // The × on a Recent row: take the folder off the list.
    ForgetRecent(usize),
    CommandPalette,
    GoToFile,
    Terminal,
    ColorTheme,
    // A Learn link, by its index in LINKS.
    Link(usize),
    // The "Started in" pill: a tip, no command.
    StartTime,
}

/// The tip for Welcome target `index` (in `WelcomeLayout::targets`), if it
/// has one: the rail's buttons, the speed pill and a Recent row's ×.
pub(in crate::windows_app) fn welcome_tip(
    targets: &[(RECT, WelcomeAction)],
    index: usize,
) -> Option<&'static str> {
    match targets.get(index)?.1 {
        WelcomeAction::Settings if index == NAV_ITEMS.len() => Some("Settings (Ctrl+,)"),
        WelcomeAction::ForgetRecent(_) => Some("Remove from Recent"),
        WelcomeAction::StartTime => Some("From launching LightLine to its first frame"),
        _ => NAV_ITEMS.get(index).map(|(label, _, _)| *label),
    }
}

// The rail, as the editor's: the same glyphs in the same rows, so going from
// Welcome to a file doesn't move them. Home sits where the editor has ☰.
const NAV_ITEMS: [(&str, DebugGlyph, WelcomeAction); 6] = [
    ("Explorer", DebugGlyph::Files, WelcomeAction::Explorer),
    ("Search", DebugGlyph::Search, WelcomeAction::Search),
    (
        "Source Control",
        DebugGlyph::SourceControl,
        WelcomeAction::SourceControl,
    ),
    ("Run & Debug", DebugGlyph::RunDebug, WelcomeAction::RunDebug),
    (
        "Extensions",
        DebugGlyph::Extensions,
        WelcomeAction::Extensions,
    ),
    (
        "AI Assistant",
        DebugGlyph::Sparkle,
        WelcomeAction::AiAssistant,
    ),
];

const START: [(&str, &str, DebugGlyph, WelcomeAction); 3] = [
    (
        "New File",
        "Ctrl+N",
        DebugGlyph::NewFile,
        WelcomeAction::NewFile,
    ),
    (
        "Open File...",
        "Ctrl+O",
        DebugGlyph::OpenFile,
        WelcomeAction::OpenFile,
    ),
    (
        "Open Folder...",
        "Ctrl+Shift+O",
        DebugGlyph::Folder,
        WelcomeAction::OpenFolder,
    ),
];

#[derive(Clone, Copy)]
enum Tint {
    Violet,
    Blue,
    Green,
    Orange,
    Muted,
}

// Title, what it's for, key, icon and its colour, command.
const STEPS: [(&str, &str, &str, DebugGlyph, Tint, WelcomeAction); 6] = [
    (
        "Command Palette",
        "Every command, by name",
        "Ctrl+Shift+P",
        DebugGlyph::Palette,
        Tint::Violet,
        WelcomeAction::CommandPalette,
    ),
    (
        "Go to File",
        "Open any file in the folder by typing part of its name",
        "Ctrl+P",
        DebugGlyph::GoToFile,
        Tint::Blue,
        WelcomeAction::GoToFile,
    ),
    (
        "Search in Files",
        "Plain text or regex, across the whole folder",
        "Ctrl+Shift+F",
        DebugGlyph::Search,
        Tint::Blue,
        WelcomeAction::Search,
    ),
    (
        "Terminal",
        "PowerShell, Command Prompt, Git Bash or WSL",
        "Ctrl+`",
        DebugGlyph::Terminal,
        Tint::Green,
        WelcomeAction::Terminal,
    ),
    (
        "Color Theme",
        "Pick a theme, or install Dracula or Catppuccin from Extensions",
        "",
        DebugGlyph::ColorTheme,
        Tint::Orange,
        WelcomeAction::ColorTheme,
    ),
    (
        "Settings",
        "Font, tab size, word wrap, format on save...",
        "Ctrl+,",
        DebugGlyph::Gear,
        Tint::Muted,
        WelcomeAction::Settings,
    ),
];

// The Help menu's links: label, icon, address, and what the status bar
// calls it once it's open.
pub(in crate::windows_app) const LINKS: [(&str, DebugGlyph, &str, &str); 4] = [
    (
        "Keyboard Shortcuts",
        DebugGlyph::Keyboard,
        SHORTCUTS_URL,
        "the keyboard shortcuts",
    ),
    ("User Guide", DebugGlyph::Book, GUIDE_URL, "the User Guide"),
    (
        "Release Notes",
        DebugGlyph::Notes,
        RELEASES_URL,
        "the release notes",
    ),
    (
        "Report an Issue",
        DebugGlyph::Info,
        NEW_ISSUE_URL,
        "a new issue",
    ),
];

#[derive(Clone, Copy)]
enum RowKind {
    Start(usize),
    Recent(usize),
    // "No recent folders yet": opens a folder.
    NoRecent,
    Step(usize),
    Link(usize),
}

#[derive(Clone, Copy)]
struct Row {
    rect: RECT,
    kind: RowKind,
    // Its index in `targets`, and its ×'s for a Recent row.
    target: usize,
    forget: Option<usize>,
}

// Computed once per paint and again per mouse move or click, so painted
// rectangles and click targets always come from the same numbers. Needs no
// DC: nothing here depends on a text's width.
pub(in crate::windows_app) struct WelcomeLayout {
    pub(in crate::windows_app) targets: Vec<(RECT, WelcomeAction)>,
    rail_right: i32,
    content_top: i32,
    status_top: i32,
    head: RECT,
    pill: Option<(RECT, usize)>,
    // Section labels: text, left, top.
    sections: Vec<(&'static str, i32, i32)>,
    rows: Vec<Row>,
    compact_steps: bool,
    foot: Option<(i32, i32)>,
}

impl App {
    pub(in crate::windows_app) fn welcome_layout(&self, rect: RECT) -> WelcomeLayout {
        let s = |value: i32| self.scale(value);
        let mut targets = Vec::new();
        let status_top = rect.bottom - s(STATUS);
        let rail_right = s(RAIL);
        let content_top = s(WORKBENCH_HEADER);

        for (index, (_, _, action)) in NAV_ITEMS.iter().enumerate() {
            let top = content_top + s(RAIL_FIRST_ROW + index as i32 * RAIL_ROW);
            targets.push((
                RECT {
                    left: 0,
                    top,
                    right: rail_right,
                    bottom: top + s(RAIL_ROW),
                },
                *action,
            ));
        }
        targets.push((
            RECT {
                left: 0,
                top: status_top - s(44),
                right: rail_right,
                bottom: status_top,
            },
            WelcomeAction::Settings,
        ));

        let page_width = rect.right - rail_right;
        let page_height = status_top - content_top;
        let wide = page_width >= s(WIDE_MIN);
        let (inner_left, inner_right) = if wide {
            let left = rail_right + (page_width - s(INNER_WIDTH)) / 2;
            (left, left + s(INNER_WIDTH))
        } else {
            (rail_right + s(NARROW_PAD), rect.right - s(NARROW_PAD))
        };
        let bottom = status_top - s(12);
        let recent_cap = if wide { RECENT_WIDE } else { RECENT_NARROW };
        let recent_count = self.recent.len().min(recent_cap);
        let link_columns = |width: i32| (width / s(LINK_CELL)).clamp(1, LINKS.len() as i32);
        let links_height = |width: i32| {
            let columns = link_columns(width);
            (LINKS.len() as i32 + columns - 1) / columns * s(LINK_H)
        };

        // How tall each half is with every row, to pick the top padding and,
        // if the window is short, the compact Get started rows.
        let start_height = s(SECTION_H) + START.len() as i32 * s(ROW_H);
        let recent_height = s(SECTION_H) + recent_count.max(1) as i32 * s(ROW_H);
        let right_width = if wide {
            inner_right - inner_left - s(LEFT_WIDTH + HALF_GAP)
        } else {
            inner_right - inner_left
        };
        let learn_height = s(SECTION_H) + links_height(right_width) + s(FOOT_H);
        let steps_height = |compact: bool| {
            s(SECTION_H) + STEPS.len() as i32 * s(if compact { ROW_H } else { STEP_H })
        };
        let body_height = |compact: bool| {
            let right = steps_height(compact) + s(BLOCK_GAP) + learn_height;
            let left = start_height + s(BLOCK_GAP) + recent_height;
            if wide {
                left.max(right)
            } else {
                left + s(BLOCK_GAP) + right
            }
        };
        let head_height = s(HEAD_H + HEAD_GAP);
        let room = page_height - s(12);
        let compact_steps = !wide || head_height + body_height(false) + s(24) > room;
        let wanted_pad = s(if wide { TOP_PAD } else { NARROW_PAD });
        let top_pad = (room - head_height - body_height(compact_steps))
            .min(wanted_pad)
            .max(s(16));

        let head = RECT {
            left: inner_left,
            top: content_top + top_pad,
            right: inner_right,
            bottom: content_top + top_pad + s(HEAD_H),
        };
        let pill = (wide && self.startup_ms.is_some_and(|ms| ms > 0)).then(|| {
            let middle = (head.top + head.bottom) / 2;
            let rect = RECT {
                left: inner_right - s(PILL_W),
                top: middle - s(PILL_H) / 2,
                right: inner_right,
                bottom: middle + s(PILL_H) / 2,
            };
            targets.push((rect, WelcomeAction::StartTime));
            (rect, targets.len() - 1)
        });

        let mut sections = Vec::new();
        let mut rows: Vec<Row> = Vec::new();
        // Adds a row (and its target) if it fits above the page's bottom.
        let mut push_row =
            |targets: &mut Vec<(RECT, WelcomeAction)>, rect: RECT, kind: RowKind, action| {
                if rect.bottom > bottom {
                    return false;
                }
                let forget = if let RowKind::Recent(index) = kind {
                    // The × comes first, so a click on it isn't the row's.
                    let size = s(20);
                    let middle = (rect.top + rect.bottom) / 2;
                    targets.push((
                        RECT {
                            left: rect.right - s(6) - size,
                            top: middle - size / 2,
                            right: rect.right - s(6),
                            bottom: middle + size / 2,
                        },
                        WelcomeAction::ForgetRecent(index),
                    ));
                    Some(targets.len() - 1)
                } else {
                    None
                };
                targets.push((rect, action));
                rows.push(Row {
                    rect,
                    kind,
                    target: targets.len() - 1,
                    forget,
                });
                true
            };
        // A row spans its column plus 8 px each side for the hover fill.
        let row_rect = |left: i32, right: i32, top: i32, height: i32| RECT {
            left: left - s(8),
            top,
            right: right + s(8),
            bottom: top + height,
        };

        let body_top = head.bottom + s(HEAD_GAP);
        let (left_right, right_left) = if wide {
            (
                inner_left + s(LEFT_WIDTH),
                inner_left + s(LEFT_WIDTH + HALF_GAP),
            )
        } else {
            (inner_right, inner_left)
        };

        // Start and Recent.
        let mut y = body_top;
        sections.push(("START", inner_left, y));
        y += s(SECTION_H);
        for (index, (_, _, _, action)) in START.iter().enumerate() {
            push_row(
                &mut targets,
                row_rect(inner_left, left_right, y, s(ROW_H)),
                RowKind::Start(index),
                *action,
            );
            y += s(ROW_H);
        }
        y += s(BLOCK_GAP);
        if y + s(SECTION_H + ROW_H) <= bottom {
            sections.push(("RECENT", inner_left, y));
        }
        y += s(SECTION_H);
        // Stacked, Recent leaves room for the right half below it.
        let recent_bottom = if wide {
            bottom
        } else {
            bottom - s(BLOCK_GAP) - steps_height(true) - s(BLOCK_GAP) - learn_height
        };
        if self.recent.is_empty() {
            push_row(
                &mut targets,
                row_rect(inner_left, left_right, y, s(ROW_H)),
                RowKind::NoRecent,
                WelcomeAction::OpenFolder,
            );
            y += s(ROW_H);
        } else {
            for index in 0..recent_count {
                // At least one folder, then only as many as leave room.
                if index > 0 && y + s(ROW_H) > recent_bottom {
                    break;
                }
                push_row(
                    &mut targets,
                    row_rect(inner_left, left_right, y, s(ROW_H)),
                    RowKind::Recent(index),
                    WelcomeAction::Recent(index),
                );
                y += s(ROW_H);
            }
        }

        // Get started and Learn.
        let mut y = if wide { body_top } else { y + s(BLOCK_GAP) };
        if y + s(SECTION_H + ROW_H) <= bottom {
            sections.push(("GET STARTED", right_left, y));
        }
        y += s(SECTION_H);
        let step_height = s(if compact_steps { ROW_H } else { STEP_H });
        let step_pad = if compact_steps { 8 } else { 10 };
        for (index, step) in STEPS.iter().enumerate() {
            let rect = RECT {
                left: right_left - s(step_pad),
                top: y,
                right: inner_right + s(step_pad),
                bottom: y + step_height,
            };
            push_row(&mut targets, rect, RowKind::Step(index), step.5);
            y += step_height;
        }
        y += s(BLOCK_GAP);
        if y + s(SECTION_H + LINK_H) <= bottom {
            sections.push(("LEARN", right_left, y));
        }
        y += s(SECTION_H);
        let columns = link_columns(right_width);
        let cell = right_width / columns;
        for index in 0..LINKS.len() {
            let column = index as i32 % columns;
            let line = index as i32 / columns;
            let left = right_left + column * cell;
            let top = y + line * s(LINK_H);
            push_row(
                &mut targets,
                RECT {
                    left,
                    top,
                    right: left + cell - s(12),
                    bottom: top + s(LINK_H),
                },
                RowKind::Link(index),
                WelcomeAction::Link(index),
            );
        }
        y += links_height(right_width) + s(4);
        let foot = (y + s(FOOT_H) <= bottom).then_some((right_left, y));

        WelcomeLayout {
            targets,
            rail_right,
            content_top,
            status_top,
            head,
            pill,
            sections,
            rows,
            compact_steps,
            foot,
        }
    }

    pub(in crate::windows_app) fn paint_welcome(&self, hwnd: HWND, hdc: HDC, rect: RECT) {
        let layout = self.welcome_layout(rect);
        let hot = match self.hot {
            Some(Hot::Welcome(index)) => Some(index),
            _ => None,
        };
        Self::fill(
            hdc,
            RECT {
                left: layout.rail_right,
                top: layout.content_top,
                right: rect.right,
                bottom: layout.status_top,
            },
            self.theme.editor_bg,
        );
        self.paint_welcome_header(hwnd, hdc, rect, &layout);
        self.paint_welcome_nav(hdc, &layout, hot);
        self.paint_welcome_head(hdc, &layout);
        self.paint_welcome_rows(hdc, &layout, hot);
        self.paint_welcome_status(hdc, rect, &layout);
    }

    fn paint_welcome_header(&self, hwnd: HWND, hdc: HDC, rect: RECT, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        let header = RECT {
            left: 0,
            top: 0,
            right: rect.right,
            bottom: layout.content_top,
        };
        Self::fill(hdc, header, self.theme.tab_bg);
        Self::fill(
            hdc,
            RECT {
                left: 0,
                top: layout.content_top - s(1).max(1),
                right: rect.right,
                bottom: layout.content_top,
            },
            self.theme.edge,
        );
        self.paint_brand(hdc, layout.content_top);
        self.paint_menu_titles(hdc, hwnd);

        self.paint_command_center(hdc, hwnd);
        self.paint_title_controls(hdc, hwnd, layout.content_top);
    }

    // The editor's rail (see paint_rail), with Home selected in the top row.
    fn paint_welcome_nav(&self, hdc: HDC, layout: &WelcomeLayout, hot: Option<usize>) {
        let s = |value: i32| self.scale(value);
        let rail = RECT {
            left: 0,
            top: layout.content_top,
            right: layout.rail_right,
            bottom: layout.status_top,
        };
        Self::fill(hdc, rail, self.theme.rail_bg);
        Self::fill(
            hdc,
            RECT {
                left: layout.rail_right - s(1).max(1),
                ..rail
            },
            self.theme.edge,
        );
        let size = s(22);
        let home = RECT {
            left: 0,
            top: layout.content_top + s(RAIL_MENU_ROW),
            right: layout.rail_right,
            bottom: layout.content_top + s(RAIL_MENU_ROW + RAIL_ROW),
        };
        Self::fill(
            hdc,
            RECT {
                left: 0,
                top: home.top + s(6),
                right: s(2).max(2),
                bottom: home.bottom - s(6),
            },
            self.theme.violet,
        );
        self.glyph_in(hdc, DebugGlyph::Home, self.theme.text, home, size);
        let color = |index: usize| {
            if hot == Some(index) {
                self.hot_color(self.theme.muted, self.theme.text)
            } else {
                self.theme.muted
            }
        };
        for (index, (_, glyph, _)) in NAV_ITEMS.iter().enumerate() {
            self.glyph_in(hdc, *glyph, color(index), layout.targets[index].0, size);
        }
        let gear = NAV_ITEMS.len();
        self.glyph_in(
            hdc,
            DebugGlyph::Gear,
            color(gear),
            layout.targets[gear].0,
            size,
        );
    }

    // The logo, name, version, one line about it, and the speed pill.
    fn paint_welcome_head(&self, hdc: HDC, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        let head = layout.head;
        let logo = s(48);
        unsafe {
            DrawIconEx(
                hdc,
                head.left,
                head.top + (head.bottom - head.top - logo) / 2,
                self.hero_icon,
                logo,
                logo,
                0,
                null_mut(),
                DI_NORMAL,
            );
            SelectObject(hdc, self.title_font);
        }
        let text_left = head.left + logo + s(16);
        let clip = RECT {
            left: text_left,
            right: layout
                .pill
                .map_or(head.right, |(pill, _)| pill.left - s(12)),
            ..head
        };
        let name = "LightLine";
        let name_middle = head.top + s(18);
        self.label_mid(hdc, name, text_left, name_middle, self.theme.text, clip);
        let name_width = self.text_width(hdc, name);
        unsafe { SelectObject(hdc, self.small_font) };
        let version = format!("v{VERSION}");
        let chip = RECT {
            left: text_left + name_width + s(10),
            top: name_middle - s(8),
            right: text_left + name_width + s(10) + self.text_width(hdc, &version) + s(12),
            bottom: name_middle + s(10),
        };
        if chip.right <= clip.right {
            self.card_outline(hdc, chip, s(3), self.theme.edge);
            self.label_mid(
                hdc,
                &version,
                chip.left + s(6),
                (chip.top + chip.bottom) / 2,
                self.theme.muted,
                chip,
            );
        }
        unsafe { SelectObject(hdc, self.ui_font) };
        self.label_ellipsis(
            hdc,
            "A fast, light code editor. Everything here is one click or one key away.",
            text_left,
            head.top + s(37),
            self.theme.muted,
            clip,
        );

        let (Some((pill, _)), Some(ms)) = (layout.pill, self.startup_ms) else {
            return;
        };
        self.card_outline(hdc, pill, (pill.bottom - pill.top) / 2, self.theme.edge);
        unsafe { SelectObject(hdc, self.small_font) };
        let lead = "Started in ";
        let value = format!("{ms} ms");
        let dot = s(7);
        let width = dot + s(7) + self.text_width(hdc, lead) + self.text_width(hdc, &value);
        let mut x = pill.left + (pill.right - pill.left - width) / 2;
        let middle = (pill.top + pill.bottom) / 2;
        Self::rounded_fill(
            hdc,
            RECT {
                left: x,
                top: middle - dot / 2,
                right: x + dot,
                bottom: middle - dot / 2 + dot,
            },
            dot,
            self.theme.green,
        );
        x += dot + s(7);
        self.label_mid(hdc, lead, x, middle, self.theme.muted, pill);
        x += self.text_width(hdc, lead);
        self.label_mid(hdc, &value, x, middle, self.theme.text, pill);
    }

    fn paint_welcome_rows(&self, hdc: HDC, layout: &WelcomeLayout, hot: Option<usize>) {
        let s = |value: i32| self.scale(value);
        unsafe {
            SelectObject(hdc, self.small_font);
            SetTextCharacterExtra(hdc, s(1));
        }
        for (label, left, top) in &layout.sections {
            Self::label(
                hdc,
                label,
                *left,
                top + s(4),
                self.theme.muted,
                RECT {
                    left: *left,
                    top: *top,
                    right: layout.head.right,
                    bottom: top + s(SECTION_H),
                },
            );
        }
        unsafe { SetTextCharacterExtra(hdc, 0) };

        for row in &layout.rows {
            let hovered = hot.is_some_and(|index| index == row.target || Some(index) == row.forget);
            let rect = row.rect;
            let middle = (rect.top + rect.bottom) / 2;
            let inside = RECT {
                left: rect.left + s(8),
                right: rect.right - s(8),
                ..rect
            };
            if hovered && !matches!(row.kind, RowKind::Link(_)) {
                Self::rounded_fill(hdc, rect, s(4), self.theme.active_bg);
            }
            unsafe { SelectObject(hdc, self.ui_font) };
            match row.kind {
                RowKind::Start(index) => {
                    let (label, key, glyph, _) = START[index];
                    self.icons.draw_glyph(
                        hdc,
                        glyph,
                        self.theme.blue,
                        inside.left,
                        middle - s(8),
                        s(16),
                    );
                    unsafe { SelectObject(hdc, self.small_font) };
                    let key_width = self.text_width(hdc, key);
                    self.label_mid(
                        hdc,
                        key,
                        inside.right - key_width,
                        middle,
                        self.theme.muted,
                        inside,
                    );
                    unsafe { SelectObject(hdc, self.ui_font) };
                    self.label_ellipsis(
                        hdc,
                        label,
                        inside.left + s(26),
                        middle - self.text_height(hdc) / 2,
                        self.link_color(hovered),
                        RECT {
                            right: inside.right - key_width - s(12),
                            ..inside
                        },
                    );
                }
                RowKind::Recent(index) => self.paint_recent_row(hdc, row, inside, index, hovered),
                RowKind::NoRecent => {
                    let lead = "No recent folders yet. ";
                    self.label_mid(hdc, lead, inside.left, middle, self.theme.muted, inside);
                    let mut x = inside.left + self.text_width(hdc, lead);
                    let link = "Open a folder";
                    self.label_mid(hdc, link, x, middle, self.link_color(hovered), inside);
                    x += self.text_width(hdc, link);
                    self.label_mid(
                        hdc,
                        " and it shows up here.",
                        x,
                        middle,
                        self.theme.muted,
                        inside,
                    );
                }
                RowKind::Step(index) => self.paint_step_row(hdc, layout, row, index),
                RowKind::Link(index) => {
                    let (label, glyph, _, _) = LINKS[index];
                    let color = self.link_color(hovered);
                    self.icons
                        .draw_glyph(hdc, glyph, color, rect.left, middle - s(7), s(14));
                    self.label_ellipsis(
                        hdc,
                        label,
                        rect.left + s(20),
                        middle - self.text_height(hdc) / 2,
                        color,
                        rect,
                    );
                    if hovered {
                        // Underlined under the mouse, as a link.
                        let width = self
                            .text_width(hdc, label)
                            .min(rect.right - rect.left - s(20));
                        let base = middle + self.text_height(hdc) / 2 - s(2);
                        Self::fill(
                            hdc,
                            RECT {
                                left: rect.left + s(20),
                                top: base,
                                right: rect.left + s(20) + width,
                                bottom: base + s(1).max(1),
                            },
                            color,
                        );
                    }
                }
            }
        }

        if let Some((left, top)) = layout.foot {
            unsafe { SelectObject(hdc, self.small_font) };
            Self::label(
                hdc,
                "Ctrl+Shift+H brings this page back.",
                left,
                top,
                self.theme.muted,
                RECT {
                    left,
                    top,
                    right: layout.head.right,
                    bottom: top + s(FOOT_H),
                },
            );
        }
    }

    fn link_color(&self, hovered: bool) -> u32 {
        if hovered {
            blend(self.theme.blue, self.theme.text, 0.35)
        } else {
            self.theme.blue
        }
    }

    // A folder: its icon, its name, the folder it's in (dimmed), and how long
    // ago it changed; under the mouse, × instead of the age.
    fn paint_recent_row(&self, hdc: HDC, row: &Row, inside: RECT, index: usize, hovered: bool) {
        let s = |value: i32| self.scale(value);
        let Some(path) = self.recent.get(index) else {
            return;
        };
        let middle = (inside.top + inside.bottom) / 2;
        if !self
            .icons
            .draw_generic(hdc, GenericIcon::Folder, inside.left, middle - s(8), s(16))
        {
            self.icons.draw_glyph(
                hdc,
                DebugGlyph::Folder,
                self.theme.blue,
                inside.left,
                middle - s(8),
                s(16),
            );
        }
        let right = if hovered {
            row.rect.right - s(32)
        } else {
            unsafe { SelectObject(hdc, self.small_font) };
            let age = recent_age(path);
            let width = self.text_width(hdc, &age);
            self.label_mid(
                hdc,
                &age,
                inside.right - width,
                middle,
                self.theme.muted,
                inside,
            );
            unsafe { SelectObject(hdc, self.ui_font) };
            inside.right - width - s(12)
        };
        let name = path.file_name().map_or_else(
            || display_path(path),
            |name| name.to_string_lossy().into_owned(),
        );
        let text_top = middle - self.text_height(hdc) / 2;
        let clip = RECT { right, ..inside };
        let name_left = inside.left + s(26);
        self.label_ellipsis(hdc, &name, name_left, text_top, self.theme.text, clip);
        let folder_left = name_left + self.text_width(hdc, &name) + s(10);
        if let Some(parent) = path.parent().filter(|_| folder_left < right) {
            self.label_ellipsis(
                hdc,
                &display_path(parent),
                folder_left,
                text_top,
                self.theme.muted,
                clip,
            );
        }
        if hovered && let Some(forget) = row.forget {
            let close = RECT {
                left: row.rect.right - s(26),
                top: middle - s(10),
                right: row.rect.right - s(6),
                bottom: middle + s(10),
            };
            let on_close = matches!(self.hot, Some(Hot::Welcome(index)) if index == forget);
            if on_close {
                Self::rounded_fill(hdc, close, s(3), self.theme.select_bg);
            }
            self.glyph_in(
                hdc,
                DebugGlyph::Close,
                if on_close {
                    self.theme.text
                } else {
                    self.theme.muted
                },
                close,
                s(14),
            );
        }
    }

    // Get started: an icon tile, the title and what it's for, and its key.
    // Compact (narrow or short window): icon, title and key on one line.
    fn paint_step_row(&self, hdc: HDC, layout: &WelcomeLayout, row: &Row, index: usize) {
        let s = |value: i32| self.scale(value);
        let (title, about, key, glyph, tint, _) = STEPS[index];
        let color = match tint {
            Tint::Violet => self.theme.violet,
            Tint::Blue => self.theme.blue,
            Tint::Green => self.theme.green,
            Tint::Orange => self.theme.orange,
            Tint::Muted => self.theme.muted,
        };
        let pad = s(if layout.compact_steps { 8 } else { 10 });
        let inside = RECT {
            left: row.rect.left + pad,
            right: row.rect.right - pad,
            ..row.rect
        };
        let middle = (inside.top + inside.bottom) / 2;
        unsafe { SelectObject(hdc, self.small_font) };
        let key_right = if key.is_empty() {
            inside.right
        } else {
            let width = self.text_width(hdc, key) + s(12);
            let chip = RECT {
                left: inside.right - width,
                top: middle - s(9),
                right: inside.right,
                bottom: middle + s(9),
            };
            self.panel_card(hdc, chip, s(3), self.theme.edge, self.theme.active_bg);
            self.label_mid(hdc, key, chip.left + s(6), middle, self.theme.muted, chip);
            chip.left - s(12)
        };
        let text_left = if layout.compact_steps {
            self.icons
                .draw_glyph(hdc, glyph, color, inside.left, middle - s(8), s(16));
            inside.left + s(26)
        } else {
            let tile = RECT {
                left: inside.left,
                top: middle - s(16),
                right: inside.left + s(32),
                bottom: middle + s(16),
            };
            self.panel_card(hdc, tile, s(6), self.theme.edge, self.theme.sidebar_bg);
            self.glyph_in(hdc, glyph, color, tile, s(17));
            tile.right + s(12)
        };
        let clip = RECT {
            left: text_left,
            right: key_right,
            ..inside
        };
        unsafe { SelectObject(hdc, self.ui_font) };
        if layout.compact_steps {
            self.label_ellipsis(
                hdc,
                title,
                text_left,
                middle - self.text_height(hdc) / 2,
                self.theme.text,
                clip,
            );
            return;
        }
        self.label_ellipsis(hdc, title, text_left, middle - s(18), self.theme.text, clip);
        unsafe { SelectObject(hdc, self.small_font) };
        self.label_ellipsis(hdc, about, text_left, middle + s(1), self.theme.muted, clip);
    }

    fn paint_welcome_status(&self, hdc: HDC, rect: RECT, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        let bar = RECT {
            left: 0,
            top: layout.status_top,
            right: rect.right,
            bottom: rect.bottom,
        };
        Self::fill(hdc, bar, self.theme.status_bg);
        Self::fill(
            hdc,
            RECT {
                bottom: bar.top + s(1).max(1),
                ..bar
            },
            self.theme.edge,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        let middle = (layout.status_top + rect.bottom) / 2;
        let left_text = match &self.workspace_branch {
            Some(branch) => format!("Ready  \u{2022}  {branch}"),
            None => "Ready".into(),
        };
        self.label_mid(hdc, &left_text, s(16), middle, self.theme.muted, bar);
    }
}

/// Milliseconds since this process was created (for the "Started in" pill),
/// or None if Windows doesn't say.
pub(in crate::windows_app) fn process_age_ms() -> Option<u32> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    let ok = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    if ok == 0 {
        return None;
    }
    // FILETIME counts 100 ns ticks since 1601; Unix time starts 11,644,473,600 s later.
    let created = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let now = (now.as_secs() + 11_644_473_600) * 10_000_000 + u64::from(now.subsec_nanos() / 100);
    u32::try_from(now.checked_sub(created)? / 10_000).ok()
}

fn recent_age(path: &std::path::Path) -> String {
    let Ok(modified) = std::fs::metadata(path).and_then(|metadata| metadata.modified()) else {
        return String::new();
    };
    let Ok(age) = modified.elapsed() else {
        return String::new();
    };
    let seconds = age.as_secs();
    if seconds < 3_600 {
        let minutes = (seconds / 60).max(1);
        format!("{minutes} min ago")
    } else if seconds < 86_400 {
        let hours = seconds / 3_600;
        format!("{hours} hour{} ago", if hours == 1 { "" } else { "s" })
    } else if seconds < 604_800 {
        let days = seconds / 86_400;
        format!("{days} day{} ago", if days == 1 { "" } else { "s" })
    } else {
        let weeks = seconds / 604_800;
        format!("{weeks} week{} ago", if weeks == 1 { "" } else { "s" })
    }
}

impl WelcomeLayout {
    pub(in crate::windows_app) fn hit(&self, x: i32, y: i32) -> Option<WelcomeAction> {
        self.targets
            .iter()
            .find(|(rect, _)| x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom)
            .map(|(_, action)| *action)
    }

    /// What to redraw when target `index` gains or loses the mouse: a Recent
    /// row's × lights its whole row.
    pub(in crate::windows_app) fn hot_rect(&self, index: usize) -> Option<RECT> {
        let rect = self.targets.get(index)?.0;
        Some(
            self.rows
                .iter()
                .find(|row| row.forget == Some(index))
                .map_or(rect, |row| row.rect),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_action_appears_once_on_the_page() {
        // The rail may share an action with a row (Search, Settings); the
        // page's rows may not repeat one another.
        let mut seen: Vec<WelcomeAction> = START.iter().map(|item| item.3).collect();
        for step in STEPS {
            assert!(!seen.contains(&step.5), "{:?} is on the page twice", step.5);
            seen.push(step.5);
        }
    }

    #[test]
    fn learn_links_are_the_help_menus() {
        let urls: Vec<&str> = LINKS.iter().map(|link| link.2).collect();
        assert_eq!(
            urls,
            [SHORTCUTS_URL, GUIDE_URL, RELEASES_URL, NEW_ISSUE_URL]
        );
        assert!(
            urls.iter()
                .all(|url| url.starts_with("https://github.com/"))
        );
    }

    #[test]
    fn process_age_is_measured() {
        assert!(process_age_ms().is_some());
    }
}
