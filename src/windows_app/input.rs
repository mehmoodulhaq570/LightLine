use super::*;

impl App {
    pub(super) fn key(&mut self, hwnd: HWND, key: u32) -> bool {
        self.clear_hover(hwnd);
        let ctrl = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
        let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
        if self.terminal_focus {
            let alt = unsafe { GetKeyState(VK_MENU as i32) } < 0;
            // Ctrl+` hides the panel even while the shell has focus.
            if ctrl && !shift && key == VK_OEM_3 as u32 {
                self.hide_terminal(hwnd);
                return true;
            }
            // Escape hides the panel; the shell keeps running until it is closed.
            if key == VK_ESCAPE as u32 && !ctrl && !shift {
                self.hide_terminal(hwnd);
                return true;
            }
            // Reserved application chords fall through to the handlers below.
            let reserved_chord = ctrl
                && match key {
                    0x50 | 0x52 | 0x42 => shift, // Ctrl+Shift+P/R/B
                    v if v == VK_TAB as u32 => true,
                    v if v == VK_PRIOR as u32 || v == VK_NEXT as u32 => true,
                    _ => false,
                };
            if !reserved_chord {
                return if ctrl && key == 0x56 {
                    // Ordinary Ctrl+V and Ctrl+Shift+V both paste into the shell.
                    self.paste_into_terminal(hwnd);
                    true
                } else if ctrl && shift && key == 0x43 {
                    self.copy_terminal_selection(hwnd);
                    true
                } else {
                    self.send_terminal_key(hwnd, key, ctrl, shift, alt)
                };
            }
        }
        // Output never takes keyboard focus, but copying its text (e.g. a
        // build error) is still a read, not an edit, so it's allowed here.
        if self.terminal_visible
            && !self.terminal_focus
            && ctrl
            && shift
            && key == 0x43
            && self.terminal_select_anchor.is_some()
        {
            self.copy_terminal_selection(hwnd);
            return true;
        }
        if self.welcome && key == VK_ESCAPE as u32 && self.workspace_root.is_some() {
            self.welcome = false;
            self.show_active_tab(hwnd);
            return true;
        }
        if self.quick_open {
            match key {
                x if x == VK_ESCAPE as u32 => self.quick_open = false,
                x if x == VK_UP as u32 => {
                    self.quick_selected = self.quick_selected.saturating_sub(1)
                }
                x if x == VK_DOWN as u32 => {
                    self.quick_selected =
                        (self.quick_selected + 1).min(self.quick_count().saturating_sub(1))
                }
                x if x == VK_BACK as u32 => {
                    self.quick_query.pop();
                    self.quick_selected = 0;
                }
                x if x == VK_RETURN as u32 => {
                    self.activate_quick_item(hwnd, self.quick_selected);
                }
                _ if !ctrl => return false,
                _ => return true,
            }
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return true;
        }
        if self.search_input {
            match key {
                x if x == VK_ESCAPE as u32 => {
                    self.search_input = false;
                    self.cancel_search();
                    self.panel_focus = false;
                    self.set_sidebar_visible(hwnd, false);
                }
                x if x == VK_RETURN as u32 => {
                    self.search_input = false;
                    self.search_project(hwnd);
                }
                x if x == VK_BACK as u32 => {
                    self.project_query.pop();
                    self.search_results.clear();
                }
                _ if !ctrl => return false,
                _ => {}
            }
            if key == VK_ESCAPE as u32 || key == VK_RETURN as u32 || key == VK_BACK as u32 {
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return true;
            }
        }
        if self.panel_focus && !ctrl {
            let count = if self.side_view == SideView::Search {
                self.search_results.len()
            } else {
                self.changes.len()
            };
            match key {
                x if x == VK_UP as u32 => {
                    self.panel_selected = self.panel_selected.saturating_sub(1)
                }
                x if x == VK_DOWN as u32 => {
                    self.panel_selected = (self.panel_selected + 1).min(count.saturating_sub(1))
                }
                x if x == VK_RETURN as u32 => {
                    if self.side_view == SideView::Search {
                        if let Some(hit) = self.search_results.get(self.panel_selected).cloned() {
                            self.panel_focus = false;
                            self.open(hwnd, Some(hit.path));
                            self.move_cursor(
                                Pos {
                                    line: hit.line,
                                    byte: hit.byte,
                                },
                                false,
                            );
                            self.keep_cursor_visible(hwnd);
                        }
                    } else if let Some(change) = self.changes.get(self.panel_selected).cloned() {
                        self.panel_focus = false;
                        self.show_diff(hwnd, change.path);
                    }
                    return true;
                }
                _ => {}
            }
            if key == VK_UP as u32 || key == VK_DOWN as u32 {
                let visible = if self.side_view == SideView::Search {
                    10
                } else {
                    20
                };
                if self.panel_selected < self.panel_first {
                    self.panel_first = self.panel_selected;
                }
                if self.panel_selected >= self.panel_first + visible {
                    self.panel_first = self.panel_selected + 1 - visible;
                }
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return true;
            }
        }
        if self.side_view == SideView::Review
            && self.review_file.is_some()
            && !self.panel_focus
            && !ctrl
        {
            match key {
                x if x == VK_UP as u32 => self.diff_first = self.diff_first.saturating_sub(1),
                x if x == VK_DOWN as u32 => {
                    self.diff_first =
                        (self.diff_first + 1).min(self.diff_rows.len().saturating_sub(1))
                }
                x if x == VK_ESCAPE as u32 => {
                    self.review_file = None;
                    self.panel_focus = true;
                }
                _ => return true,
            }
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return true;
        }
        if self.find_mode {
            match key {
                x if x == VK_ESCAPE as u32 => {
                    self.find_mode = false;
                    self.status = "Ready".into();
                    self.refresh(hwnd);
                    return true;
                }
                x if x == VK_BACK as u32 || x == VK_RETURN as u32 => return true,
                _ => {}
            }
        }
        if ctrl {
            let cursor = self.view().cursor;
            match key {
                x if x == VK_OEM_3 as u32 && !shift => {
                    self.toggle_terminal(hwnd);
                    return true;
                }
                x if x == VK_OEM_5 as u32 => {
                    self.toggle_split(hwnd);
                    return true;
                }
                0x31 if self.split_visible => {
                    self.focus_pane(hwnd, 0);
                    return true;
                }
                0x32 if self.split_visible => {
                    self.focus_pane(hwnd, 1);
                    return true;
                }
                0x48 if shift => {
                    self.show_welcome(hwnd);
                    return true;
                }
                0x50 => {
                    self.show_quick_open(hwnd);
                    return true;
                }
                0x4f if shift => {
                    self.open_folder(hwnd);
                    return true;
                }
                0x46 if shift => {
                    self.open_project_search(hwnd);
                    return true;
                }
                0x47 if shift => {
                    self.show_review(hwnd);
                    return true;
                }
                0x42 if shift => {
                    self.run_project(hwnd);
                    return true;
                }
                0x52 if shift => {
                    self.run_python_file(hwnd);
                    return true;
                }
                x if x == VK_OEM_PLUS as u32 || x == VK_ADD as u32 => {
                    self.set_zoom(hwnd, self.zoom + 20);
                    return true;
                }
                x if x == VK_OEM_MINUS as u32 || x == VK_SUBTRACT as u32 => {
                    self.set_zoom(hwnd, self.zoom - 20);
                    return true;
                }
                0x30 => {
                    self.set_zoom(hwnd, 100);
                    return true;
                }
                x if x == VK_NUMPAD0 as u32 => {
                    self.set_zoom(hwnd, 100);
                    return true;
                }
                0x41 => {
                    self.view_mut().selection_anchor = Some(Pos::default());
                    let end = self.doc().end();
                    self.view_mut().cursor = end;
                }
                0x43 => {
                    self.copy_selection(hwnd);
                }
                0x58 => {
                    if self.copy_selection(hwnd) {
                        self.replace_selection("");
                    }
                }
                0x56 => match clipboard::paste(hwnd) {
                    Ok(Some(text)) => self.replace_selection(&text),
                    Ok(None) => {}
                    Err(error) => self.error(hwnd, &error),
                },
                0x4e => {
                    self.new_file(hwnd);
                    return true;
                }
                0x46 => {
                    self.search_input = false;
                    self.panel_focus = false;
                    self.find_mode = true;
                    self.find_query.clear();
                    self.status = "Find: ".into();
                }
                0x42 => {
                    if self.side_view == SideView::Search {
                        self.cancel_search();
                    }
                    self.side_view = SideView::Files;
                    self.panel_focus = false;
                    self.set_sidebar_visible(hwnd, !self.explorer_visible);
                    self.show_active_tab(hwnd);
                    return true;
                }
                0x4f => {
                    self.open(hwnd, None);
                    return true;
                }
                0x53 => {
                    self.save(hwnd, shift);
                }
                0x57 => {
                    self.close_tab(hwnd, self.active);
                    return true;
                }
                x if x == VK_TAB as u32 => {
                    let next = if shift {
                        (self.active + self.tabs.len() - 1) % self.tabs.len()
                    } else {
                        (self.active + 1) % self.tabs.len()
                    };
                    self.activate_tab(hwnd, next);
                    return true;
                }
                x if x == VK_PRIOR as u32 => {
                    self.activate_tab(hwnd, self.active.saturating_sub(1));
                    return true;
                }
                x if x == VK_NEXT as u32 => {
                    self.activate_tab(hwnd, (self.active + 1).min(self.tabs.len() - 1));
                    return true;
                }
                0x5a if shift => {
                    self.view_mut().selection_anchor = None;
                    if let Some((cursor, line)) = self.doc_mut().redo() {
                        self.view_mut().cursor = cursor;
                        self.syntax_changed(line);
                        self.revalidate_other_view(None);
                        self.sync_lsp_edit();
                    }
                }
                0x5a => {
                    self.view_mut().selection_anchor = None;
                    if let Some((cursor, line)) = self.doc_mut().undo() {
                        self.view_mut().cursor = cursor;
                        self.syntax_changed(line);
                        self.revalidate_other_view(None);
                        self.sync_lsp_edit();
                    }
                }
                0x59 => {
                    self.view_mut().selection_anchor = None;
                    if let Some((cursor, line)) = self.doc_mut().redo() {
                        self.view_mut().cursor = cursor;
                        self.syntax_changed(line);
                        self.revalidate_other_view(None);
                        self.sync_lsp_edit();
                    }
                }
                x if x == VK_HOME as u32 => self.move_cursor(Pos::default(), shift),
                x if x == VK_END as u32 => self.move_cursor(self.doc().end(), shift),
                x if x == VK_LEFT as u32 => {
                    let target = self.doc().previous_word(cursor);
                    self.move_cursor(target, shift);
                }
                x if x == VK_RIGHT as u32 => {
                    let target = self.doc().next_word(cursor);
                    self.move_cursor(target, shift);
                }
                x if x == VK_BACK as u32 => {
                    if self.selection_range().is_some() {
                        self.replace_selection("");
                    } else {
                        let previous = self.doc().previous_word(cursor);
                        self.replace_range(previous, cursor, "");
                    }
                }
                x if x == VK_DELETE as u32 => {
                    if self.selection_range().is_some() {
                        self.replace_selection("");
                    } else {
                        let next = self.doc().next_word(cursor);
                        self.replace_range(cursor, next, "");
                    }
                }
                _ => return false,
            }
            self.refresh(hwnd);
            return true;
        }
        let cursor = self.view().cursor;
        let alt = unsafe { GetKeyState(VK_MENU as i32) } < 0;
        // Shift+Alt+F formats the current document through the language server.
        if shift && alt && !ctrl && key == 0x46 {
            self.format_document(hwnd);
            return true;
        }
        match key {
            x if x == VK_F1 as u32 => {
                self.hover_at_cursor(hwnd);
                return true;
            }
            x if x == VK_F12 as u32 => {
                self.goto_definition(hwnd);
                return true;
            }
            x if x == VK_F3 as u32 => {
                self.find_mode = false;
                self.find(hwnd, !shift);
                return true;
            }
            x if x == VK_ESCAPE as u32 => {
                if self.terminal_visible {
                    self.hide_terminal(hwnd);
                    return true;
                }
                if self.side_view != SideView::Files {
                    if self.side_view == SideView::Search {
                        self.cancel_search();
                    }
                    self.side_view = SideView::Files;
                    self.set_sidebar_visible(hwnd, false);
                    self.review_file = None;
                    self.keep_cursor_visible(hwnd);
                    return true;
                }
                self.view_mut().selection_anchor = None;
            }
            x if x == VK_LEFT as u32 => {
                let target = if !shift {
                    self.selection_range().map(|(start, _)| start)
                } else {
                    None
                }
                .unwrap_or_else(|| self.doc().previous(cursor));
                self.move_cursor(target, shift);
            }
            x if x == VK_RIGHT as u32 => {
                let target = if !shift {
                    self.selection_range().map(|(_, end)| end)
                } else {
                    None
                }
                .unwrap_or_else(|| self.doc().next(cursor));
                self.move_cursor(target, shift);
            }
            x if x == VK_UP as u32 => {
                self.move_cursor(
                    Pos {
                        line: cursor.line.saturating_sub(1),
                        byte: cursor.byte,
                    },
                    shift,
                );
            }
            x if x == VK_DOWN as u32 => {
                self.move_cursor(
                    Pos {
                        line: (cursor.line + 1).min(self.doc().line_count() - 1),
                        byte: cursor.byte,
                    },
                    shift,
                );
            }
            x if x == VK_PRIOR as u32 => {
                self.move_cursor(
                    Pos {
                        line: cursor.line.saturating_sub(self.visible_lines(hwnd)),
                        byte: cursor.byte,
                    },
                    shift,
                );
            }
            x if x == VK_NEXT as u32 => {
                self.move_cursor(
                    Pos {
                        line: (cursor.line + self.visible_lines(hwnd))
                            .min(self.doc().line_count() - 1),
                        byte: cursor.byte,
                    },
                    shift,
                );
            }
            x if x == VK_HOME as u32 => self.move_cursor(
                Pos {
                    line: cursor.line,
                    byte: 0,
                },
                shift,
            ),
            x if x == VK_END as u32 => {
                self.move_cursor(
                    Pos {
                        line: cursor.line,
                        byte: self.doc().line(cursor.line).len(),
                    },
                    shift,
                );
            }
            x if x == VK_BACK as u32 => {
                if self.selection_range().is_some() {
                    self.replace_selection("");
                } else {
                    let previous = self.doc().previous(cursor);
                    self.replace_range(previous, cursor, "");
                }
            }
            x if x == VK_DELETE as u32 => {
                if self.selection_range().is_some() {
                    self.replace_selection("");
                } else {
                    let next = self.doc().next(cursor);
                    self.replace_range(cursor, next, "");
                }
            }
            _ => return false,
        }
        self.refresh(hwnd);
        true
    }

    pub(super) fn character(&mut self, hwnd: HWND, unit: u16) {
        if unsafe { GetKeyState(VK_CONTROL as i32) } < 0 {
            return;
        }
        if self.terminal_focus {
            // Enter/Tab/Backspace/Escape and arrows arrive through key(); only
            // forward printable text (including surrogate-paired characters).
            if unit < 32 || unit == 127 {
                return;
            }
            let ch = if (0xd800..=0xdbff).contains(&unit) {
                self.pending_high_surrogate = Some(unit);
                None
            } else if (0xdc00..=0xdfff).contains(&unit) {
                self.pending_high_surrogate.take().and_then(|high| {
                    char::from_u32(
                        0x10000 + ((high as u32 - 0xd800) << 10) + (unit as u32 - 0xdc00),
                    )
                })
            } else {
                self.pending_high_surrogate = None;
                char::from_u32(unit as u32)
            };
            if let Some(ch) = ch {
                self.send_terminal_char(hwnd, ch);
            }
            return;
        }
        if self.quick_open || self.search_input {
            if unit >= 32
                && unit != 127
                && let Some(ch) = char::from_u32(unit as u32)
            {
                if self.quick_open {
                    self.quick_query.push(ch);
                    self.quick_selected = 0;
                } else {
                    self.project_query.push(ch);
                    self.search_results.clear();
                }
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            return;
        }
        if self.side_view == SideView::Review && self.review_file.is_some() {
            return;
        }
        if self.tab().read_only() {
            return;
        }
        if self.find_mode && unit == 8 {
            self.find_query.pop();
            self.status = format!("Find: {}", self.find_query);
            self.refresh(hwnd);
            return;
        }
        if self.find_mode && unit == 13 {
            self.find_mode = false;
            self.find(hwnd, true);
            return;
        }
        if (unit < 32 && unit != 9 && unit != 13) || unit == 127 {
            return;
        }
        let ch = if (0xd800..=0xdbff).contains(&unit) {
            self.pending_high_surrogate = Some(unit);
            return;
        } else if (0xdc00..=0xdfff).contains(&unit) {
            let Some(high) = self.pending_high_surrogate.take() else {
                return;
            };
            char::from_u32(0x10000 + ((high as u32 - 0xd800) << 10) + (unit as u32 - 0xdc00))
        } else {
            self.pending_high_surrogate = None;
            char::from_u32(unit as u32)
        };
        if let Some(ch) = ch {
            if self.find_mode {
                if !ch.is_control() {
                    self.find_query.push(ch);
                    self.status = format!("Find: {}", self.find_query);
                    self.refresh(hwnd);
                }
                return;
            }
            let text = if ch == '\r' {
                "\n".to_owned()
            } else {
                ch.to_string()
            };
            self.cancel_transition(hwnd);
            self.replace_selection(&text);
            self.refresh(hwnd);
        }
    }

    pub(super) fn position_at(&self, hwnd: HWND, x: i32, y: i32) -> Pos {
        self.position_at_pane(hwnd, x, y, self.focused_pane)
    }

    pub(super) fn position_at_pane(&self, hwnd: HWND, x: i32, y: i32, pane: usize) -> Pos {
        let tab = &self.tabs[self.tab_for_pane(pane)];
        let view = self.view_for_pane(pane);
        let row = ((y - self.editor_top()) / self.line_height).max(0) as usize;
        let line = (view.first_line + row).min(tab.document.line_count() - 1);
        let target = (x - self.pane_left(hwnd, pane) - self.scale(GUTTER + PAD)).max(0);
        unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.font);
            let text = tab.document.line(line);
            let boundaries: Vec<usize> = text
                .char_indices()
                .map(|(index, _)| index)
                .chain(Some(text.len()))
                .collect();
            let mut low = 0;
            let mut high = boundaries.len();
            while low < high {
                let mid = (low + high) / 2;
                if self.text_width(hdc, &text[..boundaries[mid]]) < target {
                    low = mid + 1;
                } else {
                    high = mid;
                }
            }
            let right = low.min(boundaries.len() - 1);
            let left = right.saturating_sub(1);
            let left_width = self.text_width(hdc, &text[..boundaries[left]]);
            let right_width = self.text_width(hdc, &text[..boundaries[right]]);
            let byte = if target - left_width <= right_width - target {
                boundaries[left]
            } else {
                boundaries[right]
            };
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            Pos { line, byte }
        }
    }

    pub(super) fn mouse_click(&mut self, hwnd: HWND, x: i32, y: i32, extend: bool) {
        self.clear_hover(hwnd);
        let mut rect = RECT::default();
        unsafe {
            GetClientRect(hwnd, &mut rect);
        }
        if self.welcome {
            let left = (rect.right / 2 - self.scale(250)).max(self.scale(28));
            if y >= self.scale(251) && y < self.scale(289) {
                let button = (x - left) / self.scale(145).max(1);
                if x >= left && button == 0 {
                    self.open(hwnd, None);
                } else if x >= left && button == 1 {
                    self.open_folder(hwnd);
                } else if x >= left && button == 2 {
                    self.new_file(hwnd);
                }
            } else if y >= self.scale(351) {
                let index = ((y - self.scale(351)) / self.scale(49).max(1)) as usize;
                if x >= left
                    && x < left + self.scale(430)
                    && let Some(path) = self.recent.get(index).cloned()
                {
                    self.set_workspace(hwnd, path);
                }
            }
            return;
        }
        if self.quick_open {
            let width = self.scale(560).min(rect.right - self.scale(30));
            let left = (rect.right - width) / 2;
            let top = self.scale(52);
            if x >= left
                && x < left + width
                && y >= top + self.scale(68)
                && y < top + self.scale(70 + 8 * 34)
            {
                let index = ((y - top - self.scale(68)) / self.scale(34).max(1)) as usize;
                self.activate_quick_item(hwnd, index);
            } else if x < left || x >= left + width || y < top || y >= top + self.scale(70 + 8 * 34)
            {
                self.quick_open = false;
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            return;
        }
        if y >= rect.bottom - self.scale(STATUS) {
            return;
        }
        let rail = self.scale(RAIL);
        let editor_left = self.editor_left();
        if x < rail {
            if y < self.scale(39) {
                self.show_welcome(hwnd);
            } else if y >= self.scale(RAIL_FIRST_ROW)
                && y < self.scale(RAIL_FIRST_ROW + RAIL_ROW * 6)
            {
                let row = (y - self.scale(RAIL_FIRST_ROW)) / self.scale(RAIL_ROW).max(1);
                match row {
                    0 => {
                        if self.side_view == SideView::Search {
                            self.cancel_search();
                        }
                        let already_open =
                            self.side_view == SideView::Files && self.explorer_visible;
                        self.side_view = SideView::Files;
                        self.panel_focus = false;
                        self.set_sidebar_visible(hwnd, !already_open);
                        self.show_active_tab(hwnd);
                    }
                    1 => self.open_project_search(hwnd),
                    2 => self.show_review(hwnd),
                    3 => {
                        if Tab::is_python(self.doc()) {
                            self.run_python_file(hwnd);
                        } else {
                            self.run_project(hwnd);
                        }
                    }
                    4 => {
                        self.status = "Extensions are planned for a later release".into();
                        self.refresh(hwnd);
                    }
                    5 => {
                        self.status = "AI Assistant is not installed".into();
                        self.refresh(hwnd);
                    }
                    _ => {}
                }
            } else if y >= rect.bottom - self.scale(STATUS + 40) {
                self.status = "Settings are not available yet".into();
                self.refresh(hwnd);
            }
            return;
        }
        // Sidebar-width resize handle: a few px straddling its right edge.
        if self.sidebar_width > 0
            && self.sidebar_started.is_none()
            && (x - editor_left).abs() <= self.scale(4)
        {
            self.sidebar_dragging = true;
            unsafe { SetCapture(hwnd) };
            return;
        }
        if self.sidebar_width > 0 && x < editor_left {
            if !self.explorer_visible {
                return;
            }
            if y < self.scale(39) && x >= editor_left - self.scale(36) {
                self.set_sidebar_visible(hwnd, false);
                return;
            }
            if self.side_view == SideView::Files
                && y >= self.scale(15)
                && y < self.scale(26)
                && x >= self.scale(RAIL + 12)
                && x < self.scale(RAIL + 23)
            {
                self.expanded_dirs.clear();
                self.explorer_first_row = 0;
                self.refresh(hwnd);
                return;
            }
            if self.side_view == SideView::Search {
                if y >= self.scale(47) && y < self.scale(78) {
                    self.search_input = true;
                    return;
                }
                if y >= self.scale(113) {
                    let index =
                        self.panel_first + ((y - self.scale(113)) / self.scale(48).max(1)) as usize;
                    if let Some(hit) = self.search_results.get(index).cloned() {
                        self.panel_focus = false;
                        self.open(hwnd, Some(hit.path));
                        self.move_cursor(
                            Pos {
                                line: hit.line,
                                byte: hit.byte,
                            },
                            false,
                        );
                        self.keep_cursor_visible(hwnd);
                    }
                }
                return;
            }
            if self.side_view == SideView::Review {
                if y >= self.scale(86) {
                    let index = self.panel_first
                        + ((y - self.scale(86)) / self.scale(EXPLORER_ROW).max(1)) as usize;
                    if let Some(change) = self.changes.get(index).cloned() {
                        self.panel_focus = false;
                        self.show_diff(hwnd, change.path);
                    }
                }
                return;
            }
            if y >= rect.bottom - self.scale(STATUS + 35) {
                self.show_active_tab(hwnd);
                return;
            }
            if y >= self.scale(EXPLORER_TOP) && y < rect.bottom - self.scale(STATUS + 38) {
                let row = self.explorer_first_row
                    + ((y - self.scale(EXPLORER_TOP)) / self.scale(EXPLORER_ROW)) as usize;
                if let Some(item) = self.explorer_rows().get(row) {
                    let path = item.entry.path.clone();
                    if item.entry.is_dir {
                        if self.expanded_dirs.remove(&path) {
                            self.explorer_first_row = self
                                .explorer_first_row
                                .min(self.explorer_rows().len().saturating_sub(1));
                            self.refresh(hwnd);
                        } else {
                            self.expanded_dirs.insert(path.clone());
                            self.load_directory(&path);
                            self.refresh(hwnd);
                        }
                    } else {
                        self.open(hwnd, Some(path));
                    }
                }
            }
            return;
        }
        // Terminal-height resize handle: a few px straddling its top edge.
        if self.terminal_visible && (y - self.terminal_top(hwnd)).abs() <= self.scale(4) {
            self.terminal_resizing = true;
            unsafe { SetCapture(hwnd) };
            return;
        }
        if self.terminal_visible && y >= self.terminal_top(hwnd) {
            let header_bottom = self.terminal_top(hwnd) + self.scale(34);
            if y < header_bottom && x >= rect.right - self.scale(40) {
                self.close_terminal(hwnd);
                return;
            }
            if y < header_bottom {
                let tab_slot = self.scale(90);
                let tabs_left = editor_left + self.scale(16);
                if x >= tabs_left && x < tabs_left + tab_slot {
                    self.switch_terminal_tab(hwnd, TerminalTab::Output);
                    return;
                }
                if x >= tabs_left + tab_slot && x < tabs_left + tab_slot * 2 {
                    self.switch_terminal_tab(hwnd, TerminalTab::Terminal);
                    return;
                }
            }
            self.focus_terminal(hwnd);
            self.start_terminal_selection(hwnd, x, y);
            return;
        }
        if self.side_view == SideView::Review
            && self.review_file.is_some()
            && y >= self.editor_top()
        {
            return;
        }
        if self.split_visible
            && y >= self.scale(TAB_HEIGHT)
            && (x - self.pane_divider(hwnd)).abs() <= self.scale(6)
        {
            self.divider_dragging = true;
            unsafe { SetCapture(hwnd) };
            return;
        }
        if y < self.scale(TAB_HEIGHT) {
            if Tab::is_python(self.doc())
                && x >= rect.right - self.scale(326)
                && x < rect.right - self.scale(296)
            {
                self.run_python_file(hwnd);
                return;
            }
            if x >= rect.right - self.scale(112) {
                self.toggle_split(hwnd);
                return;
            }
            if editor_left
                + self.scale(TAB_WIDTH) * self.tabs.len().saturating_sub(self.tab_first) as i32
                + self.scale(12)
                < rect.right - self.scale(285)
                && x >= rect.right - self.scale(285)
            {
                self.show_quick_open(hwnd);
                return;
            }
            let slot = ((x - editor_left).max(0) / self.scale(TAB_WIDTH).max(1)) as usize;
            let index = self.tab_first + slot;
            if index < self.tabs.len() {
                if (x - editor_left) % self.scale(TAB_WIDTH) >= self.scale(TAB_WIDTH - 30) {
                    self.close_tab(hwnd, index);
                } else {
                    self.activate_tab(hwnd, index);
                }
            }
            return;
        }
        if y < self.editor_top() {
            if self.split_visible {
                let pane = usize::from(x >= self.pane_divider(hwnd));
                self.focus_pane(hwnd, pane);
            }
            return;
        }
        if self.split_visible {
            let pane = usize::from(x >= self.pane_divider(hwnd));
            self.focus_pane(hwnd, pane);
        }
        let pos = self.position_at(hwnd, x, y);
        self.panel_focus = false;
        self.terminal_focus = false;
        self.move_cursor(pos, extend);
        self.dragging = true;
        unsafe {
            SetFocus(hwnd);
            SetCapture(hwnd);
        }
        self.refresh(hwnd);
    }

    pub(super) fn mouse_drag(&mut self, hwnd: HWND, x: i32, y: i32) {
        if self.divider_dragging {
            self.resize_split(hwnd, x);
            return;
        }
        if self.sidebar_dragging {
            self.resize_sidebar(hwnd, x);
            return;
        }
        if self.terminal_resizing {
            self.resize_terminal_panel(hwnd, y);
            return;
        }
        if self.terminal_selecting {
            self.update_terminal_selection(hwnd, x, y);
            return;
        }
        if !self.dragging {
            return;
        }
        let pos = self.position_at(hwnd, x, y);
        self.move_cursor(pos, true);
        self.refresh(hwnd);
    }
}
