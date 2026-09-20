use super::super::*;

impl App {
    pub(in crate::windows_app) fn paint(&mut self, hwnd: HWND) {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let window_dc = BeginPaint(hwnd, &mut ps);
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            if self
                .backbuffer
                .as_ref()
                .is_none_or(|buffer| buffer.width != rect.right || buffer.height != rect.bottom)
            {
                self.backbuffer = Surface::new(window_dc, rect.right, rect.bottom);
                self.transition = None;
                KillTimer(hwnd, 3);
            }
            let hdc = self
                .backbuffer
                .as_ref()
                .map_or(window_dc, |buffer| buffer.dc);
            let old_font = SelectObject(hdc, self.font);
            SelectObject(hdc, self.ui_font);
            SetBkMode(hdc, TRANSPARENT as i32);
            if self.welcome {
                self.paint_welcome(hdc, rect);
                self.paint_quick_open(hdc, rect);
                SelectObject(hdc, old_font);
                if hdc != window_dc {
                    BitBlt(window_dc, 0, 0, rect.right, rect.bottom, hdc, 0, 0, SRCCOPY);
                }
                EndPaint(hwnd, &ps);
                return;
            }
            let editor_bottom = (rect.bottom - self.scale(STATUS)).max(0);
            let code_bottom = editor_bottom
                - if self.terminal_visible {
                    self.scale(self.terminal_height)
                } else {
                    0
                };
            let editor_left = self.editor_left();
            let bg = CreateSolidBrush(EDITOR_BG);
            let gutter_bg = CreateSolidBrush(EDITOR_BG);
            let status_bg = CreateSolidBrush(STATUS_BG);
            let selection_bg = CreateSolidBrush(SELECT_BG);
            FillRect(
                hdc,
                &RECT {
                    left: editor_left,
                    top: 0,
                    right: rect.right,
                    bottom: editor_bottom,
                },
                bg,
            );
            FillRect(
                hdc,
                &RECT {
                    left: editor_left,
                    top: self.scale(TAB_HEIGHT),
                    right: editor_left + self.scale(GUTTER),
                    bottom: editor_bottom,
                },
                gutter_bg,
            );
            let tab_bg = CreateSolidBrush(TAB_BG);
            let active_bg = CreateSolidBrush(ACTIVE_BG);
            FillRect(
                hdc,
                &RECT {
                    left: editor_left,
                    top: 0,
                    right: rect.right,
                    bottom: self.scale(TAB_HEIGHT),
                },
                tab_bg,
            );
            Self::fill(
                hdc,
                RECT {
                    left: 0,
                    top: 0,
                    right: self.scale(RAIL),
                    bottom: editor_bottom,
                },
                RAIL_BG,
            );
            if self.sidebar_width > 0 {
                Self::fill(
                    hdc,
                    RECT {
                        left: self.scale(RAIL),
                        top: 0,
                        right: editor_left,
                        bottom: editor_bottom,
                    },
                    SIDEBAR_BG,
                );
            }
            Self::fill(
                hdc,
                RECT {
                    left: editor_left,
                    top: self.scale(TAB_HEIGHT),
                    right: rect.right,
                    bottom: self.scale(TAB_HEIGHT + BREADCRUMB_HEIGHT),
                },
                ACTIVE_BG,
            );
            Self::fill(
                hdc,
                RECT {
                    left: editor_left,
                    top: self.scale(TAB_HEIGHT + BREADCRUMB_HEIGHT - 1),
                    right: rect.right,
                    bottom: self.scale(TAB_HEIGHT + BREADCRUMB_HEIGHT),
                },
                EDGE,
            );
            for pane in 0..if self.split_visible { 2 } else { 1 } {
                let left = self.pane_left(hwnd, pane);
                let right = self.pane_right(hwnd, pane);
                let tab_index = self.tab_for_pane(pane);
                let path_part = self.tabs[tab_index]
                    .document
                    .path
                    .as_deref()
                    .and_then(Path::parent)
                    .and_then(Path::file_name)
                    .map(|part| part.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Editor".into());
                Self::label(
                    hdc,
                    &format!("{}  ›  {}", path_part, self.tab_label(tab_index)),
                    left + self.scale(18),
                    self.scale(TAB_HEIGHT + 3),
                    if pane == self.focused_pane {
                        TEXT
                    } else {
                        MUTED
                    },
                    RECT {
                        left,
                        top: self.scale(TAB_HEIGHT),
                        right,
                        bottom: self.editor_top(),
                    },
                );
                if self.split_visible && pane == self.focused_pane {
                    Self::fill(
                        hdc,
                        RECT {
                            left,
                            top: self.scale(TAB_HEIGHT + BREADCRUMB_HEIGHT - 2),
                            right,
                            bottom: self.scale(TAB_HEIGHT + BREADCRUMB_HEIGHT),
                        },
                        BLUE,
                    );
                }
            }
            let tab_width = self.scale(TAB_WIDTH);
            let tab_height = self.scale(TAB_HEIGHT);
            for slot in 0..self.visible_tab_count(hwnd) {
                let index = self.tab_first + slot;
                if index >= self.tabs.len() {
                    break;
                }
                let left = editor_left + slot as i32 * tab_width;
                if left >= rect.right {
                    break;
                }
                let bounds = RECT {
                    left,
                    top: 0,
                    right: (left + tab_width).min(rect.right),
                    bottom: tab_height,
                };
                if index == self.active {
                    FillRect(hdc, &bounds, active_bg);
                    Self::fill(
                        hdc,
                        RECT {
                            left,
                            top: 0,
                            right: bounds.right,
                            bottom: self.scale(2),
                        },
                        VIOLET,
                    );
                }
                let tab_icon = self.tabs[index]
                    .document
                    .path
                    .as_deref()
                    .map(|path| material_icon_for(path, false, false))
                    .unwrap_or("file");
                self.icons.draw(
                    hdc,
                    tab_icon,
                    left + self.scale(11),
                    self.scale(9),
                    self.scale(18),
                );
                let label = self.tab_label(index);
                let chars: Vec<u16> = label.encode_utf16().collect();
                SetTextColor(hdc, if index == self.active { TEXT } else { MUTED });
                let clip = RECT {
                    left: left + self.scale(37),
                    top: 0,
                    right: (left + tab_width - self.scale(30)).min(rect.right),
                    bottom: tab_height,
                };
                ExtTextOutW(
                    hdc,
                    clip.left,
                    self.scale(5),
                    ETO_CLIPPED,
                    &clip,
                    chars.as_ptr(),
                    chars.len() as u32,
                    null(),
                );
                let close = wide("×");
                TextOutW(
                    hdc,
                    left + tab_width - self.scale(23),
                    self.scale(5),
                    close.as_ptr(),
                    1,
                );
            }
            if self
                .doc()
                .path
                .as_deref()
                .and_then(Path::extension)
                .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
                && editor_left
                    + self.scale(TAB_WIDTH) * self.tabs.len().saturating_sub(self.tab_first) as i32
                    + self.scale(12)
                    < rect.right - self.scale(326)
            {
                let left = rect.right - self.scale(326);
                let brush = CreateSolidBrush(GREEN);
                let pen = CreatePen(PS_SOLID, 1, GREEN);
                let old_brush = SelectObject(hdc, brush);
                let old_pen = SelectObject(hdc, pen);
                let points = [
                    POINT {
                        x: left + self.scale(9),
                        y: self.scale(11),
                    },
                    POINT {
                        x: left + self.scale(9),
                        y: self.scale(27),
                    },
                    POINT {
                        x: left + self.scale(23),
                        y: self.scale(19),
                    },
                ];
                Polygon(hdc, points.as_ptr(), 3);
                SelectObject(hdc, old_brush);
                SelectObject(hdc, old_pen);
                DeleteObject(brush);
                DeleteObject(pen);
            }
            if editor_left
                + self.scale(TAB_WIDTH) * self.tabs.len().saturating_sub(self.tab_first) as i32
                + self.scale(12)
                < rect.right - self.scale(285)
            {
                Self::label(
                    hdc,
                    "⌕  Quick Open  Ctrl+P",
                    rect.right - self.scale(285),
                    self.scale(7),
                    MUTED,
                    RECT {
                        left: rect.right - self.scale(285),
                        top: 0,
                        right: rect.right - self.scale(112),
                        bottom: self.scale(TAB_HEIGHT),
                    },
                );
            }
            Self::label(
                hdc,
                if self.split_visible {
                    "×  Unsplit"
                } else {
                    "▥  Split"
                },
                rect.right - self.scale(107),
                self.scale(7),
                MUTED,
                RECT {
                    left: rect.right - self.scale(112),
                    top: 0,
                    right: rect.right,
                    bottom: self.scale(TAB_HEIGHT),
                },
            );
            self.paint_rail(hdc, editor_bottom);
            let sidebar_state = SaveDC(hdc);
            IntersectClipRect(hdc, self.scale(RAIL), 0, editor_left, editor_bottom);
            if self.sidebar_width > 0 && self.side_view != SideView::Files {
                self.paint_side_panel(hdc, editor_left, editor_bottom);
            }
            if self.sidebar_width > 0 && self.side_view == SideView::Files {
                let sidebar_clip = RECT {
                    left: self.scale(RAIL),
                    top: 0,
                    right: editor_left,
                    bottom: editor_bottom,
                };
                Self::fill(
                    hdc,
                    RECT {
                        left: self.scale(RAIL + 12),
                        top: self.scale(15),
                        right: self.scale(RAIL + 23),
                        bottom: self.scale(26),
                    },
                    EDGE,
                );
                Self::fill(
                    hdc,
                    RECT {
                        left: self.scale(RAIL + 14),
                        top: self.scale(17),
                        right: self.scale(RAIL + 21),
                        bottom: self.scale(24),
                    },
                    SIDEBAR_BG,
                );
                // Collapse-all-folders glyph: a single dash inside the button square.
                Self::fill(
                    hdc,
                    RECT {
                        left: self.scale(RAIL + 15),
                        top: self.scale(20),
                        right: self.scale(RAIL + 20),
                        bottom: self.scale(21),
                    },
                    MUTED,
                );
                Self::label(
                    hdc,
                    "×",
                    editor_left - self.scale(25),
                    self.scale(8),
                    MUTED,
                    sidebar_clip,
                );
                Self::fill(
                    hdc,
                    RECT {
                        left: self.scale(RAIL),
                        top: self.scale(39),
                        right: editor_left,
                        bottom: self.scale(40),
                    },
                    EDGE,
                );
                if let Some(root) = &self.workspace_root {
                    let root_name = root
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| display_path(root));
                    self.chevron(hdc, self.scale(RAIL + 16), self.scale(58), true);
                    self.icons.draw(
                        hdc,
                        "folder-open",
                        self.scale(RAIL + 23),
                        self.scale(48),
                        self.scale(17),
                    );
                    Self::label(
                        hdc,
                        &root_name,
                        self.scale(RAIL + 43),
                        self.scale(49),
                        TEXT,
                        sidebar_clip,
                    );
                    for (row, item) in self
                        .explorer_rows()
                        .iter()
                        .enumerate()
                        .skip(self.explorer_first_row)
                    {
                        let top = self.scale(
                            EXPLORER_TOP + (row - self.explorer_first_row) as i32 * EXPLORER_ROW,
                        );
                        if top >= editor_bottom - self.scale(38) {
                            break;
                        }
                        let selected = self
                            .doc()
                            .path
                            .as_deref()
                            .is_some_and(|path| path == item.entry.path);
                        if selected {
                            Self::rounded_fill(
                                hdc,
                                RECT {
                                    left: self.scale(RAIL + 7),
                                    top,
                                    right: editor_left - self.scale(8),
                                    bottom: top + self.scale(EXPLORER_ROW - 2),
                                },
                                self.scale(7),
                                SELECT_BG,
                            );
                        }
                        let name = item
                            .entry
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy();
                        let left = self.scale(RAIL + 10 + item.depth.min(6) as i32 * 13);
                        if item.entry.is_dir {
                            self.chevron(
                                hdc,
                                left + self.scale(5),
                                top + self.scale(EXPLORER_ROW / 2),
                                item.expanded,
                            );
                        }
                        let icon_name =
                            material_icon_for(&item.entry.path, item.entry.is_dir, item.expanded);
                        if !self.icons.draw(
                            hdc,
                            icon_name,
                            left + self.scale(12),
                            top + self.scale(2),
                            self.scale(18),
                        ) {
                            Self::fill(
                                hdc,
                                RECT {
                                    left: left + self.scale(16),
                                    top: top + self.scale(8),
                                    right: left + self.scale(22),
                                    bottom: top + self.scale(14),
                                },
                                MUTED,
                            );
                        }
                        Self::label(
                            hdc,
                            &name,
                            left + self.scale(36),
                            top + self.scale(1),
                            if selected {
                                TEXT
                            } else if item.entry.is_dir {
                                MUTED
                            } else {
                                rgb(185, 205, 230)
                            },
                            RECT {
                                left: left + self.scale(36),
                                top,
                                right: editor_left - self.scale(10),
                                bottom: top + self.scale(EXPLORER_ROW),
                            },
                        );
                    }
                } else {
                    Self::label(
                        hdc,
                        "Open a file to browse",
                        self.scale(RAIL + 16),
                        self.scale(49),
                        MUTED,
                        sidebar_clip,
                    );
                    Self::label(
                        hdc,
                        "its folder  (Ctrl+O)",
                        self.scale(RAIL + 16),
                        self.scale(73),
                        MUTED,
                        sidebar_clip,
                    );
                }
                Self::fill(
                    hdc,
                    RECT {
                        left: self.scale(RAIL),
                        top: editor_bottom - self.scale(35),
                        right: editor_left,
                        bottom: editor_bottom,
                    },
                    SIDEBAR_BG,
                );
                if self.doc().path.is_some() {
                    let chip_left = self.scale(RAIL + 7);
                    Self::fill(
                        hdc,
                        RECT {
                            left: chip_left,
                            top: editor_bottom - self.scale(30),
                            right: (chip_left + self.scale(115)).min(editor_left),
                            bottom: editor_bottom,
                        },
                        ACTIVE_BG,
                    );
                    Self::fill(
                        hdc,
                        RECT {
                            left: chip_left,
                            top: editor_bottom - self.scale(30),
                            right: (chip_left + self.scale(115)).min(editor_left),
                            bottom: editor_bottom - self.scale(29),
                        },
                        BLUE,
                    );
                    let icon = self
                        .doc()
                        .path
                        .as_deref()
                        .map(|path| material_icon_for(path, false, false))
                        .unwrap_or("file");
                    self.icons.draw(
                        hdc,
                        icon,
                        chip_left + self.scale(6),
                        editor_bottom - self.scale(24),
                        self.scale(15),
                    );
                    Self::label(
                        hdc,
                        &self.tab_label(self.active),
                        chip_left + self.scale(24),
                        editor_bottom - self.scale(24),
                        MUTED,
                        sidebar_clip,
                    );
                }
            }
            RestoreDC(hdc, sidebar_state);
            if self.side_view == SideView::Review && self.review_file.is_some() {
                self.paint_diff(hdc, editor_left, rect.right, code_bottom);
            } else {
                let divider = if self.split_visible {
                    self.pane_divider(hwnd)
                } else {
                    rect.right
                };
                self.paint_code_pane(
                    hdc,
                    hwnd,
                    0,
                    RECT {
                        left: editor_left,
                        top: self.editor_top(),
                        right: divider,
                        bottom: code_bottom,
                    },
                    selection_bg,
                );
                if self.split_visible {
                    self.paint_code_pane(
                        hdc,
                        hwnd,
                        1,
                        RECT {
                            left: divider,
                            top: self.editor_top(),
                            right: rect.right,
                            bottom: code_bottom,
                        },
                        selection_bg,
                    );
                    Self::fill(
                        hdc,
                        RECT {
                            left: divider - self.scale(1),
                            top: self.scale(TAB_HEIGHT),
                            right: divider + self.scale(1),
                            bottom: code_bottom,
                        },
                        EDGE,
                    );
                }
            }
            self.paint_search_preview(
                hdc,
                self.pane_left(hwnd, self.focused_pane),
                self.pane_right(hwnd, self.focused_pane),
                code_bottom,
            );
            if self.terminal_visible {
                self.paint_terminal(hdc, editor_left, rect.right, editor_bottom);
            }
            FillRect(
                hdc,
                &RECT {
                    left: 0,
                    top: editor_bottom,
                    right: rect.right,
                    bottom: rect.bottom,
                },
                status_bg,
            );
            SelectObject(hdc, self.ui_font);
            let right_label = if self.side_view == SideView::Review && self.review_file.is_some() {
                format!("Git review     {} lines", self.diff_rows.len())
            } else {
                let issues = self
                    .tab()
                    .diagnostics
                    .iter()
                    .filter(|item| item.severity <= 2)
                    .count();
                format!(
                    "{}Ln {}, Col {}     UTF-8     {}",
                    if issues == 0 {
                        String::new()
                    } else {
                        format!("{issues} issues     ")
                    },
                    self.view().cursor.line + 1,
                    self.doc().line(self.view().cursor.line)[..self.view().cursor.byte]
                        .chars()
                        .count()
                        + 1,
                    self.doc()
                        .path
                        .as_deref()
                        .and_then(Path::extension)
                        .map(|ext| ext.to_string_lossy().to_uppercase())
                        .unwrap_or_else(|| "TEXT".into())
                )
            };
            let right_width = self.text_width(hdc, &right_label);
            let right_x = (rect.right - right_width - self.scale(16)).max(self.scale(16));
            let cursor = self.view().cursor;
            let status_message = self
                .tab()
                .diagnostics
                .iter()
                .find(|item| {
                    (item.range.start.line as usize..=item.range.end.line as usize)
                        .contains(&cursor.line)
                })
                .and_then(|item| item.message.lines().next())
                .unwrap_or(&self.status);
            Self::label(
                hdc,
                status_message,
                self.scale(14),
                editor_bottom + self.scale(4),
                TEXT,
                RECT {
                    left: self.scale(14),
                    top: editor_bottom,
                    right: (right_x - self.scale(24)).max(self.scale(14)),
                    bottom: rect.bottom,
                },
            );
            Self::label(
                hdc,
                &right_label,
                right_x,
                editor_bottom + self.scale(4),
                MUTED,
                RECT {
                    left: right_x,
                    top: editor_bottom,
                    right: rect.right,
                    bottom: rect.bottom,
                },
            );
            DeleteObject(bg);
            DeleteObject(gutter_bg);
            DeleteObject(status_bg);
            DeleteObject(selection_bg);
            DeleteObject(tab_bg);
            DeleteObject(active_bg);
            if let Some(transition) = &self.transition {
                let elapsed = transition.started.elapsed().as_millis();
                if elapsed < TRANSITION_MS
                    && transition.left == editor_left
                    && transition.top == self.editor_top()
                    && transition.previous_frame.width == rect.right - editor_left
                    && transition.previous_frame.height == editor_bottom - self.editor_top()
                {
                    let left = editor_left;
                    let top = self.editor_top();
                    let width = (rect.right - left).max(0);
                    let height = (editor_bottom - top).max(0);
                    AlphaBlend(
                        hdc,
                        left,
                        top,
                        width,
                        height,
                        transition.previous_frame.dc,
                        0,
                        0,
                        width,
                        height,
                        BLENDFUNCTION {
                            BlendOp: AC_SRC_OVER as u8,
                            BlendFlags: 0,
                            SourceConstantAlpha: ((TRANSITION_MS - elapsed) * 255 / TRANSITION_MS)
                                as u8,
                            AlphaFormat: 0,
                        },
                    );
                } else {
                    self.transition = None;
                    KillTimer(hwnd, 3);
                }
            }
            SelectObject(hdc, self.ui_font);
            self.paint_quick_open(hdc, rect);
            self.paint_hover_card(hdc, rect, editor_bottom);
            self.paint_completion(hdc, rect, editor_bottom);
            SelectObject(hdc, old_font);
            if hdc != window_dc {
                BitBlt(window_dc, 0, 0, rect.right, rect.bottom, hdc, 0, 0, SRCCOPY);
            }
            EndPaint(hwnd, &ps);
        }
    }
}
