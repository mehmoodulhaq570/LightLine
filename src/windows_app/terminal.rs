use super::*;

// Posted from the terminal service wake callback; mirrors LSP_EVENT_MESSAGE.
pub(super) const TERMINAL_EVENT_MESSAGE: u32 = WM_APP + 8;

// Bottom panel geometry (logical pixels, scaled through App::scale). The
// panel's height is user-resizable (App::terminal_height); this is only its
// fixed header row.
const TERMINAL_HEADER: i32 = 34;
const TERMINAL_PAD: i32 = 8;

// A 16-entry ANSI palette tuned for the dark EDITOR_BG / SIDEBAR_BG surface.
const ANSI_PALETTE: [u32; 16] = [
    rgb(30, 34, 44),    // black
    rgb(205, 79, 79),   // red
    rgb(119, 221, 119), // green
    rgb(229, 200, 90),  // yellow
    rgb(96, 143, 244),  // blue
    rgb(190, 120, 224), // magenta
    rgb(92, 200, 214),  // cyan
    rgb(210, 218, 235), // white
    rgb(110, 120, 140), // bright black
    rgb(240, 100, 100), // bright red
    rgb(150, 240, 150), // bright green
    rgb(245, 220, 120), // bright yellow
    rgb(130, 170, 250), // bright blue
    rgb(215, 150, 245), // bright magenta
    rgb(130, 225, 235), // bright cyan
    rgb(240, 245, 252), // bright white
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
    fn session_for(&self, tab: TerminalTab) -> Option<SessionId> {
        match tab {
            TerminalTab::Terminal => self.shell_session,
            TerminalTab::Output => self.run_session,
        }
    }

    fn snapshot_for(&self, tab: TerminalTab) -> Option<&Arc<Snapshot>> {
        match tab {
            TerminalTab::Terminal => self.shell_snapshot.as_ref(),
            TerminalTab::Output => self.run_snapshot.as_ref(),
        }
    }

    fn set_session(&mut self, tab: TerminalTab, id: Option<SessionId>) {
        match tab {
            TerminalTab::Terminal => self.shell_session = id,
            TerminalTab::Output => self.run_session = id,
        }
    }

    fn set_snapshot(&mut self, tab: TerminalTab, snapshot: Option<Arc<Snapshot>>) {
        match tab {
            TerminalTab::Terminal => self.shell_snapshot = snapshot,
            TerminalTab::Output => self.run_snapshot = snapshot,
        }
    }

    fn set_applied_size(&mut self, tab: TerminalTab, size: Option<TerminalSize>) {
        match tab {
            TerminalTab::Terminal => self.shell_applied_size = size,
            TerminalTab::Output => self.run_applied_size = size,
        }
    }

    fn kind_for(tab: TerminalTab) -> SessionKind {
        match tab {
            TerminalTab::Terminal => SessionKind::Shell,
            TerminalTab::Output => SessionKind::ManagedRun,
        }
    }

    // Output is a fast, deterministic run session (build/script output), so it
    // skips profile scripts by default. Terminal is the user's actual shell,
    // so per spec it loads their normal profile unless explicitly restarted
    // without one for troubleshooting.
    fn default_no_profile(tab: TerminalTab) -> bool {
        matches!(tab, TerminalTab::Output)
    }

    // Ensure the session backing `tab` exists, show the panel on that tab, and
    // take keyboard focus. Shared by the interactive shell (Terminal) and run
    // output (Output) — they are always distinct sessions, never the same one.
    fn ensure_session(
        &mut self,
        hwnd: HWND,
        tab: TerminalTab,
        no_profile: bool,
    ) -> Option<SessionId> {
        self.welcome = false;
        self.terminal_visible = true;
        self.terminal_tab = tab;
        if let Some(id) = self.session_for(tab)
            && self
                .snapshot_for(tab)
                .is_some_and(|snapshot| snapshot.status.is_final())
        {
            let _ = self.terminal.remove(id);
            self.set_session(tab, None);
            self.set_applied_size(tab, None);
        }
        let id = match self.session_for(tab) {
            Some(id) => Some(id),
            None => {
                let cwd = self
                    .workspace_root
                    .clone()
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_else(|| PathBuf::from("."));
                // Strip canonicalize()'s \\?\ prefix so PowerShell's own
                // prompt shows an ordinary path instead of the extended form.
                let cwd = PathBuf::from(display_path(&cwd));
                let mut request = LaunchRequest::shell(cwd);
                if no_profile {
                    request = request.without_profile();
                }
                let size = self.terminal_size_for(hwnd);
                match self.terminal.start(Self::kind_for(tab), request, size) {
                    Ok(id) => {
                        self.set_session(tab, Some(id));
                        self.set_applied_size(tab, Some(size));
                        self.set_snapshot(tab, None);
                        Some(id)
                    }
                    Err(error) => {
                        self.status = format!("Terminal could not start: {error}");
                        None
                    }
                }
            }
        };
        // Output is a read-only log, like VS Code's Output panel: it still
        // runs on a real session underneath so streamed text keeps arriving,
        // but it never takes keyboard focus. Only Terminal is typable.
        if id.is_some() && tab == TerminalTab::Terminal {
            self.terminal_focus = true;
        }
        self.update_title(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
        id
    }

    // Show (or create) the persistent interactive user shell on the Terminal tab.
    pub(super) fn open_terminal(&mut self, hwnd: HWND) -> Option<SessionId> {
        self.ensure_session(
            hwnd,
            TerminalTab::Terminal,
            Self::default_no_profile(TerminalTab::Terminal),
        )
    }

    // Show (or reuse) the dedicated run session on the Output tab. Reusing a
    // still-running session means a new command queues behind whatever is
    // already executing there rather than silently killing it.
    fn ensure_run_session(&mut self, hwnd: HWND) -> Option<SessionId> {
        self.ensure_session(
            hwnd,
            TerminalTab::Output,
            Self::default_no_profile(TerminalTab::Output),
        )
    }

    // Recycle the Terminal session: stop it and, once poll_terminal finishes
    // reaping it, start a fresh one — optionally skipping the profile, e.g.
    // to recover from a broken profile script. If nothing is running yet,
    // this just starts one directly.
    pub(super) fn restart_terminal(&mut self, hwnd: HWND, no_profile: bool) {
        match self.shell_session {
            Some(id) => {
                let _ = self.terminal.stop(id);
                self.pending_terminal_restart = Some(no_profile);
                self.terminal_tab = TerminalTab::Terminal;
                self.terminal_visible = true;
                self.status = "Restarting terminal\u{2026}".into();
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            None => {
                self.ensure_session(hwnd, TerminalTab::Terminal, no_profile);
            }
        }
    }

    // Stop the active tab's session and hide the panel; reaped in poll_terminal.
    pub(super) fn close_terminal(&mut self, hwnd: HWND) {
        if let Some(id) = self.session_for(self.terminal_tab) {
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

    pub(super) fn switch_terminal_tab(&mut self, hwnd: HWND, tab: TerminalTab) {
        if self.terminal_tab == tab {
            return;
        }
        match tab {
            // Clicking Terminal should just work, the way opening the panel
            // does in any other editor: start the shell if none is running yet.
            TerminalTab::Terminal => {
                self.open_terminal(hwnd);
            }
            // Output has nothing to auto-start; it only ever shows whatever a
            // run has already produced, and never takes keyboard focus.
            TerminalTab::Output => {
                self.terminal_tab = tab;
                self.terminal_focus = false;
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }
    }

    pub(super) fn focus_terminal(&mut self, hwnd: HWND) {
        self.terminal_visible = true;
        // Output is read-only; clicking its body must not start capturing keys.
        self.terminal_focus = self.terminal_tab == TerminalTab::Terminal;
        unsafe {
            SetFocus(hwnd);
            InvalidateRect(hwnd, null(), 0);
        }
    }

    // Drain terminal events for both sessions; keep the latest snapshot for
    // each live session and reap one once its final frame arrives.
    pub(super) fn poll_terminal(&mut self, hwnd: HWND) {
        let events = self.terminal.poll(MAX_DRAIN_EVENTS);
        let mut repaint = false;
        for event in events {
            let tab = if Some(event.session_id) == self.shell_session {
                TerminalTab::Terminal
            } else if Some(event.session_id) == self.run_session {
                TerminalTab::Output
            } else {
                continue;
            };
            let final_status = event.snapshot.status.is_final();
            self.set_snapshot(tab, Some(event.snapshot));
            repaint = true;
            if final_status {
                if let Some(id) = self.session_for(tab) {
                    let _ = self.terminal.remove(id);
                }
                self.set_session(tab, None);
                self.set_applied_size(tab, None);
                if self.terminal_tab == tab {
                    self.terminal_focus = false;
                }
                if tab == TerminalTab::Terminal
                    && let Some(no_profile) = self.pending_terminal_restart.take()
                {
                    self.ensure_session(hwnd, TerminalTab::Terminal, no_profile);
                }
            }
        }
        if repaint || self.shell_session.is_some() || self.run_session.is_some() {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    pub(super) fn terminal_top(&self, hwnd: HWND) -> i32 {
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) };
        (rect.bottom - self.scale(STATUS) - self.scale(self.terminal_height)).max(0)
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
        let size = self.terminal_size_for(hwnd);
        for tab in [TerminalTab::Terminal, TerminalTab::Output] {
            let Some(id) = self.session_for(tab) else {
                continue;
            };
            if self.applied_size(tab) == Some(size) {
                continue;
            }
            if self.terminal.resize(id, size).is_ok() {
                self.set_applied_size(tab, Some(size));
            }
        }
    }

    fn applied_size(&self, tab: TerminalTab) -> Option<TerminalSize> {
        match tab {
            TerminalTab::Terminal => self.shell_applied_size,
            TerminalTab::Output => self.run_applied_size,
        }
    }

    // Returns true when the key was delivered to (or deliberately consumed by)
    // the session behind the currently active tab.
    pub(super) fn send_terminal_key(
        &mut self,
        hwnd: HWND,
        vk: u32,
        control: bool,
        shift: bool,
        alt: bool,
    ) -> bool {
        let Some(id) = self.session_for(self.terminal_tab) else {
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
        let Some(id) = self.session_for(self.terminal_tab) else {
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
        let tab = self.terminal_tab;
        if let Some(id) = self.session_for(tab)
            && self
                .snapshot_for(tab)
                .is_some_and(|snapshot| snapshot.scrollback_offset != 0)
        {
            let _ = self.terminal.scrollback(id, 0);
        }
    }

    pub(super) fn scroll_terminal(&mut self, hwnd: HWND, delta: i32) {
        let Some(id) = self.session_for(self.terminal_tab) else {
            return;
        };
        let (current, available) = self
            .snapshot_for(self.terminal_tab)
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
        let Some(id) = self.session_for(self.terminal_tab) else {
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

    // Runs `command` in the dedicated Output session, never the user's shell.
    pub(super) fn run_in_terminal(&mut self, hwnd: HWND, command: &str) {
        if let Some(id) = self.ensure_run_session(hwnd) {
            let mut line = command.to_string();
            line.push('\r');
            let _ = self.terminal.input(id, line.as_bytes());
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    // Stops both sessions (reaped asynchronously by poll_terminal, same as
    // close_terminal) and hides the panel, e.g. when switching workspaces: the
    // old shell's cwd/venv no longer matches, so nothing should carry over.
    pub(super) fn reset_terminal_sessions(&mut self, hwnd: HWND) {
        self.stop_terminal_for_close();
        self.terminal_visible = false;
        self.terminal_focus = false;
        self.keep_cursor_visible(hwnd);
    }

    pub(super) fn stop_terminal_for_close(&mut self) {
        for tab in [TerminalTab::Terminal, TerminalTab::Output] {
            if let Some(id) = self.session_for(tab) {
                let _ = self.terminal.stop(id);
            }
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
