use super::super::*;
use super::*;

// `amount` (0..=1) of `over` mixed into `base`; both are COLORREFs.
fn blend(base: u32, over: u32, amount: f32) -> u32 {
    let channel = |shift: u32| {
        let from = ((base >> shift) & 0xff) as f32;
        let to = ((over >> shift) & 0xff) as f32;
        ((from + (to - from) * amount).round() as u32) << shift
    };
    channel(0) | channel(8) | channel(16)
}

impl App {
    // What a pane shows when no file is open: how to open one, in place of
    // the stand-in document's empty line 1.
    fn paint_empty_pane(&self, hdc: HDC, bounds: RECT) {
        let lines = [
            ("No file is open", self.brand_font, self.theme.text),
            (
                "Choose one in the Explorer, or press Ctrl+P to find a file",
                self.ui_font,
                self.theme.muted,
            ),
            ("Ctrl+N starts a new file", self.ui_font, self.theme.muted),
        ];
        let row = self.scale(28);
        let top =
            bounds.top + ((bounds.bottom - bounds.top) - row * lines.len() as i32).max(0) * 2 / 5;
        unsafe {
            let saved = SaveDC(hdc);
            for (index, (text, font, color)) in lines.into_iter().enumerate() {
                SelectObject(hdc, font);
                let width = self.text_width(hdc, text);
                let x = (bounds.left + (bounds.right - bounds.left - width) / 2).max(bounds.left);
                Self::label(hdc, text, x, top + row * index as i32, color, bounds);
            }
            RestoreDC(hdc, saved);
        }
    }

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
        if let Some(preview) = &self.tabs[self.tab_for_pane(pane)].markdown {
            self.paint_markdown_pane(
                hdc,
                preview,
                RECT {
                    left,
                    top: self.editor_top(),
                    right,
                    bottom,
                },
            );
            return;
        }
        if self.tabs[self.tab_for_pane(pane)].is_placeholder() {
            self.paint_empty_pane(
                hdc,
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
            let ordered = |cursor: Pos, anchor: Option<Pos>| {
                anchor.and_then(|anchor| {
                    if anchor == cursor {
                        None
                    } else if anchor < cursor {
                        Some((anchor, cursor))
                    } else {
                        Some((cursor, anchor))
                    }
                })
            };
            // The main caret's selection, and those of any other carets.
            let selections: Vec<(Pos, Pos)> = ordered(view.cursor, view.selection_anchor)
                .into_iter()
                .chain(
                    view.extra
                        .iter()
                        .filter_map(|caret| ordered(caret.cursor, caret.anchor)),
                )
                .collect();
            let visible = self.visible_lines(hwnd) + 1;
            SelectObject(hdc, self.font);
            let space_width = self.text_width(hdc, " ").max(1);
            let guide_brush = CreateSolidBrush(self.theme.edge);
            let git_added_brush = CreateSolidBrush(self.theme.green);
            let git_mod_brush = CreateSolidBrush(self.theme.blue);
            let git_del_brush = CreateSolidBrush(self.theme.error);
            let git_diff = doc.path.as_ref().and_then(|p| self.git_diff_cache.get(p));
            // The line a paused debug session stopped on in this file, marked
            // like VS Code: a yellow arrow in the breakpoint column and a
            // tinted line.
            let paused_line = if self.debug_paused() {
                self.debug_state.frames.first().and_then(|frame| {
                    let same_file = Self::same_path(frame.path.as_deref()?, doc.path.as_deref()?);
                    same_file.then(|| frame.line.saturating_sub(1) as usize)
                })
            } else {
                None
            };
            let paused_bg = blend(self.theme.editor_bg, ui(255, 204, 0), 0.14);
            // Screen rows from the top of the view. With word wrap a line
            // takes several: its gutter (number, breakpoint, fold) goes on
            // the first, and its text continues on the rest.
            let columns = self.wrap_columns(hwnd, pane);
            let (mut index, mut first_row) = self.view_top(hwnd, pane);
            let mut screen_row = 0usize;
            // A quiet boundary makes the three gutter lanes read as one
            // compact control without visually merging into the source text.
            Self::fill(
                hdc,
                RECT {
                    left: left + self.scale(GUTTER) - self.scale(1).max(1),
                    top: self.editor_top(),
                    right: left + self.scale(GUTTER),
                    bottom,
                },
                self.theme.edge,
            );
            let hint_bg = blend(self.theme.editor_bg, self.theme.text, 0.07);
            'lines: while screen_row < visible {
                let source = doc.line(index);
                let line_hints = self.hints_on_line(self.tab_for_pane(pane), index);
                let rows = super::super::wrap::layout_line(source, columns, self.settings.tab_size);
                // Lines and rows outside the area being redrawn are skipped:
                // the backbuffer still shows them. Dragging a selection
                // redraws only the rows it changes.
                let line_top = self.editor_top() + screen_row as i32 * self.line_height;
                let line_shows = RectVisible(
                    hdc,
                    &RECT {
                        left,
                        top: line_top,
                        right,
                        bottom: line_top
                            + (rows.count() - first_row).max(1) as i32 * self.line_height,
                    },
                ) != 0;
                let spans = match &tab.syntax {
                    Some(syntax) if line_shows && source.len() <= 16_384 => {
                        syntax.spans(doc, index)
                    }
                    _ => Vec::new(),
                };
                // The selected byte ranges of this line, and whether each
                // selection carries on past its end.
                let line_selections: Vec<(usize, usize, bool)> = selections
                    .iter()
                    .filter(|(start, end)| {
                        index >= start.line
                            && index <= end.line
                            && !(index == end.line && end.byte == 0)
                    })
                    .map(|(start, end)| {
                        (
                            if index == start.line { start.byte } else { 0 },
                            if index == end.line {
                                end.byte
                            } else {
                                source.len()
                            },
                            index < end.line,
                        )
                    })
                    .collect();
                let paused_here = paused_line == Some(index);
                for row in first_row..rows.count() {
                    let y = self.editor_top() + screen_row as i32 * self.line_height;
                    if screen_row >= visible || y >= bottom {
                        break 'lines;
                    }
                    screen_row += 1;
                    if !line_shows
                        || RectVisible(
                            hdc,
                            &RECT {
                                left,
                                top: y,
                                right,
                                bottom: y + self.line_height,
                            },
                        ) == 0
                    {
                        continue;
                    }
                    let row_start = rows.start(row);
                    let row_end = rows.end(row, source.len());
                    let last_row = rows.is_last(row);
                    let text_left = code_left + rows.indent(row, self.char_width);
                    // This row's inlay hints: at its end only on the line's
                    // last row, since the next row starts there.
                    let row_hints: Vec<&InlayHintAt> = line_hints
                        .iter()
                        .filter(|hint| {
                            hint.byte >= row_start
                                && (hint.byte < row_end || (last_row && hint.byte == row_end))
                        })
                        .collect();
                    let hint_widths: Vec<i32> = row_hints
                        .iter()
                        .map(|hint| self.text_width(hdc, &hint.label))
                        .collect();
                    let hints_before = |byte: usize, at: bool| -> i32 {
                        row_hints
                            .iter()
                            .zip(&hint_widths)
                            .filter(|(hint, _)| hint.byte < byte || (at && hint.byte == byte))
                            .map(|(_, width)| width)
                            .sum()
                    };
                    // Where `byte` of this row is drawn: before any hint at
                    // it (the caret, the end of a range)...
                    let x_of = |byte: usize| {
                        text_left
                            + self.text_width(hdc, safe_slice_range(source, row_start, byte))
                            + hints_before(byte, false)
                    };
                    // ...and after it (text from `byte`).
                    let x_after = |byte: usize| {
                        text_left
                            + self.text_width(hdc, safe_slice_range(source, row_start, byte))
                            + hints_before(byte, true)
                    };
                    // Draws `from..to` of this row in `color`, in pieces
                    // that leave room for the hints inside it.
                    let draw_text = |from: usize, to: usize, color: u32, clip: &RECT| {
                        SetTextColor(hdc, color);
                        let mut piece_start = from;
                        let cuts = row_hints
                            .iter()
                            .map(|hint| hint.byte)
                            .filter(|&byte| byte > from && byte < to)
                            .chain(std::iter::once(to));
                        for cut in cuts {
                            if cut <= piece_start {
                                continue;
                            }
                            let text = safe_slice_range(source, piece_start, cut)
                                .replace('\t', &" ".repeat(self.settings.tab_size));
                            let chars: Vec<u16> = text.encode_utf16().collect();
                            ExtTextOutW(
                                hdc,
                                x_after(piece_start),
                                y,
                                ETO_CLIPPED,
                                clip,
                                chars.as_ptr(),
                                chars.len() as u32,
                                null(),
                            );
                            piece_start = cut;
                        }
                    };
                    let row_bottom = (y + self.line_height).min(bottom);
                    if paused_here || index == view.cursor.line {
                        let line_bg = if paused_here {
                            paused_bg
                        } else {
                            self.theme.line_bg
                        };
                        Self::fill(
                            hdc,
                            RECT {
                                left,
                                top: y,
                                right,
                                bottom: row_bottom,
                            },
                            line_bg,
                        );
                    }
                    if row == 0 {
                        let number = format!("{}", index + 1);
                        let num: Vec<u16> = number.encode_utf16().collect();
                        SetTextColor(
                            hdc,
                            if index == view.cursor.line {
                                self.theme.line_number_active
                            } else {
                                self.theme.line_number
                            },
                        );
                        // The number lane is last and right-aligned, after the
                        // folding and breakpoint controls.
                        let number_right =
                            left + self.scale(GUTTER) - self.scale(GUTTER_NUMBER_RIGHT_INSET);
                        let number_clip = RECT {
                            left,
                            top: y,
                            right: number_right,
                            bottom,
                        };
                        ExtTextOutW(
                            hdc,
                            number_right - self.text_width(hdc, &number),
                            y,
                            ETO_CLIPPED,
                            &number_clip,
                            num.as_ptr(),
                            num.len() as u32,
                            null(),
                        );
                        // Breakpoint dots use the anti-aliased SVG path so
                        // their small circles stay clean at every DPI scale.
                        let marker_x = left + self.scale(GUTTER_BREAKPOINT_CENTER);
                        let marker_y = y + self.line_height / 2;
                        let breakpoint = |size: i32, shift: i32| {
                            let center_x = marker_x + shift;
                            self.icons.draw_glyph(
                                hdc,
                                DebugGlyph::GutterDot,
                                ui(246, 110, 120),
                                center_x - size / 2,
                                marker_y - size / 2,
                                size,
                            );
                        };
                        if paused_here {
                            let yellow = ui(255, 204, 0);
                            let size = self.scale(18);
                            self.icons.draw_glyph(
                                hdc,
                                DebugGlyph::ExecutionArrow,
                                yellow,
                                marker_x - size / 2,
                                marker_y - size / 2,
                                size,
                            );
                            if doc.has_breakpoint(index) {
                                breakpoint(self.scale(10), -self.scale(3));
                            }
                        } else if doc.has_breakpoint(index) {
                            breakpoint(self.scale(14), 0);
                        }
                        // Folding is the first lane, separate from the
                        // breakpoint target and the line number.
                        let is_folded = doc.is_folded_start(index).is_some();
                        let is_foldable = is_folded || doc.foldable_range(index).is_some();
                        if is_foldable {
                            // Drawn as vector strokes, like the Explorer's
                            // chevrons: the editor font may have no glyph for
                            // ⌄ or ›, which rendered as an empty box.
                            self.chevron(
                                hdc,
                                left + self.scale(GUTTER_FOLD_CENTER),
                                y + self.line_height / 2,
                                !is_folded,
                            );
                        }
                        let indent_columns = source
                            .chars()
                            .take_while(|ch| *ch == ' ' || *ch == '\t')
                            .take(64)
                            .map(|ch| if ch == '\t' { 4 } else { 1 })
                            .sum::<usize>();
                        let guides = if self.settings.indent_guides { 8 } else { 0 };
                        for level in 1..=(indent_columns / 4).min(guides) {
                            let guide_x =
                                code_left + level as i32 * 4 * space_width - self.scale(4);
                            if guide_x < right {
                                FillRect(
                                    hdc,
                                    &RECT {
                                        left: guide_x,
                                        top: y,
                                        right: guide_x + 1,
                                        bottom: row_bottom,
                                    },
                                    guide_brush,
                                );
                            }
                        }
                    }
                    if let Some(diff) = git_diff {
                        let gutter_edge = left + self.scale(GUTTER) - self.scale(3);
                        let bar = if diff.added.contains(&index) {
                            Some(git_added_brush)
                        } else if diff.modified.contains(&index) {
                            Some(git_mod_brush)
                        } else {
                            None
                        };
                        // The bar runs down every row of a wrapped line.
                        if let Some(brush) = bar {
                            FillRect(
                                hdc,
                                &RECT {
                                    left: gutter_edge,
                                    top: y,
                                    right: gutter_edge + self.scale(3),
                                    bottom: row_bottom,
                                },
                                brush,
                            );
                        }
                        if row == 0 && diff.deleted.contains(&index) {
                            let pts = [
                                POINT { x: gutter_edge, y },
                                POINT {
                                    x: gutter_edge + self.scale(3),
                                    y: y + self.scale(3),
                                },
                                POINT {
                                    x: gutter_edge,
                                    y: y + self.scale(6),
                                },
                            ];
                            let old_brush = SelectObject(hdc, git_del_brush);
                            let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
                            Polygon(hdc, pts.as_ptr(), 3);
                            SelectObject(hdc, old_pen);
                            SelectObject(hdc, old_brush);
                        }
                    }
                    for &(from, to, continues) in &line_selections {
                        let from = from.max(row_start);
                        let to = to.min(row_end);
                        let past_end = continues && last_row;
                        if from < to || (past_end && from <= to) {
                            let x1 = x_after(from);
                            let x2 = x_of(to) + if past_end { self.scale(8) } else { 0 };
                            if x2 > x1 && x1 < right {
                                FillRect(
                                    hdc,
                                    &RECT {
                                        left: x1,
                                        top: y,
                                        right: x2.min(right),
                                        bottom: row_bottom,
                                    },
                                    selection_bg,
                                );
                            }
                        }
                    }
                    let clip = RECT {
                        left: code_left,
                        top: y,
                        right,
                        bottom,
                    };
                    draw_text(row_start, row_end, self.theme.text, &clip);
                    // The hints themselves, muted on a faint background.
                    let mut drawn_at: Option<(usize, i32)> = None;
                    for (hint, width) in row_hints.iter().zip(&hint_widths) {
                        // Hints at one byte go side by side.
                        let x = match drawn_at {
                            Some((byte, end)) if byte == hint.byte => end,
                            _ => x_of(hint.byte),
                        };
                        if x < right {
                            Self::fill(
                                hdc,
                                RECT {
                                    left: x,
                                    top: y + self.scale(2),
                                    right: (x + width).min(right),
                                    bottom: (y + self.line_height - self.scale(2)).min(bottom),
                                },
                                hint_bg,
                            );
                            Self::label(hdc, &hint.label, x, y, self.theme.muted, clip);
                        }
                        drawn_at = Some((hint.byte, x + width));
                    }
                    if last_row && doc.is_folded_start(index).is_some() {
                        let pill_x = x_after(row_end) + self.scale(6);
                        let pill_w = self.scale(22);
                        let pill_h = self.scale(13);
                        let pill_y = y + (self.line_height - pill_h) / 2;
                        let pill_bg = CreateSolidBrush(self.theme.active_bg);
                        FillRect(
                            hdc,
                            &RECT {
                                left: pill_x,
                                top: pill_y,
                                right: pill_x + pill_w,
                                bottom: pill_y + pill_h,
                            },
                            pill_bg,
                        );
                        DeleteObject(pill_bg);
                        SetTextColor(hdc, self.theme.line_number_active);
                        let dots: Vec<u16> = "...".encode_utf16().collect();
                        ExtTextOutW(
                            hdc,
                            pill_x + self.scale(3),
                            pill_y - self.scale(1),
                            0,
                            null(),
                            dots.as_ptr(),
                            dots.len() as u32,
                            null(),
                        );
                    }
                    for span in &spans {
                        // The part of the span on this row.
                        let (from, to) = (span.start.max(row_start), span.end.min(row_end));
                        if from >= to {
                            continue;
                        }
                        // self.theme is the single resolved source for these
                        // -- Theme::with_overrides already folded in any
                        // settings.json "colors" override once, at startup,
                        // instead of re-checking a HashMap on every span of
                        // every repaint.
                        draw_text(from, to, self.theme.syntax(span.color), &clip);
                    }
                    for diagnostic in tab
                        .diagnostics
                        .iter()
                        .filter(|item| item.range.start.line as usize == index)
                        .take(3)
                    {
                        let color = if diagnostic.severity == 1 {
                            self.theme.error
                        } else {
                            self.theme.warning
                        };
                        let start_byte =
                            lsp::utf16_to_byte(source, diagnostic.range.start.character);
                        let end_byte = if diagnostic.range.end.line as usize == index {
                            lsp::utf16_to_byte(source, diagnostic.range.end.character)
                        } else {
                            source.len()
                        }
                        .max(start_byte);
                        if row == 0 {
                            // Diagnostics have their own lane and use the
                            // anti-aliased dot also used for breakpoints.
                            let center_x = left + self.scale(GUTTER_DIAGNOSTIC_CENTER);
                            let center_y = y + self.line_height / 2;
                            let size = self.scale(14);
                            self.icons.draw_glyph(
                                hdc,
                                DebugGlyph::GutterDot,
                                color,
                                center_x - size / 2,
                                center_y - size / 2,
                                size,
                            );
                        }
                        // Underlined on each row the problem reaches; an
                        // empty range gets a short mark on its own row.
                        let on_row = if end_byte > start_byte {
                            start_byte < row_end && end_byte > row_start
                        } else {
                            rows.row_of(start_byte) == row
                        };
                        if !on_row {
                            continue;
                        }
                        let x1 = x_after(start_byte.max(row_start));
                        let x2 = x_of(end_byte.min(row_end));
                        if x1 < right {
                            Self::fill(
                                hdc,
                                RECT {
                                    left: x1,
                                    top: (y + self.line_height - self.scale(2)).min(bottom),
                                    right: x2.max(x1 + self.scale(6)).min(right),
                                    bottom: row_bottom,
                                },
                                color,
                            );
                        }
                    }
                }
                let next = doc.next_visible_line(index);
                if next == index {
                    break;
                }
                index = next;
                first_row = 0;
            }
            DeleteObject(guide_brush);
            DeleteObject(git_added_brush);
            DeleteObject(git_mod_brush);
            DeleteObject(git_del_brush);
            // Bracket matching: highlight the matching bracket pair.
            if let Some((bracket, matching)) = self.bracket_pair(pane) {
                let match_brush = CreateSolidBrush(ui(60, 80, 120));
                // Draw the highlight rectangles for both brackets.
                let draw_bracket_bg = |line_idx: usize, b: usize| {
                    if doc.is_line_hidden(line_idx) {
                        return;
                    }
                    let at = Pos {
                        line: line_idx,
                        byte: b,
                    };
                    let Some((row, row_start, indent)) = self.locate(hwnd, pane, at, visible)
                    else {
                        return;
                    };
                    let y = self.editor_top() + row as i32 * self.line_height;
                    if y >= bottom {
                        return;
                    }
                    let text = doc.line(line_idx);
                    let hints = self.hints_on_line(self.tab_for_pane(pane), line_idx);
                    let x = code_left
                        + indent
                        + self.text_width(hdc, safe_slice_range(text, row_start, b))
                        + self.hint_shift(hdc, hints, row_start, b, true);
                    let rest = if b < text.len() && text.is_char_boundary(b) {
                        &text[b..]
                    } else {
                        ""
                    };
                    let ch_text = rest
                        .chars()
                        .next()
                        .map(|c| &rest[..c.len_utf8()])
                        .unwrap_or(" ");
                    let w = self.text_width(hdc, ch_text);
                    if x < right {
                        FillRect(
                            hdc,
                            &RECT {
                                left: x,
                                top: y,
                                right: (x + w).min(right),
                                bottom: (y + self.line_height).min(bottom),
                            },
                            match_brush,
                        );
                        // The fill above paints over the bracket glyph
                        // that the earlier syntax-color pass already
                        // drew, leaving a blank highlighted box instead
                        // of a highlighted character; redraw it on top.
                        SetTextColor(hdc, self.theme.text);
                        let chars: Vec<u16> = ch_text.encode_utf16().collect();
                        TextOutW(hdc, x, y, chars.as_ptr(), chars.len() as i32);
                    }
                };
                draw_bracket_bg(bracket.line, bracket.byte);
                if let Some(other) = matching {
                    draw_bracket_bg(other.line, other.byte);
                }
                DeleteObject(match_brush);
            }
            // Other carets (multiple cursors) don't blink: the caret-only
            // repaint reaches just the main one.
            if self.focused
                && !self.terminal_focus
                && !self.search_input
                && !self.panel_focus
                && pane == self.focused_pane
                && !view.extra.is_empty()
            {
                let caret_brush = CreateSolidBrush(self.theme.cursor);
                for caret in &view.extra {
                    let cursor = doc.clamp(caret.cursor);
                    if doc.is_line_hidden(cursor.line) {
                        continue;
                    }
                    let Some((row, row_start, indent)) = self.locate(hwnd, pane, cursor, visible)
                    else {
                        continue;
                    };
                    let line = doc.line(cursor.line);
                    let hints = self.hints_on_line(self.tab_for_pane(pane), cursor.line);
                    let x = code_left
                        + indent
                        + self.text_width(hdc, safe_slice_range(line, row_start, cursor.byte))
                        + self.hint_shift(hdc, hints, row_start, cursor.byte, false);
                    let y = self.editor_top() + row as i32 * self.line_height;
                    if y < bottom && x < right {
                        FillRect(
                            hdc,
                            &RECT {
                                left: x,
                                top: y,
                                right: x + self.scale(2).max(2),
                                bottom: (y + self.line_height).min(bottom),
                            },
                            caret_brush,
                        );
                    }
                }
                DeleteObject(caret_brush);
            }
            if self.focused
                && self.caret_on
                && !self.terminal_focus
                && !self.search_input
                && !self.panel_focus
                && pane == self.focused_pane
            {
                // Its screen row, not its document line: rows above it can be
                // wrapped (several per line) or folded (one per block).
                let cursor = doc.clamp(view.cursor);
                let line = doc.line(cursor.line);
                let located = (!doc.is_line_hidden(cursor.line))
                    .then(|| self.locate(hwnd, pane, cursor, visible))
                    .flatten();
                let (x, y) = match located {
                    Some((row, row_start, indent)) => (
                        code_left
                            + indent
                            + self.text_width(hdc, safe_slice_range(line, row_start, cursor.byte))
                            + self.hint_shift(
                                hdc,
                                self.hints_on_line(self.tab_for_pane(pane), cursor.line),
                                row_start,
                                cursor.byte,
                                false,
                            ),
                        self.editor_top() + row as i32 * self.line_height,
                    ),
                    None => (right, bottom),
                };
                if y < bottom && x < right {
                    let caret = CreateSolidBrush(self.theme.cursor);
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
                    if let Some(ghost) = &self.ghost_text {
                        let active_tab = self.tab_for_pane(pane);
                        if ghost.tab == active_tab && ghost.pos == cursor {
                            SetTextColor(hdc, self.theme.muted);
                            let first_line = ghost.text.lines().next().unwrap_or("");
                            let display = if ghost.text.contains('\n') {
                                format!("{first_line} ...")
                            } else {
                                first_line.to_string()
                            };
                            let ghost_x = x + self.scale(2).max(2);
                            if ghost_x < right && !display.is_empty() {
                                let chars: Vec<u16> = display.encode_utf16().collect();
                                TextOutW(hdc, ghost_x, y, chars.as_ptr(), chars.len() as i32);
                            }
                        }
                    }
                }
            }
            // The scrollbar goes over the text: a track with a thin edge, and
            // the slider, lighter under the mouse and lighter still held.
            if let Some((track, slider)) = self.scrollbar(hwnd, pane) {
                let edge = self.scale(1).max(1);
                Self::fill(hdc, track, self.theme.editor_bg);
                Self::fill(
                    hdc,
                    RECT {
                        right: track.left + edge,
                        ..track
                    },
                    self.theme.edge,
                );
                let strength = if self.scrollbar_grab.is_some() && pane == self.focused_pane {
                    0.45
                } else if self.scrollbar_hover == Some(pane) {
                    0.3
                } else {
                    0.2
                };
                Self::fill(
                    hdc,
                    RECT {
                        left: track.left + edge,
                        ..slider
                    },
                    blend(self.theme.editor_bg, self.theme.text, strength),
                );
            }
            RestoreDC(hdc, saved);
        }
    }

    /// The bracket at or just before `pane`'s caret and the one it matches
    /// (found within 500 lines), when brackets are highlighted there.
    pub(in crate::windows_app) fn bracket_pair(&self, pane: usize) -> Option<(Pos, Option<Pos>)> {
        if !self.settings.bracket_matching || pane != self.focused_pane || self.terminal_focus {
            return None;
        }
        let doc = &self.tabs[self.tab_for_pane(pane)].document;
        let view = self.view_for_pane(pane);
        let bracket_at = |byte: usize, line_idx: usize| -> Option<(char, usize)> {
            let text = doc.line(line_idx);
            if byte >= text.len() || !text.is_char_boundary(byte) {
                return None;
            }
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
                let prev_byte = safe_slice_prefix(text, cursor_byte)
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)?;
                bracket_at(prev_byte, cursor_line)
            } else {
                None
            }
        });
        let (ch, byte) = bracket?;
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
                let scan_text = if scan_start < text.len() && text.is_char_boundary(scan_start) {
                    &text[scan_start..]
                } else {
                    ""
                };
                for (i, c) in scan_text.char_indices() {
                    let abs = scan_start + i;
                    if c == open {
                        depth += 1;
                    }
                    if c == close {
                        depth -= 1;
                    }
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
                let bwd_text = safe_slice_prefix(text, end);
                let indices: Vec<(usize, char)> = bwd_text.char_indices().collect();
                for &(i, c) in indices.iter().rev() {
                    if c == close {
                        depth += 1;
                    }
                    if c == open {
                        depth -= 1;
                    }
                    if depth == 0 {
                        match_pos = Some((scan_line, i));
                        break 'outer_bwd;
                    }
                }
                if scan_line == 0 || cursor_line - scan_line > 500 {
                    break;
                }
                scan_line -= 1;
            }
        }
        Some((
            Pos {
                line: cursor_line,
                byte,
            },
            match_pos.map(|(line, byte)| Pos { line, byte }),
        ))
    }
}
