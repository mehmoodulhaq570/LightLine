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

// Regions of the terminal header tab strip. Painted by render/terminal.rs and
// hit-tested by input.rs through the exact same layout, so the two never drift.
pub(super) enum TerminalHeaderHit {
    OutputTab,
    TerminalTab(usize),
    New,
    Kill,
    Hide,
    Body,
}

pub(super) struct TerminalHeaderLayout {
    pub(super) header_bottom: i32,
    pub(super) output: RECT,
    pub(super) terminals: Vec<RECT>,
    pub(super) plus: RECT,
    pub(super) kill: RECT,
    pub(super) hide: RECT,
}

impl App {
    // Output holds run/build results (cargo test, Run Python) in a dedicated
    // ManagedRun session on the shared `terminal` service; the interactive
    // shells live in `self.terminals`, one service each. Never conflate the
    // two: run output must not be typed into the user's shell.
    fn default_no_profile(tab: TerminalTab) -> bool {
        matches!(tab, TerminalTab::Output)
    }

    fn terminal_launch_request(&self, no_profile: bool) -> LaunchRequest {
        let cwd = self
            .workspace_root
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        // Strip canonicalize()'s \\?\ prefix so PowerShell's own prompt shows
        // an ordinary path instead of the extended form.
        let mut request = LaunchRequest::shell(PathBuf::from(display_path(&cwd)));
        if no_profile {
            request = request.without_profile();
        }
        request
    }

    // Create a fresh interactive shell pane, spawn its session, and make it
    // the active tab. Returns the new pane's index, or None on spawn failure.
    fn spawn_terminal_pane(&mut self, hwnd: HWND, no_profile: bool) -> Option<usize> {
        let request = self.terminal_launch_request(no_profile);
        let size = self.terminal_size_for(hwnd);
        let hwnd_value = hwnd as isize;
        let mut service = TerminalService::new(move || unsafe {
            PostMessageW(hwnd_value as HWND, TERMINAL_EVENT_MESSAGE, 0, 0);
        });
        match service.start(SessionKind::Shell, request, size) {
            Ok(id) => {
                self.terminal_counter += 1;
                let title = self.terminal_counter.to_string();
                self.terminals.push(TerminalPane {
                    title,
                    service,
                    id,
                    snapshot: None,
                    applied_size: Some(size),
                });
                self.terminal_active = self.terminals.len() - 1;
                Some(self.terminal_active)
            }
            Err(error) => {
                self.status = format!("Terminal could not start: {error}");
                None
            }
        }
    }

    // Snapshot backing the currently shown area (active shell or run output).
    fn active_snapshot(&self) -> Option<&Arc<Snapshot>> {
        match self.terminal_tab {
            TerminalTab::Terminal => self.terminals.get(self.terminal_active)?.snapshot.as_ref(),
            TerminalTab::Output => self.run_snapshot.as_ref(),
        }
    }

    // Add a new shell session, reveal the panel on it, and take keyboard
    // focus with the block cursor shown immediately.
    pub(super) fn new_terminal(&mut self, hwnd: HWND, no_profile: bool) {
        self.welcome = false;
        self.terminal_visible = true;
        self.terminal_tab = TerminalTab::Terminal;
        if self.spawn_terminal_pane(hwnd, no_profile).is_some() {
            self.focus_active_shell(hwnd);
        }
        self.update_title(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    fn focus_active_shell(&mut self, hwnd: HWND) {
        self.terminal_focus = !self.terminals.is_empty();
        if self.terminal_focus {
            // Beat the 530ms blink timer so the caret is visible the instant
            // focus lands in the terminal rather than after half a cycle.
            self.caret_on = true;
        }
        unsafe { SetFocus(hwnd) };
    }

    // Reveal the Terminal area: start the first shell if none exist yet,
    // otherwise just focus (keeping the already-running sessions alive).
    pub(super) fn open_terminal(&mut self, hwnd: HWND) {
        if self.terminals.is_empty() {
            self.new_terminal(hwnd, Self::default_no_profile(TerminalTab::Terminal));
            return;
        }
        self.welcome = false;
        self.terminal_visible = true;
        self.terminal_tab = TerminalTab::Terminal;
        self.terminal_active = self.terminal_active.min(self.terminals.len() - 1);
        self.focus_active_shell(hwnd);
        self.update_title(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    // Close the active shell: dropping its pane drops the service, which
    // signals stop and lets the owner thread reap the child. Hides the panel
    // once the last terminal is gone.
    pub(super) fn close_active_terminal(&mut self, hwnd: HWND) {
        if self.terminals.is_empty() {
            self.hide_terminal(hwnd);
            return;
        }
        let index = self.terminal_active.min(self.terminals.len() - 1);
        self.terminals.remove(index);
        if self.terminals.is_empty() {
            self.terminal_visible = false;
            self.terminal_focus = false;
            self.terminal_active = 0;
            self.keep_cursor_visible(hwnd);
        } else {
            self.terminal_active = index.min(self.terminals.len() - 1);
            self.focus_active_shell(hwnd);
        }
        self.update_title(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    // Recycle the active shell: spawn a replacement first, then drop the old
    // pane, so indices stay valid and the panel never flashes empty.
    pub(super) fn restart_terminal(&mut self, hwnd: HWND, no_profile: bool) {
        if self.terminals.is_empty() {
            self.new_terminal(hwnd, no_profile);
            return;
        }
        self.welcome = false;
        self.terminal_visible = true;
        self.terminal_tab = TerminalTab::Terminal;
        let old = self.terminal_active.min(self.terminals.len() - 1);
        if self.spawn_terminal_pane(hwnd, no_profile).is_some() {
            self.terminals.remove(old);
            self.terminal_active = self.terminals.len() - 1;
            self.focus_active_shell(hwnd);
            self.status = "Restarted terminal\u{2026}".into();
        }
        self.update_title(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn select_terminal(&mut self, hwnd: HWND, index: usize) {
        if index >= self.terminals.len() {
            return;
        }
        self.welcome = false;
        self.terminal_visible = true;
        self.terminal_tab = TerminalTab::Terminal;
        self.terminal_active = index;
        self.focus_active_shell(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    // Hide the panel without stopping any session, so shells keep running and
    // reappear where they left off. The far-right close and Esc both do this.
    pub(super) fn close_terminal(&mut self, hwnd: HWND) {
        self.hide_terminal(hwnd);
    }

    pub(super) fn hide_terminal(&mut self, hwnd: HWND) {
        self.terminal_visible = false;
        self.terminal_focus = false;
        self.keep_cursor_visible(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn toggle_terminal(&mut self, hwnd: HWND) {
        if self.terminal_visible {
            self.hide_terminal(hwnd);
        } else {
            self.open_terminal(hwnd);
        }
    }

    pub(super) fn switch_terminal_tab(&mut self, hwnd: HWND, tab: TerminalTab) {
        match tab {
            // Clicking Terminal should just work, the way opening the panel
            // does in any other editor: focus the shell, or start one if none.
            TerminalTab::Terminal => self.open_terminal(hwnd),
            // Output has nothing to auto-start; it only ever shows whatever a
            // run has already produced. It takes keyboard focus only while a
            // run is still live, so the program's own stdin prompts (e.g.
            // Python's input()) can be answered; a finished run is read-only.
            TerminalTab::Output => {
                self.terminal_tab = tab;
                self.terminal_focus = self.run_session.is_some();
                if self.terminal_focus {
                    self.caret_on = true;
                }
                unsafe { SetFocus(hwnd) };
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }
    }

    pub(super) fn focus_terminal(&mut self, hwnd: HWND) {
        self.terminal_visible = true;
        // A finished Output pane is read-only; clicking its body must not
        // start capturing keys. But while a run is still executing, clicking
        // it should let the user answer the program's own stdin prompts.
        if self.terminal_tab == TerminalTab::Terminal && !self.terminals.is_empty() {
            self.focus_active_shell(hwnd);
        } else {
            self.terminal_focus =
                self.terminal_tab == TerminalTab::Output && self.run_session.is_some();
            if self.terminal_focus {
                self.caret_on = true;
            }
            unsafe { SetFocus(hwnd) };
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    // Show (or create) the dedicated run session on the Output tab. Reusing a
    // still-running session means a new command queues behind whatever is
    // already executing there rather than silently killing it.
    fn ensure_run_session(&mut self, hwnd: HWND) -> Option<SessionId> {
        self.welcome = false;
        self.terminal_visible = true;
        self.terminal_tab = TerminalTab::Output;
        if let Some(id) = self.run_session
            && self
                .run_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.status.is_final())
        {
            let _ = self.terminal.remove(id);
            self.run_session = None;
            self.run_applied_size = None;
            self.run_snapshot = None;
        }
        if let Some(id) = self.run_session {
            self.focus_run_session(hwnd);
            return Some(id);
        }
        let request = self.terminal_launch_request(true);
        let size = self.terminal_size_for(hwnd);
        let started = self
            .terminal
            .start(SessionKind::ManagedRun, request, size)
            .map_err(|error| self.status = format!("Terminal could not start: {error}"));
        match started {
            Ok(id) => {
                self.run_session = Some(id);
                self.run_applied_size = Some(size);
                self.run_snapshot = None;
                self.update_title(hwnd);
                self.focus_run_session(hwnd);
                Some(id)
            }
            Err(()) => None,
        }
    }

    // Grab keyboard focus for the live run session so the user can answer a
    // program's own stdin prompts (e.g. Python's input()) without keystrokes
    // leaking into the code editor.
    fn focus_run_session(&mut self, hwnd: HWND) {
        self.terminal_focus = true;
        self.caret_on = true;
        unsafe { SetFocus(hwnd) };
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    // Drain terminal events for every shell pane plus the run session, keeping
    // the latest snapshot for each. Exited shells keep their final frame visible
    // (like VS Code) until the user closes the tab; only the run session is
    // auto-reaped on a final status.
    pub(super) fn poll_terminal(&mut self, hwnd: HWND) {
        let mut repaint = false;
        for pane in self.terminals.iter_mut() {
            for event in pane.service.poll(MAX_DRAIN_EVENTS) {
                pane.snapshot = Some(event.snapshot);
                repaint = true;
            }
        }
        for event in self.terminal.poll(MAX_DRAIN_EVENTS) {
            if Some(event.session_id) != self.run_session {
                continue;
            }
            let final_status = event.snapshot.status.is_final();
            self.run_snapshot = Some(event.snapshot);
            repaint = true;
            if final_status {
                let _ = self.terminal.remove(event.session_id);
                self.run_session = None;
                self.run_applied_size = None;
                if self.terminal_tab == TerminalTab::Output {
                    self.terminal_focus = false;
                }
            }
        }
        if repaint || !self.terminals.is_empty() || self.run_session.is_some() {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    // Header tab-strip geometry, shared by painting and hit testing so they
    // can never disagree about where a tab, `+`, kill, or hide button sits.
    pub(super) fn terminal_header_layout(
        &self,
        left: i32,
        right: i32,
        top: i32,
    ) -> TerminalHeaderLayout {
        let header_bottom = top + self.scale(TERMINAL_HEADER);
        let mut x = left + self.scale(16);
        let output = RECT {
            left: x,
            top,
            right: x + self.scale(78),
            bottom: header_bottom,
        };
        x += self.scale(78);
        let tab_width = self.scale(60);
        let gap = self.scale(6);
        let mut terminals = Vec::with_capacity(self.terminals.len());
        for _ in 0..self.terminals.len() {
            terminals.push(RECT {
                left: x + gap,
                top,
                right: x + gap + tab_width,
                bottom: header_bottom,
            });
            x += gap + tab_width;
        }
        let plus = RECT {
            left: x + gap,
            top,
            right: x + gap + self.scale(28),
            bottom: header_bottom,
        };
        x += gap + self.scale(34);
        let kill = RECT {
            left: x,
            top,
            right: x + self.scale(28),
            bottom: header_bottom,
        };
        let hide = RECT {
            left: right - self.scale(34),
            top,
            right: right - self.scale(6),
            bottom: header_bottom,
        };
        TerminalHeaderLayout {
            header_bottom,
            output,
            terminals,
            plus,
            kill,
            hide,
        }
    }

    pub(super) fn terminal_header_hit(&self, left: i32, right: i32, top: i32, x: i32, y: i32) -> TerminalHeaderHit {
        let layout = self.terminal_header_layout(left, right, top);
        let inside = |rect: &RECT| x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
        if inside(&layout.hide) {
            return TerminalHeaderHit::Hide;
        }
        if inside(&layout.output) {
            return TerminalHeaderHit::OutputTab;
        }
        for (index, rect) in layout.terminals.iter().enumerate() {
            if inside(rect) {
                return TerminalHeaderHit::TerminalTab(index);
            }
        }
        if inside(&layout.plus) {
            return TerminalHeaderHit::New;
        }
        if inside(&layout.kill) {
            return TerminalHeaderHit::Kill;
        }
        TerminalHeaderHit::Body
    }

    pub(super) fn terminal_top(&self, hwnd: HWND) -> i32 {
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) };
        (rect.bottom - self.scale(STATUS) - self.scale(self.terminal_height)).max(0)
    }

    // Pixel position to (column, row), matching the geometry paint_terminal
    // draws cells at, so hit-testing and rendering never drift apart.
    pub(super) fn terminal_cell_at(&self, hwnd: HWND, x: i32, y: i32) -> (u16, u16) {
        let cell_width = self.cell_width.max(1);
        let cell_height = self.line_height.max(1);
        let content_left = self.editor_left() + self.scale(TERMINAL_PAD);
        let header_bottom = self.terminal_top(hwnd) + self.scale(TERMINAL_HEADER);
        let column = ((x - content_left).max(0) / cell_width) as u16;
        let row = ((y - header_bottom).max(0) / cell_height) as u16;
        (column, row)
    }

    pub(super) fn start_terminal_selection(&mut self, hwnd: HWND, x: i32, y: i32) {
        let cell = self.terminal_cell_at(hwnd, x, y);
        self.terminal_selecting = true;
        self.terminal_select_anchor = Some(cell);
        self.terminal_select_end = Some(cell);
        unsafe {
            SetCapture(hwnd);
            InvalidateRect(hwnd, null(), 0);
        }
    }

    pub(super) fn update_terminal_selection(&mut self, hwnd: HWND, x: i32, y: i32) {
        if !self.terminal_selecting {
            return;
        }
        let cell = self.terminal_cell_at(hwnd, x, y);
        if self.terminal_select_end != Some(cell) {
            self.terminal_select_end = Some(cell);
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    // Normalized (start, end) with start always before end in reading order,
    // or None when there is no real selection (nothing dragged yet).
    pub(super) fn terminal_selection_range(&self) -> Option<((u16, u16), (u16, u16))> {
        let anchor = self.terminal_select_anchor?;
        let end = self.terminal_select_end?;
        if anchor == end {
            return None;
        }
        Some(if (anchor.1, anchor.0) <= (end.1, end.0) {
            (anchor, end)
        } else {
            (end, anchor)
        })
    }

    // Extracts the text between the selection's two corners, trimming
    // trailing padding spaces off each line the way a real terminal's copy does.
    fn terminal_selected_text(&self) -> Option<String> {
        let (start, finish) = self.terminal_selection_range()?;
        let snapshot = self.active_snapshot()?;
        let mut lines = Vec::new();
        for row_index in start.1..=finish.1 {
            let Some(row) = snapshot.rows.get(row_index as usize) else {
                break;
            };
            let from = if row_index == start.1 {
                start.0 as usize
            } else {
                0
            };
            let to = if row_index == finish.1 {
                (finish.0 as usize).min(row.cells.len())
            } else {
                row.cells.len()
            };
            let mut line = String::new();
            if from < to {
                for cell in &row.cells[from..to] {
                    if cell.wide_continuation {
                        continue;
                    }
                    if cell.text.is_empty() {
                        line.push(' ');
                    } else {
                        line.push_str(&cell.text);
                    }
                }
            }
            lines.push(line.trim_end().to_string());
        }
        let text = lines.join("\n");
        if text.is_empty() { None } else { Some(text) }
    }

    pub(super) fn copy_terminal_selection(&mut self, hwnd: HWND) {
        let Some(text) = self.terminal_selected_text() else {
            return;
        };
        match clipboard::copy(hwnd, &text) {
            Ok(()) => self.status = "Copied selection".into(),
            Err(error) => self.error(hwnd, &error),
        }
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
        for pane in self.terminals.iter_mut() {
            if pane.applied_size == Some(size) {
                continue;
            }
            if pane.service.resize(pane.id, size).is_ok() {
                pane.applied_size = Some(size);
            }
        }
        if self.run_applied_size != Some(size)
            && let Some(id) = self.run_session
            && self.terminal.resize(id, size).is_ok()
        {
            self.run_applied_size = Some(size);
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
        let Some(key) = vk_to_terminal_key(vk, control) else {
            return false;
        };
        let modifiers = TermModifiers {
            control,
            shift,
            alt,
        };
        self.reset_terminal_scrollback();
        let delivered = match self.terminal_tab {
            TerminalTab::Terminal => match self.terminals.get_mut(self.terminal_active) {
                Some(pane) => pane.service.key(pane.id, key, modifiers).is_ok(),
                None => false,
            },
            TerminalTab::Output => {
                self.run_session
                    .is_some_and(|id| self.terminal.key(id, key, modifiers).is_ok())
            }
        };
        if delivered {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
        delivered
    }

    pub(super) fn send_terminal_char(&mut self, hwnd: HWND, ch: char) {
        self.reset_terminal_scrollback();
        let mut buffer = [0u8; 4];
        let bytes = ch.encode_utf8(&mut buffer).as_bytes();
        let delivered = match self.terminal_tab {
            TerminalTab::Terminal => match self.terminals.get_mut(self.terminal_active) {
                Some(pane) => pane.service.input(pane.id, bytes).is_ok(),
                None => false,
            },
            TerminalTab::Output => self
                .run_session
                .is_some_and(|id| self.terminal.input(id, bytes).is_ok()),
        };
        if delivered {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    fn reset_terminal_scrollback(&mut self) {
        match self.terminal_tab {
            TerminalTab::Terminal => {
                if let Some(pane) = self.terminals.get_mut(self.terminal_active)
                    && pane
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.scrollback_offset != 0)
                {
                    let _ = pane.service.scrollback(pane.id, 0);
                }
            }
            TerminalTab::Output => {
                if let Some(id) = self.run_session
                    && self
                        .run_snapshot
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.scrollback_offset != 0)
                {
                    let _ = self.terminal.scrollback(id, 0);
                }
            }
        }
    }

    pub(super) fn scroll_terminal(&mut self, hwnd: HWND, delta: i32) {
        let Some(snapshot) = self.active_snapshot() else {
            return;
        };
        let current = snapshot.scrollback_offset;
        let available = snapshot.scrollback_available;
        let step = 3;
        let next = if delta > 0 {
            current.saturating_add(step).min(available)
        } else {
            current.saturating_sub(step)
        };
        if next == current {
            return;
        }
        let ok = match self.terminal_tab {
            TerminalTab::Terminal => match self.terminals.get(self.terminal_active) {
                Some(pane) => pane.service.scrollback(pane.id, next).is_ok(),
                None => false,
            },
            TerminalTab::Output => {
                self.run_session.is_some_and(|id| self.terminal.scrollback(id, next).is_ok())
            }
        };
        if ok {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    pub(super) fn paste_into_terminal(&mut self, hwnd: HWND) {
        let text = match clipboard::paste(hwnd) {
            Ok(Some(text)) => text,
            Ok(None) => return,
            Err(error) => {
                self.error(hwnd, &error);
                return;
            }
        };
        self.reset_terminal_scrollback();
        let sent = match self.terminal_tab {
            TerminalTab::Terminal => match self.terminals.get_mut(self.terminal_active) {
                Some(pane) => pane.service.paste(pane.id, &text).is_ok(),
                None => false,
            },
            TerminalTab::Output => self
                .run_session
                .is_some_and(|id| self.terminal.paste(id, &text).is_ok()),
        };
        if !sent {
            self.status = "Terminal is busy; try pasting again".into();
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

    // Stop every shell and the run session and hide the panel, e.g. when
    // switching workspaces: the old shell's cwd/venv no longer matches, so
    // nothing should carry over. Clearing `terminals` drops each service,
    // which signals stop and lets its owner thread reap the child.
    pub(super) fn reset_terminal_sessions(&mut self, hwnd: HWND) {
        self.stop_terminal_for_close();
        self.terminals.clear();
        self.terminal_active = 0;
        self.terminal_visible = false;
        self.terminal_focus = false;
        self.keep_cursor_visible(hwnd);
    }

    pub(super) fn stop_terminal_for_close(&mut self) {
        for pane in self.terminals.iter() {
            let _ = pane.service.stop(pane.id);
        }
        if let Some(id) = self.run_session {
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
