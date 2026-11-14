// The Extensions panel's layout, rows, clicks and hover
// (design/extensions-design.html). Painting is in render/extensions.rs; both
// use `extensions_layout` and `extension_rows`, so what's drawn is clicked.

use super::app::{Extension, ExtensionCategory};
use super::*;

// In logical pixels from the top of the panel's 40 px header.
const SEARCH_TOP: i32 = 48;
const SEARCH_HEIGHT: i32 = 28;
const CHIP_HEIGHT: i32 = 22;
pub(super) const EXTENSION_ROW: i32 = 62;
pub(super) const LIST_ROW: i32 = EXPLORER_ROW;
pub(super) const EXTENSION_INDENT: i32 = 20;

/// The panel's buttons outside the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExtensionsButton {
    /// Refresh (re-read the registry) and ··· in the header.
    Refresh,
    More,
    /// The search box's right-hand button: ✕ with a query, the funnel without.
    SearchAction,
    /// A filter chip, by its index in `ExtensionsFilter::PILLS`.
    Chip(usize),
}

/// What the mouse is over in the panel, for hover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExtensionsHot {
    Button(ExtensionsButton),
    Row(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExtensionsSection {
    Installed,
    Featured,
    Marketplace,
    InUse,
}

impl ExtensionsSection {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Installed => "INSTALLED",
            Self::Featured => "FEATURED",
            Self::Marketplace => "MARKETPLACE",
            Self::InUse => "IN USE",
        }
    }

    /// Its slot in `App::extension_sections_collapsed`.
    pub(super) fn slot(self) -> usize {
        match self {
            Self::Installed => 0,
            Self::Featured | Self::Marketplace => 1,
            Self::InUse => 2,
        }
    }
}

/// One line of the panel's list: a 62 px extension or a 22 px line.
pub(super) enum ExtensionRow {
    Header {
        section: ExtensionsSection,
        count: usize,
        /// Right-aligned and dimmed: "3 results", "Themes".
        note: String,
    },
    /// An extension, by its index in `App::extensions`.
    Extension(usize),
    /// What's in use: 0 formatting, 1 file icons, 2 color theme.
    InUse(usize),
    /// A muted line of explanation.
    Note(String),
}

impl ExtensionRow {
    pub(super) fn height(&self) -> i32 {
        match self {
            Self::Extension(_) => EXTENSION_ROW,
            _ => LIST_ROW,
        }
    }
}

pub(super) struct ExtensionsLayout {
    pub refresh: RECT,
    pub more: RECT,
    pub search: RECT,
    pub search_action: RECT,
    pub chips: Vec<RECT>,
    pub divider: i32,
    pub list_top: i32,
}

pub(super) enum ExtensionsHit {
    Button(ExtensionsButton),
    Search,
    Header(ExtensionsSection),
    /// Install, Uninstall or (Prettier) turn off.
    Action(usize),
    ColorTheme,
    Nothing,
}

fn contains(rect: &RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

/// The extensions LightLine knows and recommends.
const FEATURED: [&str; 3] = ["prettier", "material-icons", "dracula"];

impl App {
    fn searching_extensions(&self) -> bool {
        !self.extensions_query.trim().is_empty()
    }

    pub(super) fn extensions_layout(
        &self,
        left: i32,
        right: i32,
        hdc: Option<HDC>,
    ) -> ExtensionsLayout {
        let s = |value: i32| self.scale(value);
        let header = |offset: i32| RECT {
            left: right - s(offset),
            top: s(SIDE_SHIFT),
            right: right - s(offset) + s(22),
            bottom: s(40),
        };
        let search = RECT {
            left: left + s(12),
            top: s(SEARCH_TOP),
            right: right - s(12),
            bottom: s(SEARCH_TOP + SEARCH_HEIGHT),
        };
        let search_action = RECT {
            left: search.right - s(25),
            top: search.top + s(3),
            right: search.right - s(3),
            bottom: search.bottom - s(3),
        };
        let mut chips = Vec::new();
        let mut bottom = search.bottom;
        if !self.searching_extensions() {
            let top = search.bottom + s(6);
            let mut x = search.left;
            let widths = self.with_ui_dc(hdc, |hdc| {
                ExtensionsFilter::PILLS
                    .iter()
                    .map(|(_, label, _)| self.text_width(hdc, label))
                    .collect::<Vec<_>>()
            });
            for width in widths {
                let rect = RECT {
                    left: x,
                    top,
                    right: x + width + s(18),
                    bottom: top + s(CHIP_HEIGHT),
                };
                x = rect.right + s(4);
                chips.push(rect);
            }
            bottom = top + s(CHIP_HEIGHT);
        }
        ExtensionsLayout {
            refresh: header(65),
            more: header(36),
            search,
            search_action,
            chips,
            divider: bottom + s(4),
            list_top: bottom + s(7),
        }
    }

    /// Runs `f` with `hdc`, or with a screen DC holding the UI font when
    /// there's none (hit testing outside a paint).
    pub(super) fn with_ui_dc<T>(&self, hdc: Option<HDC>, f: impl FnOnce(HDC) -> T) -> T {
        if let Some(hdc) = hdc {
            return f(hdc);
        }
        unsafe {
            let hdc = GetDC(null_mut());
            let old = SelectObject(hdc, self.ui_font);
            let result = f(hdc);
            SelectObject(hdc, old);
            ReleaseDC(null_mut(), hdc);
            result
        }
    }

    fn extension_matches_filter(&self, ext: &Extension) -> bool {
        match self.extensions_filter {
            ExtensionsFilter::Featured => true,
            ExtensionsFilter::Themes => matches!(
                ext.category(),
                ExtensionCategory::IconTheme | ExtensionCategory::ColorTheme
            ),
            ExtensionsFilter::Formatters => ext.category() == ExtensionCategory::Formatter,
        }
    }

    /// The list model, built fresh so paint and hit-testing agree.
    pub(super) fn extension_rows(&self) -> Vec<ExtensionRow> {
        let mut rows = Vec::new();
        let collapsed =
            |section: ExtensionsSection| self.extension_sections_collapsed[section.slot()];
        if self.searching_extensions() {
            let matches: Vec<usize> = self
                .extensions
                .iter()
                .enumerate()
                .filter(|(_, ext)| self.extension_matches_query(ext))
                .map(|(index, _)| index)
                .collect();
            let note = match matches.len() {
                1 => "1 result".to_owned(),
                count => format!("{count} results"),
            };
            rows.push(ExtensionRow::Header {
                section: ExtensionsSection::Marketplace,
                count: matches.len(),
                note,
            });
            if !collapsed(ExtensionsSection::Marketplace) {
                if self.zed_registry_loading {
                    rows.push(ExtensionRow::Note(
                        "Searching the Zed registry\u{2026}".into(),
                    ));
                } else if matches.is_empty() {
                    rows.push(ExtensionRow::Note(format!(
                        "No extensions match \"{}\"",
                        self.extensions_query.trim()
                    )));
                }
                rows.extend(matches.into_iter().map(ExtensionRow::Extension));
                rows.push(ExtensionRow::Note(String::new()));
                rows.push(ExtensionRow::Note(
                    "LightLine installs color and icon themes,".into(),
                ));
                rows.push(ExtensionRow::Note(
                    "and languages with a Tree-sitter grammar.".into(),
                ));
            }
            return rows;
        }

        let note = match self.extensions_filter {
            ExtensionsFilter::Featured => String::new(),
            ExtensionsFilter::Themes => "Themes".to_owned(),
            ExtensionsFilter::Formatters => "Formatters".to_owned(),
        };
        let installed: Vec<usize> = self
            .extensions
            .iter()
            .enumerate()
            .filter(|(_, ext)| ext.installed && self.extension_matches_filter(ext))
            .map(|(index, _)| index)
            .collect();
        rows.push(ExtensionRow::Header {
            section: ExtensionsSection::Installed,
            count: installed.len(),
            note: note.clone(),
        });
        if !collapsed(ExtensionsSection::Installed) {
            if installed.is_empty() {
                rows.push(ExtensionRow::Note("Nothing installed yet.".into()));
            }
            rows.extend(installed.into_iter().map(ExtensionRow::Extension));
        }

        let featured: Vec<usize> = FEATURED
            .iter()
            .filter_map(|id| self.extensions.iter().position(|ext| ext.id == *id))
            .filter(|&index| {
                let ext = &self.extensions[index];
                !ext.installed && self.extension_matches_filter(ext)
            })
            .collect();
        rows.push(ExtensionRow::Header {
            section: ExtensionsSection::Featured,
            count: featured.len(),
            note,
        });
        if !collapsed(ExtensionsSection::Featured) {
            if featured.is_empty() {
                rows.push(ExtensionRow::Note(
                    "All of them are installed. Search for more.".into(),
                ));
            }
            rows.extend(featured.into_iter().map(ExtensionRow::Extension));
        }

        rows.push(ExtensionRow::Header {
            section: ExtensionsSection::InUse,
            count: 3,
            note: String::new(),
        });
        if !collapsed(ExtensionsSection::InUse) {
            rows.extend((0..3).map(ExtensionRow::InUse));
        }
        rows
    }

    /// Search matches, as `filtered_extensions` decided them.
    fn extension_matches_query(&self, ext: &Extension) -> bool {
        let q = self.extensions_query.trim().to_lowercase();
        let described = ext.category() != ExtensionCategory::Unknown;
        ext.id.to_lowercase().contains(&q)
            || ext.name.to_lowercase().contains(&q)
            || (described && ext.description.to_lowercase().contains(&q))
            || (described && ext.publisher.to_lowercase().contains(&q))
    }

    /// What an In Use row says: its name and what's in use.
    pub(super) fn in_use_row(&self, index: usize) -> (&'static str, String) {
        match index {
            0 if self.has_extension("prettier") && self.settings.prettier_enabled => {
                ("Formatting", "Prettier".into())
            }
            0 => ("Formatting", "Built-in formatter".into()),
            1 if self.has_extension("material-icons") => {
                ("File icons", "Material Icon Theme".into())
            }
            1 => ("File icons", "Built-in icons".into()),
            _ => ("Color theme", self.color_theme_name()),
        }
    }

    /// Each row on screen with its rectangle, in the panel's coordinates.
    pub(super) fn extension_rows_on_screen(
        &self,
        rows: &[ExtensionRow],
        left: i32,
        right: i32,
        list_top: i32,
    ) -> Vec<(usize, RECT)> {
        let mut top = list_top;
        // A list that shrank under the scroll position still shows its end.
        let first = self.extensions_first.min(rows.len().saturating_sub(1));
        rows.iter()
            .enumerate()
            .skip(first)
            .map(|(index, row)| {
                let rect = RECT {
                    left,
                    top,
                    right,
                    bottom: top + self.scale(row.height()),
                };
                top = rect.bottom;
                (index, rect)
            })
            .collect()
    }

    /// The extension row's button: Install, Uninstall, Turn Off, or the
    /// installing note, right-aligned on its third line.
    pub(super) fn extension_action(
        &self,
        hdc: HDC,
        rect: RECT,
        index: usize,
        hot: bool,
    ) -> (RECT, &'static str) {
        let ext = &self.extensions[index];
        let label = if ext.installing {
            if ext.id == "prettier" {
                "Checking\u{2026}"
            } else {
                "Installing\u{2026}"
            }
        } else if !ext.installed {
            "Install"
        } else if !hot {
            ""
        } else if ext.id == "prettier" {
            "Turn Off"
        } else {
            "Uninstall"
        };
        let width = if label.is_empty() {
            self.scale(20)
        } else {
            self.text_width(hdc, label) + self.scale(18)
        };
        // Level with the third line (publisher · kind), below the description.
        let bottom = rect.top + self.scale(59);
        let right = rect.right - self.scale(10);
        (
            RECT {
                left: right - width,
                top: bottom - self.scale(18),
                right,
                bottom,
            },
            label,
        )
    }

    pub(super) fn extensions_hit(&self, x: i32, y: i32) -> ExtensionsHit {
        let (left, right) = (self.scale(RAIL), self.sidebar_right());
        let layout = self.extensions_layout(left, right, None);
        if let Some(button) = self.extensions_button_at(&layout, x, y) {
            return ExtensionsHit::Button(button);
        }
        if contains(&layout.search, x, y) {
            return ExtensionsHit::Search;
        }
        if y < layout.list_top {
            return ExtensionsHit::Nothing;
        }
        let rows = self.extension_rows();
        let Some((index, rect)) = self
            .extension_rows_on_screen(&rows, left, right, layout.list_top)
            .into_iter()
            .find(|(_, rect)| y >= rect.top && y < rect.bottom)
        else {
            return ExtensionsHit::Nothing;
        };
        match &rows[index] {
            ExtensionRow::Header { section, .. } => ExtensionsHit::Header(*section),
            ExtensionRow::Extension(ext) => {
                let (button, _) =
                    self.with_ui_dc(None, |hdc| self.extension_action(hdc, rect, *ext, true));
                if contains(&button, x, y) {
                    ExtensionsHit::Action(*ext)
                } else {
                    ExtensionsHit::Nothing
                }
            }
            ExtensionRow::InUse(2) => ExtensionsHit::ColorTheme,
            _ => ExtensionsHit::Nothing,
        }
    }

    fn extensions_button_at(
        &self,
        layout: &ExtensionsLayout,
        x: i32,
        y: i32,
    ) -> Option<ExtensionsButton> {
        if contains(&layout.refresh, x, y) {
            return Some(ExtensionsButton::Refresh);
        }
        if contains(&layout.more, x, y) {
            return Some(ExtensionsButton::More);
        }
        if contains(&layout.search_action, x, y) {
            return Some(ExtensionsButton::SearchAction);
        }
        layout
            .chips
            .iter()
            .position(|rect| contains(rect, x, y))
            .map(ExtensionsButton::Chip)
    }

    /// What's under the mouse at (x, y) in the panel's coordinates.
    pub(super) fn extensions_hot_at(&self, x: i32, y: i32) -> Option<ExtensionsHot> {
        let (left, right) = (self.scale(RAIL), self.sidebar_right());
        let layout = self.extensions_layout(left, right, None);
        if let Some(button) = self.extensions_button_at(&layout, x, y) {
            return Some(ExtensionsHot::Button(button));
        }
        if y < layout.list_top {
            return None;
        }
        let rows = self.extension_rows();
        self.extension_rows_on_screen(&rows, left, right, layout.list_top)
            .into_iter()
            .find(|(_, rect)| y >= rect.top && y < rect.bottom)
            .filter(|(index, _)| {
                matches!(
                    rows[*index],
                    ExtensionRow::Extension(_) | ExtensionRow::InUse(2)
                )
            })
            .map(|(index, _)| ExtensionsHot::Row(index))
    }

    /// Where `hot` is, in the panel's coordinates.
    pub(super) fn extensions_hot_rect(&self, hot: ExtensionsHot) -> Option<RECT> {
        let (left, right) = (self.scale(RAIL), self.sidebar_right());
        let layout = self.extensions_layout(left, right, None);
        match hot {
            ExtensionsHot::Button(ExtensionsButton::Refresh) => Some(layout.refresh),
            ExtensionsHot::Button(ExtensionsButton::More) => Some(layout.more),
            ExtensionsHot::Button(ExtensionsButton::SearchAction) => Some(layout.search_action),
            ExtensionsHot::Button(ExtensionsButton::Chip(index)) => {
                layout.chips.get(index).copied()
            }
            ExtensionsHot::Row(index) => {
                let rows = self.extension_rows();
                self.extension_rows_on_screen(&rows, left, right, layout.list_top)
                    .into_iter()
                    .find(|(row, _)| *row == index)
                    .map(|(_, rect)| rect)
            }
        }
    }

    pub(super) fn extensions_hot_row(&self) -> Option<usize> {
        match self.hot {
            Some(Hot::Extensions(ExtensionsHot::Row(index))) => Some(index),
            _ => None,
        }
    }

    /// The list changed or scrolled under a still mouse.
    pub(super) fn forget_extensions_hot_row(&mut self, hwnd: HWND) {
        if self.extensions_hot_row().is_some() {
            self.clear_hot(hwnd);
        }
    }

    /// How many 22 px rows fit below the filters (for the wheel's bound).
    pub(super) fn extensions_visible_rows(&self, hwnd: HWND) -> usize {
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let layout = self.extensions_layout(self.scale(RAIL), self.sidebar_right(), None);
        let available =
            (client.bottom - self.scale(STATUS) - self.side_origin() - layout.list_top).max(0);
        (available / self.scale(EXTENSION_ROW).max(1)).max(1) as usize
    }

    /// A click in the panel (panel coordinates).
    pub(super) fn extensions_panel_click(&mut self, hwnd: HWND, x: i32, y: i32) {
        let hit = self.extensions_hit(x, y);
        if !matches!(
            hit,
            ExtensionsHit::Search | ExtensionsHit::Button(ExtensionsButton::SearchAction)
        ) {
            self.extensions_search_active = false;
        }
        self.terminal_focus = false;
        match hit {
            ExtensionsHit::Button(button) => self.extensions_button_pressed(hwnd, button),
            ExtensionsHit::Search => {
                self.extensions_search_active = true;
                self.search_input = false;
                self.commit_focus = false;
                self.panel_focus = true;
                self.ensure_zed_registry_loaded();
            }
            ExtensionsHit::Header(section) => {
                let slot = section.slot();
                self.extension_sections_collapsed[slot] = !self.extension_sections_collapsed[slot];
            }
            ExtensionsHit::Action(index) => {
                if let Some(id) = self.extensions.get(index).map(|ext| ext.id.clone()) {
                    self.toggle_extension(hwnd, &id);
                }
                self.forget_extensions_hot_row(hwnd);
            }
            ExtensionsHit::ColorTheme => {
                let layout = self.extensions_layout(self.scale(RAIL), self.sidebar_right(), None);
                let rows = self.extension_rows();
                let row = self
                    .extension_rows_on_screen(
                        &rows,
                        self.scale(RAIL),
                        self.sidebar_right(),
                        layout.list_top,
                    )
                    .into_iter()
                    .find(|(index, _)| matches!(rows[*index], ExtensionRow::InUse(2)))
                    .map(|(_, rect)| rect);
                if let Some(row) = row {
                    let x = row.left + self.scale(EXTENSION_INDENT);
                    self.show_color_theme_menu(hwnd, x, row.bottom + self.side_origin());
                }
            }
            ExtensionsHit::Nothing => {}
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    fn extensions_button_pressed(&mut self, hwnd: HWND, button: ExtensionsButton) {
        match button {
            ExtensionsButton::Refresh => self.check_extension_updates(),
            ExtensionsButton::More => {
                let layout = self.extensions_layout(self.scale(RAIL), self.sidebar_right(), None);
                let origin = self.side_origin();
                self.toggle_extensions_menu(
                    hwnd,
                    RECT {
                        top: layout.more.top + origin,
                        bottom: layout.more.bottom + origin,
                        ..layout.more
                    },
                );
            }
            ExtensionsButton::SearchAction if self.searching_extensions() => {
                self.extensions_query.clear();
                self.extensions_first = 0;
            }
            ExtensionsButton::SearchAction => {
                // The funnel steps through the filters, like the chips.
                let index = ExtensionsFilter::PILLS
                    .iter()
                    .position(|(filter, _, _)| *filter == self.extensions_filter)
                    .unwrap_or(0);
                self.extensions_filter = ExtensionsFilter::PILLS[(index + 1) % 3].0;
                self.extensions_first = 0;
            }
            ExtensionsButton::Chip(index) => {
                if let Some((filter, _, _)) = ExtensionsFilter::PILLS.get(index) {
                    self.extensions_filter = *filter;
                    self.extensions_first = 0;
                }
            }
        }
        self.forget_extensions_hot_row(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Re-read the registry list (versions, new extensions).
    pub(super) fn check_extension_updates(&mut self) {
        if !self.zed_registry_loading {
            self.zed_registry_loaded = false;
            self.status = "Checking the Zed registry...".into();
            self.ensure_zed_registry_loaded();
        }
    }

    /// The ··· menu's Show Installed / Show Featured: clears the search and
    /// filter and opens one section with the other shut.
    pub(super) fn show_extensions_section(&mut self, hwnd: HWND, section: ExtensionsSection) {
        self.extensions_query.clear();
        self.extensions_filter = ExtensionsFilter::Featured;
        self.extensions_first = 0;
        self.extension_sections_collapsed = match section {
            ExtensionsSection::Installed => [false, true, false],
            _ => [true, false, false],
        };
        self.forget_extensions_hot_row(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn open_extensions_folder(&mut self) {
        let Some(dir) = workflow::extensions_dir() else {
            return;
        };
        // An empty folder is still somewhere to look.
        let _ = std::fs::create_dir_all(&dir);
        let path = display_path(&dir);
        std::thread::spawn(move || {
            let _ = std::process::Command::new("explorer").arg(path).spawn();
        });
    }
}
