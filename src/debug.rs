//! A small Debug Adapter Protocol (DAP) client for lldb-dap, following the
//! same background-thread/channel/wake shape as [`crate::lsp`]: one worker
//! thread owns the adapter process and the request/response bookkeeping, and
//! only ever hands the UI thread flattened [`Event`]s.

use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{ChildStdin, Command as ProcessCommand, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct LaunchRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Per-file breakpoint line numbers, zero-based, sent before the program runs.
    pub breakpoints: Vec<(PathBuf, Vec<u32>)>,
}

pub enum Command {
    Continue,
    Next,
    StepIn,
    StepOut,
    Pause,
    Disconnect,
    /// Fetch the children of a struct/collection variable on demand, keyed by
    /// its `variablesReference` (0 means "no children" and is never sent).
    Variables(i64),
}

#[derive(Clone, Debug)]
pub struct StackFrame {
    pub id: i64,
    pub name: String,
    pub path: Option<PathBuf>,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct Variable {
    pub name: String,
    pub value: String,
    pub kind: String,
    /// Non-zero when this variable has children (struct fields, array
    /// elements, ...) that can be fetched with `Command::Variables`.
    pub variables_reference: i64,
}

#[derive(Clone, Debug)]
pub struct Scope {
    pub name: String,
    pub variables: Vec<Variable>,
}

pub enum Event {
    /// The adapter finished configuration; the program is now running.
    Running,
    Stopped {
        thread_id: i64,
        reason: String,
        frames: Vec<StackFrame>,
        scopes: Vec<Scope>,
    },
    Continued,
    Output {
        text: String,
    },
    /// Children of a variable requested via `Command::Variables`.
    Variables {
        reference: i64,
        variables: Vec<Variable>,
    },
    Terminated,
    Failed {
        message: String,
    },
}

pub struct DebugClient {
    sender: Sender<Command>,
}

impl DebugClient {
    pub fn start(
        request: LaunchRequest,
        events: Sender<Event>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (sender, commands) = mpsc::channel();
        thread::spawn(move || run_adapter(request, commands, events, wake));
        Self { sender }
    }

    pub fn send(&self, command: Command) -> bool {
        self.sender.send(command).is_ok()
    }
}

impl Drop for DebugClient {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Disconnect);
    }
}

/// Finds `lldb-dap` (or `lldb-dap.exe`) on PATH, including the copy rustup
/// installs alongside the `llvm-tools` component.
fn find_lldb_dap() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["lldb-dap.exe", "lldb-dap"]
    } else {
        &["lldb-dap"]
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

fn emit(event: Event, events: &Sender<Event>, wake: &Arc<dyn Fn() + Send + Sync>) {
    if events.send(event).is_ok() {
        wake();
    }
}

// Tracks which in-flight request a DAP response (matched by request_seq)
// belongs to, since responses carry no payload type of their own.
enum PendingKind {
    Initialize,
    Launch,
    Threads,
    StackTrace,
    Scopes,
    Variables { scope_name: String },
    /// A user-initiated expand-on-click fetch, not part of the stop flow;
    /// answered directly with `Event::Variables` instead of feeding `StopFlow`.
    VariablesOnDemand { reference: i64 },
    Other,
}

// State threaded through the threads -> stackTrace -> scopes -> variables
// chain that a single `stopped` event kicks off, so the UI gets one
// consolidated `Event::Stopped` instead of driving the round trips itself.
#[derive(Default)]
struct StopFlow {
    thread_id: i64,
    reason: String,
    frames: Vec<StackFrame>,
    scopes: Vec<Scope>,
    pending_scopes: VecDeque<(String, i64)>,
}

fn send_request(
    stdin: &mut ChildStdin,
    seq: &mut u64,
    pending: &mut HashMap<u64, PendingKind>,
    command: &str,
    arguments: Value,
    kind: PendingKind,
) -> io::Result<()> {
    *seq += 1;
    pending.insert(*seq, kind);
    let mut message = json!({"seq": *seq, "type": "request", "command": command});
    if !arguments.is_null() {
        message["arguments"] = arguments;
    }
    write_packet(stdin, &message)
}

// Sends a request for the current thread without tracking its response;
// used for the stepping controls, whose acks carry nothing the UI needs
// (the next `stopped`/`continued` event carries the real state change).
fn send_thread_command(
    stdin: &mut ChildStdin,
    seq: &mut u64,
    pending: &mut HashMap<u64, PendingKind>,
    command: &str,
    thread_id: i64,
) -> io::Result<()> {
    send_request(
        stdin,
        seq,
        pending,
        command,
        json!({"threadId": thread_id}),
        PendingKind::Other,
    )
}

// Pops the next scope awaiting a `variables` request and sends it; returns
// false once every scope has its variables, meaning the stop is fully
// resolved and ready to report to the UI.
fn request_next_scope(
    stdin: &mut ChildStdin,
    seq: &mut u64,
    pending: &mut HashMap<u64, PendingKind>,
    flow: &mut StopFlow,
) -> io::Result<bool> {
    let Some((name, reference)) = flow.pending_scopes.pop_front() else {
        return Ok(false);
    };
    send_request(
        stdin,
        seq,
        pending,
        "variables",
        json!({"variablesReference": reference}),
        PendingKind::Variables { scope_name: name },
    )?;
    Ok(true)
}

fn finish_stop(flow: StopFlow, events: &Sender<Event>, wake: &Arc<dyn Fn() + Send + Sync>) {
    emit(
        Event::Stopped {
            thread_id: flow.thread_id,
            reason: flow.reason,
            frames: flow.frames,
            scopes: flow.scopes,
        },
        events,
        wake,
    );
}

fn run_adapter(
    request: LaunchRequest,
    commands: Receiver<Command>,
    events: Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let Some(adapter) = find_lldb_dap() else {
        emit(
            Event::Failed {
                message: "lldb-dap was not found on PATH. Install LLVM (or `rustup component add llvm-tools`) to enable debugging.".into(),
            },
            &events,
            &wake,
        );
        return;
    };
    let mut process = ProcessCommand::new(adapter);
    process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        process.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(error) => {
            emit(
                Event::Failed {
                    message: format!("Could not start lldb-dap: {error}"),
                },
                &events,
                &wake,
            );
            return;
        }
    };
    let stderr_reader = child.stderr.take().map(|mut output| {
        thread::spawn(move || {
            let _ = io::copy(&mut output, &mut io::sink());
        })
    });
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        return;
    };
    let (incoming_tx, incoming_rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut stdout = BufReader::new(stdout);
        loop {
            match read_packet(&mut stdout) {
                Ok(Some(message)) => {
                    if incoming_tx.send(Ok(message)).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = incoming_tx.send(Err("lldb-dap closed its output".to_string()));
                    break;
                }
                Err(error) => {
                    let _ = incoming_tx.send(Err(format!("DAP read failed: {error}")));
                    break;
                }
            }
        }
    });

    let mut seq: u64 = 0;
    let mut pending: HashMap<u64, PendingKind> = HashMap::new();

    if send_request(
        &mut stdin,
        &mut seq,
        &mut pending,
        "initialize",
        json!({
            "clientID": "lightline",
            "adapterID": "lldb-dap",
            "pathFormat": "path",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "supportsVariableType": true,
        }),
        PendingKind::Initialize,
    )
    .is_err()
    {
        emit(
            Event::Failed {
                message: "Could not initialize lldb-dap".into(),
            },
            &events,
            &wake,
        );
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        return;
    }

    let mut stop_flow: Option<StopFlow> = None;
    let mut failure = None;
    'running: loop {
        while let Ok(incoming) = incoming_rx.try_recv() {
            let message = match incoming {
                Ok(message) => message,
                Err(error) => {
                    failure = Some(error);
                    break 'running;
                }
            };
            match message.get("type").and_then(Value::as_str) {
                Some("response") => {
                    let Some(request_seq) = message.get("request_seq").and_then(Value::as_u64)
                    else {
                        continue;
                    };
                    let Some(kind) = pending.remove(&request_seq) else {
                        continue;
                    };
                    let success = message
                        .get("success")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if !success {
                        let text = message
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("request failed");
                        if matches!(kind, PendingKind::Initialize | PendingKind::Launch) {
                            failure = Some(text.to_string());
                            break 'running;
                        }
                        continue;
                    }
                    match kind {
                        PendingKind::Initialize => {
                            if send_request(
                                &mut stdin,
                                &mut seq,
                                &mut pending,
                                "launch",
                                json!({
                                    "program": request.program.to_string_lossy(),
                                    "args": request.args,
                                    "cwd": request.cwd.to_string_lossy(),
                                    "stopOnEntry": false,
                                }),
                                PendingKind::Launch,
                            )
                            .is_err()
                            {
                                failure = Some("Could not send launch request".into());
                                break 'running;
                            }
                        }
                        PendingKind::Launch => {
                            emit(Event::Running, &events, &wake);
                        }
                        PendingKind::Threads => {
                            let Some(flow) = stop_flow.as_mut() else {
                                continue;
                            };
                            let thread_id = message
                                .pointer("/body/threads")
                                .and_then(Value::as_array)
                                .and_then(|threads| {
                                    threads
                                        .iter()
                                        .find(|t| {
                                            t.get("id").and_then(Value::as_i64)
                                                == Some(flow.thread_id)
                                        })
                                        .or_else(|| threads.first())
                                })
                                .and_then(|t| t.get("id"))
                                .and_then(Value::as_i64)
                                .unwrap_or(flow.thread_id);
                            flow.thread_id = thread_id;
                            if send_request(
                                &mut stdin,
                                &mut seq,
                                &mut pending,
                                "stackTrace",
                                json!({"threadId": thread_id, "startFrame": 0, "levels": 20}),
                                PendingKind::StackTrace,
                            )
                            .is_err()
                            {
                                failure = Some("Could not request stack trace".into());
                                break 'running;
                            }
                        }
                        PendingKind::StackTrace => {
                            let Some(flow) = stop_flow.as_mut() else {
                                continue;
                            };
                            flow.frames = message
                                .pointer("/body/stackFrames")
                                .and_then(Value::as_array)
                                .map(|frames| frames.iter().filter_map(parse_frame).collect())
                                .unwrap_or_default();
                            let Some(frame_id) = flow.frames.first().map(|f| f.id) else {
                                finish_stop(stop_flow.take().unwrap(), &events, &wake);
                                continue;
                            };
                            if send_request(
                                &mut stdin,
                                &mut seq,
                                &mut pending,
                                "scopes",
                                json!({"frameId": frame_id}),
                                PendingKind::Scopes,
                            )
                            .is_err()
                            {
                                failure = Some("Could not request scopes".into());
                                break 'running;
                            }
                        }
                        PendingKind::Scopes => {
                            let Some(flow) = stop_flow.as_mut() else {
                                continue;
                            };
                            flow.pending_scopes = message
                                .pointer("/body/scopes")
                                .and_then(Value::as_array)
                                .map(|scopes| {
                                    scopes
                                        .iter()
                                        .filter_map(|scope| {
                                            let name = scope.get("name")?.as_str()?.to_string();
                                            let reference = scope
                                                .get("variablesReference")
                                                .and_then(Value::as_i64)?;
                                            let expensive = scope
                                                .get("expensive")
                                                .and_then(Value::as_bool)
                                                .unwrap_or(false);
                                            (!expensive).then_some((name, reference))
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            match request_next_scope(&mut stdin, &mut seq, &mut pending, flow) {
                                Ok(true) => {}
                                Ok(false) => finish_stop(stop_flow.take().unwrap(), &events, &wake),
                                Err(_) => {
                                    failure = Some("Could not request variables".into());
                                    break 'running;
                                }
                            }
                        }
                        PendingKind::Variables { scope_name } => {
                            let Some(flow) = stop_flow.as_mut() else {
                                continue;
                            };
                            let variables = message
                                .pointer("/body/variables")
                                .and_then(Value::as_array)
                                .map(|vars| vars.iter().filter_map(parse_variable).collect())
                                .unwrap_or_default();
                            flow.scopes.push(Scope {
                                name: scope_name,
                                variables,
                            });
                            match request_next_scope(&mut stdin, &mut seq, &mut pending, flow) {
                                Ok(true) => {}
                                Ok(false) => finish_stop(stop_flow.take().unwrap(), &events, &wake),
                                Err(_) => {
                                    failure = Some("Could not request variables".into());
                                    break 'running;
                                }
                            }
                        }
                        PendingKind::VariablesOnDemand { reference } => {
                            let variables = message
                                .pointer("/body/variables")
                                .and_then(Value::as_array)
                                .map(|vars| vars.iter().filter_map(parse_variable).collect())
                                .unwrap_or_default();
                            emit(
                                Event::Variables { reference, variables },
                                &events,
                                &wake,
                            );
                        }
                        PendingKind::Other => {}
                    }
                }
                Some("event") => {
                    let event_name = message.get("event").and_then(Value::as_str).unwrap_or("");
                    match event_name {
                        "initialized" => {
                            for (path, lines) in &request.breakpoints {
                                let breakpoints: Vec<Value> = lines
                                    .iter()
                                    .map(|line| json!({"line": line + 1}))
                                    .collect();
                                if send_request(
                                    &mut stdin,
                                    &mut seq,
                                    &mut pending,
                                    "setBreakpoints",
                                    json!({
                                        "source": {"path": path.to_string_lossy()},
                                        "breakpoints": breakpoints,
                                    }),
                                    PendingKind::Other,
                                )
                                .is_err()
                                {
                                    failure = Some("Could not set breakpoints".into());
                                    break 'running;
                                }
                            }
                            if send_request(
                                &mut stdin,
                                &mut seq,
                                &mut pending,
                                "configurationDone",
                                Value::Null,
                                PendingKind::Other,
                            )
                            .is_err()
                            {
                                failure = Some("Could not finish configuration".into());
                                break 'running;
                            }
                        }
                        "stopped" => {
                            let thread_id = message
                                .pointer("/body/threadId")
                                .and_then(Value::as_i64)
                                .unwrap_or(0);
                            let reason = message
                                .pointer("/body/reason")
                                .and_then(Value::as_str)
                                .unwrap_or("stopped")
                                .to_string();
                            stop_flow = Some(StopFlow {
                                thread_id,
                                reason,
                                ..Default::default()
                            });
                            if send_request(
                                &mut stdin,
                                &mut seq,
                                &mut pending,
                                "threads",
                                Value::Null,
                                PendingKind::Threads,
                            )
                            .is_err()
                            {
                                failure = Some("Could not request threads".into());
                                break 'running;
                            }
                        }
                        "continued" => {
                            stop_flow = None;
                            emit(Event::Continued, &events, &wake);
                        }
                        "output" => {
                            if let Some(text) =
                                message.pointer("/body/output").and_then(Value::as_str)
                            {
                                emit(
                                    Event::Output {
                                        text: text.to_string(),
                                    },
                                    &events,
                                    &wake,
                                );
                            }
                        }
                        "exited" | "terminated" => {
                            emit(Event::Terminated, &events, &wake);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        let active_thread = stop_flow.as_ref().map(|f| f.thread_id).unwrap_or(1);
        let sent = match commands.recv_timeout(Duration::from_millis(30)) {
            Ok(Command::Disconnect) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Command::Continue) => {
                send_thread_command(&mut stdin, &mut seq, &mut pending, "continue", active_thread)
            }
            Ok(Command::Next) => {
                send_thread_command(&mut stdin, &mut seq, &mut pending, "next", active_thread)
            }
            Ok(Command::StepIn) => {
                send_thread_command(&mut stdin, &mut seq, &mut pending, "stepIn", active_thread)
            }
            Ok(Command::StepOut) => {
                send_thread_command(&mut stdin, &mut seq, &mut pending, "stepOut", active_thread)
            }
            Ok(Command::Pause) => {
                send_thread_command(&mut stdin, &mut seq, &mut pending, "pause", active_thread)
            }
            Ok(Command::Variables(reference)) => send_request(
                &mut stdin,
                &mut seq,
                &mut pending,
                "variables",
                json!({"variablesReference": reference}),
                PendingKind::VariablesOnDemand { reference },
            ),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(()),
        };
        if sent.is_err() {
            failure = Some("Could not send debug command".into());
            break;
        }
    }
    let _ = send_request(
        &mut stdin,
        &mut seq,
        &mut pending,
        "disconnect",
        json!({"terminateDebuggee": true}),
        PendingKind::Other,
    );
    drop(stdin);
    for _ in 0..10 {
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let _ = reader.join();
    if let Some(thread) = stderr_reader {
        let _ = thread.join();
    }
    if let Some(error) = failure {
        emit(Event::Failed { message: error }, &events, &wake);
    }
}

fn parse_frame(value: &Value) -> Option<StackFrame> {
    Some(StackFrame {
        id: value.get("id")?.as_i64()?,
        name: value.get("name")?.as_str()?.to_string(),
        path: value
            .pointer("/source/path")
            .and_then(Value::as_str)
            .map(PathBuf::from),
        line: value.get("line").and_then(Value::as_u64).unwrap_or(0) as u32,
    })
}

fn parse_variable(value: &Value) -> Option<Variable> {
    Some(Variable {
        name: value.get("name")?.as_str()?.to_string(),
        value: value
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        kind: value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        variables_reference: value
            .get("variablesReference")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    })
}

fn write_packet(writer: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

fn read_packet(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line.trim().is_empty() {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse::<usize>().ok();
        }
        if line.len() > 8192 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "DAP header too large"));
        }
    }
    let length = length
        .filter(|n| *n <= 16 * 1024 * 1024)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid DAP content length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map(Some).map_err(Into::into)
}
