use super::*;

// Posted from the terminal service wake callback; mirrors LSP_EVENT_MESSAGE.
pub(super) const TERMINAL_EVENT_MESSAGE: u32 = WM_APP + 8;

// Bottom panel geometry (logical pixels, scaled through App::scale). The
// panel's height is user-resizable (App::terminal_height); this is only its
// fixed header row.
pub(super) const TERMINAL_HEADER: i32 = 34;
pub(super) const TERMINAL_PAD: i32 = 8;

fn brighten(color: u32) -> u32 {
    let lift = |channel: u32| (channel + (255 - channel) / 2).min(255) as u8;
    rgb(
        lift(color & 0xff),
        lift((color >> 8) & 0xff),
        lift((color >> 16) & 0xff),
    )
}

// The first 16 colors come from the theme (`Theme::ansi`); the 6x6x6
// cube and the grey ramp above them are fixed by the xterm standard.
fn ansi_color(index: u8, palette: &[u32; 16]) -> u32 {
    match index {
        0..=15 => palette[index as usize],
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
pub(super) fn cell_colors(
    cell: &Cell,
    default_fg: u32,
    default_bg: u32,
    palette: &[u32; 16],
) -> (u32, u32) {
    let mut foreground = match cell.foreground {
        TermColor::Default => default_fg,
        TermColor::Idx(index) if cell.bold && index < 8 => ansi_color(index + 8, palette),
        TermColor::Idx(index) => ansi_color(index, palette),
        TermColor::Rgb(red, green, blue) => rgb(red, green, blue),
    };
    let mut background = match cell.background {
        TermColor::Default => default_bg,
        TermColor::Idx(index) => ansi_color(index, palette),
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
    powershell_literal(&path.to_string_lossy())
}

// Quote text as a PowerShell single-quoted literal. PowerShell also ends such
// a string at the typographic quotes ‘ ’ ‚ ‛, so those are
// doubled too.
pub(super) fn powershell_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for ch in text.chars() {
        if matches!(ch, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
            out.push(ch);
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

fn output_control_bytes(key: TermKey, modifiers: TermModifiers) -> Option<Vec<u8>> {
    match key {
        TermKey::Enter => Some(b"\r\n".to_vec()),
        TermKey::Backspace => Some(lightline::terminal::encode_key(
            key,
            modifiers,
            lightline::terminal::InputModes::default(),
        )),
        _ => None,
    }
}

// Regions of the terminal header tab strip. Painted by render/terminal.rs and
// hit-tested by input.rs through the exact same layout, so the two never drift.
pub(super) enum TerminalHeaderHit {
    Problems,
    OutputTab,
    TerminalTab(usize),
    New,
    ShellPicker,
    Kill,
    Hide,
    Body,
}

const MAX_TERMINAL_NAME_CHARS: usize = 48;

fn normalized_terminal_name(name: &str) -> Option<String> {
    let name: String = name
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(MAX_TERMINAL_NAME_CHARS)
        .collect();
    (!name.trim().is_empty()).then_some(name)
}

fn set_terminal_pane_name(
    terminals: &mut [TerminalPane],
    session_id: SessionId,
    name: String,
) -> bool {
    let Some(pane) = terminals.iter_mut().find(|pane| pane.id == session_id) else {
        return false;
    };
    pane.title = name;
    pane.custom_title = true;
    true
}

fn terminal_pane_index(terminals: &[TerminalPane], session_id: SessionId) -> Option<usize> {
    terminals.iter().position(|pane| pane.id == session_id)
}

fn clear_terminal_rename_for_session(
    rename_input: &mut Option<(SessionId, String)>,
    session_id: SessionId,
) {
    if rename_input
        .as_ref()
        .is_some_and(|(target, _)| *target == session_id)
    {
        *rename_input = None;
    }
}

pub(super) struct TerminalHeaderLayout {
    pub(super) header_bottom: i32,
    pub(super) problems: RECT,
    pub(super) output: RECT,
    pub(super) terminals: Vec<RECT>,
    pub(super) plus: RECT,
    pub(super) chevron: RECT,
    pub(super) kill: RECT,
    pub(super) hide: RECT,
}

pub(super) struct TerminalProfileMenuLayout {
    pub(super) rect: RECT,
    pub(super) shell_rows: Vec<(ShellKind, RECT)>,
    pub(super) settings: Option<RECT>,
    pub(super) footer: RECT,
}

pub(super) enum TerminalProfileMenuHit {
    Shell(ShellKind),
    Settings,
    Back,
    None,
}

impl App {
    // Output holds run/build results (cargo test, Run Python) in a dedicated
    // ManagedRun session on the shared `terminal` service; the interactive
    // shells live in `self.terminals`, one service each. Never conflate the
    // two: run output must not be typed into the user's shell.
    fn default_no_profile(tab: TerminalTab) -> bool {
        matches!(tab, TerminalTab::Output)
    }

    fn terminal_launch_request(&self, shell_kind: ShellKind, no_profile: bool) -> LaunchRequest {
        let cwd = self
            .workspace_root
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        // Strip canonicalize()'s \\?\ prefix so the shell prompt shows
        // an ordinary path instead of the extended form.
        let mut request = LaunchRequest::with_shell(PathBuf::from(display_path(&cwd)), shell_kind);
        if no_profile {
            request = request.without_profile();
        }
        request
    }

    // Create a fresh interactive shell pane, spawn its session, and make it
    // the active tab. Returns the new pane's index, or None on spawn failure.
    fn spawn_terminal_pane(
        &mut self,
        hwnd: HWND,
        shell_kind: ShellKind,
        no_profile: bool,
    ) -> Option<usize> {
        let request = self.terminal_launch_request(shell_kind, no_profile);
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
                    custom_title: false,
                    service,
                    id,
                    snapshot: None,
                    applied_size: Some(size),
                    shell_kind,
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

    // Add a new shell session using the default profile, reveal the panel on it,
    // and take keyboard focus with the block cursor shown immediately.
    pub(super) fn new_terminal(&mut self, hwnd: HWND, no_profile: bool) {
        let mut shell_kind = self.settings.default_terminal_profile;
        // A default profile that has since become unavailable (WSL
        // uninstalled, Git Bash removed) falls back to PowerShell rather
        // than leaving the panel without a working shell.
        if shell_kind != ShellKind::PowerShell && !shell_kind.is_available() {
            shell_kind = ShellKind::PowerShell;
        }
        self.new_terminal_with_shell(hwnd, shell_kind, no_profile);
    }

    pub(super) fn new_terminal_with_shell(
        &mut self,
        hwnd: HWND,
        shell_kind: ShellKind,
        no_profile: bool,
    ) {
        // The session starts asynchronously, so a shell that can't launch
        // would otherwise become a dead tab showing only an error. The shell
        // menu already disables these; the command palette reaches here too.
        if let Err(error) = shell_kind.resolve() {
            self.status = format!("{} is not available: {error}", shell_kind.name());
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        self.welcome = false;
        self.terminal_visible = true;
        self.terminal_tab = TerminalTab::Terminal;
        self.problems_shown = false;
        if self
            .spawn_terminal_pane(hwnd, shell_kind, no_profile)
            .is_some()
        {
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
        self.problems_shown = false;
        self.terminal_active = self.terminal_active.min(self.terminals.len() - 1);
        self.focus_active_shell(hwnd);
        self.update_title(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    // Close the active shell: dropping its pane drops the service, which
    // signals stop and lets the owner thread reap the child. Hides the panel
    // once the last terminal is gone.
    pub(super) fn close_active_terminal(&mut self, hwnd: HWND) {
        if self.terminal_tab == TerminalTab::Output {
            self.stop_running_program(hwnd);
            return;
        }
        if self.terminals.is_empty() {
            self.hide_terminal(hwnd);
            return;
        }
        let index = self.terminal_active.min(self.terminals.len() - 1);
        let removed = self.terminals.remove(index);
        clear_terminal_rename_for_session(&mut self.terminal_rename_input, removed.id);
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
        let shell_kind = self.terminals[old].shell_kind;
        if self
            .spawn_terminal_pane(hwnd, shell_kind, no_profile)
            .is_some()
        {
            let removed = self.terminals.remove(old);
            clear_terminal_rename_for_session(&mut self.terminal_rename_input, removed.id);
            self.terminal_active = self.terminals.len() - 1;
            self.focus_active_shell(hwnd);
            self.status = format!("Restarted {} terminal\u{2026}", shell_kind.name());
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
        self.problems_shown = false;
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
        self.terminal_profile_menu_open = false;
        self.terminal_profile_defaults_open = false;
        self.terminal_profile_availability.clear();
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
                self.problems_shown = false;
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
        self.problems_shown = false;
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
        let request = self.terminal_launch_request(ShellKind::PowerShell, true);
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
                self.status = match &self.run_snapshot.as_ref().unwrap().status {
                    SessionStatus::Exited { code: 0 } => "Program finished".into(),
                    SessionStatus::Exited { code } => format!("Program exited with code {code}"),
                    SessionStatus::Stopped => "Program stopped".into(),
                    SessionStatus::Failed(error) => format!("Run failed: {error}"),
                    _ => unreachable!(),
                };
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
        // Room for "PROBLEMS" and a count of up to three digits.
        let problems = RECT {
            left: x,
            top,
            right: x + self.scale(112),
            bottom: header_bottom,
        };
        x += self.scale(112);
        let output = RECT {
            left: x,
            top,
            right: x + self.scale(78),
            bottom: header_bottom,
        };
        x += self.scale(78);
        let tab_width = self.scale(92);
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
            right: x + gap + self.scale(22),
            bottom: header_bottom,
        };
        x += gap + self.scale(22);
        let chevron = RECT {
            left: x,
            top,
            right: x + self.scale(18),
            bottom: header_bottom,
        };
        x += self.scale(22);
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
            problems,
            output,
            terminals,
            plus,
            chevron,
            kill,
            hide,
        }
    }

    pub(super) fn terminal_header_hit(
        &self,
        left: i32,
        right: i32,
        top: i32,
        x: i32,
        y: i32,
    ) -> TerminalHeaderHit {
        let layout = self.terminal_header_layout(left, right, top);
        let inside =
            |rect: &RECT| x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
        if inside(&layout.hide) {
            return TerminalHeaderHit::Hide;
        }
        if inside(&layout.problems) {
            return TerminalHeaderHit::Problems;
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
        if inside(&layout.chevron) {
            return TerminalHeaderHit::ShellPicker;
        }
        if inside(&layout.kill) {
            return TerminalHeaderHit::Kill;
        }
        TerminalHeaderHit::Body
    }

    pub(super) fn terminal_context_menu_rect(
        &self,
        anchor_x: i32,
        anchor_y: i32,
        left: i32,
        right: i32,
        bottom: i32,
    ) -> RECT {
        let width = self.scale(164);
        let height = self.scale(30);
        let left = anchor_x
            .min(right - width - self.scale(6))
            .max(left + self.scale(6));
        let top = anchor_y
            .min(bottom - height - self.scale(4))
            .max(self.scale(4));
        RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }

    pub(super) fn terminal_rename_field_rect(&self, hwnd: HWND) -> Option<RECT> {
        let (session_id, _) = self.terminal_rename_input.as_ref()?;
        let index = terminal_pane_index(&self.terminals, *session_id)?;
        let left = self.editor_left();
        let right = self.editor_right(hwnd);
        let top = self.terminal_top(hwnd);
        let layout = self.terminal_header_layout(left, right, top);
        let tab = layout.terminals.get(index)?;
        Some(RECT {
            left: tab.left,
            top: layout.header_bottom + self.scale(3),
            right: (tab.left + self.scale(230)).min(right - self.scale(8)),
            bottom: layout.header_bottom + self.scale(33),
        })
    }

    pub(super) fn rename_terminal(&mut self, hwnd: HWND, session_id: SessionId) {
        let Some(index) = terminal_pane_index(&self.terminals, session_id) else {
            self.terminal_context_menu = None;
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        let Some(pane) = self.terminals.get(index) else {
            return;
        };
        let rename_state = (pane.id, pane.title.clone());
        self.terminal_context_menu = None;
        self.terminal_tab = TerminalTab::Terminal;
        self.terminal_active = index;
        self.terminal_focus = true;
        self.terminal_rename_input = Some(rename_state);
        self.caret_on = true;
        unsafe {
            SetFocus(hwnd);
            InvalidateRect(hwnd, null(), 0);
        }
    }

    pub(super) fn finish_terminal_rename(&mut self, hwnd: HWND) {
        let Some((session_id, input)) = self.terminal_rename_input.clone() else {
            return;
        };
        let Some(title) = normalized_terminal_name(&input) else {
            self.status = "Terminal name cannot be empty".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        if !set_terminal_pane_name(&mut self.terminals, session_id, title) {
            self.terminal_rename_input = None;
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        self.terminal_rename_input = None;
        self.status = "Terminal renamed".into();
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn terminal_profile_menu_layout(
        &self,
        left: i32,
        right: i32,
        top: i32,
        bottom: i32,
    ) -> TerminalProfileMenuLayout {
        let s = |v: i32| self.scale(v);
        let header = self.terminal_header_layout(left, right, top);
        let width = s(252);
        let menu_left = header
            .chevron
            .left
            .min(right - width - s(8))
            .max(left + s(8));
        let title_height = s(22);
        let row_height = s(25);
        let action_height = s(27);
        let menu_height = title_height + row_height * 4 + s(5) + action_height + s(4);
        let desired_top = header.header_bottom + s(3);
        let menu_top = desired_top.min(bottom - menu_height - s(3)).max(s(4));
        let shell_top = menu_top + title_height;
        let shell_rows = ShellKind::all()
            .iter()
            .enumerate()
            .map(|(index, shell)| {
                (
                    *shell,
                    RECT {
                        left: menu_left + s(4),
                        top: shell_top + index as i32 * row_height,
                        right: menu_left + width - s(4),
                        bottom: shell_top + (index as i32 + 1) * row_height,
                    },
                )
            })
            .collect::<Vec<_>>();
        let after_shells = shell_top + row_height * 4;
        let action = RECT {
            left: menu_left + s(4),
            top: after_shells + s(4),
            right: menu_left + width - s(4),
            bottom: after_shells + s(4) + action_height,
        };
        if self.terminal_profile_defaults_open {
            TerminalProfileMenuLayout {
                rect: RECT {
                    left: menu_left,
                    top: menu_top,
                    right: menu_left + width,
                    bottom: menu_top + menu_height,
                },
                shell_rows,
                settings: None,
                footer: action,
            }
        } else {
            TerminalProfileMenuLayout {
                rect: RECT {
                    left: menu_left,
                    top: menu_top,
                    right: menu_left + width,
                    bottom: menu_top + menu_height,
                },
                shell_rows,
                settings: Some(action),
                footer: RECT::default(),
            }
        }
    }

    pub(super) fn terminal_profile_menu_hit(
        &self,
        left: i32,
        right: i32,
        top: i32,
        bottom: i32,
        x: i32,
        y: i32,
    ) -> TerminalProfileMenuHit {
        let layout = self.terminal_profile_menu_layout(left, right, top, bottom);
        let inside =
            |rect: &RECT| x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
        for (shell, rect) in &layout.shell_rows {
            if inside(rect) {
                return TerminalProfileMenuHit::Shell(*shell);
            }
        }
        if let Some(settings) = &layout.settings
            && inside(settings)
        {
            return TerminalProfileMenuHit::Settings;
        }
        if self.terminal_profile_defaults_open && inside(&layout.footer) {
            return TerminalProfileMenuHit::Back;
        }
        TerminalProfileMenuHit::None
    }

    pub(super) fn toggle_terminal_profile_menu(&mut self, hwnd: HWND) {
        self.terminal_profile_menu_open = !self.terminal_profile_menu_open;
        self.terminal_profile_defaults_open = false;
        if self.terminal_profile_menu_open {
            self.terminal_profile_availability = ShellKind::all()
                .iter()
                .map(|shell| (*shell, shell.is_available()))
                .collect();
        } else {
            self.terminal_profile_availability.clear();
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn terminal_profile_shell_available(&self, shell: ShellKind) -> bool {
        self.terminal_profile_availability
            .iter()
            .find_map(|(candidate, available)| (*candidate == shell).then_some(*available))
            .unwrap_or(false)
    }

    pub(super) fn set_default_terminal_profile(&mut self, hwnd: HWND, shell: ShellKind) {
        self.settings.default_terminal_profile = shell;
        if let Err(e) = self.settings.save() {
            self.status = format!("Failed to save settings: {e}");
        } else {
            self.status = format!("Default terminal profile set to {}", shell.name());
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    #[allow(dead_code)]
    pub(super) fn show_shell_picker_menu(&mut self, hwnd: HWND, x: i32, y: i32) {
        use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, MF_CHECKED, MF_DISABLED, MF_GRAYED,
            MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, TPM_LEFTALIGN, TPM_RETURNCMD,
            TPM_RIGHTBUTTON, TrackPopupMenu,
        };

        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return;
        }

        const CMD_POWERSHELL: usize = 1;
        const CMD_CMD: usize = 2;
        const CMD_GIT_BASH: usize = 3;
        const CMD_WSL: usize = 4;
        const CMD_SET_DEFAULT_POWERSHELL: usize = 11;
        const CMD_SET_DEFAULT_CMD: usize = 12;
        const CMD_SET_DEFAULT_GIT_BASH: usize = 13;
        const CMD_SET_DEFAULT_WSL: usize = 14;

        let default_shell = self.settings.default_terminal_profile;

        let pwsh_avail = ShellKind::PowerShell.is_available();
        let cmd_avail = ShellKind::CommandPrompt.is_available();
        let bash_avail = ShellKind::GitBash.is_available();
        let wsl_avail = ShellKind::Wsl.is_available();

        let add_shell_item = |parent_menu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU,
                              cmd: usize,
                              kind: ShellKind,
                              avail: bool,
                              is_default: bool| {
            let mut flags = MF_STRING;
            if is_default {
                flags |= MF_CHECKED;
            } else {
                flags |= MF_UNCHECKED;
            }
            if !avail {
                flags |= MF_DISABLED | MF_GRAYED;
            }
            let text = if avail {
                if is_default {
                    format!("{} (Default)", kind.name())
                } else {
                    kind.name().to_string()
                }
            } else {
                format!("{} (Not Installed)", kind.name())
            };
            unsafe { AppendMenuW(parent_menu, flags, cmd, wide(&text).as_ptr()) };
        };

        add_shell_item(
            menu,
            CMD_POWERSHELL,
            ShellKind::PowerShell,
            pwsh_avail,
            default_shell == ShellKind::PowerShell,
        );
        add_shell_item(
            menu,
            CMD_CMD,
            ShellKind::CommandPrompt,
            cmd_avail,
            default_shell == ShellKind::CommandPrompt,
        );
        add_shell_item(
            menu,
            CMD_GIT_BASH,
            ShellKind::GitBash,
            bash_avail,
            default_shell == ShellKind::GitBash,
        );
        add_shell_item(
            menu,
            CMD_WSL,
            ShellKind::Wsl,
            wsl_avail,
            default_shell == ShellKind::Wsl,
        );

        unsafe {
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            let default_submenu = CreatePopupMenu();
            if !default_submenu.is_null() {
                add_shell_item(
                    default_submenu,
                    CMD_SET_DEFAULT_POWERSHELL,
                    ShellKind::PowerShell,
                    pwsh_avail,
                    default_shell == ShellKind::PowerShell,
                );
                add_shell_item(
                    default_submenu,
                    CMD_SET_DEFAULT_CMD,
                    ShellKind::CommandPrompt,
                    cmd_avail,
                    default_shell == ShellKind::CommandPrompt,
                );
                add_shell_item(
                    default_submenu,
                    CMD_SET_DEFAULT_GIT_BASH,
                    ShellKind::GitBash,
                    bash_avail,
                    default_shell == ShellKind::GitBash,
                );
                add_shell_item(
                    default_submenu,
                    CMD_SET_DEFAULT_WSL,
                    ShellKind::Wsl,
                    wsl_avail,
                    default_shell == ShellKind::Wsl,
                );

                AppendMenuW(
                    menu,
                    MF_POPUP,
                    default_submenu as usize,
                    wide("Select Default Profile").as_ptr(),
                );
            }
        }

        let mut pt = POINT { x, y };
        unsafe { ClientToScreen(hwnd, &mut pt) };

        let selected = unsafe {
            TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_LEFTALIGN | TPM_RIGHTBUTTON,
                pt.x,
                pt.y,
                0,
                hwnd,
                null(),
            ) as usize
        };

        unsafe { DestroyMenu(menu) };

        match selected {
            CMD_POWERSHELL => self.new_terminal_with_shell(hwnd, ShellKind::PowerShell, false),
            CMD_CMD => self.new_terminal_with_shell(hwnd, ShellKind::CommandPrompt, false),
            CMD_GIT_BASH => self.new_terminal_with_shell(hwnd, ShellKind::GitBash, false),
            CMD_WSL => self.new_terminal_with_shell(hwnd, ShellKind::Wsl, false),
            CMD_SET_DEFAULT_POWERSHELL => {
                self.set_default_terminal_profile(hwnd, ShellKind::PowerShell)
            }
            CMD_SET_DEFAULT_CMD => {
                self.set_default_terminal_profile(hwnd, ShellKind::CommandPrompt)
            }
            CMD_SET_DEFAULT_GIT_BASH => self.set_default_terminal_profile(hwnd, ShellKind::GitBash),
            CMD_SET_DEFAULT_WSL => self.set_default_terminal_profile(hwnd, ShellKind::Wsl),
            _ => {}
        }
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
                if let Some(bytes) = output_control_bytes(key, modifiers) {
                    self.run_session
                        .is_some_and(|id| self.terminal.input(id, &bytes).is_ok())
                } else {
                    self.run_session
                        .is_some_and(|id| self.terminal.key(id, key, modifiers).is_ok())
                }
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
            TerminalTab::Output => self
                .run_session
                .is_some_and(|id| self.terminal.scrollback(id, next).is_ok()),
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

    pub(super) fn run_language_plan(&mut self, hwnd: HWND, plan: lightline::runner::RunPlan) {
        match plan.powershell_command() {
            Ok(command) => self.start_language_run(hwnd, plan.cwd, command),
            Err(error) => {
                self.status = error;
                self.refresh(hwnd);
            }
        }
    }

    pub(super) fn start_language_run(&mut self, hwnd: HWND, cwd: PathBuf, command: String) {
        self.poll_terminal(hwnd);
        if self.run_session.is_some() {
            self.status = "A program is already running. Stop it before starting another.".into();
            self.refresh(hwnd);
            return;
        }
        let size = self.terminal_size_for(hwnd);
        match self.terminal.start(
            SessionKind::ManagedRun,
            LaunchRequest::Run { cwd, command },
            size,
        ) {
            Ok(id) => {
                self.welcome = false;
                self.terminal_visible = true;
                self.terminal_tab = TerminalTab::Output;
                self.run_session = Some(id);
                self.run_applied_size = Some(size);
                self.run_snapshot = None;
                self.status = "Running program...".into();
                self.focus_run_session(hwnd);
            }
            Err(error) => self.status = format!("Run could not start: {error}"),
        }
        self.refresh(hwnd);
    }

    pub(super) fn stop_running_program(&mut self, hwnd: HWND) {
        if let Some(id) = self.run_session {
            let _ = self.terminal.stop(id);
            self.status = "Stopping program...".into();
        } else {
            self.status = "No program is running".into();
        }
        self.refresh(hwnd);
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
        self.terminal_rename_input = None;
        self.terminal_context_menu = None;
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

#[cfg(test)]
mod input_tests {
    use super::*;
    use crate::windows_app::input::decode_utf16_input;

    fn pane(id: u64, title: &str) -> TerminalPane {
        TerminalPane {
            title: title.into(),
            custom_title: false,
            service: TerminalService::new(|| {}),
            id: SessionId(id),
            snapshot: None,
            applied_size: None,
            shell_kind: ShellKind::PowerShell,
        }
    }

    #[test]
    fn terminal_names_are_trimmed_bounded_and_non_empty() {
        assert_eq!(
            normalized_terminal_name("  build shell  "),
            Some("build shell".into())
        );
        assert_eq!(normalized_terminal_name(" \t\n "), None);
        assert_eq!(normalized_terminal_name("one\ntwo"), Some("onetwo".into()));
        assert_eq!(
            normalized_terminal_name(&"x".repeat(MAX_TERMINAL_NAME_CHARS + 1)),
            Some("x".repeat(MAX_TERMINAL_NAME_CHARS))
        );
    }

    #[test]
    fn rename_tracks_session_when_terminal_collection_changes() {
        let target = SessionId(1);
        let mut terminals = vec![pane(1, "A"), pane(2, "B"), pane(3, "C")];
        terminals.remove(2);
        assert!(set_terminal_pane_name(
            &mut terminals,
            target,
            "renamed A".into()
        ));
        assert_eq!(terminals[0].title, "renamed A");
        assert_eq!(terminals[1].title, "B");

        let target = SessionId(2);
        terminals = vec![pane(1, "A"), pane(2, "B"), pane(3, "C")];
        terminals.remove(0);
        assert!(set_terminal_pane_name(
            &mut terminals,
            target,
            "renamed B".into()
        ));
        assert_eq!(terminals[0].title, "renamed B");
        assert_eq!(terminals[1].title, "C");

        let mut rename_input = Some((target, "pending B".into()));
        terminals.remove(0);
        clear_terminal_rename_for_session(&mut rename_input, target);
        assert_eq!(rename_input, None);
        assert!(!set_terminal_pane_name(
            &mut terminals,
            target,
            "wrong target".into()
        ));
        assert_eq!(terminals[0].title, "C");
    }

    #[test]
    fn output_enter_and_backspace_use_pipe_appropriate_bytes() {
        let plain = TermModifiers::default();
        let control = TermModifiers {
            control: true,
            ..TermModifiers::default()
        };

        assert_eq!(
            output_control_bytes(TermKey::Enter, plain),
            Some(b"\r\n".to_vec())
        );
        assert_eq!(
            output_control_bytes(TermKey::Backspace, plain),
            Some(vec![127])
        );
        assert_eq!(
            output_control_bytes(TermKey::Backspace, control),
            Some(vec![8])
        );
    }

    #[test]
    fn utf16_surrogate_pairs_and_unicode_decoding() {
        let mut pending = None;

        // ASCII
        assert_eq!(decode_utf16_input(&mut pending, b'A' as u16), Some('A'));
        assert_eq!(pending, None);

        // BMP Unicode
        assert_eq!(decode_utf16_input(&mut pending, 0x00e9), Some('é'));
        assert_eq!(pending, None);
        assert_eq!(decode_utf16_input(&mut pending, 0x4e2d), Some('中'));
        assert_eq!(pending, None);

        // Supplementary characters: emoji 😀 (U+1F600 = [0xD83D, 0xDE00])
        assert_eq!(decode_utf16_input(&mut pending, 0xd83d), None);
        assert_eq!(pending, Some(0xd83d));
        assert_eq!(decode_utf16_input(&mut pending, 0xde00), Some('😀'));
        assert_eq!(pending, None);

        // Supplementary characters: rocket 🚀 (U+1F680 = [0xD83D, 0xDE80])
        assert_eq!(decode_utf16_input(&mut pending, 0xd83d), None);
        assert_eq!(pending, Some(0xd83d));
        assert_eq!(decode_utf16_input(&mut pending, 0xde80), Some('🚀'));
        assert_eq!(pending, None);

        // Unpaired low surrogate rejected
        assert_eq!(decode_utf16_input(&mut pending, 0xde00), None);
        assert_eq!(pending, None);
    }

    #[test]
    fn menu_rectangle_clamps_to_layout_bounds() {
        // App scaled dimensions for menu
        let width = 164;
        let height = 30;
        let left_bound = 50;
        let right_bound = 300;
        let bottom_bound = 400;

        let clamp = |anchor_x: i32, anchor_y: i32| {
            let left = anchor_x.min(right_bound - width - 6).max(left_bound + 6);
            let top = anchor_y.min(bottom_bound - height - 4).max(4);
            RECT {
                left,
                top,
                right: left + width,
                bottom: top + height,
            }
        };

        // Normal anchor
        let rect = clamp(100, 100);
        assert_eq!(rect.left, 100);
        assert_eq!(rect.top, 100);

        // Anchor near/beyond right edge clamps left so menu stays inside right_bound
        let rect = clamp(500, 100);
        assert_eq!(rect.right, right_bound - 6);
        assert!(rect.left >= left_bound + 6);

        // Anchor near/beyond bottom edge clamps top so menu stays inside bottom_bound
        let rect = clamp(100, 500);
        assert_eq!(rect.bottom, bottom_bound - 4);
        assert!(rect.top >= 4);

        // Anchor near/beyond left edge clamps left
        let rect = clamp(10, 100);
        assert_eq!(rect.left, left_bound + 6);
    }
}
