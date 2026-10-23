//! The Match Case, Whole Word and Regex toggles (Aa, ab, .*) at the right
//! of the find box's field and the Search panel's query, as in VS Code.
//! Alt+C, Alt+W and Alt+R flip them from the keyboard.

use super::*;

const SIZE: i32 = 22;
const GAP: i32 = 2;
const LABELS: [&str; 3] = ["Aa", "ab", ".*"];

/// Which toggle a key flips: Alt+C, Alt+W, Alt+R.
pub(super) fn toggle_for_key(key: u32) -> Option<usize> {
    match key {
        0x43 => Some(0),
        0x57 => Some(1),
        0x52 => Some(2),
        _ => None,
    }
}

pub(super) fn flip(options: &mut SearchOptions, toggle: usize) {
    match toggle {
        0 => options.match_case = !options.match_case,
        1 => options.whole_word = !options.whole_word,
        _ => options.regex = !options.regex,
    }
}

fn is_on(options: SearchOptions, toggle: usize) -> bool {
    [options.match_case, options.whole_word, options.regex][toggle]
}

/// "Regex, match case" and the like, for the status bar after a toggle.
pub(super) fn describe(options: SearchOptions) -> String {
    let mut on = Vec::new();
    if options.regex {
        on.push("regular expression");
    }
    if options.match_case {
        on.push("match case");
    }
    if options.whole_word {
        on.push("whole word");
    }
    if on.is_empty() {
        "Find: plain text, any case".into()
    } else {
        format!("Find: {}", on.join(", "))
    }
}

impl App {
    /// The three toggles inside the right end of `field`, left to right.
    pub(super) fn option_toggles(&self, field: RECT) -> [RECT; 3] {
        let size = self.scale(SIZE);
        let top = (field.top + field.bottom - size) / 2;
        let right = field.right - self.scale(4);
        std::array::from_fn(|index| {
            let left = right - (3 - index as i32) * size - (2 - index as i32) * self.scale(GAP);
            RECT {
                left,
                top,
                right: left + size,
                bottom: top + size,
            }
        })
    }

    /// Where the field's text has to stop so it doesn't run under them.
    pub(super) fn option_toggles_left(&self, field: RECT) -> i32 {
        self.option_toggles(field)[0].left - self.scale(4)
    }

    pub(super) fn option_toggle_at(&self, field: RECT, x: i32, y: i32) -> Option<usize> {
        self.option_toggles(field)
            .iter()
            .position(|r| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
    }

    pub(super) fn paint_option_toggles(&self, hdc: HDC, field: RECT, options: SearchOptions) {
        unsafe { SelectObject(hdc, self.ui_font) };
        for (index, rect) in self.option_toggles(field).into_iter().enumerate() {
            let on = is_on(options, index);
            if on {
                self.panel_card(
                    hdc,
                    rect,
                    self.scale(4),
                    self.theme.sky,
                    self.theme.select_bg,
                );
            }
            let label = LABELS[index];
            let color = if on {
                self.theme.text
            } else {
                self.theme.muted
            };
            let width = self.text_width(hdc, label);
            let left = (rect.left + rect.right - width) / 2;
            let middle = (rect.top + rect.bottom) / 2 - self.scale(1);
            self.label_mid(hdc, label, left, middle, color, rect);
            if index == 1 {
                // Whole Word: "ab" over a line, VS Code's mark for it.
                let y = middle + self.text_height(hdc) / 2;
                Self::fill(
                    hdc,
                    RECT {
                        left,
                        top: y,
                        right: left + width,
                        bottom: y + self.scale(1).max(1),
                    },
                    color,
                );
            }
        }
    }

    /// Flips one of the find box's toggles and finds again with it.
    pub(super) fn flip_find_option(&mut self, hwnd: HWND, toggle: usize) {
        flip(&mut self.find_options, toggle);
        self.status = describe(self.find_options);
        self.find_query_changed(hwnd);
    }

    /// Flips one of the Search panel's toggles and searches again with it.
    pub(super) fn flip_search_option(&mut self, hwnd: HWND, toggle: usize) {
        flip(&mut self.search_options, toggle);
        let typing = self.search_input;
        if !self.project_query.is_empty() {
            self.search_project();
            // Searching hands the keyboard to the results; a toggle
            // shouldn't take it from the query being typed.
            self.search_input = typing;
            self.panel_focus = true;
        }
        if !self.status.starts_with("Invalid") {
            self.status = describe(self.search_options);
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}
