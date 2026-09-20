use super::*;

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
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            app.advance_syntax(hwnd);
            app.paint(hwnd);
            0
        }
        WM_DPICHANGED => {
            app.set_dpi((wparam & 0xffff) as u32);
            app.transition = None;
            unsafe { KillTimer(hwnd, 3) };
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
            app.resize_terminal_to_fit(hwnd);
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
        WM_TIMER if wparam == 3 => {
            unsafe { InvalidateRect(hwnd, null(), 0) };
            0
        }
        WM_TIMER if wparam == 4 => {
            app.poll_workers(hwnd);
            0
        }
        WM_TIMER if wparam == 5 => {
            app.advance_sidebar(hwnd);
            0
        }
        WM_TIMER if wparam == 6 => {
            app.begin_mouse_hover(hwnd);
            0
        }
        LSP_EVENT_MESSAGE => {
            app.poll_lsp(hwnd);
            0
        }
        TERMINAL_EVENT_MESSAGE => {
            app.poll_terminal(hwnd);
            0
        }
        WM_SETCURSOR if (lparam as u32 & 0xffff) == HTCLIENT => {
            unsafe {
                let mut point = POINT::default();
                GetCursorPos(&mut point);
                ScreenToClient(hwnd, &mut point);
                let cursor = LoadCursorW(
                    null_mut(),
                    if app.terminal_resizing
                        || (app.terminal_visible
                            && (point.y - app.terminal_top(hwnd)).abs() <= app.scale(4))
                    {
                        IDC_SIZENS
                    } else if app.divider_dragging
                        || app.sidebar_dragging
                        || app.split_visible
                            && !app.welcome
                            && point.y >= app.scale(TAB_HEIGHT)
                            && (point.x - app.pane_divider(hwnd)).abs() <= app.scale(6)
                        || (app.sidebar_width > 0
                            && app.sidebar_started.is_none()
                            && (point.x - app.editor_left()).abs() <= app.scale(4))
                    {
                        IDC_SIZEWE
                    } else if app.welcome
                        || app.quick_open
                        || (app.side_view == SideView::Review && app.review_file.is_some())
                        || point.y < app.editor_top()
                        || point.x < app.editor_left()
                        || {
                            let mut rect = RECT::default();
                            GetClientRect(hwnd, &mut rect);
                            point.y
                                >= rect.bottom
                                    - app.scale(
                                        STATUS
                                            + if app.terminal_visible {
                                                app.terminal_height
                                            } else {
                                                0
                                            },
                                    )
                        }
                    {
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
            if (app.dragging
                || app.divider_dragging
                || app.sidebar_dragging
                || app.terminal_resizing
                || app.terminal_selecting)
                && wparam & 1 != 0
            {
                app.mouse_drag(
                    hwnd,
                    (lparam as u32 & 0xffff) as i16 as i32,
                    ((lparam as u32 >> 16) & 0xffff) as i16 as i32,
                );
            } else {
                app.mouse_hover_move(
                    hwnd,
                    (lparam as u32 & 0xffff) as i16 as i32,
                    ((lparam as u32 >> 16) & 0xffff) as i16 as i32,
                );
            }
            0
        }
        WM_LBUTTONUP => {
            app.dragging = false;
            app.divider_dragging = false;
            app.sidebar_dragging = false;
            app.terminal_resizing = false;
            // Selection itself (anchor/end) stays put so it's still visible
            // and copyable with Ctrl+Shift+C after releasing the mouse.
            app.terminal_selecting = false;
            unsafe {
                ReleaseCapture();
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = (wparam >> 16) as i16;
            let mut point = POINT::default();
            unsafe {
                GetCursorPos(&mut point);
                ScreenToClient(hwnd, &mut point);
            }
            let mut rect = RECT::default();
            unsafe { GetClientRect(hwnd, &mut rect) };
            if app.terminal_visible
                && point.x >= app.editor_left()
                && point.y >= app.terminal_top(hwnd)
            {
                app.scroll_terminal(hwnd, delta as i32);
                return 0;
            }
            if app.sidebar_width > 0 && point.x >= app.scale(RAIL) && point.x < app.editor_left() {
                if !app.explorer_visible {
                    return 0;
                }
                if app.side_view != SideView::Files {
                    let count = if app.side_view == SideView::Search {
                        app.search_results.len()
                    } else {
                        app.changes.len()
                    };
                    let rows = if app.side_view == SideView::Search {
                        48
                    } else {
                        EXPLORER_ROW
                    };
                    let visible = ((rect.bottom - app.scale(STATUS + 113)) / app.scale(rows).max(1))
                        .max(1) as usize;
                    let max = count.saturating_sub(visible);
                    app.panel_first = if delta > 0 {
                        app.panel_first.saturating_sub(3)
                    } else {
                        (app.panel_first + 3).min(max)
                    };
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    return 0;
                }
                let visible = ((rect.bottom - app.scale(STATUS + EXPLORER_TOP + 38))
                    / app.scale(EXPLORER_ROW).max(1))
                .max(1) as usize;
                let max_first = app.explorer_rows().len().saturating_sub(visible);
                app.explorer_first_row = if delta > 0 {
                    app.explorer_first_row.saturating_sub(3)
                } else {
                    (app.explorer_first_row + 3).min(max_first)
                };
                unsafe {
                    InvalidateRect(hwnd, null(), 0);
                }
                return 0;
            }
            if app.side_view == SideView::Review
                && app.review_file.is_some()
                && point.x >= app.editor_left()
            {
                let visible = ((rect.bottom - app.scale(STATUS) - app.editor_top())
                    / app.line_height.max(1))
                .max(1) as usize;
                let max = app.diff_rows.len().saturating_sub(visible);
                app.diff_first = if delta > 0 {
                    app.diff_first.saturating_sub(3)
                } else {
                    (app.diff_first + 3).min(max)
                };
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return 0;
            }
            if app.split_visible && point.x >= app.editor_left() && point.y >= app.editor_top() {
                let pane = usize::from(point.x >= app.pane_divider(hwnd));
                app.focus_pane(hwnd, pane);
            }
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
                app.save_session();
                app.stop_terminal_for_close();
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
        dialog::enable_native_dark_mode();
        let com_initialized = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) >= 0;
        let instance = GetModuleHandleW(null());
        let class = wide("LightLineWindow");
        let app_icons = AppIcons::new().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "LightLine icon could not be loaded",
            )
        })?;
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance,
            hIcon: app_icons.large,
            hIconSm: app_icons.small,
            hCursor: LoadCursorW(null_mut(), IDC_IBEAM),
            lpszClassName: class.as_ptr(),
            ..zeroed()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(io::Error::last_os_error());
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("LightLine").as_ptr(),
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
        let dark_titlebar: i32 = 1;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &dark_titlebar as *const i32 as *const std::ffi::c_void,
            size_of::<i32>() as u32,
        );
        let mut app = Box::new(RefCell::new(App::new(hwnd, app_icons.large)));
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
        } else {
            app.borrow_mut().restore_session(hwnd);
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        DeleteObject(app.borrow().font);
        DeleteObject(app.borrow().ui_font);
        DeleteObject(app.borrow().brand_font);
        if com_initialized {
            CoUninitialize();
        }
        Ok(())
    }
}
