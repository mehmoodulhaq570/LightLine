use super::super::app::{Extension, ExtensionCategory, ExtensionIcon};
use super::super::extensions_panel::monogram;
use super::super::extensions_panel::{
    EXTENSION_INDENT, ExtensionRow, ExtensionsButton, ExtensionsHot, ExtensionsLayout,
};
use super::super::*;
use super::blend;

impl App {
    /// The Extensions panel below its header title, laid out as
    /// design/extensions-design.html: search, filter chips, then one list of
    /// installed and featured extensions (or search results) and what's in
    /// use, extensions 62 px and other rows 22 px.
    pub(in crate::windows_app) fn paint_extensions_panel(
        &self,
        hdc: HDC,
        left: i32,
        right: i32,
        bottom: i32,
        clip: RECT,
    ) {
        let layout = self.extensions_layout(left, right, Some(hdc));
        // Hover repaints one row; rows outside the update region are skipped.
        let mut update = RECT::default();
        unsafe { GetClipBox(hdc, &mut update) };
        let shows = |rect: &RECT| rect.bottom > update.top && rect.top < update.bottom;

        self.paint_extensions_icon_button(
            hdc,
            layout.refresh,
            ExtensionsButton::Refresh,
            DebugGlyph::Refresh,
        );
        self.paint_extensions_icon_button(
            hdc,
            layout.more,
            ExtensionsButton::More,
            DebugGlyph::More,
        );
        if shows(&layout.search) {
            self.paint_extensions_search(hdc, &layout);
        }
        for (index, rect) in layout.chips.iter().enumerate() {
            self.paint_extensions_chip(hdc, *rect, index);
        }
        Self::fill(
            hdc,
            RECT {
                left,
                top: layout.divider,
                right,
                bottom: layout.divider + 1,
            },
            self.theme.edge,
        );

        let rows = self.extension_rows();
        let hot_row = self.extensions_hot_row();
        for (index, rect) in self.extension_rows_on_screen(&rows, left, right, layout.list_top) {
            if rect.top >= bottom {
                break;
            }
            if !shows(&rect) {
                continue;
            }
            self.paint_extension_row(hdc, &rows[index], rect, hot_row == Some(index), clip);
        }
    }

    fn extensions_hot(&self, button: ExtensionsButton) -> bool {
        self.hot == Some(Hot::Extensions(ExtensionsHot::Button(button)))
    }

    fn extensions_hover_fill(&self) -> u32 {
        self.hot_color(self.theme.sidebar_bg, self.theme.active_bg)
    }

    fn paint_extensions_icon_button(
        &self,
        hdc: HDC,
        rect: RECT,
        button: ExtensionsButton,
        glyph: DebugGlyph,
    ) {
        let hot = self.extensions_hot(button);
        if hot {
            Self::rounded_fill(hdc, rect, self.scale(4), self.extensions_hover_fill());
        }
        let color = if hot {
            self.hot_color(self.theme.muted, self.theme.text)
        } else {
            self.theme.muted
        };
        self.glyph_in(hdc, glyph, color, rect, self.scale(16));
    }

    fn paint_extensions_search(&self, hdc: HDC, layout: &ExtensionsLayout) {
        let s = |value: i32| self.scale(value);
        let rect = layout.search;
        let focused = self.extensions_search_active && self.focused;
        self.panel_card(
            hdc,
            rect,
            s(4),
            if focused {
                self.theme.blue
            } else {
                self.theme.edge
            },
            self.theme.active_bg,
        );
        let middle = (rect.top + rect.bottom) / 2;
        let size = s(16);
        self.icons.draw_glyph(
            hdc,
            DebugGlyph::Search,
            if focused {
                self.theme.text
            } else {
                self.theme.muted
            },
            rect.left + s(8),
            middle - size / 2,
            size,
        );
        let text_left = rect.left + s(30);
        let limit = RECT {
            right: layout.search_action.left - s(4),
            ..rect
        };
        let query = &self.extensions_query;
        let top = middle - self.text_height(hdc) / 2;
        if query.is_empty() {
            self.label_ellipsis(
                hdc,
                "Search the Zed registry",
                text_left,
                top,
                self.theme.muted,
                limit,
            );
        } else {
            self.label_ellipsis(hdc, query, text_left, top, self.theme.text, limit);
        }
        if focused && self.caret_on {
            let x = (text_left + self.text_width(hdc, query)).min(limit.right);
            Self::fill(
                hdc,
                RECT {
                    left: x,
                    top: middle - s(8),
                    right: x + 1,
                    bottom: middle + s(8),
                },
                self.theme.text,
            );
        }
        let glyph = if query.is_empty() {
            DebugGlyph::Filter
        } else {
            DebugGlyph::Close
        };
        let hot = self.extensions_hot(ExtensionsButton::SearchAction);
        if hot {
            Self::rounded_fill(
                hdc,
                layout.search_action,
                s(3),
                self.hot_color(self.theme.active_bg, hot::hover_fill()),
            );
        }
        // The funnel is lit while a filter other than All is on.
        let filtered = query.is_empty() && self.extensions_filter != ExtensionsFilter::Featured;
        let color = if filtered {
            self.theme.blue
        } else if hot {
            self.theme.text
        } else {
            self.theme.muted
        };
        self.glyph_in(hdc, glyph, color, layout.search_action, size);
    }

    fn paint_extensions_chip(&self, hdc: HDC, rect: RECT, index: usize) {
        let Some((filter, label, _)) = ExtensionsFilter::PILLS.get(index) else {
            return;
        };
        let on = self.extensions_filter == *filter;
        let hot = self.extensions_hot(ExtensionsButton::Chip(index));
        let height = rect.bottom - rect.top;
        let (edge, fill) = if on {
            (self.theme.blue, self.theme.select_bg)
        } else if hot {
            (self.theme.edge, self.extensions_hover_fill())
        } else {
            (self.theme.edge, self.theme.sidebar_bg)
        };
        self.panel_card(hdc, rect, height, edge, fill);
        let width = self.text_width(hdc, label);
        self.label_mid(
            hdc,
            label,
            (rect.left + rect.right - width) / 2,
            (rect.top + rect.bottom) / 2,
            if on || hot {
                self.theme.text
            } else {
                self.theme.muted
            },
            rect,
        );
    }

    fn paint_extension_row(&self, hdc: HDC, row: &ExtensionRow, rect: RECT, hot: bool, clip: RECT) {
        let s = |value: i32| self.scale(value);
        let middle = (rect.top + rect.bottom) / 2;
        let limit = RECT {
            right: rect.right - s(10),
            ..rect
        };
        match row {
            ExtensionRow::Header {
                section,
                count,
                note,
            } => {
                let collapsed = self.extension_sections_collapsed[section.slot()];
                self.chevron(hdc, rect.left + s(12), middle, !collapsed);
                let title = section.title();
                let title_left = rect.left + s(22);
                self.label_mid(
                    hdc,
                    title,
                    title_left,
                    middle,
                    blend(self.theme.muted, self.theme.text, 0.35),
                    clip,
                );
                let count_text = count.to_string();
                let count_width = self.text_width(hdc, &count_text);
                let pill_left = title_left + self.text_width(hdc, title) + s(6);
                let pill = RECT {
                    left: pill_left,
                    top: middle - s(8),
                    right: pill_left + (count_width + s(10)).max(s(18)),
                    bottom: middle + s(8),
                };
                Self::rounded_fill(hdc, pill, s(16), self.theme.edge);
                self.label_mid(
                    hdc,
                    &count_text,
                    (pill.left + pill.right - count_width) / 2,
                    middle,
                    self.theme.text,
                    pill,
                );
                if !note.is_empty() {
                    let width = self.text_width(hdc, note);
                    self.label_mid(
                        hdc,
                        note,
                        limit.right - width,
                        middle,
                        self.theme.muted,
                        rect,
                    );
                }
            }
            ExtensionRow::Note(text) => {
                self.label_ellipsis(
                    hdc,
                    text,
                    rect.left + s(EXTENSION_INDENT),
                    middle - self.text_height(hdc) / 2,
                    self.theme.muted,
                    limit,
                );
            }
            ExtensionRow::InUse(index) => {
                let (name, value) = self.in_use_row(*index);
                let theme_row = *index == 2;
                if hot && theme_row {
                    Self::fill(hdc, rect, self.extensions_hover_fill());
                }
                let dot = s(7);
                self.icons.draw_glyph(
                    hdc,
                    DebugGlyph::GutterDot,
                    if theme_row {
                        self.theme.violet
                    } else {
                        self.theme.green
                    },
                    rect.left + s(EXTENSION_INDENT),
                    middle - dot / 2,
                    dot,
                );
                let name_left = rect.left + s(EXTENSION_INDENT + 15);
                self.label_mid(hdc, name, name_left, middle, self.theme.text, rect);
                let value_left = name_left + s(92);
                let mut value_limit = limit;
                if theme_row {
                    let chevron = RECT {
                        left: limit.right - s(16),
                        ..rect
                    };
                    self.glyph_in(
                        hdc,
                        DebugGlyph::ChevronRight,
                        self.theme.muted,
                        chevron,
                        s(16),
                    );
                    value_limit.right = chevron.left - s(4);
                }
                self.label_ellipsis(
                    hdc,
                    &value,
                    value_left,
                    middle - self.text_height(hdc) / 2,
                    if theme_row {
                        self.theme.blue
                    } else {
                        self.theme.muted
                    },
                    value_limit,
                );
            }
            ExtensionRow::Extension(index) => self.paint_extension(hdc, rect, *index, hot),
        }
    }

    /// An extension: icon, name and version, description, publisher and
    /// kind, and its one button.
    fn paint_extension(&self, hdc: HDC, rect: RECT, index: usize, hot: bool) {
        let Some(ext) = self.extensions.get(index) else {
            return;
        };
        let s = |value: i32| self.scale(value);
        if hot {
            Self::fill(hdc, rect, self.extensions_hover_fill());
        }
        let icon = RECT {
            left: rect.left + s(EXTENSION_INDENT),
            top: rect.top + s(10),
            right: rect.left + s(EXTENSION_INDENT + 36),
            bottom: rect.top + s(46),
        };
        self.paint_extension_icon(hdc, icon, ext);

        let text_left = icon.right + s(10);
        let (action, label) = self.extension_action(hdc, rect, index, hot);
        let kind = ext.category();
        let kind_color = match kind {
            ExtensionCategory::Formatter => ui(86, 182, 240),
            ExtensionCategory::IconTheme => self.theme.green,
            ExtensionCategory::ColorTheme => ui(174, 109, 242),
            ExtensionCategory::Language => self.theme.sky,
            ExtensionCategory::Unknown => self.theme.muted,
        };

        // Line 1: the name (14 px), then the version; lines 2 and 3 are in
        // the 12 px font, each on its own band of the 62 px row.
        let line = |n: i32| rect.top + s([5, 24, 40][n as usize]);
        let limit = RECT {
            right: rect.right - s(10),
            ..rect
        };
        let title = ext.name.split(" - ").next().unwrap_or(&ext.name);
        unsafe { SelectObject(hdc, self.ui_font) };
        let title_width = self
            .text_width(hdc, title)
            .min(limit.right - text_left - s(40));
        self.label_ellipsis(
            hdc,
            title,
            text_left,
            line(0),
            self.theme.text,
            RECT {
                right: text_left + title_width,
                ..rect
            },
        );
        unsafe { SelectObject(hdc, self.small_font) };
        let small_top = |n: i32| line(n) + s(1);
        self.label_ellipsis(
            hdc,
            &ext.version,
            text_left + title_width + s(6),
            small_top(0) + s(1),
            self.theme.muted,
            limit,
        );
        // Line 2: the description.
        let description = if kind == ExtensionCategory::Unknown {
            "Install to see whether LightLine supports it yet"
        } else {
            ext.description.as_str()
        };
        self.label_ellipsis(
            hdc,
            description,
            text_left,
            small_top(1),
            blend(self.theme.muted, self.theme.text, 0.45),
            limit,
        );
        // Line 3: publisher · kind, up to the button.
        let publisher = if kind == ExtensionCategory::Unknown {
            "Zed registry"
        } else {
            ext.publisher.split(" (").next().unwrap_or(&ext.publisher)
        };
        let line3_limit = RECT {
            right: action.left - s(8),
            ..rect
        };
        let publisher_width = self
            .text_width(hdc, publisher)
            .min(line3_limit.right - text_left);
        self.label_ellipsis(
            hdc,
            publisher,
            text_left,
            small_top(2),
            self.theme.muted,
            line3_limit,
        );
        let mut x = text_left + publisher_width + s(5);
        if x < line3_limit.right - s(20) {
            self.label_ellipsis(
                hdc,
                "\u{00b7}",
                x,
                small_top(2),
                self.theme.muted,
                line3_limit,
            );
            x += self.text_width(hdc, "\u{00b7}") + s(5);
            self.label_ellipsis(hdc, kind.label(), x, small_top(2), kind_color, line3_limit);
        }

        // The button.
        let action_middle = (action.top + action.bottom) / 2;
        if ext.installing {
            self.panel_card(hdc, action, s(3), self.theme.edge, self.theme.sidebar_bg);
            self.label_mid(
                hdc,
                label,
                action.left + s(9),
                action_middle,
                self.theme.blue,
                action,
            );
        } else if !ext.installed {
            Self::rounded_fill(hdc, action, s(3), ui(18, 114, 196));
            self.label_mid(
                hdc,
                label,
                action.left + s(9),
                action_middle,
                ui(255, 255, 255),
                action,
            );
        } else if label.is_empty() {
            self.glyph_in(hdc, DebugGlyph::Check, self.theme.green, action, s(16));
        } else {
            self.panel_card(
                hdc,
                action,
                s(3),
                self.theme.edge,
                self.extensions_hover_fill(),
            );
            self.label_mid(
                hdc,
                label,
                action.left + s(9),
                action_middle,
                self.theme.text,
                action,
            );
        }
        unsafe { SelectObject(hdc, self.ui_font) };
    }

    /// The extension's icon, by the same rules for every extension: a color
    /// theme shows a swatch of its colors, an icon theme its own folder, a
    /// language its file icon, anything else a monogram. Prettier shows its
    /// own logo (offset bars in its colors) in the same style, and Dracula and
    /// Material, known before they're installed, look as they will after.
    fn paint_extension_icon(&self, hdc: HDC, icon: RECT, ext: &Extension) {
        let s = |value: i32| self.scale(value);
        match ext.id.as_str() {
            "prettier" => {
                self.paint_prettier_tile(hdc, icon);
                return;
            }
            // Dracula's colors (its own file once installed).
            "dracula" if !matches!(ext.icon, Some(ExtensionIcon::Swatch { .. })) => {
                let background = ui(40, 42, 54);
                self.paint_swatch(
                    hdc,
                    icon,
                    background,
                    blend(background, ui(255, 255, 255), 0.14),
                    &[ui(255, 121, 198), ui(241, 250, 140), ui(80, 250, 123)],
                );
                return;
            }
            // Its own folder once installed; a plain folder before.
            "material-icons" if !matches!(ext.icon, Some(ExtensionIcon::Svg(_))) => {
                self.panel_card(hdc, icon, s(6), self.theme.edge, self.theme.sidebar_bg);
                let size = s(22);
                let x = (icon.left + icon.right - size) / 2;
                let y = (icon.top + icon.bottom - size) / 2;
                if !self
                    .icons
                    .draw_generic(hdc, GenericIcon::Folder, x, y, size)
                {
                    self.draw_vector_folder(hdc, x, y, size, false);
                }
                return;
            }
            _ => {}
        }
        let size = s(22);
        let x = (icon.left + icon.right - size) / 2;
        let y = (icon.top + icon.bottom - size) / 2;
        match &ext.icon {
            Some(ExtensionIcon::Swatch {
                background,
                edge,
                lines,
            }) => self.paint_swatch(hdc, icon, *background, *edge, lines),
            Some(ExtensionIcon::Svg(path)) => {
                self.panel_card(hdc, icon, s(6), self.theme.edge, self.theme.sidebar_bg);
                if !self.icons.draw_svg_file(hdc, path, x, y, size) {
                    self.paint_monogram(hdc, icon, &ext.id);
                }
            }
            Some(ExtensionIcon::Language(suffix)) => {
                self.panel_card(hdc, icon, s(6), self.theme.edge, self.theme.sidebar_bg);
                let sample = std::path::PathBuf::from(format!("file.{suffix}"));
                let use_theme = self.has_extension("material-icons");
                if !self
                    .icons
                    .draw_for_path(hdc, &sample, false, false, use_theme, x, y, size)
                {
                    self.paint_monogram(hdc, icon, &ext.id);
                }
            }
            None => self.paint_monogram(hdc, icon, &ext.id),
        }
    }

    /// A color theme's tile: its background with three short lines of "code"
    /// in its colors.
    fn paint_swatch(&self, hdc: HDC, icon: RECT, background: u32, edge: u32, lines: &[u32; 3]) {
        let s = |value: i32| self.scale(value);
        self.panel_card(hdc, icon, s(6), edge, background);
        let middle = (icon.top + icon.bottom) / 2;
        for (index, (color, width)) in lines.iter().zip([15, 22, 11]).enumerate() {
            let top = middle - s(7) + s(5) * index as i32;
            Self::rounded_fill(
                hdc,
                RECT {
                    left: icon.left + s(7),
                    top,
                    right: icon.left + s(7 + width),
                    bottom: top + s(3),
                },
                s(3),
                *color,
            );
        }
    }

    /// Prettier's logo in the tiles' style: its offset bars in its colors.
    fn paint_prettier_tile(&self, hdc: HDC, icon: RECT) {
        let s = |value: i32| self.scale(value);
        let background = ui(26, 43, 52);
        self.panel_card(
            hdc,
            icon,
            s(6),
            blend(background, ui(255, 255, 255), 0.14),
            background,
        );
        // (left, width, color) from the top: teal, yellow, red, purple.
        let bars = [
            (7, 14, ui(86, 179, 180)),
            (13, 15, ui(247, 186, 62)),
            (7, 10, ui(234, 94, 94)),
            (11, 17, ui(191, 133, 191)),
        ];
        let top = (icon.top + icon.bottom) / 2 - s(9);
        for (index, (left, width, color)) in bars.into_iter().enumerate() {
            let y = top + s(5) * index as i32;
            Self::rounded_fill(
                hdc,
                RECT {
                    left: icon.left + s(left),
                    top: y,
                    right: icon.left + s(left + width),
                    bottom: y + s(3),
                },
                s(3),
                color,
            );
        }
    }

    /// Initials on one of eight hues picked from the id: a tile that's the
    /// same every time for the same extension.
    fn paint_monogram(&self, hdc: HDC, icon: RECT, id: &str) {
        let (letters, hue) = monogram(id);
        let hue = [
            self.theme.blue,
            self.theme.violet,
            self.theme.green,
            self.theme.warning,
            self.theme.error,
            self.theme.teal,
            self.theme.pink,
            self.theme.orange,
        ][hue];
        self.panel_card(
            hdc,
            icon,
            self.scale(6),
            blend(self.theme.edge, hue, 0.35),
            blend(self.theme.sidebar_bg, hue, 0.22),
        );
        unsafe { SelectObject(hdc, self.brand_font) };
        let width = self.text_width(hdc, &letters);
        self.label_mid(
            hdc,
            &letters,
            (icon.left + icon.right - width) / 2,
            (icon.top + icon.bottom) / 2,
            blend(hue, self.theme.text, 0.35),
            icon,
        );
        unsafe { SelectObject(hdc, self.small_font) };
    }
}
