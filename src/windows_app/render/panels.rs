use super::super::*;

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
        let width = self.scale(560).min(rect.right - self.scale(30));
        let left = (rect.right - width) / 2;
        let top = self.scale(52);
        let bottom = top + self.scale(70 + 8 * 34);
        Self::fill(
            hdc,
            RECT {
                left,
                top,
                right: left + width,
                bottom,
            },
            self.theme.status_bg,
        );
        Self::fill(
            hdc,
            RECT {
                left,
                top,
                right: left + width,
                bottom: top + self.scale(2),
            },
            self.theme.violet,
        );
        let history_title = match self.history_pick {
            Some(_) => format!("Saved versions of {}", self.history_file_name()),
            None => String::new(),
        };
        Self::label(
            hdc,
            &format!(
                "{}  ›  {}{}",
                if self.language_pick {
                    "Language of this file"
                } else if self.history_pick.is_some() {
                    history_title.as_str()
                } else {
                    "Quick Open"
                },
                self.quick_query,
                if self.focused && self.caret_on {
                    "|"
                } else {
                    ""
                }
            ),
            left + self.scale(16),
            top + self.scale(11),
            self.theme.text,
            RECT {
                left,
                top,
                right: left + width,
                bottom: top + self.scale(43),
            },
        );
        Self::label(
            hdc,
            if self.language_pick {
                "Type to filter  ·  Enter chooses  ·  Esc closes"
            } else if self.history_pick.is_some() {
                "Type to filter  ·  Enter compares with the file now  ·  Esc closes"
            } else {
                "Type to filter  ·  Enter opens  ·  Esc closes"
            },
            left + self.scale(16),
            top + self.scale(39),
            self.theme.muted,
            RECT {
                left,
                top,
                right: left + width,
                bottom,
            },
        );
        // QUICK_ROWS rows are shown (not the 8 the box has room for) so the list
        // never shares its last row with the hint line below it.
        let items: Vec<String> = if self.language_pick {
            self.language_picks()
                .into_iter()
                .map(|(label, _)| label)
                .collect()
        } else if self.history_pick.is_some() {
            self.history_picks()
                .into_iter()
                .map(|(label, _)| label)
                .collect()
        } else if self.quick_query.starts_with('>') {
            self.quick_commands()
                .iter()
                .map(|(name, _)| format!(">  {name}"))
                .collect()
        } else if self.quick_query.starts_with('@') {
            self.quick_symbols()
                .into_iter()
                .map(crate::windows_app::symbols::symbol_label)
                .collect()
        } else if self.quick_query.starts_with('#') {
            self.quick_workspace_symbols()
                .iter()
                .map(|found| self.workspace_symbol_label(found))
                .collect()
        } else if self.quick_query.starts_with(':') {
            self.goto_line_items()
        } else {
            self.quick_matches()
                .iter()
                .map(|path| {
                    path.strip_prefix(self.workspace_root.as_deref().unwrap_or(Path::new("")))
                        .unwrap_or(path)
                        .display()
                        .to_string()
                })
                .collect()
        };
        let shown = items
            .iter()
            .enumerate()
            .skip(self.quick_first)
            .take(QUICK_ROWS);
        for (row, (index, label)) in shown.enumerate() {
            let y = top + self.scale(68 + row as i32 * 34);
            if index == self.quick_selected {
                Self::fill(
                    hdc,
                    RECT {
                        left: left + self.scale(8),
                        top: y - self.scale(2),
                        right: left + width - self.scale(8),
                        bottom: y + self.scale(30),
                    },
                    self.theme.select_bg,
                );
            }
            Self::label(
                hdc,
                label,
                left + self.scale(18),
                y + self.scale(2),
                self.theme.text,
                RECT {
                    left: left + self.scale(18),
                    top: y,
                    right: left + width - self.scale(12),
                    bottom: y + self.scale(29),
                },
            );
        }
        if items.is_empty() {
            let symbols_message;
            Self::label(
                hdc,
                if self.language_pick {
                    "No matching languages"
                } else if self.history_pick.is_some() {
                    "No matching versions"
                } else if self.quick_query.starts_with('@') {
                    symbols_message = self.symbols_empty_message();
                    &symbols_message
                } else if self.quick_query.starts_with('#') {
                    symbols_message = self.workspace_symbols_message();
                    &symbols_message
                } else if self.quick_query.starts_with(':') {
                    symbols_message = self.goto_line_message();
                    &symbols_message
                } else if self.quick_query.starts_with('>') {
                    "No matching commands"
                } else if self.quick_loading {
                    "Loading workspace files..."
                } else if self.workspace_root.is_none() {
                    "Open a workspace first (Ctrl+Shift+O)"
                } else {
                    "No matching files"
                },
                left + self.scale(18),
                top + self.scale(80),
                self.theme.muted,
                RECT {
                    left,
                    top,
                    right: left + width,
                    bottom,
                },
            );
        }
        let mut hints = Vec::new();
        if !self.language_pick
            && self.history_pick.is_none()
            && !self.quick_query.starts_with(['>', '@', '#', ':'])
        {
            hints.push(
                "> commands  ·  @ symbols in this file  ·  # in the project  ·  : line".to_string(),
            );
        }
        if items.len() > QUICK_ROWS {
            let last = (self.quick_first + QUICK_ROWS).min(items.len());
            hints.push(format!(
                "{}–{} of {}  ·  scroll for more",
                self.quick_first + 1,
                last,
                items.len()
            ));
        }
        if !hints.is_empty() {
            Self::label(
                hdc,
                &hints.join("  ·  "),
                left + self.scale(18),
                bottom - self.scale(28),
                self.theme.muted,
                RECT {
                    left,
                    top,
                    right: left + width,
                    bottom,
                },
            );
        }
    }
}
