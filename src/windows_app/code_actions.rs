// Quick Fixes and refactorings (Ctrl+.): what the language server offers at
// the caret or selection, in a list under the caret. Asked for only on
// Ctrl+., never while typing or moving the caret.

use super::workspace_edit::count;
use super::*;
use lightline::lsp::{
    CodeAction, Command as LspCommand, FileEdit, Position as LspPosition, Range as LspRange,
};

// Rows shown at once; the list scrolls past them.
const ROWS: usize = 9;
const ROW: i32 = 28;
const PADDING: i32 = 6;
const WIDTH: i32 = 460;

// What's asked for.
#[derive(Clone, Copy, PartialEq)]
enum Kinds {
    // Ctrl+.: fixes and refactorings.
    Fixes,
    // Source Action...: whole-file actions, such as organize imports.
    Source,
    // Shift+Alt+O: organize imports, made at once when it's the only one.
    OrganizeImports,
}

pub(super) struct CodeActionRequest {
    language: LspLanguage,
    kinds: Kinds,
    id: u64,
    uri: String,
    version: i32,
    x: i32,
    y: i32,
}

pub(super) struct CodeActionMenu {
    language: LspLanguage,
    // The document's `id`; the menu goes when its tab shows another file.
    document: u64,
    actions: Vec<CodeAction>,
    selected: usize,
    first: usize,
    x: i32,
    y: i32,
}

impl App {
    pub(super) fn request_code_actions(&mut self, hwnd: HWND) {
        self.ask_code_actions(hwnd, Kinds::Fixes);
    }

    pub(super) fn request_source_actions(&mut self, hwnd: HWND) {
        self.ask_code_actions(hwnd, Kinds::Source);
    }

    pub(super) fn organize_imports(&mut self, hwnd: HWND) {
        self.ask_code_actions(hwnd, Kinds::OrganizeImports);
    }

    fn ask_code_actions(&mut self, hwnd: HWND, kinds: Kinds) {
        self.dismiss_code_actions(hwnd);
        if self.welcome || self.tab().read_only() || self.tab().is_placeholder() {
            return;
        }
        if Tab::lsp_language(self.doc()).is_none() {
            self.status = "Code actions need a language server, and this file type has none".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        if !self.tab().lsp_opened {
            self.ensure_lsp(hwnd);
        }
        let tab = self.tab();
        let (Some(language), Some(path), true) = (
            tab.lsp_language,
            tab.document.path.as_deref(),
            tab.lsp_opened,
        ) else {
            self.status = "Language server not ready yet".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        let doc = &tab.document;
        let at = |pos: Pos| LspPosition {
            line: pos.line as u32,
            character: doc.utf16_column(pos) as u32,
        };
        let (start, end) = self
            .selection_range()
            .unwrap_or((self.view().cursor, self.view().cursor));
        let mut range = LspRange {
            start: at(start),
            end: at(end),
        };
        // A caret elsewhere on a line with a problem asks about the
        // problem: servers offer a fix only where it is.
        if kinds == Kinds::Fixes
            && start == end
            && !tab
                .diagnostics
                .iter()
                .any(|problem| contains(problem.range, range.start))
            && let Some(problem) = tab
                .diagnostics
                .iter()
                .find(|problem| problem.range.start.line == range.start.line)
        {
            range = problem.range;
        }
        let diagnostics: Vec<LspDiagnostic> = tab
            .diagnostics
            .iter()
            .filter(|problem| {
                problem.range.start.line <= range.end.line
                    && problem.range.end.line >= range.start.line
            })
            .cloned()
            .collect();
        let uri = lsp::file_uri(path);
        let version = tab.lsp_version;
        let only = match kinds {
            Kinds::Fixes => Vec::new(),
            Kinds::Source => vec!["source".to_owned()],
            Kinds::OrganizeImports => vec!["source.organizeImports".to_owned()],
        };
        self.request_id += 1;
        let id = self.request_id;
        let sent = self.tab_lsp(self.active).is_some_and(|client| {
            client.send(LspCommand::CodeActions {
                id,
                uri: uri.clone(),
                version,
                range,
                diagnostics,
                only,
            })
        });
        if sent {
            let caret = self.caret_rect(hwnd);
            self.code_action_request = Some(CodeActionRequest {
                language,
                kinds,
                id,
                uri,
                version,
                x: caret.left,
                y: caret.bottom,
            });
            self.status = match kinds {
                Kinds::Fixes => "Looking for fixes...",
                Kinds::Source => "Looking for source actions...",
                Kinds::OrganizeImports => "Organizing imports...",
            }
            .into();
        } else {
            self.status = "Language server not ready yet".into();
        }
        unsafe { InvalidateRect(hwnd, &self.status_area(hwnd), 0) };
    }

    /// The language server's answer to `request_code_actions`.
    pub(super) fn finish_code_actions(
        &mut self,
        language: LspLanguage,
        id: u64,
        uri: &str,
        version: i32,
        result: Result<Vec<CodeAction>, String>,
    ) {
        let Some(request) = self.code_action_request.take_if(|request| {
            request.language == language
                && request.id == id
                && request.uri == uri
                && request.version == version
        }) else {
            return;
        };
        // Offered for the text as it was; not after it changed, or for a
        // file no longer shown.
        if self.open_tab_for_uri(uri) != Some(self.active) || self.tab().lsp_version != version {
            self.status.clear();
            return;
        }
        let none = match request.kinds {
            Kinds::Fixes => "No quick fixes here",
            Kinds::Source => "No source actions for this file",
            Kinds::OrganizeImports => "The language server can't organize imports here",
        };
        let mut actions = match result {
            Ok(actions) if actions.is_empty() => {
                self.status = none.into();
                return;
            }
            Ok(actions) => actions,
            Err(reason) => {
                self.status = format!("{none}: {reason}");
                return;
            }
        };
        if request.kinds == Kinds::OrganizeImports && actions.len() == 1 {
            let action = actions.remove(0);
            self.run_code_action(language, action);
            return;
        }
        // The server's preferred fix first, then fixes, then refactorings.
        actions.sort_by_key(|action| (!action.preferred, !action.kind.starts_with("quickfix")));
        self.status = format!(
            "{} available: Enter applies, Esc closes",
            count(actions.len(), "action")
        );
        self.code_actions = Some(CodeActionMenu {
            language,
            document: self.doc().id(),
            actions,
            selected: 0,
            first: 0,
            x: request.x,
            y: request.y,
        });
    }

    /// Edits the server asked for while running a command.
    pub(super) fn apply_server_edit(
        &mut self,
        language: LspLanguage,
        result: Result<Vec<FileEdit>, String>,
    ) {
        let files = match result {
            Ok(files) => files,
            Err(reason) => {
                self.status = format!("Can't make the server's change: {reason}");
                return;
            }
        };
        match self.apply_workspace_edit(files, language, false) {
            Ok(applied) if !applied.failed.is_empty() => {
                self.status = format!("Couldn't save {}", applied.failed.join(", "));
            }
            Ok(_) => {}
            Err(reason) => self.status = format!("Can't make the server's change: {reason}"),
        }
    }

    pub(super) fn dismiss_code_actions(&mut self, hwnd: HWND) {
        self.code_action_request = None;
        if self.code_actions.take().is_some() {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    // The list, under the caret or above it at the bottom of the editor.
    fn code_action_layout(&self, hwnd: HWND) -> Option<RECT> {
        let menu = self.code_actions.as_ref()?;
        if self.welcome || self.doc().id() != menu.document {
            return None;
        }
        let s = |value: i32| self.scale(value);
        let area = self.editor_area(hwnd, false);
        let rows = menu.actions.len().min(ROWS) as i32;
        let height = rows * s(ROW) + s(PADDING) * 2;
        let width = s(WIDTH).min(area.right - area.left - s(16));
        let left = menu.x.min(area.right - s(8) - width).max(area.left);
        let below = menu.y + s(2);
        let top = if below + height <= area.bottom {
            below
        } else {
            (menu.y - self.line_height - height - s(2)).max(area.top)
        };
        Some(RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        })
    }

    pub(super) fn paint_code_actions(&self, hdc: HDC, hwnd: HWND) {
        let (Some(menu), Some(outer)) = (&self.code_actions, self.code_action_layout(hwnd)) else {
            return;
        };
        let s = |value: i32| self.scale(value);
        self.panel_card(hdc, outer, s(8), self.theme.edge, self.theme.sidebar_bg);
        unsafe { SelectObject(hdc, self.ui_font) };
        let end = (menu.first + ROWS).min(menu.actions.len());
        for (row, index) in (menu.first..end).enumerate() {
            let action = &menu.actions[index];
            let top = outer.top + s(PADDING) + row as i32 * s(ROW);
            let line = RECT {
                left: outer.left + s(4),
                top,
                right: outer.right - s(4),
                bottom: top + s(ROW),
            };
            if index == menu.selected {
                Self::rounded_fill(hdc, line, s(6), self.theme.select_bg);
            }
            let middle = top + s(ROW) / 2;
            let tag = kind_tag(&action.kind);
            let tag_width = if tag.is_empty() { 0 } else { s(80) };
            let text_left = line.left + s(10);
            let text = RECT {
                left: text_left,
                top,
                right: line.right - s(10) - tag_width,
                bottom: top + s(ROW),
            };
            let text_top = middle - self.text_height(hdc) / 2;
            self.label_ellipsis(
                hdc,
                &action.title,
                text_left,
                text_top,
                self.theme.text,
                text,
            );
            if !tag.is_empty() {
                let tag_left = line.right - s(10) - self.text_width(hdc, tag);
                self.label_mid(hdc, tag, tag_left, middle, self.theme.muted, line);
            }
        }
    }

    /// Keys while the list is open: true when it took the key. Others
    /// close it and go on to the editor.
    pub(super) fn code_action_key(&mut self, hwnd: HWND, key: u32) -> bool {
        let Some(menu) = &mut self.code_actions else {
            return false;
        };
        let last = menu.actions.len() - 1;
        let selected = match key {
            x if x == VK_UP as u32 => menu.selected.checked_sub(1).unwrap_or(last),
            x if x == VK_DOWN as u32 => (menu.selected + 1) % (last + 1),
            x if x == VK_PRIOR as u32 => menu.selected.saturating_sub(ROWS),
            x if x == VK_NEXT as u32 => (menu.selected + ROWS).min(last),
            x if x == VK_HOME as u32 => 0,
            x if x == VK_END as u32 => last,
            x if x == VK_RETURN as u32 || x == VK_TAB as u32 => {
                let index = menu.selected;
                self.apply_code_action(hwnd, index);
                return true;
            }
            x if x == VK_ESCAPE as u32 => {
                self.dismiss_code_actions(hwnd);
                self.status.clear();
                return true;
            }
            _ => {
                self.dismiss_code_actions(hwnd);
                return false;
            }
        };
        menu.selected = selected;
        if selected < menu.first {
            menu.first = selected;
        } else if selected >= menu.first + ROWS {
            menu.first = selected + 1 - ROWS;
        }
        if let Some(area) = self.code_action_layout(hwnd) {
            unsafe { InvalidateRect(hwnd, &area, 0) };
        }
        true
    }

    /// A click on a row applies it (true); one elsewhere closes the list
    /// and goes on to what was clicked.
    pub(super) fn code_action_click(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some(outer) = self.code_action_layout(hwnd) else {
            self.code_actions = None;
            return false;
        };
        if x < outer.left || x >= outer.right || y < outer.top || y >= outer.bottom {
            self.dismiss_code_actions(hwnd);
            return false;
        }
        let row = ((y - outer.top - self.scale(PADDING)).max(0) / self.scale(ROW).max(1)) as usize;
        if let Some(menu) = &self.code_actions {
            let index = menu.first + row;
            if index < menu.actions.len() {
                self.apply_code_action(hwnd, index);
            }
        }
        true
    }

    fn apply_code_action(&mut self, hwnd: HWND, index: usize) {
        let Some(menu) = self.code_actions.take() else {
            return;
        };
        unsafe { InvalidateRect(hwnd, null(), 0) };
        if let Some(action) = menu.actions.into_iter().nth(index) {
            self.run_code_action(menu.language, action);
        }
    }

    // Makes the action's edit, then has the server run its command.
    fn run_code_action(&mut self, language: LspLanguage, action: CodeAction) {
        let mut status = format!("Applied: {}", action.title);
        if !action.edit.is_empty() {
            match self.apply_workspace_edit(action.edit, language, false) {
                Ok(applied) => {
                    if applied.files > 1 {
                        status = format!("{status} ({} files)", applied.files);
                    }
                    if !applied.failed.is_empty() {
                        status = format!("{status}; couldn't save {}", applied.failed.join(", "));
                    }
                }
                Err(reason) => {
                    self.status = format!("Can't apply {}: {reason}", action.title);
                    return;
                }
            }
        }
        if let Some((command, arguments)) = action.command
            && let Some(client) = self.tab_lsp(self.active)
        {
            client.send(LspCommand::ExecuteCommand { command, arguments });
        }
        self.status = status;
    }
}

fn contains(range: LspRange, at: LspPosition) -> bool {
    let key = |p: LspPosition| (p.line, p.character);
    key(range.start) <= key(at) && key(at) <= key(range.end)
}

// A word for what kind of action it is, shown at the row's right.
fn kind_tag(kind: &str) -> &'static str {
    match kind.split('.').next().unwrap_or("") {
        "quickfix" => "fix",
        "refactor" => "refactor",
        "source" => "source",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_show_as_short_tags() {
        assert_eq!(kind_tag("quickfix"), "fix");
        assert_eq!(kind_tag("refactor.extract"), "refactor");
        assert_eq!(kind_tag("source.organizeImports"), "source");
        assert_eq!(kind_tag(""), "");
    }

    #[test]
    fn a_problem_contains_its_ends() {
        let at = |line, character| LspPosition { line, character };
        let range = LspRange {
            start: at(3, 4),
            end: at(3, 9),
        };
        assert!(contains(range, at(3, 4)));
        assert!(contains(range, at(3, 9)));
        assert!(!contains(range, at(3, 10)));
        assert!(!contains(range, at(2, 5)));
    }
}
