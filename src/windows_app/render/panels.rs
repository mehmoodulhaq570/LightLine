use super::super::*;

impl App {
    pub(in crate::windows_app) fn paint_rail(&self, hdc: HDC, editor_bottom: i32) {
        let clip = RECT {
            left: 0,
            top: 0,
            right: self.scale(RAIL),
            bottom: editor_bottom,
        };
        unsafe {
            DrawIconEx(
                hdc,
                self.scale(10),
                self.scale(5),
                self.brand_icon,
                self.scale(32),
                self.scale(32),
                0,
                null_mut(),
                DI_NORMAL,
            );
            SelectObject(hdc, self.brand_font);
        }
        Self::label(hdc, "LightLine", self.scale(48), self.scale(9), TEXT, clip);
        Self::rounded_fill(
            hdc,
            RECT {
                left: self.scale(118),
                top: self.scale(13),
                right: self.scale(146),
                bottom: self.scale(29),
            },
            self.scale(6),
            rgb(61, 45, 145),
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        Self::label(hdc, "IDE", self.scale(121), self.scale(12), TEXT, clip);
        Self::fill(
            hdc,
            RECT {
                left: 0,
                top: self.scale(40),
                right: self.scale(RAIL),
                bottom: self.scale(41),
            },
            EDGE,
        );

        let labels = [
            "Explorer",
            "Search",
            "Source Control",
            "Run & Debug",
            "Extensions",
            "AI Assistant",
        ];
        let selected = match self.side_view {
            SideView::Files => self.explorer_visible.then_some(0),
            SideView::Search => self.explorer_visible.then_some(1),
            SideView::Review => self.explorer_visible.then_some(2),
        };
        for (index, label) in labels.iter().enumerate() {
            let top = self.scale(RAIL_FIRST_ROW + index as i32 * RAIL_ROW);
            let is_selected = selected == Some(index) || (index == 3 && self.terminal_visible);
            if is_selected {
                Self::rounded_fill(
                    hdc,
                    RECT {
                        left: self.scale(8),
                        top,
                        right: self.scale(RAIL - 8),
                        bottom: top + self.scale(28),
                    },
                    self.scale(8),
                    ACTIVE_BG,
                );
                Self::fill(
                    hdc,
                    RECT {
                        left: 0,
                        top: top + self.scale(3),
                        right: self.scale(3),
                        bottom: top + self.scale(25),
                    },
                    BLUE,
                );
            }
            let color = if is_selected {
                TEXT
            } else if index >= 4 {
                rgb(111, 134, 166)
            } else {
                MUTED
            };
            self.rail_icon(
                hdc,
                index,
                self.scale(23),
                top + self.scale(5),
                if is_selected { BLUE } else { color },
            );
            Self::label(hdc, label, self.scale(48), top + self.scale(5), color, clip);
        }
        if let Some(root) = &self.workspace_root {
            let name = root.file_name().unwrap_or_default().to_string_lossy();
            Self::label(
                hdc,
                "◈  Workspace",
                self.scale(23),
                editor_bottom - self.scale(112),
                MUTED,
                clip,
            );
            Self::label(
                hdc,
                &name,
                self.scale(23),
                editor_bottom - self.scale(89),
                TEXT,
                clip,
            );
            if let Some(branch) = &self.workspace_branch {
                Self::label(
                    hdc,
                    &format!("⑂  {branch}"),
                    self.scale(23),
                    editor_bottom - self.scale(65),
                    MUTED,
                    clip,
                );
            }
        }
        Self::label(
            hdc,
            "⚙",
            self.scale(23),
            editor_bottom - self.scale(31),
            MUTED,
            clip,
        );
        Self::label(
            hdc,
            "→",
            self.scale(54),
            editor_bottom - self.scale(31),
            MUTED,
            clip,
        );
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
        let title = if self.side_view == SideView::Search {
            "SEARCH IN FILES"
        } else {
            "CHANGES"
        };
        Self::label(
            hdc,
            title,
            left + self.scale(16),
            self.scale(11),
            MUTED,
            clip,
        );
        Self::fill(
            hdc,
            RECT {
                left,
                top: self.scale(39),
                right: editor_left,
                bottom: self.scale(40),
            },
            EDGE,
        );
        if self.side_view == SideView::Search {
            Self::fill(
                hdc,
                RECT {
                    left: left + self.scale(8),
                    top: self.scale(47),
                    right: editor_left - self.scale(8),
                    bottom: self.scale(78),
                },
                ACTIVE_BG,
            );
            let query_label = if self.project_query.is_empty() && !self.search_input {
                "Type query, press Enter".to_owned()
            } else {
                format!(
                    "{}{}",
                    self.project_query,
                    if self.search_input && self.focused && self.caret_on {
                        "|"
                    } else {
                        ""
                    }
                )
            };
            Self::label(
                hdc,
                &query_label,
                left + self.scale(16),
                self.scale(52),
                if self.project_query.is_empty() {
                    MUTED
                } else {
                    TEXT
                },
                clip,
            );
            Self::label(
                hdc,
                &if self.search_cancel.is_some() {
                    "SEARCHING...".to_owned()
                } else {
                    format!("{} RESULTS", self.search_results.len())
                },
                left + self.scale(16),
                self.scale(87),
                MUTED,
                clip,
            );
            for (index, hit) in self
                .search_results
                .iter()
                .enumerate()
                .skip(self.panel_first)
            {
                let top = self.scale(113 + (index - self.panel_first) as i32 * 48);
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
                        SELECT_BG,
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
                    TEXT,
                    clip,
                );
                Self::label(
                    hdc,
                    &hit.preview,
                    left + self.scale(14),
                    top + self.scale(19),
                    MUTED,
                    RECT {
                        left: left + self.scale(14),
                        top,
                        right: editor_left - self.scale(8),
                        bottom: top + self.scale(46),
                    },
                );
            }
        } else {
            Self::label(
                hdc,
                &if self.review_loading {
                    "Loading Git changes...".to_owned()
                } else {
                    format!("{} changed files", self.changes.len())
                },
                left + self.scale(16),
                self.scale(52),
                MUTED,
                clip,
            );
            for (index, change) in self.changes.iter().enumerate().skip(self.panel_first) {
                let top = self.scale(86 + (index - self.panel_first) as i32 * EXPLORER_ROW);
                if top >= editor_bottom {
                    break;
                }
                if self.review_file.as_ref() == Some(&change.path)
                    || self.panel_focus && index == self.panel_selected
                {
                    Self::fill(
                        hdc,
                        RECT {
                            left: left + self.scale(7),
                            top,
                            right: editor_left - self.scale(7),
                            bottom: top + self.scale(EXPLORER_ROW - 2),
                        },
                        SELECT_BG,
                    );
                }
                Self::label(hdc, &change.status, left + self.scale(12), top, GREEN, clip);
                let name_right = editor_left - self.scale(7);
                self.label_ellipsis(
                    hdc,
                    &display_path(&change.path),
                    left + self.scale(37),
                    top,
                    TEXT,
                    RECT {
                        left: left + self.scale(37),
                        top,
                        right: name_right,
                        bottom: top + self.scale(EXPLORER_ROW),
                    },
                );
            }
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
            EDITOR_BG,
        );
        Self::fill(
            hdc,
            RECT {
                left: mid,
                top,
                right: mid + 1,
                bottom,
            },
            EDGE,
        );
        let name = self
            .review_file
            .as_ref()
            .map(|path| display_path(path))
            .unwrap_or_default();
        Self::label(
            hdc,
            &format!("BEFORE  ·  {name}"),
            left + self.scale(16),
            top + self.scale(9),
            MUTED,
            RECT {
                left,
                top,
                right: mid,
                bottom: top + self.scale(36),
            },
        );
        Self::label(
            hdc,
            &format!("AFTER  ·  {name}"),
            mid + self.scale(16),
            top + self.scale(9),
            MUTED,
            RECT {
                left: mid,
                top,
                right,
                bottom: top + self.scale(36),
            },
        );
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
                    rgb(47, 31, 42),
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
                    rgb(24, 55, 50),
                );
            }
            if let Some(number) = row.before_number {
                Self::label(
                    hdc,
                    &number.to_string(),
                    left + self.scale(10),
                    y,
                    MUTED,
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
                    MUTED,
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
                TEXT,
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
                TEXT,
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
                "No unstaged text changes in this file.",
                left + self.scale(18),
                top + self.scale(62),
                MUTED,
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
            STATUS_BG,
        );
        Self::fill(
            hdc,
            RECT {
                left: x,
                top: y,
                right: x + self.scale(3),
                bottom: y + self.scale(152),
            },
            BLUE,
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
            TEXT,
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
                GREEN
            } else {
                MUTED
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
            STATUS_BG,
        );
        Self::fill(
            hdc,
            RECT {
                left,
                top,
                right: left + width,
                bottom: top + self.scale(2),
            },
            VIOLET,
        );
        Self::label(
            hdc,
            &format!(
                "Quick Open  ›  {}{}",
                self.quick_query,
                if self.focused && self.caret_on {
                    "|"
                } else {
                    ""
                }
            ),
            left + self.scale(16),
            top + self.scale(11),
            TEXT,
            RECT {
                left,
                top,
                right: left + width,
                bottom: top + self.scale(43),
            },
        );
        Self::label(
            hdc,
            "Type to filter  ·  Enter opens  ·  Esc closes",
            left + self.scale(16),
            top + self.scale(39),
            MUTED,
            RECT {
                left,
                top,
                right: left + width,
                bottom,
            },
        );
        // Capped at 7 (not the 8 rows the box has room for) so a full list never
        // shares its last row with the "Type > for commands" hint below it.
        let items: Vec<String> = if self.quick_query.starts_with('>') {
            self.quick_commands()
                .iter()
                .take(7)
                .map(|(name, _)| format!(">  {name}"))
                .collect()
        } else {
            self.quick_matches()
                .iter()
                .take(7)
                .map(|path| {
                    path.strip_prefix(self.workspace_root.as_deref().unwrap_or(Path::new("")))
                        .unwrap_or(path)
                        .display()
                        .to_string()
                })
                .collect()
        };
        for (index, label) in items.iter().enumerate() {
            let y = top + self.scale(68 + index as i32 * 34);
            if index == self.quick_selected {
                Self::fill(
                    hdc,
                    RECT {
                        left: left + self.scale(8),
                        top: y - self.scale(2),
                        right: left + width - self.scale(8),
                        bottom: y + self.scale(30),
                    },
                    SELECT_BG,
                );
            }
            Self::label(
                hdc,
                label,
                left + self.scale(18),
                y + self.scale(2),
                TEXT,
                RECT {
                    left: left + self.scale(18),
                    top: y,
                    right: left + width - self.scale(12),
                    bottom: y + self.scale(29),
                },
            );
        }
        if items.is_empty() {
            Self::label(
                hdc,
                if self.quick_loading {
                    "Loading workspace files..."
                } else if self.workspace_root.is_none() {
                    "Open a workspace first (Ctrl+Shift+O)"
                } else {
                    "No matching files or commands"
                },
                left + self.scale(18),
                top + self.scale(80),
                MUTED,
                RECT {
                    left,
                    top,
                    right: left + width,
                    bottom,
                },
            );
        }
        if !self.quick_query.starts_with('>') {
            Self::label(
                hdc,
                "Type > for commands",
                left + self.scale(18),
                bottom - self.scale(28),
                MUTED,
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
