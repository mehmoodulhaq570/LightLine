use super::*;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW};

// WM_MOUSELEAVE is not re-exported by the enabled windows-sys feature set.
const WM_MOUSE_LEAVE: u32 = 0x02A3;

// ---------------------------------------------------------------------------
// Native dialog dark mode
// ---------------------------------------------------------------------------
// The file/folder pickers (GetOpenFileNameW/GetSaveFileNameW/SHBrowseForFolderW)
// are large common controls we cannot custom-draw. Opting the process into a
// dark "preferred app mode" makes those native dialogs render with a dark title
// bar and client area so they no longer clash with the dark workbench. The two
// entry points are undocumented uxtheme ordinals (135/136); we resolve them at
// runtime and ignore failure on systems where they are unavailable.
pub(super) fn enable_native_dark_mode() {
    const PREFERRED_MODE_FORCE_DARK: i32 = 2;
    const SET_PREFERRED_APP_MODE: usize = 135;
    const FLUSH_MENU_THEMES: usize = 136;
    unsafe {
        let module = LoadLibraryExW(wide("uxtheme.dll").as_ptr(), null_mut(), 0);
        if module.is_null() {
            return;
        }
        if let Some(address) = GetProcAddress(module, SET_PREFERRED_APP_MODE as *const u8) {
            let set_preferred: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(address);
            set_preferred(PREFERRED_MODE_FORCE_DARK);
        }
        if let Some(address) = GetProcAddress(module, FLUSH_MENU_THEMES as *const u8) {
            let flush: unsafe extern "system" fn() -> i32 = std::mem::transmute(address);
            flush();
        }
    }
}

// ---------------------------------------------------------------------------
// Custom LightLine-styled modal message box
// ---------------------------------------------------------------------------
// A small owner-drawn, borderless popup that mirrors the workbench palette and
// typography. It runs its own nested message loop and returns the id of the
// button the user chose, so callers keep the same synchronous shape as the old
// MessageBoxW calls.

// Button result codes (kept aligned with the classic MessageBox IDs).
pub(super) const DLG_OK: isize = 1;
pub(super) const DLG_CANCEL: isize = 2;
pub(super) const DLG_YES: isize = 6;
pub(super) const DLG_NO: isize = 7;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DialogIcon {
    Question,
    Error,
}

impl DialogIcon {
    // DialogIcon is a small standalone enum (modal dialogs are built before
    // an App reference is threaded through their call chain), so it can't
    // reach `self.theme` here the way render code can. Kept as the same
    // fixed literal Theme::default_dark().blue currently holds rather than
    // expanding this refactor into rewiring the dialog module's call chain.
    fn tint(self) -> u32 {
        match self {
            DialogIcon::Question => rgb(56, 189, 248),
            DialogIcon::Error => rgb(205, 79, 79),
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            DialogIcon::Question => "?",
            DialogIcon::Error => "!",
        }
    }
}

pub(super) struct DialogButton {
    pub label: &'static str,
    pub id: isize,
    pub is_default: bool,
    pub is_cancel: bool,
}

pub(super) const BTN_OK: DialogButton = DialogButton {
    label: "OK",
    id: DLG_OK,
    is_default: true,
    is_cancel: false,
};

pub(super) const BTN_CANCEL: DialogButton = DialogButton {
    label: "Cancel",
    id: DLG_CANCEL,
    is_default: false,
    is_cancel: true,
};

pub(super) const BTN_DELETE: DialogButton = DialogButton {
    label: "Delete",
    id: DLG_OK,
    is_default: true,
    is_cancel: false,
};

pub(super) fn yes_no_cancel() -> [DialogButton; 3] {
    [
        DialogButton {
            label: "Yes",
            id: DLG_YES,
            is_default: true,
            is_cancel: false,
        },
        DialogButton {
            label: "No",
            id: DLG_NO,
            is_default: false,
            is_cancel: false,
        },
        DialogButton {
            label: "Cancel",
            id: DLG_CANCEL,
            is_default: false,
            is_cancel: true,
        },
    ]
}

// Dialog surface colors, tuned for the dark workbench.
const DLG_BG: u32 = rgb(15, 25, 41);
const DLG_HEADER: u32 = rgb(20, 33, 55);
const DLG_BORDER: u32 = rgb(48, 68, 100);
// Same tradeoff as DialogIcon::tint above: a module-level const can't read
// `self.theme` at all, so this stays the fixed default value.
const DLG_ACCENT: u32 = rgb(56, 189, 248);
// paint_dialog() below is a standalone free function with no App/theme
// reference reachable at all: modal dialogs run their own nested message
// loop, architecturally outside the main window's paint pipeline (see the
// module doc comment at the top of this file). Rewiring that to reach
// self.theme is real scope beyond this foundation pass, so these stay fixed
// at Theme::default_dark()'s text/muted values.
const DLG_TEXT: u32 = rgb(226, 234, 248);
const DLG_MUTED: u32 = rgb(136, 156, 188);
const DLG_BTN: u32 = rgb(31, 47, 74);
const DLG_BTN_HOVER: u32 = rgb(41, 60, 93);
const DLG_BTN_PRESSED: u32 = rgb(24, 37, 60);
const DLG_BTN_PRIMARY: u32 = rgb(52, 96, 168);
const DLG_BTN_PRIMARY_HOVER: u32 = rgb(64, 116, 200);

struct DialogButtonItem {
    label: Vec<u16>,
    id: isize,
    is_default: bool,
    is_cancel: bool,
    width: i32,
    rect: RECT,
}

struct DialogState {
    title: Vec<u16>,
    message: Vec<u16>,
    icon: DialogIcon,
    buttons: Vec<DialogButtonItem>,
    default_index: usize,
    cancel_index: Option<usize>,
    focus: usize,
    hover: Option<usize>,
    hover_close: bool,
    pressed: Option<usize>,
    pressed_close: bool,
    result: *mut isize,
    font: HFONT,
    title_font: HFONT,
    dpi: u32,
    pad: i32,
    header_height: i32,
    radius: i32,
    icon_rect: RECT,
    text_rect: RECT,
    close_rect: RECT,
}

impl DialogState {
    fn scale(&self, pixels: i32) -> i32 {
        scaled(pixels, self.dpi, 100)
    }
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn measure_width(hdc: HDC, text: &[u16]) -> i32 {
    let mut size = SIZE::default();
    unsafe {
        GetTextExtentPoint32W(hdc, text.as_ptr(), text.len() as i32, &mut size);
    }
    size.cx
}

pub(super) fn show_dialog(
    owner: HWND,
    title: &str,
    message: &str,
    icon: DialogIcon,
    buttons: &[DialogButton],
) -> isize {
    if buttons.is_empty() {
        return DLG_CANCEL;
    }
    let fallback = buttons
        .iter()
        .find(|button| button.is_cancel)
        .or(buttons.first())
        .map(|button| button.id)
        .unwrap_or(DLG_CANCEL);

    unsafe {
        let instance = GetModuleHandleW(null());
        let class = wide("LightLineDialog");
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(dialog_wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..zeroed()
        };
        if RegisterClassExW(&wc) == 0 && GetLastError() != 1410 {
            return fallback;
        }

        let dpi = {
            let raw = if owner.is_null() {
                96
            } else {
                GetDpiForWindow(owner)
            };
            raw.max(96)
        };
        let body_font = create_font("Segoe UI", 14, 400, dpi);
        let title_font = create_font("Segoe UI Semibold", 15, 600, dpi);

        // Layout metrics (logical pixels scaled by the owner's DPI).
        let pad = scaled(24, dpi, 100);
        let header_height = scaled(46, dpi, 100);
        let icon_size = scaled(38, dpi, 100);
        let icon_gap = scaled(16, dpi, 100);
        let button_height = scaled(34, dpi, 100);
        let button_gap = scaled(12, dpi, 100);
        let button_pad_x = scaled(20, dpi, 100);
        let radius = scaled(8, dpi, 100);
        let base_width = scaled(440, dpi, 100);

        let measure_dc = GetDC(null_mut());
        let previous = SelectObject(measure_dc, body_font);
        let mut items: Vec<DialogButtonItem> = Vec::with_capacity(buttons.len());
        let mut row_width = 0;
        for button in buttons {
            let label = utf16(button.label);
            let width = measure_width(measure_dc, &label) + button_pad_x * 2;
            row_width += width + button_gap;
            items.push(DialogButtonItem {
                label,
                id: button.id,
                is_default: button.is_default,
                is_cancel: button.is_cancel,
                width,
                rect: RECT::default(),
            });
        }
        row_width -= button_gap;
        let title_width = measure_width(measure_dc, &utf16(title));

        let message_utf16 = utf16(message);
        let mut calc = RECT {
            left: 0,
            top: 0,
            right: (base_width - pad * 2 - icon_size - icon_gap).max(scaled(120, dpi, 100)),
            bottom: 0,
        };
        DrawTextW(
            measure_dc,
            message_utf16.as_ptr(),
            message_utf16.len() as i32,
            &mut calc,
            DT_WORDBREAK | DT_CALCRECT | DT_EDITCONTROL | DT_NOPREFIX,
        );
        let text_height = (calc.bottom - calc.top).max(scaled(20, dpi, 100));
        SelectObject(measure_dc, previous);
        ReleaseDC(null_mut(), measure_dc);

        let mut width = base_width
            .max(row_width + pad * 2)
            .max(title_width + icon_size + icon_gap + pad * 2)
            .min(scaled(680, dpi, 100));
        width = width.max(row_width + pad * 2);

        let content_top = header_height + pad;
        let block = text_height.max(icon_size);
        let buttons_top = content_top + block + pad;
        let height = buttons_top + button_height + pad;

        // Right-align the button row.
        let mut cursor = width - pad;
        for item in items.iter_mut().rev() {
            item.rect = RECT {
                left: cursor - item.width,
                top: buttons_top,
                right: cursor,
                bottom: buttons_top + button_height,
            };
            cursor = item.rect.left - button_gap;
        }

        let text_rect = RECT {
            left: pad + icon_size + icon_gap,
            top: content_top,
            right: width - pad,
            bottom: content_top + text_height,
        };
        let icon_rect = RECT {
            left: pad,
            top: content_top,
            right: pad + icon_size,
            bottom: content_top + icon_size,
        };
        let close_size = scaled(30, dpi, 100);
        let close_rect = RECT {
            left: width - pad - close_size,
            top: (header_height - close_size) / 2,
            right: width - pad,
            bottom: (header_height + close_size) / 2,
        };

        let default_index = items.iter().position(|b| b.is_default).unwrap_or(0);
        let cancel_index = items.iter().position(|b| b.is_cancel);

        let mut result: isize = fallback;
        let state = Box::new(DialogState {
            title: utf16(title),
            message: message_utf16,
            icon,
            buttons: items,
            default_index,
            cancel_index,
            focus: default_index,
            hover: None,
            hover_close: false,
            pressed: None,
            pressed_close: false,
            result: &mut result as *mut isize,
            font: body_font,
            title_font,
            dpi,
            pad,
            header_height,
            radius,
            icon_rect,
            text_rect,
            close_rect,
        });
        let state_ptr = Box::into_raw(state);

        let (x, y) = if owner.is_null() {
            (CW_USEDEFAULT, CW_USEDEFAULT)
        } else {
            let mut frame = RECT::default();
            GetWindowRect(owner, &mut frame);
            let owner_w = frame.right - frame.left;
            let owner_h = frame.bottom - frame.top;
            (
                frame.left + (owner_w - width) / 2,
                frame.top + (owner_h - height) / 2,
            )
        };
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            wide("LightLine").as_ptr(),
            WS_POPUP,
            x,
            y,
            width,
            height,
            owner,
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            drop(Box::from_raw(state_ptr));
            DeleteObject(body_font);
            DeleteObject(title_font);
            return fallback;
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
        if !owner.is_null() {
            EnableWindow(owner, 0);
        }
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);

        let mut msg = MSG::default();
        let mut retrieved = GetMessageW(&mut msg, null_mut(), 0, 0);
        while retrieved > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            retrieved = GetMessageW(&mut msg, null_mut(), 0, 0);
        }

        if !owner.is_null() {
            EnableWindow(owner, 1);
            SetForegroundWindow(owner);
            SetFocus(owner);
        }
        result
    }
}

fn create_font(name: &str, size: i32, weight: i32, dpi: u32) -> HFONT {
    unsafe {
        CreateFontW(
            -scaled(size, dpi, 100),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            1,
            0,
            0,
            CLEARTYPE_QUALITY as u32,
            0,
            wide(name).as_ptr(),
        )
    }
}

fn state_from(hwnd: HWND) -> *mut DialogState {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState }
}

unsafe extern "system" fn dialog_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_DESTROY {
        let ptr = state_from(hwnd);
        if !ptr.is_null() {
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            let state = unsafe { Box::from_raw(ptr) };
            unsafe {
                DeleteObject(state.font);
                DeleteObject(state.title_font);
            }
            drop(state);
        }
        unsafe { PostQuitMessage(0) };
        return 0;
    }
    let ptr = state_from(hwnd);
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let state = unsafe { &mut *ptr };
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint_dialog(state, hwnd);
            0
        }
        WM_NCHITTEST => {
            let mut point = POINT {
                x: (lparam as i32 & 0xffff) as i16 as i32,
                y: ((lparam as i32 >> 16) & 0xffff) as i16 as i32,
            };
            unsafe { ScreenToClient(hwnd, &mut point) };
            let on_close = in_rect(&state.close_rect, point.x, point.y);
            if point.y < state.header_height && !on_close {
                HTCAPTION as LRESULT
            } else {
                HTCLIENT as LRESULT
            }
        }
        WM_MOUSEMOVE => {
            let x = (lparam as i32 & 0xffff) as i16 as i32;
            let y = ((lparam as i32 >> 16) & 0xffff) as i16 as i32;
            let hit = hit_button(state, x, y);
            let hit_close = in_rect(&state.close_rect, x, y);
            if hit != state.hover || hit_close != state.hover_close {
                state.hover = hit;
                state.hover_close = hit_close;
                unsafe { InvalidateRect(hwnd, null(), 0) };
                let mut event = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                unsafe { TrackMouseEvent(&mut event) };
            }
            0
        }
        WM_MOUSE_LEAVE => {
            if state.hover.is_some() || state.hover_close {
                state.hover = None;
                state.hover_close = false;
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            0
        }
        WM_LBUTTONDOWN => {
            let x = (lparam as i32 & 0xffff) as i16 as i32;
            let y = ((lparam as i32 >> 16) & 0xffff) as i16 as i32;
            unsafe { SetCapture(hwnd) };
            if in_rect(&state.close_rect, x, y) {
                state.pressed_close = true;
                state.pressed = None;
            } else {
                state.pressed_close = false;
                state.pressed = hit_button(state, x, y);
            }
            unsafe { InvalidateRect(hwnd, null(), 0) };
            0
        }
        WM_LBUTTONUP => {
            let x = (lparam as i32 & 0xffff) as i16 as i32;
            let y = ((lparam as i32 >> 16) & 0xffff) as i16 as i32;
            unsafe { ReleaseCapture() };
            let pressed_close = state.pressed_close;
            let pressed = state.pressed;
            state.pressed = None;
            state.pressed_close = false;
            unsafe { InvalidateRect(hwnd, null(), 0) };
            if pressed_close {
                if in_rect(&state.close_rect, x, y) {
                    activate_cancel(state, hwnd);
                }
            } else if let Some(index) = pressed
                && in_rect(&state.buttons[index].rect, x, y)
            {
                activate(state, hwnd, index);
            }
            0
        }
        WM_KEYDOWN => {
            let count = state.buttons.len();
            match wparam as u16 {
                VK_TAB if count > 1 => {
                    let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
                    state.focus = if shift {
                        (state.focus + count - 1) % count
                    } else {
                        (state.focus + 1) % count
                    };
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
                VK_LEFT | VK_UP if count > 1 => {
                    state.focus = (state.focus + count - 1) % count;
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
                VK_RIGHT | VK_DOWN if count > 1 => {
                    state.focus = (state.focus + 1) % count;
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
                VK_RETURN => activate(state, hwnd, state.default_index),
                VK_ESCAPE => activate_cancel(state, hwnd),
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            activate_cancel(state, hwnd);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn in_rect(rect: &RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
}

fn hit_button(state: &DialogState, x: i32, y: i32) -> Option<usize> {
    state
        .buttons
        .iter()
        .position(|button| in_rect(&button.rect, x, y))
}

fn activate(state: &mut DialogState, hwnd: HWND, index: usize) {
    if let Some(button) = state.buttons.get(index) {
        unsafe {
            *state.result = button.id;
        }
    }
    unsafe { DestroyWindow(hwnd) };
}

fn activate_cancel(state: &mut DialogState, hwnd: HWND) {
    let index = state.cancel_index.unwrap_or(state.default_index);
    activate(state, hwnd, index);
}

fn paint_dialog(state: &mut DialogState, hwnd: HWND) {
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut paint);
        let mut client = RECT::default();
        GetClientRect(hwnd, &mut client);

        App::fill(hdc, client, DLG_BG);
        let header = RECT {
            left: 0,
            top: 0,
            right: client.right,
            bottom: state.header_height,
        };
        App::fill(hdc, header, DLG_HEADER);
        let accent = RECT {
            left: 0,
            top: 0,
            right: client.right,
            bottom: state.scale(3).max(2),
        };
        App::fill(hdc, accent, DLG_ACCENT);
        draw_border(hdc, client, DLG_BORDER);

        SetBkMode(hdc, TRANSPARENT as i32);

        // Title.
        SelectObject(hdc, state.title_font);
        SetTextColor(hdc, DLG_TEXT);
        let mut title_rect = RECT {
            left: state.pad,
            top: 0,
            right: state.close_rect.left - state.scale(6),
            bottom: state.header_height,
        };
        DrawTextW(
            hdc,
            state.title.as_ptr(),
            state.title.len() as i32,
            &mut title_rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        // Close glyph.
        SetTextColor(
            hdc,
            if state.hover_close {
                DLG_TEXT
            } else {
                DLG_MUTED
            },
        );
        let mut close_rect = state.close_rect;
        let close_glyph = utf16("\u{2715}");
        DrawTextW(
            hdc,
            close_glyph.as_ptr(),
            close_glyph.len() as i32,
            &mut close_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );

        draw_icon(hdc, state);

        // Message body.
        SelectObject(hdc, state.font);
        SetTextColor(hdc, DLG_TEXT);
        let mut text_rect = state.text_rect;
        DrawTextW(
            hdc,
            state.message.as_ptr(),
            state.message.len() as i32,
            &mut text_rect,
            DT_LEFT | DT_WORDBREAK | DT_EDITCONTROL | DT_NOPREFIX,
        );

        // Buttons.
        for (index, button) in state.buttons.iter().enumerate() {
            let is_primary = button.is_default;
            let hovered = state.hover == Some(index);
            let pressed = state.pressed == Some(index);
            let color = if pressed {
                DLG_BTN_PRESSED
            } else if is_primary {
                if hovered {
                    DLG_BTN_PRIMARY_HOVER
                } else {
                    DLG_BTN_PRIMARY
                }
            } else if hovered {
                DLG_BTN_HOVER
            } else {
                DLG_BTN
            };
            App::rounded_fill(hdc, button.rect, state.radius, color);
            let text_color = if is_primary {
                rgb(240, 246, 255)
            } else {
                DLG_TEXT
            };
            SetTextColor(hdc, text_color);
            let mut rect = button.rect;
            DrawTextW(
                hdc,
                button.label.as_ptr(),
                button.label.len() as i32,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            if state.focus == index {
                let pen = CreatePen(PS_SOLID, state.scale(1).max(1), DLG_ACCENT);
                let previous_pen = SelectObject(hdc, pen);
                let previous_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
                let inset = state.scale(2);
                Rectangle(
                    hdc,
                    button.rect.left - inset,
                    button.rect.top - inset,
                    button.rect.right + inset,
                    button.rect.bottom + inset,
                );
                SelectObject(hdc, previous_brush);
                SelectObject(hdc, previous_pen);
                DeleteObject(pen);
            }
        }
        EndPaint(hwnd, &paint);
    }
}

fn draw_border(hdc: HDC, rect: RECT, color: u32) {
    unsafe {
        let pen = CreatePen(PS_SOLID, 1, color);
        let previous_pen = SelectObject(hdc, pen);
        let previous_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
        Rectangle(hdc, 0, 0, rect.right, rect.bottom);
        SelectObject(hdc, previous_brush);
        SelectObject(hdc, previous_pen);
        DeleteObject(pen);
    }
}

fn draw_icon(hdc: HDC, state: &DialogState) {
    unsafe {
        let brush = CreateSolidBrush(state.icon.tint());
        let previous_brush = SelectObject(hdc, brush);
        let previous_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        Ellipse(
            hdc,
            state.icon_rect.left,
            state.icon_rect.top,
            state.icon_rect.right,
            state.icon_rect.bottom,
        );
        SelectObject(hdc, previous_brush);
        SelectObject(hdc, previous_pen);
        DeleteObject(brush);

        SelectObject(hdc, state.title_font);
        SetTextColor(hdc, rgb(10, 16, 28));
        let mut rect = state.icon_rect;
        let glyph = utf16(state.icon.glyph());
        DrawTextW(
            hdc,
            glyph.as_ptr(),
            glyph.len() as i32,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
    }
}
