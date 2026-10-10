// The Problems tab of the bottom panel: the errors, warnings and notes the
// language servers report for the open files, by file. Ctrl+Shift+M, the
// PROBLEMS tab or the counts in the status bar open it. A click on a row
// selects it and goes there. While the list has the keyboard (once opened or
// clicked), Up and Down select, Enter goes to the selection and Escape gives
// the keyboard back to the editor; typing always goes to the editor.

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
        self.terminal_tab = TerminalTab::Problems;
        self.terminal_focus = false;
        self.panel_focus = false;
        self.problem_focus = true;
        self.problem_selected = 0;
        self.problem_first = 0;
        unsafe {
            SetFocus(hwnd);
            InvalidateRect(hwnd, null(), 0);
        }
    }

    // The panel below its tab strip, while the Problems tab shows.
    fn problems_body(&self, hwnd: HWND) -> Option<RECT> {
        if !self.terminal_visible || self.terminal_tab != TerminalTab::Problems || self.welcome {
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

    // The row at the list's `slot`th visible place: its rectangle.
    fn problem_row_rect(&self, body: RECT, slot: usize) -> RECT {
        let top = body.top + self.scale(4) + slot as i32 * self.scale(ROW);
        RECT {
            top,
            bottom: top + self.scale(ROW),
            ..body
        }
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
        // Text stops short of the scrollbar.
        let right = body.right - s(SCROLLBAR);
        let visible = self.visible_problem_rows(body);
        for (slot, row) in rows
            .iter()
            .skip(self.problem_first)
            .take(visible)
            .enumerate()
        {
            let rect = self.problem_row_rect(body, slot);
            let (top, middle) = (rect.top, rect.top + s(ROW) / 2);
            // The selection shows while the list has the keyboard.
            if self.problem_focus && self.problem_first + slot == self.problem_selected {
                Self::fill(
                    hdc,
                    RECT {
                        left: rect.left + s(4),
                        right: right - s(4),
                        ..rect
                    },
                    self.theme.card_edge,
                );
            }
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
                    let text_right = (right - s(12) - place_width - s(12)).max(text_left);
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

    /// A click in the Problems list: selects the row and goes there (a
    /// problem to its place, a file heading to its tab). True when the click
    /// was on the list.
    pub(super) fn problems_click(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some(body) = self.problems_body(hwnd) else {
            return false;
        };
        if x < body.left || x >= body.right || y < body.top || y >= body.bottom {
            return false;
        }
        if self.problems_scrollbar_at(hwnd, x, y) {
            self.problems_scrollbar_press(hwnd, y);
            return true;
        }
        let slot = ((y - body.top - self.scale(4)).max(0) / self.scale(ROW).max(1)) as usize;
        let index = self.problem_first + slot;
        if index < self.problem_rows().len() {
            self.problem_selected = index;
            self.open_problem(hwnd, index);
        }
        // The list keeps the keyboard, so Up and Down go on from here.
        self.problem_focus = true;
        unsafe { SetFocus(hwnd) };
        true
    }

    // Goes to row `index`: its tab, and for a problem its place.
    fn open_problem(&mut self, hwnd: HWND, index: usize) {
        let Some(row) = self.problem_rows().get(index).copied() else {
            return;
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
            self.doc_mut().unfold_to_reveal(line);
            self.move_cursor(Pos { line, byte }, false);
            self.keep_cursor_visible(hwnd);
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Up, Down, Enter and Escape while the list has the keyboard. True when
    /// the key was the list's.
    pub(super) fn problems_key(&mut self, hwnd: HWND, key: u32, ctrl: bool) -> bool {
        if !self.problem_focus
            || ctrl
            || self.panel_focus
            || self.terminal_focus
            || self.problems_body(hwnd).is_none()
        {
            return false;
        }
        let count = self.problem_rows().len();
        match key {
            k if k == VK_UP as u32 => {
                self.select_problem(hwnd, self.problem_selected.saturating_sub(1))
            }
            k if k == VK_DOWN as u32 => self.select_problem(hwnd, self.problem_selected + 1),
            k if k == VK_RETURN as u32 => {
                if self.problem_selected < count {
                    self.open_problem(hwnd, self.problem_selected);
                }
                self.problem_focus = false;
                self.invalidate_problems(hwnd);
            }
            k if k == VK_ESCAPE as u32 => {
                self.problem_focus = false;
                self.invalidate_problems(hwnd);
            }
            _ => return false,
        }
        true
    }

    // Selects row `index` (clamped) and scrolls it into view.
    fn select_problem(&mut self, hwnd: HWND, index: usize) {
        let Some(body) = self.problems_body(hwnd) else {
            return;
        };
        let count = self.problem_rows().len();
        if count == 0 {
            return;
        }
        let visible = self.visible_problem_rows(body);
        self.problem_selected = index.min(count - 1);
        if self.problem_selected < self.problem_first {
            self.problem_first = self.problem_selected;
        } else if self.problem_selected >= self.problem_first + visible {
            self.problem_first = self.problem_selected + 1 - visible;
        }
        unsafe { InvalidateRect(hwnd, &body, 0) };
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
        let first = (self.problem_first as isize + rows).clamp(0, last_first as isize) as usize;
        if first != self.problem_first {
            self.problem_first = first;
            unsafe { InvalidateRect(hwnd, &body, 0) };
        }
        true
    }

    /// The list's scrollbar track and slider, when it doesn't all fit.
    pub(super) fn problems_scrollbar(&self, hwnd: HWND) -> Option<(RECT, RECT)> {
        let body = self.problems_body(hwnd)?;
        let total = self.problem_rows().len();
        let visible = self.visible_problem_rows(body);
        let height = body.bottom - body.top;
        if total <= visible || height <= 0 {
            return None;
        }
        let track = RECT {
            left: body.right - self.scale(SCROLLBAR),
            ..body
        };
        let length =
            ((height as usize * visible / total) as i32).clamp(self.scale(20).min(height), height);
        let room = (height - length).max(0);
        let max_first = total - visible;
        let top =
            track.top + (room as usize * self.problem_first.min(max_first) / max_first) as i32;
        Some((
            track,
            RECT {
                top,
                bottom: top + length,
                ..track
            },
        ))
    }

    /// Whether the list's scrollbar is at (`x`, `y`).
    pub(super) fn problems_scrollbar_at(&self, hwnd: HWND, x: i32, y: i32) -> bool {
        // Cheap test first: this runs on every mouse move over the panel.
        let Some(body) = self.problems_body(hwnd) else {
            return false;
        };
        if x < body.right - self.scale(SCROLLBAR) || x >= body.right {
            return false;
        }
        self.problems_scrollbar(hwnd)
            .is_some_and(|(track, _)| y >= track.top && y < track.bottom)
    }

    /// A press on the scrollbar: on the slider it starts a drag; on the track
    /// it first moves the slider's middle there.
    pub(super) fn problems_scrollbar_press(&mut self, hwnd: HWND, y: i32) {
        let Some((track, slider)) = self.problems_scrollbar(hwnd) else {
            return;
        };
        if y < slider.top || y >= slider.bottom {
            let length = slider.bottom - slider.top;
            let room = (track.bottom - track.top - length).max(1);
            let offset = (y - length / 2 - track.top).clamp(0, room) as usize;
            let max_first = self.problems_max_first(hwnd);
            self.problem_first =
                ((offset * max_first + room as usize / 2) / room as usize).min(max_first);
        }
        self.problem_scrollbar_grab = Some((y, self.problem_first));
        unsafe {
            SetCapture(hwnd);
            InvalidateRect(hwnd, &track, 0);
        }
        self.invalidate_problems(hwnd);
    }

    /// While the slider is held: the list follows the mouse.
    pub(super) fn problems_scrollbar_drag(&mut self, hwnd: HWND, y: i32) {
        let Some((from_y, from_first)) = self.problem_scrollbar_grab else {
            return;
        };
        let Some((track, slider)) = self.problems_scrollbar(hwnd) else {
            return;
        };
        let max_first = self.problems_max_first(hwnd);
        let room = (track.bottom - track.top - (slider.bottom - slider.top)).max(1) as i64;
        let moved = (y - from_y) as i64 * max_first as i64 / room;
        let first = (from_first as i64 + moved).clamp(0, max_first as i64) as usize;
        if first != self.problem_first {
            self.problem_first = first;
            self.invalidate_problems(hwnd);
        }
    }

    fn problems_max_first(&self, hwnd: HWND) -> usize {
        self.problems_body(hwnd).map_or(0, |body| {
            self.problem_rows()
                .len()
                .saturating_sub(self.visible_problem_rows(body))
        })
    }

    /// After problems changed: keeps the selection and scroll in range.
    pub(super) fn clamp_problems(&mut self) {
        let count = self.problem_rows().len();
        self.problem_selected = self.problem_selected.min(count.saturating_sub(1));
        self.problem_first = self.problem_first.min(count.saturating_sub(1));
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
