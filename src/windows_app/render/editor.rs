use super::super::*;

// The status bar names the language the way an editor does ("Rust", not "RS"),
// falling back to the bare extension for types we have no display name for.
fn language_label(path: Option<&Path>) -> String {
    let extension = path
        .and_then(Path::extension)
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("rs") => "Rust".into(),
        Some("py" | "pyw") => "Python".into(),
        Some("toml") => "TOML".into(),
        Some("json" | "jsonc") => "JSON".into(),
        Some("md" | "markdown") => "Markdown".into(),
        Some("yaml" | "yml") => "YAML".into(),
        Some("js" | "mjs" | "jsx") => "JavaScript".into(),
        Some("ts" | "tsx") => "TypeScript".into(),
        Some("html" | "htm") => "HTML".into(),
        Some("css") => "CSS".into(),
        Some("c" | "h") => "C".into(),
        Some("cc" | "cpp" | "cxx" | "hpp") => "C++".into(),
        Some(other) => other.to_uppercase(),
        None => "Plain Text".into(),
    }
}

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
            let gap = self.chrome_gap();
            let chrome_top = self.chrome_top();
            let tab_strip_bottom = self.tab_strip_bottom();
            let card_bottom = editor_bottom - gap;
            let card_radius = self.scale(CARD_RADIUS);
            let bg = CreateSolidBrush(EDITOR_BG);
            let gutter_bg = CreateSolidBrush(EDITOR_BG);
            let status_bg = CreateSolidBrush(STATUS_BG);
            let selection_bg = CreateSolidBrush(SELECT_BG);
            // The backdrop the cards float on. The rail stays flush to the
            // window edge; only the side panel and editor become cards.
            Self::fill(hdc, rect, SHELL_BG);
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
                let panel = RECT {
                    left: self.scale(RAIL) + gap,
                    top: chrome_top,
                    right: self.sidebar_right(),
                    bottom: card_bottom,
                };
                self.panel_card(hdc, panel, card_radius, CARD_EDGE, SIDEBAR_BG);
            }
            // Editor card: the rounded body first, then square-filled regions
            // inside it for the gutter and tab strip, which stay clear of the
            // rounded corners.
            let editor_card = RECT {
                left: editor_left,
                top: chrome_top,
                right: self.editor_right(hwnd),
                bottom: card_bottom,
            };
            self.panel_card(hdc, editor_card, card_radius, CARD_EDGE, EDITOR_BG);
            FillRect(
                hdc,
                &RECT {
                    left: editor_left + card_radius,
                    top: tab_strip_bottom,
                    right: editor_card.right - card_radius,
                    bottom: card_bottom - self.scale(1).max(1),
                },
                bg,
            );
            FillRect(
                hdc,
                &RECT {
                    left: editor_left + self.scale(1).max(1),
                    top: tab_strip_bottom,
                    right: editor_left + self.scale(GUTTER),
                    bottom: card_bottom - self.scale(1).max(1),
                },
                gutter_bg,
            );
            let tab_bg = CreateSolidBrush(TAB_BG);
            let active_bg = CreateSolidBrush(ACTIVE_BG);
            // Tab strip rides the card's rounded top, so it is drawn as a
            // rounded fill clipped to the strip's height.
            Self::rounded_fill(
                hdc,
                RECT {
                    left: editor_left + self.scale(1).max(1),
                    top: chrome_top + self.scale(1).max(1),
                    right: editor_card.right - self.scale(1).max(1),
                    bottom: tab_strip_bottom + card_radius,
                },
                card_radius,
                TAB_BG,
            );
            FillRect(
                hdc,
                &RECT {
                    left: editor_left + self.scale(1).max(1),
                    top: tab_strip_bottom - card_radius,
                    right: editor_card.right - self.scale(1).max(1),
                    bottom: tab_strip_bottom,
                },
                tab_bg,
            );
            Self::fill(
                hdc,
                RECT {
                    left: editor_left + self.scale(1).max(1),
                    top: tab_strip_bottom,
                    right: editor_card.right - self.scale(1).max(1),
                    bottom: tab_strip_bottom + self.scale(BREADCRUMB_HEIGHT),
                },
                ACTIVE_BG,
            );
            Self::fill(
                hdc,
                RECT {
                    left: editor_left + self.scale(1).max(1),
                    top: tab_strip_bottom + self.scale(BREADCRUMB_HEIGHT - 1),
                    right: editor_card.right - self.scale(1).max(1),
                    bottom: tab_strip_bottom + self.scale(BREADCRUMB_HEIGHT),
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
                    &format!("{}  >  {}", path_part, self.tab_label(tab_index)),
                    left + self.scale(18),
                    tab_strip_bottom + self.scale(3),
                    if pane == self.focused_pane {
                        TEXT
                    } else {
                        MUTED
                    },
                    RECT {
                        left,
                        top: tab_strip_bottom,
                        right: right - self.scale(50),
                        bottom: self.editor_top(),
                    },
                );
                Self::label(
                    hdc,
                    "[\u{2502}]   \u{2026}",
                    right - self.scale(48),
                    tab_strip_bottom + self.scale(3),
                    MUTED,
                    RECT {
                        left: right - self.scale(50),
                        top: tab_strip_bottom,
                        right,
                        bottom: self.editor_top(),
                    },
                );
                if self.split_visible && pane == self.focused_pane {
                    Self::fill(
                        hdc,
                        RECT {
                            left,
                            top: tab_strip_bottom + self.scale(BREADCRUMB_HEIGHT - 2),
                            right,
                            bottom: tab_strip_bottom + self.scale(BREADCRUMB_HEIGHT),
                        },
                        BLUE,
                    );
                }
            }
            let tab_width = self.scale(TAB_WIDTH);
            for slot in 0..self.visible_tab_count(hwnd) {
                let index = self.tab_first + slot;
                if index >= self.tabs.len() {
                    break;
                }
                let left = editor_left + slot as i32 * tab_width;
                if left >= editor_card.right {
                    break;
                }
                let bounds = RECT {
                    left,
                    top: chrome_top,
                    right: (left + tab_width).min(editor_card.right),
                    bottom: tab_strip_bottom,
                };
                if index == self.active {
                    FillRect(hdc, &bounds, active_bg);
                    Self::fill(
                        hdc,
                        RECT {
                            left,
                            top: chrome_top,
                            right: bounds.right,
                            bottom: chrome_top + self.scale(2),
                        },
                        VIOLET,
                    );
                }
                let tab_icon = if self.has_extension("material-icons") {
                    self.tabs[index]
                        .document
                        .path
                        .as_deref()
                        .map(|path| material_icon_for(path, false, false))
                        .unwrap_or("file")
                } else {
                    "file"
                };
                self.icons.draw(
                    hdc,
                    tab_icon,
                    left + self.scale(11),
                    chrome_top + self.scale(9),
                    self.scale(18),
                );
                let label = self.tab_label(index);
                let chars: Vec<u16> = label.encode_utf16().collect();
                SetTextColor(hdc, if index == self.active { TEXT } else { MUTED });
                let clip = RECT {
                    left: left + self.scale(37),
                    top: chrome_top,
                    right: (left + tab_width - self.scale(30)).min(editor_card.right),
                    bottom: tab_strip_bottom,
                };
                ExtTextOutW(
                    hdc,
                    clip.left,
                    chrome_top + self.scale(5),
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
                    chrome_top + self.scale(5),
                    close.as_ptr(),
                    1,
                );
            }
            if Tab::is_runnable(self.doc())
                && editor_left
                    + self.scale(TAB_WIDTH) * self.tabs.len().saturating_sub(self.tab_first) as i32
                    + self.scale(12)
                    < editor_card.right - self.scale(326)
            {
                let left = editor_card.right - self.scale(326);
                let brush = CreateSolidBrush(GREEN);
                let pen = CreatePen(PS_SOLID, 1, GREEN);
                let old_brush = SelectObject(hdc, brush);
                let old_pen = SelectObject(hdc, pen);
                let points = [
                    POINT {
                        x: left + self.scale(9),
                        y: chrome_top + self.scale(11),
                    },
                    POINT {
                        x: left + self.scale(9),
                        y: chrome_top + self.scale(27),
                    },
                    POINT {
                        x: left + self.scale(23),
                        y: chrome_top + self.scale(19),
                    },
                ];
                Polygon(hdc, points.as_ptr(), 3);
                SelectObject(hdc, old_brush);
                SelectObject(hdc, old_pen);
                DeleteObject(brush);
                DeleteObject(pen);
            }
            self.paint_rail(hdc, editor_bottom);
            let sidebar_state = SaveDC(hdc);
            // Clip the side panel to its card so its contents cannot spill
            // into the gap between the cards.
            IntersectClipRect(
                hdc,
                self.scale(RAIL) + gap,
                chrome_top,
                self.sidebar_right(),
                card_bottom,
            );
            if self.sidebar_width > 0 && self.side_view != SideView::Files {
                self.paint_side_panel(hdc, self.sidebar_right(), card_bottom);
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
                            let sel_rect = RECT {
                                left: self.scale(RAIL + 7),
                                top,
                                right: editor_left - self.scale(8),
                                bottom: top + self.scale(EXPLORER_ROW - 2),
                            };
                            self.panel_card(
                                hdc,
                                sel_rect,
                                self.scale(6),
                                rgb(48, 84, 156),
                                rgb(26, 44, 90),
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
                        let icon_name = if self.has_extension("material-icons") {
                            material_icon_for(&item.entry.path, item.entry.is_dir, item.expanded)
                        } else if item.entry.is_dir {
                            if item.expanded {
                                "folder-open"
                            } else {
                                "folder"
                            }
                        } else {
                            "file"
                        };
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
            }
            RestoreDC(hdc, sidebar_state);
            if self.side_view == SideView::Review && self.review_file.is_some() {
                self.paint_diff(hdc, editor_left, editor_card.right, code_bottom);
            } else {
                let divider = if self.split_visible {
                    self.pane_divider(hwnd)
                } else {
                    editor_card.right
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
                            right: editor_card.right,
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
                self.paint_terminal(hdc, editor_left, editor_card.right, card_bottom);
            }
            // Card borders go on last: the interior fills above are square, so
            // drawing the outlines now is what keeps the rounded corners and
            // the 1px edge from being painted over.
            if self.sidebar_width > 0 {
                self.card_outline(
                    hdc,
                    RECT {
                        left: self.scale(RAIL) + gap,
                        top: chrome_top,
                        right: self.sidebar_right(),
                        bottom: card_bottom,
                    },
                    card_radius,
                    CARD_EDGE,
                );
            }
            self.card_outline(hdc, editor_card, card_radius, CARD_EDGE);
            if self.ai_assistant_visible {
                let ai_card = RECT {
                    left: editor_card.right + gap,
                    top: chrome_top,
                    right: rect.right - gap,
                    bottom: card_bottom,
                };
                self.paint_ai_assistant(hdc, ai_card);
                self.card_outline(hdc, ai_card, card_radius, CARD_EDGE);
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
            Self::fill(
                hdc,
                RECT {
                    left: 0,
                    top: editor_bottom,
                    right: rect.right,
                    bottom: editor_bottom + self.scale(1).max(1),
                },
                EDGE,
            );
            SelectObject(hdc, self.ui_font);

            // Left file chip
            let file_label = self.tab_label(self.active);
            let chip_w = self.scale(28) + self.text_width(hdc, &file_label);
            let chip_rect = RECT {
                left: self.scale(12),
                top: editor_bottom + self.scale(4),
                right: self.scale(12) + chip_w,
                bottom: rect.bottom - self.scale(4),
            };
            Self::rounded_fill(hdc, chip_rect, self.scale(4), rgb(24, 38, 70));
            let icon = if self.has_extension("material-icons") {
                self.doc()
                    .path
                    .as_deref()
                    .map(|path| material_icon_for(path, false, false))
                    .unwrap_or("file")
            } else {
                "file"
            };
            self.icons.draw(
                hdc,
                icon,
                chip_rect.left + self.scale(6),
                chip_rect.top + self.scale(2),
                self.scale(15),
            );
            Self::label(
                hdc,
                &file_label,
                chip_rect.left + self.scale(24),
                chip_rect.top + self.scale(2),
                TEXT,
                chip_rect,
            );

            // Right side: branch and Ready status indicator
            let branch = self.git_head_label();
            let right_branch = format!("\u{2442}  {branch}");
            let right_ready = "\u{25cf}  Ready";
            let ready_width = self.text_width(hdc, right_ready);
            let branch_width = self.text_width(hdc, &right_branch);

            let ready_x = rect.right - ready_width - self.scale(16);
            let branch_x = ready_x - branch_width - self.scale(20);

            Self::label(hdc, &right_branch, branch_x, editor_bottom + self.scale(4), MUTED, rect);
            Self::label(hdc, "\u{25cf}", ready_x, editor_bottom + self.scale(4), rgb(52, 211, 153), rect);
            Self::label(hdc, "Ready", ready_x + self.scale(14), editor_bottom + self.scale(4), TEXT, rect);

            // Middle info: Ln, Col, Spaces, Encoding, Language
            let mid_info = format!(
                "Ln {}, Col {}    Spaces: 4    UTF-8    {}",
                self.view().cursor.line + 1,
                self.doc().line(self.view().cursor.line)[..self.view().cursor.byte]
                    .chars()
                    .count()
                    + 1,
                language_label(self.doc().path.as_deref()),
            );
            let mid_x = chip_rect.right + self.scale(24);
            Self::label(
                hdc,
                &mid_info,
                mid_x,
                editor_bottom + self.scale(4),
                MUTED,
                RECT {
                    left: mid_x,
                    top: editor_bottom,
                    right: (branch_x - self.scale(16)).max(mid_x),
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
