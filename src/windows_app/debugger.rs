use super::app::DebugState;
use super::*;
use lightline::debug::LaunchRequest as DebugLaunchRequest;
use serde_json::Value;
use std::process::{Command as ProcessCommand, Stdio};

pub(super) const DEBUG_EVENT_MESSAGE: u32 = WM_APP + 9;

// A single flattened VARIABLES-tree row: either a scope header, a variable
// (leaf or expandable), or a "Loading..." placeholder for children still in
// flight. See `App::debug_variable_rows`.
pub(super) struct DebugVariableRow {
    pub(super) y: i32,
    pub(super) depth: usize,
    // DAP variablesReference; 0 for headers, leaves, and the loading row.
    pub(super) reference: i64,
    pub(super) expandable: bool,
    pub(super) expanded: bool,
    pub(super) is_header: bool,
    pub(super) loading: bool,
    pub(super) name: String,
    pub(super) value: String,
}

pub(super) struct DebugPanelLayout {
    pub(super) variables_header_y: i32,
    pub(super) variables_body_y: i32,
    pub(super) variable_rows: Vec<DebugVariableRow>,
    pub(super) call_stack_header_y: i32,
    pub(super) call_stack_body_y: i32,
    pub(super) breakpoints_header_y: i32,
    pub(super) breakpoints_body_y: i32,
}

impl App {
    // Play button / F5: build the workspace, then launch it under lldb-dap
    // with the breakpoints currently set across all open tabs.
    pub(super) fn start_debug_session(&mut self, hwnd: HWND) {
        if self.debug.is_some() {
            self.debug_continue(hwnd);
            return;
        }
        let Some(root) = self.workspace_root.clone() else {
            self.status = "Open a Rust workspace to debug".into();
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        let breakpoints: Vec<(PathBuf, Vec<u32>)> = self
            .tabs
            .iter()
            .filter_map(|tab| {
                let path = tab.document.path.clone()?;
                let lines: Vec<u32> = tab.document.breakpoints().iter().map(|&l| l as u32).collect();
                (!lines.is_empty()).then_some((path, lines))
            })
            .collect();
        self.debug_state = DebugState {
            status: "Building...".into(),
            ..Default::default()
        };
        self.debug_pending_root = Some(root.clone());
        self.debug_pending_breakpoints = breakpoints;
        let tx = self.worker_tx.clone();
        self.worker_started(hwnd);
        std::thread::spawn(move || {
            let result = build_debug_binary(&root);
            let _ = tx.send(WorkerMessage::DebugBuild(result));
        });
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn debug_build_finished(&mut self, hwnd: HWND, result: Result<PathBuf, String>) {
        match result {
            Ok(program) => {
                let Some(root) = self.debug_pending_root.take() else {
                    return;
                };
                let breakpoints = std::mem::take(&mut self.debug_pending_breakpoints);
                let hwnd_value = hwnd as isize;
                let wake = Arc::new(move || unsafe {
                    PostMessageW(hwnd_value as HWND, DEBUG_EVENT_MESSAGE, 0, 0);
                });
                let request = DebugLaunchRequest {
                    program,
                    args: Vec::new(),
                    cwd: root,
                    breakpoints,
                };
                self.debug = Some(DebugClient::start(
                    request,
                    self.debug_event_tx.clone(),
                    wake,
                ));
                self.debug_state.status = "Starting...".into();
            }
            Err(error) => {
                self.debug_pending_root = None;
                self.debug_pending_breakpoints.clear();
                self.debug_state.status = format!("Build failed: {error}");
                self.status = "Debug build failed".into();
            }
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn poll_debug(&mut self, hwnd: HWND) {
        let events: Vec<DebugEvent> = self.debug_events.try_iter().collect();
        if events.is_empty() {
            return;
        }
        for event in events {
            match event {
                DebugEvent::Running => {
                    self.debug_state.status = "Running".into();
                    self.debug_state.running = true;
                }
                DebugEvent::Stopped {
                    thread_id,
                    reason,
                    frames,
                    scopes,
                } => {
                    self.debug_state.status = format!("Paused: {reason}");
                    self.debug_state.running = false;
                    self.debug_state.thread_id = thread_id;
                    self.debug_state.frames = frames;
                    self.debug_state.scopes = scopes;
                    // Old variablesReference ids are meaningless once the
                    // debuggee moves; a stale expansion would fetch garbage
                    // or a now-reused reference.
                    self.debug_state.expanded.clear();
                    self.debug_state.children.clear();
                    self.status = format!("Debugger stopped ({reason})");
                    if let Some(frame) = self.debug_state.frames.first()
                        && let Some(path) = frame.path.clone()
                    {
                        let line = frame.line.saturating_sub(1) as usize;
                        self.open(hwnd, Some(path));
                        let byte_pos = {
                            let doc = self.doc();
                            let line = line.min(doc.line_count().saturating_sub(1));
                            Pos { line, byte: 0 }
                        };
                        self.move_cursor(byte_pos, false);
                        self.keep_cursor_visible(hwnd);
                    }
                }
                DebugEvent::Continued => {
                    self.debug_state.status = "Running".into();
                    self.debug_state.running = true;
                    self.debug_state.frames.clear();
                    self.debug_state.scopes.clear();
                    self.debug_state.expanded.clear();
                    self.debug_state.children.clear();
                }
                DebugEvent::Variables { reference, variables } => {
                    self.debug_state.children.insert(reference, variables);
                }
                DebugEvent::Output { text } => {
                    let trimmed = text.trim_end();
                    if !trimmed.is_empty() {
                        self.status = trimmed.chars().take(200).collect();
                    }
                }
                DebugEvent::Terminated => {
                    self.debug = None;
                    self.debug_state = DebugState {
                        status: "Program exited".into(),
                        ..Default::default()
                    };
                    self.status = "Debug session ended".into();
                }
                DebugEvent::Failed { message } => {
                    self.debug = None;
                    self.debug_state = DebugState {
                        status: message.clone(),
                        ..Default::default()
                    };
                    self.status = message;
                }
            }
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn debug_continue(&mut self, hwnd: HWND) {
        if let Some(client) = &self.debug {
            client.send(DebugCommand::Continue);
            unsafe { InvalidateRect(hwnd, null(), 0) };
        } else {
            self.start_debug_session(hwnd);
        }
    }

    pub(super) fn debug_pause(&mut self, hwnd: HWND) {
        if let Some(client) = &self.debug {
            client.send(DebugCommand::Pause);
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    pub(super) fn debug_step_over(&mut self, hwnd: HWND) {
        if let Some(client) = &self.debug {
            client.send(DebugCommand::Next);
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    pub(super) fn debug_step_in(&mut self, hwnd: HWND) {
        if let Some(client) = &self.debug {
            client.send(DebugCommand::StepIn);
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    pub(super) fn debug_step_out(&mut self, hwnd: HWND) {
        if let Some(client) = &self.debug {
            client.send(DebugCommand::StepOut);
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    pub(super) fn debug_restart(&mut self, hwnd: HWND) {
        self.debug = None;
        while self.debug_events.try_recv().is_ok() {}
        self.debug_state = DebugState {
            status: "Restarting...".into(),
            ..Default::default()
        };
        self.start_debug_session(hwnd);
    }

    pub(super) fn toggle_debug_section(&mut self, hwnd: HWND, section: usize) {
        if let Some(expanded) = self.debug_sections_expanded.get_mut(section) {
            *expanded = !*expanded;
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    // Toggles a struct/collection variable's expansion. `reference` is its
    // DAP `variablesReference`; a bare/leaf value has 0 and is never clickable
    // (checked by the caller). Expanding for the first time kicks off the
    // fetch; the children arrive later as `DebugEvent::Variables`.
    pub(super) fn toggle_debug_variable(&mut self, hwnd: HWND, reference: i64) {
        if reference == 0 {
            return;
        }
        if !self.debug_state.expanded.remove(&reference) {
            self.debug_state.expanded.insert(reference);
            if !self.debug_state.children.contains_key(&reference)
                && let Some(client) = &self.debug
            {
                client.send(DebugCommand::Variables(reference));
            }
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    // Where the VARIABLES tree body starts, in the same window coordinates
    // paint_debug_panel draws in: past the toolbar, the status line, and the
    // "VARIABLES" section header. Shared so the click handler in input.rs
    // lands on exactly the rows that were actually painted.
    pub(super) fn debug_panel_layout(&self, bottom: i32) -> DebugPanelLayout {
        let s = |v: i32| self.scale(v);
        let header_height = s(28);
        let row_height = s(23);
        let scope_height = s(22);
        let gap = s(7);
        let variables_header_y = s(190);
        let variables_body_y = variables_header_y + header_height;
        let mut variable_rows = Vec::new();
        let mut y = variables_body_y;
        if self.debug_sections_expanded[0] {
            if self.debug_state.scopes.is_empty() {
                y += s(34);
            } else {
                // Keep enough room for the remaining section headers even
                // when the debugger reports a very large locals tree.
                let rows_bottom = (bottom - header_height * 2 - gap * 2).max(y);
                let (rows, next_y) =
                    self.debug_variable_rows(y, rows_bottom, row_height, scope_height);
                variable_rows = rows;
                y = next_y + s(5);
            }
        }
        let call_stack_header_y = y + gap;
        let call_stack_body_y = call_stack_header_y + header_height;
        y = call_stack_body_y;
        if self.debug_sections_expanded[1] {
            y += if self.debug_state.frames.is_empty() {
                s(34)
            } else {
                row_height * self.debug_state.frames.len().min(5) as i32 + s(5)
            };
        }
        let breakpoints_header_y = y + gap;
        let breakpoints_body_y = breakpoints_header_y + header_height;
        DebugPanelLayout {
            variables_header_y,
            variables_body_y,
            variable_rows,
            call_stack_header_y,
            call_stack_body_y,
            breakpoints_header_y,
            breakpoints_body_y,
        }
    }

    #[allow(dead_code)]
    pub(super) fn debug_variables_start_y(&self) -> i32 {
        self.scale(218)
    }

    // Flattens the VARIABLES tree (scope headers, their variables, and any
    // expanded children) into rows with absolute y positions. Used by both
    // paint_debug_panel (to draw) and the click handler (to hit-test), so the
    // two can never drift apart the way "renders one thing, clicks another"
    // bugs usually happen.
    pub(super) fn debug_variable_rows(
        &self,
        start_y: i32,
        bottom: i32,
        row_height: i32,
        header_height: i32,
    ) -> (Vec<DebugVariableRow>, i32) {
        let mut rows = Vec::new();
        let mut y = start_y;
        for scope in &self.debug_state.scopes {
            if y + header_height > bottom {
                break;
            }
            rows.push(DebugVariableRow {
                y,
                depth: 0,
                reference: 0,
                expandable: false,
                expanded: false,
                is_header: true,
                loading: false,
                name: scope.name.clone(),
                value: String::new(),
            });
            y += header_height;
            let mut truncated = false;
            for variable in &scope.variables {
                if !self.push_debug_variable_row(&mut rows, variable, 0, &mut y, bottom, row_height) {
                    truncated = true;
                    break;
                }
            }
            if truncated {
                break;
            }
        }
        (rows, y)
    }

    // Returns false once `bottom` is reached, so the caller can stop walking
    // remaining scopes/siblings instead of producing invisible rows.
    fn push_debug_variable_row(
        &self,
        rows: &mut Vec<DebugVariableRow>,
        variable: &DebugVariable,
        depth: usize,
        y: &mut i32,
        bottom: i32,
        row_height: i32,
    ) -> bool {
        if *y + row_height > bottom {
            return false;
        }
        let expandable = variable.variables_reference != 0;
        let expanded = expandable && self.debug_state.expanded.contains(&variable.variables_reference);
        rows.push(DebugVariableRow {
            y: *y,
            depth,
            reference: variable.variables_reference,
            expandable,
            expanded,
            is_header: false,
            loading: false,
            name: variable.name.clone(),
            value: variable.value.clone(),
        });
        *y += row_height;
        if !expanded {
            return true;
        }
        match self.debug_state.children.get(&variable.variables_reference) {
            Some(children) => {
                for child in children {
                    if !self.push_debug_variable_row(rows, child, depth + 1, y, bottom, row_height) {
                        return false;
                    }
                }
                true
            }
            None => {
                if *y + row_height > bottom {
                    return false;
                }
                rows.push(DebugVariableRow {
                    y: *y,
                    depth: depth + 1,
                    reference: 0,
                    expandable: false,
                    expanded: false,
                    is_header: false,
                    loading: true,
                    name: "Loading\u{2026}".into(),
                    value: String::new(),
                });
                *y += row_height;
                true
            }
        }
    }

    pub(super) fn debug_stop(&mut self, hwnd: HWND) {
        // Dropping the client sends Disconnect and tears the adapter down.
        self.debug = None;
        self.debug_state = DebugState {
            status: "Not running".into(),
            ..Default::default()
        };
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}

// Builds the workspace and resolves the produced `[[bin]]` executable via
// cargo's own JSON build log, so this works the same whether the workspace
// has one crate or several.
fn build_debug_binary(root: &Path) -> Result<PathBuf, String> {
    let mut command = ProcessCommand::new("cargo");
    command
        .args(["build", "--message-format=json"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = command
        .output()
        .map_err(|error| format!("Could not run cargo build: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        // `--message-format=json` puts rustc's actual diagnostics on stdout as
        // "compiler-message" entries; stderr just carries cargo's own
        // multi-line progress text, which makes an unreadable one-line dump
        // in the panel. Surface the last real compiler error instead.
        let mut message = None;
        for line in stdout.lines() {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line)
                && value.get("reason").and_then(Value::as_str) == Some("compiler-message")
                && value.pointer("/message/level").and_then(Value::as_str) == Some("error")
                && let Some(text) = value.pointer("/message/message").and_then(Value::as_str)
            {
                message = Some(text.to_string());
            }
        }
        let message = message.unwrap_or_else(|| {
            let stderr = String::from_utf8_lossy(&output.stderr);
            stderr
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("cargo build failed")
                .trim()
                .to_string()
        });
        return Err(message);
    }
    let mut executable = None;
    for line in stdout.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let is_bin_artifact = value.get("reason").and_then(Value::as_str) == Some("compiler-artifact")
            && value
                .pointer("/target/kind")
                .and_then(Value::as_array)
                .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("bin")));
        if is_bin_artifact
            && let Some(exe) = value.get("executable").and_then(Value::as_str)
        {
            executable = Some(PathBuf::from(exe));
        }
    }
    executable.ok_or_else(|| "cargo build did not produce a [[bin]] executable".to_string())
}
