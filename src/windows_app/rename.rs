// Rename Symbol (F2 in the editor). A box under the name takes the new one;
// the language server then says what to change in which files, and
// `apply_workspace_edit` makes the changes, saving every file it changes
// that had no unsaved work.

use super::input::decode_utf16_input;
use super::render::safe_slice_range;
use super::workspace_edit::count;
use super::*;
use lightline::lsp::{Command as LspCommand, FileEdit, Position as LspPosition};

const WIDTH: i32 = 280;
const ROW: i32 = 30;
const INSET: i32 = 6;
// The longest name the box takes.
const MAX_NAME: usize = 200;

pub(super) struct RenameBox {
    pane: usize,
    // The document's `id`: the box goes when its tab shows another file.
    document: u64,
    // Where the name starts, and the name.
    start: Pos,
    old_name: String,
    text: String,
    // Shown selected: the first key typed replaces it.
    selected: bool,
}

pub(super) struct RenameRequest {
    language: LspLanguage,
    id: u64,
    uri: String,
    version: i32,
    old_name: String,
    new_name: String,
}

impl App {
    pub(super) fn start_rename(&mut self, hwnd: HWND) {
        if self.welcome || self.tab().read_only() || self.tab().is_placeholder() {
            return;
        }
        if Tab::lsp_language(self.doc()).is_none() {
            self.status = "Rename needs a language server, and this file type has none".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let cursor = self.doc().clamp(self.view().cursor);
        let Some((from, to)) = name_at(self.doc().line(cursor.line), cursor.byte) else {
            self.status = "Put the caret on a name to rename it".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        if !self.tab().lsp_opened {
            self.ensure_lsp(hwnd);
        }
        if !self.tab().lsp_opened {
            self.status = "Language server not ready yet".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        self.dismiss_completion(hwnd);
        self.clear_hover(hwnd);
        self.find_mode = false;
        self.replace_mode = false;
        let name = self.doc().line(cursor.line)[from..to].to_owned();
        self.rename_box = Some(RenameBox {
            pane: self.focused_pane,
            document: self.doc().id(),
            start: Pos {
                line: cursor.line,
                byte: from,
            },
            old_name: name.clone(),
            text: name,
            selected: true,
        });
        self.status = "Type the new name: Enter renames, Esc cancels".into();
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// The box and its input field, under the name or above it at the
    /// bottom of the editor. None while the name is scrolled out of view.
    fn rename_layout(&self, hwnd: HWND) -> Option<(RECT, RECT)> {
        let rename = self.rename_box.as_ref()?;
        let tab = &self.tabs[self.tab_for_pane(rename.pane)];
        if self.welcome || tab.document.id() != rename.document {
            return None;
        }
        let (row, row_start, indent) = self.locate(
            hwnd,
            rename.pane,
            rename.start,
            self.visible_lines(hwnd) + 1,
        )?;
        let s = |value: i32| self.scale(value);
        let prefix = safe_slice_range(
            tab.document.line(rename.start.line),
            row_start,
            rename.start.byte,
        );
        let prefix_width = unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.font);
            let width = self.text_width(hdc, prefix);
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            width
        };
        let name_left = self.pane_left(hwnd, rename.pane) + s(GUTTER + PAD) + indent + prefix_width;
        let min_left = self.pane_left(hwnd, rename.pane) + s(GUTTER);
        let max_right = self.pane_right(hwnd, rename.pane) - s(20);
        let width = s(WIDTH).min(max_right - min_left);
        let height = s(INSET) * 2 + s(ROW);
        // The typed text lines up with the name it replaces.
        let left = (name_left - s(INSET + 9))
            .min(max_right - width)
            .max(min_left);
        let row_top = self.editor_top() + row as i32 * self.line_height;
        let below = row_top + self.line_height + s(4);
        let top = if below + height <= self.editor_area(hwnd, false).bottom {
            below
        } else {
            row_top - s(4) - height
        };
        let card = RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let field = RECT {
            left: card.left + s(INSET),
            top: card.top + s(INSET),
            right: card.right - s(INSET),
            bottom: card.bottom - s(INSET),
        };
        Some((card, field))
    }

    // What painting the box covers, its shadow included.
    fn rename_area(&self, hwnd: HWND) -> Option<RECT> {
        let (card, _) = self.rename_layout(hwnd)?;
        let margin = self.scale(6);
        Some(RECT {
            left: card.left - margin,
            top: card.top - margin,
            right: card.right + margin,
            bottom: card.bottom + margin,
        })
    }

    pub(super) fn paint_rename_box(&self, hdc: HDC, hwnd: HWND) {
        let (Some(rename), Some((card, field))) = (&self.rename_box, self.rename_layout(hwnd))
        else {
            return;
        };
        let s = |value: i32| self.scale(value);
        let shadow = RECT {
            left: card.left + s(4),
            top: card.top + s(5),
            right: card.right + s(4),
            bottom: card.bottom + s(5),
        };
        Self::rounded_fill(hdc, shadow, s(9), ui(3, 8, 18));
        self.panel_card(hdc, card, s(8), self.theme.edge, self.theme.sidebar_bg);
        unsafe { SelectObject(hdc, self.ui_font) };
        self.paint_find_field(hdc, field, &rename.text, "New name", true, rename.selected);
    }

    /// Closes the box without renaming.
    pub(super) fn cancel_rename(&mut self, hwnd: HWND) {
        if self.rename_box.is_none() {
            return;
        }
        if let Some(area) = self.rename_area(hwnd) {
            unsafe { InvalidateRect(hwnd, &area, 0) };
        }
        self.rename_box = None;
        self.status.clear();
        let status = self.status_area(hwnd);
        unsafe { InvalidateRect(hwnd, &status, 0) };
    }

    // Changes the box's text with `edit`, and redraws the box.
    fn edit_rename(&mut self, hwnd: HWND, edit: impl FnOnce(&mut RenameBox)) {
        if let Some(rename) = &mut self.rename_box {
            edit(rename);
        }
        if let Some(area) = self.rename_area(hwnd) {
            unsafe { InvalidateRect(hwnd, &area, 0) };
        }
    }

    /// Keys for the open box, which takes them all. False when no box is
    /// open, or its name went out of view: then it closes and the key goes
    /// on to the editor.
    pub(super) fn rename_key(&mut self, hwnd: HWND, key: u32, ctrl: bool) -> bool {
        if self.rename_box.is_none() {
            return false;
        }
        if self.rename_layout(hwnd).is_none() {
            self.cancel_rename(hwnd);
            return false;
        }
        match key {
            x if x == VK_ESCAPE as u32 => self.cancel_rename(hwnd),
            x if x == VK_RETURN as u32 => self.commit_rename(hwnd),
            x if x == VK_BACK as u32 => self.edit_rename(hwnd, |rename| {
                if std::mem::take(&mut rename.selected) || ctrl {
                    rename.text.clear();
                } else {
                    rename.text.pop();
                }
            }),
            0x41 if ctrl => self.edit_rename(hwnd, |rename| rename.selected = true),
            0x56 if ctrl => match clipboard::paste(hwnd) {
                Ok(Some(text)) => {
                    let pasted = text.lines().next().unwrap_or("").trim().to_owned();
                    self.edit_rename(hwnd, |rename| type_into(rename, &pasted));
                }
                Ok(None) => {}
                Err(error) => self.error(hwnd, &error),
            },
            // The caret stays at the end of the text; these keep the text.
            x if x == VK_LEFT as u32
                || x == VK_RIGHT as u32
                || x == VK_HOME as u32
                || x == VK_END as u32 =>
            {
                self.edit_rename(hwnd, |rename| rename.selected = false)
            }
            _ => {}
        }
        true
    }

    /// Typed characters for the open box; false when none is open.
    pub(super) fn rename_char(&mut self, hwnd: HWND, unit: u16) -> bool {
        if self.rename_box.is_none() {
            return false;
        }
        if let Some(ch) = decode_utf16_input(&mut self.pending_high_surrogate, unit)
            && !ch.is_control()
        {
            self.edit_rename(hwnd, |rename| {
                type_into(rename, ch.encode_utf8(&mut [0; 4]));
            });
        }
        true
    }

    /// A click on the box stays in it (true); one elsewhere closes it and
    /// goes on to what was clicked.
    pub(super) fn rename_click(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        if self.rename_box.is_none() {
            return false;
        }
        if let Some((card, _)) = self.rename_layout(hwnd)
            && x >= card.left
            && x < card.right
            && y >= card.top
            && y < card.bottom
        {
            return true;
        }
        self.cancel_rename(hwnd);
        false
    }

    // Enter: asks the language server for the edits.
    fn commit_rename(&mut self, hwnd: HWND) {
        if let Some(area) = self.rename_area(hwnd) {
            unsafe { InvalidateRect(hwnd, &area, 0) };
        }
        let Some(rename) = self.rename_box.take() else {
            return;
        };
        let new_name = rename.text.trim().to_owned();
        let index = self.tab_for_pane(rename.pane);
        let tab = &self.tabs[index];
        self.status = if new_name.is_empty() || new_name == rename.old_name {
            String::new()
        } else if let (true, Some(language), Some(path)) = (
            tab.lsp_opened,
            tab.lsp_language,
            tab.document.path.as_deref(),
        ) {
            let uri = lsp::file_uri(path);
            let version = tab.lsp_version;
            let position = LspPosition {
                line: rename.start.line as u32,
                character: tab.document.utf16_column(rename.start) as u32,
            };
            self.request_id += 1;
            let id = self.request_id;
            let sent = self.tab_lsp(index).is_some_and(|client| {
                client.send(LspCommand::Rename {
                    id,
                    uri: uri.clone(),
                    version,
                    position,
                    new_name: new_name.clone(),
                })
            });
            if sent {
                let status = format!("Renaming {} to {new_name}...", rename.old_name);
                self.rename_target = Some(RenameRequest {
                    language,
                    id,
                    uri,
                    version,
                    old_name: rename.old_name,
                    new_name,
                });
                status
            } else {
                "Language server not ready yet".into()
            }
        } else {
            "Language server not ready yet".into()
        };
        let status = self.status_area(hwnd);
        unsafe { InvalidateRect(hwnd, &status, 0) };
    }

    /// The language server's answer to the request `commit_rename` sent.
    pub(super) fn finish_rename(
        &mut self,
        language: LspLanguage,
        id: u64,
        uri: &str,
        version: i32,
        result: Result<Vec<FileEdit>, String>,
    ) {
        let Some(request) = self.rename_target.take_if(|request| {
            request.language == language
                && request.id == id
                && request.uri == uri
                && request.version == version
        }) else {
            return;
        };
        let Some(asked) = self.open_tab_for_uri(uri) else {
            return;
        };
        if self.tabs[asked].lsp_version != version {
            self.status = "The file changed while renaming; press F2 to try again".into();
            return;
        }
        let old = &request.old_name;
        let files = match result {
            Ok(files) if files.is_empty() => {
                self.status = format!("Nothing to rename at {old}");
                return;
            }
            Ok(files) => files,
            Err(reason) => {
                self.status = format!("Can't rename: {reason}");
                return;
            }
        };
        match self.apply_workspace_edit(files, language, true) {
            Ok(applied) => {
                // Short, so it fits the status bar of a small window.
                let files = if applied.files > 1 {
                    format!(" in {} files", applied.files)
                } else {
                    String::new()
                };
                self.status = format!(
                    "Renamed to {}{files} ({})",
                    request.new_name,
                    count(applied.places, "place")
                );
                if !applied.failed.is_empty() {
                    self.status = format!(
                        "{}; couldn't save {}",
                        self.status,
                        applied.failed.join(", ")
                    );
                }
            }
            Err(reason) => self.status = format!("Can't rename {old}: {reason}"),
        }
    }
}

// Types `text` into the box, over its text when that is selected.
fn type_into(rename: &mut RenameBox, text: &str) {
    if std::mem::take(&mut rename.selected) {
        rename.text.clear();
    }
    for ch in text.chars() {
        if rename.text.chars().count() >= MAX_NAME {
            break;
        }
        rename.text.push(ch);
    }
}

// The name around byte `at` of `line` (touching it from either side): the
// range of its bytes. None when there is none, or it's a number.
fn name_at(line: &str, at: usize) -> Option<(usize, usize)> {
    let is_name = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let at = at.min(line.len());
    let start = line[..at]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_name(*c))
        .last()
        .map_or(at, |(index, _)| index);
    let end = line[at..]
        .char_indices()
        .find(|(_, c)| !is_name(*c))
        .map_or(line.len(), |(index, _)| at + index);
    let name = &line[start..end];
    (!name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit())).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_under_or_beside_the_caret() {
        let line = "let total = sum(values);";
        let name = |at| name_at(line, at).map(|(start, end)| &line[start..end]);
        assert_eq!(name(4), Some("total"));
        assert_eq!(name(6), Some("total"));
        // Right after the name, as after double-clicking or typing it.
        assert_eq!(name(9), Some("total"));
        assert_eq!(name(16), Some("values"));
        assert_eq!(name(10), None);
        assert_eq!(name(line.len()), None);
        // Numbers aren't names; names with digits, underscores and other
        // scripts are.
        assert_eq!(name_at("x = 42", 5), None);
        assert_eq!(name_at("max_2 = 1", 2), Some((0, 5)));
        assert_eq!(name_at("größe = 1", 4), Some((0, 7)));
    }

    #[test]
    fn typing_replaces_a_selected_name() {
        let mut rename = RenameBox {
            pane: 0,
            document: 0,
            start: Pos::default(),
            old_name: "total".into(),
            text: "total".into(),
            selected: true,
        };
        type_into(&mut rename, "s");
        assert_eq!(rename.text, "s");
        type_into(&mut rename, "um");
        assert_eq!(rename.text, "sum");
        type_into(&mut rename, &"x".repeat(MAX_NAME));
        assert_eq!(rename.text.chars().count(), MAX_NAME);
    }
}
