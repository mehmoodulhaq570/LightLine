use super::*;
use lightline::run_config::Configurations;

impl App {
    pub(super) fn run_choice_rect(&self, hwnd: HWND) -> Option<RECT> {
        if self.welcome || self.run_configuration_root().is_none() {
            return None;
        }
        let button = self.file_action_rect(hwnd);
        let right = button.left - self.scale(8);
        let left = right - self.scale(166);
        let tabs = self
            .tabs
            .len()
            .saturating_sub(self.tab_first)
            .min(self.visible_tab_count(hwnd));
        (left > self.editor_left() + tabs as i32 * self.scale(TAB_WIDTH) + self.scale(8)).then_some(
            RECT {
                left,
                right,
                top: button.top + self.scale(5),
                bottom: button.bottom - self.scale(5),
            },
        )
    }

    fn run_choice_rows(&self, hwnd: HWND) -> Vec<RECT> {
        let Some((saved, _, first)) = &self.run_choice else {
            return Vec::new();
        };
        let Some(button) = self.run_choice_rect(hwnd) else {
            return Vec::new();
        };
        let mut client = RECT::default();
        unsafe {
            GetClientRect(hwnd, &mut client);
        }
        let row = self.scale(32).max(1);
        let count = ((client.bottom - button.bottom - self.scale(28)) / row).clamp(1, 8) as usize;
        let width = self.scale(260).min(client.right - self.scale(16));
        let left = (button.right - width).max(self.scale(8));
        (0..count.min(saved.configurations.len() + 2 - first))
            .map(|i| RECT {
                left,
                right: left + width,
                top: button.bottom + self.scale(6) + i as i32 * row,
                bottom: button.bottom + self.scale(6) + (i + 1) as i32 * row,
            })
            .collect()
    }

    pub(super) fn run_choice_click(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        if let Some(rect) = self.run_choice_rect(hwnd)
            && contains(rect, x, y)
        {
            if self.run_choice.is_some() {
                self.run_choice = None;
            } else if let Some(root) = self.run_configuration_root() {
                match Configurations::load(&root) {
                    Ok(saved) => {
                        let selected = saved
                            .selected
                            .as_ref()
                            .and_then(|name| {
                                saved.configurations.iter().position(|c| &c.name == name)
                            })
                            .map_or(0, |i| i + 1);
                        self.run_choice = Some((saved, selected, selected.saturating_sub(5)));
                        self.quick_open = false;
                        self.terminal_focus = false;
                    }
                    Err(error) => self.error(hwnd, &error),
                }
            }
            self.refresh(hwnd);
            return true;
        }
        if self.run_choice.is_some() {
            let hit = self
                .run_choice_rows(hwnd)
                .iter()
                .position(|r| contains(*r, x, y));
            if let Some(index) = hit {
                let first = self.run_choice.as_ref().unwrap().2;
                self.select_run_choice(hwnd, index + first);
            } else {
                self.run_choice = None;
                self.refresh(hwnd);
            }
            return true;
        }
        false
    }

    fn select_run_choice(&mut self, hwnd: HWND, index: usize) {
        let Some((mut saved, _, _)) = self.run_choice.take() else {
            return;
        };
        if index == saved.configurations.len() + 1 {
            self.show_run_configurations(hwnd);
            return;
        }
        saved.selected = index
            .checked_sub(1)
            .and_then(|i| saved.configurations.get(i))
            .map(|c| c.name.clone());
        if let Some(root) = self.run_configuration_root() {
            match saved.save(&root) {
                Ok(()) => {
                    self.status = format!(
                        "Run configuration: {}",
                        saved.selected.as_deref().unwrap_or("Automatic")
                    )
                }
                Err(error) => self.error(hwnd, &error),
            }
        }
        self.refresh(hwnd);
    }

    pub(super) fn run_choice_key(&mut self, hwnd: HWND, key: u32) -> bool {
        let Some((saved, selected, first)) = &mut self.run_choice else {
            return false;
        };
        if key == VK_ESCAPE as u32 {
            self.run_choice = None;
        } else if key == VK_RETURN as u32 {
            let index = *selected;
            self.select_run_choice(hwnd, index);
            return true;
        } else {
            if key == VK_UP as u32 {
                *selected = selected.saturating_sub(1);
            }
            if key == VK_DOWN as u32 {
                *selected = (*selected + 1).min(saved.configurations.len() + 1);
            }
            if key == VK_HOME as u32 {
                *selected = 0;
            }
            if key == VK_END as u32 {
                *selected = saved.configurations.len() + 1;
            }
            if *selected < *first {
                *first = *selected;
            }
            if *selected >= *first + 6 {
                *first = selected.saturating_sub(5);
            }
        }
        self.refresh(hwnd);
        true
    }

    pub(super) fn paint_run_choice(&self, hwnd: HWND, hdc: HDC) {
        let Some(button) = self.run_choice_rect(hwnd) else {
            return;
        };
        let saved = self
            .run_configuration_root()
            .and_then(|root| Configurations::load(&root).ok());
        let name = saved
            .as_ref()
            .and_then(|s| s.selected.as_deref())
            .unwrap_or("Automatic");
        self.panel_card(
            hdc,
            button,
            self.scale(5),
            self.theme.edge,
            self.theme.editor_bg,
        );
        self.label_mid(
            hdc,
            name,
            button.left + self.scale(9),
            (button.top + button.bottom) / 2,
            self.theme.text,
            RECT {
                right: button.right - self.scale(24),
                ..button
            },
        );
        self.label_mid(
            hdc,
            "⌄",
            button.right - self.scale(18),
            (button.top + button.bottom) / 2,
            self.theme.muted,
            button,
        );
        if let Some((saved, selected, first)) = &self.run_choice {
            for (row, rect) in self.run_choice_rows(hwnd).into_iter().enumerate() {
                let index = row + first;
                Self::fill(
                    hdc,
                    rect,
                    if index == *selected {
                        self.theme.select_bg
                    } else {
                        self.theme.sidebar_bg
                    },
                );
                let label = if index == 0 {
                    "Automatic"
                } else if index == saved.configurations.len() + 1 {
                    "Configure..."
                } else {
                    &saved.configurations[index - 1].name
                };
                self.label_mid(
                    hdc,
                    label,
                    rect.left + self.scale(10),
                    (rect.top + rect.bottom) / 2,
                    self.theme.text,
                    rect,
                );
            }
        }
    }

    pub(super) fn show_run_error(&mut self, hwnd: HWND, error: String) {
        self.error(hwnd,&format!("Could not run the program\n\n{error}\n\nAfter installing a tool, add it to PATH and restart LightLine. For a custom executable, set its full path in Run Configurations."));
    }
}

fn contains(r: RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}
