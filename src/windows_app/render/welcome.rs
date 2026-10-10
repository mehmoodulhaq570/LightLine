use super::super::*;

// Welcome screen geometry, in logical pixels (everything goes through scale()).
const PAGE_GUTTER: i32 = 20;
const PAGE_TOP: i32 = 12;
const WELCOME_RAIL_FIRST_ROW: i32 = 18;
const WELCOME_RAIL_ROW: i32 = 52;
const WELCOME_CARD_GAP: i32 = 10;
const PANEL_HEADER_H: i32 = 45;
const RECENT_ROW_H: i32 = 50;
const QUICK_ROW_H: i32 = 33;
const COMMUNITY_H: i32 = 53;

const CARD_BG: u32 = rgb(16, 27, 45);
// Distinct from (and one shade lighter than) the theme's general
// card_edge -- a pre-existing welcome-screen-only variation, not a copy/
// paste of the theme color, so it keeps its own name rather than being
// folded into Theme.
const WELCOME_CARD_EDGE: u32 = rgb(32, 48, 76);
const CHIP_BG: u32 = rgb(24, 38, 62);

// Everything on this screen maps to a real command; nothing is decorative.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::windows_app) enum WelcomeAction {
    Explorer,
    Search,
    SourceControl,
    RunDebug,
    Extensions,
    AiAssistant,
    OpenFile,
    OpenFolder,
    NewFile,
    Terminal,
    CommandPalette,
    Community,
    Recent(usize),
    Settings,
}

/// What the welcome rail's button `index` (as in `WelcomeLayout::targets`,
/// which starts with them) opens, for its tip; None past them.
pub(in crate::windows_app) fn welcome_rail_tip(
    targets: &[(RECT, WelcomeAction)],
    index: usize,
) -> Option<&'static str> {
    if let Some((label, _, _)) = NAV_ITEMS.get(index) {
        return Some(label);
    }
    (targets.get(index)?.1 == WelcomeAction::Settings).then_some("Settings (Ctrl+,)")
}

// Rail icon indices shared with App::rail_icon, so the welcome nav and the
// editor's rail draw the exact same glyph for the same destination.
const NAV_ITEMS: [(&str, usize, WelcomeAction); 6] = [
    ("Explorer", 0, WelcomeAction::Explorer),
    ("Search", 1, WelcomeAction::Search),
    ("Source Control", 2, WelcomeAction::SourceControl),
    ("Run & Debug", 3, WelcomeAction::RunDebug),
    ("Extensions", 4, WelcomeAction::Extensions),
    ("AI Assistant", 5, WelcomeAction::AiAssistant),
];

const QUICK: [(&str, &str, WelcomeAction); 6] = [
    ("New File", "Ctrl+N", WelcomeAction::NewFile),
    ("Open File", "Ctrl+O", WelcomeAction::OpenFile),
    ("Open Folder", "Ctrl+Shift+O", WelcomeAction::OpenFolder),
    ("Command Palette", "Ctrl+P", WelcomeAction::CommandPalette),
    ("Search in Files", "Ctrl+Shift+F", WelcomeAction::Search),
    ("Terminal", "Ctrl+`", WelcomeAction::Terminal),
];

const STEPS: [(&str, &str, WelcomeAction); 4] = [
    (
        "Open a project folder",
        "Start coding in seconds",
        WelcomeAction::OpenFolder,
    ),
    (
        "Search across files",
        "Find anything instantly",
        WelcomeAction::Search,
    ),
    (
        "Run your first build",
        "See your code come to life",
        WelcomeAction::RunDebug,
    ),
    (
        "Review Git changes",
        "Track what you changed",
        WelcomeAction::SourceControl,
    ),
];

// Computed once per paint and again per click, so painted rectangles and click
// targets are always derived from the same numbers.
pub(in crate::windows_app) struct WelcomeLayout {
    pub(in crate::windows_app) targets: Vec<(RECT, WelcomeAction)>,
    rail_right: i32,
    content_top: i32,
    status_top: i32,
    hero_panel: RECT,
    open_folder: RECT,
    new_file: RECT,
    recent_panel: RECT,
    recent_rows: usize,
    quick_panel: RECT,
    quick_rows: usize,
    steps_panel: Option<RECT>,
    community: RECT,
}

impl App {
    pub(in crate::windows_app) fn welcome_layout(&self, rect: RECT) -> WelcomeLayout {
        let s = |value: i32| self.scale(value);
        let mut targets = Vec::new();
        let status_top = rect.bottom - s(STATUS);
        let rail_right = s(RAIL);
        let content_top = s(WORKBENCH_HEADER);
        let gutter = s(PAGE_GUTTER);

        let page_left = rail_right + gutter;
        let page_right = rect.right - gutter;
        let page_top = content_top + s(PAGE_TOP);
        let page_bottom = status_top - s(8);
        let gap = s(WELCOME_CARD_GAP);
        let community = RECT {
            left: page_left,
            top: page_bottom - s(COMMUNITY_H),
            right: page_right,
            bottom: page_bottom,
        };
        let row_bottom = community.top - gap;
        let row_space = (row_bottom - page_top - gap).max(0);
        let mut hero_height = row_space * 55 / 100;
        if row_space >= s(270) {
            hero_height = hero_height.max(s(180)).min(row_space - s(90));
        }
        let lower_top = page_top + hero_height + gap;
        let page_width = page_right - page_left;
        let show_aside = page_width >= s(1040);
        let aside_width = if show_aside {
            (page_width - gap) * 312 / 1000
        } else {
            0
        };
        let hero_panel = RECT {
            left: page_left,
            top: page_top,
            right: if show_aside {
                page_right - aside_width - gap
            } else {
                page_right
            },
            bottom: page_top + hero_height,
        };
        let steps_panel = show_aside.then_some(RECT {
            left: hero_panel.right + gap,
            top: page_top,
            right: page_right,
            bottom: hero_panel.bottom,
        });

        for (index, (_, _, action)) in NAV_ITEMS.iter().enumerate() {
            // Home occupies row zero; the six workbench destinations follow it.
            let top =
                content_top + s(WELCOME_RAIL_FIRST_ROW) + (index as i32 + 1) * s(WELCOME_RAIL_ROW);
            targets.push((
                RECT {
                    left: s(7),
                    top,
                    right: rail_right - s(7),
                    bottom: top + s(42),
                },
                *action,
            ));
        }

        targets.push((
            RECT {
                left: s(7),
                top: status_top - s(42),
                right: rail_right - s(7),
                bottom: status_top - s(4),
            },
            WelcomeAction::Settings,
        ));

        let compact_hero = hero_panel.bottom - hero_panel.top < s(270);
        let button_height = if compact_hero { s(46) } else { s(50) };
        let button_bottom_gap = if compact_hero { s(14) } else { s(35) };
        let button_top = hero_panel.bottom - button_bottom_gap - button_height;
        let open_folder = RECT {
            left: hero_panel.left + s(21),
            top: button_top,
            right: hero_panel.left + s(207),
            bottom: button_top + button_height,
        };
        let new_file = RECT {
            left: open_folder.right + s(14),
            top: button_top,
            right: open_folder.right + s(169),
            bottom: button_top + button_height,
        };
        targets.push((open_folder, WelcomeAction::OpenFolder));
        targets.push((new_file, WelcomeAction::NewFile));

        let lower_height = (row_bottom - lower_top).max(0);
        let body = (lower_height - s(PANEL_HEADER_H)).max(0);
        let recent_rows = self
            .recent
            .len()
            .min(4)
            .min((body / s(RECENT_ROW_H)).max(0) as usize);
        let show_quick = page_width >= s(560);
        let quick_rows = if show_quick {
            QUICK.len().min((body / s(QUICK_ROW_H)).max(0) as usize)
        } else {
            0
        };
        let recent_width = if show_quick {
            (page_width - gap) * 595 / 1000
        } else {
            page_width
        };
        let recent_panel = RECT {
            left: page_left,
            top: lower_top,
            right: page_left + recent_width,
            bottom: row_bottom,
        };
        let quick_panel = RECT {
            left: if show_quick {
                recent_panel.right + gap
            } else {
                page_right
            },
            top: lower_top,
            right: page_right,
            bottom: row_bottom,
        };
        for index in 0..recent_rows {
            let top = recent_panel.top + s(PANEL_HEADER_H) + index as i32 * s(RECENT_ROW_H);
            targets.push((
                RECT {
                    left: recent_panel.left + s(8),
                    top,
                    right: recent_panel.right - s(8),
                    bottom: top + s(RECENT_ROW_H),
                },
                WelcomeAction::Recent(index),
            ));
        }
        for (index, (_, _, action)) in QUICK.iter().take(quick_rows).enumerate() {
            let top = quick_panel.top + s(PANEL_HEADER_H) + index as i32 * s(QUICK_ROW_H);
            targets.push((
                RECT {
                    left: quick_panel.left + s(8),
                    top,
                    right: quick_panel.right - s(8),
                    bottom: top + s(QUICK_ROW_H),
                },
                *action,
            ));
        }

        if let Some(steps) = steps_panel {
            let step_height =
                (steps.bottom - steps.top - s(PANEL_HEADER_H) - s(8)) / STEPS.len() as i32;
            for (index, (_, _, action)) in STEPS.iter().enumerate() {
                let top = steps.top + s(PANEL_HEADER_H) + index as i32 * step_height;
                targets.push((
                    RECT {
                        left: steps.left + s(8),
                        top,
                        right: steps.right - s(8),
                        bottom: top + step_height,
                    },
                    *action,
                ));
            }
        }
        targets.push((community, WelcomeAction::Community));

        WelcomeLayout {
            targets,
            rail_right,
            content_top,
            status_top,
            hero_panel,
            open_folder,
            new_file,
            recent_panel,
            recent_rows,
            quick_panel,
            quick_rows,
            steps_panel,
            community,
        }
    }

    pub(in crate::windows_app) fn paint_welcome(&self, hwnd: HWND, hdc: HDC, rect: RECT) {
        let layout = self.welcome_layout(rect);
        Self::fill(hdc, rect, self.theme.shell_bg);
        self.paint_welcome_header(hwnd, hdc, rect, &layout);
        self.paint_welcome_nav(hdc, &layout);
        self.paint_welcome_hero(hdc, &layout);
        self.paint_welcome_recent(hdc, &layout);
        self.paint_welcome_quick(hdc, &layout);
        self.paint_welcome_aside(hdc, &layout);
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
        unsafe {
            DrawIconEx(
                hdc,
                s(14),
                s(9),
                self.brand_icon,
                s(32),
                s(32),
                0,
                null_mut(),
                DI_NORMAL,
            );
            SelectObject(hdc, self.brand_font);
        }
        let name_width = self.text_width(hdc, "LightLine");
        Self::label(
            hdc,
            "LightLine",
            s(56),
            s(9),
            self.theme.text,
            RECT {
                left: s(56),
                top: 0,
                right: s(56) + name_width,
                bottom: layout.content_top,
            },
        );
        self.paint_ide_badge(
            hdc,
            self.ui_font,
            s(56) + name_width + s(6),
            s(25),
            s(20),
            s(6),
            s(5),
            ui(63, 47, 150),
        );
        unsafe { SelectObject(hdc, self.ui_font) };

        let command = self.command_center_rect(hwnd);
        if command.right > command.left {
            self.panel_card(
                hdc,
                command,
                s(6),
                self.command_center_edge(),
                ui(12, 25, 48),
            );
            self.rail_icon(
                hdc,
                1,
                command.left + s(10),
                command.top + s(6),
                self.theme.muted,
            );
            Self::label(
                hdc,
                "Search files, symbols, commands...",
                command.left + s(36),
                command.top + s(5),
                self.theme.muted,
                RECT {
                    left: command.left + s(36),
                    top: command.top,
                    right: command.right - s(55),
                    bottom: command.bottom,
                },
            );
            let key = RECT {
                left: command.right - s(48),
                top: command.top + s(4),
                right: command.right - s(7),
                bottom: command.bottom - s(4),
            };
            Self::rounded_fill(hdc, key, s(4), ui(25, 43, 76));
            Self::label(
                hdc,
                "Ctrl P",
                key.left + s(5),
                key.top + s(1),
                self.theme.text,
                key,
            );
        }

        self.paint_hot_title_button(hdc, hwnd);
        let button = s(46);
        let controls_left = rect.right - button * 3;
        let middle = layout.content_top / 2;
        self.stroke(hdc, self.theme.muted, |hdc| unsafe {
            MoveToEx(hdc, controls_left + s(17), middle + s(5), null_mut());
            LineTo(hdc, controls_left + s(29), middle + s(5));
            let max_left = controls_left + button + s(17);
            Rectangle(
                hdc,
                max_left,
                middle - s(6),
                max_left + s(12),
                middle + s(6),
            );
            let close_left = controls_left + button * 2 + s(17);
            MoveToEx(hdc, close_left, middle - s(6), null_mut());
            LineTo(hdc, close_left + s(12), middle + s(6));
            MoveToEx(hdc, close_left + s(12), middle - s(6), null_mut());
            LineTo(hdc, close_left, middle + s(6));
        });
    }

    fn paint_welcome_nav(&self, hdc: HDC, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        let clip = RECT {
            left: 0,
            top: layout.content_top,
            right: layout.rail_right,
            bottom: layout.status_top,
        };
        Self::fill(hdc, clip, self.theme.rail_bg);
        Self::fill(
            hdc,
            RECT {
                left: layout.rail_right - s(1).max(1),
                top: layout.content_top,
                right: layout.rail_right,
                bottom: layout.status_top,
            },
            self.theme.edge,
        );
        // Welcome is the selected activity and remains intentionally inert.
        let first_top = layout.content_top + s(WELCOME_RAIL_FIRST_ROW);
        let welcome_row = RECT {
            left: s(7),
            top: first_top,
            right: layout.rail_right - s(7),
            bottom: first_top + s(42),
        };
        self.panel_card(hdc, welcome_row, s(7), ui(50, 84, 154), ui(18, 35, 72));
        Self::fill(
            hdc,
            RECT {
                left: 0,
                top: welcome_row.top + s(5),
                right: s(3),
                bottom: welcome_row.bottom - s(5),
            },
            self.theme.violet,
        );
        self.home_glyph(
            hdc,
            s(19),
            welcome_row.top + s(11),
            s(20),
            label_on(ui(18, 35, 72), 240, 245, 255),
        );

        for (index, (_, icon, _)) in NAV_ITEMS.iter().enumerate() {
            let top = first_top + (index as i32 + 1) * s(WELCOME_RAIL_ROW);
            self.rail_icon(hdc, *icon, s(19), top + s(11), self.theme.muted);
        }
        Self::label(
            hdc,
            "⚙",
            s(18),
            layout.status_top - s(34),
            self.theme.muted,
            clip,
        );
    }

    fn paint_welcome_hero(&self, hdc: HDC, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        let panel = layout.hero_panel;
        let panel_width = panel.right - panel.left;
        let panel_height = panel.bottom - panel.top;
        let compact = panel_height < s(270);
        self.gradient_card(
            hdc,
            panel,
            s(12),
            themed(rgb(9, 20, 39)),
            themed(rgb(17, 22, 55)),
        );
        self.card_outline(hdc, panel, s(12), themed(WELCOME_CARD_EDGE));

        let left = panel.left + s(21);
        let clip = RECT {
            left,
            top: panel.top + s(10),
            right: panel.right - s(18),
            bottom: panel.bottom - s(10),
        };
        unsafe {
            SelectObject(hdc, self.ui_font);
            SetTextCharacterExtra(hdc, s(2));
        }
        Self::label(
            hdc,
            "WELCOME TO",
            left,
            panel.top + s(if compact { 12 } else { 17 }),
            self.theme.violet,
            clip,
        );
        unsafe { SetTextCharacterExtra(hdc, 0) };

        let logo_top = panel.top + s(if compact { 30 } else { 44 });
        let logo_size = s(if compact { 50 } else { 58 });
        unsafe {
            DrawIconEx(
                hdc,
                left,
                logo_top,
                self.hero_icon,
                logo_size,
                logo_size,
                0,
                null_mut(),
                DI_NORMAL,
            );
            SelectObject(hdc, self.hero_font);
        }
        let word_left = left + logo_size + s(14);
        self.label_mid(
            hdc,
            "LightLine",
            word_left,
            logo_top + logo_size / 2,
            self.theme.text,
            clip,
        );
        let badge_left = word_left + self.text_width(hdc, "LightLine") + s(16);
        self.paint_ide_badge(
            hdc,
            self.title_font,
            badge_left,
            logo_top + logo_size / 2,
            s(if compact { 34 } else { 36 }),
            s(12),
            s(7),
            ui(78, 56, 176),
        );
        unsafe { SelectObject(hdc, self.title_font) };
        let slogan_top = logo_top + s(if compact { 50 } else { 70 });
        if !compact || layout.open_folder.top - slogan_top >= s(24) {
            Self::label(
                hdc,
                "Build Faster. Think Smarter.",
                left,
                slogan_top,
                self.theme.text,
                clip,
            );
        }
        unsafe { SelectObject(hdc, self.ui_font) };
        if panel_height >= s(250) {
            unsafe { SetTextCharacterExtra(hdc, s(1)) };
            Self::label(
                hdc,
                "A modern, AI-powered IDE for developers.",
                left,
                logo_top + s(if compact { 99 } else { 115 }),
                self.theme.muted,
                clip,
            );
            Self::label(
                hdc,
                "Fast to start, focused to work in.",
                left,
                logo_top + s(if compact { 117 } else { 138 }),
                self.theme.muted,
                clip,
            );
            unsafe { SetTextCharacterExtra(hdc, 0) };
        }

        let primary = layout.open_folder;
        Self::rounded_fill(hdc, primary, s(7), ui(112, 52, 238));
        self.welcome_folder_outline(
            hdc,
            primary.left + s(29),
            primary.top + (primary.bottom - primary.top - s(22)) / 2,
            s(22),
            self.theme.text,
        );
        unsafe { SelectObject(hdc, self.brand_font) };
        self.label_mid(
            hdc,
            "Open Folder",
            primary.left + s(67),
            (primary.top + primary.bottom) / 2,
            self.theme.text,
            primary,
        );

        let secondary = layout.new_file;
        self.panel_card(
            hdc,
            secondary,
            s(7),
            ui(77, 102, 145),
            themed(rgb(11, 23, 42)),
        );
        if !self.icons.draw_generic(
            hdc,
            GenericIcon::File,
            secondary.left + s(26),
            secondary.top + (secondary.bottom - secondary.top - s(20)) / 2,
            s(20),
        ) {
            self.draw_vector_file(
                hdc,
                std::path::Path::new("untitled"),
                secondary.left + s(26),
                secondary.top + (secondary.bottom - secondary.top - s(20)) / 2,
                s(20),
            );
        }
        self.label_mid(
            hdc,
            "New File",
            secondary.left + s(60),
            (secondary.top + secondary.bottom) / 2,
            self.theme.text,
            secondary,
        );

        // The right side is deliberately drawn as vectors so it remains sharp
        // at every DPI and follows the selected theme.
        if panel_width < s(760) || panel_height < s(180) {
            return;
        }
        let art_left = panel.left + panel_width * 55 / 100;
        let art_top = panel.top + panel_height * 23 / 100;
        let art_width = (panel_width * 32 / 100).min(s(274));
        let art_height = (panel_height * 54 / 100).min(s(168));
        let art_right = art_left + art_width;
        let art_bottom = art_top + art_height;
        let code = RECT {
            left: art_left,
            top: art_top,
            right: art_right,
            bottom: art_bottom,
        };
        let code_width = code.right - code.left;
        let code_height = code.bottom - code.top;

        self.gradient_card(hdc, code, s(9), ui(23, 37, 79), ui(12, 26, 57));
        self.card_outline(hdc, code, s(9), ui(64, 91, 177));
        let chrome_height = if compact { 24 } else { 30 };
        Self::fill(
            hdc,
            RECT {
                left: code.left,
                top: code.top + s(chrome_height),
                right: code.right,
                bottom: code.top + s(chrome_height + 1),
            },
            ui(52, 75, 134),
        );
        Self::fill(
            hdc,
            RECT {
                left: code.left + s(42),
                top: code.top + s(chrome_height + 1),
                right: code.left + s(43),
                bottom: code.bottom - s(1),
            },
            ui(45, 68, 120),
        );
        for index in 0..3 {
            let size = s(if compact { 5 } else { 6 });
            Self::rounded_fill(
                hdc,
                RECT {
                    left: code.left + s(16 + index * 15),
                    top: code.top + s(if compact { 10 } else { 13 }),
                    right: code.left + s(16 + index * 15) + size,
                    bottom: code.top + s(if compact { 10 } else { 13 }) + size,
                },
                size,
                ui(86, 127, 214),
            );
        }
        let line_colors = [
            ui(44, 73, 132),
            ui(109, 55, 224),
            ui(42, 139, 221),
            ui(50, 91, 163),
        ];
        let code_rows = if compact { 5 } else { 6 };
        let row_top = if compact { 34 } else { 47 };
        let row_gap = if compact { 14 } else { 17 };
        let row_height = if compact { 5 } else { 6 };
        for index in 0..code_rows {
            let y = code.top + s(row_top + index * row_gap);
            Self::rounded_fill(
                hdc,
                RECT {
                    left: code.left + s(12),
                    top: y,
                    right: code.left + s(30),
                    bottom: y + s(row_height),
                },
                s(2),
                ui(42, 69, 125),
            );
            let width = if compact {
                s(40 + ((index * 17) % 42))
            } else {
                s(55 + ((index * 19) % 48))
            };
            Self::rounded_fill(
                hdc,
                RECT {
                    left: code.left + s(54),
                    top: y,
                    right: code.left + s(54) + width,
                    bottom: y + s(row_height),
                },
                s(3),
                line_colors[index as usize % line_colors.len()],
            );
        }
        let bolt_x = code.left + code_width * 71 / 100;
        let bolt_y = code.top + code_height * 26 / 100;
        let bolt =
            [(51, 0), (0, 61), (26, 61), (13, 104), (62, 45), (36, 45)].map(|(x, y)| POINT {
                x: bolt_x + s(x),
                y: bolt_y + s(y),
            });
        self.welcome_lightning_bolt(hdc, &bolt);
        unsafe { SelectObject(hdc, self.ui_font) };
        unsafe { SetTextCharacterExtra(hdc, s(2)) };
        let labels_left = code.right + s(21);
        for (index, label) in ["CODE", "BUILD", "DEBUG", "CREATE"].iter().enumerate() {
            Self::label(
                hdc,
                label,
                labels_left,
                code.top + s(58 + index as i32 * 22),
                ui(128, 151, 212),
                clip,
            );
        }
        Self::fill(
            hdc,
            RECT {
                left: labels_left,
                top: code.top + s(156),
                right: labels_left + s(22),
                bottom: code.top + s(157),
            },
            ui(74, 137, 224),
        );
        if !compact {
            unsafe { SetTextCharacterExtra(hdc, 0) };
            let tagline_right = code.right + s(89);
            let first = "A BRIGHTER";
            let second = "DEVELOPMENT TOMORROW";
            Self::label(
                hdc,
                first,
                tagline_right - self.text_width(hdc, first),
                code.top + s(178),
                ui(105, 128, 186),
                clip,
            );
            Self::label(
                hdc,
                second,
                tagline_right - self.text_width(hdc, second),
                code.top + s(197),
                ui(105, 128, 186),
                clip,
            );
        }
        unsafe { SetTextCharacterExtra(hdc, 0) };
    }

    fn paint_welcome_recent(&self, hdc: HDC, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        let panel = layout.recent_panel;
        self.panel_card(hdc, panel, s(9), themed(WELCOME_CARD_EDGE), themed(CARD_BG));
        let clip = RECT {
            left: panel.left + s(14),
            top: panel.top,
            right: panel.right - s(12),
            bottom: panel.bottom,
        };
        unsafe { SelectObject(hdc, self.brand_font) };
        self.clock_glyph(
            hdc,
            panel.left + s(10),
            panel.top + s(16),
            s(19),
            self.theme.blue,
        );
        self.label_mid(
            hdc,
            "Recent Projects",
            panel.left + s(42),
            panel.top + s(26),
            self.theme.text,
            clip,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        if panel.right - panel.left >= s(320) {
            let see_all = "View all  \u{2192}";
            let see_all_width = self.text_width(hdc, see_all);
            self.label_mid(
                hdc,
                see_all,
                panel.right - s(23) - see_all_width,
                panel.top + s(26),
                self.theme.muted,
                clip,
            );
        }
        Self::fill(
            hdc,
            RECT {
                left: panel.left + s(10),
                top: panel.top + s(PANEL_HEADER_H),
                right: panel.right - s(10),
                bottom: panel.top + s(PANEL_HEADER_H) + s(1).max(1),
            },
            self.theme.edge,
        );
        if layout.recent_rows == 0 {
            Self::label(
                hdc,
                "Workspaces you open will show up here.",
                panel.left + s(18),
                panel.top + s(60),
                self.theme.muted,
                clip,
            );
            return;
        }
        for (index, path) in self.recent.iter().take(layout.recent_rows).enumerate() {
            let top = panel.top + s(PANEL_HEADER_H) + index as i32 * s(RECENT_ROW_H);
            let row = RECT {
                left: panel.left + s(8),
                top,
                right: panel.right - s(8),
                bottom: top + s(RECENT_ROW_H),
            };
            if index > 0 {
                Self::fill(
                    hdc,
                    RECT {
                        left: panel.left + s(10),
                        top,
                        right: panel.right - s(10),
                        bottom: top + s(1).max(1),
                    },
                    self.theme.edge,
                );
            }
            self.welcome_folder_glyph(hdc, row.left + s(20), top + s(13), s(26), ui(79, 142, 242));
            let text_left = row.left + s(71);
            let show_age = panel.right - panel.left >= s(340);
            let row_clip = RECT {
                left: text_left,
                top: row.top,
                right: if show_age {
                    row.right - s(116)
                } else {
                    row.right - s(24)
                },
                bottom: row.bottom,
            };
            Self::label(
                hdc,
                &path.file_name().unwrap_or_default().to_string_lossy(),
                text_left,
                top + s(8),
                self.theme.text,
                row_clip,
            );
            self.label_ellipsis(
                hdc,
                &display_path(path),
                text_left,
                top + s(27),
                self.theme.muted,
                row_clip,
            );
            if show_age {
                let age = recent_age(path);
                let age_width = self.text_width(hdc, &age);
                self.label_mid(
                    hdc,
                    &age,
                    row.right - s(59) - age_width,
                    (row.top + row.bottom) / 2,
                    self.theme.muted,
                    row,
                );
            }
            self.chevron(hdc, row.right - s(26), (row.top + row.bottom) / 2, false);
        }
    }

    fn paint_welcome_quick(&self, hdc: HDC, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        let panel = layout.quick_panel;
        if panel.right <= panel.left || panel.bottom <= panel.top {
            return;
        }
        self.panel_card(hdc, panel, s(9), themed(WELCOME_CARD_EDGE), themed(CARD_BG));
        let clip = RECT {
            left: panel.left + s(14),
            top: panel.top,
            right: panel.right - s(12),
            bottom: panel.bottom,
        };
        unsafe { SelectObject(hdc, self.brand_font) };
        self.rail_icon(
            hdc,
            5,
            panel.left + s(21),
            panel.top + s(17),
            self.theme.violet,
        );
        self.label_mid(
            hdc,
            "Quick Actions",
            panel.left + s(59),
            panel.top + s(26),
            self.theme.text,
            clip,
        );
        Self::fill(
            hdc,
            RECT {
                left: panel.left + s(18),
                top: panel.top + s(PANEL_HEADER_H),
                right: panel.right - s(18),
                bottom: panel.top + s(PANEL_HEADER_H) + s(1).max(1),
            },
            self.theme.edge,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        for (index, (label, shortcut, _)) in QUICK.iter().take(layout.quick_rows).enumerate() {
            let top = panel.top + s(PANEL_HEADER_H) + index as i32 * s(QUICK_ROW_H);
            let middle = top + s(QUICK_ROW_H) / 2;
            if index > 0 {
                Self::fill(
                    hdc,
                    RECT {
                        left: panel.left + s(18),
                        top,
                        right: panel.right - s(18),
                        bottom: top + s(1).max(1),
                    },
                    self.theme.edge,
                );
            }
            let show_shortcut = panel.right - panel.left >= s(220);
            let chip_width = self.text_width(hdc, shortcut) + s(16);
            let chip = RECT {
                left: panel.right - s(27) - chip_width,
                top: middle - s(11),
                right: panel.right - s(27),
                bottom: middle + s(11),
            };
            if show_shortcut {
                Self::rounded_fill(hdc, chip, s(5), themed(CHIP_BG));
                self.label_mid(
                    hdc,
                    shortcut,
                    chip.left + s(8),
                    middle,
                    self.theme.muted,
                    clip,
                );
            }
            match index {
                0 | 1 => {
                    if !self.icons.draw_generic(
                        hdc,
                        GenericIcon::File,
                        panel.left + s(25),
                        middle - s(9),
                        s(18),
                    ) {
                        self.draw_vector_file(
                            hdc,
                            std::path::Path::new("untitled"),
                            panel.left + s(25),
                            middle - s(9),
                            s(18),
                        );
                    }
                }
                2 => {
                    self.welcome_folder_glyph(
                        hdc,
                        panel.left + s(25),
                        middle - s(10),
                        s(18),
                        ui(139, 174, 225),
                    );
                }
                3 => Self::label(
                    hdc,
                    "\u{2318}",
                    panel.left + s(25),
                    middle - s(11),
                    self.theme.muted,
                    clip,
                ),
                4 => self.rail_icon(hdc, 1, panel.left + s(25), middle - s(9), self.theme.muted),
                _ => self.prompt_glyph(
                    hdc,
                    panel.left + s(25),
                    middle - s(9),
                    s(18),
                    self.theme.muted,
                ),
            }
            let label_clip = RECT {
                left: panel.left + s(69),
                top,
                right: if show_shortcut {
                    chip.left - s(8)
                } else {
                    panel.right - s(12)
                },
                bottom: top + s(QUICK_ROW_H),
            };
            self.label_mid(
                hdc,
                label,
                panel.left + s(69),
                middle,
                self.theme.text,
                label_clip,
            );
        }
    }

    fn paint_welcome_aside(&self, hdc: HDC, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        if let Some(panel) = layout.steps_panel {
            self.panel_card(hdc, panel, s(9), themed(WELCOME_CARD_EDGE), themed(CARD_BG));
            let clip = RECT {
                left: panel.left + s(14),
                top: panel.top,
                right: panel.right - s(12),
                bottom: panel.bottom,
            };
            unsafe { SelectObject(hdc, self.brand_font) };
            self.bulb_glyph(
                hdc,
                panel.left + s(18),
                panel.top + s(17),
                s(18),
                self.theme.text,
            );
            self.label_mid(
                hdc,
                "Getting Started",
                panel.left + s(48),
                panel.top + s(27),
                self.theme.text,
                clip,
            );
            Self::fill(
                hdc,
                RECT {
                    left: panel.left + s(18),
                    top: panel.top + s(PANEL_HEADER_H),
                    right: panel.right - s(18),
                    bottom: panel.top + s(PANEL_HEADER_H) + s(1).max(1),
                },
                self.theme.edge,
            );
            let step_height =
                (panel.bottom - panel.top - s(PANEL_HEADER_H) - s(8)) / STEPS.len() as i32;
            for (index, (title, subtitle, _)) in STEPS.iter().enumerate() {
                let top = panel.top + s(PANEL_HEADER_H) + index as i32 * step_height;
                let middle = top + step_height / 2;
                let ring = s(30);
                self.ring_glyph(
                    hdc,
                    panel.left + s(18),
                    middle - ring / 2,
                    ring,
                    self.theme.violet,
                );
                unsafe { SelectObject(hdc, self.brand_font) };
                let number = (index + 1).to_string();
                let number_width = self.text_width(hdc, &number);
                self.label_mid(
                    hdc,
                    &number,
                    panel.left + s(18) + (ring - number_width) / 2,
                    middle,
                    self.theme.text,
                    clip,
                );
                let text_left = panel.left + s(64);
                let row_clip = RECT {
                    left: text_left,
                    top,
                    right: panel.right - s(30),
                    bottom: top + step_height,
                };
                self.label_ellipsis(
                    hdc,
                    title,
                    text_left,
                    middle - s(17),
                    self.theme.text,
                    row_clip,
                );
                unsafe { SelectObject(hdc, self.ui_font) };
                self.label_ellipsis(
                    hdc,
                    subtitle,
                    text_left,
                    middle + s(2),
                    self.theme.muted,
                    row_clip,
                );
                self.chevron(hdc, panel.right - s(20), middle, false);
                if index + 1 < STEPS.len() {
                    Self::fill(
                        hdc,
                        RECT {
                            left: panel.left + s(18),
                            top: top + step_height - s(1).max(1),
                            right: panel.right - s(18),
                            bottom: top + step_height,
                        },
                        self.theme.edge,
                    );
                }
            }
        }

        let community = layout.community;
        self.gradient_card(hdc, community, s(9), ui(38, 35, 111), ui(25, 29, 75));
        self.card_outline(hdc, community, s(9), ui(79, 67, 185));
        let clip = RECT {
            left: community.left + s(16),
            top: community.top,
            right: community.right - s(12),
            bottom: community.bottom,
        };
        self.community_glyph(
            hdc,
            community.left + s(24),
            community.top + s(18),
            s(28),
            ui(137, 91, 242),
        );
        Self::fill(
            hdc,
            RECT {
                left: community.left + s(82),
                top: community.top + s(14),
                right: community.left + s(83),
                bottom: community.bottom - s(14),
            },
            ui(111, 86, 219),
        );
        let community_width = community.right - community.left;
        let narrow = community_width < s(520);
        let button_width = if narrow { s(138) } else { s(192) };
        let button = RECT {
            left: community.right - s(12) - button_width,
            top: community.top + s(12),
            right: community.right - s(12),
            bottom: community.bottom - s(12),
        };
        unsafe { SelectObject(hdc, self.brand_font) };
        self.label_mid(
            hdc,
            if narrow {
                "Community"
            } else {
                "Join the Community"
            },
            community.left + s(104),
            (community.top + community.bottom) / 2,
            self.theme.text,
            RECT {
                right: button.left - s(10),
                ..clip
            },
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        if community_width >= s(820) {
            self.label_mid(
                hdc,
                "Report issues, request features and read the source.",
                community.left + s(271),
                (community.top + community.bottom) / 2,
                ui(186, 200, 230),
                RECT {
                    right: button.left - s(12),
                    ..clip
                },
            );
        }
        self.panel_card(hdc, button, s(6), ui(91, 75, 222), ui(34, 38, 103));
        let label = if narrow {
            "Open  \u{2197}"
        } else {
            "Open Community  \u{2197}"
        };
        let label_width = self.text_width(hdc, label);
        self.label_mid(
            hdc,
            label,
            button.left + (button.right - button.left - label_width) / 2,
            (button.top + button.bottom) / 2,
            self.theme.text,
            button,
        );
    }

    fn paint_welcome_status(&self, hdc: HDC, rect: RECT, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        Self::fill(
            hdc,
            RECT {
                left: 0,
                top: layout.status_top,
                right: rect.right,
                bottom: rect.bottom,
            },
            self.theme.status_bg,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        let middle = (layout.status_top + rect.bottom) / 2;
        let clip = RECT {
            left: 0,
            top: layout.status_top,
            right: rect.right,
            bottom: rect.bottom,
        };
        let left_text = match &self.workspace_branch {
            Some(branch) => format!("Ready  \u{2022}  {branch}"),
            None => "Ready".into(),
        };
        self.label_mid(hdc, &left_text, s(16), middle, self.theme.muted, clip);
        let hint = "Ctrl+O file  \u{2022}  Ctrl+N new  \u{2022}  Ctrl+Shift+O folder";
        let width = self.text_width(hdc, hint);
        if rect.right >= width + s(180) {
            self.label_mid(
                hdc,
                hint,
                rect.right - s(16) - width,
                middle,
                self.theme.muted,
                clip,
            );
        }
    }

    // --- small vector glyphs, drawn to match the hand-drawn rail icons ---

    pub(super) fn stroke(&self, hdc: HDC, color: u32, draw: impl FnOnce(HDC)) {
        unsafe {
            let pen = CreatePen(PS_SOLID, self.scale(2).max(2), color);
            if pen.is_null() {
                return;
            }
            let previous_pen = SelectObject(hdc, pen);
            let previous_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
            draw(hdc);
            SelectObject(hdc, previous_brush);
            SelectObject(hdc, previous_pen);
            DeleteObject(pen);
        }
    }

    fn welcome_lightning_bolt(&self, hdc: HDC, bolt: &[POINT; 6]) {
        let channel = |color: u32, shift: u32| (((color >> shift) & 0xff) as u16) << 8;
        let colors = [
            ui(105, 125, 249),
            ui(180, 69, 235),
            ui(125, 78, 241),
            ui(18, 207, 240),
            ui(43, 182, 247),
            ui(137, 87, 245),
        ];
        let vertices: [TRIVERTEX; 6] = std::array::from_fn(|index| TRIVERTEX {
            x: bolt[index].x,
            y: bolt[index].y,
            Red: channel(colors[index], 0),
            Green: channel(colors[index], 8),
            Blue: channel(colors[index], 16),
            Alpha: 0,
        });
        let mesh = [
            GRADIENT_TRIANGLE {
                Vertex1: 5,
                Vertex2: 0,
                Vertex3: 1,
            },
            GRADIENT_TRIANGLE {
                Vertex1: 5,
                Vertex2: 1,
                Vertex3: 2,
            },
            GRADIENT_TRIANGLE {
                Vertex1: 5,
                Vertex2: 2,
                Vertex3: 3,
            },
            GRADIENT_TRIANGLE {
                Vertex1: 5,
                Vertex2: 3,
                Vertex3: 4,
            },
        ];
        unsafe {
            GradientFill(
                hdc,
                vertices.as_ptr(),
                vertices.len() as u32,
                mesh.as_ptr().cast(),
                mesh.len() as u32,
                GRADIENT_FILL_TRIANGLE,
            );

            let outline = CreatePen(PS_SOLID, self.scale(1).max(1), ui(91, 103, 225));
            if !outline.is_null() {
                let previous_pen = SelectObject(hdc, outline);
                let previous_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
                Polygon(hdc, bolt.as_ptr(), bolt.len() as i32);
                SelectObject(hdc, previous_brush);
                SelectObject(hdc, previous_pen);
                DeleteObject(outline);
            }
        }
    }

    fn home_glyph(&self, hdc: HDC, x: i32, y: i32, size: i32, color: u32) {
        self.stroke(hdc, color, |hdc| unsafe {
            let points = [
                POINT { x, y: y + size / 2 },
                POINT { x: x + size / 2, y },
                POINT {
                    x: x + size,
                    y: y + size / 2,
                },
            ];
            Polyline(hdc, points.as_ptr(), points.len() as i32);
            Rectangle(hdc, x + size / 6, y + size / 2, x + size * 5 / 6, y + size);
        });
    }

    fn prompt_glyph(&self, hdc: HDC, x: i32, y: i32, size: i32, color: u32) {
        self.stroke(hdc, color, |hdc| unsafe {
            MoveToEx(hdc, x + size / 5, y + size / 4, null_mut());
            LineTo(hdc, x + size / 2, y + size / 2);
            LineTo(hdc, x + size / 5, y + size * 3 / 4);
            MoveToEx(hdc, x + size * 9 / 16, y + size * 3 / 4, null_mut());
            LineTo(hdc, x + size, y + size * 3 / 4);
        });
    }

    fn clock_glyph(&self, hdc: HDC, x: i32, y: i32, size: i32, color: u32) {
        self.stroke(hdc, color, |hdc| unsafe {
            Ellipse(hdc, x, y, x + size, y + size);
            MoveToEx(hdc, x + size / 2, y + size / 4, null_mut());
            LineTo(hdc, x + size / 2, y + size / 2);
            LineTo(hdc, x + size * 3 / 4, y + size / 2);
        });
    }

    fn ring_glyph(&self, hdc: HDC, x: i32, y: i32, size: i32, color: u32) {
        self.stroke(hdc, color, |hdc| unsafe {
            Ellipse(hdc, x, y, x + size, y + size);
        });
    }

    fn bulb_glyph(&self, hdc: HDC, x: i32, y: i32, size: i32, color: u32) {
        self.stroke(hdc, color, |hdc| unsafe {
            Ellipse(hdc, x + size / 5, y, x + size * 4 / 5, y + size * 3 / 5);
            MoveToEx(hdc, x + size * 2 / 5, y + size * 3 / 4, null_mut());
            LineTo(hdc, x + size * 3 / 5, y + size * 3 / 4);
            MoveToEx(hdc, x + size * 2 / 5, y + size, null_mut());
            LineTo(hdc, x + size * 3 / 5, y + size);
        });
    }

    fn welcome_folder_glyph(&self, hdc: HDC, x: i32, y: i32, size: i32, color: u32) {
        let u = |value: i32| value * size / 16;
        let points = [
            POINT {
                x: x + u(1),
                y: y + u(3),
            },
            POINT {
                x: x + u(6),
                y: y + u(3),
            },
            POINT {
                x: x + u(8),
                y: y + u(5),
            },
            POINT {
                x: x + u(15),
                y: y + u(5),
            },
            POINT {
                x: x + u(15),
                y: y + u(14),
            },
            POINT {
                x: x + u(1),
                y: y + u(14),
            },
        ];
        unsafe {
            let brush = CreateSolidBrush(color);
            if brush.is_null() {
                let _ = self
                    .icons
                    .draw_generic(hdc, GenericIcon::Folder, x, y, size);
                return;
            }
            let previous_brush = SelectObject(hdc, brush);
            let previous_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
            Polygon(hdc, points.as_ptr(), points.len() as i32);
            SelectObject(hdc, previous_pen);
            SelectObject(hdc, previous_brush);
            DeleteObject(brush);
        }
    }

    fn welcome_folder_outline(&self, hdc: HDC, x: i32, y: i32, size: i32, color: u32) {
        let u = |value: i32| value * size / 16;
        self.stroke(hdc, color, |hdc| unsafe {
            let points = [
                POINT {
                    x: x + u(1),
                    y: y + u(3),
                },
                POINT {
                    x: x + u(6),
                    y: y + u(3),
                },
                POINT {
                    x: x + u(8),
                    y: y + u(5),
                },
                POINT {
                    x: x + u(15),
                    y: y + u(5),
                },
                POINT {
                    x: x + u(15),
                    y: y + u(14),
                },
                POINT {
                    x: x + u(1),
                    y: y + u(14),
                },
                POINT {
                    x: x + u(1),
                    y: y + u(3),
                },
            ];
            Polyline(hdc, points.as_ptr(), points.len() as i32);
        });
    }

    fn community_glyph(&self, hdc: HDC, x: i32, y: i32, size: i32, color: u32) {
        self.stroke(hdc, color, |hdc| unsafe {
            let head = size / 4;
            Ellipse(
                hdc,
                x + size / 2 - head / 2,
                y,
                x + size / 2 + head / 2,
                y + head,
            );
            Ellipse(
                hdc,
                x + size / 10,
                y + size / 5,
                x + size / 10 + head,
                y + size / 5 + head,
            );
            Ellipse(
                hdc,
                x + size - size / 10 - head,
                y + size / 5,
                x + size - size / 10,
                y + size / 5 + head,
            );
            Arc(
                hdc,
                x + size / 4,
                y + size / 3,
                x + size * 3 / 4,
                y + size,
                x + size * 3 / 4,
                y + size * 3 / 4,
                x + size / 4,
                y + size * 3 / 4,
            );
            Arc(
                hdc,
                x,
                y + size / 2,
                x + size / 2,
                y + size,
                x + size / 2,
                y + size * 4 / 5,
                x,
                y + size * 4 / 5,
            );
            Arc(
                hdc,
                x + size / 2,
                y + size / 2,
                x + size,
                y + size,
                x + size,
                y + size * 4 / 5,
                x + size / 2,
                y + size * 4 / 5,
            );
        });
    }
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
}
