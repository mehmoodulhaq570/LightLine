use super::super::*;

impl App {
    pub(in crate::windows_app) fn text_width(&self, hdc: HDC, text: &str) -> i32 {
        let expanded = text.replace('\t', &" ".repeat(self.settings.tab_size));
        let utf16: Vec<u16> = expanded.encode_utf16().collect();
        let mut size = SIZE::default();
        unsafe {
            GetTextExtentPoint32W(hdc, utf16.as_ptr(), utf16.len() as i32, &mut size);
        }
        size.cx
    }

    // Height of the currently selected font's text, so rows can center their
    // label instead of relying on a hand-tuned offset per font size.
    pub(in crate::windows_app) fn text_height(&self, hdc: HDC) -> i32 {
        let sample: Vec<u16> = "Ag".encode_utf16().collect();
        let mut size = SIZE::default();
        unsafe {
            GetTextExtentPoint32W(hdc, sample.as_ptr(), sample.len() as i32, &mut size);
        }
        size.cy
    }

    // Draws `text` vertically centered on `center_y` rather than top-aligned.
    pub(in crate::windows_app) fn label_mid(
        &self,
        hdc: HDC,
        text: &str,
        x: i32,
        center_y: i32,
        color: u32,
        clip: RECT,
    ) {
        let top = center_y - self.text_height(hdc) / 2;
        Self::label(hdc, text, x, top, color, clip);
    }

    // A rounded card with a vertical two-stop gradient. GradientFill only
    // paints rectangles, so the rounded corners come from clipping to a
    // round-rect region for the duration of the fill.
    pub(in crate::windows_app) fn gradient_card(
        &self,
        hdc: HDC,
        rect: RECT,
        radius: i32,
        top_color: u32,
        bottom_color: u32,
    ) {
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return;
        }
        // COLORREF packs as 0x00BBGGRR; TRIVERTEX wants 16-bit channels.
        let channel = |color: u32, shift: u32| ((color >> shift) & 0xff) as u16 * 257;
        let vertices = [
            TRIVERTEX {
                x: rect.left,
                y: rect.top,
                Red: channel(top_color, 0),
                Green: channel(top_color, 8),
                Blue: channel(top_color, 16),
                Alpha: 0,
            },
            TRIVERTEX {
                x: rect.right,
                y: rect.bottom,
                Red: channel(bottom_color, 0),
                Green: channel(bottom_color, 8),
                Blue: channel(bottom_color, 16),
                Alpha: 0,
            },
        ];
        let mesh = GRADIENT_RECT {
            UpperLeft: 0,
            LowerRight: 1,
        };
        unsafe {
            let region = CreateRoundRectRgn(
                rect.left,
                rect.top,
                rect.right + 1,
                rect.bottom + 1,
                radius,
                radius,
            );
            if region.is_null() {
                Self::fill(hdc, rect, top_color);
                return;
            }
            // Within whatever clip is already set (the part of the window
            // being repainted, an enclosing card), and put back afterwards:
            // replacing the clip, then clearing it, left the rest of the
            // frame unclipped.
            let saved = SaveDC(hdc);
            ExtSelectClipRgn(hdc, region, RGN_AND);
            GradientFill(
                hdc,
                vertices.as_ptr(),
                vertices.len() as u32,
                std::ptr::from_ref(&mesh).cast(),
                1,
                GRADIENT_FILL_RECT_V,
            );
            RestoreDC(hdc, saved);
            DeleteObject(region);
        }
    }

    // Just the rounded outline of a card. Drawn after a card's contents so
    // interior fills, which are square, cannot paint over the border.
    pub(in crate::windows_app) fn card_outline(
        &self,
        hdc: HDC,
        rect: RECT,
        radius: i32,
        color: u32,
    ) {
        unsafe {
            let pen = CreatePen(PS_SOLID, self.scale(1).max(1), color);
            if pen.is_null() {
                return;
            }
            let previous_pen = SelectObject(hdc, pen);
            let previous_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
            RoundRect(
                hdc,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                radius,
                radius,
            );
            SelectObject(hdc, previous_brush);
            SelectObject(hdc, previous_pen);
            DeleteObject(pen);
        }
    }

    // A rounded panel: 1px border in `edge` with `body` filled inside it.
    pub(in crate::windows_app) fn panel_card(
        &self,
        hdc: HDC,
        rect: RECT,
        radius: i32,
        edge: u32,
        body: u32,
    ) {
        Self::rounded_fill(hdc, rect, radius, edge);
        Self::rounded_fill(
            hdc,
            RECT {
                left: rect.left + self.scale(1).max(1),
                top: rect.top + self.scale(1).max(1),
                right: rect.right - self.scale(1).max(1),
                bottom: rect.bottom - self.scale(1).max(1),
            },
            (radius - self.scale(1)).max(1),
            body,
        );
    }

    pub(in crate::windows_app) fn caret_rect(&self, hwnd: HWND) -> RECT {
        unsafe {
            let cursor = self.doc().clamp(self.view().cursor);
            let located = self.locate(
                hwnd,
                self.focused_pane,
                cursor,
                self.visible_lines(hwnd) + 1,
            );
            // Measured from the start of the caret's screen row.
            let (row, row_start, indent) = located.map_or((-1, 0, 0), |(row, start, indent)| {
                (row as i32, start, indent)
            });
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.font);
            let line = self.doc().line(cursor.line);
            let prefix = safe_slice_range(line, row_start, cursor.byte);
            let x = self.code_left(hwnd) + indent + self.text_width(hdc, prefix);
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            let y = self.editor_top() + row * self.line_height;
            RECT {
                left: x,
                top: y,
                right: x + self.scale(2).max(2),
                bottom: y + self.line_height,
            }
        }
    }

    pub(in crate::windows_app) fn invalidate_caret(&self, hwnd: HWND) {
        let rect = self.caret_rect(hwnd);
        unsafe {
            InvalidateRect(hwnd, &rect, 0);
        }
    }

    /// The code panes, and with `with_tabs` the tab strip and breadcrumbs
    /// above them; never a terminal below them.
    pub(in crate::windows_app) fn editor_area(&self, hwnd: HWND, with_tabs: bool) -> RECT {
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let editor_bottom = (client.bottom - self.scale(STATUS)).max(0);
        let terminal = if self.terminal_visible {
            self.scale(self.terminal_height)
        } else {
            0
        };
        RECT {
            left: self.editor_left(),
            top: if with_tabs {
                self.chrome_top()
            } else {
                self.editor_top()
            },
            right: self.editor_right(hwnd),
            bottom: editor_bottom - terminal,
        }
    }

    pub(in crate::windows_app) fn status_area(&self, hwnd: HWND) -> RECT {
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        RECT {
            top: client.bottom - self.scale(STATUS),
            ..client
        }
    }

    /// Whether a keystroke or click in the editor changes nothing outside the
    /// editor and the status bar. An open popup, hover card or menu can reach
    /// past them, and the assistant panel describes the selection.
    pub(in crate::windows_app) fn keystroke_stays_in_editor(&self) -> bool {
        !self.welcome
            && self.hover_card.is_none()
            && self.completion.is_none()
            && self.code_actions.is_none()
            && self.view().extra.is_empty()
            && self.editor_context.is_none()
            && self.more_menu.is_none()
            && !self.ai_assistant_visible
    }

    /// The focused pane's caret, selection and bracket highlight, and what
    /// else it shows, taken before a click, drag or key acts; see
    /// `caret_changes`.
    pub(in crate::windows_app) fn caret_frame(&self, hwnd: HWND) -> CaretFrame {
        let pane = self.focused_pane;
        let tab = &self.tabs[self.tab_for_pane(pane)];
        let doc = &tab.document;
        let view = self.view_for_pane(pane);
        let (start, end) = match view.selection_anchor {
            Some(anchor) if anchor < view.cursor => (anchor, view.cursor),
            Some(anchor) if anchor > view.cursor => (view.cursor, anchor),
            _ => (view.cursor, view.cursor),
        };
        CaretFrame {
            scene: CaretScene {
                pane,
                tab: self.tab_for_pane(pane),
                split: self.split_visible,
                text: tab.image.is_none() && tab.markdown.is_none() && !tab.is_placeholder(),
                top: self.view_top(hwnd, pane),
                serial: doc.change_serial(),
                lines: doc.line_count(),
                shown_lines: doc.visible_line_count(),
                breakpoints: doc.breakpoints().len(),
                find: self.find_mode,
                keyboard: [
                    self.focused,
                    self.panel_focus,
                    self.terminal_focus,
                    self.search_input,
                    self.commit_focus,
                    self.extensions_search_active,
                ],
                stays: self.keystroke_stays_in_editor(),
            },
            cursor: view.cursor,
            start,
            end,
            brackets: self.bracket_pair(pane),
            dirty: doc.is_dirty(),
        }
    }

    /// What to redraw after a click, drag or key that began at `before`. When
    /// only the caret, the selection or the bracket highlight moved, that's
    /// the rows of the lines they left or reached, and the status bar; each
    /// mouse move of a drag redrew the whole editor. Otherwise the editor and
    /// the status bar.
    pub(in crate::windows_app) fn caret_changes(
        &self,
        hwnd: HWND,
        before: &CaretFrame,
    ) -> Vec<RECT> {
        let after = self.caret_frame(hwnd);
        let everything = || vec![self.editor_area(hwnd, true), self.status_area(hwnd)];
        if !after.scene.stays || !after.scene.text {
            return everything();
        }
        let mut lines = vec![
            (before.cursor.line, before.cursor.line),
            (after.cursor.line, after.cursor.line),
        ];
        let mut scrollbar = false;
        if after.scene != before.scene {
            // Typing changes the text, but only on the lines it edited: each
            // keystroke used to redraw every line on screen.
            let Some(edited) = self.edited_lines(&before.scene, &after.scene) else {
                return everything();
            };
            lines.push(edited);
            scrollbar = after.scene.lines != before.scene.lines;
        }
        // A selection edge that moved changes the lines between its places.
        for (old, new) in [(before.start, after.start), (before.end, after.end)] {
            if old != new {
                lines.push((old.line.min(new.line), old.line.max(new.line)));
            }
        }
        for (bracket, matching) in before.brackets.iter().chain(&after.brackets) {
            lines.push((bracket.line, bracket.line));
            if let Some(other) = matching {
                lines.push((other.line, other.line));
            }
        }
        let mut areas = self.rows_showing(hwnd, self.focused_pane, &lines);
        areas.push(self.status_area(hwnd));
        if scrollbar && let Some((track, _)) = self.scrollbar(hwnd, self.focused_pane) {
            areas.push(track);
        }
        // The tab and the breadcrumb mark unsaved changes with `*`.
        if after.dirty != before.dirty {
            areas.push(RECT {
                bottom: self.editor_top(),
                ..self.editor_area(hwnd, true)
            });
        }
        areas
    }

    /// The lines (first, last) to redraw for an edit that took the focused
    /// pane from `before` to `after`, when that is all that changed: the
    /// edited lines, or from the first of them down when lines were added or
    /// removed or the text wraps, since everything below then moves. None
    /// when anything else changed too (a scroll, a fold, another tab).
    fn edited_lines(&self, before: &CaretScene, after: &CaretScene) -> Option<(usize, usize)> {
        let same_view = CaretScene {
            serial: before.serial,
            lines: before.lines,
            shown_lines: before.shown_lines,
            ..*after
        } == *before;
        let lines_added = after.lines as isize - before.lines as isize;
        // A fold opened or dropped by the edit changes more than the edit.
        let folds_kept = after.shown_lines as isize - before.shown_lines as isize == lines_added;
        let doc = &self.tabs[after.tab].document;
        let change = doc.last_change()?;
        if !same_view
            || !folds_kept
            || change.serial != after.serial
            || after.serial != before.serial + 1
        {
            return None;
        }
        // The line above too: its fold chevron can depend on this one's
        // indentation.
        let first = change.start.line.saturating_sub(1);
        if lines_added != 0 || self.wraps(after.tab) {
            Some((first, usize::MAX))
        } else {
            Some((first, change.new_end().line))
        }
    }

    /// The rows of `pane` showing any of `lines` (inclusive ranges), one
    /// rectangle per run of adjacent rows, across the gutter and the text.
    pub(in crate::windows_app) fn rows_showing(
        &self,
        hwnd: HWND,
        pane: usize,
        lines: &[(usize, usize)],
    ) -> Vec<RECT> {
        let doc = &self.tabs[self.tab_for_pane(pane)].document;
        // A line inside a fold shows on the fold's row.
        let lines: Vec<(usize, usize)> = lines
            .iter()
            .map(|&(first, last)| (doc.visible_line_for(first), last))
            .collect();
        let columns = self.wrap_columns(hwnd, pane);
        // Inside the card's left border and clear of the scrollbar strip,
        // neither of which shows the text: what paint's fast path redraws.
        let left = self.pane_left(hwnd, pane) + self.scale(1).max(1);
        let right = self.pane_right(hwnd, pane) - self.scale(SCROLLBAR);
        let bottom = self.editor_area(hwnd, false).bottom;
        let (mut line, mut first_row) = self.view_top(hwnd, pane);
        let mut top = self.editor_top();
        let mut areas: Vec<RECT> = Vec::new();
        while top < bottom {
            let rows =
                super::super::wrap::layout_line(doc.line(line), columns, self.settings.tab_size)
                    .count()
                    - first_row;
            let rows_bottom = (top + rows as i32 * self.line_height).min(bottom);
            if lines
                .iter()
                .any(|&(first, last)| (first..=last).contains(&line))
            {
                match areas.last_mut() {
                    Some(area) if area.bottom == top => area.bottom = rows_bottom,
                    _ => areas.push(RECT {
                        left,
                        top,
                        right,
                        bottom: rows_bottom,
                    }),
                }
            }
            top = rows_bottom;
            let next = doc.next_visible_line(line);
            if next == line {
                break;
            }
            line = next;
            first_row = 0;
        }
        areas
    }

    /// Runs `update`, which asks for the whole window to be redrawn, and
    /// narrows that to `areas`: typing only changes the editor and the status
    /// bar, but each keystroke redrew everything, 5–9 ms a time. Whatever was
    /// already waiting to be redrawn stays waiting.
    pub(in crate::windows_app) fn repaint_only(
        &mut self,
        hwnd: HWND,
        areas: &[RECT],
        update: impl FnOnce(&mut Self),
    ) {
        unsafe {
            let pending = CreateRectRgn(0, 0, 0, 0);
            if pending.is_null() {
                // Without somewhere to keep what's waiting, don't narrow.
                update(self);
                return;
            }
            let had_pending = GetUpdateRgn(hwnd, pending, 0) > NULLREGION;
            update(self);
            ValidateRect(hwnd, null());
            if had_pending {
                InvalidateRgn(hwnd, pending, 0);
            }
            DeleteObject(pending);
            for area in areas {
                InvalidateRect(hwnd, area, 0);
            }
        }
    }

    /// Redraws what a caret blink changes. Usually that's just the editor's
    /// caret; when a text box elsewhere has the caret (Quick Open, the
    /// terminal, a search box or the commit message), the whole window is
    /// redrawn, as each paints its own caret from `caret_on`.
    pub(in crate::windows_app) fn invalidate_blink(&self, hwnd: HWND) {
        let editor_only = !self.welcome
            && !self.quick_open
            && !self.terminal_focus
            && !self.search_input
            && !self.panel_focus
            && !self.extensions_search_active
            && !self.commit_focus;
        if editor_only {
            self.invalidate_caret(hwnd);
        } else {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    pub(in crate::windows_app) fn fill(hdc: HDC, rect: RECT, color: u32) {
        unsafe {
            let brush = CreateSolidBrush(color);
            FillRect(hdc, &rect, brush);
            DeleteObject(brush);
        }
    }

    pub(in crate::windows_app) fn rounded_fill(hdc: HDC, rect: RECT, radius: i32, color: u32) {
        unsafe {
            let brush = CreateSolidBrush(color);
            let old_brush = SelectObject(hdc, brush);
            let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
            RoundRect(
                hdc,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                radius,
                radius,
            );
            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            DeleteObject(brush);
        }
    }

    pub(in crate::windows_app) fn label(
        hdc: HDC,
        text: &str,
        x: i32,
        y: i32,
        color: u32,
        clip: RECT,
    ) {
        unsafe {
            let chars: Vec<u16> = text.encode_utf16().collect();
            SetTextColor(hdc, color);
            ExtTextOutW(
                hdc,
                x,
                y,
                ETO_CLIPPED,
                &clip,
                chars.as_ptr(),
                chars.len() as u32,
                null(),
            );
        }
    }

    // Like `label`, but trims from the end and appends an ellipsis instead of
    // letting GDI hard-clip mid-character when the text doesn't fit between
    // `x` and `clip.right`.
    pub(in crate::windows_app) fn label_ellipsis(
        &self,
        hdc: HDC,
        text: &str,
        x: i32,
        y: i32,
        color: u32,
        clip: RECT,
    ) {
        let max_width = clip.right - x;
        if self.text_width(hdc, text) <= max_width {
            Self::label(hdc, text, x, y, color, clip);
            return;
        }
        let ellipsis = "\u{2026}";
        let budget = (max_width - self.text_width(hdc, ellipsis)).max(0);
        let mut end = text.len();
        while end > 0 && self.text_width(hdc, &text[..end]) > budget {
            end -= 1;
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
        }
        Self::label(
            hdc,
            &format!("{}{ellipsis}", &text[..end]),
            x,
            y,
            color,
            clip,
        );
    }

    pub(in crate::windows_app) fn chevron(&self, hdc: HDC, x: i32, y: i32, expanded: bool) {
        unsafe {
            let half = self.scale(4).max(4);
            let pen = CreatePen(PS_SOLID, self.scale(2).max(2), self.theme.muted);
            if pen.is_null() {
                return;
            }
            let previous = SelectObject(hdc, pen);
            if expanded {
                MoveToEx(hdc, x - half, y - half / 2, null_mut());
                LineTo(hdc, x, y + half / 2);
                LineTo(hdc, x + half, y - half / 2);
            } else {
                MoveToEx(hdc, x - half / 2, y - half, null_mut());
                LineTo(hdc, x + half / 2, y);
                LineTo(hdc, x - half / 2, y + half);
            }
            SelectObject(hdc, previous);
            DeleteObject(pen);
        }
    }

    pub(in crate::windows_app) fn rail_icon(
        &self,
        hdc: HDC,
        kind: usize,
        x: i32,
        y: i32,
        color: u32,
    ) {
        if kind == 0
            && self
                .icons
                .draw_generic(hdc, GenericIcon::FolderOpen, x, y, self.scale(17))
        {
            return;
        }
        unsafe {
            let pen = CreatePen(PS_SOLID, self.scale(1).max(1), color);
            let previous_pen = SelectObject(hdc, pen);
            let previous_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
            let s = |value| self.scale(value);
            match kind {
                0 => {
                    // Back sheet
                    let back_pts = [
                        POINT {
                            x: x + s(2),
                            y: y + s(1),
                        },
                        POINT {
                            x: x + s(9),
                            y: y + s(1),
                        },
                        POINT {
                            x: x + s(12),
                            y: y + s(4),
                        },
                        POINT {
                            x: x + s(12),
                            y: y + s(12),
                        },
                        POINT {
                            x: x + s(2),
                            y: y + s(12),
                        },
                        POINT {
                            x: x + s(2),
                            y: y + s(1),
                        },
                    ];
                    Polyline(hdc, back_pts.as_ptr(), back_pts.len() as i32);
                    // Front sheet
                    let front_pts = [
                        POINT {
                            x: x + s(5),
                            y: y + s(4),
                        },
                        POINT {
                            x: x + s(13),
                            y: y + s(4),
                        },
                        POINT {
                            x: x + s(16),
                            y: y + s(7),
                        },
                        POINT {
                            x: x + s(16),
                            y: y + s(17),
                        },
                        POINT {
                            x: x + s(5),
                            y: y + s(17),
                        },
                        POINT {
                            x: x + s(5),
                            y: y + s(4),
                        },
                    ];
                    Polyline(hdc, front_pts.as_ptr(), front_pts.len() as i32);
                    // Dog-ear fold on front sheet
                    MoveToEx(hdc, x + s(13), y + s(4), null_mut());
                    LineTo(hdc, x + s(13), y + s(7));
                    LineTo(hdc, x + s(16), y + s(7));
                    // Content lines
                    MoveToEx(hdc, x + s(8), y + s(9), null_mut());
                    LineTo(hdc, x + s(13), y + s(9));
                    MoveToEx(hdc, x + s(8), y + s(12), null_mut());
                    LineTo(hdc, x + s(13), y + s(12));
                }
                1 => {
                    Ellipse(hdc, x + s(2), y + s(2), x + s(11), y + s(11));
                    MoveToEx(hdc, x + s(10), y + s(10), null_mut());
                    LineTo(hdc, x + s(16), y + s(16));
                }
                2 => {
                    MoveToEx(hdc, x + s(5), y + s(4), null_mut());
                    LineTo(hdc, x + s(5), y + s(15));
                    MoveToEx(hdc, x + s(5), y + s(12), null_mut());
                    LineTo(hdc, x + s(13), y + s(7));
                    for (cx, cy) in [(5, 3), (5, 16), (13, 6)] {
                        Ellipse(
                            hdc,
                            x + s(cx - 2),
                            y + s(cy - 2),
                            x + s(cx + 2),
                            y + s(cy + 2),
                        );
                    }
                }
                3 => {
                    let points = [
                        POINT {
                            x: x + s(4),
                            y: y + s(2),
                        },
                        POINT {
                            x: x + s(14),
                            y: y + s(9),
                        },
                        POINT {
                            x: x + s(4),
                            y: y + s(16),
                        },
                        POINT {
                            x: x + s(4),
                            y: y + s(2),
                        },
                    ];
                    Polyline(hdc, points.as_ptr(), points.len() as i32);
                }
                4 => {
                    Rectangle(hdc, x + s(2), y + s(2), x + s(9), y + s(9));
                    Rectangle(hdc, x + s(10), y + s(2), x + s(17), y + s(9));
                    Rectangle(hdc, x + s(2), y + s(10), x + s(9), y + s(17));
                    Rectangle(hdc, x + s(10), y + s(10), x + s(17), y + s(17));
                }
                5 => {
                    let points = [
                        POINT {
                            x: x + s(9),
                            y: y + s(1),
                        },
                        POINT {
                            x: x + s(11),
                            y: y + s(7),
                        },
                        POINT {
                            x: x + s(17),
                            y: y + s(9),
                        },
                        POINT {
                            x: x + s(11),
                            y: y + s(11),
                        },
                        POINT {
                            x: x + s(9),
                            y: y + s(17),
                        },
                        POINT {
                            x: x + s(7),
                            y: y + s(11),
                        },
                        POINT {
                            x: x + s(1),
                            y: y + s(9),
                        },
                        POINT {
                            x: x + s(7),
                            y: y + s(7),
                        },
                        POINT {
                            x: x + s(9),
                            y: y + s(1),
                        },
                    ];
                    Polyline(hdc, points.as_ptr(), points.len() as i32);
                }
                _ => {}
            }
            SelectObject(hdc, previous_brush);
            SelectObject(hdc, previous_pen);
            DeleteObject(pen);
        }
    }

    pub(in crate::windows_app) fn draw_vector_folder(
        &self,
        hdc: HDC,
        x: i32,
        y: i32,
        size: i32,
        expanded: bool,
    ) {
        unsafe {
            let s = |v: i32| (v * size) / 16;
            let folder_tab_color = ui(235, 175, 65); // warm amber gold tab
            let folder_body_color = ui(215, 155, 45); // deeper gold body
            let folder_flap_color = ui(248, 192, 75); // bright front flap when open

            let brush_tab = CreateSolidBrush(folder_tab_color);
            let brush_body = CreateSolidBrush(folder_body_color);
            let pen_border = CreatePen(PS_SOLID, 1, ui(170, 115, 25));

            let prev_brush = SelectObject(hdc, brush_tab);
            let prev_pen = SelectObject(hdc, pen_border);

            if !expanded {
                // Closed folder:
                // 1. Back/tab
                let tab_pts = [
                    POINT {
                        x: x + s(1),
                        y: y + s(2),
                    },
                    POINT {
                        x: x + s(6),
                        y: y + s(2),
                    },
                    POINT {
                        x: x + s(8),
                        y: y + s(4),
                    },
                    POINT {
                        x: x + s(14),
                        y: y + s(4),
                    },
                    POINT {
                        x: x + s(14),
                        y: y + s(13),
                    },
                    POINT {
                        x: x + s(1),
                        y: y + s(13),
                    },
                ];
                Polygon(hdc, tab_pts.as_ptr(), tab_pts.len() as i32);

                // 2. Front body
                SelectObject(hdc, brush_body);
                Rectangle(hdc, x + s(1), y + s(5), x + s(15), y + s(14));
            } else {
                // Open folder:
                // 1. Back folder sheet
                let back_pts = [
                    POINT {
                        x: x + s(1),
                        y: y + s(2),
                    },
                    POINT {
                        x: x + s(6),
                        y: y + s(2),
                    },
                    POINT {
                        x: x + s(8),
                        y: y + s(4),
                    },
                    POINT {
                        x: x + s(14),
                        y: y + s(4),
                    },
                    POINT {
                        x: x + s(14),
                        y: y + s(13),
                    },
                    POINT {
                        x: x + s(1),
                        y: y + s(13),
                    },
                ];
                Polygon(hdc, back_pts.as_ptr(), back_pts.len() as i32);

                // 2. Front perspective flap open
                let brush_flap = CreateSolidBrush(folder_flap_color);
                SelectObject(hdc, brush_flap);
                let flap_pts = [
                    POINT {
                        x: x + s(1),
                        y: y + s(7),
                    },
                    POINT {
                        x: x + s(13),
                        y: y + s(7),
                    },
                    POINT {
                        x: x + s(15),
                        y: y + s(14),
                    },
                    POINT {
                        x: x + s(3),
                        y: y + s(14),
                    },
                ];
                Polygon(hdc, flap_pts.as_ptr(), flap_pts.len() as i32);
                DeleteObject(brush_flap);
            }

            SelectObject(hdc, prev_brush);
            SelectObject(hdc, prev_pen);
            DeleteObject(brush_tab);
            DeleteObject(brush_body);
            DeleteObject(pen_border);
        }
    }

    pub(in crate::windows_app) fn draw_vector_file(
        &self,
        hdc: HDC,
        path: &Path,
        x: i32,
        y: i32,
        size: i32,
    ) {
        unsafe {
            let s = |v: i32| (v * size) / 16;
            let style = file_type_style(path);

            let brush_body = CreateSolidBrush(style.body_color);
            let pen_body = CreatePen(PS_SOLID, 1, style.body_color);
            let prev_brush = SelectObject(hdc, brush_body);
            let prev_pen = SelectObject(hdc, pen_body);

            // Document sheet with top-right dog-ear fold
            let doc_pts = [
                POINT {
                    x: x + s(2),
                    y: y + s(1),
                },
                POINT {
                    x: x + s(10),
                    y: y + s(1),
                },
                POINT {
                    x: x + s(14),
                    y: y + s(5),
                },
                POINT {
                    x: x + s(14),
                    y: y + s(15),
                },
                POINT {
                    x: x + s(2),
                    y: y + s(15),
                },
            ];
            Polygon(hdc, doc_pts.as_ptr(), doc_pts.len() as i32);

            // Dog-ear corner fold triangle (accent color)
            let brush_fold = CreateSolidBrush(style.accent_color);
            let pen_fold = CreatePen(PS_SOLID, 1, style.accent_color);
            SelectObject(hdc, brush_fold);
            SelectObject(hdc, pen_fold);
            let fold_pts = [
                POINT {
                    x: x + s(10),
                    y: y + s(1),
                },
                POINT {
                    x: x + s(14),
                    y: y + s(5),
                },
                POINT {
                    x: x + s(10),
                    y: y + s(5),
                },
            ];
            Polygon(hdc, fold_pts.as_ptr(), fold_pts.len() as i32);

            // Inner lines or badge
            if !style.badge.is_empty() {
                // Subtle badge pill inside the file
                let badge_pill = RECT {
                    left: x + s(3),
                    top: y + s(7),
                    right: x + s(13),
                    bottom: y + s(14),
                };
                let badge_brush = CreateSolidBrush(ui(16, 24, 38)); // dark contrast pill
                let badge_pen = CreatePen(PS_SOLID, 1, ui(16, 24, 38));
                SelectObject(hdc, badge_brush);
                SelectObject(hdc, badge_pen);
                RoundRect(
                    hdc,
                    badge_pill.left,
                    badge_pill.top,
                    badge_pill.right,
                    badge_pill.bottom,
                    s(3),
                    s(3),
                );
                DeleteObject(badge_brush);
                DeleteObject(badge_pen);

                // Badge text in accent color
                SelectObject(hdc, self.ui_font);
                SetTextColor(hdc, style.accent_color);
                SetBkMode(hdc, TRANSPARENT as i32);
                let utf16: Vec<u16> = style.badge.encode_utf16().collect();
                let mut text_rect = badge_pill;
                DrawTextW(
                    hdc,
                    utf16.as_ptr(),
                    utf16.len() as i32,
                    &mut text_rect,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            } else {
                // Default clean document horizontal lines
                let line_pen = CreatePen(PS_SOLID, 1, style.accent_color);
                SelectObject(hdc, line_pen);
                MoveToEx(hdc, x + s(4), y + s(8), null_mut());
                LineTo(hdc, x + s(11), y + s(8));
                MoveToEx(hdc, x + s(4), y + s(11), null_mut());
                LineTo(hdc, x + s(9), y + s(11));
                DeleteObject(line_pen);
            }

            SelectObject(hdc, prev_brush);
            SelectObject(hdc, prev_pen);
            DeleteObject(brush_body);
            DeleteObject(pen_body);
            DeleteObject(brush_fold);
            DeleteObject(pen_fold);
        }
    }
}

/// See `App::caret_frame`.
pub(in crate::windows_app) struct CaretFrame {
    scene: CaretScene,
    cursor: Pos,
    start: Pos,
    end: Pos,
    brackets: Option<(Pos, Option<Pos>)>,
    dirty: bool,
}

/// Everything besides the caret, the selection and the bracket highlight
/// that decides what the focused pane shows: the file, where it's scrolled
/// to, its text and folds, and what has the keyboard. When any of it
/// changes, the whole editor is redrawn.
#[derive(Clone, Copy, PartialEq)]
struct CaretScene {
    pane: usize,
    tab: usize,
    split: bool,
    text: bool,
    top: (usize, usize),
    serial: u64,
    lines: usize,
    shown_lines: usize,
    breakpoints: usize,
    find: bool,
    keyboard: [bool; 6],
    stays: bool,
}

#[derive(Clone, Copy)]
pub(in crate::windows_app) struct FileTypeStyle {
    pub body_color: u32,
    pub accent_color: u32,
    pub badge: &'static str,
}

pub(in crate::windows_app) fn file_type_style(path: &Path) -> FileTypeStyle {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        "rs" => FileTypeStyle {
            body_color: ui(222, 105, 45), // Rust orange
            accent_color: ui(245, 145, 80),
            badge: "Rs",
        },
        "py" | "pyw" => FileTypeStyle {
            body_color: ui(53, 114, 165),   // Python blue
            accent_color: ui(255, 212, 59), // Python yellow
            badge: "Py",
        },
        "md" | "markdown" => FileTypeStyle {
            body_color: ui(65, 145, 195), // Markdown cyan blue
            accent_color: ui(160, 220, 255),
            badge: "M",
        },
        "toml" => FileTypeStyle {
            body_color: ui(205, 145, 55), // Cargo toml gold
            accent_color: ui(255, 200, 100),
            badge: "C",
        },
        "json" => FileTypeStyle {
            body_color: ui(215, 165, 45), // JSON amber
            accent_color: ui(255, 220, 105),
            badge: "{}",
        },
        "yaml" | "yml" => FileTypeStyle {
            body_color: ui(195, 80, 145), // YAML magenta
            accent_color: ui(240, 125, 195),
            badge: "Y",
        },
        "png" | "jpg" | "jpeg" | "svg" | "ico" | "webp" | "bmp" => FileTypeStyle {
            body_color: ui(165, 95, 220), // Image purple
            accent_color: ui(215, 160, 255),
            badge: "IMG",
        },
        "ps1" | "bat" | "cmd" | "sh" => FileTypeStyle {
            body_color: ui(45, 165, 115), // Terminal green
            accent_color: ui(95, 215, 160),
            badge: ">_",
        },
        "lock" => FileTypeStyle {
            body_color: ui(125, 135, 150), // Lock silver
            accent_color: ui(175, 185, 200),
            badge: "LK",
        },
        _ if name.starts_with(".git") => FileTypeStyle {
            body_color: ui(240, 80, 50), // Git red/orange
            accent_color: ui(255, 130, 100),
            badge: "Git",
        },
        _ => FileTypeStyle {
            body_color: ui(90, 105, 130), // Default document slate
            accent_color: ui(145, 160, 185),
            badge: "",
        },
    }
}

#[inline]
pub(in crate::windows_app) fn safe_slice_prefix(s: &str, byte: usize) -> &str {
    let mut end = byte.min(s.len());
    while !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    &s[..end]
}

#[inline]
pub(in crate::windows_app) fn safe_slice_range(s: &str, start: usize, end: usize) -> &str {
    let mut st = start.min(s.len());
    while !s.is_char_boundary(st) {
        st = st.saturating_sub(1);
    }
    let mut en = end.min(s.len());
    while !s.is_char_boundary(en) {
        en = en.saturating_sub(1);
    }
    if en < st {
        en = st;
    }
    &s[st..en]
}
