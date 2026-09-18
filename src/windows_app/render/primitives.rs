use super::super::*;

impl App {
    pub(in crate::windows_app) fn text_width(&self, hdc: HDC, text: &str) -> i32 {
        let expanded = text.replace('\t', "    ");
        let utf16: Vec<u16> = expanded.encode_utf16().collect();
        let mut size = SIZE::default();
        unsafe {
            GetTextExtentPoint32W(hdc, utf16.as_ptr(), utf16.len() as i32, &mut size);
        }
        size.cx
    }

    pub(in crate::windows_app) fn caret_rect(&self, hwnd: HWND) -> RECT {
        unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.font);
            let line = self.doc().line(self.view().cursor.line);
            let x = self.code_left(hwnd) + self.text_width(hdc, &line[..self.view().cursor.byte]);
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            let y = self.editor_top()
                + (self.view().cursor.line as i64 - self.view().first_line as i64) as i32
                    * self.line_height;
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

    pub(in crate::windows_app) fn chevron(&self, hdc: HDC, x: i32, y: i32, expanded: bool) {
        unsafe {
            let half = self.scale(4).max(4);
            let pen = CreatePen(PS_SOLID, self.scale(2).max(2), MUTED);
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
        if kind == 0 {
            self.icons.draw(hdc, "folder-open", x, y, self.scale(17));
            return;
        }
        unsafe {
            let pen = CreatePen(PS_SOLID, self.scale(1).max(1), color);
            let previous_pen = SelectObject(hdc, pen);
            let previous_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
            let s = |value| self.scale(value);
            match kind {
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
}
