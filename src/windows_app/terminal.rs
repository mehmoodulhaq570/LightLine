use super::*;

// Posted from the terminal service wake callback; mirrors LSP_EVENT_MESSAGE.
pub(super) const TERMINAL_EVENT_MESSAGE: u32 = WM_APP + 8;

// Bottom panel geometry (logical pixels, scaled through App::scale).
pub(super) const TERMINAL_HEIGHT: i32 = 210;
const TERMINAL_HEADER: i32 = 34;
const TERMINAL_PAD: i32 = 8;

// A 16-entry ANSI palette tuned for the dark EDITOR_BG / SIDEBAR_BG surface.
const ANSI_PALETTE: [u32; 16] = [
    rgb(30, 34, 44),  // black
    rgb(205, 79, 79), // red
    rgb(119, 221, 119),// green
    rgb(229, 200, 90), // yellow
    rgb(96, 143, 244), // blue
    rgb(190, 120, 224),// magenta
    rgb(92, 200, 214), // cyan
    rgb(210, 218, 235),// white
    rgb(110, 120, 140),// bright black
    rgb(240, 100, 100),// bright red
    rgb(150, 240, 150),// bright green
    rgb(245, 220, 120),// bright yellow
    rgb(130, 170, 250),// bright blue
    rgb(215, 150, 245),// bright magenta
    rgb(130, 225, 235),// bright cyan
    rgb(240, 245, 252),// bright white
];

fn brighten(color: u32) -> u32 {
    let lift = |channel: u32| (channel + (255 - channel) / 2).min(255) as u8;
    rgb(
        lift(color & 0xff),
        lift((color >> 8) & 0xff),
        lift((color >> 16) & 0xff),
    )
}

fn ansi_color(index: u8) -> u32 {
    match index {
        0..=15 => ANSI_PALETTE[index as usize],
        16..=231 => {
            let value = index - 16;
            let channel = |component: u8| {
                if component == 0 {
                    0
                } else {
                    (55 + u32::from(component) * 40).min(255) as u8
                }
            };
            rgb(
                channel(value / 36),
                channel((value / 6) % 6),
                channel(value % 6),
            )
        }
        _ => {
            let gray = (8 + u32::from(index - 232) * 10).min(255) as u8;
            rgb(gray, gray, gray)
        }
    }
}

// Map a cell color to a GDI color. Default resolves to the panel palette; a bold
// foreground promotes the base 8 ANSI colors to their bright variants.
pub(super) fn cell_colors(cell: &Cell) -> (u32, u32) {
    let mut foreground = match cell.foreground {
        TermColor::Default => TEXT,
        TermColor::Idx(index) if cell.bold && index < 8 => ansi_color(index + 8),
        TermColor::Idx(index) => ansi_color(index),
        TermColor::Rgb(red, green, blue) => rgb(red, green, blue),
    };
    let mut background = match cell.background {
        TermColor::Default => SIDEBAR_BG,
        TermColor::Idx(index) => ansi_color(index),
        TermColor::Rgb(red, green, blue) => rgb(red, green, blue),
    };
    if cell.dim && !matches!(cell.foreground, TermColor::Default) {
        foreground = brighten(background).min(foreground);
        foreground = dim_color(foreground);
    }
    if cell.inverse {
        std::mem::swap(&mut foreground, &mut background);
    }
    (foreground, background)
}

fn dim_color(color: u32) -> u32 {
    let half = |channel: u32| (channel / 2) as u8;
    rgb(
        half(color & 0xff),
        half((color >> 8) & 0xff),
        half((color >> 16) & 0xff),
    )
}

// Quote a path as a PowerShell literal string (single quotes, doubled to escape)
// so it can be typed safely into the interactive shell.
pub(super) fn powershell_quoted(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('\'');
    for ch in raw.chars() {
        if ch == '\'' {
            out.push('\'');
        }
        out.push(ch);
    }
    out.push('\'');
    out
}

fn vk_to_terminal_key(vk: u32, control: bool) -> Option<TermKey> {
    if control && (0x41..=0x5a).contains(&vk) {
        let letter = (vk as u8 - b'A' + b'a') as char;
        return Some(TermKey::Char(letter));
    }
    if (0x70..=0x7b).contains(&vk) {
        return Some(TermKey::Function((vk - 0x70 + 1) as u8));
    }
    let key = if vk == VK_RETURN as u32 {
        TermKey::Enter
    } else if vk == VK_TAB as u32 {
        TermKey::Tab
    } else if vk == VK_BACK as u32 {
        TermKey::Backspace
    } else if vk == VK_ESCAPE as u32 {
        TermKey::Escape
    } else if vk == VK_UP as u32 {
        TermKey::Up
    } else if vk == VK_DOWN as u32 {
        TermKey::Down
    } else if vk == VK_LEFT as u32 {
        TermKey::Left
    } else if vk == VK_RIGHT as u32 {
        TermKey::Right
    } else if vk == VK_HOME as u32 {
        TermKey::Home
    } else if vk == VK_END as u32 {
        TermKey::End
    } else if vk == VK_INSERT as u32 {
        TermKey::Insert
    } else if vk == VK_DELETE as u32 {
        TermKey::Delete
    } else if vk == VK_PRIOR as u32 {
        TermKey::PageUp
    } else if vk == VK_NEXT as u32 {
        TermKey::PageDown
    } else {
        return None;
    };
    Some(key)
}

impl App {
    // Ensure a shell session exists, show the panel, and take keyboard focus.
    pub(super) fn open_terminal(&mut self, hwnd: HWND) -> Option<SessionId> {
        self.welcome = false;
        self.terminal_visible = true;
        if let Some(id) = self.terminal_session
            && self
                .terminal_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.status.is_final())
        {
            let _ = self.terminal.remove(id);
            self.terminal_session = None;
            self.terminal_applied_size = None;
        }
        let id = match self.terminal_session {
            Some(id) => Some(id),
            None => {
                let cwd = self
                    .workspace_root
                    .clone()
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_else(|| PathBuf::from("."));
                let request = LaunchRequest::shell(cwd).without_profile();
                let size = self.terminal_size_for(hwnd);
                match self.terminal.start(SessionKind::Shell, request, size) {
                    Ok(id) => {
                        self.terminal_session = Some(id);
                        self.terminal_applied_size = Some(size);
                        self.terminal_snapshot = None;
                        Some(id)
                    }
                    Err(error) => {
                        self.status = format!("Terminal could not start: {error}");
                        None
                    }
                }
            }
        };
        if id.is_some() {
            self.terminal_focus = true;
        }
        self.update_title(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
        id
    }

    // Stop the running shell and hide the panel; the session is reaped in poll_terminal.
    pub(super) fn close_terminal(&mut self, hwnd: HWND) {
        if let Some(id) = self.terminal_session {
            let _ = self.terminal.stop(id);
        }
        self.terminal_visible = false;
        self.terminal_focus = false;
        self.keep_cursor_visible(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn hide_terminal(&mut self, hwnd: HWND) {
        self.terminal_visible = false;
        self.terminal_focus = false;
        self.keep_cursor_visible(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn toggle_terminal(&mut self, hwnd: HWND) {
        if self.terminal_visible {
            self.terminal_visible = false;
            self.terminal_focus = false;
            self.keep_cursor_visible(hwnd);
            unsafe { InvalidateRect(hwnd, null(), 0) };
        } else {
            self.open_terminal(hwnd);
        }
    }

    pub(super) fn focus_terminal(&mut self, hwnd: HWND) {
        self.terminal_visible = true;
        self.terminal_focus = true;
        unsafe {
            SetFocus(hwnd);
            InvalidateRect(hwnd, null(), 0);
        }
    }

    // Drain terminal events; keep the latest snapshot for the live session and reap
    // a session once its final frame arrives.
    pub(super) fn poll_terminal(&mut self, hwnd: HWND) {
        let events = self.terminal.poll(MAX_DRAIN_EVENTS);
        let mut latest: Option<Arc<Snapshot>> = None;
        for event in events {
            if Some(event.session_id) == self.terminal_session {
                latest = Some(event.snapshot);
            }
        }
        let mut repaint = false;
        if let Some(snapshot) = latest {
            let final_status = snapshot.status.is_final();
            let id = snapshot.session_id;
            self.terminal_snapshot = Some(snapshot);
            repaint = true;
            if final_status {
                let _ = self.terminal.remove(id);
                self.terminal_session = None;
                self.terminal_applied_size = None;
                self.terminal_focus = false;
            }
        }
        if repaint || self.terminal_session.is_some() {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    pub(super) fn terminal_top(&self, hwnd: HWND) -> i32 {
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) };
        (rect.bottom - self.scale(STATUS) - self.scale(TERMINAL_HEIGHT)).max(0)
    }

    pub(super) fn terminal_size_for(&self, hwnd: HWND) -> TerminalSize {
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) };
        let cell_width = self.measure_cell_width(hwnd).max(1);
        let cell_height = self.line_height.max(1);
        let left = self.editor_left() + self.scale(TERMINAL_PAD);
        let right = rect.right - self.scale(TERMINAL_PAD);
        let top = self.terminal_top(hwnd) + self.scale(TERMINAL_HEADER);
        let bottom = rect.bottom - self.scale(STATUS) - self.scale(TERMINAL_PAD);
        let columns = ((right - left).max(1) / cell_width).clamp(1, 400) as u16;
        let rows = ((bottom - top).max(1) / cell_height).clamp(1, 200) as u16;
        TerminalSize::new(rows, columns).unwrap_or_default()
    }

    pub(super) fn resize_terminal_to_fit(&mut self, hwnd: HWND) {
        let Some(id) = self.terminal_session else {
            return;
        };
        let size = self.terminal_size_for(hwnd);
        if self.terminal_applied_size == Some(size) {
            return;
        }
        if self.terminal.resize(id, size).is_ok() {
            self.terminal_applied_size = Some(size);
        }
    }

    // Returns true when the key was delivered to (or deliberately consumed by) the shell.
    pub(super) fn send_terminal_key(
        &mut self,
        hwnd: HWND,
        vk: u32,
        control: bool,
        shift: bool,
        alt: bool,
    ) -> bool {
        let Some(id) = self.terminal_session else {
            return false;
        };
        let Some(key) = vk_to_terminal_key(vk, control) else {
            return false;
        };
        let modifiers = TermModifiers {
            control,
            shift,
            alt,
        };
        self.reset_terminal_scrollback();
        if self.terminal.key(id, key, modifiers).is_ok() {
            unsafe { InvalidateRect(hwnd, null(), 0) };
            true
        } else {
            false
        }
    }

    pub(super) fn send_terminal_char(&mut self, hwnd: HWND, ch: char) {
        let Some(id) = self.terminal_session else {
            return;
        };
        self.reset_terminal_scrollback();
        let mut buffer = [0u8; 4];
        let bytes = ch.encode_utf8(&mut buffer).as_bytes();
        if self.terminal.input(id, bytes).is_ok() {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    fn reset_terminal_scrollback(&mut self) {
        if let Some(id) = self.terminal_session {
            if self
                .terminal_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.scrollback_offset != 0)
            {
                let _ = self.terminal.scrollback(id, 0);
            }
        }
    }

    pub(super) fn scroll_terminal(&mut self, hwnd: HWND, delta: i32) {
        let Some(id) = self.terminal_session else {
            return;
        };
        let (current, available) = self
            .terminal_snapshot
            .as_ref()
            .map_or((0, 0), |snapshot| {
                (snapshot.scrollback_offset, snapshot.scrollback_available)
            });
        let step = 3;
        let next = if delta > 0 {
            current.saturating_add(step).min(available)
        } else {
            current.saturating_sub(step)
        };
        if next != current && self.terminal.scrollback(id, next).is_ok() {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    pub(super) fn paste_into_terminal(&mut self, hwnd: HWND) {
        let Some(id) = self.terminal_session else {
            return;
        };
        match clipboard::paste(hwnd) {
            Ok(Some(text)) => {
                self.reset_terminal_scrollback();
                if self.terminal.paste(id, &text).is_err() {
                    self.status = "Terminal is busy; try pasting again".into();
                }
            }
            Ok(None) => {}
            Err(error) => self.error(hwnd, &error),
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn run_in_terminal(&mut self, hwnd: HWND, command: &str) {
        if let Some(id) = self.open_terminal(hwnd) {
            let mut line = command.to_string();
            line.push('\r');
            let _ = self.terminal.input(id, line.as_bytes());
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn stop_terminal_for_close(&mut self) {
        if let Some(id) = self.terminal_session {
            let _ = self.terminal.stop(id);
        }
    }

    pub(super) fn refresh_terminal_cell_width(&mut self, hdc: HDC) {
        let old = unsafe { SelectObject(hdc, self.font) };
        let width = self.text_width(hdc, "M");
        unsafe { SelectObject(hdc, old) };
        if width > 0 {
            self.cell_width = width;
        }
    }

    fn measure_cell_width(&self, hwnd: HWND) -> i32 {
        if self.cell_width > 0 {
            return self.cell_width;
        }
        unsafe {
            let hdc = GetDC(hwnd);
            let old = SelectObject(hdc, self.font);
            let width = self.text_width(hdc, "M");
            SelectObject(hdc, old);
            ReleaseDC(hwnd, hdc);
            width.max(1)
        }
    }
}
