use super::super::*;

// A short letter/symbol tag per LSP CompletionItemKind so the popup reads like
// an IDE list without needing a full icon set. Unknown kinds fall back to a dot.
fn completion_glyph(kind: u8) -> &'static str {
    match kind {
        2..=4 => "f",                         // method, function, constructor
        5 | 10 => "#",                        // field, property
        6 => "v",                             // variable
        7 | 8 | 9 | 13 | 19 | 22 | 23 => "C", // class, interface, module, enum, folder, struct, type-param
        14 => "k",                            // keyword
        15 => "~",                            // snippet
        _ => "\u{2022}",                      // text and everything else
    }
}

impl App {
    pub(in crate::windows_app) fn paint_completion(
        &self,
        hdc: HDC,
        window: RECT,
        editor_bottom: i32,
    ) {
        let Some(popup) = &self.completion else {
            return;
        };
        let total = popup.items.len();
        if total == 0 {
            return;
        }
        let visible_rows = 9usize.min(total);
        let start = popup
            .selected
            .saturating_sub(visible_rows / 2)
            .min(total.saturating_sub(visible_rows));
        let end = (start + visible_rows).min(total);

        let row_height = self.scale(28);
        let padding = self.scale(6);
        let width = self
            .scale(380)
            .min((window.right - popup.x - self.scale(12)).max(self.scale(170)));
        let height = (end - start) as i32 * row_height + padding * 2;

        // Anchor under the caret, flipping above or nudging left when the list
        // would spill past the editor's bottom or right edge.
        let mut x = popup.x;
        if x + width > window.right - self.scale(8) {
            x = (window.right - self.scale(8) - width).max(self.editor_left());
        }
        let mut y = popup.y + self.scale(2);
        if y + height > editor_bottom {
            y = (popup.y - height - self.scale(20)).max(self.editor_top());
        }

        let outer = RECT {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        };
        Self::fill(hdc, outer, self.theme.edge);
        Self::fill(
            hdc,
            RECT {
                left: x + 1,
                top: y + 1,
                right: x + width - 1,
                bottom: y + height - 1,
            },
            ui(22, 33, 54),
        );

        for (row, index) in (start..end).enumerate() {
            let item = &popup.items[index];
            let top = y + padding + row as i32 * row_height;
            let text_y = top + self.scale(4);
            let selected_row = index == popup.selected;
            if selected_row {
                Self::fill(
                    hdc,
                    RECT {
                        left: x + 1,
                        top,
                        right: x + width - 1,
                        bottom: top + row_height,
                    },
                    self.theme.select_bg,
                );
            }
            let glyph_color = if selected_row {
                self.theme.text
            } else {
                self.theme.muted
            };
            // A path suggestion shows the Explorer's icon for the entry.
            let icon_path = popup
                .folder
                .as_ref()
                .filter(|_| {
                    matches!(
                        item.kind,
                        super::super::path_complete::FILE_KIND
                            | super::super::path_complete::FOLDER_KIND
                    )
                })
                .map(|folder| folder.join(&item.label));
            if let Some(path) = icon_path {
                let is_dir = item.kind == super::super::path_complete::FOLDER_KIND;
                let size = self.scale(16);
                let (icon_x, icon_y) = (x + padding, top + (row_height - size) / 2);
                let drawn = self.icons.draw_for_path(
                    hdc,
                    &path,
                    is_dir,
                    false,
                    self.has_extension("material-icons"),
                    icon_x,
                    icon_y,
                    size,
                );
                if !drawn && is_dir {
                    self.draw_vector_folder(hdc, icon_x, icon_y, size, false);
                } else if !drawn {
                    self.draw_vector_file(hdc, &path, icon_x, icon_y, size);
                }
            } else {
                Self::label(
                    hdc,
                    completion_glyph(item.kind),
                    x + padding,
                    text_y,
                    glyph_color,
                    outer,
                );
            }
            let text_x = x + padding + self.scale(20);
            let detail = item.detail.as_deref().unwrap_or("");
            let label_right = if detail.is_empty() {
                x + width - padding
            } else {
                (x + width - padding - self.scale(150)).max(text_x + self.scale(60))
            };
            self.label_ellipsis(
                hdc,
                &item.label,
                text_x,
                text_y,
                self.theme.text,
                RECT {
                    left: text_x,
                    top,
                    right: label_right,
                    bottom: top + row_height,
                },
            );
            if !detail.is_empty() {
                self.label_ellipsis(
                    hdc,
                    detail,
                    label_right + self.scale(6),
                    text_y,
                    self.theme.muted,
                    RECT {
                        left: label_right + self.scale(6),
                        top,
                        right: x + width - padding,
                        bottom: top + row_height,
                    },
                );
            }
        }
    }
}
