use super::super::*;
use super::blend;
use crate::windows_app::panels::{QUICK_MODES, QuickLayout};

impl App {
    pub(in crate::windows_app) fn paint_rail(&self, hdc: HDC, editor_bottom: i32) {
        let selected = match self.side_view {
            SideView::Files => self.explorer_visible.then_some(0),
            SideView::Search => self.explorer_visible.then_some(1),
            SideView::Review => self.explorer_visible.then_some(2),
            SideView::Debug => self.explorer_visible.then_some(3),
            SideView::Extensions => self.explorer_visible.then_some(4),
            SideView::Settings => None,
        };
        // Flat, as VS Code's: the active view has a bar at the rail's edge
        // and a bright icon; under the mouse an icon brightens.
        let s = |value: i32| self.scale(value);
        let size = s(22);
        let row = s(RAIL_ROW);
        let rail_width = s(RAIL);
        let at_row = |top: i32| RECT {
            left: 0,
            top,
            right: rail_width,
            bottom: top + row,
        };
        let menu_color = if self.hot == Some(Hot::RailMenu) {
            self.hot_color(self.theme.muted, self.theme.text)
        } else {
            self.theme.muted
        };
        self.glyph_in(
            hdc,
            DebugGlyph::Menu,
            menu_color,
            at_row(s(RAIL_MENU_ROW)),
            size,
        );
        for index in 0..6 {
            let top = s(RAIL_FIRST_ROW + index as i32 * RAIL_ROW);
            let is_selected = selected == Some(index) || (index == 5 && self.ai_assistant_visible);
            if is_selected {
                Self::fill(
                    hdc,
                    RECT {
                        left: 0,
                        top: top + s(6),
                        right: s(2).max(2),
                        bottom: top + row - s(6),
                    },
                    self.theme.violet,
                );
            }
            let color = if is_selected {
                self.theme.text
            } else if self.hot == Some(Hot::Rail(index)) {
                self.hot_color(self.theme.muted, self.theme.text)
            } else {
                self.theme.muted
            };
            let glyph = match index {
                0 => DebugGlyph::Files,
                1 => DebugGlyph::Search,
                2 => DebugGlyph::SourceControl,
                3 => DebugGlyph::RunDebug,
                4 => DebugGlyph::Extensions,
                _ => DebugGlyph::Sparkle,
            };
            self.glyph_in(hdc, glyph, color, at_row(top), size);
        }
        let settings_open = self.side_view == SideView::Settings && self.explorer_visible;
        let gear = RECT {
            left: 0,
            top: editor_bottom - s(44),
            right: rail_width,
            bottom: editor_bottom,
        };
        if settings_open {
            Self::fill(
                hdc,
                RECT {
                    left: 0,
                    top: gear.top + s(6),
                    right: s(2).max(2),
                    bottom: gear.bottom - s(6),
                },
                self.theme.violet,
            );
        }
        let gear_color = if settings_open {
            self.theme.text
        } else if self.hot == Some(Hot::RailSettings) {
            self.hot_color(self.theme.muted, self.theme.text)
        } else {
            self.theme.muted
        };
        self.glyph_in(hdc, DebugGlyph::Gear, gear_color, gear, size);
    }

    pub(in crate::windows_app) fn paint_side_panel(
        &self,
        hdc: HDC,
        editor_left: i32,
        editor_bottom: i32,
    ) {
        let left = self.scale(RAIL);
        let clip = RECT {
            left,
            top: 0,
            right: editor_left,
            bottom: editor_bottom,
        };
        let title = match self.side_view {
            SideView::Files => "EXPLORER",
            SideView::Search => "SEARCH IN FILES",
            SideView::Review => "SOURCE CONTROL",
            SideView::Debug => "RUN AND DEBUG",
            SideView::Extensions => "EXTENSIONS",
            SideView::Settings => "SETTINGS",
        };
        Self::label(
            hdc,
            title,
            left + self.scale(16),
            self.scale(11),
            self.theme.muted,
            clip,
        );
        // Each panel draws its own header buttons (git.rs, debug_panel.rs,
        // extensions_panel.rs lay them out).
        Self::fill(
            hdc,
            RECT {
                left,
                top: self.scale(39),
                right: editor_left,
                bottom: self.scale(40),
            },
            self.theme.edge,
        );
        if self.side_view == SideView::Debug {
            self.paint_debug_panel(hdc, left, editor_left, editor_bottom, clip);
            return;
        }
        if self.side_view == SideView::Settings {
            self.paint_settings_panel(hdc, left, editor_left, editor_bottom);
            return;
        }
        if self.side_view == SideView::Extensions {
            self.paint_extensions_panel(hdc, left, editor_left, editor_bottom, clip);
            return;
        }
        if self.side_view == SideView::Search {
            let typing_query = self.search_input && !self.replace_focus;
            let typing_replacement = self.search_input && self.replace_focus;
            Self::fill(
                hdc,
                self.search_query_box(left, editor_left),
                self.theme.active_bg,
            );
            let query_label = if self.project_query.is_empty() && !typing_query {
                "Type query, press Enter".to_owned()
            } else {
                format!(
                    "{}{}",
                    self.project_query,
                    if typing_query && self.focused && self.caret_on {
                        "|"
                    } else {
                        ""
                    }
                )
            };
            let query_box = self.search_query_box(left, editor_left);
            Self::label(
                hdc,
                &query_label,
                left + self.scale(16),
                self.scale(52),
                if self.project_query.is_empty() {
                    self.theme.muted
                } else {
                    self.theme.text
                },
                RECT {
                    right: self.option_toggles_left(query_box),
                    ..clip
                },
            );
            self.paint_option_toggles(hdc, query_box, self.search_options);
            let replace_box = self.replace_box(left, editor_left);
            Self::fill(hdc, replace_box, self.theme.active_bg);
            let replace_label = if self.search_replacement.is_empty() && !typing_replacement {
                "Replace with (Tab)".to_owned()
            } else {
                format!(
                    "{}{}",
                    self.search_replacement,
                    if typing_replacement && self.focused && self.caret_on {
                        "|"
                    } else {
                        ""
                    }
                )
            };
            Self::label(
                hdc,
                &replace_label,
                left + self.scale(16),
                replace_box.top + self.scale(5),
                if self.search_replacement.is_empty() {
                    self.theme.muted
                } else {
                    self.theme.text
                },
                RECT {
                    right: replace_box.right - self.scale(4),
                    ..clip
                },
            );
            let button = self.replace_all_button(editor_left);
            let ready = !self.project_query.is_empty() && self.replace_request.is_none();
            self.panel_card(
                hdc,
                button,
                self.scale(5),
                if ready {
                    self.theme.violet
                } else {
                    self.theme.edge
                },
                self.theme.sidebar_bg,
            );
            let button_label = if self.replace_request.is_some() {
                "Finding..."
            } else {
                "Replace all"
            };
            let width = self.text_width(hdc, button_label);
            Self::label(
                hdc,
                button_label,
                (button.left + button.right - width) / 2,
                button.top + self.scale(5),
                if ready {
                    self.theme.text
                } else {
                    self.theme.muted
                },
                RECT {
                    left: button.left,
                    right: button.right,
                    ..clip
                },
            );
            Self::label(
                hdc,
                &if self.search_cancel.is_some() {
                    "SEARCHING...".to_owned()
                } else {
                    format!("{} RESULTS", self.search_results.len())
                },
                left + self.scale(16),
                self.search_count_top(),
                self.theme.muted,
                clip,
            );
            let list_top = self.search_list_top();
            for (index, hit) in self
                .search_results
                .iter()
                .enumerate()
                .skip(self.panel_first)
            {
                let top = list_top + (index - self.panel_first) as i32 * self.scale(48);
                if top >= editor_bottom {
                    break;
                }
                if self.panel_focus && index == self.panel_selected {
                    Self::fill(
                        hdc,
                        RECT {
                            left: left + self.scale(7),
                            top,
                            right: editor_left - self.scale(7),
                            bottom: top + self.scale(45),
                        },
                        self.theme.select_bg,
                    );
                }
                Self::label(
                    hdc,
                    &format!(
                        "{}:{}",
                        hit.path.file_name().unwrap_or_default().to_string_lossy(),
                        hit.line + 1
                    ),
                    left + self.scale(14),
                    top,
                    self.theme.text,
                    clip,
                );
                Self::label(
                    hdc,
                    &hit.preview,
                    left + self.scale(14),
                    top + self.scale(19),
                    self.theme.muted,
                    RECT {
                        left: left + self.scale(14),
                        top,
                        right: editor_left - self.scale(8),
                        bottom: top + self.scale(46),
                    },
                );
            }
        } else {
            self.paint_git_review(hdc, left, editor_left, editor_bottom, clip);
        }
    }

    pub(in crate::windows_app) fn paint_diff(&self, hdc: HDC, left: i32, right: i32, bottom: i32) {
        let top = self.editor_top();
        let mid = left + (right - left) / 2;
        Self::fill(
            hdc,
            RECT {
                left,
                top,
                right,
                bottom,
            },
            self.theme.editor_bg,
        );
        Self::fill(
            hdc,
            RECT {
                left: mid,
                top,
                right: mid + 1,
                bottom,
            },
            self.theme.edge,
        );
        let history = self.history_labels();
        let (restore, close) = self.history_buttons(right);
        let (before_label, after_label, after_right) = match &history {
            Some((before, after)) => (before.clone(), after.clone(), restore.left - self.scale(8)),
            None => {
                let name = self
                    .review_file
                    .as_ref()
                    .map(|path| display_path(path))
                    .unwrap_or_default();
                (
                    format!("BEFORE  ·  {name}"),
                    format!("AFTER  ·  {name}"),
                    right,
                )
            }
        };
        Self::label(
            hdc,
            &before_label,
            left + self.scale(16),
            top + self.scale(9),
            self.theme.muted,
            RECT {
                left,
                top,
                right: mid,
                bottom: top + self.scale(36),
            },
        );
        Self::label(
            hdc,
            &after_label,
            mid + self.scale(16),
            top + self.scale(9),
            self.theme.muted,
            RECT {
                left: mid,
                top,
                right: after_right,
                bottom: top + self.scale(36),
            },
        );
        if history.is_some() {
            // Restore is the action this view is for; Close (or Esc) leaves.
            Self::rounded_fill(hdc, restore, self.scale(6), self.theme.violet);
            let width = self.text_width(hdc, "Restore");
            self.label_mid(
                hdc,
                "Restore",
                (restore.left + restore.right - width) / 2,
                (restore.top + restore.bottom) / 2,
                label_on(self.theme.violet, 255, 255, 255),
                restore,
            );
            self.panel_card(
                hdc,
                close,
                self.scale(6),
                self.theme.edge,
                self.theme.editor_bg,
            );
            let width = self.text_width(hdc, "Close");
            self.label_mid(
                hdc,
                "Close",
                (close.left + close.right - width) / 2,
                (close.top + close.bottom) / 2,
                self.theme.text,
                close,
            );
        }
        unsafe { SelectObject(hdc, self.font) };
        for (index, row) in self.diff_rows.iter().enumerate().skip(self.diff_first) {
            let y = top + self.scale(43) + (index - self.diff_first) as i32 * self.line_height;
            if y >= bottom {
                break;
            }
            if row.changed && row.before_number.is_some() {
                Self::fill(
                    hdc,
                    RECT {
                        left,
                        top: y,
                        right: mid,
                        bottom: (y + self.line_height).min(bottom),
                    },
                    ui(47, 31, 42),
                );
            }
            if row.changed && row.after_number.is_some() {
                Self::fill(
                    hdc,
                    RECT {
                        left: mid + 1,
                        top: y,
                        right,
                        bottom: (y + self.line_height).min(bottom),
                    },
                    ui(24, 55, 50),
                );
            }
            if let Some(number) = row.before_number {
                Self::label(
                    hdc,
                    &number.to_string(),
                    left + self.scale(10),
                    y,
                    self.theme.muted,
                    RECT {
                        left,
                        top: y,
                        right: mid,
                        bottom,
                    },
                );
            }
            if let Some(number) = row.after_number {
                Self::label(
                    hdc,
                    &number.to_string(),
                    mid + self.scale(10),
                    y,
                    self.theme.muted,
                    RECT {
                        left: mid,
                        top: y,
                        right,
                        bottom,
                    },
                );
            }
            Self::label(
                hdc,
                &row.before,
                left + self.scale(50),
                y,
                self.theme.text,
                RECT {
                    left: left + self.scale(50),
                    top: y,
                    right: mid - self.scale(8),
                    bottom,
                },
            );
            Self::label(
                hdc,
                &row.after,
                mid + self.scale(50),
                y,
                self.theme.text,
                RECT {
                    left: mid + self.scale(50),
                    top: y,
                    right: right - self.scale(8),
                    bottom,
                },
            );
        }
        unsafe { SelectObject(hdc, self.ui_font) };
        if self.diff_rows.is_empty() {
            Self::label(
                hdc,
                if history.is_some() {
                    "This version is the same as the file now."
                } else if self.review_staged {
                    "Nothing staged in this file."
                } else {
                    "No unstaged text changes in this file."
                },
                left + self.scale(18),
                top + self.scale(62),
                self.theme.muted,
                RECT {
                    left,
                    top,
                    right,
                    bottom,
                },
            );
        }
    }

    pub(in crate::windows_app) fn paint_search_preview(
        &self,
        hdc: HDC,
        left: i32,
        right: i32,
        bottom: i32,
    ) {
        if self.side_view != SideView::Search || !self.panel_focus || self.search_input {
            return;
        }
        let Some(hit) = self.search_results.get(self.panel_selected) else {
            return;
        };
        let width = self.scale(620).min(right - left - self.scale(30));
        if width < self.scale(250) {
            return;
        }
        let x = left + self.scale(15);
        let y = (bottom - self.scale(176)).max(self.editor_top() + self.scale(18));
        Self::fill(
            hdc,
            RECT {
                left: x,
                top: y,
                right: x + width,
                bottom: y + self.scale(152),
            },
            self.theme.status_bg,
        );
        Self::fill(
            hdc,
            RECT {
                left: x,
                top: y,
                right: x + self.scale(3),
                bottom: y + self.scale(152),
            },
            self.theme.blue,
        );
        Self::label(
            hdc,
            &format!(
                "PREVIEW  ·  {}:{}",
                hit.path.file_name().unwrap_or_default().to_string_lossy(),
                hit.line + 1
            ),
            x + self.scale(13),
            y + self.scale(8),
            self.theme.text,
            RECT {
                left: x,
                top: y,
                right: x + width,
                bottom: y + self.scale(30),
            },
        );
        unsafe { SelectObject(hdc, self.font) };
        for (index, (number, line)) in hit.context.iter().enumerate() {
            let top = y + self.scale(34) + index as i32 * self.scale(21);
            let color = if *number == hit.line + 1 {
                self.theme.green
            } else {
                self.theme.muted
            };
            Self::label(
                hdc,
                &format!("{number:>4}  {line}"),
                x + self.scale(12),
                top,
                color,
                RECT {
                    left: x + self.scale(12),
                    top,
                    right: x + width - self.scale(10),
                    bottom: y + self.scale(150),
                },
            );
        }
        unsafe { SelectObject(hdc, self.ui_font) };
    }

    pub(in crate::windows_app) fn paint_quick_open(&self, hdc: HDC, rect: RECT) {
        if !self.quick_open {
            return;
        }
        let s = |value: i32| self.scale(value);
        let rows = self.quick_rows();
        let layout = self.quick_layout(hdc, rect.right, rows.len());
        let card = layout.card;
        // A shadow GDI can draw: a darker band below and to the right.
        let shade = blend(self.theme.shell_bg, ui(0, 0, 0), 0.6);
        Self::fill(
            hdc,
            RECT {
                left: card.left + s(3),
                top: card.bottom - s(3),
                right: card.right + s(3),
                bottom: card.bottom + s(3),
            },
            shade,
        );
        Self::fill(
            hdc,
            RECT {
                left: card.right - s(3),
                top: card.top + s(4),
                right: card.right + s(3),
                bottom: card.bottom,
            },
            shade,
        );
        let body = blend(self.theme.editor_bg, self.theme.active_bg, 0.45);
        // Square at the top, where it joins the input: the card starts
        // higher and is clipped; its top edge is the line they share.
        let state = unsafe { SaveDC(hdc) };
        unsafe { IntersectClipRect(hdc, card.left, card.top, card.right, card.bottom) };
        self.panel_card(
            hdc,
            RECT {
                top: card.top - s(8),
                ..card
            },
            s(6),
            self.theme.blue,
            body,
        );
        unsafe { RestoreDC(hdc, state) };
        Self::fill(
            hdc,
            RECT {
                bottom: card.top + s(1).max(1),
                ..card
            },
            self.theme.blue,
        );
        unsafe { SelectObject(hdc, self.ui_font) };

        let needle = self.quick_needle();
        if rows.is_empty() {
            let row = layout.rows[0];
            let message = self.quick_empty_message();
            self.label_mid(
                hdc,
                &message,
                row.left + s(10),
                (row.top + row.bottom) / 2,
                self.theme.muted,
                row,
            );
        }
        let shown = rows.iter().enumerate().skip(self.quick_first);
        for (row, (index, item)) in layout.rows.iter().zip(shown) {
            let middle = (row.top + row.bottom) / 2;
            if index == self.quick_selected {
                Self::rounded_fill(hdc, *row, s(4), self.theme.select_bg);
            }
            let mut x = row.left + s(10);
            match &item.icon {
                QuickIcon::File(path) => {
                    let size = s(16);
                    let top = middle - size / 2;
                    if !self.icons.draw_for_path(
                        hdc,
                        path,
                        false,
                        false,
                        self.has_extension("material-icons"),
                        x,
                        top,
                        size,
                    ) {
                        self.draw_vector_file(hdc, path, x, top, size);
                    }
                    x += s(24);
                }
                QuickIcon::Command => {
                    let icon = RECT {
                        left: x,
                        right: x + s(16),
                        ..*row
                    };
                    self.glyph_in(hdc, DebugGlyph::ChevronRight, self.theme.muted, icon, s(12));
                    x += s(24);
                }
                QuickIcon::None => {}
            }
            let keys_left = self.paint_keycaps(hdc, &item.keys, row.right - s(8), middle, body);
            let right = keys_left - s(8);
            let end = self.label_match(
                hdc,
                &item.name,
                (x, middle, right),
                &needle,
                self.theme.text,
                self.theme.sky,
            );
            if !item.detail.is_empty() && end + s(24) < right {
                // Highlighted only when the name doesn't hold the match.
                let detail_needle = if item.name.to_ascii_lowercase().contains(&needle) {
                    ""
                } else {
                    &needle
                };
                self.label_match(
                    hdc,
                    &item.detail,
                    (end + s(8), middle, right),
                    detail_needle,
                    self.theme.muted,
                    self.theme.sky,
                );
            }
        }
        self.paint_quick_footer(hdc, &layout, rows.len(), body);
    }

    // The footer: a line, the mode buttons (or the picker's name) and the
    // keys, or where the list is when it scrolls.
    fn paint_quick_footer(&self, hdc: HDC, layout: &QuickLayout, count: usize, body: u32) {
        let s = |value: i32| self.scale(value);
        let footer = layout.footer;
        Self::fill(
            hdc,
            RECT {
                left: footer.left + s(1).max(1),
                top: footer.top,
                right: footer.right - s(1).max(1),
                bottom: footer.top + s(1).max(1),
            },
            self.theme.edge,
        );
        let middle = (footer.top + footer.bottom) / 2;
        unsafe { SelectObject(hdc, self.small_font) };
        let current = self.quick_query.chars().next();
        let mut used_right = footer.left + s(8);
        for &(rect, prefix) in &layout.modes {
            let on = current == Some(prefix);
            if on {
                Self::rounded_fill(hdc, rect, s(4), self.theme.active_bg);
            }
            let name = QUICK_MODES
                .iter()
                .find(|(mode, _)| *mode == prefix)
                .map_or("", |(_, name)| name);
            let prefix_text = prefix.to_string();
            let x = rect.left + s(7);
            self.label_mid(hdc, &prefix_text, x, middle, self.theme.text, rect);
            let name_x = x + self.text_width(hdc, &format!("{prefix}  "));
            let color = if on {
                self.theme.text
            } else {
                self.theme.muted
            };
            self.label_mid(hdc, name, name_x, middle, color, rect);
            used_right = rect.right;
        }
        if layout.modes.is_empty() {
            let title = if self.language_pick {
                "Language of this file".to_string()
            } else {
                format!("Saved versions of {}", self.history_file_name())
            };
            let clip = RECT {
                right: footer.left + (footer.right - footer.left) / 2,
                ..footer
            };
            self.label_ellipsis(
                hdc,
                &title,
                footer.left + s(12),
                middle - self.text_height(hdc) / 2,
                self.theme.muted,
                clip,
            );
            used_right = clip.right;
        }
        let right = footer.right - s(10);
        if count > QUICK_ROWS {
            let last = (self.quick_first + QUICK_ROWS).min(count);
            let text = format!("{}\u{2013}{} of {count}", self.quick_first + 1, last);
            let width = self.text_width(hdc, &text);
            self.label_mid(hdc, &text, right - width, middle, self.theme.muted, footer);
        } else {
            let verb = if self.language_pick {
                "choose"
            } else if self.history_pick.is_some() {
                "compare"
            } else if self.quick_query.starts_with('>') {
                "run"
            } else {
                "open"
            };
            let hints: [(&[&str], &str); 2] = [(&["Enter"], verb), (&["Esc"], "close")];
            // Only when they fit beside the mode buttons.
            let cap = |key: &str| self.text_width(hdc, key) + s(10) + s(3);
            let width: i32 = hints
                .iter()
                .map(|(keys, word)| {
                    keys.iter().map(|key| cap(key)).sum::<i32>()
                        + self.text_width(hdc, word)
                        + s(12)
                })
                .sum();
            if right - width > used_right + s(8) {
                let mut x = right;
                for (keys, word) in hints.iter().rev() {
                    let word_width = self.text_width(hdc, word);
                    self.label_mid(hdc, word, x - word_width, middle, self.theme.muted, footer);
                    x = self.paint_keycaps(hdc, keys, x - word_width - s(4), middle, body) - s(8);
                }
            }
        }
        unsafe { SelectObject(hdc, self.ui_font) };
    }

    /// Keycaps ending at `right`, right to left ("" is a gap between two
    /// shortcuts); returns their left edge. Leaves the UI font selected
    /// unless the small font was.
    fn paint_keycaps(&self, hdc: HDC, keys: &[&str], right: i32, middle: i32, body: u32) -> i32 {
        if keys.is_empty() {
            return right;
        }
        let s = |value: i32| self.scale(value);
        let old = unsafe { SelectObject(hdc, self.small_font) };
        let fill = blend(body, self.theme.text, 0.12);
        let mut x = right;
        for key in keys.iter().rev() {
            if key.is_empty() {
                x -= s(6);
                continue;
            }
            let width = self.text_width(hdc, key) + s(10);
            let cap = RECT {
                left: x - width,
                top: middle - s(9),
                right: x,
                bottom: middle + s(9),
            };
            Self::rounded_fill(hdc, cap, s(4), fill);
            self.label_mid(hdc, key, cap.left + s(5), middle, self.theme.text, cap);
            x = cap.left - s(3);
        }
        unsafe { SelectObject(hdc, old) };
        x + s(3)
    }

    // What was typed after the mode's prefix, lowercase, for highlighting.
    fn quick_needle(&self) -> String {
        let query = if self.language_pick || self.history_pick.is_some() {
            self.quick_query.as_str()
        } else {
            self.quick_query
                .strip_prefix(['>', '@', '#', ':'])
                .unwrap_or(&self.quick_query)
        };
        query.trim().to_ascii_lowercase()
    }

    fn quick_empty_message(&self) -> String {
        if self.language_pick {
            "No matching languages".into()
        } else if self.history_pick.is_some() {
            "No matching versions".into()
        } else if self.quick_query.starts_with('@') {
            self.symbols_empty_message()
        } else if self.quick_query.starts_with('#') {
            self.workspace_symbols_message()
        } else if self.quick_query.starts_with(':') {
            self.goto_line_message()
        } else if self.quick_query.starts_with('>') {
            "No matching commands".into()
        } else if self.quick_loading {
            "Loading workspace files...".into()
        } else if self.workspace_root.is_none() {
            "Open a workspace first (Ctrl+Shift+O)".into()
        } else {
            "No matching files".into()
        }
    }

    // Every item in the current mode, as its row shows it.
    fn quick_rows(&self) -> Vec<QuickRow> {
        let plain = |name: String| QuickRow {
            icon: QuickIcon::None,
            name,
            detail: String::new(),
            keys: Vec::new(),
        };
        if self.language_pick {
            self.language_picks()
                .into_iter()
                .map(|(label, _)| plain(label))
                .collect()
        } else if self.history_pick.is_some() {
            self.history_picks()
                .into_iter()
                .map(|(label, _)| plain(label))
                .collect()
        } else if self.quick_query.starts_with('>') {
            self.quick_commands()
                .into_iter()
                .map(|(name, _)| {
                    let (name, keys) = split_shortcut(name);
                    QuickRow {
                        icon: QuickIcon::Command,
                        name: name.to_string(),
                        detail: String::new(),
                        keys,
                    }
                })
                .collect()
        } else if self.quick_query.starts_with('@') {
            self.quick_symbols()
                .into_iter()
                .map(|symbol| plain(crate::windows_app::symbols::symbol_label(symbol)))
                .collect()
        } else if self.quick_query.starts_with('#') {
            self.quick_workspace_symbols()
                .iter()
                .map(|found| plain(self.workspace_symbol_label(found)))
                .collect()
        } else if self.quick_query.starts_with(':') {
            self.goto_line_items().into_iter().map(plain).collect()
        } else {
            let root = self.workspace_root.as_deref().unwrap_or(Path::new(""));
            self.quick_matches()
                .into_iter()
                .map(|path| {
                    let relative = path.strip_prefix(root).unwrap_or(&path);
                    let name = relative.file_name().map_or_else(
                        || relative.display().to_string(),
                        |name| name.to_string_lossy().into_owned(),
                    );
                    let detail = relative
                        .parent()
                        .map(|parent| parent.display().to_string())
                        .unwrap_or_default();
                    QuickRow {
                        icon: QuickIcon::File(path.clone()),
                        name,
                        detail,
                        keys: Vec::new(),
                    }
                })
                .collect()
        }
    }
}

enum QuickIcon {
    File(PathBuf),
    Command,
    None,
}

/// One Quick Open row: its icon, name, a dimmed detail after the name (a
/// file's folder) and keycaps at the right.
struct QuickRow {
    icon: QuickIcon,
    name: String,
    detail: String,
    keys: Vec<&'static str>,
}

/// A command's name and the keys in its trailing "(Ctrl+Shift+R)" or
/// "(Ctrl+G, :)", as keycaps ("" between two shortcuts). Brackets that hold
/// words, as in "(for debugging)", stay in the name.
fn split_shortcut(name: &'static str) -> (&'static str, Vec<&'static str>) {
    let is_key = |key: &str| {
        matches!(key, "Ctrl" | "Shift" | "Alt")
            || key.chars().count() == 1
            || (key.len() > 1 && key.starts_with('F') && key[1..].parse::<u8>().is_ok())
    };
    if let Some(open) = name.rfind(" (")
        && let Some(inner) = name[open + 2..].strip_suffix(')')
        && inner
            .split(", ")
            .all(|shortcut| shortcut.split('+').all(is_key))
    {
        let mut keys = Vec::new();
        for (index, shortcut) in inner.split(", ").enumerate() {
            if index > 0 {
                keys.push("");
            }
            keys.extend(shortcut.split('+'));
        }
        return (&name[..open], keys);
    }
    (name, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::split_shortcut;

    #[test]
    fn shortcuts_become_keycaps() {
        assert_eq!(
            split_shortcut("Run Active File (Ctrl+Shift+R)"),
            ("Run Active File", vec!["Ctrl", "Shift", "R"])
        );
        assert_eq!(
            split_shortcut("Go to Line (Ctrl+G, :)"),
            ("Go to Line", vec!["Ctrl", "G", "", ":"])
        );
        assert_eq!(
            split_shortcut("Rename Symbol (F2)"),
            ("Rename Symbol", vec!["F2"])
        );
        assert_eq!(
            split_shortcut("Quick Fix (Ctrl+.)"),
            ("Quick Fix", vec!["Ctrl", "."])
        );
    }

    #[test]
    fn words_in_brackets_stay_in_the_name() {
        assert_eq!(
            split_shortcut("Python: Install debugpy (for debugging)"),
            ("Python: Install debugpy (for debugging)", vec![])
        );
        assert_eq!(
            split_shortcut("Search in files"),
            ("Search in files", vec![])
        );
    }
}
