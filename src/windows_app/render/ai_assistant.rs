use super::super::*;

impl App {
    pub(in crate::windows_app) fn paint_ai_assistant(&self, hdc: HDC, rect: RECT) {
        let s = |value: i32| self.scale(value);
        let card_radius = s(CARD_RADIUS);
        self.panel_card(
            hdc,
            rect,
            card_radius,
            self.theme.card_edge,
            self.theme.sidebar_bg,
        );

        let clip = rect;
        let header_h = s(42);
        let header_bottom = rect.top + header_h;

        // --- Header Bar ---
        unsafe { SelectObject(hdc, self.brand_font) };
        self.sparkle_glyph(
            hdc,
            rect.left + s(14),
            rect.top + s(13),
            s(16),
            self.theme.violet,
        );
        Self::label(
            hdc,
            "AI Assistant",
            rect.left + s(36),
            rect.top + s(11),
            self.theme.text,
            clip,
        );

        // "✦ Tera" model badge
        let model_left = rect.left + s(130);
        let badge_rect = RECT {
            left: model_left,
            top: rect.top + s(10),
            right: model_left + s(64),
            bottom: rect.top + s(30),
        };
        Self::rounded_fill(hdc, badge_rect, s(5), rgb(38, 26, 80));
        self.sparkle_glyph(
            hdc,
            model_left + s(6),
            rect.top + s(15),
            s(10),
            rgb(180, 150, 255),
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        Self::label(
            hdc,
            "Tera",
            model_left + s(20),
            rect.top + s(12),
            rgb(200, 185, 255),
            clip,
        );

        // Header controls (minimize, refresh, close)
        let btn_y = rect.top + s(11);
        Self::label(
            hdc,
            "\u{2014}",
            rect.right - s(66),
            btn_y,
            self.theme.muted,
            clip,
        );
        Self::label(
            hdc,
            "\u{21bb}",
            rect.right - s(46),
            btn_y,
            self.theme.muted,
            clip,
        );
        Self::label(
            hdc,
            "\u{00d7}",
            rect.right - s(24),
            btn_y,
            self.theme.muted,
            clip,
        );

        // Header bottom divider
        Self::fill(
            hdc,
            RECT {
                left: rect.left + s(1),
                top: header_bottom,
                right: rect.right - s(1),
                bottom: header_bottom + s(1).max(1),
            },
            self.theme.edge,
        );

        // --- Chat Body Area ---
        let chat_top = header_bottom + s(12);

        // 1. User Message Bubble (aligned right)
        let user_bubble_w = (rect.right - rect.left - s(48)).max(s(180));
        let user_bubble = RECT {
            left: rect.right - s(16) - user_bubble_w,
            top: chat_top,
            right: rect.right - s(16),
            bottom: chat_top + s(44),
        };
        self.panel_card(hdc, user_bubble, s(8), rgb(38, 64, 120), rgb(18, 34, 66));
        Self::label(
            hdc,
            "Explain this function and suggest an",
            user_bubble.left + s(12),
            user_bubble.top + s(6),
            self.theme.text,
            user_bubble,
        );
        Self::label(
            hdc,
            "optimization if possible.",
            user_bubble.left + s(12),
            user_bubble.top + s(22),
            self.theme.text,
            user_bubble,
        );

        // 2. Assistant Message (Tera)
        let assistant_top = user_bubble.bottom + s(14);
        let avatar_rect = RECT {
            left: rect.left + s(16),
            top: assistant_top,
            right: rect.left + s(38),
            bottom: assistant_top + s(22),
        };
        Self::rounded_fill(hdc, avatar_rect, s(6), rgb(72, 50, 175));
        self.sparkle_glyph(
            hdc,
            avatar_rect.left + s(5),
            avatar_rect.top + s(5),
            s(12),
            self.theme.text,
        );

        unsafe { SelectObject(hdc, self.brand_font) };
        Self::label(
            hdc,
            "Tera",
            avatar_rect.right + s(8),
            assistant_top + s(2),
            self.theme.text,
            clip,
        );

        unsafe { SelectObject(hdc, self.ui_font) };
        let text_y = assistant_top + s(28);
        let text_clip = RECT {
            left: rect.left + s(16),
            top: text_y,
            right: rect.right - s(16),
            bottom: rect.bottom - s(90),
        };

        Self::label(
            hdc,
            "This function calculates the tabs UI layout for a",
            text_clip.left,
            text_y,
            self.theme.text,
            text_clip,
        );
        Self::label(
            hdc,
            "window. It iterates through visible tabs, computes",
            text_clip.left,
            text_y + s(17),
            self.theme.text,
            text_clip,
        );
        Self::label(
            hdc,
            "their bounds and draws them on the screen.",
            text_clip.left,
            text_y + s(34),
            self.theme.text,
            text_clip,
        );

        Self::label(
            hdc,
            "Possible optimization:",
            text_clip.left,
            text_y + s(58),
            rgb(180, 200, 230),
            text_clip,
        );
        Self::label(
            hdc,
            "1. Avoid repeated method calls inside the loop",
            text_clip.left,
            text_y + s(75),
            self.theme.muted,
            text_clip,
        );
        Self::label(
            hdc,
            "   (e.g., self.scale(TAB_WIDTH)) by caching the values.",
            text_clip.left,
            text_y + s(90),
            self.theme.muted,
            text_clip,
        );
        Self::label(
            hdc,
            "2. Use an iterator instead of manual index handling",
            text_clip.left,
            text_y + s(107),
            self.theme.muted,
            text_clip,
        );
        Self::label(
            hdc,
            "   for cleaner and safer code.",
            text_clip.left,
            text_y + s(122),
            self.theme.muted,
            text_clip,
        );

        // 3. Code Block: "Optimized Version"
        let code_top = text_y + s(144);
        let code_h = s(120);
        let code_rect = RECT {
            left: rect.left + s(14),
            top: code_top,
            right: rect.right - s(14),
            bottom: code_top + code_h,
        };
        self.panel_card(hdc, code_rect, s(6), rgb(28, 48, 85), rgb(10, 16, 26));

        // Code header
        Self::label(
            hdc,
            "Optimized Version",
            code_rect.left + s(12),
            code_rect.top + s(8),
            rgb(52, 211, 153),
            code_rect,
        );
        let copy_rect = RECT {
            left: code_rect.right - s(48),
            top: code_rect.top + s(6),
            right: code_rect.right - s(10),
            bottom: code_rect.top + s(24),
        };
        Self::rounded_fill(hdc, copy_rect, s(4), rgb(22, 34, 58));
        Self::label(
            hdc,
            "Copy",
            copy_rect.left + s(8),
            copy_rect.top + s(2),
            self.theme.muted,
            copy_rect,
        );

        // Code lines (monospace font)
        unsafe { SelectObject(hdc, self.font) };
        let code_line_h = self.line_height.min(s(16));
        let c_top = code_rect.top + s(30);

        let lines = [
            (
                "let ",
                self.theme.blue,
                "tab_width = self.scale(TAB_WIDTH);",
                self.theme.text,
            ),
            (
                "let ",
                self.theme.blue,
                "tab_height = self.scale(TAB_HEIGHT);",
                self.theme.text,
            ),
            ("", self.theme.text, "", self.theme.text),
            (
                "for ",
                self.theme.blue,
                "(index, _) in self.visible_tabs().enumerate() {",
                self.theme.text,
            ),
            (
                "    if ",
                self.theme.blue,
                "index >= self.tabs.len() { break; }",
                self.theme.text,
            ),
            (
                "    let ",
                self.theme.blue,
                "left = index as i32 * tab_width;",
                self.theme.text,
            ),
        ];

        for (i, (kw, kw_col, rest, rest_col)) in lines.iter().enumerate() {
            let y = c_top + i as i32 * code_line_h;
            if y + code_line_h > code_rect.bottom - s(4) {
                break;
            }
            let mut x = code_rect.left + s(12);
            if !kw.is_empty() {
                Self::label(hdc, kw, x, y, *kw_col, code_rect);
                x += self.text_width(hdc, kw);
            }
            if !rest.is_empty() {
                Self::label(hdc, rest, x, y, *rest_col, code_rect);
            }
        }

        // --- Bottom Input Area ---
        unsafe { SelectObject(hdc, self.ui_font) };
        let input_h = s(42);
        let input_rect = RECT {
            left: rect.left + s(14),
            top: rect.bottom - s(72),
            right: rect.right - s(14),
            bottom: rect.bottom - s(72) + input_h,
        };
        self.panel_card(hdc, input_rect, s(8), rgb(42, 68, 120), rgb(14, 22, 38));

        Self::label(
            hdc,
            "Ask Tera anything...",
            input_rect.left + s(12),
            input_rect.top + s(12),
            self.theme.muted,
            input_rect,
        );

        // Circular send button
        let send_btn = RECT {
            left: input_rect.right - s(32),
            top: input_rect.top + s(8),
            right: input_rect.right - s(8),
            bottom: input_rect.top + s(32),
        };
        Self::rounded_fill(hdc, send_btn, s(12), rgb(68, 88, 225));
        Self::label(
            hdc,
            "\u{27a4}",
            send_btn.left + s(6),
            send_btn.top + s(4),
            self.theme.text,
            send_btn,
        );

        // Bottom chips row
        let chip_y = input_rect.bottom + s(8);
        let tera_chip = RECT {
            left: rect.left + s(14),
            top: chip_y,
            right: rect.left + s(68),
            bottom: chip_y + s(20),
        };
        Self::rounded_fill(hdc, tera_chip, s(4), rgb(32, 24, 70));
        self.sparkle_glyph(
            hdc,
            tera_chip.left + s(5),
            chip_y + s(5),
            s(9),
            rgb(180, 150, 255),
        );
        Self::label(
            hdc,
            "Tera",
            tera_chip.left + s(18),
            chip_y + s(2),
            rgb(200, 185, 255),
            clip,
        );

        let model_chip = RECT {
            left: tera_chip.right + s(8),
            top: chip_y,
            right: tera_chip.right + s(84),
            bottom: chip_y + s(20),
        };
        Self::rounded_fill(hdc, model_chip, s(4), rgb(20, 32, 58));
        Self::label(
            hdc,
            "Medium \u{25be}",
            model_chip.left + s(8),
            chip_y + s(2),
            self.theme.muted,
            clip,
        );

        // Attachment & Mic icons on right
        Self::label(
            hdc,
            "\u{2301}",
            rect.right - s(48),
            chip_y + s(2),
            self.theme.muted,
            clip,
        );
        Self::label(
            hdc,
            "\u{25c8}",
            rect.right - s(28),
            chip_y + s(2),
            self.theme.muted,
            clip,
        );
    }

    pub(in crate::windows_app) fn sparkle_glyph(
        &self,
        hdc: HDC,
        x: i32,
        y: i32,
        size: i32,
        color: u32,
    ) {
        unsafe {
            let pen = CreatePen(PS_SOLID, self.scale(1).max(1), color);
            let brush = CreateSolidBrush(color);
            let prev_pen = SelectObject(hdc, pen);
            let prev_brush = SelectObject(hdc, brush);

            let s = size;
            let cx = x + s / 2;
            let cy = y + s / 2;
            let points = [
                POINT { x: cx, y },
                POINT {
                    x: cx + s / 6,
                    y: cy - s / 6,
                },
                POINT { x: x + s, y: cy },
                POINT {
                    x: cx + s / 6,
                    y: cy + s / 6,
                },
                POINT { x: cx, y: y + s },
                POINT {
                    x: cx - s / 6,
                    y: cy + s / 6,
                },
                POINT { x, y: cy },
                POINT {
                    x: cx - s / 6,
                    y: cy - s / 6,
                },
            ];
            Polygon(hdc, points.as_ptr(), points.len() as i32);

            SelectObject(hdc, prev_brush);
            SelectObject(hdc, prev_pen);
            DeleteObject(brush);
            DeleteObject(pen);
        }
    }
}
