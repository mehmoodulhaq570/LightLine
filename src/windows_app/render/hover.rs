use super::super::*;

impl App {
    pub(in crate::windows_app) fn paint_hover_card(
        &self,
        hdc: HDC,
        window: RECT,
        editor_bottom: i32,
    ) {
        let Some(card) = &self.hover_card else { return };
        let margin = self.scale(10);
        let padding = self.scale(12);
        let width = self
            .scale(440)
            .min((window.right - self.editor_left() - margin * 2).max(0));
        if width < self.scale(160) {
            return;
        }
        let chars: Vec<u16> = card.text.encode_utf16().collect();
        let mut measure = RECT {
            left: 0,
            top: 0,
            right: width - padding * 2,
            bottom: 0,
        };
        unsafe {
            SelectObject(hdc, self.ui_font);
            DrawTextW(
                hdc,
                chars.as_ptr(),
                chars.len() as i32,
                &mut measure,
                DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
            );
        }
        let height = (measure.bottom + padding * 2).clamp(self.scale(42), self.scale(190));
        let x = card.x.clamp(
            self.editor_left() + margin,
            (window.right - width - margin).max(self.editor_left() + margin),
        );
        let y = if card.y + height > editor_bottom - margin {
            (card.y - height - self.scale(28)).max(self.editor_top())
        } else {
            card.y
        };
        Self::fill(
            hdc,
            RECT {
                left: x,
                top: y,
                right: x + width,
                bottom: y + height,
            },
            EDGE,
        );
        Self::fill(
            hdc,
            RECT {
                left: x + 1,
                top: y + 1,
                right: x + width - 1,
                bottom: y + height - 1,
            },
            rgb(24, 38, 60),
        );
        let mut content = RECT {
            left: x + padding,
            top: y + padding,
            right: x + width - padding,
            bottom: y + height - padding,
        };
        unsafe {
            SetTextColor(hdc, TEXT);
            DrawTextW(
                hdc,
                chars.as_ptr(),
                chars.len() as i32,
                &mut content,
                DT_WORDBREAK | DT_NOPREFIX,
            );
        }
    }
}
