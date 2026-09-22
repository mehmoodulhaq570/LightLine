use super::super::*;

// Welcome screen geometry, in logical pixels (everything goes through scale()).
const NAV_WIDTH: i32 = 232;
const ASIDE_WIDTH: i32 = 330;
const GUTTER: i32 = 28;
const MIN_CENTER: i32 = 520;
const NAV_FIRST_ROW: i32 = 92;
const NAV_ROW_H: i32 = 42;
const HERO_TOP: i32 = 46;
const CARD_H: i32 = 148;
const CARD_GAP: i32 = 14;
const PANEL_HEADER_H: i32 = 52;
const RECENT_ROW_H: i32 = 52;
const QUICK_ROW_H: i32 = 42;
const STEP_ROW_H: i32 = 60;
const COMMUNITY_H: i32 = 104;

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

const CARDS: [(&str, &str, WelcomeAction); 4] = [
    (
        "Open Project",
        "Start with your workspace",
        WelcomeAction::OpenFolder,
    ),
    ("Write Code", "Fast, focused editing", WelcomeAction::NewFile),
    (
        "Run & Debug",
        "Find issues, fix faster",
        WelcomeAction::RunDebug,
    ),
    (
        "Terminal",
        "Run commands in place",
        WelcomeAction::Terminal,
    ),
];

// (gradient top, gradient bottom, icon badge) per card.
const CARD_COLORS: [(u32, u32, u32); 4] = [
    (rgb(54, 38, 128), rgb(30, 26, 74), rgb(124, 92, 246)),
    (rgb(26, 58, 140), rgb(17, 34, 84), rgb(59, 130, 246)),
    (rgb(14, 92, 92), rgb(9, 52, 58), rgb(45, 212, 191)),
    (rgb(104, 36, 128), rgb(58, 24, 82), rgb(217, 70, 239)),
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
    nav_right: i32,
    status_top: i32,
    content_left: i32,
    content_right: i32,
    hero_top: i32,
    cards: Vec<RECT>,
    recent_panel: RECT,
    recent_rows: usize,
    quick_panel: RECT,
    quick_rows: usize,
    open_folder: RECT,
    steps_panel: Option<RECT>,
    community: Option<RECT>,
}

impl App {
    pub(in crate::windows_app) fn welcome_layout(&self, rect: RECT) -> WelcomeLayout {
        let s = |value: i32| self.scale(value);
        let mut targets = Vec::new();
        let status_top = rect.bottom - s(STATUS);
        let nav_right = s(NAV_WIDTH).min((rect.right / 3).max(s(64)));
        let gutter = s(GUTTER);

        let content_left = nav_right + gutter;
        let aside_left = rect.right - gutter - s(ASIDE_WIDTH);
        // The aside only earns its space while the centre column can breathe.
        let show_aside = aside_left - gutter - content_left >= s(MIN_CENTER);
        let content_right = if show_aside {
            aside_left - gutter
        } else {
            rect.right - gutter
        };

        for (index, (_, _, action)) in NAV_ITEMS.iter().enumerate() {
            // Row 0 is the (already active) Welcome entry, so items start at 1.
            let top = s(NAV_FIRST_ROW) + (index as i32 + 1) * s(NAV_ROW_H);
            targets.push((
                RECT {
                    left: s(10),
                    top,
                    right: nav_right - s(10),
                    bottom: top + s(NAV_ROW_H) - s(4),
                },
                *action,
            ));
        }

        let open_folder = RECT {
            left: s(18),
            top: s(NAV_FIRST_ROW) + s(NAV_ROW_H) * 8 + s(52),
            right: nav_right - s(18),
            bottom: s(NAV_FIRST_ROW) + s(NAV_ROW_H) * 8 + s(92),
        };
        targets.push((open_folder, WelcomeAction::OpenFolder));

        let hero_top = s(HERO_TOP);
        let cards_top = hero_top + s(190);
        let card_gap = s(CARD_GAP);
        let card_width = (content_right - content_left - card_gap * 3) / 4;
        let mut cards = Vec::with_capacity(4);
        for (index, (_, _, action)) in CARDS.iter().enumerate() {
            let left = content_left + index as i32 * (card_width + card_gap);
            let bounds = RECT {
                left,
                top: cards_top,
                right: left + card_width,
                bottom: cards_top + s(CARD_H),
            };
            cards.push(bounds);
            targets.push((bounds, *action));
        }

        let panels_top = cards_top + s(CARD_H) + s(26);
        let available = (status_top - s(22) - panels_top).max(s(120));
        let body = (available - s(PANEL_HEADER_H)).max(0);
        let recent_rows = self
            .recent
            .len()
            .min(4)
            .min((body / s(RECENT_ROW_H)).max(0) as usize);
        let quick_rows = QUICK.len().min((body / s(QUICK_ROW_H)).max(0) as usize);
        let panel_height = (s(PANEL_HEADER_H)
            + (recent_rows as i32 * s(RECENT_ROW_H)).max(quick_rows as i32 * s(QUICK_ROW_H))
            + s(10))
        .min(available);
        let recent_width = (content_right - content_left - card_gap) * 58 / 100;
        let recent_panel = RECT {
            left: content_left,
            top: panels_top,
            right: content_left + recent_width,
            bottom: panels_top + panel_height,
        };
        let quick_panel = RECT {
            left: recent_panel.right + card_gap,
            top: panels_top,
            right: content_right,
            bottom: panels_top + panel_height,
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

        let (steps_panel, community) = if show_aside {
            let steps = RECT {
                left: aside_left,
                top: hero_top,
                right: rect.right - gutter,
                bottom: hero_top + s(PANEL_HEADER_H) + s(STEP_ROW_H) * STEPS.len() as i32 + s(10),
            };
            for (index, (_, _, action)) in STEPS.iter().enumerate() {
                let top = steps.top + s(PANEL_HEADER_H) + index as i32 * s(STEP_ROW_H);
                targets.push((
                    RECT {
                        left: steps.left + s(8),
                        top,
                        right: steps.right - s(8),
                        bottom: top + s(STEP_ROW_H),
                    },
                    *action,
                ));
            }
            // Anchored to the bottom of the panel row rather than stacked
            // directly under the steps, so the column reads as a full column
            // instead of trailing off into dead space.
            let community_bottom = (panels_top + panel_height)
                .max(steps.bottom + s(16) + s(COMMUNITY_H));
            let community = RECT {
                left: aside_left,
                top: community_bottom - s(COMMUNITY_H),
                right: rect.right - gutter,
                bottom: community_bottom,
            };
            targets.push((community, WelcomeAction::Community));
            (Some(steps), Some(community))
        } else {
            (None, None)
        };

        WelcomeLayout {
            targets,
            nav_right,
            status_top,
            content_left,
            content_right,
            hero_top,
            cards,
            recent_panel,
            recent_rows,
            quick_panel,
            quick_rows,
            open_folder,
            steps_panel,
            community,
        }
    }

    pub(in crate::windows_app) fn paint_welcome(&self, hdc: HDC, rect: RECT) {
        let layout = self.welcome_layout(rect);
        Self::fill(hdc, rect, self.theme.editor_bg);
        self.paint_welcome_nav(hdc, &layout);
        self.paint_welcome_hero(hdc, &layout);
        self.paint_welcome_cards(hdc, &layout);
        self.paint_welcome_recent(hdc, &layout);
        self.paint_welcome_quick(hdc, &layout);
        self.paint_welcome_aside(hdc, &layout);
        self.paint_welcome_status(hdc, rect, &layout);
    }

    fn paint_welcome_nav(&self, hdc: HDC, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        let clip = RECT {
            left: 0,
            top: 0,
            right: layout.nav_right,
            bottom: layout.status_top,
        };
        Self::fill(hdc, clip, self.theme.rail_bg);
        Self::fill(
            hdc,
            RECT {
                left: layout.nav_right - s(1).max(1),
                top: 0,
                right: layout.nav_right,
                bottom: layout.status_top,
            },
            self.theme.edge,
        );
        unsafe {
            DrawIconEx(
                hdc,
                s(20),
                s(22),
                self.brand_icon,
                s(28),
                s(28),
                0,
                null_mut(),
                DI_NORMAL,
            );
            SelectObject(hdc, self.brand_font);
        }
        Self::label(hdc, "Lightline", s(56), s(26), self.theme.text, clip);
        let badge_left = s(56) + self.text_width(hdc, "Lightline") + s(10);
        Self::rounded_fill(
            hdc,
            RECT {
                left: badge_left,
                top: s(27),
                right: badge_left + s(34),
                bottom: s(45),
            },
            s(5),
            rgb(78, 56, 176),
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        Self::label(hdc, "IDE", badge_left + s(8), s(29), self.theme.text, clip);

        // "Welcome" is where we already are, so it renders active and inert.
        let welcome_row = RECT {
            left: s(10),
            top: s(NAV_FIRST_ROW),
            right: layout.nav_right - s(10),
            bottom: s(NAV_FIRST_ROW) + s(NAV_ROW_H) - s(4),
        };
        Self::rounded_fill(hdc, welcome_row, s(8), rgb(40, 52, 122));
        self.home_glyph(hdc, s(24), welcome_row.top + s(10), s(16), self.theme.text);
        self.label_mid(
            hdc,
            "Welcome",
            s(56),
            (welcome_row.top + welcome_row.bottom) / 2,
            self.theme.text,
            clip,
        );

        for (index, (label, icon, _)) in NAV_ITEMS.iter().enumerate() {
            let top = s(NAV_FIRST_ROW) + (index as i32 + 1) * s(NAV_ROW_H);
            let middle = top + (s(NAV_ROW_H) - s(4)) / 2;
            self.rail_icon(hdc, *icon, s(22), top + s(9), self.theme.muted);
            self.label_mid(hdc, label, s(56), middle, self.theme.muted, clip);
        }

        let divider = layout.open_folder.top - s(76);
        Self::fill(
            hdc,
            RECT {
                left: s(18),
                top: divider,
                right: layout.nav_right - s(18),
                bottom: divider + s(1).max(1),
            },
            self.theme.edge,
        );
        Self::label(hdc, "WORKSPACE", s(18), divider + s(18), self.theme.muted, clip);
        let workspace = match &self.workspace_root {
            Some(root) => root.file_name().unwrap_or_default().to_string_lossy(),
            None => "No project open".into(),
        };
        self.label_ellipsis(
            hdc,
            &workspace,
            s(18),
            divider + s(42),
            if self.workspace_root.is_some() {
                self.theme.text
            } else {
                self.theme.muted
            },
            RECT {
                left: s(18),
                top: divider + s(42),
                right: layout.nav_right - s(14),
                bottom: divider + s(66),
            },
        );
        self.panel_card(hdc, layout.open_folder, s(8), self.theme.edge, self.theme.active_bg);
        self.icons.draw_generic(
            hdc,
            GenericIcon::Folder,
            layout.open_folder.left + s(14),
            layout.open_folder.top + s(11),
            s(16),
        );
        self.label_mid(
            hdc,
            "Open Folder",
            layout.open_folder.left + s(40),
            (layout.open_folder.top + layout.open_folder.bottom) / 2,
            self.theme.text,
            clip,
        );
    }

    fn paint_welcome_hero(&self, hdc: HDC, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        let left = layout.content_left;
        let clip = RECT {
            left,
            top: 0,
            right: layout.content_right,
            bottom: layout.status_top,
        };
        unsafe {
            SelectObject(hdc, self.ui_font);
            SetTextCharacterExtra(hdc, s(2));
        }
        Self::label(hdc, "WELCOME TO", left, layout.hero_top, self.theme.violet, clip);
        unsafe { SetTextCharacterExtra(hdc, 0) };

        let logo_top = layout.hero_top + s(26);
        unsafe {
            DrawIconEx(
                hdc,
                left,
                logo_top,
                self.hero_icon,
                s(54),
                s(54),
                0,
                null_mut(),
                DI_NORMAL,
            );
            SelectObject(hdc, self.hero_font);
        }
        let word_left = left + s(68);
        self.label_mid(
            hdc,
            "Lightline",
            word_left,
            logo_top + s(27),
            self.theme.text,
            clip,
        );
        let badge_left = word_left + self.text_width(hdc, "Lightline") + s(16);
        Self::rounded_fill(
            hdc,
            RECT {
                left: badge_left,
                top: logo_top + s(12),
                right: badge_left + s(58),
                bottom: logo_top + s(42),
            },
            s(7),
            rgb(78, 56, 176),
        );
        unsafe { SelectObject(hdc, self.title_font) };
        self.label_mid(
            hdc,
            "IDE",
            badge_left + s(12),
            logo_top + s(27),
            self.theme.text,
            clip,
        );
        Self::label(
            hdc,
            "Build Faster, Think Smarter",
            left,
            logo_top + s(66),
            self.theme.text,
            clip,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        Self::label(
            hdc,
            "A modern, AI-powered IDE for developers.",
            left,
            logo_top + s(110),
            self.theme.muted,
            clip,
        );
        Self::label(
            hdc,
            "Fast to start, focused to work in.",
            left,
            logo_top + s(132),
            self.theme.muted,
            clip,
        );
    }

    fn paint_welcome_cards(&self, hdc: HDC, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        for (index, bounds) in layout.cards.iter().enumerate() {
            let (title, subtitle, _) = CARDS[index];
            let (top_color, bottom_color, badge_color) = CARD_COLORS[index];
            self.gradient_card(hdc, *bounds, s(12), top_color, bottom_color);
            let clip = RECT {
                left: bounds.left + s(14),
                top: bounds.top,
                right: bounds.right - s(10),
                bottom: bounds.bottom,
            };
            let badge = RECT {
                left: bounds.left + s(16),
                top: bounds.top + s(16),
                right: bounds.left + s(56),
                bottom: bounds.top + s(56),
            };
            Self::rounded_fill(hdc, badge, s(9), badge_color);
            match index {
                0 => {
                    self.icons.draw_generic(
                        hdc,
                        GenericIcon::FolderOpen,
                        badge.left + s(11),
                        badge.top + s(11),
                        s(18),
                    );
                }
                1 => {
                    self.icons.draw_generic(
                        hdc,
                        GenericIcon::File,
                        badge.left + s(11),
                        badge.top + s(11),
                        s(18),
                    );
                }
                2 => self.rail_icon(hdc, 3, badge.left + s(11), badge.top + s(11), self.theme.text),
                _ => self.prompt_glyph(hdc, badge.left + s(10), badge.top + s(10), s(20), self.theme.text),
            }
            unsafe { SelectObject(hdc, self.brand_font) };
            Self::label(
                hdc,
                title,
                bounds.left + s(16),
                bounds.top + s(70),
                self.theme.text,
                clip,
            );
            unsafe { SelectObject(hdc, self.ui_font) };
            self.label_ellipsis(
                hdc,
                subtitle,
                bounds.left + s(16),
                bounds.top + s(94),
                rgb(186, 200, 230),
                clip,
            );
            Self::label(
                hdc,
                "\u{2192}",
                bounds.left + s(16),
                bounds.bottom - s(30),
                self.theme.text,
                clip,
            );
        }
    }

    fn paint_welcome_recent(&self, hdc: HDC, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        let panel = layout.recent_panel;
        self.panel_card(hdc, panel, s(12), WELCOME_CARD_EDGE, CARD_BG);
        let clip = RECT {
            left: panel.left + s(14),
            top: panel.top,
            right: panel.right - s(12),
            bottom: panel.bottom,
        };
        unsafe { SelectObject(hdc, self.brand_font) };
        self.clock_glyph(hdc, panel.left + s(16), panel.top + s(17), s(17), self.theme.blue);
        self.label_mid(
            hdc,
            "Recent Projects",
            panel.left + s(42),
            panel.top + s(26),
            self.theme.text,
            clip,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
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
                bottom: top + s(RECENT_ROW_H) - s(4),
            };
            Self::rounded_fill(hdc, row, s(8), self.theme.active_bg);
            self.icons
                .draw_generic(hdc, GenericIcon::Folder, row.left + s(12), top + s(15), s(17));
            let text_left = row.left + s(40);
            let row_clip = RECT {
                left: text_left,
                top: row.top,
                right: row.right - s(10),
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
        }
    }

    fn paint_welcome_quick(&self, hdc: HDC, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        let panel = layout.quick_panel;
        self.panel_card(hdc, panel, s(12), WELCOME_CARD_EDGE, CARD_BG);
        let clip = RECT {
            left: panel.left + s(14),
            top: panel.top,
            right: panel.right - s(12),
            bottom: panel.bottom,
        };
        unsafe { SelectObject(hdc, self.brand_font) };
        self.rail_icon(hdc, 5, panel.left + s(14), panel.top + s(17), self.theme.violet);
        self.label_mid(
            hdc,
            "Quick Actions",
            panel.left + s(42),
            panel.top + s(26),
            self.theme.text,
            clip,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        for (index, (label, shortcut, _)) in QUICK.iter().take(layout.quick_rows).enumerate() {
            let top = panel.top + s(PANEL_HEADER_H) + index as i32 * s(QUICK_ROW_H);
            let middle = top + s(QUICK_ROW_H) / 2;
            let chip_width = self.text_width(hdc, shortcut) + s(16);
            let chip = RECT {
                left: panel.right - s(14) - chip_width,
                top: middle - s(11),
                right: panel.right - s(14),
                bottom: middle + s(11),
            };
            Self::rounded_fill(hdc, chip, s(5), CHIP_BG);
            self.label_mid(hdc, shortcut, chip.left + s(8), middle, self.theme.muted, clip);
            self.label_mid(hdc, label, panel.left + s(42), middle, self.theme.text, clip);
        }
    }

    fn paint_welcome_aside(&self, hdc: HDC, layout: &WelcomeLayout) {
        let s = |value: i32| self.scale(value);
        let Some(panel) = layout.steps_panel else {
            return;
        };
        self.panel_card(hdc, panel, s(12), WELCOME_CARD_EDGE, CARD_BG);
        let clip = RECT {
            left: panel.left + s(14),
            top: panel.top,
            right: panel.right - s(12),
            bottom: panel.bottom,
        };
        unsafe { SelectObject(hdc, self.brand_font) };
        self.bulb_glyph(hdc, panel.left + s(16), panel.top + s(16), s(18), rgb(245, 205, 110));
        self.label_mid(
            hdc,
            "Getting Started",
            panel.left + s(44),
            panel.top + s(26),
            self.theme.text,
            clip,
        );
        for (index, (title, subtitle, _)) in STEPS.iter().enumerate() {
            let top = panel.top + s(PANEL_HEADER_H) + index as i32 * s(STEP_ROW_H);
            let middle = top + s(STEP_ROW_H) / 2;
            self.ring_glyph(hdc, panel.left + s(18), middle - s(11), s(22), self.theme.violet);
            let row_clip = RECT {
                left: panel.left + s(52),
                top,
                right: panel.right - s(26),
                bottom: top + s(STEP_ROW_H),
            };
            unsafe { SelectObject(hdc, self.brand_font) };
            self.label_ellipsis(hdc, title, panel.left + s(52), middle - s(19), self.theme.text, row_clip);
            unsafe { SelectObject(hdc, self.ui_font) };
            self.label_ellipsis(hdc, subtitle, panel.left + s(52), middle + s(2), self.theme.muted, row_clip);
            self.chevron(hdc, panel.right - s(20), middle, false);
        }

        let Some(community) = layout.community else {
            return;
        };
        self.gradient_card(
            hdc,
            community,
            s(12),
            rgb(46, 40, 132),
            rgb(28, 30, 78),
        );
        let clip = RECT {
            left: community.left + s(16),
            top: community.top,
            right: community.right - s(12),
            bottom: community.bottom,
        };
        let badge = RECT {
            left: community.left + s(16),
            top: community.top + s(20),
            right: community.left + s(64),
            bottom: community.top + s(68),
        };
        Self::rounded_fill(hdc, badge, s(10), rgb(96, 82, 220));
        self.rail_icon(hdc, 2, badge.left + s(15), badge.top + s(15), self.theme.text);
        unsafe { SelectObject(hdc, self.brand_font) };
        Self::label(
            hdc,
            "Join the Community",
            community.left + s(78),
            community.top + s(22),
            self.theme.text,
            clip,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        Self::label(
            hdc,
            "Report issues, request features",
            community.left + s(78),
            community.top + s(46),
            rgb(186, 200, 230),
            clip,
        );
        Self::label(
            hdc,
            "and read the source.",
            community.left + s(78),
            community.top + s(66),
            rgb(186, 200, 230),
            clip,
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
            Some(branch) => format!("{}  \u{2022}  {}", self.status, branch),
            None => self.status.clone(),
        };
        self.label_mid(hdc, &left_text, s(16), middle, self.theme.muted, clip);
        let hint = "Ctrl+O file  \u{2022}  Ctrl+N new  \u{2022}  Ctrl+Shift+O folder";
        let width = self.text_width(hdc, hint);
        self.label_mid(hdc, hint, rect.right - s(16) - width, middle, self.theme.muted, clip);
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

    fn home_glyph(&self, hdc: HDC, x: i32, y: i32, size: i32, color: u32) {
        self.stroke(hdc, color, |hdc| unsafe {
            let points = [
                POINT {
                    x,
                    y: y + size / 2,
                },
                POINT {
                    x: x + size / 2,
                    y,
                },
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
}

impl WelcomeLayout {
    pub(in crate::windows_app) fn hit(&self, x: i32, y: i32) -> Option<WelcomeAction> {
        self.targets
            .iter()
            .find(|(rect, _)| x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom)
            .map(|(_, action)| *action)
    }
}
