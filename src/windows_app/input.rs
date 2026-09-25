use super::git::GitHit;
use super::terminal::{TERMINAL_HEADER, TerminalHeaderHit};
use super::*;

fn is_editor_ctrl_key(key: u32, shift: bool) -> bool {
    matches!(key, 0x41 | 0x43 | 0x56 | 0x5a | 0x59)
        || (key == 0x58 && !shift)
        || key == VK_HOME as u32
        || key == VK_END as u32
        || key == VK_LEFT as u32
        || key == VK_RIGHT as u32
        || key == VK_SPACE as u32
        || key == VK_BACK as u32
        || key == VK_DELETE as u32
}

fn is_editor_navigation_or_edit_key(key: u32) -> bool {
    key == VK_LEFT as u32
        || key == VK_RIGHT as u32
        || key == VK_UP as u32
        || key == VK_DOWN as u32
        || key == VK_PRIOR as u32
        || key == VK_NEXT as u32
        || key == VK_HOME as u32
        || key == VK_END as u32
        || key == VK_TAB as u32
        || key == VK_BACK as u32
        || key == VK_DELETE as u32
}

impl App {
    pub(super) fn key(&mut self, hwnd: HWND, key: u32) -> bool {
        self.clear_hover(hwnd);
        let ctrl = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
        let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
        if self.terminal_focus
            && !self.quick_open
            && !self.search_input
            && !(self.side_view == SideView::Extensions && self.extensions_search_active)
            && !(self.side_view == SideView::Review && self.commit_focus)
            && self.explorer_input.is_none()
        {
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
                    0x50 | 0x52 | 0x42 | 0x57 => shift, // Ctrl+Shift+P/R/B/W
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
        if let Some(input) = self.explorer_input.clone() {
            match key {
                x if x == VK_ESCAPE as u32 => {
                    self.explorer_input = None;
                    self.refresh(hwnd);
                }
                x if x == VK_BACK as u32 => {
                    if let Some(input) = &mut self.explorer_input {
                        input.buffer.pop();
                    }
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
                x if x == VK_RETURN as u32 => {
                    let buf = input.buffer.trim().to_string();
                    self.explorer_input = None;
                    if !buf.is_empty() {
                        if input.is_rename {
                            if let Some(old) = input.old_path {
                                self.rename_entry(hwnd, &old, &buf);
                            }
                        } else if input.is_folder {
                            self.create_folder_at(hwnd, &input.target_dir, &buf);
                        } else {
                            self.create_file_at(hwnd, &input.target_dir, &buf);
                        }
                    }
                    self.refresh(hwnd);
                }
                _ => {}
            }
            return true;
        }
        if key == VK_DELETE as u32
            && self.side_view == SideView::Files
            && self.explorer_visible
            && self.panel_focus
            && !self.quick_open
            && self.explorer_input.is_none()
            && let Some(path) = self.selected_explorer_path.clone()
        {
            self.delete_entry(hwnd, &path);
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
                    if !self.extensions_query.is_empty() {
                        self.extensions_query.clear();
                    } else {
                        self.extensions_search_active = false;
                        self.panel_focus = false;
                    }
                }
                x if x == VK_RETURN as u32 => {
                    self.panel_focus = true;
                }
                x if x == VK_BACK as u32 => {
                    if ctrl {
                        self.extensions_query.clear();
                    } else {
                        self.extensions_query.pop();
                    }
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
                x if x == VK_ESCAPE as u32 => {
                    self.panel_focus = false;
                    if self.side_view == SideView::Search {
                        self.cancel_search();
                        self.side_view = SideView::Files;
                        self.set_sidebar_visible(hwnd, false);
                        self.review_file = None;
                        self.keep_cursor_visible(hwnd);
                    } else {
                        unsafe { InvalidateRect(hwnd, null(), 0) };
                    }
                    return true;
                }
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
            // Keep editor navigation and deletion away from the hidden caret,
            // but let application commands such as F5/F10/F11 continue below.
            if is_editor_navigation_or_edit_key(key) {
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
        if ctrl && self.panel_focus && is_editor_ctrl_key(key, shift) {
            return true;
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
                0x57 if shift => {
                    self.close_workspace(hwnd);
                    return true;
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
                        self.recompute_gutter_diff();
                    }
                }
                0x59 => {
                    self.view_mut().selection_anchor = None;
                    if let Some((cursor, line)) = self.doc_mut().redo() {
                        self.view_mut().cursor = cursor;
                        self.syntax_changed(line);
                        self.revalidate_other_view(None);
                        self.sync_lsp_edit();
                        self.recompute_gutter_diff();
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
                    self.search_input = false;
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
        if self.terminal_focus
            && !self.quick_open
            && !self.search_input
            && !(self.side_view == SideView::Extensions && self.extensions_search_active)
            && !(self.side_view == SideView::Review && self.commit_focus)
            && self.explorer_input.is_none()
        {
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
        if let Some(input) = &mut self.explorer_input {
            if unit >= 32
                && unit != 127
                && let Some(ch) = char::from_u32(unit as u32)
                && !['/', '\\', ':', '*', '?', '"', '<', '>', '|'].contains(&ch)
            {
                input.buffer.push(ch);
                unsafe { InvalidateRect(hwnd, null(), 0) };
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
        if self.panel_focus {
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
                let cursor = self.view().cursor;
                let line = self.doc().line(cursor.line);
                let indent: String = line
                    .chars()
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .collect();
                let trimmed = line.trim_end();
                // Auto-indent: add one extra indent level after { or :,
                // unless the user has turned it off.
                if self.settings.auto_indent && (trimmed.ends_with('{') || trimmed.ends_with(':')) {
                    let unit = if self.settings.insert_spaces {
                        " ".repeat(self.settings.tab_size)
                    } else {
                        "\t".to_string()
                    };
                    format!("\n{indent}{unit}")
                } else {
                    format!("\n{indent}")
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

    // Clicking the gutter toggles a breakpoint or code fold on that line instead of moving
    // the caret; debugging is keyed off document state, not editor selection.
    fn toggle_breakpoint_at(&mut self, hwnd: HWND, pane: usize, y: i32) {
        let tab_index = self.tab_for_pane(pane);
        let first_line = self.view_for_pane(pane).first_line;
        let row = ((y - self.editor_top()) / self.line_height).max(0) as usize;
        let line = self.tabs[tab_index]
            .document
            .visual_row_to_doc_line(first_line, row)
            .unwrap_or_else(|| self.tabs[tab_index].document.line_count().saturating_sub(1));
        self.tabs[tab_index].document.toggle_breakpoint(line);
        self.refresh(hwnd);
    }

    fn toggle_fold_at(&mut self, hwnd: HWND, pane: usize, y: i32) {
        let tab_index = self.tab_for_pane(pane);
        let first_line = self.view_for_pane(pane).first_line;
        let row = ((y - self.editor_top()) / self.line_height).max(0) as usize;
        let doc = &mut self.tabs[tab_index].document;
        let line = doc
            .visual_row_to_doc_line(first_line, row)
            .unwrap_or_else(|| doc.line_count().saturating_sub(1));
        if doc.toggle_fold(line) {
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
        let line = tab
            .document
            .visual_row_to_doc_line(view.first_line, row)
            .unwrap_or_else(|| tab.document.line_count().saturating_sub(1));
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
        // The custom title bar is shared by the editor and Welcome screen.
        // Handle it before page-specific hit testing so the window controls
        // and command center remain live on both surfaces.
        if y < self.chrome_top() {
            let title_button = self.scale(46);
            let controls_left = rect.right - title_button * 3;
            if x >= controls_left {
                let button = ((x - controls_left) / title_button.max(1)).clamp(0, 2);
                unsafe {
                    match button {
                        0 => ShowWindow(hwnd, SW_MINIMIZE),
                        1 => ShowWindow(
                            hwnd,
                            if IsZoomed(hwnd) != 0 { SW_RESTORE } else { SW_MAXIMIZE },
                        ),
                        _ => PostMessageW(hwnd, WM_CLOSE, 0, 0),
                    };
                }
                return;
            }
            let command = self.command_center_rect(hwnd);
            if x >= command.left
                && x < command.right
                && y >= command.top
                && y < command.bottom
            {
                self.show_quick_open(hwnd);
            } else if x < self.scale(176) {
                self.show_welcome(hwnd);
            }
            return;
        }
        if self.welcome {
            if let Some(action) = self.welcome_layout(rect).hit(x, y) {
                self.run_welcome_action(hwnd, action);
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
            self.terminal_focus = false;
            let y = y - self.chrome_top();
            let panel_bottom = rect.bottom - self.chrome_top();
            if y >= self.scale(RAIL_FIRST_ROW)
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
            } else if y >= panel_bottom - self.scale(STATUS + 40) {
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
            self.terminal_focus = false;
            let y = y - self.chrome_top();
            let panel_bottom = rect.bottom - self.chrome_top();
            if !self.explorer_visible {
                return;
            }
            if y < self.scale(39) {
                if x >= editor_left - self.scale(26) {
                    self.set_sidebar_visible(hwnd, false);
                    return;
                }
                if self.side_view == SideView::Files
                    && x >= editor_left - self.scale(50)
                    && x < editor_left - self.scale(26)
                {
                    self.collapse_all_folders(hwnd);
                    return;
                }
            }
            if self.side_view == SideView::Search {
                if y >= self.scale(47) && y < self.scale(78) {
                    self.search_input = true;
                    self.panel_focus = true;
                    self.refresh(hwnd);
                    return;
                }
                if y >= self.scale(113) {
                    let index =
                        self.panel_first + ((y - self.scale(113)) / self.scale(48).max(1)) as usize;
                    if let Some(hit) = self.search_results.get(index).cloned() {
                        self.search_input = false;
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
                    let bottom = (panel_bottom - self.scale(STATUS)).max(0);
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
                let (dpi, zoom) = (self.dpi, self.zoom);
                let s = |v: i32| scaled(v, dpi, zoom);
                let rail = s(RAIL);
                let left = rail;

                // 1. Search bar click
                if y >= s(46) && y <= s(76) {
                    let right = self.sidebar_right();
                    let search_left = left + s(8);
                    let search_right = right - s(8);
                    if x >= search_left && x <= search_right {
                        // Clear button click
                        if !self.extensions_query.is_empty() && x >= search_right - s(30) && x <= search_right {
                            self.extensions_query.clear();
                            self.refresh(hwnd);
                            return;
                        }
                        self.extensions_search_active = true;
                        self.search_input = false;
                        self.commit_focus = false;
                        self.panel_focus = true;
                        self.terminal_focus = false;
                        self.ensure_zed_registry_loaded(hwnd);
                        self.refresh(hwnd);
                        return;
                    }
                }

                // 2. Subtabs click (Segmented Pill Capsule)
                if y >= s(84) && y <= s(110) {
                    self.extensions_search_active = false;
                    self.terminal_focus = false;
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
            if self.explorer_input.is_some() {
                self.explorer_input = None;
                self.refresh(hwnd);
            }
            if y >= self.scale(40)
                && y < self.scale(EXPLORER_TOP)
                && let Some(root) = self.workspace_root.clone()
            {
                    let s = |v: i32| self.scale(v);
                    if x >= editor_left - s(26) && x <= editor_left - s(4) {
                        self.close_workspace(hwnd);
                        return;
                    } else if x >= editor_left - s(48) && x < editor_left - s(26) {
                        self.directory_cache.clear();
                        self.load_directory(&root);
                        self.refresh(hwnd);
                        return;
                    } else if x >= editor_left - s(70) && x < editor_left - s(48) {
                        let target = self.selected_dir_or_root().unwrap_or(root);
                        self.start_explorer_input(target, true, false, None, hwnd);
                        return;
                    } else if x >= editor_left - s(92) && x < editor_left - s(70) {
                        let target = self.selected_dir_or_root().unwrap_or(root);
                        self.start_explorer_input(target, false, false, None, hwnd);
                        return;
                    } else {
                        if self.expanded_dirs.contains(&root) {
                            self.expanded_dirs.remove(&root);
                        } else {
                            self.expanded_dirs.insert(root.clone());
                            self.load_directory(&root);
                        }
                        self.selected_explorer_path = Some(root);
                        self.panel_focus = true;
                        self.refresh(hwnd);
                        return;
                    }
            }
            if y >= panel_bottom - self.scale(STATUS + 35) {
                self.show_active_tab(hwnd);
                return;
            }
            if y >= self.scale(EXPLORER_TOP) && y < panel_bottom - self.scale(STATUS + 38) {
                self.panel_focus = true;
                let row = self.explorer_first_row
                    + ((y - self.scale(EXPLORER_TOP)) / self.scale(EXPLORER_ROW)) as usize;
                if let Some(item) = self.explorer_rows().get(row) {
                    let path = item.entry.path.clone();
                    self.selected_explorer_path = Some(path.clone());
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
                    TerminalHeaderHit::Problems => {
                        self.status = "No problems in the active workspace".into();
                        self.refresh(hwnd);
                    }
                    TerminalHeaderHit::OutputTab => {
                        self.switch_terminal_tab(hwnd, TerminalTab::Output);
                    }
                    TerminalHeaderHit::TerminalTab(index) => self.select_terminal(hwnd, index),
                    TerminalHeaderHit::New => self.new_terminal(hwnd, false),
                    TerminalHeaderHit::ShellPicker => self.show_shell_picker_menu(hwnd, x, y),
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
        // The split control is drawn at the right edge of each pane's breadcrumb
        // row, from App::pane_actions. That row had no hit test of its own, so
        // the glyph was decoration and a click in it only focused a pane. This
        // is checked ahead of the divider grab zone because the two overlap at a
        // pane's edge, and a control the user can see should beat a drag.
        if y >= self.tab_strip_bottom() && y < self.editor_top() {
            for pane in 0..if self.split_visible { 2 } else { 1 } {
                let (split, _) = self.pane_actions(self.pane_right(hwnd, pane));
                if x >= split.left && x < split.right {
                    self.focus_pane(hwnd, pane);
                    self.toggle_split(hwnd);
                    return;
                }
            }
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
            let command = self.command_center_rect(hwnd);
            if x >= command.left
                && x < command.right
                && y >= command.top
                && y < command.bottom
            {
                self.show_quick_open(hwnd);
                return;
            }
            if Tab::is_runnable(self.doc())
                && x >= card_right - self.scale(82)
                && x < card_right - self.scale(50)
            {
                self.run_active_file(hwnd);
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
        let pane_left = self.pane_left(hwnd, pane);
        if x < pane_left + self.scale(GUTTER) {
            if x < pane_left + self.scale(24) {
                self.toggle_breakpoint_at(hwnd, pane, y);
            } else {
                self.toggle_fold_at(hwnd, pane, y);
            }
            return;
        }
        let pos = self.position_at(hwnd, x, y);
        self.panel_focus = false;
        self.terminal_focus = false;
        self.extensions_search_active = false;
        self.search_input = false;
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

    pub(super) fn mouse_right_click(&mut self, hwnd: HWND, x: i32, y: i32) {
        if self.terminal_visible {
            let mut rect = RECT::default();
            unsafe { GetClientRect(hwnd, &mut rect) };
            let left = self.editor_left();
            let top = self.terminal_top(hwnd);
            let right = rect.right;
            if x >= left && x < right && y >= top && y < top + self.scale(TERMINAL_HEADER) {
                let hit = self.terminal_header_hit(left, right, top, x, y);
                if matches!(hit, TerminalHeaderHit::New | TerminalHeaderHit::ShellPicker) {
                    self.show_shell_picker_menu(hwnd, x, y);
                    return;
                }
            }
        }

        let editor_left = self.editor_left();
        let rail = self.scale(RAIL);
        if x < rail || x >= editor_left || self.side_view != SideView::Files || !self.explorer_visible {
            return;
        }
        let Some(root) = self.workspace_root.clone() else {
            return;
        };

        let mut target_path: Option<PathBuf> = None;
        let mut is_dir = true;

        let panel_y = y - self.chrome_top();
        let row_top = self.scale(EXPLORER_TOP);
        if panel_y >= row_top {
            let row_idx = self.explorer_first_row
                + ((panel_y - row_top) / self.scale(EXPLORER_ROW)) as usize;
            if let Some(row) = self.explorer_rows().get(row_idx) {
                target_path = Some(row.entry.path.clone());
                is_dir = row.entry.is_dir;
                self.selected_explorer_path = Some(row.entry.path.clone());
            }
        }

        if target_path.is_none() {
            target_path = Some(root.clone());
            is_dir = true;
            self.selected_explorer_path = Some(root.clone());
        }

        let clicked_path = target_path.unwrap();

        use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, TrackPopupMenu, MF_SEPARATOR, MF_STRING,
            TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
        };

        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }

            const CMD_NEW_FILE: usize = 1;
            const CMD_NEW_FOLDER: usize = 2;
            const CMD_REVEAL: usize = 3;
            const CMD_COPY_PATH: usize = 4;
            const CMD_COPY_REL_PATH: usize = 5;
            const CMD_RENAME: usize = 6;
            const CMD_DELETE: usize = 7;
            const CMD_CLOSE_WORKSPACE: usize = 8;

            AppendMenuW(menu, MF_STRING, CMD_NEW_FILE, wide("New File...").as_ptr());
            AppendMenuW(menu, MF_STRING, CMD_NEW_FOLDER, wide("New Folder...").as_ptr());
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(menu, MF_STRING, CMD_REVEAL, wide("Reveal in File Explorer").as_ptr());
            AppendMenuW(menu, MF_STRING, CMD_COPY_PATH, wide("Copy Path").as_ptr());
            AppendMenuW(
                menu,
                MF_STRING,
                CMD_COPY_REL_PATH,
                wide("Copy Relative Path").as_ptr(),
            );

            if clicked_path != root {
                AppendMenuW(menu, MF_SEPARATOR, 0, null());
                AppendMenuW(menu, MF_STRING, CMD_RENAME, wide("Rename...").as_ptr());
                AppendMenuW(menu, MF_STRING, CMD_DELETE, wide("Delete").as_ptr());
            } else {
                AppendMenuW(menu, MF_SEPARATOR, 0, null());
                AppendMenuW(
                    menu,
                    MF_STRING,
                    CMD_CLOSE_WORKSPACE,
                    wide("Close Folder").as_ptr(),
                );
            }

            let mut pt = POINT { x, y };
            ClientToScreen(hwnd, &mut pt);

            let cmd = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_LEFTALIGN | TPM_RIGHTBUTTON,
                pt.x,
                pt.y,
                0,
                hwnd,
                null(),
            ) as usize;

            DestroyMenu(menu);

            let parent_dir = if is_dir {
                clicked_path.clone()
            } else {
                clicked_path
                    .parent()
                    .unwrap_or(&root)
                    .to_path_buf()
            };

            match cmd {
                CMD_NEW_FILE => {
                    self.start_explorer_input(parent_dir, false, false, None, hwnd);
                }
                CMD_NEW_FOLDER => {
                    self.start_explorer_input(parent_dir, true, false, None, hwnd);
                }
                CMD_REVEAL => {
                    let path_str = clicked_path.to_string_lossy().to_string();
                    std::thread::spawn(move || {
                        let _ = std::process::Command::new("explorer")
                            .args(["/select,", &path_str])
                            .spawn();
                    });
                }
                CMD_COPY_PATH => {
                    let _ = clipboard::copy(hwnd, &clicked_path.to_string_lossy());
                    self.status = "Path copied to clipboard".into();
                    InvalidateRect(hwnd, null(), 0);
                }
                CMD_COPY_REL_PATH => {
                    let rel = clicked_path.strip_prefix(&root).unwrap_or(&clicked_path);
                    let _ = clipboard::copy(hwnd, &rel.to_string_lossy());
                    self.status = "Relative path copied to clipboard".into();
                    InvalidateRect(hwnd, null(), 0);
                }
                CMD_RENAME => {
                    let old_name = clicked_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    self.start_explorer_input(
                        parent_dir,
                        is_dir,
                        true,
                        Some(clicked_path),
                        hwnd,
                    );
                    if let Some(input) = &mut self.explorer_input {
                        input.buffer = old_name;
                    }
                    self.refresh(hwnd);
                }
                CMD_DELETE => {
                    self.delete_entry(hwnd, &clicked_path);
                }
                CMD_CLOSE_WORKSPACE => {
                    self.close_workspace(hwnd);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;

    #[test]
    fn sidebar_guard_allows_extensions_shortcut() {
        assert!(is_editor_ctrl_key(0x58, false));
        assert!(!is_editor_ctrl_key(0x58, true));
        assert!(is_editor_ctrl_key(0x5a, true));
        assert!(is_editor_navigation_or_edit_key(VK_BACK as u32));
        assert!(is_editor_navigation_or_edit_key(VK_DELETE as u32));
        assert!(!is_editor_navigation_or_edit_key(VK_F5 as u32));
        assert!(!is_editor_navigation_or_edit_key(VK_F10 as u32));
        assert!(!is_editor_navigation_or_edit_key(VK_F11 as u32));
    }
}
