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

    /// The "IDE" badge after the product name, sized from its text in
    /// `font` so the text stays inside on any font or scale (it ran past a
    /// fixed-width box, #3). Centered on `middle`, `pad` either side of the
    /// text; returns its right edge.
    #[allow(clippy::too_many_arguments)]
    /// The title bar's brand: the LightLine mark (drawn 1:1, never
    /// stretched), the name and the IDE badge, centred in the `height`-tall
    /// bar. Leaves the UI font selected.
    pub(in crate::windows_app) fn paint_brand(&self, hdc: HDC, height: i32) {
        let s = |value: i32| self.scale(value);
        let middle = height / 2;
        let mark = self.icons.draw_brand(hdc, s(12), middle, s(22));
        let name_left = s(12) + mark + s(8);
        unsafe { SelectObject(hdc, self.brand_font) };
        let name_width = self.text_width(hdc, "LightLine");
        self.label_mid(
            hdc,
            "LightLine",
            name_left,
            middle,
            self.theme.text,
            RECT {
                left: name_left,
                top: 0,
                right: name_left + name_width,
                bottom: height,
            },
        );
        self.paint_ide_badge(
            hdc,
            self.ui_font,
            (name_left + name_width + s(6), middle),
            (s(18), s(5), s(4)),
            ui(63, 47, 150),
        );
        unsafe { SelectObject(hdc, self.ui_font) };
    }

    /// Minimize, maximize (restore while maximized) and close, at the title
    /// bar's right end, `height` tall; the one under the mouse lit.
    pub(in crate::windows_app) fn paint_title_controls(&self, hdc: HDC, hwnd: HWND, height: i32) {
        self.paint_hot_title_button(hdc, hwnd);
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let button = self.scale(46);
        let left = client.right - button * 3;
        let maximized = unsafe { IsZoomed(hwnd) } != 0;
        let glyphs = [
            DebugGlyph::Minimize,
            if maximized {
                DebugGlyph::Restore
            } else {
                DebugGlyph::Maximize
            },
            DebugGlyph::Close,
        ];
        for (index, glyph) in glyphs.into_iter().enumerate() {
            let color = if index == 2 && self.hot == Some(Hot::TitleButton(2)) {
                ui(255, 255, 255)
            } else if self.hot == Some(Hot::TitleButton(index)) {
                self.theme.text
            } else {
                self.theme.muted
            };
            let rect = RECT {
                left: left + button * index as i32,
                top: 0,
                right: left + button * (index as i32 + 1),
                bottom: height,
            };
            self.glyph_in(hdc, glyph, color, rect, self.scale(18));
        }
    }

    /// The search box in the title bar (Quick Open, Ctrl+P).
    pub(in crate::windows_app) fn paint_command_center(&self, hdc: HDC, hwnd: HWND) {
        let s = |value: i32| self.scale(value);
        let rect = self.command_center_rect(hwnd);
        if rect.right <= rect.left {
            return;
        }
        let middle = (rect.top + rect.bottom) / 2;
        self.panel_card(hdc, rect, s(5), self.command_center_edge(), ui(12, 25, 48));
        self.glyph_in(
            hdc,
            DebugGlyph::Search,
            self.theme.muted,
            RECT {
                left: rect.left + s(4),
                right: rect.left + s(28),
                ..rect
            },
            s(16),
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        self.label_mid(
            hdc,
            "Search files, symbols, commands...",
            rect.left + s(30),
            middle,
            self.theme.muted,
            RECT {
                left: rect.left + s(30),
                right: rect.right - s(52),
                ..rect
            },
        );
        let key = RECT {
            left: rect.right - s(46),
            top: rect.top + s(4),
            right: rect.right - s(5),
            bottom: rect.bottom - s(4),
        };
        Self::rounded_fill(hdc, key, s(4), ui(25, 43, 76));
        unsafe { SelectObject(hdc, self.small_font) };
        let width = self.text_width(hdc, "Ctrl P");
        self.label_mid(
            hdc,
            "Ctrl P",
            (key.left + key.right - width) / 2,
            middle,
            self.theme.text,
            key,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
    }

    /// The "IDE" badge after the name: `at` is its left edge and middle,
    /// `shape` its height, padding and corner radius. Returns its right edge.
    pub(in crate::windows_app) fn paint_ide_badge(
        &self,
        hdc: HDC,
        font: HFONT,
        (left, middle): (i32, i32),
        (height, pad, radius): (i32, i32, i32),
        color: u32,
    ) -> i32 {
        unsafe {
            let previous = SelectObject(hdc, font);
            let right = left + self.text_width(hdc, "IDE") + pad * 2;
            let badge = RECT {
                left,
                top: middle - height / 2,
                right,
                bottom: middle - height / 2 + height,
            };
            Self::rounded_fill(hdc, badge, radius, color);
            self.label_mid(hdc, "IDE", left + pad, middle, self.theme.text, badge);
            SelectObject(hdc, previous);
            right
        }
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
            let hints = self.hints_on_line(self.tab_for_pane(self.focused_pane), cursor.line);
            let code_left = self.code_left(hwnd);
            let x = code_left - self.scroll_x(self.focused_pane)
                + indent
                + self.text_width(hdc, prefix)
                + self.hint_shift(hdc, hints, row_start, cursor.byte, false);
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            let y = self.editor_top() + row * self.line_height;
            // Scrolled off the left edge: nothing to redraw there.
            let (left, right) = if x < code_left {
                (code_left, code_left)
            } else {
                (x, x + self.scale(2).max(2))
            };
            RECT {
                left,
                top: y,
                right,
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
        let terminal = self.terminal_panel();
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
            && self.menu_bar.is_none()
            && self.menu_bar_focus.is_none()
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
                scroll_x: self.scroll_x(pane),
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
        let mut edited_lines = None;
        if after.scene != before.scene {
            // Typing changes the text, but only on the lines it edited: each
            // keystroke used to redraw every line on screen.
            let Some(edited) = self.edited_lines(&before.scene, &after.scene) else {
                return everything();
            };
            lines.push(edited);
            edited_lines = Some(edited);
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
        if let Some((first, last)) = edited_lines {
            areas.extend(self.edit_shown_elsewhere(hwnd, after.scene.tab, first, last));
        }
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

    /// What else shows lines `first..=last` of tab `tab`, just edited in the
    /// focused pane: the other pane, whole, when it shows the same file, and
    /// a sticky-scroll strip pinning one of those lines.
    fn edit_shown_elsewhere(&self, hwnd: HWND, tab: usize, first: usize, last: usize) -> Vec<RECT> {
        let mut areas = Vec::new();
        let panes = if self.split_visible { 0..2 } else { 0..1 };
        let bottom = self.editor_area(hwnd, false).bottom - self.chrome_gap();
        for pane in panes {
            if self.tab_for_pane(pane) != tab {
                continue;
            }
            let (left, right) = (self.pane_left(hwnd, pane), self.pane_right(hwnd, pane));
            if pane != self.focused_pane {
                areas.push(RECT {
                    left,
                    top: self.editor_top(),
                    right,
                    bottom,
                });
                continue;
            }
            let pinned = self.sticky_shown.borrow()[pane].clone();
            if pinned.iter().any(|line| (first..=last).contains(line)) {
                areas.push(RECT {
                    left,
                    top: self.editor_top(),
                    right: right - self.scale(SCROLLBAR),
                    bottom: (self.editor_top()
                        + pinned.len() as i32 * self.line_height
                        + self.scale(1).max(1))
                    .min(bottom),
                });
            }
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
        let bottom = self.editor_area(hwnd, false).bottom - self.chrome_gap();
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
        if self.debug_prompt_has_keyboard(hwnd) {
            self.invalidate_debug_prompt(hwnd);
            return;
        }
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
        let end = Self::fitting_prefix(hdc, text, budget);
        Self::label(
            hdc,
            &format!("{}{ellipsis}", &text[..end]),
            x,
            y,
            color,
            clip,
        );
    }

    /// The byte length of the longest start of `text` that is at most
    /// `budget` pixels wide. One GDI call measures every prefix at once; the
    /// loop it replaces measured the text again for each character it cut,
    /// which added up on a list of long commit subjects.
    pub(in crate::windows_app) fn fitting_prefix(hdc: HDC, text: &str, budget: i32) -> usize {
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let mut fit = 0i32;
        let mut size = SIZE::default();
        let measured = unsafe {
            GetTextExtentExPointW(
                hdc,
                utf16.as_ptr(),
                utf16.len() as i32,
                budget.max(0),
                &mut fit,
                null_mut(),
                &mut size,
            )
        };
        if measured == 0 {
            return 0;
        }
        // `fit` counts UTF-16 units; end on the last whole character inside.
        let mut units = 0usize;
        let mut end = 0usize;
        for (byte, ch) in text.char_indices() {
            units += ch.len_utf16();
            if units > fit.max(0) as usize {
                break;
            }
            end = byte + ch.len_utf8();
        }
        end
    }

    /// Draws `glyph` in `color`, `size` pixels square, centred in `rect`.
    pub(in crate::windows_app) fn glyph_in(
        &self,
        hdc: HDC,
        glyph: DebugGlyph,
        color: u32,
        rect: RECT,
        size: i32,
    ) {
        let x = (rect.left + rect.right - size) / 2;
        let y = (rect.top + rect.bottom - size) / 2;
        self.icons.draw_glyph(hdc, glyph, color, x, y, size);
    }

    /// A tree's fold chevron centred on (x, y): down when `expanded`.
    pub(in crate::windows_app) fn chevron(&self, hdc: HDC, x: i32, y: i32, expanded: bool) {
        let size = self.scale(16);
        let glyph = if expanded {
            DebugGlyph::ChevronDown
        } else {
            DebugGlyph::ChevronRight
        };
        self.icons.draw_glyph(
            hdc,
            glyph,
            self.theme.muted,
            x - size / 2,
            y - size / 2,
            size,
        );
    }

    /// One of the activity icons, about 18 px, its top left at (x, y):
    /// 0 Explorer, 1 Search, 2 Source Control, 3 Run and Debug,
    /// 4 Extensions, 5 AI Assistant.
    pub(in crate::windows_app) fn rail_icon(
        &self,
        hdc: HDC,
        kind: usize,
        x: i32,
        y: i32,
        color: u32,
    ) {
        let glyph = match kind {
            0 => DebugGlyph::Files,
            1 => DebugGlyph::Search,
            2 => DebugGlyph::SourceControl,
            3 => DebugGlyph::RunDebug,
            4 => DebugGlyph::Extensions,
            _ => DebugGlyph::Sparkle,
        };
        self.icons.draw_glyph(
            hdc,
            glyph,
            color,
            x - self.scale(1),
            y - self.scale(1),
            self.scale(20),
        );
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
    scroll_x: i32,
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
