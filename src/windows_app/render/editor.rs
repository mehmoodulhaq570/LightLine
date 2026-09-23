use super::super::*;

// The status bar names the language the way an editor does ("Rust", not "RS"),
// falling back to the bare extension for types we have no display name for.
pub(in crate::windows_app) fn language_label(path: Option<&Path>) -> String {
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
    // Geometry for the trailing "{Language}" name in the bottom status bar's
    // "Ln X, Col Y  Spaces: 4  UTF-8  {Language}" line. Shared between
    // painting and the click handler (input.rs) so clicking it is guaranteed
    // to land on exactly what was drawn. Previously this whole line was a
    // single opaque label with nothing behind it to click.
    pub(in crate::windows_app) fn status_language_control(
        &self,
        hdc: HDC,
        rect: RECT,
        editor_bottom: i32,
        chip_right: i32,
        branch_x: i32,
    ) -> (i32, i32, String, RECT) {
        let s = |v: i32| self.scale(v);
        let left_bound = chip_right + s(24);
        let clip_right = (branch_x - s(16)).max(left_bound);
        let indent_label = if self.settings.insert_spaces { "Spaces" } else { "Tab Size" };
        let prefix = format!(
            "Ln {}, Col {}    {indent_label}: {}    UTF-8    ",
            self.view().cursor.line + 1,
            self.doc().line(self.view().cursor.line)[..self.view().cursor.byte]
                .chars()
                .count()
                + 1,
            self.settings.tab_size,
        );
        let prefix_width = self.text_width(hdc, &prefix);
        let language = language_label(self.doc().path.as_deref());
        let language_width = self.text_width(hdc, &language);
        let mid_x = (clip_right - prefix_width - language_width).max(left_bound);
        let language_left = (mid_x + prefix_width).min(clip_right);
        let hit_rect = RECT {
            left: language_left,
            top: editor_bottom,
            right: (language_left + language_width).min(clip_right),
            bottom: rect.bottom,
        };
        (mid_x, clip_right, prefix, hit_rect)
    }

    // Mirrors the paint-time geometry in status_language_control using a
    // throwaway HDC, the same way App::position_at_pane measures text for
    // click-to-cursor mapping outside of a paint call.
    pub(in crate::windows_app) fn click_status_language(&mut self, hwnd: HWND, rect: RECT, x: i32, y: i32) {
        let editor_bottom = (rect.bottom - self.scale(STATUS)).max(0);
        let language_rect = unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.ui_font);
            let branch = self.git_head_label();
            let left_branch = format!("\u{2442}  {branch}");
            let health = "⊗ 0    ⚠ 0";
            let left_info_right = self.scale(16)
                + self.text_width(hdc, &left_branch)
                + self.scale(24)
                + self.text_width(hdc, health);
            let ready_width = self.text_width(hdc, "\u{25cf}  Ready");
            let ready_x = rect.right - ready_width - self.scale(16);
            let (_, _, _, language_rect) =
                self.status_language_control(hdc, rect, editor_bottom, left_info_right, ready_x);
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            language_rect
        };
        if x >= language_rect.left && x < language_rect.right && y >= language_rect.top {
            self.open_language_actions(hwnd);
        }
    }

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
                self.paint_welcome(hwnd, hdc, rect);
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
            let bg = CreateSolidBrush(self.theme.editor_bg);
            let gutter_bg = CreateSolidBrush(self.theme.editor_bg);
            let status_bg = CreateSolidBrush(self.theme.status_bg);
            let selection_bg = CreateSolidBrush(self.theme.select_bg);
            // The backdrop the cards float on. The rail stays flush to the
            // window edge; only the side panel and editor become cards.
            Self::fill(hdc, rect, self.theme.shell_bg);
            Self::fill(
                hdc,
                RECT {
                    left: 0,
                    top: 0,
                    right: self.scale(RAIL),
                    bottom: editor_bottom,
                },
                self.theme.rail_bg,
            );
            // Dedicated workbench header: brand at left, command center in the
            // middle, and the native window controls remain in the OS frame.
            Self::fill(
                hdc,
                RECT {
                    left: 0,
                    top: 0,
                    right: rect.right,
                    bottom: chrome_top,
                },
                self.theme.tab_bg,
            );
            Self::fill(
                hdc,
                RECT {
                    left: 0,
                    top: chrome_top - self.scale(1).max(1),
                    right: rect.right,
                    bottom: chrome_top,
                },
                self.theme.edge,
            );
            DrawIconEx(
                hdc,
                self.scale(14),
                self.scale(9),
                self.brand_icon,
                self.scale(32),
                self.scale(32),
                0,
                null_mut(),
                DI_NORMAL,
            );
            SelectObject(hdc, self.brand_font);
            Self::label(
                hdc,
                "LightLine",
                self.scale(56),
                self.scale(12),
                self.theme.text,
                RECT {
                    left: self.scale(56),
                    top: 0,
                    right: self.scale(160),
                    bottom: chrome_top,
                },
            );
            Self::rounded_fill(
                hdc,
                RECT {
                    left: self.scale(132),
                    top: self.scale(15),
                    right: self.scale(164),
                    bottom: self.scale(35),
                },
                self.scale(5),
                rgb(63, 47, 150),
            );
            SelectObject(hdc, self.ui_font);
            Self::label(
                hdc,
                "IDE",
                self.scale(138),
                self.scale(15),
                self.theme.text,
                RECT {
                    left: self.scale(132),
                    top: 0,
                    right: self.scale(164),
                    bottom: chrome_top,
                },
            );
            let title_button = self.scale(46);
            let controls_left = rect.right - title_button * 3;
            let controls_mid_y = chrome_top / 2;
            self.stroke(hdc, self.theme.muted, |hdc| {
                // Minimize
                MoveToEx(
                    hdc,
                    controls_left + self.scale(17),
                    controls_mid_y + self.scale(5),
                    null_mut(),
                );
                LineTo(
                    hdc,
                    controls_left + self.scale(29),
                    controls_mid_y + self.scale(5),
                );
                // Maximize / restore
                let max_left = controls_left + title_button + self.scale(17);
                Rectangle(
                    hdc,
                    max_left,
                    controls_mid_y - self.scale(6),
                    max_left + self.scale(12),
                    controls_mid_y + self.scale(6),
                );
                // Close
                let close_left = controls_left + title_button * 2 + self.scale(17);
                MoveToEx(hdc, close_left, controls_mid_y - self.scale(6), null_mut());
                LineTo(
                    hdc,
                    close_left + self.scale(12),
                    controls_mid_y + self.scale(6),
                );
                MoveToEx(hdc, close_left + self.scale(12), controls_mid_y - self.scale(6), null_mut());
                LineTo(hdc, close_left, controls_mid_y + self.scale(6));
            });
            if self.sidebar_width > 0 {
                let panel = RECT {
                    left: self.scale(RAIL) + gap,
                    top: chrome_top,
                    right: self.sidebar_right(),
                    bottom: card_bottom,
                };
                self.panel_card(hdc, panel, card_radius, self.theme.card_edge, self.theme.sidebar_bg);
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
            self.panel_card(hdc, editor_card, card_radius, self.theme.card_edge, self.theme.editor_bg);
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
            let tab_bg = CreateSolidBrush(self.theme.tab_bg);
            let active_bg = CreateSolidBrush(self.theme.active_bg);
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
                self.theme.tab_bg,
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
                self.theme.active_bg,
            );
            Self::fill(
                hdc,
                RECT {
                    left: editor_left + self.scale(1).max(1),
                    top: tab_strip_bottom + self.scale(BREADCRUMB_HEIGHT - 1),
                    right: editor_card.right - self.scale(1).max(1),
                    bottom: tab_strip_bottom + self.scale(BREADCRUMB_HEIGHT),
                },
                self.theme.edge,
            );
            for pane in 0..if self.split_visible { 2 } else { 1 } {
                let left = self.pane_left(hwnd, pane);
                let right = self.pane_right(hwnd, pane);
                // Both glyphs below are centered inside the rects the mouse
                // handler tests, so the control a click lands on is the control
                // actually drawn there.
                let (split_rect, more_rect) = self.pane_actions(right);
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
                        self.theme.text
                    } else {
                        self.theme.muted
                    },
                    RECT {
                        left,
                        top: tab_strip_bottom,
                        right: split_rect.left - self.scale(6),
                        bottom: self.editor_top(),
                    },
                );
                for (rect, glyph) in [(&split_rect, "[\u{2502}]"), (&more_rect, "\u{2026}")] {
                    let glyph_x =
                        rect.left + (rect.right - rect.left - self.text_width(hdc, glyph)) / 2;
                    Self::label(
                        hdc,
                        glyph,
                        glyph_x,
                        tab_strip_bottom + self.scale(3),
                        self.theme.muted,
                        RECT {
                            left: split_rect.left,
                            top: tab_strip_bottom,
                            right,
                            bottom: self.editor_top(),
                        },
                    );
                }
                if self.split_visible && pane == self.focused_pane {
                    Self::fill(
                        hdc,
                        RECT {
                            left,
                            top: tab_strip_bottom + self.scale(BREADCRUMB_HEIGHT - 2),
                            right,
                            bottom: tab_strip_bottom + self.scale(BREADCRUMB_HEIGHT),
                        },
                        self.theme.blue,
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
                        self.theme.violet,
                    );
                }
                let use_theme = self.has_extension("material-icons");
                match self.tabs[index].document.path.as_deref() {
                    Some(path) => {
                        if !self.icons.draw_for_path(
                            hdc,
                            path,
                            false,
                            false,
                            use_theme,
                            left + self.scale(11),
                            chrome_top + self.scale(9),
                            self.scale(18),
                        ) {
                            self.draw_vector_file(
                                hdc,
                                path,
                                left + self.scale(11),
                                chrome_top + self.scale(9),
                                self.scale(18),
                            );
                        }
                    }
                    None => {
                        if !self.icons.draw_generic(
                            hdc,
                            GenericIcon::File,
                            left + self.scale(11),
                            chrome_top + self.scale(9),
                            self.scale(18),
                        ) {
                            self.draw_vector_file(
                                hdc,
                                std::path::Path::new("untitled"),
                                left + self.scale(11),
                                chrome_top + self.scale(9),
                                self.scale(18),
                            );
                        }
                    }
                }
                let label = self.tab_label(index);
                let chars: Vec<u16> = label.encode_utf16().collect();
                SetTextColor(hdc, if index == self.active { self.theme.text } else { self.theme.muted });
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
                    < editor_card.right - self.scale(92)
            {
                let left = editor_card.right - self.scale(78);
                let brush = CreateSolidBrush(self.theme.green);
                let pen = CreatePen(PS_SOLID, 1, self.theme.green);
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

            // Persistent command center: a compact, clickable Ctrl+P surface
            // matching the approved workbench mockup.
            let command_rect = self.command_center_rect(hwnd);
            if command_rect.right > command_rect.left {
                self.panel_card(
                    hdc,
                    command_rect,
                    self.scale(6),
                    rgb(43, 76, 132),
                    rgb(12, 25, 48),
                );
                self.rail_icon(
                    hdc,
                    1,
                    command_rect.left + self.scale(10),
                    command_rect.top + self.scale(6),
                    self.theme.muted,
                );
                Self::label(
                    hdc,
                    "Search files, symbols, commands...",
                    command_rect.left + self.scale(36),
                    command_rect.top + self.scale(5),
                    self.theme.muted,
                    RECT {
                        left: command_rect.left + self.scale(36),
                        top: command_rect.top,
                        right: command_rect.right - self.scale(55),
                        bottom: command_rect.bottom,
                    },
                );
                let key_rect = RECT {
                    left: command_rect.right - self.scale(48),
                    top: command_rect.top + self.scale(4),
                    right: command_rect.right - self.scale(7),
                    bottom: command_rect.bottom - self.scale(4),
                };
                Self::rounded_fill(hdc, key_rect, self.scale(4), rgb(25, 43, 76));
                Self::label(
                    hdc,
                    "Ctrl P",
                    key_rect.left + self.scale(5),
                    key_rect.top + self.scale(1),
                    self.theme.text,
                    key_rect,
                );
            }
            let rail_state = SaveDC(hdc);
            SetViewportOrgEx(hdc, 0, chrome_top, null_mut());
            self.paint_rail(hdc, editor_bottom - chrome_top);
            RestoreDC(hdc, rail_state);
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
            SetViewportOrgEx(hdc, 0, chrome_top, null_mut());
            let sidebar_bottom = card_bottom - chrome_top;
            if self.sidebar_width > 0 && self.side_view != SideView::Files {
                self.paint_side_panel(hdc, self.sidebar_right(), sidebar_bottom);
            }
            if self.sidebar_width > 0 && self.side_view == SideView::Files {
                if let Some(root) = self.workspace_root.clone()
                    && !self.directory_cache.contains_key(&root)
                {
                    self.load_directory(&root);
                }
                let sidebar_clip = RECT {
                    left: self.scale(RAIL),
                    top: 0,
                    right: editor_left,
                    bottom: sidebar_bottom,
                };
                Self::label(
                    hdc,
                    "EXPLORER",
                    self.scale(RAIL + 16),
                    self.scale(11),
                    self.theme.muted,
                    sidebar_clip,
                );
                // Collapse all subfolders icon at top right
                let collapse_all_rect = RECT {
                    left: editor_left - self.scale(48),
                    top: self.scale(10),
                    right: editor_left - self.scale(28),
                    bottom: self.scale(30),
                };
                self.draw_collapse_all_icon(hdc, collapse_all_rect, self.theme.muted);
                // Collapse sidebar button at top right
                let collapse_rect = RECT {
                    left: editor_left - self.scale(26),
                    top: self.scale(10),
                    right: editor_left - self.scale(6),
                    bottom: self.scale(30),
                };
                self.draw_close_icon(hdc, collapse_rect, self.theme.muted);
                Self::fill(
                    hdc,
                    RECT {
                        left: self.scale(RAIL),
                        top: self.scale(39),
                        right: editor_left,
                        bottom: self.scale(40),
                    },
                    self.theme.edge,
                );
                if let Some(root) = &self.workspace_root {
                    let root_name = root
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| display_path(root));
                    let root_expanded = self.expanded_dirs.contains(root);
                    self.chevron(hdc, self.scale(RAIL + 16), self.scale(58), root_expanded);
                    let root_clip = RECT {
                        left: self.scale(RAIL + 28),
                        top: self.scale(40),
                        right: editor_left - self.scale(96),
                        bottom: self.scale(70),
                    };
                    Self::label(
                        hdc,
                        &root_name,
                        self.scale(RAIL + 28),
                        self.scale(49),
                        self.theme.text,
                        root_clip,
                    );
                    // Toolbar action buttons on the workspace header
                    let s = |v: i32| self.scale(v);
                    let btn_y = s(48);
                    let btn_h = s(20);

                    // New File button
                    let rect_file = RECT {
                        left: editor_left - s(92),
                        top: btn_y,
                        right: editor_left - s(72),
                        bottom: btn_y + btn_h,
                    };
                    self.draw_new_file_icon(hdc, rect_file, self.theme.muted);

                    // New Folder button
                    let rect_folder = RECT {
                        left: editor_left - s(70),
                        top: btn_y,
                        right: editor_left - s(50),
                        bottom: btn_y + btn_h,
                    };
                    self.draw_new_folder_icon(hdc, rect_folder, self.theme.muted);

                    // Refresh button
                    let rect_refresh = RECT {
                        left: editor_left - s(48),
                        top: btn_y,
                        right: editor_left - s(28),
                        bottom: btn_y + btn_h,
                    };
                    self.draw_refresh_icon(hdc, rect_refresh, self.theme.muted);

                    // Close Workspace button
                    let rect_close = RECT {
                        left: editor_left - s(26),
                        top: btn_y,
                        right: editor_left - s(6),
                        bottom: btn_y + btn_h,
                    };
                    self.draw_close_icon(hdc, rect_close, self.theme.muted);

                    let input_is_new = self.explorer_input.as_ref().is_some_and(|inp| !inp.is_rename);
                    if let Some(input) = &self.explorer_input && !input.is_rename {
                        let top = self.scale(EXPLORER_TOP);
                        let input_rect = RECT {
                            left: self.scale(RAIL + 20),
                            top,
                            right: editor_left - self.scale(8),
                            bottom: top + self.scale(EXPLORER_ROW - 2),
                        };
                        self.panel_card(
                            hdc,
                            input_rect,
                            self.scale(4),
                            self.theme.blue,
                            rgb(16, 26, 48),
                        );
                        if input.is_folder {
                            if !self.icons.draw_generic(
                                hdc,
                                GenericIcon::FolderOpen,
                                self.scale(RAIL + 25),
                                top + self.scale(2),
                                self.scale(18),
                            ) {
                                self.draw_vector_folder(
                                    hdc,
                                    self.scale(RAIL + 25),
                                    top + self.scale(2),
                                    self.scale(18),
                                    true,
                                );
                            }
                        } else {
                            if !self.icons.draw_generic(
                                hdc,
                                GenericIcon::File,
                                self.scale(RAIL + 25),
                                top + self.scale(2),
                                self.scale(18),
                            ) {
                                self.draw_vector_file(
                                    hdc,
                                    std::path::Path::new(&input.buffer),
                                    self.scale(RAIL + 25),
                                    top + self.scale(2),
                                    self.scale(18),
                                );
                            }
                        }
                        let text_x = self.scale(RAIL + 49);
                        Self::label(
                            hdc,
                            &input.buffer,
                            text_x,
                            top + self.scale(1),
                            self.theme.text,
                            input_rect,
                        );
                        let text_w = self.text_width(hdc, &input.buffer);
                        let caret_x = text_x + text_w;
                        if caret_x < input_rect.right - self.scale(4) {
                            Self::fill(
                                hdc,
                                RECT {
                                    left: caret_x,
                                    top: top + self.scale(3),
                                    right: caret_x + self.scale(2),
                                    bottom: top + self.scale(EXPLORER_ROW - 5),
                                },
                                self.theme.text,
                            );
                        }
                    }

                    let row_offset = if input_is_new { 1 } else { 0 };

                    for (row, item) in self
                        .explorer_rows()
                        .iter()
                        .enumerate()
                        .skip(self.explorer_first_row)
                    {
                        let top = self.scale(
                            EXPLORER_TOP + (row + row_offset - self.explorer_first_row) as i32 * EXPLORER_ROW,
                        );
                        if top >= sidebar_bottom - self.scale(38) {
                            break;
                        }
                        let is_being_renamed = self.explorer_input.as_ref().is_some_and(|inp| {
                            inp.is_rename && inp.old_path.as_deref() == Some(&item.entry.path)
                        });
                        let selected = self.selected_explorer_path.as_deref() == Some(&item.entry.path)
                            || self
                                .doc()
                                .path
                                .as_deref()
                                .is_some_and(|path| path == item.entry.path);
                        if selected || is_being_renamed {
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
                                if is_being_renamed { self.theme.blue } else { rgb(48, 84, 156) },
                                if is_being_renamed { rgb(16, 26, 48) } else { rgb(26, 44, 90) },
                            );
                        }
                        let name = item
                            .entry
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy();
                        // Draw subtle vertical indent guidelines for nested levels
                        for d in 0..item.depth.min(6) {
                            let guide_x = self.scale(RAIL + 23 + d as i32 * 14 + 5);
                            Self::fill(
                                hdc,
                                RECT {
                                    left: guide_x,
                                    top,
                                    right: guide_x + 1.max(self.scale(1)),
                                    bottom: top + self.scale(EXPLORER_ROW),
                                },
                                rgb(38, 52, 78),
                            );
                        }

                        let left = self.scale(RAIL + 23 + item.depth.min(6) as i32 * 14);
                        if item.entry.is_dir {
                            self.chevron(
                                hdc,
                                left + self.scale(5),
                                top + self.scale(EXPLORER_ROW / 2),
                                item.expanded,
                            );
                        }
                        if !self.icons.draw_for_path(
                            hdc,
                            &item.entry.path,
                            item.entry.is_dir,
                            item.expanded,
                            self.has_extension("material-icons"),
                            left + self.scale(12),
                            top + self.scale(2),
                            self.scale(18),
                        ) {
                            if item.entry.is_dir {
                                self.draw_vector_folder(
                                    hdc,
                                    left + self.scale(12),
                                    top + self.scale(2),
                                    self.scale(18),
                                    item.expanded,
                                );
                            } else {
                                self.draw_vector_file(
                                    hdc,
                                    &item.entry.path,
                                    left + self.scale(12),
                                    top + self.scale(2),
                                    self.scale(18),
                                );
                            }
                        }
                        if is_being_renamed {
                            let input_buf = self.explorer_input.as_ref().map(|i| i.buffer.as_str()).unwrap_or("");
                            let text_x = left + self.scale(36);
                            let text_clip = RECT {
                                left: text_x,
                                top,
                                right: editor_left - self.scale(10),
                                bottom: top + self.scale(EXPLORER_ROW),
                            };
                            Self::label(
                                hdc,
                                input_buf,
                                text_x,
                                top + self.scale(1),
                                self.theme.text,
                                text_clip,
                            );
                            let text_w = self.text_width(hdc, input_buf);
                            let caret_x = text_x + text_w;
                            if caret_x < text_clip.right {
                                Self::fill(
                                    hdc,
                                    RECT {
                                        left: caret_x,
                                        top: top + self.scale(3),
                                        right: caret_x + self.scale(2),
                                        bottom: top + self.scale(EXPLORER_ROW - 5),
                                    },
                                    self.theme.text,
                                );
                            }
                        } else {
                            Self::label(
                                hdc,
                                &name,
                                left + self.scale(36),
                                top + self.scale(1),
                                if selected {
                                    self.theme.text
                                } else if item.entry.is_dir {
                                    self.theme.muted
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
                    }
                } else {
                    Self::label(
                        hdc,
                        "Open a file to browse",
                        self.scale(RAIL + 16),
                        self.scale(49),
                        self.theme.muted,
                        sidebar_clip,
                    );
                    Self::label(
                        hdc,
                        "its folder  (Ctrl+O)",
                        self.scale(RAIL + 16),
                        self.scale(73),
                        self.theme.muted,
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
                        self.theme.edge,
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
                    self.theme.card_edge,
                );
            }
            self.card_outline(hdc, editor_card, card_radius, self.theme.card_edge);
            if self.ai_assistant_visible {
                let ai_card = RECT {
                    left: editor_card.right + gap,
                    top: chrome_top,
                    right: rect.right - gap,
                    bottom: card_bottom,
                };
                self.paint_ai_assistant(hdc, ai_card);
                self.card_outline(hdc, ai_card, card_radius, self.theme.card_edge);
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
                self.theme.edge,
            );
            SelectObject(hdc, self.ui_font);

            // Durable repository health stays on the left; editor-specific
            // details sit on the right beside the Ready indicator.
            let branch = self.git_head_label();
            let left_branch = format!("\u{2442}  {branch}");
            let left_x = self.scale(16);
            Self::label(
                hdc,
                &left_branch,
                left_x,
                editor_bottom + self.scale(5),
                self.theme.text,
                rect,
            );
            let health_x = left_x + self.text_width(hdc, &left_branch) + self.scale(24);
            Self::label(
                hdc,
                "⊗ 0    ⚠ 0",
                health_x,
                editor_bottom + self.scale(5),
                self.theme.muted,
                rect,
            );
            let left_info_right = health_x + self.text_width(hdc, "⊗ 0    ⚠ 0");

            let right_ready = "\u{25cf}  Ready";
            let ready_width = self.text_width(hdc, right_ready);
            let ready_x = rect.right - ready_width - self.scale(16);
            Self::label(hdc, "\u{25cf}", ready_x, editor_bottom + self.scale(5), rgb(52, 211, 153), rect);
            Self::label(hdc, "Ready", ready_x + self.scale(14), editor_bottom + self.scale(5), self.theme.text, rect);

            // Middle info: Ln, Col, Spaces, Encoding, Language. The language
            // name is a real clickable control (see status_language_control),
            // so it's drawn in the accent color used for other clickable
            // labels instead of blending into the plain muted text.
            let (mid_x, clip_right, prefix, language_rect) =
                self.status_language_control(hdc, rect, editor_bottom, left_info_right, ready_x);
            let label_clip = RECT { left: mid_x, top: editor_bottom, right: clip_right, bottom: rect.bottom };
            Self::label(hdc, &prefix, mid_x, editor_bottom + self.scale(5), self.theme.muted, label_clip);
            let language = language_label(self.doc().path.as_deref());
            Self::label(
                hdc,
                &language,
                language_rect.left,
                editor_bottom + self.scale(5),
                rgb(80, 160, 220),
                label_clip,
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

    pub(in crate::windows_app) fn draw_new_file_icon(&self, hdc: HDC, rect: RECT, color: u32) {
        let cx = (rect.left + rect.right) / 2;
        let cy = (rect.top + rect.bottom) / 2;
        let s = |v: i32| self.scale(v);
        let pen = unsafe { CreatePen(PS_SOLID, s(1).max(1), color) };
        if pen.is_null() {
            return;
        }
        unsafe {
            let old_pen = SelectObject(hdc, pen);
            let l = cx - s(5);
            let r = cx + s(5);
            let t = cy - s(6);
            let b = cy + s(6);
            let fold = s(3);

            // Document outline with folded top-right corner
            MoveToEx(hdc, l, t, null_mut());
            LineTo(hdc, r - fold, t);
            LineTo(hdc, r, t + fold);
            LineTo(hdc, r, b);
            LineTo(hdc, l, b);
            LineTo(hdc, l, t);

            // Corner fold crease
            MoveToEx(hdc, r - fold, t, null_mut());
            LineTo(hdc, r - fold, t + fold);
            LineTo(hdc, r, t + fold);

            // Plus mark inside
            let pcx = cx - s(1);
            let pcy = cy + s(1);
            let pr = s(2).max(2);
            MoveToEx(hdc, pcx - pr, pcy, null_mut());
            LineTo(hdc, pcx + pr + 1, pcy);
            MoveToEx(hdc, pcx, pcy - pr, null_mut());
            LineTo(hdc, pcx, pcy + pr + 1);

            SelectObject(hdc, old_pen);
            DeleteObject(pen);
        }
    }

    pub(in crate::windows_app) fn draw_new_folder_icon(&self, hdc: HDC, rect: RECT, color: u32) {
        let cx = (rect.left + rect.right) / 2;
        let cy = (rect.top + rect.bottom) / 2;
        let s = |v: i32| self.scale(v);
        let pen = unsafe { CreatePen(PS_SOLID, s(1).max(1), color) };
        if pen.is_null() {
            return;
        }
        unsafe {
            let old_pen = SelectObject(hdc, pen);
            let l = cx - s(6);
            let r = cx + s(6);
            let t = cy - s(4);
            let b = cy + s(5);
            let tab_w = s(4);

            // Folder tab outline
            MoveToEx(hdc, l, t, null_mut());
            LineTo(hdc, l + tab_w, t);
            LineTo(hdc, l + tab_w + s(2), t + s(2));
            LineTo(hdc, r, t + s(2));
            LineTo(hdc, r, b);
            LineTo(hdc, l, b);
            LineTo(hdc, l, t);

            // Plus mark inside folder
            let pcx = cx;
            let pcy = cy + s(1);
            let pr = s(2).max(2);
            MoveToEx(hdc, pcx - pr, pcy, null_mut());
            LineTo(hdc, pcx + pr + 1, pcy);
            MoveToEx(hdc, pcx, pcy - pr, null_mut());
            LineTo(hdc, pcx, pcy + pr + 1);

            SelectObject(hdc, old_pen);
            DeleteObject(pen);
        }
    }

    pub(in crate::windows_app) fn draw_refresh_icon(&self, hdc: HDC, rect: RECT, color: u32) {
        let cx = (rect.left + rect.right) / 2;
        let cy = (rect.top + rect.bottom) / 2;
        let s = |v: i32| self.scale(v);
        let r = s(5).max(4);
        let pen = unsafe { CreatePen(PS_SOLID, s(1).max(1), color) };
        if pen.is_null() {
            return;
        }
        unsafe {
            let old_pen = SelectObject(hdc, pen);
            // 3/4 circular curve
            MoveToEx(hdc, cx, cy - r, null_mut());
            LineTo(hdc, cx + r, cy - r / 2);
            LineTo(hdc, cx + r, cy + r / 2);
            LineTo(hdc, cx, cy + r);
            LineTo(hdc, cx - r, cy);
            LineTo(hdc, cx - r / 2, cy - r / 2);

            // Arrowhead at top pointing clockwise
            MoveToEx(hdc, cx - s(3), cy - r - s(2), null_mut());
            LineTo(hdc, cx, cy - r);
            LineTo(hdc, cx - s(3), cy - r + s(2));

            SelectObject(hdc, old_pen);
            DeleteObject(pen);
        }
    }

    pub(in crate::windows_app) fn draw_close_icon(&self, hdc: HDC, rect: RECT, color: u32) {
        let cx = (rect.left + rect.right) / 2;
        let cy = (rect.top + rect.bottom) / 2;
        let s = |v: i32| self.scale(v);
        let r = s(4).max(3);
        let pen = unsafe { CreatePen(PS_SOLID, s(1).max(1), color) };
        if pen.is_null() {
            return;
        }
        unsafe {
            let old_pen = SelectObject(hdc, pen);
            MoveToEx(hdc, cx - r, cy - r, null_mut());
            LineTo(hdc, cx + r + 1, cy + r + 1);
            MoveToEx(hdc, cx + r, cy - r, null_mut());
            LineTo(hdc, cx - r - 1, cy + r + 1);
            SelectObject(hdc, old_pen);
            DeleteObject(pen);
        }
    }

    pub(in crate::windows_app) fn draw_collapse_all_icon(&self, hdc: HDC, rect: RECT, color: u32) {
        let cx = (rect.left + rect.right) / 2;
        let cy = (rect.top + rect.bottom) / 2;
        let s = |v: i32| self.scale(v);
        let r = s(4).max(3);
        let pen = unsafe { CreatePen(PS_SOLID, s(1).max(1), color) };
        if pen.is_null() {
            return;
        }
        unsafe {
            let old_pen = SelectObject(hdc, pen);
            // Upper chevron
            MoveToEx(hdc, cx - r, cy - s(1), null_mut());
            LineTo(hdc, cx, cy - s(1) - r / 2);
            LineTo(hdc, cx + r + 1, cy - s(1));
            // Lower chevron
            MoveToEx(hdc, cx - r, cy + s(3), null_mut());
            LineTo(hdc, cx, cy + s(3) - r / 2);
            LineTo(hdc, cx + r + 1, cy + s(3));
            SelectObject(hdc, old_pen);
            DeleteObject(pen);
        }
    }
}
