// The Problems tab of the bottom panel: the errors, warnings and notes the
// language servers report for the open files, by file. A click on one goes
// there. Ctrl+Shift+M, the PROBLEMS tab, or the counts in the status bar
// open it. It never takes the keyboard: typing still goes to the editor.

use super::terminal::TERMINAL_HEADER;
use super::*;

// Each row's height, in logical pixels.
const ROW: i32 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProblemRow {
    // A file's heading: its tab.
    File(usize),
    // A problem: its tab, and its index in that tab's diagnostics.
    Item(usize, usize),
}

impl App {
    /// The list as shown: each open file with problems, then its problems
    /// by line. Hints (severity 4) are left out, as in the status bar.
    pub(super) fn problem_rows(&self) -> Vec<ProblemRow> {
        let mut rows = Vec::new();
        for (tab_index, tab) in self.tabs.iter().enumerate() {
            let mut items: Vec<usize> = (0..tab.diagnostics.len())
                .filter(|&index| tab.diagnostics[index].severity <= 3)
                .collect();
            if items.is_empty() {
                continue;
            }
            let at = |index: usize| {
                let range = tab.diagnostics[index].range;
                (range.start.line, range.start.character)
            };
            items.sort_by_key(|&index| at(index));
            rows.push(ProblemRow::File(tab_index));
            rows.extend(
                items
                    .into_iter()
                    .map(|index| ProblemRow::Item(tab_index, index)),
            );
        }
        rows
    }

    pub(super) fn show_problems(&mut self, hwnd: HWND) {
        self.welcome = false;
        self.terminal_visible = true;
        self.problems_shown = true;
        // The list has no keyboard use; keys stay with the editor.
        self.terminal_focus = false;
        self.problems_first = 0;
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    // The panel below its tab strip, while the Problems tab shows.
    fn problems_body(&self, hwnd: HWND) -> Option<RECT> {
        if !(self.terminal_visible && self.problems_shown) || self.welcome {
            return None;
        }
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        Some(RECT {
            left: self.editor_left(),
            top: self.terminal_top(hwnd) + self.scale(TERMINAL_HEADER),
            right: self.editor_right(hwnd),
            bottom: client.bottom - self.scale(STATUS),
        })
    }

    fn visible_problem_rows(&self, body: RECT) -> usize {
        ((body.bottom - body.top - self.scale(4)) / self.scale(ROW).max(1)).max(1) as usize
    }

    pub(super) fn paint_problems(&self, hdc: HDC, hwnd: HWND) {
        let Some(body) = self.problems_body(hwnd) else {
            return;
        };
        let s = |value: i32| self.scale(value);
        let rows = self.problem_rows();
        unsafe { SelectObject(hdc, self.ui_font) };
        if rows.is_empty() {
            Self::label(
                hdc,
                "No problems have been detected in the open files.",
                body.left + s(18),
                body.top + s(8),
                self.theme.muted,
                body,
            );
            return;
        }
        let visible = self.visible_problem_rows(body);
        for (slot, row) in rows
            .iter()
            .skip(self.problems_first)
            .take(visible)
            .enumerate()
        {
            let top = body.top + s(4) + slot as i32 * s(ROW);
            let middle = top + s(ROW) / 2;
            match *row {
                ProblemRow::File(tab_index) => {
                    let tab = &self.tabs[tab_index];
                    let path = tab.document.path.as_deref();
                    let name = path
                        .and_then(Path::file_name)
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Untitled".into());
                    let folder = path
                        .and_then(Path::parent)
                        .map(|folder| {
                            self.workspace_root
                                .as_deref()
                                .and_then(|root| folder.strip_prefix(root).ok())
                                .unwrap_or(folder)
                                .display()
                                .to_string()
                        })
                        .unwrap_or_default();
                    let count = tab.diagnostics.iter().filter(|d| d.severity <= 3).count();
                    let x = body.left + s(16);
                    self.label_mid(hdc, &name, x, middle, self.theme.text, body);
                    let x = x + self.text_width(hdc, &name) + s(10);
                    let detail = if folder.is_empty() {
                        count.to_string()
                    } else {
                        format!("{folder}   {count}")
                    };
                    self.label_mid(hdc, &detail, x, middle, self.theme.muted, body);
                }
                ProblemRow::Item(tab_index, index) => {
                    let problem = &self.tabs[tab_index].diagnostics[index];
                    let (mark, color) = match problem.severity {
                        1 => ("\u{2297}", self.theme.error),
                        2 => ("\u{26a0}", self.theme.warning),
                        _ => ("i", self.theme.info),
                    };
                    let x = body.left + s(34);
                    self.label_mid(hdc, mark, x, middle, color, body);
                    let place = format!(
                        "[Ln {}, Col {}]",
                        problem.range.start.line + 1,
                        problem.range.start.character + 1
                    );
                    let place_width = self.text_width(hdc, &place);
                    let message = problem.message.lines().next().unwrap_or("").trim();
                    let text_left = x + s(22);
                    let text_right = (body.right - s(16) - place_width - s(12)).max(text_left);
                    let text_top = middle - self.text_height(hdc) / 2;
                    self.label_ellipsis(
                        hdc,
                        message,
                        text_left,
                        text_top,
                        self.theme.text,
                        RECT {
                            left: text_left,
                            top,
                            right: text_right,
                            bottom: top + s(ROW),
                        },
                    );
                    self.label_mid(
                        hdc,
                        &place,
                        text_right + s(12),
                        middle,
                        self.theme.muted,
                        body,
                    );
                }
            }
        }
    }

    /// A click in the Problems list: a problem goes to its place, a file
    /// heading to its tab. True when the click was on the list.
    pub(super) fn problems_click(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some(body) = self.problems_body(hwnd) else {
            return false;
        };
        if x < body.left || x >= body.right || y < body.top || y >= body.bottom {
            return false;
        }
        let slot = ((y - body.top - self.scale(4)).max(0) / self.scale(ROW).max(1)) as usize;
        let Some(row) = self.problem_rows().get(self.problems_first + slot).copied() else {
            return true;
        };
        let (tab_index, problem) = match row {
            ProblemRow::File(tab_index) => (tab_index, None),
            ProblemRow::Item(tab_index, index) => (tab_index, Some(index)),
        };
        self.panel_focus = false;
        self.terminal_focus = false;
        if tab_index != self.active {
            self.activate_tab(hwnd, tab_index);
        }
        if let Some(position) =
            problem.map(|index| self.tabs[tab_index].diagnostics[index].range.start)
        {
            let doc = self.doc();
            let line = (position.line as usize).min(doc.line_count().saturating_sub(1));
            let byte = lsp::utf16_to_byte(doc.line(line), position.character);
            self.move_cursor(Pos { line, byte }, false);
            self.keep_cursor_visible(hwnd);
        }
        unsafe {
            SetFocus(hwnd);
            InvalidateRect(hwnd, null(), 0);
        }
        true
    }

    /// The mouse wheel over the list: true when it scrolled it.
    pub(super) fn scroll_problems(&mut self, hwnd: HWND, x: i32, y: i32, rows: isize) -> bool {
        let Some(body) = self.problems_body(hwnd) else {
            return false;
        };
        if x < body.left || x >= body.right || y < body.top || y >= body.bottom {
            return false;
        }
        let total = self.problem_rows().len();
        let last_first = total.saturating_sub(self.visible_problem_rows(body));
        let first = (self.problems_first as isize + rows).clamp(0, last_first as isize) as usize;
        if first != self.problems_first {
            self.problems_first = first;
            unsafe { InvalidateRect(hwnd, &body, 0) };
        }
        true
    }

    /// After problems changed: redraws the tab's count and, when it shows,
    /// the list.
    pub(super) fn invalidate_problems(&self, hwnd: HWND) {
        if !self.terminal_visible || self.welcome {
            return;
        }
        let top = self.terminal_top(hwnd);
        let header = self.terminal_header_layout(self.editor_left(), self.editor_right(hwnd), top);
        unsafe { InvalidateRect(hwnd, &header.problems, 0) };
        if let Some(body) = self.problems_body(hwnd) {
            unsafe { InvalidateRect(hwnd, &body, 0) };
        }
    }
}
