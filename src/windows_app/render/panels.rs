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
            SideView::Debug => self.explorer_visible.then_some(3),
            SideView::Extensions => self.explorer_visible.then_some(4),
        };
        for (index, label) in labels.iter().enumerate() {
            let top = self.scale(RAIL_FIRST_ROW + index as i32 * RAIL_ROW);
            let is_selected = selected == Some(index)
                || (index == 5 && self.ai_assistant_visible);
            if is_selected {
                let pill = RECT {
                    left: self.scale(8),
                    top,
                    right: self.scale(RAIL - 8),
                    bottom: top + self.scale(28),
                };
                self.panel_card(hdc, pill, self.scale(6), rgb(45, 78, 140), rgb(24, 40, 78));
            }
            let color = if is_selected {
                rgb(240, 245, 255)
            } else {
                MUTED
            };
            self.rail_icon(
                hdc,
                index,
                self.scale(23),
                top + self.scale(5),
                if is_selected { rgb(56, 189, 248) } else { color },
            );
            Self::label(hdc, label, self.scale(48), top + self.scale(5), color, clip);
        }
        let name = self
            .workspace_root
            .as_ref()
            .and_then(|r| r.file_name())
            .map(|n| n.to_string_lossy())
            .unwrap_or_else(|| "my-project".into());
        Self::label(
            hdc,
            "WORKSPACE",
            self.scale(20),
            editor_bottom - self.scale(104),
            MUTED,
            clip,
        );
        Self::label(
            hdc,
            &name,
            self.scale(20),
            editor_bottom - self.scale(82),
            TEXT,
            clip,
        );
        let branch = self.git_head_label();
        Self::label(
            hdc,
            &format!("\u{2442}  {branch}"),
            self.scale(20),
            editor_bottom - self.scale(58),
            MUTED,
            clip,
        );
        Self::label(
            hdc,
            "\u{2699}   \u{2192}",
            self.scale(20),
            editor_bottom - self.scale(32),
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
        let title = match self.side_view {
            SideView::Files => "EXPLORER",
            SideView::Search => "SEARCH IN FILES",
            SideView::Review => "SOURCE CONTROL",
            SideView::Debug => "RUN & DEBUG",
            SideView::Extensions => "EXTENSIONS",
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
        if self.side_view == SideView::Debug {
            self.paint_debug_panel(hdc, left, editor_left, editor_bottom, clip);
            return;
        }
        if self.side_view == SideView::Extensions {
            self.paint_extensions_panel(hdc, left, editor_left, editor_bottom, clip);
            return;
        }
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
                if self.review_staged {
                    "Nothing staged in this file."
                } else {
                    "No unstaged text changes in this file."
                },
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

    // Geometry shared with the click handler in input.rs: four evenly-sized
    // toolbar buttons (continue/pause, step over, step in, stop) above the
    // live status line.
    pub(in crate::windows_app) fn debug_toolbar_button(
        &self,
        left: i32,
        right: i32,
        index: i32,
    ) -> RECT {
        let s = |v: i32| self.scale(v);
        let gap = s(6);
        let width = (right - left - s(16) - gap * 3) / 4;
        let button_left = left + s(8) + index * (width + gap);
        RECT {
            left: button_left,
            top: s(48),
            right: button_left + width,
            bottom: s(78),
        }
    }

    // Vector icons instead of text glyphs: the UI font lacks several of the
    // Unicode symbols a first draft of this toolbar used, which rendered as
    // blank tofu boxes. Simple GDI shapes always render, and match how the
    // Python "Run" arrow in the tab strip is already drawn.
    fn debug_icon(&self, hdc: HDC, rect: RECT, index: i32, color: u32) {
        let cx = (rect.left + rect.right) / 2;
        let cy = (rect.top + rect.bottom) / 2;
        let r = self.scale(6);
        unsafe {
            let brush = CreateSolidBrush(color);
            let pen = CreatePen(PS_SOLID, self.scale(2).max(1), color);
            let old_brush = SelectObject(hdc, brush);
            let old_pen = SelectObject(hdc, pen);
            match index {
                // Continue/resume: a plain right-pointing triangle.
                0 => {
                    let points = [
                        POINT { x: cx - r + self.scale(1), y: cy - r },
                        POINT { x: cx - r + self.scale(1), y: cy + r },
                        POINT { x: cx + r, y: cy },
                    ];
                    Polygon(hdc, points.as_ptr(), 3);
                }
                // Step over: a rightward arrow.
                1 => {
                    MoveToEx(hdc, cx - r, cy, null_mut());
                    LineTo(hdc, cx + r - self.scale(2), cy);
                    let head = [
                        POINT { x: cx + r - self.scale(4), y: cy - self.scale(4) },
                        POINT { x: cx + r, y: cy },
                        POINT { x: cx + r - self.scale(4), y: cy + self.scale(4) },
                    ];
                    SelectObject(hdc, GetStockObject(NULL_PEN));
                    Polygon(hdc, head.as_ptr(), 3);
                }
                // Step in: a downward arrow.
                2 => {
                    MoveToEx(hdc, cx, cy - r, null_mut());
                    LineTo(hdc, cx, cy + r - self.scale(2));
                    let head = [
                        POINT { x: cx - self.scale(4), y: cy + r - self.scale(4) },
                        POINT { x: cx, y: cy + r },
                        POINT { x: cx + self.scale(4), y: cy + r - self.scale(4) },
                    ];
                    SelectObject(hdc, GetStockObject(NULL_PEN));
                    Polygon(hdc, head.as_ptr(), 3);
                }
                // Pause: two vertical bars, shown on the first slot while running.
                3 => {
                    let bar = self.scale(3);
                    FillRect(
                        hdc,
                        &RECT { left: cx - r, top: cy - r, right: cx - r + bar, bottom: cy + r },
                        brush,
                    );
                    FillRect(
                        hdc,
                        &RECT { left: cx + r - bar, top: cy - r, right: cx + r, bottom: cy + r },
                        brush,
                    );
                }
                // Stop: a filled square.
                _ => {
                    FillRect(
                        hdc,
                        &RECT { left: cx - r, top: cy - r, right: cx + r, bottom: cy + r },
                        brush,
                    );
                }
            }
            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            DeleteObject(brush);
            DeleteObject(pen);
        }
    }

    fn paint_debug_panel(&self, hdc: HDC, left: i32, right: i32, bottom: i32, clip: RECT) {
        let s = |v: i32| self.scale(v);
        let state = &self.debug_state;
        let has_session = self.debug.is_some();
        let running = has_session && state.running;

        let disabled = rgb(70, 84, 112);
        let buttons = [
            (if running { 3 } else { 0 }, rgb(34, 197, 94), true),
            (1, rgb(56, 189, 248), has_session),
            (2, rgb(56, 189, 248), has_session),
            (4, rgb(220, 60, 60), has_session),
        ];
        for (index, (icon, color, enabled)) in buttons.iter().enumerate() {
            let rect = self.debug_toolbar_button(left, right, index as i32);
            let active_color = if *enabled { *color } else { disabled };
            self.panel_card(hdc, rect, s(5), active_color, rgb(16, 24, 44));
            self.debug_icon(hdc, rect, *icon, if *enabled { rgb(255, 255, 255) } else { disabled });
        }

        let mut y = s(88);
        let (dot_color, status_color) = if state.status.starts_with("Build failed")
            || state.status.contains("not found")
            || state.status.contains("error")
        {
            (rgb(220, 60, 60), rgb(240, 180, 180))
        } else if running {
            (rgb(34, 197, 94), rgb(200, 220, 245))
        } else if has_session {
            (rgb(250, 204, 21), rgb(200, 220, 245))
        } else {
            (MUTED, MUTED)
        };
        unsafe {
            let brush = CreateSolidBrush(dot_color);
            let old_brush = SelectObject(hdc, brush);
            let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
            Ellipse(hdc, left + s(8), y + s(2), left + s(8) + s(7), y + s(9));
            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            DeleteObject(brush);
        }
        self.label_ellipsis(
            hdc,
            &state.status,
            left + s(20),
            y,
            status_color,
            RECT { left, top: clip.top, right: right - s(8), bottom: clip.bottom },
        );
        y += s(24);

        // 1. VARIABLES
        Self::fill(hdc, RECT { left, top: y, right, bottom: y + s(22) }, ACTIVE_BG);
        Self::label(hdc, "▼ VARIABLES", left + s(8), y + s(3), rgb(80, 160, 220), clip);
        y += s(24);

        if state.scopes.is_empty() {
            Self::label(hdc, "Not stopped", left + s(8), y, MUTED, clip);
            y += s(18);
        } else {
            let (rows, next_y) = self.debug_variable_rows(y, bottom, s(18), s(16));
            for row in &rows {
                let indent = s(12) * row.depth as i32;
                if row.is_header {
                    Self::label(hdc, &row.name, left + s(8), row.y, MUTED, clip);
                    continue;
                }
                if row.loading {
                    Self::label(hdc, &row.name, left + s(20) + indent, row.y, MUTED, clip);
                    continue;
                }
                // Only an expandable variable gets an arrow; a plain value
                // (an int, a string, ...) has nothing to click, so it stays
                // blank instead of showing a ">" that does nothing.
                let glyph = if !row.expandable {
                    " "
                } else if row.expanded {
                    "\u{25be}"
                } else {
                    "\u{25b8}"
                };
                Self::label(hdc, glyph, left + s(8) + indent, row.y, MUTED, clip);
                self.label_ellipsis(
                    hdc,
                    &row.name,
                    left + s(20) + indent,
                    row.y,
                    rgb(205, 220, 245),
                    RECT { left, top: clip.top, right: right - s(96), bottom: clip.bottom },
                );
                self.label_ellipsis(
                    hdc,
                    &row.value,
                    right - s(92),
                    row.y,
                    rgb(130, 150, 180),
                    RECT { left, top: clip.top, right: right - s(8), bottom: clip.bottom },
                );
            }
            y = next_y;
        }

        y += s(6);
        // 2. CALL STACK
        if y + s(28) <= bottom {
            Self::fill(hdc, RECT { left, top: y, right, bottom: y + s(22) }, ACTIVE_BG);
            Self::label(hdc, "▼ CALL STACK", left + s(8), y + s(3), rgb(80, 160, 220), clip);
            y += s(24);
            if state.frames.is_empty() {
                Self::label(hdc, "Not stopped", left + s(8), y, MUTED, clip);
                y += s(18);
            }
            for frame in &state.frames {
                if y + s(18) > bottom {
                    break;
                }
                let location = frame
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|name| format!("{}: {}", name.to_string_lossy(), frame.line))
                    .unwrap_or_default();
                self.label_ellipsis(
                    hdc,
                    &frame.name,
                    left + s(8),
                    y,
                    TEXT,
                    RECT { left, top: clip.top, right: right - s(94), bottom: clip.bottom },
                );
                Self::label(hdc, &location, right - s(90), y, MUTED, clip);
                y += s(18);
            }
        }

        y += s(6);
        // 3. BREAKPOINTS
        if y + s(28) <= bottom {
            Self::fill(hdc, RECT { left, top: y, right, bottom: y + s(22) }, ACTIVE_BG);
            Self::label(hdc, "▼ BREAKPOINTS", left + s(8), y + s(3), rgb(80, 160, 220), clip);
            y += s(24);
            let breakpoints: Vec<(String, usize)> = self
                .tabs
                .iter()
                .filter_map(|tab| {
                    let name = tab.document.path.as_ref()?.file_name()?.to_string_lossy().into_owned();
                    Some((name, tab.document.breakpoints()))
                })
                .flat_map(|(name, lines)| lines.iter().map(move |line| (name.clone(), line + 1)))
                .collect();
            if breakpoints.is_empty() {
                Self::label(hdc, "Click a line's gutter to add one", left + s(8), y, MUTED, clip);
            }
            for (name, line) in &breakpoints {
                if y + s(18) > bottom {
                    break;
                }
                unsafe {
                    let brush = CreateSolidBrush(rgb(220, 60, 60));
                    let old_brush = SelectObject(hdc, brush);
                    let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
                    Ellipse(hdc, left + s(8), y + s(4), left + s(8) + s(8), y + s(12));
                    SelectObject(hdc, old_pen);
                    SelectObject(hdc, old_brush);
                    DeleteObject(brush);
                }
                self.label_ellipsis(
                    hdc,
                    &format!("{name}: {line}"),
                    left + s(24),
                    y,
                    TEXT,
                    RECT { left, top: clip.top, right: right - s(8), bottom: clip.bottom },
                );
                y += s(18);
            }
        }
    }

    fn paint_extensions_panel(&self, hdc: HDC, left: i32, right: i32, bottom: i32, clip: RECT) {
        let s = |v: i32| self.scale(v);

        // 1. VS Code style Search bar with vector icon and filter
        let search_rect = RECT {
            left: left + s(8),
            top: s(46),
            right: right - s(8),
            bottom: s(76),
        };
        let search_border = if self.extensions_search_active {
            rgb(56, 189, 248)
        } else {
            rgb(35, 48, 76)
        };
        self.panel_card(hdc, search_rect, s(5), search_border, rgb(13, 20, 36));

        // Vector magnifying glass icon
        let icon_cx = search_rect.left + s(14);
        let icon_cy = (search_rect.top + search_rect.bottom) / 2;
        let icon_color = if self.extensions_search_active {
            rgb(56, 189, 248)
        } else {
            rgb(100, 116, 145)
        };
        self.stroke(hdc, icon_color, |hdc| unsafe {
            Ellipse(hdc, icon_cx - s(4), icon_cy - s(4), icon_cx + s(4), icon_cy + s(4));
            MoveToEx(hdc, icon_cx + s(3), icon_cy + s(3), null_mut());
            LineTo(hdc, icon_cx + s(7), icon_cy + s(7));
        });

        // Search text / placeholder
        let text_x = search_rect.left + s(26);
        let text_clip = RECT {
            left: text_x,
            top: search_rect.top,
            right: search_rect.right - s(24),
            bottom: search_rect.bottom,
        };
        let text_y = (search_rect.top + search_rect.bottom) / 2 - self.text_height(hdc) / 2;

        if self.extensions_query.is_empty() {
            if self.extensions_search_active {
                if self.caret_on {
                    Self::label(hdc, "|", text_x, text_y, rgb(56, 189, 248), text_clip);
                }
            } else {
                Self::label(
                    hdc,
                    "Search Extensions...",
                    text_x,
                    text_y,
                    rgb(95, 110, 140),
                    text_clip,
                );
            }
        } else {
            let display = if self.extensions_search_active && self.caret_on {
                format!("{}|", self.extensions_query)
            } else {
                self.extensions_query.clone()
            };
            Self::label(hdc, &display, text_x, text_y, TEXT, text_clip);

            // Clear "×" button
            let clear_x = search_rect.right - s(18);
            Self::label(hdc, "×", clear_x, text_y, rgb(148, 163, 184), search_rect);
        }

        // Funnel filter icon on the right if query is empty
        if self.extensions_query.is_empty() {
            self.stroke(hdc, rgb(95, 110, 140), |hdc| unsafe {
                let fx = search_rect.right - s(18);
                let fy = search_rect.top + s(9);
                MoveToEx(hdc, fx, fy, null_mut());
                LineTo(hdc, fx + s(10), fy);
                MoveToEx(hdc, fx + s(2), fy + s(5), null_mut());
                LineTo(hdc, fx + s(8), fy + s(5));
                MoveToEx(hdc, fx + s(4), fy + s(10), null_mut());
                LineTo(hdc, fx + s(6), fy + s(10));
            });
        }

        // 2. Modern Segmented Pill Capsule Tabs
        let tabs_rect = RECT {
            left: left + s(8),
            top: s(84),
            right: right - s(8),
            bottom: s(110),
        };
        self.panel_card(hdc, tabs_rect, s(5), rgb(28, 40, 68), rgb(13, 20, 36));

        let half_w = (tabs_rect.right - tabs_rect.left) / 2;
        let mkt_rect = RECT {
            left: tabs_rect.left + s(2),
            top: tabs_rect.top + s(2),
            right: tabs_rect.left + half_w - s(1),
            bottom: tabs_rect.bottom - s(2),
        };
        let inst_rect = RECT {
            left: tabs_rect.left + half_w + s(1),
            top: tabs_rect.top + s(2),
            right: tabs_rect.right - s(2),
            bottom: tabs_rect.bottom - s(2),
        };

        let installed_count = self.extensions.iter().filter(|e| e.installed).count();
        let inst_label = format!("Installed ({installed_count})");

        if self.extensions_tab == ExtensionsTab::Marketplace {
            self.panel_card(hdc, mkt_rect, s(4), rgb(56, 189, 248), rgb(30, 50, 88));
            let mkt_w = self.text_width(hdc, "Marketplace");
            let mkt_x = mkt_rect.left + ((mkt_rect.right - mkt_rect.left) - mkt_w) / 2;
            Self::label(
                hdc,
                "Marketplace",
                mkt_x,
                (mkt_rect.top + mkt_rect.bottom) / 2 - self.text_height(hdc) / 2,
                rgb(255, 255, 255),
                mkt_rect,
            );

            let inst_w = self.text_width(hdc, &inst_label);
            let inst_x = inst_rect.left + ((inst_rect.right - inst_rect.left) - inst_w) / 2;
            Self::label(
                hdc,
                &inst_label,
                inst_x,
                (inst_rect.top + inst_rect.bottom) / 2 - self.text_height(hdc) / 2,
                rgb(148, 163, 184),
                inst_rect,
            );
        } else {
            let mkt_w = self.text_width(hdc, "Marketplace");
            let mkt_x = mkt_rect.left + ((mkt_rect.right - mkt_rect.left) - mkt_w) / 2;
            Self::label(
                hdc,
                "Marketplace",
                mkt_x,
                (mkt_rect.top + mkt_rect.bottom) / 2 - self.text_height(hdc) / 2,
                rgb(148, 163, 184),
                mkt_rect,
            );

            self.panel_card(hdc, inst_rect, s(4), rgb(56, 189, 248), rgb(30, 50, 88));
            let inst_w = self.text_width(hdc, &inst_label);
            let inst_x = inst_rect.left + ((inst_rect.right - inst_rect.left) - inst_w) / 2;
            Self::label(
                hdc,
                &inst_label,
                inst_x,
                (inst_rect.top + inst_rect.bottom) / 2 - self.text_height(hdc) / 2,
                rgb(255, 255, 255),
                inst_rect,
            );
        }

        // 3. Section Category Header
        let visible = self.filtered_extensions();
        let section_y = s(118);
        let (section_title, section_count) = if self.extensions_tab == ExtensionsTab::Installed {
            ("INSTALLED", installed_count)
        } else {
            ("POPULAR", visible.len())
        };
        let sec_text = format!("▾  {section_title} ({section_count})");
        Self::label(hdc, &sec_text, left + s(12), section_y, rgb(110, 130, 160), clip);
        let sec_w = self.text_width(hdc, &sec_text);
        Self::fill(
            hdc,
            RECT {
                left: left + s(16) + sec_w,
                top: section_y + s(8),
                right: right - s(8),
                bottom: section_y + s(9),
            },
            rgb(28, 40, 68),
        );

        // 4. Extensions List
        let mut ey = s(136);
        let card_h = s(86);
        let card_gap = s(8);

        if visible.is_empty() {
            let notice = if self.extensions_tab == ExtensionsTab::Installed {
                "No installed extensions"
            } else {
                "No extensions found"
            };
            Self::label(hdc, notice, left + s(16), ey + s(12), MUTED, clip);
            return;
        }

        for ext in &visible {
            if ey + card_h > bottom {
                break;
            }

            let card_rect = RECT {
                left: left + s(8),
                top: ey,
                right: right - s(8),
                bottom: ey + card_h,
            };
            self.panel_card(hdc, card_rect, s(6), rgb(32, 46, 76), rgb(15, 23, 42));

            // Left Icon Badge (40x40 rounded badge with brand styling)
            let icon_rect = RECT {
                left: card_rect.left + s(8),
                top: ey + s(8),
                right: card_rect.left + s(48),
                bottom: ey + s(48),
            };

            if ext.id == "prettier" {
                Self::rounded_fill(hdc, icon_rect, s(6), rgb(22, 28, 44));
                self.card_outline(hdc, icon_rect, s(6), rgb(48, 64, 98));

                // Authentic Prettier 4-Color Stripes
                let stripe_y = icon_rect.bottom - s(8);
                let stripe_h = s(3);
                let sw = s(6);
                let sx0 = icon_rect.left + s(7);
                Self::fill(hdc, RECT { left: sx0, top: stripe_y, right: sx0 + sw, bottom: stripe_y + stripe_h }, rgb(86, 182, 240));
                Self::fill(hdc, RECT { left: sx0 + sw + s(1), top: stripe_y, right: sx0 + sw * 2 + s(1), bottom: stripe_y + stripe_h }, rgb(236, 72, 153));
                Self::fill(hdc, RECT { left: sx0 + sw * 2 + s(2), top: stripe_y, right: sx0 + sw * 3 + s(2), bottom: stripe_y + stripe_h }, rgb(245, 197, 24));
                Self::fill(hdc, RECT { left: sx0 + sw * 3 + s(3), top: stripe_y, right: sx0 + sw * 4 + s(3), bottom: stripe_y + stripe_h }, rgb(168, 85, 247));

                // Curly code braces
                unsafe { SelectObject(hdc, self.brand_font); }
                self.label_mid(hdc, "{ }", icon_rect.left + s(9), (icon_rect.top + icon_rect.bottom) / 2 - s(4), rgb(255, 255, 255), icon_rect);
                unsafe { SelectObject(hdc, self.ui_font); }
            } else {
                Self::rounded_fill(hdc, icon_rect, s(6), rgb(14, 28, 54));
                self.card_outline(hdc, icon_rect, s(6), rgb(35, 70, 125));
                self.icons.draw(hdc, "folder-src", icon_rect.left + s(7), icon_rect.top + s(7), s(26));
            }

            let content_left = card_rect.left + s(54);
            let content_right = card_rect.right - s(8);

            // Row 1: Extension Title & Verified badge & Version
            let display_title = ext.name.split(" - ").next().unwrap_or(ext.name);
            unsafe { SelectObject(hdc, self.brand_font); }
            Self::label(hdc, display_title, content_left, ey + s(6), rgb(245, 247, 250), card_rect);
            let title_w = self.text_width(hdc, display_title);
            unsafe { SelectObject(hdc, self.ui_font); }

            let badge_x = content_left + title_w + s(5);
            Self::rounded_fill(
                hdc,
                RECT {
                    left: badge_x,
                    top: ey + s(8),
                    right: badge_x + s(12),
                    bottom: ey + s(20),
                },
                s(3),
                rgb(56, 189, 248),
            );
            Self::label(hdc, "✓", badge_x + s(2), ey + s(7), rgb(255, 255, 255), card_rect);

            let ver_x = badge_x + s(16);
            Self::label(hdc, ext.version, ver_x, ey + s(7), rgb(100, 116, 140), card_rect);

            // Row 2: Description (clean single-line with ellipsis)
            let desc_clip = RECT {
                left: content_left,
                top: ey + s(27),
                right: content_right,
                bottom: ey + s(46),
            };
            self.label_ellipsis(hdc, ext.description, content_left, ey + s(27), rgb(148, 163, 184), desc_clip);

            // Row 3: Metadata (Publisher, Downloads & Rating) on left
            let meta_text = format!("by {}   ↓ {}   {}", ext.publisher, ext.downloads, ext.rating);
            let meta_clip = RECT {
                left: content_left,
                top: ey + s(53),
                right: card_rect.right - s(86),
                bottom: ey + s(75),
            };
            self.label_ellipsis(hdc, &meta_text, content_left, ey + s(53), rgb(100, 116, 145), meta_clip);

            // Row 3: Action Button on right
            let btn_w = s(76);
            let btn_h = s(22);
            let btn_rect = RECT {
                left: card_rect.right - btn_w - s(8),
                top: ey + s(51),
                right: card_rect.right - s(8),
                bottom: ey + s(51) + btn_h,
            };

            if ext.installing {
                self.panel_card(hdc, btn_rect, s(4), rgb(56, 189, 248), rgb(18, 38, 72));
                let text_w = self.text_width(hdc, "Installing...");
                let tx = btn_rect.left + ((btn_rect.right - btn_rect.left) - text_w) / 2;
                self.label_mid(hdc, "Installing...", tx, (btn_rect.top + btn_rect.bottom) / 2, rgb(186, 230, 253), btn_rect);
            } else if ext.installed {
                self.panel_card(hdc, btn_rect, s(4), rgb(48, 70, 105), rgb(20, 32, 54));
                let text_w = self.text_width(hdc, "✓ Installed");
                let tx = btn_rect.left + ((btn_rect.right - btn_rect.left) - text_w) / 2;
                self.label_mid(hdc, "✓ Installed", tx, (btn_rect.top + btn_rect.bottom) / 2, rgb(148, 195, 245), btn_rect);
            } else {
                Self::rounded_fill(hdc, btn_rect, s(4), rgb(14, 99, 156));
                let text_w = self.text_width(hdc, "Install");
                let tx = btn_rect.left + ((btn_rect.right - btn_rect.left) - text_w) / 2;
                self.label_mid(hdc, "Install", tx, (btn_rect.top + btn_rect.bottom) / 2, rgb(255, 255, 255), btn_rect);
            }

            ey += card_h + card_gap;
        }

        // 5. Features & Capabilities Showcase (Eliminating the empty void!)
        let guide_top = ey + s(14);
        let guide_h = s(160);
        if guide_top + guide_h < bottom {
            let guide_rect = RECT {
                left: left + s(8),
                top: guide_top,
                right: right - s(8),
                bottom: guide_top + guide_h,
            };
            self.panel_card(hdc, guide_rect, s(6), rgb(28, 42, 68), rgb(12, 18, 34));

            // Header
            Self::label(hdc, "WORKFLOW CAPABILITIES", guide_rect.left + s(10), guide_rect.top + s(8), rgb(100, 116, 145), guide_rect);
            Self::fill(
                hdc,
                RECT {
                    left: guide_rect.left + s(10),
                    top: guide_rect.top + s(24),
                    right: guide_rect.right - s(10),
                    bottom: guide_rect.top + s(25),
                },
                rgb(28, 42, 68),
            );

            // Row 1: Prettier
            let r1_y = guide_rect.top + s(32);
            Self::label(hdc, "{ }", guide_rect.left + s(10), r1_y, rgb(56, 189, 248), guide_rect);
            Self::label(hdc, "Prettier Formatting", guide_rect.left + s(30), r1_y, rgb(226, 232, 240), guide_rect);
            Self::label(hdc, "Shift + Alt + F formats active document", guide_rect.left + s(30), r1_y + s(16), rgb(100, 116, 145), guide_rect);

            // Row 2: Material Icons
            let r2_y = r1_y + s(40);
            self.icons.draw(hdc, "folder-src", guide_rect.left + s(8), r2_y, s(16));
            Self::label(hdc, "Material Icon Theme", guide_rect.left + s(30), r2_y, rgb(226, 232, 240), guide_rect);
            Self::label(hdc, "Visual themes for 20+ file types and tabs", guide_rect.left + s(30), r2_y + s(16), rgb(100, 116, 145), guide_rect);

            // Row 3: Real-Time Engine
            let r3_y = r2_y + s(40);
            Self::label(hdc, "⚡", guide_rect.left + s(10), r3_y, rgb(245, 197, 24), guide_rect);
            Self::label(hdc, "Real-Time Node Subprocess", guide_rect.left + s(30), r3_y, rgb(226, 232, 240), guide_rect);
            Self::label(hdc, "Non-blocking background CLI execution", guide_rect.left + s(30), r3_y + s(16), rgb(100, 116, 145), guide_rect);
        }
    }
}

