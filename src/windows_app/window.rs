use super::*;

static EDITOR_WINDOW: AtomicIsize = AtomicIsize::new(0);
// Whether the last WM_SYSKEYDOWN was consumed by LightLine (see WM_SYSCHAR).
static SYSKEY_HANDLED: AtomicBool = AtomicBool::new(false);
// The WM_CHAR code (13 or 9) of an Enter/Tab the key handler consumed.
static CONSUMED_CHAR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

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
    crash::guard(hwnd, msg, wparam, lparam, || {
        handle_message(hwnd, msg, wparam, lparam)
    })
}

fn handle_message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    #[cfg(debug_assertions)]
    if msg == crash::CRASH_TEST_MESSAGE {
        panic!("crash test requested by a debug build's CRASH_TEST_MESSAGE");
    }
    // Remove the standard caption while retaining the thick resize frame.
    // LightLine paints and hit-tests its own workbench title bar below.
    if msg == WM_NCCALCSIZE {
        return 0;
    }
    if msg == WM_GETMINMAXINFO {
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        if !monitor.is_null() {
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..unsafe { zeroed() }
            };
            if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
                let minmax = unsafe { &mut *(lparam as *mut MINMAXINFO) };
                minmax.ptMaxPosition.x = info.rcWork.left - info.rcMonitor.left;
                minmax.ptMaxPosition.y = info.rcWork.top - info.rcMonitor.top;
                minmax.ptMaxSize.x = info.rcWork.right - info.rcWork.left;
                minmax.ptMaxSize.y = info.rcWork.bottom - info.rcWork.top;
                minmax.ptMaxTrackSize = minmax.ptMaxSize;
                return 0;
            }
        }
    }
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
        WM_NCHITTEST => {
            let default_hit = unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            if default_hit != HTCLIENT as LRESULT {
                return default_hit;
            }
            let mut point = POINT {
                x: (lparam as u32 & 0xffff) as i16 as i32,
                y: ((lparam as u32 >> 16) & 0xffff) as i16 as i32,
            };
            unsafe { ScreenToClient(hwnd, &mut point) };
            let mut rect = RECT::default();
            unsafe { GetClientRect(hwnd, &mut rect) };
            let controls_left = rect.right - app.scale(46 * 3);
            let command = app.command_center_rect(hwnd);
            let over_command = point.x >= command.left
                && point.x < command.right
                && point.y >= command.top
                && point.y < command.bottom;
            if point.y >= 0
                && point.y < app.chrome_top()
                && point.x >= app.scale(176)
                && point.x < controls_left
                && !over_command
            {
                HTCAPTION as LRESULT
            } else {
                HTCLIENT as LRESULT
            }
        }
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
                // Focus shows in more than the caret, so all of it is redrawn.
                InvalidateRect(hwnd, null(), 0);
            }
            app.poll_watcher(hwnd);
            app.recovery_tick();
            // Coming back to the window is when an outside commit, pull or
            // branch switch is most likely to have happened.
            app.refresh_git();
            0
        }
        WM_KILLFOCUS => {
            app.focused = false;
            app.dismiss_editor_context(hwnd);
            app.dismiss_more_menu(hwnd);
            unsafe {
                KillTimer(hwnd, 1);
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_TIMER if wparam == 1 => {
            if app.focused {
                app.caret_on = !app.caret_on;
                app.invalidate_blink(hwnd);
            }
            0
        }
        WM_TIMER if wparam == 2 => {
            app.advance_syntax(hwnd);
            // New colors only show in the code panes.
            let panes = app.editor_area(hwnd, false);
            unsafe {
                InvalidateRect(hwnd, &panes, 0);
            }
            0
        }
        WM_TIMER if wparam == 3 => {
            unsafe { InvalidateRect(hwnd, null(), 0) };
            0
        }
        WORKER_EVENT_MESSAGE => {
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
        WATCHER_EVENT_MESSAGE => {
            app.poll_watcher(hwnd);
            0
        }
        WM_TIMER if wparam == RECOVERY_TIMER => {
            unsafe { KillTimer(hwnd, RECOVERY_TIMER) };
            app.recovery_armed.set(false);
            app.recovery_tick();
            0
        }
        WM_TIMER if wparam == GUTTER_DIFF_TIMER => {
            // Changed-line marks only show in the code panes' gutters, and
            // outside git (or in an untracked file) they rarely change.
            let marks = |app: &App| {
                app.doc()
                    .path
                    .as_ref()
                    .and_then(|path| app.git_diff_cache.get(path).cloned())
            };
            let before = marks(&app);
            app.repaint_only(hwnd, &[], |app| app.start_gutter_diff());
            if marks(&app) != before {
                let panes = app.editor_area(hwnd, false);
                unsafe { InvalidateRect(hwnd, &panes, 0) };
            }
            0
        }
        WM_TIMER if wparam == MARKDOWN_TIMER => {
            unsafe {
                KillTimer(hwnd, MARKDOWN_TIMER);
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_TIMER if wparam == STATUS_TIMER => {
            unsafe {
                KillTimer(hwnd, STATUS_TIMER);
                InvalidateRect(hwnd, null(), 0);
            }
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
        DEBUG_EVENT_MESSAGE => {
            app.poll_debug(hwnd);
            0
        }
        AI_EVENT_MESSAGE => {
            app.poll_ai(hwnd);
            0
        }
        WM_SETCURSOR if (lparam as u32 & 0xffff) == HTCLIENT => {
            unsafe {
                let mut point = POINT::default();
                GetCursorPos(&mut point);
                ScreenToClient(hwnd, &mut point);
                if app.run_config_panel.is_some() {
                    app.run_config_hover(hwnd, point.x, point.y);
                    return 1;
                }
                let over_ai = app.ai_assistant_visible
                    && !app.welcome
                    && !app.quick_open
                    && point.x >= app.editor_right(hwnd);
                let over_ai_input = over_ai
                    && app.ai.hits.borrow().composer.is_some_and(|rect| {
                        point.x >= rect.left
                            && point.x < rect.right
                            && point.y >= rect.top
                            && point.y < rect.bottom
                    });
                let cursor = LoadCursorW(
                    null_mut(),
                    if over_ai_input {
                        IDC_IBEAM
                    } else if over_ai {
                        IDC_ARROW
                    } else if app.terminal_resizing
                        || (app.terminal_visible
                            && (point.y - app.terminal_top(hwnd)).abs() <= app.scale(4))
                    {
                        IDC_SIZENS
                    } else if app.divider_dragging
                        || app.sidebar_dragging
                        || app.split_divider_at(hwnd, point.x, point.y)
                        || (app.sidebar_width > 0
                            && app.sidebar_started.is_none()
                            && (point.x - app.editor_left()).abs() <= app.scale(4))
                    {
                        IDC_SIZEWE
                    } else if app.welcome
                        || app.quick_open
                        || app.scrollbar_grab.is_some()
                        || app.scrollbar_at(hwnd, point.x, point.y).is_some()
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
            let key = wparam as u32;
            if app.key(hwnd, key) {
                // The editor inserts newlines and tabs from WM_CHAR. When a
                // handler consumed Enter or Tab instead (running a palette
                // command, accepting a completion, confirming an Explorer
                // rename), the WM_CHAR TranslateMessage generates for it must
                // not reach the editor as an extra newline or tab.
                let consumed = if key == VK_RETURN as u32 {
                    13
                } else if key == VK_TAB as u32 {
                    9
                } else {
                    0
                };
                CONSUMED_CHAR.store(consumed, Ordering::Relaxed);
                0
            } else {
                CONSUMED_CHAR.store(0, Ordering::Relaxed);
                drop(app);
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
        // Alt chords and F10 arrive as WM_SYSKEYDOWN instead of WM_KEYDOWN,
        // so without this Shift+Alt+F (Format Document), F10 (Step Over) and
        // Alt keys in the terminal never reached the key handler. Everything
        // else -- Alt+F4, Alt+Space -- keeps Windows' default behavior.
        WM_SYSKEYDOWN => {
            let key = wparam as u32;
            let shift = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
            let routed = key == VK_F10 as u32
                || (shift && key == 0x46)
                // Alt+Enter: Replace All in the find box.
                || (key == VK_RETURN as u32 && app.find_mode)
                // Alt+Z: toggle word wrap, as in VS Code.
                || (key == 0x5A && !shift && !app.terminal_focus)
                // Alt+\: inline AI completion
                || ((key == VK_OEM_5 as u32 || key == 0xDC) && !shift && !app.terminal_focus)
                || (app.terminal_focus
                    && key != VK_F4 as u32
                    && key != VK_SPACE as u32
                    && key != VK_MENU as u32);
            let handled = routed && app.key(hwnd, key);
            SYSKEY_HANDLED.store(handled, Ordering::Relaxed);
            if handled {
                0
            } else {
                drop(app);
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
        // The WM_SYSCHAR that follows a handled Alt chord would otherwise
        // make Windows play its "no such menu" error sound.
        WM_SYSCHAR if SYSKEY_HANDLED.swap(false, Ordering::Relaxed) => 0,
        WM_CHAR => {
            let consumed = CONSUMED_CHAR.swap(0, Ordering::Relaxed);
            if consumed == 0 || consumed != wparam as u32 {
                app.character(hwnd, wparam as u16);
            }
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
                || app.scrollbar_grab.is_some()
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
            app.run_config_end_drag();
            if app.scrollbar_grab.take().is_some() {
                app.invalidate_scrollbar(hwnd, app.focused_pane);
                // Released away from the scrollbar, it's no longer under the
                // mouse; the move that would say so came while it was held.
                // Released on it, it is, and the mouse capture may have ended
                // the watch for the mouse leaving the window: set it again.
                let over = app.scrollbar_at(
                    hwnd,
                    (lparam as u32 & 0xffff) as i16 as i32,
                    ((lparam as u32 >> 16) & 0xffff) as i16 as i32,
                );
                app.scrollbar_hover = None;
                app.set_scrollbar_hover(hwnd, over);
            }
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
        WM_CAPTURECHANGED | WM_CANCELMODE => {
            app.run_config_end_drag();
            // WM_LBUTTONUP has already cleared these before its ReleaseCapture
            // sends WM_CAPTURECHANGED, so only repaint when a drag was cut short.
            let was_dragging = app.dragging
                || app.divider_dragging
                || app.sidebar_dragging
                || app.terminal_resizing
                || app.terminal_selecting;
            app.dragging = false;
            app.divider_dragging = false;
            app.sidebar_dragging = false;
            app.terminal_resizing = false;
            app.terminal_selecting = false;
            if app.scrollbar_grab.take().is_some() {
                app.invalidate_scrollbar(hwnd, app.focused_pane);
            }
            if was_dragging {
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            0
        }
        WM_MOUSELEAVE => {
            app.set_scrollbar_hover(hwnd, None);
            0
        }
        WM_RBUTTONUP => {
            let x = (lparam as u32 & 0xffff) as i16 as i32;
            let y = ((lparam as u32 >> 16) & 0xffff) as i16 as i32;
            app.mouse_right_click(hwnd, x, y);
            0
        }
        WM_MOUSEWHEEL => {
            if app.run_choice.is_some() {
                app.run_choice_key(
                    hwnd,
                    if (wparam >> 16) as i16 > 0 {
                        VK_UP as u32
                    } else {
                        VK_DOWN as u32
                    },
                );
                return 0;
            }
            if app.run_config_panel.is_some() {
                let mut point = POINT::default();
                unsafe {
                    GetCursorPos(&mut point);
                    ScreenToClient(hwnd, &mut point);
                }
                app.run_config_scroll(hwnd, (wparam >> 16) as i16 as i32, point.x);
                return 0;
            }
            if app.editor_context.is_some() {
                return 0;
            }
            let delta = (wparam >> 16) as i16;
            let mut point = POINT::default();
            unsafe {
                GetCursorPos(&mut point);
                ScreenToClient(hwnd, &mut point);
            }
            if app.quick_open {
                app.scroll_quick_open(hwnd, delta as i32);
                return 0;
            }
            // Over the Assistant panel the wheel scrolls the conversation,
            // never the editor or terminal beside it.
            if app.ai_assistant_visible && !app.welcome && point.x >= app.editor_right(hwnd) {
                app.ai_scroll(hwnd, delta as i32);
                return 0;
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
                if app.side_view == SideView::Settings {
                    app.scroll_settings_panel(hwnd, delta as i32);
                    return 0;
                }
                if app.side_view != SideView::Files {
                    // Every list scrolls by whole rows, and the source control
                    // list mixes headers, files and commits, so its bound and
                    // its page size come from the row model itself.
                    let (count, visible) = match app.side_view {
                        SideView::Search => (
                            app.search_results.len(),
                            ((rect.bottom - app.scale(STATUS + WORKBENCH_HEADER + 113))
                                / app.scale(48).max(1))
                            .max(1) as usize,
                        ),
                        SideView::Review => (app.git_rows().len(), app.git_visible_rows(hwnd)),
                        _ => (
                            app.changes.len(),
                            ((rect.bottom - app.scale(STATUS + WORKBENCH_HEADER + 113))
                                / app.scale(EXPLORER_ROW).max(1))
                            .max(1) as usize,
                        ),
                    };
                    let max = count.saturating_sub(visible);
                    app.panel_first = if delta > 0 {
                        app.panel_first.saturating_sub(3)
                    } else {
                        (app.panel_first + 3).min(max)
                    };
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    return 0;
                }
                let visible = ((rect.bottom
                    - app.scale(STATUS + WORKBENCH_HEADER + EXPLORER_TOP + 38))
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
            // The wheel scrolls the pane under the mouse, as in VS Code, and
            // leaves the focus where it is. Focusing that pane switched the
            // active tab: scrolling a preview beside its file hid the file's
            // preview button, and closed the find box.
            let pane =
                if app.split_visible && point.x >= app.editor_left() && point.y >= app.editor_top()
                {
                    usize::from(point.x >= app.pane_divider(hwnd))
                } else {
                    app.focused_pane
                };
            // A Markdown preview scrolls by pixels, not document lines.
            if app.tabs[app.tab_for_pane(pane)].markdown.is_some() {
                let pixels = -(delta as i32) * app.scale(48) / 120;
                app.scroll_markdown(hwnd, pane, pixels);
                return 0;
            }
            // Scroll by screen rows: a wrapped line has several, a folded
            // block one.
            if delta != 0 {
                let rows = if delta > 0 { -3 } else { 3 };
                app.scroll_pane_rows(hwnd, pane, rows);
            }
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_CLOSE => {
            if app.can_close_window(hwnd) {
                if !app.flush_session(hwnd) {
                    return 0;
                }
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
    crash::install_hook();
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
            WS_EX_APPWINDOW,
            class.as_ptr(),
            wide("LightLine").as_ptr(),
            WS_THICKFRAME,
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
        let mut app = Box::new(RefCell::new(App::new(
            hwnd,
            app_icons.large,
            app_icons.hero,
        )));
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            (&mut *app as *mut RefCell<App>) as isize,
        );
        connect_parent_console(hwnd);
        app.borrow().update_title(hwnd);
        ShowWindow(hwnd, SW_SHOW);
        SetFocus(hwnd);
        if let Some(path) = std::env::args_os().nth(1) {
            if workflow::load_session()
                .tabs
                .iter()
                .any(|tab| tab.recovery.is_some())
            {
                app.borrow_mut().restore_session(hwnd);
            }
            app.borrow_mut().open(hwnd, Some(PathBuf::from(path)));
        } else {
            app.borrow_mut().restore_session(hwnd);
        }
        // No shell starts here: launching PowerShell (and its profile) on every
        // start cost startup time, hid the Welcome screen and took keyboard
        // focus from the editor. Ctrl+` starts the first terminal on demand.
        // Reported only now: opening the startup file or session sets its own
        // status, which would otherwise replace this straight away.
        if let Err(error) = lightline::settings::Settings::try_load() {
            app.borrow_mut().status = error;
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
