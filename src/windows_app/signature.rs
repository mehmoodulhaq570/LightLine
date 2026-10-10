// Parameter hints: typing `(` or `,` in a call shows its signature above the
// caret, with the parameter being typed highlighted. While it shows, each
// edit or caret move asks again, so it follows along; outside a call the
// server answers with nothing and it closes. Ctrl+Shift+Space asks by hand.

use super::*;
use lightline::lsp::{Command as LspCommand, Position as LspPosition, SignatureHelp};

const PADDING: i32 = 8;

pub(super) struct SignatureCard {
    // The document's `id`: the card goes when its tab shows another file.
    document: u64,
    help: SignatureHelp,
    // The caret's top and left when it was answered.
    x: i32,
    y: i32,
}

pub(super) struct SignatureRequest {
    language: LspLanguage,
    id: u64,
    uri: String,
    version: i32,
}

impl App {
    /// After a key or typed character in the editor: asks for the
    /// signature on `(` and `,`, and again after anything while a hint
    /// shows or is on its way.
    pub(super) fn follow_signature(&mut self, hwnd: HWND, typed: Option<char>) {
        if matches!(typed, Some('(' | ','))
            || self.signature.is_some()
            || self.signature_request.is_some()
        {
            self.request_signature(hwnd);
        }
    }

    pub(super) fn request_signature(&mut self, hwnd: HWND) {
        let tab = self.tab();
        let (Some(language), Some(path), true, false) = (
            tab.lsp_language,
            tab.document.path.as_deref(),
            tab.lsp_opened,
            tab.read_only(),
        ) else {
            self.hide_signature(hwnd);
            return;
        };
        let cursor = self.view().cursor;
        let position = LspPosition {
            line: cursor.line as u32,
            character: tab.document.utf16_column(cursor) as u32,
        };
        let uri = lsp::file_uri(path);
        let version = tab.lsp_version;
        self.request_id += 1;
        let id = self.request_id;
        if self.tab_lsp(self.active).is_some_and(|client| {
            client.send(LspCommand::SignatureHelp {
                id,
                uri: uri.clone(),
                version,
                position,
            })
        }) {
            // Only the latest answer counts.
            self.signature_request = Some(SignatureRequest {
                language,
                id,
                uri,
                version,
            });
        }
    }

    pub(super) fn finish_signature(
        &mut self,
        hwnd: HWND,
        language: LspLanguage,
        id: u64,
        uri: &str,
        version: i32,
        help: Option<SignatureHelp>,
    ) {
        if self.signature_request.as_ref().is_none_or(|request| {
            request.language != language
                || request.id != id
                || request.uri != uri
                || request.version != version
        }) {
            return;
        }
        self.signature_request = None;
        if self.tab().lsp_version != version {
            return;
        }
        let Some(help) = help else {
            self.hide_signature(hwnd);
            return;
        };
        let caret = self.caret_rect(hwnd);
        // The same call on the same line keeps its place while typing goes
        // on, rather than sliding along with the caret.
        let (x, y) = match &self.signature {
            Some(shown)
                if shown.document == self.doc().id()
                    && shown.help.label == help.label
                    && shown.y == caret.top =>
            {
                (shown.x, shown.y)
            }
            _ => (caret.left, caret.top),
        };
        let card = SignatureCard {
            document: self.doc().id(),
            help,
            x,
            y,
        };
        if self
            .signature
            .as_ref()
            .is_some_and(|shown| shown.help == card.help && shown.x == card.x && shown.y == card.y)
        {
            return;
        }
        self.invalidate_signature(hwnd);
        self.signature = Some(card);
        self.invalidate_signature(hwnd);
    }

    pub(super) fn hide_signature(&mut self, hwnd: HWND) {
        self.signature_request = None;
        if self.signature.is_some() {
            self.invalidate_signature(hwnd);
            self.signature = None;
        }
    }

    fn invalidate_signature(&self, hwnd: HWND) {
        if let Some(area) = self.signature_layout(hwnd) {
            unsafe { InvalidateRect(hwnd, &area, 0) };
        }
    }

    // Above the caret's line, or below it at the top of the editor.
    fn signature_layout(&self, hwnd: HWND) -> Option<RECT> {
        let card = self.signature.as_ref()?;
        if self.welcome || self.doc().id() != card.document {
            return None;
        }
        let s = |value: i32| self.scale(value);
        let area = self.editor_area(hwnd, false);
        let width = unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.font);
            let width = self.text_width(hdc, &card.help.label);
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            width
        };
        let width = (width + s(PADDING) * 2).min(area.right - area.left - s(16));
        let height = self.line_height + s(PADDING) * 2;
        let left = (card.x - s(PADDING))
            .min(area.right - s(8) - width)
            .max(area.left);
        let above = card.y - s(4) - height;
        let top = if above >= area.top {
            above
        } else {
            card.y + self.line_height + s(4)
        };
        Some(RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        })
    }

    pub(super) fn paint_signature(&self, hdc: HDC, hwnd: HWND) {
        let (Some(card), Some(outer)) = (&self.signature, self.signature_layout(hwnd)) else {
            return;
        };
        let s = |value: i32| self.scale(value);
        self.panel_card(hdc, outer, s(6), self.theme.edge, self.theme.sidebar_bg);
        // Put back afterwards: what's painted next expects its own font.
        let previous = unsafe { SelectObject(hdc, self.font) };
        let label = &card.help.label;
        let (start, end) = card.help.active.unwrap_or((label.len(), label.len()));
        let clip = RECT {
            left: outer.left + s(PADDING),
            right: outer.right - s(PADDING),
            ..outer
        };
        let top = outer.top + s(PADDING);
        let mut x = clip.left;
        for (part, color) in [
            (&label[..start], self.theme.muted),
            (&label[start..end], self.theme.sky),
            (&label[end..], self.theme.muted),
        ] {
            if part.is_empty() {
                continue;
            }
            Self::label(hdc, part, x, top, color, clip);
            x += self.text_width(hdc, part);
        }
        if end > start {
            // Underlined too, for themes where the colors are close.
            let left = clip.left + self.text_width(hdc, &label[..start]);
            let right = (left + self.text_width(hdc, &label[start..end])).min(clip.right);
            let y = top + self.line_height - s(2);
            Self::fill(
                hdc,
                RECT {
                    left,
                    top: y,
                    right,
                    bottom: y + s(1).max(1),
                },
                self.theme.sky,
            );
        }
        unsafe { SelectObject(hdc, previous) };
    }
}
