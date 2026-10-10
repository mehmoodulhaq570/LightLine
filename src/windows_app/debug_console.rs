// The Debug Console tab of the bottom panel: what the debug adapter reports
// while a session runs (the program's output when the adapter captures it,
// its errors, and its own messages), notes of the session's progress, and a
// prompt at the bottom to evaluate expressions while paused, as in VS Code.
// It follows the newest line until the wheel scrolls it back, and is emptied
// when a new session starts. Ctrl+Shift+Y or its tab shows it; a click in it
// (or showing it during a session) gives the prompt the keyboard, and
// Escape gives it back to the editor.

use super::terminal::TERMINAL_HEADER;
use super::*;
use lightline::debug::Command as DebugCommand;

// Lines kept; older ones are dropped.
const MAX_LINES: usize = 10_000;
// Expressions remembered for Up and Down.
const MAX_HISTORY: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OutputKind {
    // The program's output, and values of expressions.
    Program,
    // Errors (stderr, failed expressions).
    Error,
    // Worth noticing ("important").
    Notice,
    // The adapter's own messages ("console") and the session's notes.
    Adapter,
    // An expression typed at the prompt.
    Input,
}

#[derive(Default)]
pub(super) struct DebugConsole {
    lines: Vec<(OutputKind, String)>,
    // Text after the last line break, waiting for the rest of its line.
    partial: Option<(OutputKind, String)>,
    // The first line shown; None follows the newest.
    first: Option<usize>,
    // The prompt: what's typed, whether it has the keyboard, and the
    // expressions entered before (Up and Down go through them).
    input: String,
    pub(super) focus: bool,
    history: Vec<String>,
    history_at: Option<usize>,
}

impl DebugConsole {
    /// Adds what the adapter sent, under its DAP output category ("input"
    /// for an expression typed at the prompt).
    pub(super) fn push(&mut self, category: &str, text: &str) {
        let kind = match category {
            "stdout" => OutputKind::Program,
            "stderr" => OutputKind::Error,
            "important" => OutputKind::Notice,
            "input" => OutputKind::Input,
            _ => OutputKind::Adapter,
        };
        let mut rest = text.replace("\r\n", "\n");
        if let Some((partial_kind, partial)) = self.partial.take() {
            if partial_kind == kind {
                rest.insert_str(0, &partial);
            } else {
                self.lines.push((partial_kind, partial));
            }
        }
        let ends_line = rest.ends_with('\n');
        let mut pieces: Vec<&str> = rest.split('\n').collect();
        if ends_line {
            pieces.pop();
        } else if let Some(last) = pieces.pop() {
            self.partial = Some((kind, last.to_string()));
        }
        self.lines
            .extend(pieces.into_iter().map(|line| (kind, line.to_string())));
        if self.lines.len() > MAX_LINES {
            let extra = self.lines.len() - MAX_LINES;
            self.lines.drain(..extra);
            self.first = self.first.map(|first| first.saturating_sub(extra));
        }
    }

    /// A whole line of LightLine's own about the session.
    pub(super) fn note(&mut self, text: &str) {
        self.push("console", &format!("{text}\n"));
    }

    /// A new session: the lines go; the prompt's history stays.
    pub(super) fn clear(&mut self) {
        self.lines.clear();
        self.partial = None;
        self.first = None;
    }

    // Every line, with the unfinished one last.
    fn all(&self) -> impl Iterator<Item = &(OutputKind, String)> {
        self.lines.iter().chain(self.partial.as_ref())
    }

    fn count(&self) -> usize {
        self.lines.len() + usize::from(self.partial.is_some())
    }

    // Up (`older`) or Down through the expressions entered before.
    fn recall(&mut self, older: bool) {
        if self.history.is_empty() {
            return;
        }
        let at = match (self.history_at, older) {
            (None, true) => Some(self.history.len() - 1),
            (None, false) => None,
            (Some(at), true) => Some(at.saturating_sub(1)),
            (Some(at), false) => (at + 1 < self.history.len()).then_some(at + 1),
        };
        self.history_at = at;
        self.input = at.map(|at| self.history[at].clone()).unwrap_or_default();
    }
}

impl App {
    pub(super) fn show_debug_console(&mut self, hwnd: HWND) {
        self.welcome = false;
        self.terminal_visible = true;
        self.terminal_tab = TerminalTab::DebugConsole;
        self.terminal_focus = false;
        self.problem_focus = false;
        self.panel_focus = false;
        self.debug_console.focus = self.debug.is_some();
        unsafe {
            SetFocus(hwnd);
            InvalidateRect(hwnd, null(), 0);
        }
    }

    // The panel below its tab strip, while the Debug Console shows.
    fn debug_console_body(&self, hwnd: HWND) -> Option<RECT> {
        if !self.terminal_visible || self.terminal_tab != TerminalTab::DebugConsole || self.welcome
        {
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

    // The prompt shows while a session runs.
    fn debug_prompt_shown(&self) -> bool {
        self.debug.is_some()
    }

    /// Whether typing goes to the prompt.
    pub(super) fn debug_prompt_has_keyboard(&self, hwnd: HWND) -> bool {
        self.debug_console.focus
            && self.debug_prompt_shown()
            && !self.terminal_focus
            && !self.panel_focus
            && self.debug_console_body(hwnd).is_some()
    }

    // Rows for lines (the prompt takes the last).
    fn debug_console_rows(&self, body: RECT) -> usize {
        let rows = ((body.bottom - body.top - self.scale(8)) / self.line_height.max(1)).max(1);
        (rows as usize)
            .saturating_sub(usize::from(self.debug_prompt_shown()))
            .max(1)
    }

    // The first line shown: the one scrolled to, or those that end with the
    // newest.
    fn debug_console_first(&self, body: RECT) -> usize {
        let last_first = self
            .debug_console
            .count()
            .saturating_sub(self.debug_console_rows(body));
        self.debug_console
            .first
            .map_or(last_first, |first| first.min(last_first))
    }

    // The prompt's row.
    fn debug_prompt_rect(&self, body: RECT) -> RECT {
        let top = body.bottom - self.scale(4) - self.line_height;
        RECT {
            top,
            bottom: top + self.line_height,
            ..body
        }
    }

    pub(super) fn paint_debug_console(&self, hdc: HDC, hwnd: HWND) {
        let Some(body) = self.debug_console_body(hwnd) else {
            return;
        };
        let s = |value: i32| self.scale(value);
        let tab_text = " ".repeat(self.settings.tab_size);
        if self.debug_console.count() == 0 && !self.debug_prompt_shown() {
            unsafe { SelectObject(hdc, self.ui_font) };
            Self::label(
                hdc,
                "Output from the debugger shows here while a debug session runs (F5); \
                 while paused, type an expression to see its value.",
                body.left + s(18),
                body.top + s(8),
                self.theme.muted,
                body,
            );
            return;
        }
        unsafe { SelectObject(hdc, self.font) };
        let first = self.debug_console_first(body);
        let rows = self.debug_console_rows(body);
        let text_right = RECT {
            right: body.right - s(8),
            ..body
        };
        for (slot, (kind, text)) in self.debug_console.all().skip(first).take(rows).enumerate() {
            let color = match kind {
                OutputKind::Program => self.theme.text,
                OutputKind::Error => self.theme.error,
                OutputKind::Notice => self.theme.warning,
                OutputKind::Adapter => self.theme.muted,
                OutputKind::Input => self.theme.blue,
            };
            let y = body.top + s(4) + slot as i32 * self.line_height;
            Self::label(
                hdc,
                &text.replace('\t', &tab_text),
                body.left + s(16),
                y,
                color,
                text_right,
            );
        }
        if self.debug_prompt_shown() {
            let prompt = self.debug_prompt_rect(body);
            Self::fill(
                hdc,
                RECT {
                    top: prompt.top - s(2),
                    bottom: prompt.top - s(1),
                    ..prompt
                },
                self.theme.edge,
            );
            let x = body.left + s(16);
            Self::label(hdc, "\u{203a}", x, prompt.top, self.theme.blue, text_right);
            let input_x = x + self.text_width(hdc, "\u{203a} ");
            if self.debug_console.input.is_empty() && !self.debug_console.focus {
                Self::label(
                    hdc,
                    "Click here, then type an expression and press Enter",
                    input_x,
                    prompt.top,
                    self.theme.muted,
                    text_right,
                );
            } else {
                Self::label(
                    hdc,
                    &self.debug_console.input,
                    input_x,
                    prompt.top,
                    self.theme.text,
                    text_right,
                );
            }
            if self.debug_prompt_has_keyboard(hwnd) && self.caret_on {
                let caret_x = input_x + self.text_width(hdc, &self.debug_console.input);
                Self::fill(
                    hdc,
                    RECT {
                        left: caret_x,
                        top: prompt.top,
                        right: caret_x + s(2).max(2),
                        bottom: prompt.bottom,
                    },
                    self.theme.cursor,
                );
            }
        }
    }

    /// A click in the console's body: the prompt takes the keyboard. True
    /// when the click was there.
    pub(super) fn debug_console_click(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some(body) = self.debug_console_body(hwnd) else {
            return false;
        };
        if x < body.left || x >= body.right || y < body.top || y >= body.bottom {
            return false;
        }
        self.debug_console.focus = self.debug_prompt_shown();
        self.terminal_focus = false;
        self.panel_focus = false;
        unsafe {
            SetFocus(hwnd);
            InvalidateRect(hwnd, &body, 0);
        }
        // The editor's caret hides while the prompt has the keyboard.
        self.invalidate_caret(hwnd);
        true
    }

    /// A typed character for the prompt. True when the prompt took it.
    pub(super) fn debug_console_char(&mut self, hwnd: HWND, unit: u16) -> bool {
        if !self.debug_prompt_has_keyboard(hwnd) {
            return false;
        }
        // Control characters (Enter, Backspace, Escape) are keys, handled in
        // debug_console_key.
        if let Some(ch) = char::from_u32(unit as u32).filter(|ch| !ch.is_control()) {
            self.debug_console.input.push(ch);
            self.debug_console.history_at = None;
            self.invalidate_debug_prompt(hwnd);
        }
        true
    }

    /// Enter, Backspace, Escape, Up, Down and Ctrl+V at the prompt. True
    /// when the prompt took the key; F5, F10 and the rest still work.
    pub(super) fn debug_console_key(&mut self, hwnd: HWND, key: u32, ctrl: bool) -> bool {
        if !self.debug_prompt_has_keyboard(hwnd) {
            return false;
        }
        match key {
            k if k == VK_RETURN as u32 && !ctrl => self.evaluate_debug_input(hwnd),
            k if k == VK_BACK as u32 => {
                self.debug_console.input.pop();
            }
            k if k == VK_ESCAPE as u32 => {
                self.debug_console.focus = false;
                self.invalidate_caret(hwnd);
            }
            k if k == VK_UP as u32 => self.debug_console.recall(true),
            k if k == VK_DOWN as u32 => self.debug_console.recall(false),
            0x56 if ctrl => {
                if let Ok(Some(text)) = clipboard::paste(hwnd) {
                    let line = text.lines().next().unwrap_or("");
                    self.debug_console.input.push_str(line);
                }
            }
            // Keys that would move the editor's caret unseen.
            k if !ctrl
                && [VK_LEFT, VK_RIGHT, VK_HOME, VK_END, VK_DELETE, VK_TAB]
                    .iter()
                    .any(|&other| k == other as u32) => {}
            _ => return false,
        }
        self.invalidate_debug_prompt(hwnd);
        true
    }

    // Enter at the prompt: shows the expression and asks for its value, in
    // the paused frame.
    fn evaluate_debug_input(&mut self, hwnd: HWND) {
        let expression = self.debug_console.input.trim().to_string();
        if expression.is_empty() {
            return;
        }
        self.debug_console.input.clear();
        self.debug_console.history_at = None;
        self.debug_console.history.retain(|old| *old != expression);
        self.debug_console.history.push(expression.clone());
        if self.debug_console.history.len() > MAX_HISTORY {
            self.debug_console.history.remove(0);
        }
        self.debug_console
            .push("input", &format!("\u{203a} {expression}\n"));
        self.debug_console.first = None;
        let frame = if self.debug_paused() {
            self.debug_state.frames.first().map(|frame| frame.id)
        } else {
            None
        };
        let sent = self
            .debug
            .as_ref()
            .is_some_and(|client| client.send(DebugCommand::Evaluate { expression, frame }));
        if !sent {
            self.debug_console
                .push("stderr", "The debugger isn't running.\n");
        }
        self.invalidate_debug_console(hwnd);
    }

    /// An expression's value (or why there's none) arrived.
    pub(super) fn finish_debug_evaluate(&mut self, result: Result<String, String>) {
        match result {
            Ok(value) => self.debug_console.push("stdout", &format!("{value}\n")),
            Err(reason) => self.debug_console.push("stderr", &format!("{reason}\n")),
        }
        self.debug_console.first = None;
    }

    pub(super) fn invalidate_debug_prompt(&self, hwnd: HWND) {
        if let Some(body) = self.debug_console_body(hwnd) {
            let prompt = self.debug_prompt_rect(body);
            unsafe { InvalidateRect(hwnd, &prompt, 0) };
        }
    }

    /// The mouse wheel over the console: true when it was over it.
    pub(super) fn scroll_debug_console(&mut self, hwnd: HWND, x: i32, y: i32, rows: isize) -> bool {
        let Some(body) = self.debug_console_body(hwnd) else {
            return false;
        };
        if x < body.left || x >= body.right || y < body.top || y >= body.bottom {
            return false;
        }
        let last_first = self
            .debug_console
            .count()
            .saturating_sub(self.debug_console_rows(body));
        let first =
            (self.debug_console_first(body) as isize + rows).clamp(0, last_first as isize) as usize;
        // Back at the end, it follows new lines again.
        self.debug_console.first = (first < last_first).then_some(first);
        unsafe { InvalidateRect(hwnd, &body, 0) };
        true
    }

    /// New output: redraws the console when it shows.
    pub(super) fn invalidate_debug_console(&self, hwnd: HWND) {
        if let Some(body) = self.debug_console_body(hwnd) {
            unsafe { InvalidateRect(hwnd, &body, 0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(console: &DebugConsole) -> Vec<(OutputKind, &str)> {
        console
            .all()
            .map(|(kind, text)| (*kind, text.as_str()))
            .collect()
    }

    #[test]
    fn output_is_split_into_lines_by_kind() {
        let mut console = DebugConsole::default();
        console.push("stdout", "one\ntw");
        console.push("stdout", "o\n");
        console.push("stderr", "oops\r\n");
        console.push("console", "Launching");
        console.push("stdout", "three");
        assert_eq!(
            texts(&console),
            [
                (OutputKind::Program, "one"),
                (OutputKind::Program, "two"),
                (OutputKind::Error, "oops"),
                (OutputKind::Adapter, "Launching"),
                (OutputKind::Program, "three"),
            ]
        );
        console.clear();
        assert_eq!(console.count(), 0);
    }

    #[test]
    fn old_lines_are_dropped() {
        let mut console = DebugConsole::default();
        for index in 0..MAX_LINES + 5 {
            console.push("stdout", &format!("{index}\n"));
        }
        assert_eq!(console.count(), MAX_LINES);
        assert_eq!(console.all().next().unwrap().1, "5");
    }

    #[test]
    fn up_and_down_go_through_earlier_expressions() {
        let mut console = DebugConsole {
            history: vec!["a".into(), "b".into()],
            ..Default::default()
        };
        console.recall(true);
        assert_eq!(console.input, "b");
        console.recall(true);
        console.recall(true);
        assert_eq!(console.input, "a");
        console.recall(false);
        assert_eq!(console.input, "b");
        console.recall(false);
        assert_eq!(console.input, "");
    }
}
