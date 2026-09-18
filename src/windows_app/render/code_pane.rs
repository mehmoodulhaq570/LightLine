use super::super::*;

impl App {
    pub(in crate::windows_app) fn paint_code_pane(
        &self,
        hdc: HDC,
        hwnd: HWND,
        pane: usize,
        bounds: RECT,
        selection_bg: HBRUSH,
    ) {
        let (left, right, bottom) = (bounds.left, bounds.right, bounds.bottom);
        if right <= left {
            return;
        }
        unsafe {
            let saved = SaveDC(hdc);
            IntersectClipRect(hdc, left, self.editor_top(), right, bottom);
            let tab = &self.tabs[self.tab_for_pane(pane)];
            let doc = &tab.document;
            let view = self.view_for_pane(pane);
            let code_left = left + self.scale(GUTTER + PAD);
            let selection = view.selection_anchor.and_then(|anchor| {
                if anchor == view.cursor {
                    None
                } else if anchor < view.cursor {
                    Some((anchor, view.cursor))
                } else {
                    Some((view.cursor, anchor))
                }
            });
            let visible = self.visible_lines(hwnd) + 1;
            SelectObject(hdc, self.font);
            let space_width = self.text_width(hdc, " ").max(1);
            let guide_brush = CreateSolidBrush(EDGE);
            for row in 0..visible {
                let index = view.first_line + row;
                if index >= doc.line_count() {
                    break;
                }
                let y = self.editor_top() + row as i32 * self.line_height;
                if y >= bottom {
                    break;
                }
                if index == view.cursor.line {
                    Self::fill(
                        hdc,
                        RECT {
                            left,
                            top: y,
                            right,
                            bottom: (y + self.line_height).min(bottom),
                        },
                        LINE_BG,
                    );
                }
                let number = format!("{}", index + 1);
                let num: Vec<u16> = number.encode_utf16().collect();
                SetTextColor(
                    hdc,
                    if index == view.cursor.line {
                        TEXT
                    } else {
                        MUTED
                    },
                );
                let number_clip = RECT {
                    left,
                    top: y,
                    right: left + self.scale(GUTTER),
                    bottom,
                };
                ExtTextOutW(
                    hdc,
                    left + self.scale(12),
                    y,
                    ETO_CLIPPED,
                    &number_clip,
                    num.as_ptr(),
                    num.len() as u32,
                    null(),
                );
                let source = doc.line(index);
                let indent_columns = source
                    .chars()
                    .take_while(|ch| *ch == ' ' || *ch == '\t')
                    .take(64)
                    .map(|ch| if ch == '\t' { 4 } else { 1 })
                    .sum::<usize>();
                for level in 1..=(indent_columns / 4).min(8) {
                    let guide_x = code_left + level as i32 * 4 * space_width - self.scale(4);
                    if guide_x < right {
                        FillRect(
                            hdc,
                            &RECT {
                                left: guide_x,
                                top: y,
                                right: guide_x + 1,
                                bottom: (y + self.line_height).min(bottom),
                            },
                            guide_brush,
                        );
                    }
                }
                if let Some((start, end)) = selection
                    && index >= start.line
                    && index <= end.line
                    && !(index == end.line && end.byte == 0)
                {
                    let from = if index == start.line { start.byte } else { 0 };
                    let to = if index == end.line {
                        end.byte
                    } else {
                        source.len()
                    };
                    let x1 = code_left + self.text_width(hdc, &source[..from]);
                    let x2 = code_left
                        + self.text_width(hdc, &source[..to])
                        + if index < end.line { self.scale(8) } else { 0 };
                    if x2 > x1 && x1 < right {
                        FillRect(
                            hdc,
                            &RECT {
                                left: x1,
                                top: y,
                                right: x2.min(right),
                                bottom: (y + self.line_height).min(bottom),
                            },
                            selection_bg,
                        );
                    }
                }
                let line = source.replace('\t', "    ");
                let chars: Vec<u16> = line.encode_utf16().collect();
                SetTextColor(hdc, TEXT);
                let clip = RECT {
                    left: code_left,
                    top: y,
                    right,
                    bottom,
                };
                ExtTextOutW(
                    hdc,
                    code_left,
                    y,
                    ETO_CLIPPED,
                    &clip,
                    chars.as_ptr(),
                    chars.len() as u32,
                    null(),
                );
                if source.len() <= 16_384
                    && let Some(syntax) = &tab.syntax
                {
                    for span in syntax.spans(doc, index) {
                        let color = match span.color {
                            Color::Comment => MUTED,
                            Color::String => GREEN,
                            Color::Keyword => BLUE,
                            Color::Type => TEAL,
                            Color::Number => rgb(248, 180, 130),
                            Color::Macro => VIOLET,
                        };
                        SetTextColor(hdc, color);
                        let left = code_left + self.text_width(hdc, &source[..span.start]);
                        let text = source[span.start..span.end].replace('\t', "    ");
                        let chars: Vec<u16> = text.encode_utf16().collect();
                        ExtTextOutW(
                            hdc,
                            left,
                            y,
                            ETO_CLIPPED,
                            &clip,
                            chars.as_ptr(),
                            chars.len() as u32,
                            null(),
                        );
                    }
                }
            }
            DeleteObject(guide_brush);
            if self.focused && self.caret_on && pane == self.focused_pane {
                let line = doc.line(view.cursor.line);
                let x = code_left + self.text_width(hdc, &line[..view.cursor.byte]);
                let y = self.editor_top()
                    + (view.cursor.line as i64 - view.first_line as i64) as i32 * self.line_height;
                if y >= self.editor_top() && y < bottom && x < right {
                    let caret = CreateSolidBrush(BLUE);
                    FillRect(
                        hdc,
                        &RECT {
                            left: x,
                            top: y,
                            right: x + self.scale(2).max(2),
                            bottom: (y + self.line_height).min(bottom),
                        },
                        caret,
                    );
                    DeleteObject(caret);
                }
            }
            RestoreDC(hdc, saved);
        }
    }
}
