#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_app {
    use my_editor::clipboard;
    use my_editor::document::{Document, Pos};
    use my_editor::syntax::{Color, RustSyntax};
    use std::cell::RefCell;
    use std::io;
    use std::mem::{size_of, zeroed};
    use std::path::{Path, PathBuf};
    use std::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, CTRL_BREAK_EVENT, CTRL_C_EVENT, GetConsoleWindow,
        SetConsoleCtrlHandler,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    use windows_sys::Win32::UI::Controls::SetScrollInfo;
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const GUTTER: i32 = 64;
    const TOP: i32 = 8;
    const STATUS: i32 = 27;
    const PAD: i32 = 10;
    const TAB_HEIGHT: i32 = 34;
    const TAB_WIDTH: i32 = 180;
    static EDITOR_WINDOW: AtomicIsize = AtomicIsize::new(0);

    unsafe extern "system" fn console_control(event: u32) -> i32 {
        if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
            let hwnd = EDITOR_WINDOW.load(Ordering::Relaxed) as HWND;
            if !hwnd.is_null() {
                unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
            }
            return 1;
        }
        0
    }

    fn connect_parent_console(hwnd: HWND) {
        unsafe {
            let attached = AttachConsole(ATTACH_PARENT_PROCESS) != 0;
            let console = GetConsoleWindow();
            if attached || !console.is_null() {
                EDITOR_WINDOW.store(hwnd as isize, Ordering::Relaxed);
                // Launchers can pass down the inheritable "ignore Ctrl+C" setting.
                SetConsoleCtrlHandler(None, 0);
                SetConsoleCtrlHandler(Some(console_control), 1);
            }
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    #[derive(Default)]
    struct EditorView {
        cursor: Pos,
        selection_anchor: Option<Pos>,
        first_line: usize,
    }

    struct Tab {
        document: Document,
        view: EditorView,
        syntax: Option<RustSyntax>,
    }

    impl Tab {
        fn new(document: Document) -> Self {
            let syntax = Self::is_rust(&document).then(RustSyntax::new);
            Self {
                document,
                view: EditorView::default(),
                syntax,
            }
        }

        fn is_rust(document: &Document) -> bool {
            document
                .path
                .as_deref()
                .and_then(Path::extension)
                .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
        }

        fn update_syntax_language(&mut self) {
            if Self::is_rust(&self.document) {
                if self.syntax.is_none() {
                    self.syntax = Some(RustSyntax::new());
                }
            } else {
                self.syntax = None;
            }
        }
    }

    struct App {
        tabs: Vec<Tab>,
        active: usize,
        tab_first: usize,
        font: HFONT,
        dpi: u32,
        line_height: i32,
        status: String,
        focused: bool,
        caret_on: bool,
        dragging: bool,
        find_mode: bool,
        find_query: String,
        pending_high_surrogate: Option<u16>,
    }

    impl App {
        fn font_for_dpi(dpi: u32) -> HFONT {
            let font_name = wide("Consolas");
            unsafe {
                CreateFontW(
                    -((19 * dpi as i32 + 48) / 96),
                    0,
                    0,
                    0,
                    400,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    font_name.as_ptr(),
                )
            }
        }

        fn new(hwnd: HWND) -> Self {
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
            Self {
                tabs: vec![Tab::new(Document::new())],
                active: 0,
                tab_first: 0,
                font: Self::font_for_dpi(dpi),
                dpi,
                line_height: (24 * dpi as i32 + 48) / 96,
                status: "Ready".into(),
                focused: false,
                caret_on: true,
                dragging: false,
                find_mode: false,
                find_query: String::new(),
                pending_high_surrogate: None,
            }
        }

        fn tab(&self) -> &Tab {
            &self.tabs[self.active]
        }
        fn tab_mut(&mut self) -> &mut Tab {
            &mut self.tabs[self.active]
        }
        fn doc(&self) -> &Document {
            &self.tab().document
        }
        fn doc_mut(&mut self) -> &mut Document {
            &mut self.tab_mut().document
        }
        fn view(&self) -> &EditorView {
            &self.tab().view
        }
        fn view_mut(&mut self) -> &mut EditorView {
            &mut self.tab_mut().view
        }
        fn editor_top(&self) -> i32 {
            self.scale(TAB_HEIGHT + TOP)
        }

        fn syntax_changed(&mut self, line: usize) {
            if let Some(syntax) = &mut self.tab_mut().syntax {
                syntax.invalidate_from(line);
            }
        }

        fn advance_syntax(&mut self, hwnd: HWND) {
            let target = (self.view().first_line + self.visible_lines(hwnd))
                .min(self.doc().line_count().saturating_sub(1));
            let tab = self.tab_mut();
            let pending = tab
                .syntax
                .as_mut()
                .is_some_and(|syntax| !syntax.advance_to(&tab.document, target, 2048));
            unsafe {
                if pending {
                    SetTimer(hwnd, 2, 16, None);
                } else {
                    KillTimer(hwnd, 2);
                }
            }
        }

        fn tab_label(&self, index: usize) -> String {
            let doc = &self.tabs[index].document;
            let name = doc
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".into());
            format!("{}{}", name, if doc.is_dirty() { " *" } else { "" })
        }

        fn visible_tab_count(&self, hwnd: HWND) -> usize {
            let mut rect = RECT::default();
            unsafe {
                GetClientRect(hwnd, &mut rect);
            }
            (rect.right / self.scale(TAB_WIDTH).max(1)).max(1) as usize
        }

        fn show_active_tab(&mut self, hwnd: HWND) {
            let count = self.visible_tab_count(hwnd);
            if self.active < self.tab_first {
                self.tab_first = self.active;
            } else if self.active >= self.tab_first + count {
                self.tab_first = self.active + 1 - count;
            }
            self.find_mode = false;
            self.dragging = false;
            self.update_title(hwnd);
            self.update_scrollbar(hwnd);
            self.advance_syntax(hwnd);
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
        }

        fn activate_tab(&mut self, hwnd: HWND, index: usize) {
            if index < self.tabs.len() {
                self.active = index;
                self.show_active_tab(hwnd);
            }
        }

        fn same_path(a: &Path, b: &Path) -> bool {
            let absolute = |path: &Path| {
                std::fs::canonicalize(path)
                    .or_else(|_| {
                        let parent = path.parent().unwrap_or_else(|| Path::new("."));
                        std::fs::canonicalize(parent)
                            .map(|p| p.join(path.file_name().unwrap_or_default()))
                    })
                    .unwrap_or_else(|_| path.to_path_buf())
                    .to_string_lossy()
                    .to_lowercase()
            };
            absolute(a) == absolute(b)
        }

        fn close_tab(&mut self, hwnd: HWND, index: usize) {
            self.activate_tab(hwnd, index);
            if !self.can_discard(hwnd) {
                return;
            }
            self.tabs.remove(index);
            if self.tabs.is_empty() {
                self.tabs.push(Tab::new(Document::new()));
                self.active = 0;
            } else {
                self.active = index.min(self.tabs.len() - 1);
            }
            self.status = "Ready".into();
            self.show_active_tab(hwnd);
        }

        fn can_close_window(&mut self, hwnd: HWND) -> bool {
            for index in 0..self.tabs.len() {
                if self.tabs[index].document.is_dirty() {
                    self.activate_tab(hwnd, index);
                    if !self.can_discard(hwnd) {
                        return false;
                    }
                }
            }
            true
        }

        fn scale(&self, pixels: i32) -> i32 {
            (pixels * self.dpi as i32 + 48) / 96
        }

        fn set_dpi(&mut self, dpi: u32) {
            let dpi = dpi.max(96);
            if dpi == self.dpi {
                return;
            }
            let font = Self::font_for_dpi(dpi);
            if font.is_null() {
                return;
            }
            unsafe {
                DeleteObject(self.font);
            }
            self.font = font;
            self.dpi = dpi;
            self.line_height = (24 * dpi as i32 + 48) / 96;
        }

        fn visible_lines(&self, hwnd: HWND) -> usize {
            let mut rect = RECT::default();
            unsafe {
                GetClientRect(hwnd, &mut rect);
            }
            ((rect.bottom - rect.top - self.editor_top() - self.scale(STATUS)).max(1)
                / self.line_height)
                .max(1) as usize
        }

        fn update_scrollbar(&self, hwnd: HWND) {
            let visible = self.visible_lines(hwnd);
            let info = SCROLLINFO {
                cbSize: size_of::<SCROLLINFO>() as u32,
                fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
                nMin: 0,
                nMax: self
                    .doc()
                    .line_count()
                    .saturating_sub(1)
                    .min(i32::MAX as usize) as i32,
                nPage: visible as u32,
                nPos: self.view().first_line.min(i32::MAX as usize) as i32,
                nTrackPos: 0,
            };
            unsafe {
                SetScrollInfo(hwnd, SB_VERT, &info, 1);
            }
        }

        fn keep_cursor_visible(&mut self, hwnd: HWND) {
            let visible = self.visible_lines(hwnd);
            let line = self.view().cursor.line;
            if line < self.view().first_line {
                self.view_mut().first_line = line;
            }
            if line >= self.view().first_line + visible {
                self.view_mut().first_line = line + 1 - visible;
            }
            self.update_scrollbar(hwnd);
            self.caret_on = true;
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
        }

        fn update_title(&self, hwnd: HWND) {
            let file = self
                .doc()
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".into());
            let title = format!(
                "{}{} — My Editor",
                file,
                if self.doc().is_dirty() { " *" } else { "" }
            );
            unsafe {
                SetWindowTextW(hwnd, wide(&title).as_ptr());
            }
        }

        fn refresh(&mut self, hwnd: HWND) {
            self.update_title(hwnd);
            self.keep_cursor_visible(hwnd);
            self.advance_syntax(hwnd);
        }

        fn selection_range(&self) -> Option<(Pos, Pos)> {
            let anchor = self.view().selection_anchor?;
            let cursor = self.view().cursor;
            if anchor == cursor {
                None
            } else if anchor < cursor {
                Some((anchor, cursor))
            } else {
                Some((cursor, anchor))
            }
        }

        fn move_cursor(&mut self, pos: Pos, extend: bool) {
            let cursor = self.view().cursor;
            if extend {
                self.view_mut().selection_anchor.get_or_insert(cursor);
            } else {
                self.view_mut().selection_anchor = None;
            }
            let pos = self.doc().clamp(pos);
            self.view_mut().cursor = pos;
        }

        fn replace_selection(&mut self, text: &str) {
            let (start, end) = self
                .selection_range()
                .unwrap_or((self.view().cursor, self.view().cursor));
            self.replace_range(start, end, text);
            self.view_mut().selection_anchor = None;
        }

        fn replace_range(&mut self, start: Pos, end: Pos, text: &str) {
            let cursor = self.doc_mut().replace(start, end, text);
            self.syntax_changed(start.line);
            self.view_mut().cursor = cursor;
        }

        fn copy_selection(&mut self, hwnd: HWND) -> bool {
            let Some((start, end)) = self.selection_range() else {
                return false;
            };
            let text = self.doc().text_range(start, end);
            match clipboard::copy(hwnd, &text) {
                Ok(()) => {
                    self.status = "Copied selection".into();
                    true
                }
                Err(error) => {
                    self.error(hwnd, &error);
                    false
                }
            }
        }

        fn find(&mut self, hwnd: HWND, forward: bool) {
            if self.find_query.is_empty() {
                self.status = "Find: enter a query with Ctrl+F".into();
                self.refresh(hwnd);
                return;
            }
            let match_at = if forward {
                self.doc()
                    .find_forward(self.view().cursor, &self.find_query)
            } else {
                let origin = self
                    .selection_range()
                    .map(|(start, _)| start)
                    .unwrap_or(self.view().cursor);
                self.doc().find_backward(origin, &self.find_query)
            };
            if let Some(start) = match_at {
                let end = Pos {
                    line: start.line,
                    byte: start.byte + self.find_query.len(),
                };
                self.view_mut().selection_anchor = Some(start);
                self.view_mut().cursor = end;
                self.status = format!("Found: {}", self.find_query);
            } else {
                self.status = format!("Not found: {}", self.find_query);
            }
            self.refresh(hwnd);
        }

        fn text_width(&self, hdc: HDC, text: &str) -> i32 {
            let expanded = text.replace('\t', "    ");
            let utf16: Vec<u16> = expanded.encode_utf16().collect();
            let mut size = SIZE::default();
            unsafe {
                GetTextExtentPoint32W(hdc, utf16.as_ptr(), utf16.len() as i32, &mut size);
            }
            size.cx
        }

        fn caret_rect(&self, hwnd: HWND) -> RECT {
            unsafe {
                let hdc = GetDC(hwnd);
                let old = SelectObject(hdc, self.font);
                let line = self.doc().line(self.view().cursor.line);
                let x = self.scale(GUTTER + PAD)
                    + self.text_width(hdc, &line[..self.view().cursor.byte]);
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

        fn invalidate_caret(&self, hwnd: HWND) {
            let rect = self.caret_rect(hwnd);
            unsafe {
                InvalidateRect(hwnd, &rect, 0);
            }
        }

        fn paint(&self, hwnd: HWND) {
            unsafe {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let old_font = SelectObject(hdc, self.font);
                SetBkMode(hdc, TRANSPARENT as i32);
                let mut rect = RECT::default();
                GetClientRect(hwnd, &mut rect);
                let editor_bottom = (rect.bottom - self.scale(STATUS)).max(0);
                let bg = CreateSolidBrush(0x001d1b19);
                let gutter_bg = CreateSolidBrush(0x00252220);
                let status_bg = CreateSolidBrush(0x00322d29);
                let selection_bg = CreateSolidBrush(0x007d4f33);
                let selection = self.selection_range();
                FillRect(
                    hdc,
                    &RECT {
                        left: 0,
                        top: 0,
                        right: rect.right,
                        bottom: editor_bottom,
                    },
                    bg,
                );
                FillRect(
                    hdc,
                    &RECT {
                        left: 0,
                        top: self.scale(TAB_HEIGHT),
                        right: self.scale(GUTTER),
                        bottom: editor_bottom,
                    },
                    gutter_bg,
                );
                let tab_bg = CreateSolidBrush(0x00252220);
                let active_bg = CreateSolidBrush(0x001d1b19);
                FillRect(
                    hdc,
                    &RECT {
                        left: 0,
                        top: 0,
                        right: rect.right,
                        bottom: self.scale(TAB_HEIGHT),
                    },
                    tab_bg,
                );
                let tab_width = self.scale(TAB_WIDTH);
                let tab_height = self.scale(TAB_HEIGHT);
                for slot in 0..self.visible_tab_count(hwnd) {
                    let index = self.tab_first + slot;
                    if index >= self.tabs.len() {
                        break;
                    }
                    let left = slot as i32 * tab_width;
                    if left >= rect.right {
                        break;
                    }
                    let bounds = RECT {
                        left,
                        top: 0,
                        right: (left + tab_width).min(rect.right),
                        bottom: tab_height,
                    };
                    if index == self.active {
                        FillRect(hdc, &bounds, active_bg);
                    }
                    let label = self.tab_label(index);
                    let chars: Vec<u16> = label.encode_utf16().collect();
                    SetTextColor(
                        hdc,
                        if index == self.active {
                            0x00e3ded8
                        } else {
                            0x00a59c92
                        },
                    );
                    let clip = RECT {
                        left: left + self.scale(12),
                        top: 0,
                        right: (left + tab_width - self.scale(30)).min(rect.right),
                        bottom: tab_height,
                    };
                    ExtTextOutW(
                        hdc,
                        clip.left,
                        self.scale(5),
                        ETO_CLIPPED,
                        &clip,
                        chars.as_ptr(),
                        chars.len() as u32,
                        null(),
                    );
                    let close = wide("×");
                    TextOutW(
                        hdc,
                        left + tab_width - self.scale(23),
                        self.scale(5),
                        close.as_ptr(),
                        1,
                    );
                }
                let visible = self.visible_lines(hwnd) + 1;
                for row in 0..visible {
                    let index = self.view().first_line + row;
                    if index >= self.doc().line_count() {
                        break;
                    }
                    let y = self.editor_top() + row as i32 * self.line_height;
                    if y >= editor_bottom {
                        break;
                    }
                    let number = format!("{}", index + 1);
                    let num: Vec<u16> = number.encode_utf16().collect();
                    SetTextColor(hdc, 0x00958b81);
                    let number_clip = RECT {
                        left: 0,
                        top: y,
                        right: self.scale(GUTTER),
                        bottom: editor_bottom,
                    };
                    ExtTextOutW(
                        hdc,
                        self.scale(12),
                        y,
                        ETO_CLIPPED,
                        &number_clip,
                        num.as_ptr(),
                        num.len() as u32,
                        null(),
                    );
                    let source = self.doc().line(index);
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
                        let x1 = self.scale(GUTTER + PAD) + self.text_width(hdc, &source[..from]);
                        let x2 = self.scale(GUTTER + PAD)
                            + self.text_width(hdc, &source[..to])
                            + if index < end.line { self.scale(8) } else { 0 };
                        if x2 > x1 && x1 < rect.right {
                            FillRect(
                                hdc,
                                &RECT {
                                    left: x1,
                                    top: y,
                                    right: x2.min(rect.right),
                                    bottom: (y + self.line_height).min(editor_bottom),
                                },
                                selection_bg,
                            );
                        }
                    }
                    let line = source.replace('\t', "    ");
                    let chars: Vec<u16> = line.encode_utf16().collect();
                    SetTextColor(hdc, 0x00e3ded8);
                    let clip = RECT {
                        left: self.scale(GUTTER + PAD),
                        top: y,
                        right: rect.right,
                        bottom: editor_bottom,
                    };
                    ExtTextOutW(
                        hdc,
                        self.scale(GUTTER + PAD),
                        y,
                        ETO_CLIPPED,
                        &clip,
                        chars.as_ptr(),
                        chars.len() as u32,
                        null(),
                    );
                    if source.len() <= 16_384
                        && let Some(syntax) = &self.tab().syntax
                    {
                        for span in syntax.spans(self.doc(), index) {
                            let color = match span.color {
                                Color::Comment => 0x009caa82,
                                Color::String => 0x008fcfba,
                                Color::Keyword => 0x00e8a57d,
                                Color::Type => 0x00cfb78e,
                                Color::Number => 0x00a8c5e8,
                                Color::Macro => 0x00dbb6d7,
                            };
                            SetTextColor(hdc, color);
                            let left = self.scale(GUTTER + PAD)
                                + self.text_width(hdc, &source[..span.start]);
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
                if self.focused && self.caret_on {
                    let line = self.doc().line(self.view().cursor.line);
                    let x = self.scale(GUTTER + PAD)
                        + self.text_width(hdc, &line[..self.view().cursor.byte]);
                    let y = self.editor_top()
                        + (self.view().cursor.line as i64 - self.view().first_line as i64) as i32
                            * self.line_height;
                    if y >= self.scale(TAB_HEIGHT) && y < editor_bottom && x < rect.right {
                        let caret = CreateSolidBrush(0x00ffc078);
                        FillRect(
                            hdc,
                            &RECT {
                                left: x,
                                top: y,
                                right: x + self.scale(2).max(2),
                                bottom: (y + self.line_height).min(editor_bottom),
                            },
                            caret,
                        );
                        DeleteObject(caret);
                    }
                }
                FillRect(
                    hdc,
                    &RECT {
                        left: 0,
                        top: editor_bottom,
                        right: rect.right,
                        bottom: rect.bottom,
                    },
                    status_bg,
                );
                let label = format!(
                    "{}    Ln {}, Col {}    {} lines",
                    self.status,
                    self.view().cursor.line + 1,
                    self.doc().line(self.view().cursor.line)[..self.view().cursor.byte]
                        .chars()
                        .count()
                        + 1,
                    self.doc().line_count()
                );
                let chars: Vec<u16> = label.encode_utf16().collect();
                SetTextColor(hdc, 0x00e3ded8);
                TextOutW(
                    hdc,
                    self.scale(12),
                    editor_bottom + self.scale(4),
                    chars.as_ptr(),
                    chars.len() as i32,
                );
                DeleteObject(bg);
                DeleteObject(gutter_bg);
                DeleteObject(status_bg);
                DeleteObject(selection_bg);
                DeleteObject(tab_bg);
                DeleteObject(active_bg);
                SelectObject(hdc, old_font);
                EndPaint(hwnd, &ps);
            }
        }

        fn error(&mut self, hwnd: HWND, error: &impl std::fmt::Display) {
            self.status = error.to_string();
            unsafe {
                MessageBoxW(
                    hwnd,
                    wide(&self.status).as_ptr(),
                    wide("My Editor").as_ptr(),
                    MB_OK | MB_ICONERROR,
                );
            }
            self.refresh(hwnd);
        }

        fn dialog(&self, hwnd: HWND, save: bool) -> Option<PathBuf> {
            let mut buffer = [0u16; 32768];
            if save && let Some(path) = &self.doc().path {
                let name: Vec<u16> = path.file_name()?.to_string_lossy().encode_utf16().collect();
                buffer[..name.len()].copy_from_slice(&name);
            }
            let filter = wide("Text files\0*.txt;*.rs;*.py;*.c;*.cpp;*.h;*.md\0All files\0*.*\0");
            let mut dialog: OPENFILENAMEW = unsafe { zeroed() };
            dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
            dialog.hwndOwner = hwnd;
            dialog.lpstrFilter = filter.as_ptr();
            dialog.lpstrFile = buffer.as_mut_ptr();
            dialog.nMaxFile = buffer.len() as u32;
            dialog.Flags = OFN_EXPLORER
                | OFN_PATHMUSTEXIST
                | if save {
                    OFN_OVERWRITEPROMPT
                } else {
                    OFN_FILEMUSTEXIST
                };
            let ok = unsafe {
                if save {
                    GetSaveFileNameW(&mut dialog)
                } else {
                    GetOpenFileNameW(&mut dialog)
                }
            };
            if ok == 0 {
                return None;
            }
            Some(PathBuf::from(String::from_utf16_lossy(
                &buffer[..buffer.iter().position(|c| *c == 0)?],
            )))
        }

        fn save(&mut self, hwnd: HWND, save_as: bool) -> bool {
            let path = if save_as || self.doc().path.is_none() {
                match self.dialog(hwnd, true) {
                    Some(path) => path,
                    None => return false,
                }
            } else {
                self.doc().path.clone().unwrap()
            };
            if self.tabs.iter().enumerate().any(|(index, tab)| {
                index != self.active
                    && tab
                        .document
                        .path
                        .as_deref()
                        .is_some_and(|other| Self::same_path(other, &path))
            }) {
                self.error(hwnd, &"That file is already open in another tab");
                return false;
            }
            match self.doc_mut().save(&path) {
                Ok(()) => {
                    self.tab_mut().update_syntax_language();
                    self.status = format!("Saved {}", path.display());
                    self.refresh(hwnd);
                    true
                }
                Err(error) => {
                    self.error(hwnd, &error);
                    false
                }
            }
        }

        fn can_discard(&mut self, hwnd: HWND) -> bool {
            if !self.doc().is_dirty() {
                return true;
            }
            let answer = unsafe {
                MessageBoxW(
                    hwnd,
                    wide(&format!("Save changes to {}?", self.tab_label(self.active))).as_ptr(),
                    wide("My Editor").as_ptr(),
                    MB_YESNOCANCEL | MB_ICONQUESTION,
                )
            };
            if answer == IDYES {
                self.save(hwnd, false)
            } else {
                answer == IDNO
            }
        }

        fn open(&mut self, hwnd: HWND, path: Option<PathBuf>) {
            let Some(path) = path.or_else(|| self.dialog(hwnd, false)) else {
                return;
            };
            if let Some(index) = self.tabs.iter().position(|tab| {
                tab.document
                    .path
                    .as_deref()
                    .is_some_and(|open| Self::same_path(open, &path))
            }) {
                self.activate_tab(hwnd, index);
                self.status = format!("Already open: {}", path.display());
                return;
            }
            match Document::open(path.clone()) {
                Ok(document) => {
                    if self.tabs.len() == 1
                        && self.doc().path.is_none()
                        && !self.doc().is_dirty()
                        && self.doc().line(0).is_empty()
                    {
                        self.tabs[0] = Tab::new(document);
                        self.active = 0;
                    } else {
                        self.tabs.push(Tab::new(document));
                        self.active = self.tabs.len() - 1;
                    }
                    self.status = format!("Opened {}", path.display());
                    self.show_active_tab(hwnd);
                }
                Err(error) => self.error(hwnd, &error),
            }
        }

        fn key(&mut self, hwnd: HWND, key: u32) -> bool {
            let ctrl = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
            let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
            if self.find_mode {
                match key {
                    x if x == VK_ESCAPE as u32 => {
                        self.find_mode = false;
                        self.status = "Ready".into();
                        self.refresh(hwnd);
                        return true;
                    }
                    x if x == VK_BACK as u32 || x == VK_RETURN as u32 => return true,
                    _ => {}
                }
            }
            if ctrl {
                let cursor = self.view().cursor;
                match key {
                    0x41 => {
                        self.view_mut().selection_anchor = Some(Pos::default());
                        let end = self.doc().end();
                        self.view_mut().cursor = end;
                    }
                    0x43 => {
                        self.copy_selection(hwnd);
                    }
                    0x58 => {
                        if self.copy_selection(hwnd) {
                            self.replace_selection("");
                        }
                    }
                    0x56 => match clipboard::paste(hwnd) {
                        Ok(Some(text)) => self.replace_selection(&text),
                        Ok(None) => {}
                        Err(error) => self.error(hwnd, &error),
                    },
                    0x4e => {
                        self.tabs.push(Tab::new(Document::new()));
                        self.active = self.tabs.len() - 1;
                        self.status = "New document".into();
                        self.show_active_tab(hwnd);
                        return true;
                    }
                    0x46 => {
                        self.find_mode = true;
                        self.find_query.clear();
                        self.status = "Find: ".into();
                    }
                    0x4f => {
                        self.open(hwnd, None);
                        return true;
                    }
                    0x53 => {
                        self.save(hwnd, shift);
                    }
                    0x57 => {
                        self.close_tab(hwnd, self.active);
                        return true;
                    }
                    x if x == VK_TAB as u32 => {
                        let next = if shift {
                            (self.active + self.tabs.len() - 1) % self.tabs.len()
                        } else {
                            (self.active + 1) % self.tabs.len()
                        };
                        self.activate_tab(hwnd, next);
                        return true;
                    }
                    x if x == VK_PRIOR as u32 => {
                        self.activate_tab(hwnd, self.active.saturating_sub(1));
                        return true;
                    }
                    x if x == VK_NEXT as u32 => {
                        self.activate_tab(hwnd, (self.active + 1).min(self.tabs.len() - 1));
                        return true;
                    }
                    0x5a if shift => {
                        self.view_mut().selection_anchor = None;
                        if let Some((cursor, line)) = self.doc_mut().redo() {
                            self.view_mut().cursor = cursor;
                            self.syntax_changed(line);
                        }
                    }
                    0x5a => {
                        self.view_mut().selection_anchor = None;
                        if let Some((cursor, line)) = self.doc_mut().undo() {
                            self.view_mut().cursor = cursor;
                            self.syntax_changed(line);
                        }
                    }
                    0x59 => {
                        self.view_mut().selection_anchor = None;
                        if let Some((cursor, line)) = self.doc_mut().redo() {
                            self.view_mut().cursor = cursor;
                            self.syntax_changed(line);
                        }
                    }
                    x if x == VK_HOME as u32 => self.move_cursor(Pos::default(), shift),
                    x if x == VK_END as u32 => self.move_cursor(self.doc().end(), shift),
                    x if x == VK_LEFT as u32 => {
                        let target = self.doc().previous_word(cursor);
                        self.move_cursor(target, shift);
                    }
                    x if x == VK_RIGHT as u32 => {
                        let target = self.doc().next_word(cursor);
                        self.move_cursor(target, shift);
                    }
                    x if x == VK_BACK as u32 => {
                        if self.selection_range().is_some() {
                            self.replace_selection("");
                        } else {
                            let previous = self.doc().previous_word(cursor);
                            self.replace_range(previous, cursor, "");
                        }
                    }
                    x if x == VK_DELETE as u32 => {
                        if self.selection_range().is_some() {
                            self.replace_selection("");
                        } else {
                            let next = self.doc().next_word(cursor);
                            self.replace_range(cursor, next, "");
                        }
                    }
                    _ => return false,
                }
                self.refresh(hwnd);
                return true;
            }
            let cursor = self.view().cursor;
            match key {
                x if x == VK_F3 as u32 => {
                    self.find_mode = false;
                    self.find(hwnd, !shift);
                    return true;
                }
                x if x == VK_ESCAPE as u32 => {
                    self.view_mut().selection_anchor = None;
                }
                x if x == VK_LEFT as u32 => {
                    let target = if !shift {
                        self.selection_range().map(|(start, _)| start)
                    } else {
                        None
                    }
                    .unwrap_or_else(|| self.doc().previous(cursor));
                    self.move_cursor(target, shift);
                }
                x if x == VK_RIGHT as u32 => {
                    let target = if !shift {
                        self.selection_range().map(|(_, end)| end)
                    } else {
                        None
                    }
                    .unwrap_or_else(|| self.doc().next(cursor));
                    self.move_cursor(target, shift);
                }
                x if x == VK_UP as u32 => {
                    self.move_cursor(
                        Pos {
                            line: cursor.line.saturating_sub(1),
                            byte: cursor.byte,
                        },
                        shift,
                    );
                }
                x if x == VK_DOWN as u32 => {
                    self.move_cursor(
                        Pos {
                            line: (cursor.line + 1).min(self.doc().line_count() - 1),
                            byte: cursor.byte,
                        },
                        shift,
                    );
                }
                x if x == VK_PRIOR as u32 => {
                    self.move_cursor(
                        Pos {
                            line: cursor.line.saturating_sub(self.visible_lines(hwnd)),
                            byte: cursor.byte,
                        },
                        shift,
                    );
                }
                x if x == VK_NEXT as u32 => {
                    self.move_cursor(
                        Pos {
                            line: (cursor.line + self.visible_lines(hwnd))
                                .min(self.doc().line_count() - 1),
                            byte: cursor.byte,
                        },
                        shift,
                    );
                }
                x if x == VK_HOME as u32 => self.move_cursor(
                    Pos {
                        line: cursor.line,
                        byte: 0,
                    },
                    shift,
                ),
                x if x == VK_END as u32 => {
                    self.move_cursor(
                        Pos {
                            line: cursor.line,
                            byte: self.doc().line(cursor.line).len(),
                        },
                        shift,
                    );
                }
                x if x == VK_BACK as u32 => {
                    if self.selection_range().is_some() {
                        self.replace_selection("");
                    } else {
                        let previous = self.doc().previous(cursor);
                        self.replace_range(previous, cursor, "");
                    }
                }
                x if x == VK_DELETE as u32 => {
                    if self.selection_range().is_some() {
                        self.replace_selection("");
                    } else {
                        let next = self.doc().next(cursor);
                        self.replace_range(cursor, next, "");
                    }
                }
                _ => return false,
            }
            self.refresh(hwnd);
            true
        }

        fn character(&mut self, hwnd: HWND, unit: u16) {
            if unsafe { GetKeyState(VK_CONTROL as i32) } < 0 {
                return;
            }
            if self.find_mode && unit == 8 {
                self.find_query.pop();
                self.status = format!("Find: {}", self.find_query);
                self.refresh(hwnd);
                return;
            }
            if self.find_mode && unit == 13 {
                self.find_mode = false;
                self.find(hwnd, true);
                return;
            }
            if (unit < 32 && unit != 9 && unit != 13) || unit == 127 {
                return;
            }
            let ch = if (0xd800..=0xdbff).contains(&unit) {
                self.pending_high_surrogate = Some(unit);
                return;
            } else if (0xdc00..=0xdfff).contains(&unit) {
                let Some(high) = self.pending_high_surrogate.take() else {
                    return;
                };
                char::from_u32(0x10000 + ((high as u32 - 0xd800) << 10) + (unit as u32 - 0xdc00))
            } else {
                self.pending_high_surrogate = None;
                char::from_u32(unit as u32)
            };
            if let Some(ch) = ch {
                if self.find_mode {
                    if !ch.is_control() {
                        self.find_query.push(ch);
                        self.status = format!("Find: {}", self.find_query);
                        self.refresh(hwnd);
                    }
                    return;
                }
                let text = if ch == '\r' {
                    "\n".to_owned()
                } else {
                    ch.to_string()
                };
                self.replace_selection(&text);
                self.refresh(hwnd);
            }
        }

        fn position_at(&self, hwnd: HWND, x: i32, y: i32) -> Pos {
            let row = ((y - self.editor_top()) / self.line_height).max(0) as usize;
            let line = (self.view().first_line + row).min(self.doc().line_count() - 1);
            let target = (x - self.scale(GUTTER + PAD)).max(0);
            unsafe {
                let hdc = GetDC(hwnd);
                let old = SelectObject(hdc, self.font);
                let text = self.doc().line(line);
                let boundaries: Vec<usize> = text
                    .char_indices()
                    .map(|(index, _)| index)
                    .chain(Some(text.len()))
                    .collect();
                let mut low = 0;
                let mut high = boundaries.len();
                while low < high {
                    let mid = (low + high) / 2;
                    if self.text_width(hdc, &text[..boundaries[mid]]) < target {
                        low = mid + 1;
                    } else {
                        high = mid;
                    }
                }
                let right = low.min(boundaries.len() - 1);
                let left = right.saturating_sub(1);
                let left_width = self.text_width(hdc, &text[..boundaries[left]]);
                let right_width = self.text_width(hdc, &text[..boundaries[right]]);
                let byte = if target - left_width <= right_width - target {
                    boundaries[left]
                } else {
                    boundaries[right]
                };
                SelectObject(hdc, old);
                ReleaseDC(hwnd, hdc);
                Pos { line, byte }
            }
        }

        fn mouse_click(&mut self, hwnd: HWND, x: i32, y: i32, extend: bool) {
            if y < self.scale(TAB_HEIGHT) {
                let slot = (x.max(0) / self.scale(TAB_WIDTH).max(1)) as usize;
                let index = self.tab_first + slot;
                if index < self.tabs.len() {
                    if x % self.scale(TAB_WIDTH) >= self.scale(TAB_WIDTH - 30) {
                        self.close_tab(hwnd, index);
                    } else {
                        self.activate_tab(hwnd, index);
                    }
                }
                return;
            }
            let mut rect = RECT::default();
            unsafe {
                GetClientRect(hwnd, &mut rect);
            }
            if y >= rect.bottom - self.scale(STATUS) {
                return;
            }
            let pos = self.position_at(hwnd, x, y);
            self.move_cursor(pos, extend);
            self.dragging = true;
            unsafe {
                SetFocus(hwnd);
                SetCapture(hwnd);
            }
            self.refresh(hwnd);
        }

        fn mouse_drag(&mut self, hwnd: HWND, x: i32, y: i32) {
            if !self.dragging {
                return;
            }
            let pos = self.position_at(hwnd, x, y);
            self.move_cursor(pos, true);
            self.refresh(hwnd);
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_DESTROY {
            EDITOR_WINDOW.store(0, Ordering::Relaxed);
            unsafe { PostQuitMessage(0) };
            return 0;
        }
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RefCell<App> };
        if ptr.is_null() {
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }
        let cell = unsafe { &*ptr };
        let Ok(mut app) = cell.try_borrow_mut() else {
            return if msg == WM_CLOSE {
                0
            } else {
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            };
        };
        match msg {
            WM_PAINT => {
                app.advance_syntax(hwnd);
                app.paint(hwnd);
                0
            }
            WM_DPICHANGED => {
                app.set_dpi((wparam & 0xffff) as u32);
                let suggested = unsafe { &*(lparam as *const RECT) };
                unsafe {
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        suggested.left,
                        suggested.top,
                        suggested.right - suggested.left,
                        suggested.bottom - suggested.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                app.keep_cursor_visible(hwnd);
                0
            }
            WM_SIZE => {
                app.keep_cursor_visible(hwnd);
                let count = app.visible_tab_count(hwnd);
                if app.active >= app.tab_first + count {
                    app.tab_first = app.active + 1 - count;
                }
                0
            }
            WM_SETFOCUS => {
                app.focused = true;
                app.caret_on = true;
                unsafe {
                    SetTimer(hwnd, 1, 530, None);
                }
                app.invalidate_caret(hwnd);
                0
            }
            WM_KILLFOCUS => {
                app.focused = false;
                unsafe {
                    KillTimer(hwnd, 1);
                }
                app.invalidate_caret(hwnd);
                0
            }
            WM_TIMER if wparam == 1 => {
                if app.focused {
                    app.caret_on = !app.caret_on;
                    app.invalidate_caret(hwnd);
                }
                0
            }
            WM_TIMER if wparam == 2 => {
                app.advance_syntax(hwnd);
                unsafe {
                    InvalidateRect(hwnd, null(), 0);
                }
                0
            }
            WM_SETCURSOR if (lparam as u32 & 0xffff) == HTCLIENT => {
                unsafe {
                    let mut point = POINT::default();
                    GetCursorPos(&mut point);
                    ScreenToClient(hwnd, &mut point);
                    let cursor = LoadCursorW(
                        null_mut(),
                        if point.y < app.scale(TAB_HEIGHT) {
                            IDC_ARROW
                        } else {
                            IDC_IBEAM
                        },
                    );
                    SetCursor(if cursor.is_null() {
                        LoadCursorW(null_mut(), IDC_ARROW)
                    } else {
                        cursor
                    });
                }
                1
            }
            WM_KEYDOWN => {
                if app.key(hwnd, wparam as u32) {
                    0
                } else {
                    drop(app);
                    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
                }
            }
            WM_CHAR => {
                app.character(hwnd, wparam as u16);
                0
            }
            WM_LBUTTONDOWN => {
                app.mouse_click(
                    hwnd,
                    (lparam as u32 & 0xffff) as i16 as i32,
                    ((lparam as u32 >> 16) & 0xffff) as i16 as i32,
                    unsafe { GetKeyState(VK_SHIFT as i32) } < 0,
                );
                0
            }
            WM_MOUSEMOVE => {
                if app.dragging && wparam & 1 != 0 {
                    app.mouse_drag(
                        hwnd,
                        (lparam as u32 & 0xffff) as i16 as i32,
                        ((lparam as u32 >> 16) & 0xffff) as i16 as i32,
                    );
                }
                0
            }
            WM_LBUTTONUP => {
                app.dragging = false;
                unsafe {
                    ReleaseCapture();
                }
                0
            }
            WM_MOUSEWHEEL => {
                let delta = (wparam >> 16) as i16;
                if delta > 0 {
                    app.view_mut().first_line = app.view().first_line.saturating_sub(3);
                } else if delta < 0 {
                    app.view_mut().first_line =
                        (app.view().first_line + 3).min(app.doc().line_count().saturating_sub(1));
                }
                app.update_scrollbar(hwnd);
                unsafe {
                    InvalidateRect(hwnd, null(), 0);
                }
                0
            }
            WM_VSCROLL => {
                let code = (wparam & 0xffff) as i32;
                let max = app.doc().line_count().saturating_sub(1);
                app.view_mut().first_line = match code {
                    SB_LINEUP => app.view().first_line.saturating_sub(1),
                    SB_LINEDOWN => (app.view().first_line + 1).min(max),
                    SB_PAGEUP => app
                        .view()
                        .first_line
                        .saturating_sub(app.visible_lines(hwnd)),
                    SB_PAGEDOWN => (app.view().first_line + app.visible_lines(hwnd)).min(max),
                    SB_THUMBPOSITION | SB_THUMBTRACK => {
                        let mut info = SCROLLINFO {
                            cbSize: size_of::<SCROLLINFO>() as u32,
                            fMask: SIF_TRACKPOS,
                            ..unsafe { zeroed() }
                        };
                        unsafe {
                            GetScrollInfo(hwnd, SB_VERT, &mut info);
                        }
                        (info.nTrackPos.max(0) as usize).min(max)
                    }
                    _ => app.view().first_line,
                };
                app.update_scrollbar(hwnd);
                unsafe {
                    InvalidateRect(hwnd, null(), 0);
                }
                0
            }
            WM_CLOSE => {
                if app.can_close_window(hwnd) {
                    unsafe {
                        DestroyWindow(hwnd);
                    }
                }
                0
            }
            _ => {
                drop(app);
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
    }

    pub fn run() -> io::Result<()> {
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(null());
            let class = wide("MyEditorWindow");
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wnd_proc),
                hInstance: instance,
                hCursor: LoadCursorW(null_mut(), IDC_IBEAM),
                lpszClassName: class.as_ptr(),
                ..zeroed()
            };
            if RegisterClassW(&wc) == 0 {
                return Err(io::Error::last_os_error());
            }
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                wide("My Editor").as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VSCROLL,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1000,
                700,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            if hwnd.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut app = Box::new(RefCell::new(App::new(hwnd)));
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                (&mut *app as *mut RefCell<App>) as isize,
            );
            connect_parent_console(hwnd);
            app.borrow().update_title(hwnd);
            app.borrow().update_scrollbar(hwnd);
            ShowWindow(hwnd, SW_SHOW);
            SetFocus(hwnd);
            if let Some(path) = std::env::args_os().nth(1) {
                app.borrow_mut().open(hwnd, Some(PathBuf::from(path)));
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            DeleteObject(app.borrow().font);
            Ok(())
        }
    }
}

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_app::run() {
        eprintln!("My Editor: {error}");
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("My Editor currently supports Windows only.");
}
