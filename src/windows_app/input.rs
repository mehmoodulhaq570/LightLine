use super::git::GitHit;
use super::terminal::TerminalHeaderHit;
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
            // Ctrl+Shift+` opens a fresh shell session and focuses it.
            if ctrl && shift && key == VK_OEM_3 as u32 {
                self.new_terminal(hwnd, false);
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
        if self.side_view == SideView::Extensions && self.extensions_search_active {
            match key {
                x if x == VK_ESCAPE as u32 => {
                    self.extensions_query.clear();
                    self.extensions_search_active = false;
                    self.panel_focus = false;
                }
                x if x == VK_RETURN as u32 => {
                    self.extensions_search_active = false;
                }
                x if x == VK_BACK as u32 => {
                    self.extensions_query.pop();
                }
                _ if !ctrl => return false,
                _ => {}
            }
            if key == VK_ESCAPE as u32 || key == VK_RETURN as u32 || key == VK_BACK as u32 {
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return true;
            }
        }
        // The commit message box borrows the same single-line text handling as
        // the search box: keys here edit a string, they never touch a document.
        if self.commit_focus && self.side_view == SideView::Review {
            match key {
                x if x == VK_ESCAPE as u32 => {
                    self.commit_focus = false;
                    self.panel_focus = true;
                }
                x if x == VK_RETURN as u32 => {
                    self.commit_focus = false;
                    self.panel_focus = true;
                    self.git_commit_pressed(hwnd);
                }
                x if x == VK_BACK as u32 => {
                    self.commit_message.pop();
                }
                _ if !ctrl => return false,
                _ => {}
            }
            if key == VK_ESCAPE as u32 || key == VK_RETURN as u32 || key == VK_BACK as u32 {
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return true;
            }
        }
        if self.panel_focus
            && !ctrl
            && self.side_view == SideView::Review
            && !self.commit_focus
        {
            // The list mixes section titles with rows, so navigation has to
            // step over the ones that cannot be opened.
            let index = self.panel_selected;
            match key {
                x if x == VK_UP as u32 => {
                    self.git_move_selection(hwnd, -1);
                    return true;
                }
                x if x == VK_DOWN as u32 => {
                    self.git_move_selection(hwnd, 1);
                    return true;
                }
                x if x == VK_RETURN as u32 => {
                    // Handing the keyboard to the diff lets Up/Down scroll it,
                    // which is what opening a row was for.
                    self.panel_focus = false;
                    self.git_activate_row(hwnd, index);
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    return true;
                }
                x if x == VK_SPACE as u32 => {
                    self.git_toggle_row(hwnd, index);
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    return true;
                }
                _ => {}
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
                        self.show_diff(hwnd, change.path, change.staged);
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
                    self.replace_mode = false;
                    self.status = "Ready".into();
                    self.refresh(hwnd);
                    return true;
                }
                x if x == VK_TAB as u32 && self.replace_mode => {
                    self.replace_field = 1 - self.replace_field;
                    self.update_find_replace_status();
                    self.refresh(hwnd);
                    return true;
                }
                x if x == VK_RETURN as u32 => {
                    if self.replace_mode {
                        let alt = unsafe { GetKeyState(VK_MENU as i32) } < 0;
                        if alt || ctrl {
                            self.replace_all(hwnd);
                        } else {
                            self.replace_next(hwnd);
                        }
                    } else {
                        self.find_mode = false;
                        self.find(hwnd, !shift);
                    }
                    return true;
                }
                x if x == VK_BACK as u32 => return true,
                _ => {}
            }
        }
        if !ctrl {
            match key {
                x if x == VK_F5 as u32 && shift => {
                    self.debug_stop(hwnd);
                    return true;
                }
                x if x == VK_F5 as u32 => {
                    self.debug_continue(hwnd);
                    return true;
                }
                x if x == VK_F10 as u32 => {
                    self.debug_step_over(hwnd);
                    return true;
                }
                x if x == VK_F11 as u32 && shift => {
                    self.debug_step_out(hwnd);
                    return true;
                }
                x if x == VK_F11 as u32 => {
                    self.debug_step_in(hwnd);
                    return true;
                }
                _ => {}
            }
        }
        if ctrl {
            let cursor = self.view().cursor;
            match key {
                x if x == VK_SPACE as u32 => {
                    self.trigger_completion(hwnd);
                    return true;
                }
                x if x == VK_OEM_3 as u32 && shift => {
                    self.new_terminal(hwnd, false);
                    return true;
                }
                x if x == VK_OEM_3 as u32 => {
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
                0x48 if !shift => {
                    self.search_input = false;
                    self.panel_focus = false;
                    self.find_mode = true;
                    self.replace_mode = true;
                    self.find_query.clear();
                    self.replace_query.clear();
                    self.replace_field = 0;
                    self.update_find_replace_status();
                    self.refresh(hwnd);
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
                0x44 if shift => {
                    self.toggle_side_view(hwnd, SideView::Debug);
                    return true;
                }
                0x58 if shift => {
                    self.toggle_side_view(hwnd, SideView::Extensions);
                    return true;
                }
                0x42 if shift => {
                    self.run_project(hwnd);
                    return true;
                }
                0x52 if shift => {
                    self.run_active_file(hwnd);
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
                x if x == VK_OEM_COMMA as u32 => {
                    self.open_settings(hwnd);
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
        // A completion popup, when open, captures navigation and commit keys
        // before they reach the editor; any other key dismisses it first.
        if self.completion_active() {
            match key {
                x if x == VK_DOWN as u32 => {
                    self.completion_move(hwnd, 1);
                    return true;
                }
                x if x == VK_UP as u32 => {
                    self.completion_move(hwnd, -1);
                    return true;
                }
                x if x == VK_RETURN as u32 || x == VK_TAB as u32 => {
                    self.accept_completion(hwnd);
                    return true;
                }
                x if x == VK_ESCAPE as u32 => {
                    self.dismiss_completion(hwnd);
                    return true;
                }
                _ => self.dismiss_completion(hwnd),
            }
        }
        match key {
            x if x == VK_F1 as u32 => {
                self.hover_at_cursor(hwnd);
                return true;
            }
            x if x == VK_F12 as u32 && shift => {
                self.find_references(hwnd);
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
                    // Delete both characters of an empty auto-closed pair.
                    let line = self.doc().line(cursor.line);
                    let before = if cursor.byte > 0 {
                        line.as_bytes().get(cursor.byte - 1).copied()
                    } else {
                        None
                    };
                    let after = line.as_bytes().get(cursor.byte).copied();
                    let is_empty_pair = matches!(
                        (before, after),
                        (Some(b'('), Some(b')'))
                            | (Some(b'['), Some(b']'))
                            | (Some(b'{'), Some(b'}'))
                            | (Some(b'"'), Some(b'"'))
                            | (Some(b'\''), Some(b'\''))
                            | (Some(b'`'), Some(b'`'))
                    );
                    if is_empty_pair {
                        let previous = self.doc().previous(cursor);
                        let next = self.doc().next(cursor);
                        self.replace_range(previous, next, "");
                    } else {
                        let previous = self.doc().previous(cursor);
                        self.replace_range(previous, cursor, "");
                    }
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
        if self.commit_focus && self.side_view == SideView::Review {
            // Control characters arrive through key(); only real text lands here.
            if unit >= 32 && unit != 127 && let Some(ch) = char::from_u32(unit as u32) {
                self.commit_message.push(ch);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            return;
        }
        if self.quick_open || self.search_input || (self.side_view == SideView::Extensions && self.extensions_search_active) {
            if unit >= 32
                && unit != 127
                && let Some(ch) = char::from_u32(unit as u32)
            {
                if self.quick_open {
                    self.quick_query.push(ch);
                    self.quick_selected = 0;
                } else if self.search_input {
                    self.project_query.push(ch);
                    self.search_results.clear();
                } else {
                    self.extensions_query.push(ch);
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
        // Typing over the identifier closes the popup; Ctrl+Space re-opens it.
        self.dismiss_completion(hwnd);
        if self.find_mode && unit == 8 {
            if self.replace_mode {
                if self.replace_field == 0 {
                    self.find_query.pop();
                } else {
                    self.replace_query.pop();
                }
                self.update_find_replace_status();
            } else {
                self.find_query.pop();
                self.status = format!("Find: {}", self.find_query);
            }
            self.refresh(hwnd);
            return;
        }
        if self.find_mode && unit == 13 {
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
                    if self.replace_mode {
                        if self.replace_field == 0 {
                            self.find_query.push(ch);
                        } else {
                            self.replace_query.push(ch);
                        }
                        self.update_find_replace_status();
                    } else {
                        self.find_query.push(ch);
                        self.status = format!("Find: {}", self.find_query);
                    }
                    self.refresh(hwnd);
                }
                return;
            }
            // Auto-closing pairs: when typing an opening bracket or quote,
            // insert the closing counterpart and leave the cursor between them.
            let closing = if self.settings.auto_close_pairs {
                match ch {
                    '(' => Some(')'),
                    '[' => Some(']'),
                    '{' => Some('}'),
                    '"' => Some('"'),
                    '\'' => Some('\''),
                    '`' => Some('`'),
                    _ => None,
                }
            } else {
                None
            };
            // For quotes, only auto-close when the character after the cursor
            // is whitespace, end-of-line, or a closing bracket — not mid-word.
            let should_auto_close = closing.is_some_and(|close| {
                if matches!(ch, '"' | '\'' | '`') {
                    let line = self.doc().line(self.view().cursor.line);
                    let after = &line[self.view().cursor.byte..];
                    after.is_empty()
                        || after.starts_with(|c: char| c.is_whitespace() || ")]}".contains(c))
                        || (close == ch && after.starts_with(ch))
                } else {
                    true
                }
            });
            // Skip over a closing quote/bracket if the cursor is already on it.
            if matches!(ch, ')' | ']' | '}' | '"' | '\'' | '`') {
                let line = self.doc().line(self.view().cursor.line);
                if line[self.view().cursor.byte..].starts_with(ch) {
                    let next = self.doc().next(self.view().cursor);
                    self.view_mut().cursor = next;
                    self.refresh(hwnd);
                    return;
                }
            }
            let text = if ch == '\r' {
                // Auto-indent: add extra indentation after { or :
                let cursor = self.view().cursor;
                let line = self.doc().line(cursor.line);
                let indent: String = line
                    .chars()
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .collect();
                let trimmed = line.trim_end();
                if trimmed.ends_with('{') || trimmed.ends_with(':') {
                    format!("\n{}    ", indent)
                } else {
                    format!("\n{}", indent)
                }
            } else if should_auto_close {
                format!("{}{}", ch, closing.unwrap())
            } else {
                ch.to_string()
            };
            self.cancel_transition(hwnd);
            self.replace_selection(&text);
            // For auto-close pairs, move the cursor back before the closing char.
            if should_auto_close && ch != '\r' {
                let pos = self.doc().previous(self.view().cursor);
                self.view_mut().cursor = pos;
            }
            self.refresh(hwnd);
        }
    }

    // Clicking the gutter toggles a breakpoint on that line instead of moving
    // the caret; debugging is keyed off document state, not editor selection.
    fn toggle_breakpoint_at(&mut self, hwnd: HWND, pane: usize, y: i32) {
        let tab_index = self.tab_for_pane(pane);
        let view = self.view_for_pane(pane);
        let row = ((y - self.editor_top()) / self.line_height).max(0) as usize;
        let line = (view.first_line + row).min(self.tabs[tab_index].document.line_count() - 1);
        self.tabs[tab_index].document.toggle_breakpoint(line);
        self.refresh(hwnd);
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

    // Every welcome-screen target maps onto an existing command, so the page
    // never offers something the rest of the app cannot actually do.
    fn run_welcome_action(&mut self, hwnd: HWND, action: WelcomeAction) {
        match action {
            WelcomeAction::Explorer => {
                self.welcome = false;
                self.side_view = SideView::Files;
                self.panel_focus = false;
                self.set_sidebar_visible(hwnd, true);
                self.show_active_tab(hwnd);
            }
            WelcomeAction::Search => self.open_project_search(hwnd),
            WelcomeAction::SourceControl => self.show_review(hwnd),
            WelcomeAction::RunDebug => self.run_active_file(hwnd),
            WelcomeAction::Extensions => {
                self.welcome = false;
                self.set_sidebar_visible(hwnd, true);
                self.side_view = SideView::Extensions;
                self.status = "Extensions".into();
                self.refresh(hwnd);
            }
            WelcomeAction::AiAssistant => {
                self.status = "AI Assistant is not installed".into();
                self.refresh(hwnd);
            }
            WelcomeAction::OpenFile => self.open(hwnd, None),
            WelcomeAction::OpenFolder => self.open_folder(hwnd),
            WelcomeAction::NewFile => self.new_file(hwnd),
            WelcomeAction::Terminal => self.open_terminal(hwnd),
            WelcomeAction::CommandPalette => self.show_quick_open(hwnd),
            WelcomeAction::Community => {
                use windows_sys::Win32::UI::Shell::ShellExecuteW;
                let operation = wide("open");
                let url = wide("https://github.com/mehmoodulhaq570/LightLine");
                unsafe {
                    ShellExecuteW(
                        hwnd,
                        operation.as_ptr(),
                        url.as_ptr(),
                        null(),
                        null(),
                        SW_SHOWNORMAL,
                    );
                }
                self.status = "Opened the project page in your browser".into();
                self.refresh(hwnd);
            }
            WelcomeAction::Recent(index) => {
                if let Some(path) = self.recent.get(index).cloned() {
                    self.set_workspace(hwnd, path);
                }
            }
        }
    }

    pub(super) fn mouse_click(&mut self, hwnd: HWND, x: i32, y: i32, extend: bool) {
        self.clear_hover(hwnd);
        let mut rect = RECT::default();
        unsafe {
            GetClientRect(hwnd, &mut rect);
        }
        if self.welcome {
            if let Some(action) = self.welcome_layout(rect).hit(x, y) {
                self.run_welcome_action(hwnd, action);
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
        if self.ai_assistant_visible {
            let gap = self.chrome_gap();
            let ai_left = self.editor_right(hwnd) + gap;
            if x >= ai_left && x < rect.right - gap {
                let chrome_top = self.chrome_top();
                if y >= chrome_top && y <= chrome_top + self.scale(42) {
                    if x >= rect.right - gap - self.scale(30) {
                        self.ai_assistant_visible = false;
                        self.refresh(hwnd);
                        return;
                    }
                    if x >= rect.right - gap - self.scale(54) {
                        self.status = "Cleared Tera conversation".into();
                        self.refresh(hwnd);
                        return;
                    }
                }
                return;
            }
        }
        if y >= rect.bottom - self.scale(STATUS) {
            self.click_status_language(hwnd, rect, x, y);
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
                    0 => self.toggle_side_view(hwnd, SideView::Files),
                    1 => self.toggle_side_view(hwnd, SideView::Search),
                    2 => self.toggle_side_view(hwnd, SideView::Review),
                    3 => self.toggle_side_view(hwnd, SideView::Debug),
                    4 => self.toggle_side_view(hwnd, SideView::Extensions),
                    5 => {
                        self.status = "AI Assistant is planned for a future enhancement".into();
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
                let panel_left = self.scale(RAIL);
                match self.git_hit(x, y, panel_left, editor_left) {
                    GitHit::CommitBox => {
                        self.commit_focus = true;
                        self.panel_focus = true;
                        unsafe { InvalidateRect(hwnd, null(), 0) };
                    }
                    hit => {
                        // Anywhere else takes the keyboard away from the box.
                        self.commit_focus = false;
                        match hit {
                            GitHit::CommitButton => self.git_commit_pressed(hwnd),
                            GitHit::Refresh => self.refresh_git(hwnd),
                            GitHit::Push => self.git_remote(hwnd, workflow::RemoteAction::Push),
                            GitHit::Pull => self.git_remote(hwnd, workflow::RemoteAction::Pull),
                            GitHit::Fetch => {
                                self.git_remote(hwnd, workflow::RemoteAction::Fetch)
                            }
                            GitHit::StageAll => self.git_stage_all(hwnd),
                            GitHit::UnstageAll => self.git_unstage_all(hwnd),
                            GitHit::Toggle(index) => self.git_toggle_row(hwnd, index),
                            GitHit::Discard(index) => self.git_discard_row(hwnd, index),
                            GitHit::Row(index) => {
                                self.panel_selected = index;
                                self.panel_focus = false;
                                self.git_activate_row(hwnd, index);
                            }
                            _ => {}
                        }
                        unsafe { InvalidateRect(hwnd, null(), 0) };
                    }
                }
                return;
            }
            if self.side_view == SideView::Debug {
                let rail = self.scale(RAIL);
                if y >= self.scale(47) && y <= self.scale(75) {
                    for index in 0..4 {
                        let rect = self.debug_toolbar_button(rail, editor_left, index);
                        if x >= rect.left && x < rect.right {
                            match index {
                                0 => self.debug_continue(hwnd),
                                1 => self.debug_step_over(hwnd),
                                2 => self.debug_step_in(hwnd),
                                _ => self.debug_stop(hwnd),
                            }
                            return;
                        }
                    }
                    return;
                }
                if x >= rail && x < editor_left {
                    let bottom = (rect.bottom - self.scale(STATUS)).max(0);
                    let start_y = self.debug_variables_start_y();
                    let (rows, _) = self.debug_variable_rows(start_y, bottom, self.scale(18), self.scale(16));
                    for row in &rows {
                        if row.expandable && y >= row.y && y < row.y + self.scale(18) {
                            self.toggle_debug_variable(hwnd, row.reference);
                            return;
                        }
                    }
                }
                return;
            }
            if self.side_view == SideView::Extensions {
                let s = |v: i32| self.scale(v);
                let rail = self.scale(RAIL);
                let left = rail;

                // 1. Search bar click
                if y >= s(46) && y <= s(76) {
                    let search_right = editor_left - s(8);
                    // Clear button click
                    if !self.extensions_query.is_empty() && x >= search_right - s(30) && x <= search_right {
                        self.extensions_query.clear();
                        self.refresh(hwnd);
                        return;
                    }
                    self.extensions_search_active = true;
                    self.search_input = false;
                    self.panel_focus = true;
                    self.ensure_zed_registry_loaded(hwnd);
                    self.refresh(hwnd);
                    return;
                }

                // 2. Subtabs click (Segmented Pill Capsule)
                if y >= s(84) && y <= s(110) {
                    let tabs_left = left + s(8);
                    let tabs_right = editor_left - s(8);
                    let half_w = (tabs_right - tabs_left) / 2;
                    if x >= tabs_left && x < tabs_left + half_w {
                        self.extensions_tab = ExtensionsTab::Marketplace;
                        self.refresh(hwnd);
                        return;
                    }
                    if x >= tabs_left + half_w && x <= tabs_right {
                        self.extensions_tab = ExtensionsTab::Installed;
                        self.refresh(hwnd);
                        return;
                    }
                }

                // 3. Card action button click
                let card_h = s(86);
                let card_step = card_h + s(8);
                let start_y = s(136);
                let visible = self.filtered_extensions();
                if y >= start_y {
                    let row = ((y - start_y) / card_step.max(1)) as usize;
                    if row < visible.len() {
                        let ey = start_y + row as i32 * card_step;
                        let card_right = editor_left - s(8);
                        let btn_w = s(76);
                        let btn_h = s(22);
                        let btn_left = card_right - btn_w - s(8);
                        let btn_right = card_right - s(8);
                        let btn_top = ey + s(51);
                        let btn_bottom = btn_top + btn_h;

                        // Generous hit box around the button
                        if x >= btn_left - s(8) && x <= btn_right + s(8) && y >= btn_top - s(6) && y <= btn_bottom + s(8) {
                            let id = visible[row].id.to_string();
                            self.toggle_extension(hwnd, &id);
                            return;
                        }
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
            let top = self.terminal_top(hwnd);
            let header_bottom = top + self.scale(34);
            if y < header_bottom {
                match self.terminal_header_hit(editor_left, rect.right, top, x, y) {
                    // The far-right close hides the panel but keeps every
                    // shell session running, exactly like dismissing a dock.
                    TerminalHeaderHit::Hide => self.close_terminal(hwnd),
                    TerminalHeaderHit::OutputTab => {
                        self.switch_terminal_tab(hwnd, TerminalTab::Output);
                    }
                    TerminalHeaderHit::TerminalTab(index) => self.select_terminal(hwnd, index),
                    TerminalHeaderHit::New => self.new_terminal(hwnd, false),
                    TerminalHeaderHit::Kill => self.close_active_terminal(hwnd),
                    TerminalHeaderHit::Body => {
                        self.focus_terminal(hwnd);
                        self.start_terminal_selection(hwnd, x, y);
                    }
                }
                return;
            }
            self.focus_terminal(hwnd);
            self.start_terminal_selection(hwnd, x, y);
            return;
        }
        if self.side_view == SideView::Review
            && self.review_file.is_some()
            && y >= self.editor_top()
        {
            // The two panes mirror a file that is also open in the editor, so a
            // click jumps to the line under the pointer instead of doing nothing.
            self.open_diff_line(hwnd, y);
            return;
        }
        if self.split_visible
            && y >= self.tab_strip_bottom()
            && (x - self.pane_divider(hwnd)).abs() <= self.scale(6)
        {
            self.divider_dragging = true;
            unsafe { SetCapture(hwnd) };
            return;
        }
        // The editor is a card inset from the window edge, so the tab strip's
        // controls are measured from the card's right edge, not the window's.
        let card_right = rect.right - self.chrome_gap();
        if y < self.tab_strip_bottom() {
            if Tab::is_runnable(self.doc())
                && x >= card_right - self.scale(326)
                && x < card_right - self.scale(296)
            {
                self.run_active_file(hwnd);
                return;
            }
            if x >= card_right - self.scale(112) {
                self.toggle_split(hwnd);
                return;
            }
            if editor_left
                + self.scale(TAB_WIDTH) * self.tabs.len().saturating_sub(self.tab_first) as i32
                + self.scale(12)
                < card_right - self.scale(285)
                && x >= card_right - self.scale(285)
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
        let pane = self.focused_pane;
        if x < self.pane_left(hwnd, pane) + self.scale(GUTTER) {
            self.toggle_breakpoint_at(hwnd, pane, y);
            return;
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
