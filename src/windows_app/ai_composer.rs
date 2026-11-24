//! The AI Assistant's message box as a text field: a caret that moves with
//! the arrow keys, Home and End, Up and Down across wrapped lines, and a
//! click; typing, pasting and deleting happen at the caret.
//! The painter (render/ai_assistant.rs) wraps the text with `wrap_ranges`
//! and records the rows it drew in `AiChat::composer_rows`, which clicks
//! and Up/Down read.

use super::*;

/// How long Copy shows its tick.
pub(super) const AI_COPIED_TIMER: usize = 29;
const COPIED_FOR_MS: u32 = 1500;

/// One drawn row of the message box: where it is, and the bytes of the
/// text on it.
#[derive(Clone, Copy, Default)]
pub(super) struct ComposerRow {
    pub(super) rect: RECT,
    pub(super) start: usize,
    pub(super) end: usize,
}

/// The wrapped line the caret is on: the last one starting at or before
/// it, so a caret at a wrap shows at the start of the next line.
pub(super) fn caret_line(lines: &[(usize, usize)], caret: usize) -> usize {
    lines
        .iter()
        .rposition(|(start, _)| *start <= caret)
        .unwrap_or(0)
}

/// Where the caret goes one character left of `at`.
fn previous_char(text: &str, at: usize) -> usize {
    text[..at]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

/// Where the caret goes one character right of `at`.
fn next_char(text: &str, at: usize) -> usize {
    text[at..]
        .chars()
        .next()
        .map_or(at, |ch| at + ch.len_utf8())
}

/// The start of the word before `at` (Ctrl+Left, Ctrl+Backspace): spaces
/// first, then the word's letters.
pub(super) fn previous_word(text: &str, at: usize) -> usize {
    let mut index = at;
    let mut seen_word = false;
    for (position, ch) in text[..at].char_indices().rev() {
        if ch.is_whitespace() {
            if seen_word {
                break;
            }
        } else {
            seen_word = true;
        }
        index = position;
    }
    index
}

/// The start of the next word after `at` (Ctrl+Right, Ctrl+Delete): past
/// the rest of this word, then the spaces after it.
pub(super) fn next_word(text: &str, at: usize) -> usize {
    let mut seen_space = false;
    for (position, ch) in text[at..].char_indices() {
        if ch.is_whitespace() {
            seen_space = true;
        } else if seen_space {
            return at + position;
        }
    }
    text.len()
}

impl App {
    /// Puts `text` in the message box at the caret, the caret after it.
    pub(super) fn ai_insert(&mut self, text: &str) -> bool {
        if self.ai.input.len() + text.len() > super::ai_chat::MAX_INPUT_BYTES {
            return false;
        }
        let caret = self.ai.caret.min(self.ai.input.len());
        self.ai.input.insert_str(caret, text);
        self.ai.caret = caret + text.len();
        self.ai_input_changed();
        true
    }

    /// Empties the message box.
    pub(super) fn ai_clear_input(&mut self) {
        self.ai.input.clear();
        self.ai.caret = 0;
        self.ai_input_changed();
    }

    // Takes out the text between the caret and `to`.
    fn ai_delete_to(&mut self, to: usize) {
        let caret = self.ai.caret.min(self.ai.input.len());
        let (start, end) = if to < caret { (to, caret) } else { (caret, to) };
        if start < end {
            self.ai.input.replace_range(start..end, "");
            self.ai.caret = start;
            self.ai_input_changed();
        }
    }

    /// The editing keys for the message box: Backspace, Delete, the arrows,
    /// Home and End. None for any other key.
    pub(super) fn ai_caret_key(&mut self, hwnd: HWND, key: u32, ctrl: bool) -> Option<bool> {
        let text = &self.ai.input;
        let caret = self.ai.caret.min(text.len());
        match key {
            k if k == VK_BACK as u32 => {
                if text.is_empty() && !self.ai.attachments.is_empty() {
                    // Backspace in an empty box takes off the last file.
                    self.ai.attachments.pop();
                } else {
                    let to = if ctrl {
                        previous_word(text, caret)
                    } else {
                        previous_char(text, caret)
                    };
                    self.ai_delete_to(to);
                }
            }
            k if k == VK_DELETE as u32 => {
                let to = if ctrl {
                    next_word(text, caret)
                } else {
                    next_char(text, caret)
                };
                self.ai_delete_to(to);
            }
            k if k == VK_LEFT as u32 => {
                self.ai.caret = if ctrl {
                    previous_word(text, caret)
                } else {
                    previous_char(text, caret)
                };
            }
            k if k == VK_RIGHT as u32 => {
                self.ai.caret = if ctrl {
                    next_word(text, caret)
                } else {
                    next_char(text, caret)
                };
            }
            k if k == VK_HOME as u32 || k == VK_END as u32 => {
                let home = k == VK_HOME as u32;
                self.ai.caret = if ctrl {
                    if home { 0 } else { text.len() }
                } else {
                    let lines = self.ai.composer_lines.borrow();
                    match lines.get(caret_line(&lines, caret)) {
                        Some(&(start, end)) => {
                            if home {
                                start
                            } else {
                                end
                            }
                        }
                        None if home => 0,
                        None => text.len(),
                    }
                };
            }
            k if k == VK_UP as u32 || k == VK_DOWN as u32 => {
                self.ai_caret_vertical(hwnd, k == VK_UP as u32);
            }
            _ => return None,
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
        Some(true)
    }

    // Up or Down: the same place on the wrapped line above or below; past
    // the first or last line, the start or end of the text.
    fn ai_caret_vertical(&mut self, hwnd: HWND, up: bool) {
        let lines = self.ai.composer_lines.borrow().clone();
        let caret = self.ai.caret.min(self.ai.input.len());
        let line = caret_line(&lines, caret);
        let target = if up {
            line.checked_sub(1)
        } else {
            Some(line + 1).filter(|next| *next < lines.len())
        };
        let Some(target) = target else {
            self.ai.caret = if up { 0 } else { self.ai.input.len() };
            return;
        };
        let (start, _) = lines[line];
        let x = self.ai_measure(hwnd, &self.ai.input[start..caret.max(start)]);
        let (target_start, target_end) = lines[target];
        self.ai.caret = self.ai_byte_at(hwnd, (target_start, target_end), x);
    }

    // The width of `text` in the message box's font.
    fn ai_measure(&self, hwnd: HWND, text: &str) -> i32 {
        unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.ui_font);
            let width = self.text_width(hdc, text);
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            width
        }
    }

    // The place in the bytes `start..end` of the box's text nearest to `x`
    // pixels from the line's left.
    fn ai_byte_at(&self, hwnd: HWND, (start, end): (usize, usize), x: i32) -> usize {
        let line = &self.ai.input[start..end];
        let mut best = (start, x.abs());
        unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.ui_font);
            for (index, ch) in line.char_indices() {
                let after = index + ch.len_utf8();
                let distance = (self.text_width(hdc, &line[..after]) - x).abs();
                if distance < best.1 {
                    best = (start + after, distance);
                }
            }
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
        }
        best.0
    }

    /// A click in the message box puts the caret where it landed.
    pub(super) fn ai_click_caret(&mut self, hwnd: HWND, x: i32, y: i32) {
        let rows = self.ai.composer_rows.borrow().clone();
        if rows.is_empty() {
            self.ai.caret = self.ai.input.len();
            return;
        }
        let row = rows
            .iter()
            .find(|row| y < row.rect.bottom)
            .unwrap_or(&rows[rows.len() - 1]);
        self.ai.caret = self.ai_byte_at(hwnd, (row.start, row.end), x - row.rect.left);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Copies `text`, and shows a tick on the Copy icon for a moment.
    pub(super) fn ai_copy(&mut self, hwnd: HWND, text: &str) {
        match clipboard::copy(hwnd, text) {
            Ok(()) => {
                self.status = "Copied".into();
                self.ai.copied = Some(text.to_string());
                unsafe { SetTimer(hwnd, AI_COPIED_TIMER, COPIED_FOR_MS, None) };
            }
            Err(error) => self.status = format!("Couldn't copy: {error}"),
        }
    }

    /// The tick's time is up.
    pub(super) fn ai_copied_done(&mut self, hwnd: HWND) {
        unsafe { KillTimer(hwnd, AI_COPIED_TIMER) };
        if self.ai.copied.take().is_some() {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_skipped_like_an_editor() {
        let text = "fix  the mean() call";
        assert_eq!(previous_word(text, text.len()), 16);
        assert_eq!(previous_word(text, 9), 5);
        assert_eq!(previous_word(text, 5), 0);
        assert_eq!(previous_word(text, 0), 0);
        assert_eq!(next_word(text, 0), 5);
        assert_eq!(next_word(text, 5), 9);
        assert_eq!(next_word(text, 15), 16);
        assert_eq!(next_word(text, 16), text.len());
    }

    #[test]
    fn the_caret_moves_by_whole_characters() {
        let text = "aé😀b";
        assert_eq!(next_char(text, 1), 3);
        assert_eq!(next_char(text, 3), 7);
        assert_eq!(previous_char(text, 7), 3);
        assert_eq!(previous_char(text, 0), 0);
        assert_eq!(next_char(text, text.len()), text.len());
    }

    #[test]
    fn a_caret_at_a_wrap_is_on_the_next_line() {
        // "hello world" wrapped after "hello", the space dropped.
        let lines = [(0, 5), (6, 11)];
        assert_eq!(caret_line(&lines, 3), 0);
        assert_eq!(caret_line(&lines, 5), 0);
        assert_eq!(caret_line(&lines, 6), 1);
        assert_eq!(caret_line(&lines, 11), 1);
        assert_eq!(caret_line(&[], 4), 0);
    }
}
