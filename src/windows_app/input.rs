use super::git::GitHit;
use super::terminal::{TERMINAL_HEADER, TerminalHeaderHit, TerminalProfileMenuHit};
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

pub(super) fn decode_utf16_input(pending_high: &mut Option<u16>, unit: u16) -> Option<char> {
    if (0xd800..=0xdbff).contains(&unit) {
        *pending_high = Some(unit);
        None
    } else if (0xdc00..=0xdfff).contains(&unit) {
        let high = pending_high.take()?;
        char::decode_utf16([high, unit]).next()?.ok()
    } else {
        *pending_high = None;
        char::decode_utf16([unit]).next()?.ok()
    }
}

impl App {
    pub(super) fn key(&mut self, hwnd: HWND, key: u32) -> bool {
        self.clear_hover(hwnd);
        let ctrl = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
        let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
        let alt = unsafe { GetKeyState(VK_MENU as i32) } < 0;
        if self.run_choice_key(hwnd, key) {
            return true;
        }
        if self.run_config_panel.is_some() {
            self.run_config_key(hwnd, key, ctrl, shift);
            return true;
        }
        if self.terminal_rename_input.is_some() {
            if !self.terminal_focus
                || !self.terminal_visible
                || self.terminal_tab != TerminalTab::Terminal
            {
                self.terminal_rename_input = None;
                unsafe { InvalidateRect(hwnd, null(), 0) };
            } else {
                match key {
                    x if x == VK_ESCAPE as u32 => {
                        self.terminal_rename_input = None;
                        unsafe { InvalidateRect(hwnd, null(), 0) };
                        return true;
                    }
                    x if x == VK_BACK as u32 => {
                        if let Some((_, name)) = &mut self.terminal_rename_input {
                            name.pop();
                        }
                        unsafe { InvalidateRect(hwnd, null(), 0) };
                        return true;
                    }
                    x if x == VK_RETURN as u32 => {
                        self.finish_terminal_rename(hwnd);
                        return true;
                    }
                    _ => return true,
                }
            }
        }
        if self.rename_key(hwnd, key, ctrl) || self.code_action_key(hwnd, key) {
            return true;
        }
        if self.editor_context_key(hwnd, key, ctrl, shift) {
            return true;
        }
        if self.more_menu_key(hwnd, key) {
            return true;
        }
        if alt && key == 0x5A && !ctrl && !shift && !self.terminal_focus && !self.welcome {
            self.toggle_word_wrap(hwnd);
            return true;
        }
        // The model search owns keys whenever its popup is open. Route it
        // directly instead of depending on the chat composer's focus state.
        if self.ai.model_menu_open {
            if self.ai_key(hwnd, key, ctrl, shift) {
                return true;
            }
        } else if self.ai_typing() && self.ai_key(hwnd, key, ctrl, shift) {
            return true;
        }
        if self.terminal_profile_menu_open && key == VK_ESCAPE as u32 && !ctrl && !shift {
            self.terminal_profile_menu_open = false;
            self.terminal_profile_defaults_open = false;
            self.terminal_profile_availability.clear();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return true;
        }
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
                    0x50 => true,                          // Ctrl+P and Ctrl+Shift+P
                    0x52 | 0x42 | 0x57 => shift,           // Ctrl+Shift+R/B/W
                    v if v == VK_OEM_COMMA as u32 => true, // Ctrl+, (settings)
                    v if v == VK_TAB as u32 => true,
                    v if v == VK_PRIOR as u32 || v == VK_NEXT as u32 => true,
                    _ => false,
                };
            // A program being debugged runs in the Output tab; its debugger
            // keys still drive the session instead of reaching the program.
            let debugger_key = !ctrl
                && self.debug.is_some()
                && [VK_F5, VK_F10, VK_F11].iter().any(|&vk| key == vk as u32);
            if !reserved_chord && !debugger_key {
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
        if key == VK_F2 as u32
            && self.side_view == SideView::Files
            && self.explorer_visible
            && self.panel_focus
            && !self.quick_open
            && let Some(path) = self.selected_explorer_path.clone()
            && let Some(parent) = path.parent().map(Path::to_path_buf)
        {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            self.start_explorer_input(parent, path.is_dir(), true, Some(path), hwnd);
            if let Some(input) = &mut self.explorer_input {
                input.buffer = name;
            }
            return true;
        }
        if self.quick_open {
            match key {
                x if x == VK_ESCAPE as u32 => self.quick_open = false,
                x if x == VK_UP as u32 => self.quick_select(self.quick_selected.saturating_sub(1)),
                x if x == VK_DOWN as u32 => self.quick_select(self.quick_selected + 1),
                x if x == VK_PRIOR as u32 => {
                    self.quick_select(self.quick_selected.saturating_sub(QUICK_ROWS))
                }
                x if x == VK_NEXT as u32 => self.quick_select(self.quick_selected + QUICK_ROWS),
                x if x == VK_BACK as u32 => {
                    self.quick_query.pop();
                    self.quick_selected = 0;
                    self.quick_first = 0;
                    self.ensure_workspace_symbols();
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
                    self.search_project();
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
        if self.problems_key(hwnd, key, ctrl) {
            return true;
        }
        if self.panel_focus && !ctrl && self.side_view == SideView::Review && !self.commit_focus {
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
        // A Markdown preview has no caret: navigation keys scroll it.
        if self.tab().markdown.is_some() && !ctrl && self.markdown_key(hwnd, key) {
            return true;
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
                x if x == VK_F7 as u32 && shift => {
                    let prev_hunk = (0..self.diff_first).rev().find(|&i| {
                        self.diff_rows[i].changed && (i == 0 || !self.diff_rows[i - 1].changed)
                    });
                    if let Some(target) = prev_hunk {
                        self.diff_first = target;
                    }
                }
                x if x == VK_F7 as u32 => {
                    let next_hunk = ((self.diff_first + 1)..self.diff_rows.len())
                        .find(|&i| self.diff_rows[i].changed && !self.diff_rows[i - 1].changed);
                    if let Some(target) = next_hunk {
                        self.diff_first = target;
                    }
                }
                0x53 => {
                    if let Some(path) = self.review_file.clone() {
                        if self.review_staged {
                            self.git_write(hwnd, GitAction::Unstage(vec![path.clone()]));
                            self.review_staged = false;
                        } else {
                            self.git_write(hwnd, GitAction::Stage(vec![path.clone()]));
                            self.review_staged = true;
                        }
                        self.show_diff(hwnd, path, self.review_staged);
                    }
                }
                _ => return true,
            }
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return true;
        }
        if self.find_mode {
            match key {
                x if x == VK_ESCAPE as u32 => {
                    self.close_find(hwnd);
                    return true;
                }
                x if x == VK_TAB as u32 && self.replace_mode => {
                    self.replace_field = 1 - self.replace_field;
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    return true;
                }
                // Paste into the find box, not the document: its first line.
                0x56 if ctrl => {
                    match clipboard::paste(hwnd) {
                        Ok(Some(text)) => {
                            self.find_box_input(hwnd, Some(text.lines().next().unwrap_or("")))
                        }
                        Ok(None) => {}
                        Err(error) => self.error(hwnd, &error),
                    }
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
                // Not while a sidebar list has the keys: the editor caret is hidden.
                x if x == VK_F9 as u32 && !self.panel_focus => {
                    let line = self.view().cursor.line;
                    self.doc_mut().toggle_breakpoint(line);
                    self.refresh(hwnd);
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
        if !self.panel_focus && self.multi_cursor_key(hwnd, key, ctrl, shift) {
            return true;
        }
        if ctrl {
            let cursor = self.view().cursor;
            match key {
                0x44 if !shift && !alt && !self.panel_focus => {
                    self.add_next_match(hwnd);
                    return true;
                }
                x if alt && !self.panel_focus && (x == VK_UP as u32 || x == VK_DOWN as u32) => {
                    self.add_caret_vertically(hwnd, x == VK_DOWN as u32);
                    return true;
                }
                x if x == VK_SPACE as u32 && shift => {
                    self.request_signature(hwnd);
                    return true;
                }
                0x54 if !shift => {
                    self.show_workspace_symbols(hwnd);
                    return true;
                }
                // Ctrl+Shift+M, as in VS Code.
                0x4D if shift => {
                    if self.terminal_tab == TerminalTab::Problems && self.terminal_visible {
                        self.problem_focus = false;
                        self.hide_terminal(hwnd);
                    } else {
                        self.show_problems(hwnd);
                    }
                    return true;
                }
                x if x == VK_SPACE as u32 => {
                    self.trigger_completion(hwnd);
                    return true;
                }
                x if x == VK_OEM_PERIOD as u32 && !self.panel_focus => {
                    self.request_code_actions(hwnd);
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
                // As in VS Code: Ctrl+Shift+V previews the Markdown file.
                0x56 if shift => {
                    self.open_markdown_preview(hwnd, false);
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
                    self.open_find(hwnd, true);
                    return true;
                }
                0x50 => {
                    self.show_quick_open(hwnd);
                    if shift {
                        self.quick_query = ">".into();
                    }
                    return true;
                }
                0x45 if shift && self.ai_diagnostic_at_cursor().is_some() => {
                    self.ai_run_task(hwnd, AiTask::ExplainError);
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
                    self.open_find(hwnd, false);
                    return true;
                }
                0x42 => {
                    self.toggle_sidebar(hwnd);
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
                    self.open_settings_panel(hwnd);
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
                0x5a | 0x59 => self.undo_or_redo(key == 0x59 || shift),
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
        // Alt+\ asks the AI model for an inline completion at the caret.
        if alt && !shift && !ctrl && (key == VK_OEM_5 as u32 || key == 0xDC) {
            self.trigger_inline_ai(hwnd);
            return true;
        }
        // Shift+Alt+F formats the current document.
        if shift && alt && !ctrl && key == 0x46 {
            self.format_document(hwnd);
            return true;
        }
        if shift && alt && !ctrl && key == 0x4F {
            self.organize_imports(hwnd);
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
        if key == VK_TAB as u32 && !ctrl && !shift && self.accept_ghost_text(hwnd) {
            return true;
        }
        // Checked before the key acts: Backspace and Delete clear a hover card.
        let stays_in_editor = self.keystroke_stays_in_editor();
        let before = self.caret_frame(hwnd);
        match key {
            x if x == VK_F1 as u32 => {
                self.hover_at_cursor(hwnd);
                return true;
            }
            x if x == VK_F12 as u32 && shift => {
                self.find_references(hwnd);
                return true;
            }
            x if x == VK_F2 as u32 && !self.panel_focus => {
                self.start_rename(hwnd);
                return true;
            }
            x if x == VK_F12 as u32 => {
                self.goto_definition(hwnd);
                return true;
            }
            x if x == VK_F3 as u32 => {
                self.find(hwnd, !shift);
                return true;
            }
            x if x == VK_ESCAPE as u32 => {
                if self.signature.is_some() || self.signature_request.is_some() {
                    self.hide_signature(hwnd);
                    return true;
                }
                if self.clear_ghost_text(hwnd) {
                    return true;
                }
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
            // Vertical movement goes by screen rows (a wrapped line has
            // several, a folded block one) and keeps the caret's horizontal
            // position.
            x if x == VK_UP as u32 => {
                let target = self.cursor_moved_by_rows(hwnd, -1);
                self.move_cursor(target, shift);
            }
            x if x == VK_DOWN as u32 => {
                let target = self.cursor_moved_by_rows(hwnd, 1);
                self.move_cursor(target, shift);
            }
            x if x == VK_PRIOR as u32 => {
                let page = self.visible_lines(hwnd) as isize;
                let target = self.cursor_moved_by_rows(hwnd, -page);
                self.move_cursor(target, shift);
            }
            x if x == VK_NEXT as u32 => {
                let page = self.visible_lines(hwnd) as isize;
                let target = self.cursor_moved_by_rows(hwnd, page);
                self.move_cursor(target, shift);
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
        self.refresh_after_editor_input(hwnd, stays_in_editor, &before);
        self.follow_signature(hwnd, None);
        true
    }

    // After a key or a click moved the caret, selected or edited text: redraws
    // what that changed in the editor and the status bar (see caret_changes),
    // or everything when something outside them may have changed.
    fn refresh_after_editor_input(
        &mut self,
        hwnd: HWND,
        stays_in_editor: bool,
        before: &CaretFrame,
    ) {
        if stays_in_editor {
            // Measured after refresh, which can scroll to the caret.
            self.repaint_only(hwnd, &[], |app| app.refresh(hwnd));
            for area in self.caret_changes(hwnd, before) {
                unsafe { InvalidateRect(hwnd, &area, 0) };
            }
        } else {
            self.refresh(hwnd);
        }
    }

    pub(super) fn character(&mut self, hwnd: HWND, unit: u16) {
        if self.run_choice.is_some() {
            return;
        }
        if unsafe { GetKeyState(VK_CONTROL as i32) } < 0 {
            return;
        }
        if self.run_config_panel.is_some() {
            self.run_config_character(hwnd, unit);
            return;
        }
        if self.rename_char(hwnd, unit) {
            return;
        }
        if self.code_actions.is_some() || self.code_action_request.is_some() {
            self.dismiss_code_actions(hwnd);
        }
        if self.terminal_rename_input.is_some() {
            if !self.terminal_focus
                || !self.terminal_visible
                || self.terminal_tab != TerminalTab::Terminal
            {
                self.terminal_rename_input = None;
                unsafe { InvalidateRect(hwnd, null(), 0) };
            } else {
                if let Some(ch) = decode_utf16_input(&mut self.pending_high_surrogate, unit)
                    && let Some((_, name)) = &mut self.terminal_rename_input
                    && !ch.is_control()
                    && name.chars().count() < 48
                {
                    name.push(ch);
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
                return;
            }
        }
        if self.ai.model_menu_open && self.ai_assistant_visible {
            self.ai_char(hwnd, unit);
            return;
        }
        if self.ai_typing() {
            self.ai_char(hwnd, unit);
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
            if unit >= 32
                && unit != 127
                && let Some(ch) = char::from_u32(unit as u32)
            {
                self.commit_message.push(ch);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            return;
        }
        if self.quick_open
            || self.search_input
            || (self.side_view == SideView::Extensions && self.extensions_search_active)
        {
            if unit >= 32
                && unit != 127
                && let Some(ch) = char::from_u32(unit as u32)
            {
                if self.quick_open {
                    self.quick_query.push(ch);
                    self.quick_selected = 0;
                    self.quick_first = 0;
                    self.ensure_symbols(hwnd);
                    self.ensure_workspace_symbols();
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
            self.find_box_input(hwnd, None);
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
                    self.find_box_input(hwnd, Some(ch.encode_utf8(&mut [0; 4])));
                }
                return;
            }
            if self.multi_cursor_char(hwnd, ch) {
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
                    self.follow_signature(hwnd, Some(ch));
                    return;
                }
            }
            let text = if ch == '\r' {
                // A selection is replaced, so what stays on the line is the
                // text before its start.
                let start = self
                    .selection_range()
                    .map_or(self.view().cursor, |(start, _)| start);
                let line = self.doc().line(start.line);
                let unit = if self.settings.insert_spaces {
                    " ".repeat(self.settings.tab_size)
                } else {
                    "\t".to_string()
                };
                newline_text(&line[..start.byte], self.settings.auto_indent, &unit)
            } else if should_auto_close {
                format!("{}{}", ch, closing.unwrap())
            } else {
                ch.to_string()
            };
            // Checked before the edit, which clears a hover card.
            let stays_in_editor = self.keystroke_stays_in_editor();
            let before = self.caret_frame(hwnd);
            self.cancel_transition(hwnd);
            self.replace_selection(&text);
            // For auto-close pairs, move the cursor back before the closing char.
            if should_auto_close && ch != '\r' {
                let pos = self.doc().previous(self.view().cursor);
                self.view_mut().cursor = pos;
            }
            self.refresh_after_editor_input(hwnd, stays_in_editor, &before);
            self.follow_signature(hwnd, Some(ch));
        }
    }

    // Clicking the gutter toggles a breakpoint or code fold on that line instead of moving
    // the caret; debugging is keyed off document state, not editor selection.
    fn toggle_breakpoint_at(&mut self, hwnd: HWND, pane: usize, y: i32) {
        let tab_index = self.tab_for_pane(pane);
        let (line, _) = self.row_at_y(hwnd, pane, y);
        self.tabs[tab_index].document.toggle_breakpoint(line);
        self.refresh(hwnd);
    }

    fn toggle_fold_at(&mut self, hwnd: HWND, pane: usize, y: i32) {
        let tab_index = self.tab_for_pane(pane);
        let (line, _) = self.row_at_y(hwnd, pane, y);
        let doc = &mut self.tabs[tab_index].document;
        if doc.toggle_fold(line) {
            // A caret inside the block just folded moves to the fold's first
            // line; otherwise keep_cursor_visible would reveal it again and
            // the fold would reopen immediately.
            let tab = &mut self.tabs[tab_index];
            for view in &mut tab.views {
                if tab.document.is_line_hidden(view.cursor.line) {
                    let byte = view.cursor.byte;
                    view.cursor = tab.document.clamp(Pos { line, byte });
                    view.selection_anchor = None;
                }
                if view
                    .selection_anchor
                    .is_some_and(|anchor| tab.document.is_line_hidden(anchor.line))
                {
                    view.selection_anchor = None;
                }
            }
            self.refresh(hwnd);
        }
    }

    pub(super) fn position_at(&self, hwnd: HWND, x: i32, y: i32) -> Pos {
        self.position_at_pane(hwnd, x, y, self.focused_pane)
    }

    // The document line and wrapped row at height `y` in `pane`; below the
    // end of the document, its last row.
    fn row_at_y(&self, hwnd: HWND, pane: usize, y: i32) -> (usize, usize) {
        let screen_row = ((y - self.editor_top()) / self.line_height.max(1)).max(0) as usize;
        self.at_screen_row(hwnd, pane, screen_row)
            .unwrap_or_else(|| {
                let doc = &self.tabs[self.tab_for_pane(pane)].document;
                let last = doc.visible_line_for(doc.line_count() - 1);
                (last, self.line_rows(hwnd, pane, last).count() - 1)
            })
    }

    pub(super) fn position_at_pane(&self, hwnd: HWND, x: i32, y: i32, pane: usize) -> Pos {
        let tab = &self.tabs[self.tab_for_pane(pane)];
        let (line, row) = self.row_at_y(hwnd, pane, y);
        let layout = self.line_rows(hwnd, pane, line);
        let text = tab.document.line(line);
        let row_left = self.pane_left(hwnd, pane)
            + self.scale(GUTTER + PAD)
            + layout.indent(row, self.char_width);
        unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.font);
            let (row_start, row_end) = (layout.start(row), layout.end(row, text.len()));
            // Inlay hints before the click aren't text: their widths come
            // off, and a click on one is at its place.
            let mut x = (x - row_left).max(0);
            let mut on_hint = None;
            let mut shift = 0;
            for hint in self.hints_on_line(self.tab_for_pane(pane), line) {
                if hint.byte < row_start || hint.byte > row_end {
                    continue;
                }
                let left = self.text_width(hdc, &text[row_start..hint.byte]) + shift;
                if x < left {
                    break;
                }
                let width = self.text_width(hdc, &hint.label);
                if x < left + width {
                    on_hint = Some(hint.byte);
                    break;
                }
                shift += width;
            }
            x -= shift;
            let byte = on_hint.unwrap_or_else(|| {
                self.nearest_byte(hdc, text, (row_start, row_end), layout.is_last(row), x)
            });
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
                self.toggle_ai_assistant(hwnd);
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
            WelcomeAction::Settings => self.open_settings_panel(hwnd),
        }
    }

    pub(super) fn mouse_click(&mut self, hwnd: HWND, x: i32, y: i32, extend: bool) {
        if self.problem_focus {
            self.problem_focus = false;
            self.invalidate_problems(hwnd);
        }
        if self.run_choice_click(hwnd, x, y) {
            return;
        }
        if self.run_config_panel.is_some() {
            self.run_config_click(hwnd, x, y, extend, false);
            return;
        }
        if self.editor_context_click(hwnd, x, y) {
            return;
        }
        if self.more_menu_click(hwnd, x, y) {
            return;
        }
        if self.rename_click(hwnd, x, y) || self.code_action_click(hwnd, x, y) {
            return;
        }
        self.hide_signature(hwnd);
        self.clear_hover(hwnd);
        let mut rect = RECT::default();
        unsafe {
            GetClientRect(hwnd, &mut rect);
        }
        if let Some((session_id, menu)) = self.terminal_context_menu {
            self.terminal_context_menu = None;
            if x >= menu.left && x < menu.right && y >= menu.top && y < menu.bottom {
                self.rename_terminal(hwnd, session_id);
            } else {
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            return;
        }
        if self.terminal_rename_input.is_some() {
            let field = self.terminal_rename_field_rect(hwnd);
            let inside =
                field.is_some_and(|f| x >= f.left && x < f.right && y >= f.top && y < f.bottom);
            if !inside {
                self.terminal_rename_input = None;
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }
        if self.terminal_profile_menu_open {
            let left = self.editor_left();
            let top = self.terminal_top(hwnd);
            let bottom = rect.bottom - self.scale(STATUS);
            match self.terminal_profile_menu_hit(left, rect.right, top, bottom, x, y) {
                TerminalProfileMenuHit::Shell(shell) => {
                    if self.terminal_profile_shell_available(shell) {
                        self.terminal_profile_menu_open = false;
                        self.terminal_profile_availability.clear();
                        if self.terminal_profile_defaults_open {
                            self.terminal_profile_defaults_open = false;
                            self.set_default_terminal_profile(hwnd, shell);
                        } else {
                            self.new_terminal_with_shell(hwnd, shell, false);
                        }
                    }
                }
                TerminalProfileMenuHit::Settings => {
                    self.terminal_profile_defaults_open = true;
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
                TerminalProfileMenuHit::Back => {
                    self.terminal_profile_defaults_open = false;
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
                TerminalProfileMenuHit::None => {
                    self.terminal_profile_menu_open = false;
                    self.terminal_profile_defaults_open = false;
                    self.terminal_profile_availability.clear();
                    unsafe { InvalidateRect(hwnd, null(), 0) };
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
                // The hint line below the rows isn't an item.
                let row = ((y - top - self.scale(68)) / self.scale(34).max(1)) as usize;
                if row < QUICK_ROWS {
                    self.activate_quick_item(hwnd, self.quick_first + row);
                }
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
                            if IsZoomed(hwnd) != 0 {
                                SW_RESTORE
                            } else {
                                SW_MAXIMIZE
                            },
                        ),
                        _ => PostMessageW(hwnd, WM_CLOSE, 0, 0),
                    };
                }
                return;
            }
            let command = self.command_center_rect(hwnd);
            if x >= command.left && x < command.right && y >= command.top && y < command.bottom {
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
                self.ai_click(hwnd, x, y);
                return;
            }
        }
        // A click anywhere else takes the keyboard from the message box.
        self.ai.focused = false;
        if y >= rect.bottom - self.scale(STATUS) {
            self.click_status_language(hwnd, rect, x, y);
            return;
        }
        let rail = self.scale(RAIL);
        let editor_left = self.editor_left();
        // The themed debug picker behaves like a popup: its rows take the
        // click first, and clicking anywhere else dismisses it before normal
        // workbench hit-testing continues.
        if self.debug_config_menu_open {
            let panel_y = y - self.chrome_top();
            let menu = self.debug_config_menu_rect(rail, editor_left);
            if x >= menu.left && x < menu.right && panel_y >= menu.top && panel_y < menu.bottom {
                if let Some(index) = self.debug_config_menu_row(panel_y) {
                    let choice = match index {
                        0 => None,
                        1 => Some(DebugConfig::RustWorkspace),
                        _ => Some(DebugConfig::PythonFile),
                    };
                    self.select_debug_config(hwnd, choice);
                }
                return;
            }
            let selector = self.debug_config_rect(rail, editor_left);
            let on_selector = x >= selector.left
                && x < selector.right
                && panel_y >= selector.top
                && panel_y < selector.bottom;
            if !on_selector {
                self.debug_config_menu_open = false;
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }
        if x < rail {
            self.terminal_focus = false;
            // The same hit test as the hover highlight (hot.rs).
            match self.rail_item_at(y - self.chrome_top(), rect.bottom - self.chrome_top()) {
                Some(Hot::RailMenu) => self.toggle_sidebar(hwnd),
                Some(Hot::Rail(0)) => self.toggle_side_view(hwnd, SideView::Files),
                Some(Hot::Rail(1)) => self.toggle_side_view(hwnd, SideView::Search),
                Some(Hot::Rail(2)) => self.toggle_side_view(hwnd, SideView::Review),
                Some(Hot::Rail(3)) => self.toggle_side_view(hwnd, SideView::Debug),
                Some(Hot::Rail(4)) => self.toggle_side_view(hwnd, SideView::Extensions),
                Some(Hot::Rail(5)) => self.toggle_ai_assistant(hwnd),
                Some(Hot::RailSettings) => self.toggle_side_view(hwnd, SideView::Settings),
                _ => {}
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
                if self.side_view == SideView::Extensions
                    && x >= editor_left - self.scale(44)
                    && x < editor_left - self.scale(18)
                {
                    if !self.zed_registry_loading {
                        self.zed_registry_loaded = false;
                        self.status = "Refreshing extension registry...".into();
                        self.ensure_zed_registry_loaded();
                    }
                    return;
                }
                if self.side_view == SideView::Debug {
                    if x >= editor_left - self.scale(38) {
                        self.show_quick_open(hwnd);
                        self.quick_query = ">".into();
                        return;
                    }
                    if x >= editor_left - self.scale(68) {
                        self.open_settings(hwnd);
                        return;
                    }
                }
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
            if self.side_view == SideView::Settings {
                if y >= self.scale(40) {
                    self.settings_panel_click(hwnd, x, y);
                }
                return;
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
                            GitHit::Refresh => self.refresh_git(),
                            GitHit::Push => self.git_remote(hwnd, workflow::RemoteAction::Push),
                            GitHit::Pull => self.git_remote(hwnd, workflow::RemoteAction::Pull),
                            GitHit::Fetch => self.git_remote(hwnd, workflow::RemoteAction::Fetch),
                            GitHit::ToggleSection(section) => {
                                self.git_toggle_section(hwnd, section)
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
                let config = self.debug_config_rect(rail, editor_left);
                if x >= config.left && x < config.right && y >= config.top && y < config.bottom {
                    self.show_debug_config_menu(hwnd, config.left, config.bottom);
                    return;
                }
                let start = self.debug_start_button(editor_left);
                if x >= start.left && x < start.right && y >= start.top && y < start.bottom {
                    if self.debug.is_none() {
                        self.start_debug_session(hwnd);
                    }
                    return;
                }
                if y >= self.scale(94) && y < self.scale(130) {
                    for index in 0..6 {
                        let rect = self.debug_toolbar_button(rail, editor_left, index);
                        if x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom {
                            let enabled = match index {
                                0 => self.debug.is_some(),
                                1..=3 => self.debug_paused(),
                                _ => self.debug.is_some(),
                            };
                            if !enabled {
                                return;
                            }
                            match index {
                                0 if self.debug_state.running => self.debug_pause(hwnd),
                                0 => self.debug_continue(hwnd),
                                1 => self.debug_step_over(hwnd),
                                2 => self.debug_step_in(hwnd),
                                3 => self.debug_step_out(hwnd),
                                4 => self.debug_restart(hwnd),
                                _ => self.debug_stop(hwnd),
                            }
                            return;
                        }
                    }
                    return;
                }
                if x >= rail && x < editor_left {
                    let bottom = (panel_bottom - self.scale(STATUS)).max(0);
                    let layout = self.debug_panel_layout(bottom);
                    let section_headers = [
                        layout.variables_header_y,
                        layout.call_stack_header_y,
                        layout.breakpoints_header_y,
                    ];
                    for (section, header_y) in section_headers.iter().enumerate() {
                        if y >= *header_y && y < *header_y + self.scale(28) {
                            self.toggle_debug_section(hwnd, section);
                            return;
                        }
                    }
                    for row in &layout.variable_rows {
                        if row.expandable && y >= row.y && y < row.y + self.scale(23) {
                            self.toggle_debug_variable(hwnd, row.reference);
                            return;
                        }
                    }
                }
                return;
            }
            if self.side_view == SideView::Extensions {
                // The "Color Theme" row under Active Capabilities, where it
                // was last drawn (in window coordinates, as painted).
                let window_y = y + self.chrome_top();
                if let Some(row) = self.color_theme_row.get()
                    && x >= row.left
                    && x < row.right
                    && window_y >= row.top
                    && window_y < row.bottom
                {
                    self.show_color_theme_menu(hwnd, row.left, row.bottom);
                    return;
                }
                let (dpi, zoom) = (self.dpi, self.zoom);
                let s = |v: i32| scaled(v, dpi, zoom);
                let rail = s(RAIL);
                let left = rail;

                // 1. Search bar click
                if y >= s(48) && y <= s(84) {
                    let right = self.sidebar_right();
                    let search_left = left + s(8);
                    let search_right = right - s(8);
                    if x >= search_left && x <= search_right {
                        // Clear button click
                        if !self.extensions_query.is_empty()
                            && x >= search_right - s(30)
                            && x <= search_right
                        {
                            self.extensions_query.clear();
                            self.refresh(hwnd);
                            return;
                        }
                        self.extensions_search_active = true;
                        self.search_input = false;
                        self.commit_focus = false;
                        self.panel_focus = true;
                        self.terminal_focus = false;
                        self.ensure_zed_registry_loaded();
                        self.refresh(hwnd);
                        return;
                    }
                }

                // 2. Subtabs click (Segmented Pill Capsule)
                if y >= s(92) && y <= s(122) {
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

                // 3. Marketplace sorting/filter pills.
                if self.extensions_tab == ExtensionsTab::Marketplace && y >= s(132) && y <= s(160) {
                    let mut filter_x = left + s(8);
                    for (filter, _, width) in ExtensionsFilter::PILLS {
                        if x >= filter_x && x < filter_x + s(width) {
                            self.extensions_filter = filter;
                            self.refresh(hwnd);
                            return;
                        }
                        filter_x += s(width + 4);
                    }
                }

                // 4. Card action button click
                let card_h = s(116);
                let card_step = card_h + s(8);
                let start_y = s(194);
                let visible = self.filtered_extensions();
                let bottom = (panel_bottom - self.scale(STATUS)).max(0);
                if y >= start_y {
                    let row = ((y - start_y) / card_step.max(1)) as usize;
                    let ey = start_y + row as i32 * card_step;
                    // Only cards that were painted, i.e. that fit (see
                    // paint_extensions_panel).
                    if row < visible.len() && ey + card_h <= bottom {
                        let card_right = editor_left - s(8);
                        let btn_w = s(if visible[row].installing {
                            86
                        } else if visible[row].installed {
                            82
                        } else {
                            72
                        });
                        let btn_h = s(26);
                        let btn_left = card_right - btn_w - s(10);
                        let btn_right = card_right - s(10);
                        let btn_top = ey + s(80);
                        let btn_bottom = btn_top + btn_h;

                        // Generous hit box around the button
                        if y < ey + card_h
                            && x >= btn_left - s(8)
                            && x <= btn_right + s(8)
                            && y >= btn_top - s(6)
                            && y <= btn_bottom + s(8)
                        {
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
                    self.directory_requests.clear();
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
                let clicked = self
                    .explorer_rows()
                    .get(row)
                    .map(|item| (item.entry.path.clone(), item.entry.is_dir));
                if let Some((path, is_dir)) = clicked {
                    self.selected_explorer_path = Some(path.clone());
                    if is_dir {
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
            let layout = self.terminal_header_layout(editor_left, rect.right, top);
            let header_bottom = layout.header_bottom;
            if y < header_bottom {
                match self.terminal_header_hit(editor_left, rect.right, top, x, y) {
                    // The far-right close hides the panel but keeps every
                    // shell session running, exactly like dismissing a dock.
                    TerminalHeaderHit::Hide => self.close_terminal(hwnd),
                    TerminalHeaderHit::Problems => self.show_problems(hwnd),
                    TerminalHeaderHit::OutputTab => {
                        self.switch_terminal_tab(hwnd, TerminalTab::Output);
                    }
                    TerminalHeaderHit::TerminalTab(index) => self.select_terminal(hwnd, index),
                    TerminalHeaderHit::New => self.new_terminal(hwnd, false),
                    TerminalHeaderHit::ShellPicker => self.toggle_terminal_profile_menu(hwnd),
                    TerminalHeaderHit::Kill => self.close_active_terminal(hwnd),
                    TerminalHeaderHit::Body => {
                        self.focus_terminal(hwnd);
                        self.start_terminal_selection(hwnd, x, y);
                    }
                }
                return;
            }
            if self.problems_click(hwnd, x, y) {
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
                let (split, more) = self.pane_actions(self.pane_right(hwnd, pane));
                if self.tabs[self.tab_for_pane(pane)].is_placeholder() {
                    continue;
                }
                if x >= split.left && x < split.right {
                    self.focus_pane(hwnd, pane);
                    self.toggle_split(hwnd);
                    return;
                }
                if x >= more.left && x < more.right {
                    self.focus_pane(hwnd, pane);
                    self.toggle_more_menu(hwnd, pane);
                    return;
                }
            }
        }
        if self.split_divider_at(hwnd, x, y) {
            self.divider_dragging = true;
            unsafe { SetCapture(hwnd) };
            return;
        }
        if y < self.tab_strip_bottom() {
            let command = self.command_center_rect(hwnd);
            if x >= command.left && x < command.right && y >= command.top && y < command.bottom {
                self.show_quick_open(hwnd);
                return;
            }
            let button = self.file_action_rect(hwnd);
            if let Some(action) = self.shown_file_action(hwnd)
                && x >= button.left
                && x < button.right
            {
                match action {
                    FileAction::Run => self.run_active_file(hwnd),
                    FileAction::PreviewMarkdown => self.toggle_markdown_preview(hwnd),
                }
                return;
            }
            let slot = ((x - editor_left).max(0) / self.scale(TAB_WIDTH).max(1)) as usize;
            let index = self.tab_first + slot;
            // Only the tabs that fit are drawn; the space past them isn't a tab.
            if slot < self.visible_tab_count(hwnd)
                && index < self.tabs.len()
                && !self.tabs[index].is_placeholder()
            {
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
        if self.find_click(hwnd, x, y) {
            return;
        }
        if self.split_visible {
            let pane = usize::from(x >= self.pane_divider(hwnd));
            self.focus_pane(hwnd, pane);
        }
        if x >= self.editor_right(hwnd) {
            return;
        }
        let pane = self.focused_pane;
        if self.tabs[self.tab_for_pane(pane)].markdown.is_some() {
            self.click_markdown(hwnd, pane, x, y);
            return;
        }
        if self.tabs[self.tab_for_pane(pane)].is_placeholder() {
            return;
        }
        if self.scrollbar_at(hwnd, x, y) == Some(pane) {
            self.scrollbar_press(hwnd, y);
            return;
        }
        let pane_left = self.pane_left(hwnd, pane);
        if x < pane_left + self.scale(GUTTER) {
            // Match the visual order: folding is the narrow first lane;
            // breakpoint and line-number clicks use the remaining gutter.
            if x < pane_left + self.scale(GUTTER_FOLD_LANE) {
                self.toggle_fold_at(hwnd, pane, y);
            } else {
                self.toggle_breakpoint_at(hwnd, pane, y);
            }
            return;
        }
        let pos = self.position_at(hwnd, x, y);
        // Alt+Click adds a caret there (multiple cursors).
        if unsafe { GetKeyState(VK_MENU as i32) } < 0 && !self.tab().read_only() {
            self.panel_focus = false;
            self.terminal_focus = false;
            self.search_input = false;
            self.find_mode = false;
            self.toggle_caret_at(hwnd, pos);
            unsafe { SetFocus(hwnd) };
            return;
        }
        // Only the editor and the status bar change, unless this click takes
        // the keyboard from a box elsewhere, whose caret or outline goes.
        let stays_in_editor = self.keystroke_stays_in_editor()
            && !(self.panel_focus
                || self.terminal_focus
                || self.extensions_search_active
                || self.search_input
                || self.commit_focus);
        let before = self.caret_frame(hwnd);
        self.panel_focus = false;
        self.terminal_focus = false;
        self.extensions_search_active = false;
        self.search_input = false;
        // Typing goes back to the document, so the find box closes.
        self.find_mode = false;
        self.replace_mode = false;
        self.move_cursor(pos, extend);
        self.dragging = true;
        unsafe {
            SetFocus(hwnd);
            SetCapture(hwnd);
        }
        self.refresh_after_editor_input(hwnd, stays_in_editor, &before);
    }

    pub(super) fn mouse_drag(&mut self, hwnd: HWND, x: i32, y: i32) {
        if self.run_config_panel.is_some() {
            self.run_config_click(hwnd, x, y, true, true);
            return;
        }
        if self.problem_scrollbar_grab.is_some() {
            self.problems_scrollbar_drag(hwnd, y);
            return;
        }
        if self.scrollbar_grab.is_some() {
            self.scrollbar_drag(hwnd, y);
            return;
        }
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
        // Selecting by dragging redrew the whole window on every mouse move.
        let stays_in_editor = self.keystroke_stays_in_editor();
        let before = self.caret_frame(hwnd);
        self.move_cursor(pos, true);
        self.refresh_after_editor_input(hwnd, stays_in_editor, &before);
    }

    /// Right-click on the editor's text: the AI actions for the error at the
    /// click and for the selection, then Cut, Copy, Paste and Select All.
    /// False when the click wasn't on editable text.
    fn editor_context_menu(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) };
        let bottom = rect.bottom
            - self.scale(
                STATUS
                    + if self.terminal_visible {
                        self.terminal_height
                    } else {
                        0
                    },
            );
        if self.welcome
            || self.quick_open
            || x < self.editor_left()
            || x >= self.editor_right(hwnd)
            || y < self.editor_top()
            || y >= bottom
        {
            return false;
        }
        let pane = usize::from(self.split_visible && x >= self.pane_divider(hwnd));
        self.focus_pane(hwnd, pane);
        if self.tab().read_only() || self.tab().is_placeholder() {
            return false;
        }
        // A click outside the selection moves the cursor there first.
        let pos = self.position_at_pane(hwnd, x, y, pane);
        let on_selection = self
            .selection_range()
            .is_some_and(|(start, end)| start <= pos && pos <= end);
        if !on_selection {
            self.move_cursor(pos, false);
        }
        self.ai.focused = false;
        self.terminal_focus = false;
        self.panel_focus = false;
        unsafe { InvalidateRect(hwnd, null(), 0) };

        let selected = self.selection_range().is_some();
        let problem = self
            .ai_diagnostic_at_cursor()
            .map(|diagnostic| EditorContextDiagnostic::from_lsp(&diagnostic));
        let has_hunk = self.active_hunk_at_cursor().is_some();
        let can_rename = Tab::lsp_language(self.doc()).is_some();
        self.editor_context =
            Some(EditorContextMenu::new(x, y, problem, selected, has_hunk).with_rename(can_rename));
        unsafe { InvalidateRect(hwnd, null(), 0) };
        true
    }

    pub(super) fn mouse_right_click(&mut self, hwnd: HWND, x: i32, y: i32) {
        self.cancel_rename(hwnd);
        self.dismiss_code_actions(hwnd);
        if self.run_config_panel.is_some() {
            return;
        }
        self.dismiss_editor_context(hwnd);
        if self.terminal_visible {
            let mut rect = RECT::default();
            unsafe { GetClientRect(hwnd, &mut rect) };
            let left = self.editor_left();
            let top = self.terminal_top(hwnd);
            let right = self.editor_right(hwnd);
            if x >= left && x < right && y >= top && y < top + self.scale(TERMINAL_HEADER) {
                let hit = self.terminal_header_hit(left, right, top, x, y);
                match hit {
                    TerminalHeaderHit::TerminalTab(index) => {
                        if let Some(pane) = self.terminals.get(index) {
                            let bottom =
                                (rect.bottom - self.scale(STATUS)).max(0) - self.chrome_gap();
                            let menu_rect =
                                self.terminal_context_menu_rect(x, y, left, right, bottom);
                            self.terminal_context_menu = Some((pane.id, menu_rect));
                        }
                        unsafe { InvalidateRect(hwnd, null(), 0) };
                        return;
                    }
                    TerminalHeaderHit::New | TerminalHeaderHit::ShellPicker => {
                        self.toggle_terminal_profile_menu(hwnd);
                        return;
                    }
                    _ => {}
                }
            }
        }

        if self.editor_context_menu(hwnd, x, y) {
            return;
        }

        let editor_left = self.editor_left();
        let rail = self.scale(RAIL);
        if x < rail
            || x >= editor_left
            || self.side_view != SideView::Files
            || !self.explorer_visible
        {
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
            let row_idx =
                self.explorer_first_row + ((panel_y - row_top) / self.scale(EXPLORER_ROW)) as usize;
            let clicked = self
                .explorer_rows()
                .get(row_idx)
                .map(|row| (row.entry.path.clone(), row.entry.is_dir));
            if let Some((path, dir)) = clicked {
                target_path = Some(path.clone());
                is_dir = dir;
                self.selected_explorer_path = Some(path);
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
            AppendMenuW, CreatePopupMenu, DestroyMenu, MF_SEPARATOR, MF_STRING, TPM_LEFTALIGN,
            TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
        };

        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }

            const CMD_NEW_FILE: usize = 1;
            const CMD_NEW_FOLDER: usize = 2;
            const CMD_ADD_FILE: usize = 9;
            const CMD_REVEAL: usize = 3;
            const CMD_COPY_PATH: usize = 4;
            const CMD_COPY_REL_PATH: usize = 5;
            const CMD_RENAME: usize = 6;
            const CMD_DELETE: usize = 7;
            const CMD_CLOSE_WORKSPACE: usize = 8;

            AppendMenuW(menu, MF_STRING, CMD_NEW_FILE, wide("New File...").as_ptr());
            AppendMenuW(
                menu,
                MF_STRING,
                CMD_NEW_FOLDER,
                wide("New Folder...").as_ptr(),
            );
            AppendMenuW(menu, MF_STRING, CMD_ADD_FILE, wide("Add File...").as_ptr());
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(
                menu,
                MF_STRING,
                CMD_REVEAL,
                wide("Reveal in File Explorer").as_ptr(),
            );
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
                clicked_path.parent().unwrap_or(&root).to_path_buf()
            };

            match cmd {
                CMD_NEW_FILE => {
                    self.start_explorer_input(parent_dir, false, false, None, hwnd);
                }
                CMD_NEW_FOLDER => {
                    self.start_explorer_input(parent_dir, true, false, None, hwnd);
                }
                CMD_ADD_FILE => {
                    self.add_file_to_project(hwnd, &parent_dir);
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
                    self.start_explorer_input(parent_dir, is_dir, true, Some(clicked_path), hwnd);
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

// What Enter inserts, given the text left before the caret on its line: a
// newline with that text's indentation, one `unit` deeper after a `{` or `:`
// when auto-indent is on. Only the text before the caret counts: Enter at
// the start of `def f():` moves the line down as it is.
pub(super) fn newline_text(before: &str, auto_indent: bool, unit: &str) -> String {
    let indent: String = before
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let opens_block = before.trim_end().ends_with(['{', ':']);
    if auto_indent && opens_block {
        format!("\n{indent}{unit}")
    } else {
        format!("\n{indent}")
    }
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;

    #[test]
    fn enter_indents_from_the_text_before_the_caret() {
        // After a block opener: one level deeper.
        assert_eq!(newline_text("    def f():", true, "    "), "\n        ");
        assert_eq!(newline_text("fn main() {", true, "    "), "\n    ");
        // At the start of such a line: the line just moves down.
        assert_eq!(newline_text("", true, "    "), "\n");
        // Inside its indentation: only the indentation before the caret.
        assert_eq!(newline_text("  ", true, "    "), "\n  ");
        // Mid-line, or with auto-indent off: the same indentation.
        assert_eq!(newline_text("    let x = {a", true, "    "), "\n    ");
        assert_eq!(newline_text("\tif x:", false, "\t"), "\n\t");
    }

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
