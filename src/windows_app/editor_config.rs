// The project's .editorconfig (lightline::editorconfig) in the editor:
// indentation (indent_style), what Tab and the guides use; at save, trailing
// whitespace, a final newline and line endings; and the status bar's
// "Spaces: 2 · .editorconfig", which lists the rules when clicked.
//
// Rules are read the first time a tab needs them (its status bar or a Tab
// key), kept on the tab, and dropped when the watcher reports that an
// .editorconfig changed or a folder gained one. Nothing is read while typing.

use super::*;
use lightline::editorconfig::{self, EndOfLine, IndentStyle, Rules};
use std::rc::Rc;

impl App {
    /// The .editorconfig rules for tab `index`'s file; None when the
    /// `editorConfig` setting is off, the tab has no file, or no section
    /// matches it.
    pub(super) fn editorconfig_for(&self, index: usize) -> Option<Rc<Rules>> {
        if !self.settings.editor_config {
            return None;
        }
        let tab = self.tabs.get(index)?;
        if tab.is_placeholder() || tab.markdown.is_some() || tab.image.is_some() {
            return None;
        }
        let path = tab.document.path.as_deref()?;
        let mut cache = tab.editorconfig.borrow_mut();
        if cache.as_ref().is_none_or(|(known, _)| known != path) {
            let rules = editorconfig::rules_for(path);
            // A change to any file read (even one that matched nothing)
            // can change the rules.
            if let Some(watcher) = &self.watcher {
                for file in &rules.files {
                    watcher.watch_file(file.clone());
                }
            }
            *cache = Some((path.to_path_buf(), Rc::new(rules)));
        }
        cache
            .as_ref()
            .map(|(_, rules)| rules.clone())
            .filter(|rules| !rules.is_empty())
    }

    /// Indentation the active file's .editorconfig sets, as `indent_style`
    /// returns it: (spaces, size), each None when not set.
    pub(super) fn editorconfig_indent(&self, index: usize) -> (Option<bool>, Option<usize>) {
        self.editorconfig_for(index).map_or((None, None), |rules| {
            (
                rules.indent_style.map(|style| style == IndentStyle::Space),
                rules.indent_size,
            )
        })
    }

    /// Columns a tab character takes in tab `index`: its .editorconfig's
    /// `tab_width` (which defaults to `indent_size`), else the tabSize
    /// setting.
    pub(super) fn tab_width_for(&self, index: usize) -> usize {
        self.editorconfig_for(index)
            .and_then(|rules| rules.tab_width)
            .unwrap_or(self.settings.tab_size)
            .max(1)
    }

    /// `tab_width_for` the tab holding `doc`.
    pub(super) fn tab_width_of(&self, doc: &Document) -> usize {
        if !self.settings.editor_config {
            return self.settings.tab_size.max(1);
        }
        self.tabs
            .iter()
            .position(|tab| tab.document.id() == doc.id())
            .map_or(self.settings.tab_size.max(1), |index| {
                self.tab_width_for(index)
            })
    }

    /// An .editorconfig changed, or a folder gained or lost one: every tab
    /// reads its rules again when next asked.
    pub(super) fn forget_editorconfig(&mut self) {
        for tab in &self.tabs {
            *tab.editorconfig.borrow_mut() = None;
        }
    }

    /// Before the active tab is written to `path`: trims trailing spaces
    /// and tabs, adds a final newline and sets the line ending, as its
    /// .editorconfig asks. The edits are one undo step, and the caret and
    /// selection stay where they were.
    pub(super) fn apply_editorconfig_on_save(&mut self, path: &Path) {
        if !self.settings.editor_config {
            return;
        }
        let rules = editorconfig::rules_for(path);
        if rules.is_empty() {
            return;
        }
        let (cursor, anchor) = (self.view().cursor, self.view().selection_anchor);
        // Trimming only shortens lines: a position past a line's new end
        // moves to it.
        let mut trimmed: Vec<(usize, usize)> = Vec::new();
        let trim = rules.trim_trailing_whitespace == Some(true);
        let last = self.doc().line_count() - 1;
        let add_newline =
            rules.insert_final_newline == Some(true) && !self.doc().line(last).is_empty();
        if trim || add_newline {
            self.doc_mut().begin_group();
            if trim {
                for line in 0..self.doc().line_count() {
                    let text = self.doc().line(line);
                    let kept = text.trim_end_matches([' ', '\t']).len();
                    if kept < text.len() {
                        let end = Pos {
                            line,
                            byte: text.len(),
                        };
                        self.replace_range(Pos { line, byte: kept }, end, "");
                        trimmed.push((line, kept));
                    }
                }
            }
            if add_newline {
                let end = Pos {
                    line: last,
                    byte: self.doc().line(last).len(),
                };
                self.replace_range(end, end, "\n");
            }
            self.doc_mut().end_group();
            let keep = |pos: Pos| match trimmed.iter().find(|(line, _)| *line == pos.line) {
                Some(&(_, kept)) if pos.byte > kept => Pos {
                    line: pos.line,
                    byte: kept,
                },
                _ => pos,
            };
            let cursor = self.doc().clamp(keep(cursor));
            let anchor = anchor.map(|anchor| self.doc().clamp(keep(anchor)));
            self.view_mut().cursor = cursor;
            self.view_mut().selection_anchor = anchor;
        }
        if let Some(ending) = rules.end_of_line {
            self.doc_mut().set_line_ending(ending == EndOfLine::Crlf);
        }
    }

    /// The status bar's ".editorconfig" was clicked: a menu of the rules
    /// that apply, and a way to open the file they come from.
    pub(super) fn show_editorconfig_rules(&mut self, hwnd: HWND, x: i32, y: i32) {
        use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, MF_GRAYED, MF_SEPARATOR, MF_STRING,
            TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, TrackPopupMenu,
        };
        let Some(rules) = self.editorconfig_for(self.active) else {
            return;
        };
        let Some((file, headers)) = rules.source.clone() else {
            return;
        };
        let chosen = unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }
            let title = format!("From .editorconfig  {}", headers.join(", "));
            AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, wide(&title).as_ptr());
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            for (key, value) in &rules.shown {
                let line = format!("{key} = {value}");
                AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, wide(&line).as_ptr());
            }
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(menu, MF_STRING, 1, wide("Open .editorconfig").as_ptr());
            let mut point = POINT { x, y };
            ClientToScreen(hwnd, &mut point);
            let chosen = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_LEFTALIGN | TPM_BOTTOMALIGN,
                point.x,
                point.y,
                0,
                hwnd,
                null(),
            );
            DestroyMenu(menu);
            chosen
        };
        if chosen == 1 {
            self.open(hwnd, Some(file));
        }
    }
}
