use super::app::DebugState;
use super::*;
use lightline::debug::LaunchRequest as DebugLaunchRequest;
use serde_json::Value;
use std::process::{Command as ProcessCommand, Stdio};

pub(super) const DEBUG_EVENT_MESSAGE: u32 = WM_APP + 9;

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
