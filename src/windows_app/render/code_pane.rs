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
        if let Some(image) = &self.tabs[self.tab_for_pane(pane)].image {
            self.paint_image_pane(
                hdc,
                image,
                RECT {
                    left,
                    top: self.editor_top(),
                    right,
                    bottom,
                },
            );
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
            let git_added_brush = CreateSolidBrush(GREEN);
            let git_mod_brush = CreateSolidBrush(BLUE);
            let git_diff = doc.path.as_ref().and_then(|p| self.git_diff_cache.get(p));
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
                if let Some((added, modified)) = git_diff {
                    let gutter_edge = left + self.scale(GUTTER) - self.scale(3);
                    if added.contains(&index) {
                        FillRect(
                            hdc,
                            &RECT {
                                left: gutter_edge,
                                top: y,
                                right: gutter_edge + self.scale(3),
                                bottom: (y + self.line_height).min(bottom),
                            },
                            git_added_brush,
                        );
                    } else if modified.contains(&index) {
                        FillRect(
                            hdc,
                            &RECT {
                                left: gutter_edge,
                                top: y,
                                right: gutter_edge + self.scale(3),
                                bottom: (y + self.line_height).min(bottom),
                            },
                            git_mod_brush,
                        );
                    }
                }
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
                            Color::Function => rgb(220, 210, 130),
                            Color::Operator => rgb(200, 200, 220),
                            Color::Attribute => rgb(180, 140, 230),
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
                for diagnostic in tab
                    .diagnostics
                    .iter()
                    .filter(|item| item.range.start.line as usize == index)
                    .take(3)
                {
                    let color = if diagnostic.severity == 1 {
                        rgb(246, 110, 120)
                    } else {
                        rgb(245, 184, 95)
                    };
                    let start_byte = lsp::utf16_to_byte(source, diagnostic.range.start.character);
                    let end_byte = if diagnostic.range.end.line as usize == index {
                        lsp::utf16_to_byte(source, diagnostic.range.end.character)
                    } else {
                        source.len()
                    };
                    let x1 = code_left + self.text_width(hdc, &source[..start_byte]);
                    let x2 = code_left + self.text_width(hdc, &source[..end_byte.max(start_byte)]);
                    Self::fill(
                        hdc,
                        RECT {
                            left: left + self.scale(48),
                            top: y + self.line_height / 2 - self.scale(3),
                            right: left + self.scale(54),
                            bottom: y + self.line_height / 2 + self.scale(3),
                        },
                        color,
                    );
                    if x1 < right {
                        Self::fill(
                            hdc,
                            RECT {
                                left: x1,
                                top: (y + self.line_height - self.scale(2)).min(bottom),
                                right: x2.max(x1 + self.scale(6)).min(right),
                                bottom: (y + self.line_height).min(bottom),
                            },
                            color,
                        );
                    }
                }
            }
            DeleteObject(guide_brush);
            DeleteObject(git_added_brush);
            DeleteObject(git_mod_brush);
            // Bracket matching: highlight the matching bracket pair.
            if pane == self.focused_pane && !self.terminal_focus {
                let match_brush = CreateSolidBrush(rgb(60, 80, 120));
                let bracket_at = |byte: usize, line_idx: usize| -> Option<(char, usize)> {
                    let text = doc.line(line_idx);
                    let ch = text[byte..].chars().next()?;
                    if matches!(ch, '(' | ')' | '[' | ']' | '{' | '}') {
                        Some((ch, byte))
                    } else {
                        None
                    }
                };
                let cursor_line = view.cursor.line;
                let cursor_byte = view.cursor.byte;
                // Check the character at the cursor, then the one before it.
                let bracket = bracket_at(cursor_byte, cursor_line).or_else(|| {
                    if cursor_byte > 0 {
                        let text = doc.line(cursor_line);
                        let prev_byte = text[..cursor_byte]
                            .char_indices()
                            .last()
                            .map(|(i, _)| i)?;
                        bracket_at(prev_byte, cursor_line)
                    } else {
                        None
                    }
                });
                if let Some((ch, byte)) = bracket {
                    let (open, close, forward) = match ch {
                        '(' => ('(', ')', true),
                        ')' => ('(', ')', false),
                        '[' => ('[', ']', true),
                        ']' => ('[', ']', false),
                        '{' => ('{', '}', true),
                        '}' => ('{', '}', false),
                        _ => ('(', ')', true),
                    };
                    // Scan for the matching bracket, tracking nesting depth.
                    let mut depth: i32 = 0;
                    let mut match_pos: Option<(usize, usize)> = None;
                    if forward {
                        let mut scan_line = cursor_line;
                        let mut scan_start = byte;
                        'outer_fwd: while scan_line < doc.line_count() && scan_line < cursor_line + 500 {
                            let text = doc.line(scan_line);
                            for (i, c) in text[scan_start..].char_indices() {
                                let abs = scan_start + i;
                                if c == open { depth += 1; }
                                if c == close { depth -= 1; }
                                if depth == 0 {
                                    match_pos = Some((scan_line, abs));
                                    break 'outer_fwd;
                                }
                            }
                            scan_line += 1;
                            scan_start = 0;
                        }
                    } else {
                        let mut scan_line = cursor_line;
                        let mut first = true;
                        'outer_bwd: loop {
                            let text = doc.line(scan_line);
                            let end = if first { byte } else { text.len() };
                            first = false;
                            let indices: Vec<(usize, char)> = text[..end].char_indices().collect();
                            for &(i, c) in indices.iter().rev() {
                                if c == close { depth += 1; }
                                if c == open { depth -= 1; }
                                if depth == 0 {
                                    match_pos = Some((scan_line, i));
                                    break 'outer_bwd;
                                }
                            }
                            if scan_line == 0 || cursor_line - scan_line > 500 { break; }
                            scan_line -= 1;
                        }
                    }
                    // Draw the highlight rectangles for both brackets.
                    let draw_bracket_bg = |line_idx: usize, b: usize| {
                        if line_idx < view.first_line { return; }
                        let row = line_idx - view.first_line;
                        let y = self.editor_top() + row as i32 * self.line_height;
                        if y >= bottom { return; }
                        let text = doc.line(line_idx);
                        let x = code_left + self.text_width(hdc, &text[..b]);
                        let ch_text = &text[b..text.len().min(b + 1)];
                        let w = self.text_width(hdc, if ch_text.is_empty() { " " } else { ch_text });
                        if x < right {
                            FillRect(hdc, &RECT {
                                left: x, top: y,
                                right: (x + w).min(right),
                                bottom: (y + self.line_height).min(bottom),
                            }, match_brush);
                        }
                    };
                    draw_bracket_bg(cursor_line, byte);
                    if let Some((ml, mb)) = match_pos {
                        draw_bracket_bg(ml, mb);
                    }
                }
                DeleteObject(match_brush);
            }
            if self.focused && self.caret_on && !self.terminal_focus && pane == self.focused_pane {
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
