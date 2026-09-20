//! Small, on-demand LSP clients for language-support slices.

use serde_json::{Map, Value, json};
use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command as ProcessCommand, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
}

impl Language {
    pub fn name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Python => "Python",
        }
    }

    fn language_id(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: u8,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

#[derive(Clone, Debug)]
pub struct TextEdit {
    pub range: Range,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct CompletionItem {
    pub label: String,
    pub kind: u8,
    pub detail: Option<String>,
    // Text to insert: the server's textEdit.newText, else insertText, else
    // label, with any snippet placeholders stripped to plain text.
    pub insert: String,
    // The server's own replacement range, when it sent a textEdit. The GUI
    // applies this range if present, otherwise it replaces the identifier
    // prefix it tracked when the request was made.
    pub edit_start: Option<Position>,
    pub edit_end: Option<Position>,
}

pub enum Command {
    Open {
        uri: String,
        text: String,
        version: i32,
    },
    Change {
        uri: String,
        version: i32,
        range: Range,
        text: String,
    },
    Save {
        uri: String,
    },
    Close {
        uri: String,
    },
    Hover {
        id: u64,
        uri: String,
        version: i32,
        position: Position,
    },
    Definition {
        id: u64,
        uri: String,
        version: i32,
        position: Position,
    },
    Format {
        id: u64,
        uri: String,
        version: i32,
    },
    Completion {
        id: u64,
        uri: String,
        version: i32,
        position: Position,
    },
    Shutdown,
}

pub enum Event {
    Ready {
        language: Language,
    },
    Diagnostics {
        language: Language,
        uri: String,
        version: Option<i32>,
        items: Vec<Diagnostic>,
    },
    Hover {
        language: Language,
        id: u64,
        uri: String,
        version: i32,
        text: Option<String>,
    },
    Definition {
        language: Language,
        id: u64,
        uri: String,
        version: i32,
        targets: Vec<Location>,
    },
    Format {
        language: Language,
        id: u64,
        uri: String,
        version: i32,
        edits: Vec<TextEdit>,
    },
    Completion {
        language: Language,
        id: u64,
        uri: String,
        version: i32,
        items: Vec<CompletionItem>,
    },
    Stopped {
        language: Language,
        message: String,
    },
    Status {
        language: Language,
        message: String,
    },
}

pub struct Client {
    language: Language,
    root: PathBuf,
    sender: Sender<Command>,
}

#[derive(Clone, Debug)]
struct ServerConfig {
    language: Language,
    display_name: &'static str,
    command: String,
    args: Vec<String>,
    settings: Value,
}

struct PendingHover {
    uri: String,
    version: i32,
    position: Position,
    retries: u8,
}

// Tracks in-flight requests by id so the reader loop can route each response
// back to the right kind of event. Hover keeps a retry budget because
// rust-analyzer answers with a "server busy" code while it is still indexing.
#[derive(Default)]
struct Pending {
    hovers: HashMap<u64, PendingHover>,
    definitions: HashMap<u64, PendingHover>,
    formats: HashMap<u64, (String, i32)>,
    completions: HashMap<u64, PendingHover>,
}

fn hover_request(id: u64, hover: &PendingHover) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"textDocument/hover","params":{
        "textDocument":{"uri":hover.uri},
        "position":{"line":hover.position.line,"character":hover.position.character}
    }})
}

fn definition_request(id: u64, request: &PendingHover) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"textDocument/definition","params":{
        "textDocument":{"uri":request.uri},
        "position":{"line":request.position.line,"character":request.position.character}
    }})
}

fn format_request(id: u64, uri: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"textDocument/formatting","params":{
        "textDocument":{"uri":uri},
        "options":{"tabSize":4,"insertSpaces":true}
    }})
}

fn completion_request(id: u64, request: &PendingHover) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"textDocument/completion","params":{
        "textDocument":{"uri":request.uri},
        "position":{"line":request.position.line,"character":request.position.character},
        "context":{"triggerKind":1}
    }})
}

impl Client {
    pub fn start(
        language: Language,
        root: PathBuf,
        python_interpreter: Option<PathBuf>,
        events: Sender<Event>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (sender, commands) = mpsc::channel();
        let server_root = root.clone();
        let config = server_config(language, python_interpreter.as_deref());
        thread::spawn(move || run_server(server_root, config, commands, events, wake));
        Self {
            language,
            root,
            sender,
        }
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn send(&self, command: Command) -> bool {
        self.sender.send(command).is_ok()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Shutdown);
    }
}

fn server_config(language: Language, python_interpreter: Option<&Path>) -> ServerConfig {
    match language {
        Language::Rust => ServerConfig {
            language,
            display_name: "rust-analyzer",
            command: "rust-analyzer".into(),
            args: Vec::new(),
            settings: Value::Null,
        },
        Language::Python => ServerConfig {
            language,
            display_name: "Pyright",
            command: "pyright-langserver".into(),
            args: vec!["--stdio".into()],
            settings: python_settings(python_interpreter),
        },
    }
}

fn python_settings(interpreter: Option<&Path>) -> Value {
    let mut python = Map::new();
    if let Some(path) = interpreter {
        python.insert(
            "pythonPath".into(),
            Value::String(path.to_string_lossy().into_owned()),
        );
        if let Some((venv_path, venv)) = venv_from_interpreter(path) {
            python.insert(
                "venvPath".into(),
                Value::String(venv_path.to_string_lossy().into_owned()),
            );
            python.insert("venv".into(), Value::String(venv));
        }
    }
    json!({
        "python": Value::Object(python),
        "python.analysis": {
            "diagnosticMode": "openFilesOnly",
            "autoSearchPaths": true,
            "useLibraryCodeForTypes": true
        }
    })
}

fn venv_from_interpreter(path: &Path) -> Option<(PathBuf, String)> {
    let file = path.file_name()?.to_string_lossy();
    if !file.eq_ignore_ascii_case("python.exe") && !file.eq_ignore_ascii_case("python") {
        return None;
    }
    let scripts_or_bin = path.parent()?;
    let folder_name = scripts_or_bin.file_name()?.to_string_lossy();
    if !folder_name.eq_ignore_ascii_case("Scripts") && !folder_name.eq_ignore_ascii_case("bin") {
        return None;
    }
    let venv = scripts_or_bin.parent()?;
    let parent = venv.parent()?.to_path_buf();
    let name = venv.file_name()?.to_string_lossy().into_owned();
    Some((parent, name))
}

fn emit(event: Event, events: &Sender<Event>, wake: &Arc<dyn Fn() + Send + Sync>) {
    if events.send(event).is_ok() {
        wake();
    }
}

fn stopped(language: Language, message: impl Into<String>) -> Event {
    Event::Stopped {
        language,
        message: message.into(),
    }
}

fn stop_message(config: &ServerConfig, error: &str, stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.contains("is not recognized as an internal or external command") {
        if config.language == Language::Python {
            return NODE_MISSING.to_string();
        }
        return format!(
            "{} is not installed or not on PATH. Install it with `rustup component add rust-analyzer rust-src`, then reopen this file. Editing and running still work without it.",
            config.display_name
        );
    }
    if stderr.is_empty() {
        error.to_string()
    } else {
        format!("{error}: {stderr}")
    }
}

const NODE_MISSING: &str = "Pyright setup needs Node.js. Install it once from nodejs.org (or `winget install OpenJS.NodeJS.LTS`) and reopen this file — LightLine then downloads and configures Pyright automatically. Editing and running Python work without it.";

/// Resolve a program by looking for any of `file_names` in each PATH folder.
fn find_in_path(file_names: &[&str]) -> Option<PathBuf> {
    find_in_path_with(&std::env::var_os("PATH")?, file_names)
}

fn find_in_path_with(path_var: &std::ffi::OsStr, file_names: &[&str]) -> Option<PathBuf> {
    std::env::split_paths(path_var)
        .flat_map(|dir| file_names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

/// LightLine's own Pyright location, like IDEs that provision language servers
/// for you: `%APPDATA%\LightLine\pyright`.
fn pyright_home() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("APPDATA")?)
            .join("LightLine")
            .join("pyright"),
    )
}

fn managed_pyright_script() -> Option<PathBuf> {
    let script = pyright_home()?
        .join("node_modules")
        .join("pyright")
        .join("langserver.index.js");
    script.is_file().then_some(script)
}

enum PyrightLaunch {
    /// A user-installed `pyright-langserver` on PATH (run through the cmd wrapper).
    OnPath,
    /// The managed copy under `%APPDATA%\LightLine\pyright`, run with Node.
    Node { script: PathBuf },
    /// Nothing installed yet, but npm and Node exist: install the managed copy.
    Install,
    /// Node.js itself is missing, so no automatic setup is possible.
    Missing(String),
}

fn resolve_pyright() -> PyrightLaunch {
    if find_in_path(&[
        "pyright-langserver.cmd",
        "pyright-langserver.exe",
        "pyright-langserver.bat",
        "pyright-langserver",
    ])
    .is_some()
    {
        return PyrightLaunch::OnPath;
    }
    let node = find_in_path(&["node.exe", "node"]).is_some();
    if let Some(script) = managed_pyright_script() {
        return if node {
            PyrightLaunch::Node { script }
        } else {
            PyrightLaunch::Missing(NODE_MISSING.into())
        };
    }
    if node && find_in_path(&["npm.cmd", "npm"]).is_some() {
        return PyrightLaunch::Install;
    }
    PyrightLaunch::Missing(NODE_MISSING.into())
}

/// One-time silent install of Pyright into LightLine's own data folder.
fn install_pyright() -> Result<PathBuf, String> {
    let home = pyright_home().ok_or_else(|| "could not resolve %APPDATA%".to_string())?;
    std::fs::create_dir_all(&home).map_err(|error| error.to_string())?;
    let npm = find_in_path(&["npm.cmd", "npm"]).ok_or("npm not found on PATH")?;
    let mut command = ProcessCommand::new(npm);
    command
        .args([
            "install",
            "--prefix",
            home.to_str().unwrap_or_default(),
            "pyright",
            "--no-audit",
            "--no-fund",
            "--loglevel=error",
        ])
        .current_dir(&home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`npm install pyright` failed: {stderr}"));
    }
    managed_pyright_script().ok_or_else(|| "npm install finished without Pyright".to_string())
}

fn run_server(
    root: PathBuf,
    config: ServerConfig,
    commands: Receiver<Command>,
    events: Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    #[cfg(windows)]
    let mut process = if config.language == Language::Python {
        match resolve_pyright() {
            PyrightLaunch::OnPath => {
                let mut command = ProcessCommand::new("cmd.exe");
                command.args(["/D", "/C", "pyright-langserver"]);
                command
            }
            PyrightLaunch::Node { script } => {
                let mut command = ProcessCommand::new("node");
                command.arg(script);
                command
            }
            PyrightLaunch::Install => {
                emit(
                    Event::Status {
                        language: config.language,
                        message: "Setting up Pyright automatically (one-time download)…".into(),
                    },
                    &events,
                    &wake,
                );
                match install_pyright() {
                    Ok(script) => {
                        let mut command = ProcessCommand::new("node");
                        command.arg(script);
                        command
                    }
                    Err(error) => {
                        emit(
                            stopped(
                                config.language,
                                format!("Could not set up Pyright automatically: {error}. You can still install it manually with `npm.cmd install -g pyright`."),
                            ),
                            &events,
                            &wake,
                        );
                        return;
                    }
                }
            }
            PyrightLaunch::Missing(message) => {
                emit(stopped(config.language, message), &events, &wake);
                return;
            }
        }
    } else {
        ProcessCommand::new(&config.command)
    };
    #[cfg(not(windows))]
    let mut process = ProcessCommand::new(&config.command);
    process
        .args(&config.args)
        .current_dir(&root)
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
            let hint = if config.language == Language::Python {
                ". Install Node.js once (nodejs.org) and LightLine sets Pyright up automatically"
            } else {
                ""
            };
            emit(
                stopped(
                    config.language,
                    format!("Could not start {}: {error}{hint}", config.display_name),
                ),
                &events,
                &wake,
            );
            return;
        }
    };
    let stderr = Arc::new(Mutex::new(String::new()));
    let stderr_copy = stderr.clone();
    let stderr_reader = child.stderr.take().map(|mut output| {
        thread::spawn(move || {
            let mut message = String::new();
            let _ = output.by_ref().take(4096).read_to_string(&mut message);
            let _ = io::copy(&mut output, &mut io::sink());
            if let Ok(mut saved) = stderr_copy.lock() {
                *saved = message;
            }
        })
    });
    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        return;
    };
    let (incoming_tx, incoming_rx) = mpsc::channel();
    let reader_name = config.display_name;
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
                    let _ = incoming_tx.send(Err(format!("{reader_name} closed its output")));
                    break;
                }
                Err(error) => {
                    let _ = incoming_tx.send(Err(format!("LSP read failed: {error}")));
                    break;
                }
            }
        }
    });

    let root_uri = file_uri(&root);
    let name = root.file_name().unwrap_or_default().to_string_lossy();
    let mut params = json!({
        "processId": std::process::id(),
        "rootUri": root_uri,
        "workspaceFolders": [{"uri":root_uri,"name":name}],
        "capabilities": {
            "general": {"positionEncodings":["utf-16"]},
            "workspace": {"configuration":true,"workspaceFolders":true},
            "textDocument": {
                "synchronization": {"didSave":true},
                "hover": {"contentFormat":["plaintext","markdown"]},
                "definition": {"linkSupport":false},
                "formatting": {},
                "completion": {
                    "contextSupport":true,
                    "completionItem": {
                        "snippetSupport":false,
                        "documentationFormat":["plaintext"],
                        "resolveSupport":{"properties":[]}
                    }
                },
                "publishDiagnostics": {"versionSupport":true}
            }
        },
        "clientInfo": {"name":"LightLine","version":"0.1.0"}
    });
    if !config.settings.is_null() {
        params["initializationOptions"] = json!({"settings": config.settings.clone()});
    }
    let initialize = json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params":params});
    if write_packet(&mut stdin, &initialize).is_err() {
        emit(
            stopped(
                config.language,
                format!("Could not initialize {}", config.display_name),
            ),
            &events,
            &wake,
        );
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        return;
    }

    let mut ready = false;
    let mut waiting = VecDeque::new();
    let mut pending = Pending::default();
    let mut hover_retries: Vec<(Instant, u64, PendingHover)> = Vec::new();
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
            if is_initialize_response(&message) {
                if let Some(error) = message.get("error") {
                    failure = Some(format!(
                        "{} initialization failed: {error}",
                        config.display_name
                    ));
                    break 'running;
                }
                if write_packet(
                    &mut stdin,
                    &json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
                )
                .is_err()
                {
                    failure = Some(format!(
                        "Could not finish {} initialization",
                        config.display_name
                    ));
                    break 'running;
                }
                if !config.settings.is_null()
                    && write_packet(
                        &mut stdin,
                        &json!({"jsonrpc":"2.0","method":"workspace/didChangeConfiguration","params":{"settings":config.settings.clone()}}),
                    )
                    .is_err()
                {
                    failure = Some(format!("Could not configure {}", config.display_name));
                    break 'running;
                }
                ready = true;
                emit(
                    Event::Ready {
                        language: config.language,
                    },
                    &events,
                    &wake,
                );
                while let Some(command) = waiting.pop_front() {
                    if send_command(&mut stdin, &config, command, &mut pending).is_err() {
                        failure = Some(format!("Could not write to {}", config.display_name));
                        break 'running;
                    }
                }
            } else if let Some(method) = message.get("method").and_then(Value::as_str) {
                if let Some(id) = message.get("id") {
                    let result = match method {
                        "workspace/configuration" => configuration_response(&message, &config),
                        "workspace/workspaceFolders" => json!([{"uri":root_uri,"name":name}]),
                        "workspace/applyEdit" => json!({"applied":false}),
                        _ => Value::Null,
                    };
                    if write_packet(
                        &mut stdin,
                        &json!({"jsonrpc":"2.0","id":id,"result":result}),
                    )
                    .is_err()
                    {
                        failure = Some(format!("Could not answer {}", config.display_name));
                        break 'running;
                    }
                } else if method == "textDocument/publishDiagnostics"
                    && let Some((uri, version, items)) = parse_diagnostics(&message)
                {
                    emit(
                        Event::Diagnostics {
                            language: config.language,
                            uri,
                            version,
                            items,
                        },
                        &events,
                        &wake,
                    );
                }
            } else if let Some(id) = message.get("id").and_then(Value::as_u64) {
                if let Some(mut hover) = pending.hovers.remove(&id) {
                    if message.pointer("/error/code").and_then(Value::as_i64) == Some(-32801)
                        && hover.retries < 2
                    {
                        hover.retries += 1;
                        hover_retries.push((
                            Instant::now() + Duration::from_millis(250 * u64::from(hover.retries)),
                            id,
                            hover,
                        ));
                        continue;
                    }
                    let text = message.get("result").and_then(hover_text);
                    emit(
                        Event::Hover {
                            language: config.language,
                            id,
                            uri: hover.uri,
                            version: hover.version,
                            text,
                        },
                        &events,
                        &wake,
                    );
                } else if let Some(request) = pending.definitions.remove(&id) {
                    let targets = parse_locations(&message);
                    emit(
                        Event::Definition {
                            language: config.language,
                            id,
                            uri: request.uri,
                            version: request.version,
                            targets,
                        },
                        &events,
                        &wake,
                    );
                } else if let Some((uri, version)) = pending.formats.remove(&id) {
                    let edits = parse_text_edits(&message);
                    emit(
                        Event::Format {
                            language: config.language,
                            id,
                            uri,
                            version,
                            edits,
                        },
                        &events,
                        &wake,
                    );
                } else if let Some(request) = pending.completions.remove(&id) {
                    let items = parse_completion(&message);
                    emit(
                        Event::Completion {
                            language: config.language,
                            id,
                            uri: request.uri,
                            version: request.version,
                            items,
                        },
                        &events,
                        &wake,
                    );
                }
            }
        }
        let mut index = 0;
        while index < hover_retries.len() {
            if hover_retries[index].0 <= Instant::now() {
                let (_, id, hover) = hover_retries.swap_remove(index);
                let request = hover_request(id, &hover);
                if write_packet(&mut stdin, &request).is_err() {
                    failure = Some(format!(
                        "Could not retry hover with {}",
                        config.display_name
                    ));
                    break 'running;
                }
                pending.hovers.insert(id, hover);
            } else {
                index += 1;
            }
        }
        match commands.recv_timeout(Duration::from_millis(30)) {
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(command) if ready => {
                if send_command(&mut stdin, &config, command, &mut pending).is_err() {
                    failure = Some(format!("Could not write to {}", config.display_name));
                    break;
                }
            }
            Ok(command) => waiting.push_back(command),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    if ready {
        let _ = write_packet(
            &mut stdin,
            &json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}),
        );
        let _ = write_packet(
            &mut stdin,
            &json!({"jsonrpc":"2.0","method":"exit","params":null}),
        );
    }
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
        let stderr_text = stderr.lock().map(|saved| saved.clone()).unwrap_or_default();
        emit(
            stopped(
                config.language,
                stop_message(&config, &error, &stderr_text),
            ),
            &events,
            &wake,
        );
    }
}

fn configuration_response(message: &Value, config: &ServerConfig) -> Value {
    let Some(items) = message.pointer("/params/items").and_then(Value::as_array) else {
        return Value::Array(Vec::new());
    };
    let responses = items
        .iter()
        .map(|item| {
            item.get("section")
                .and_then(Value::as_str)
                .and_then(|section| config.settings.get(section).cloned())
                .unwrap_or(Value::Null)
        })
        .collect();
    Value::Array(responses)
}

fn send_command(
    stdin: &mut ChildStdin,
    config: &ServerConfig,
    command: Command,
    pending: &mut Pending,
) -> io::Result<()> {
    let message = match command {
        Command::Open { uri, text, version } => {
            json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":config.language.language_id(),"version":version,"text":text}}})
        }
        Command::Change {
            uri,
            version,
            range,
            text,
        } => {
            json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":version},"contentChanges":[{"range":{"start":{"line":range.start.line,"character":range.start.character},"end":{"line":range.end.line,"character":range.end.character}},"text":text}]}})
        }
        Command::Save { uri } => {
            json!({"jsonrpc":"2.0","method":"textDocument/didSave","params":{"textDocument":{"uri":uri}}})
        }
        Command::Close { uri } => {
            json!({"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":uri}}})
        }
        Command::Hover {
            id,
            uri,
            version,
            position,
        } => {
            let hover = PendingHover {
                uri,
                version,
                position,
                retries: 0,
            };
            let request = hover_request(id, &hover);
            pending.hovers.insert(id, hover);
            request
        }
        Command::Definition {
            id,
            uri,
            version,
            position,
        } => {
            let request_state = PendingHover {
                uri,
                version,
                position,
                retries: 0,
            };
            let request = definition_request(id, &request_state);
            pending.definitions.insert(id, request_state);
            request
        }
        Command::Format { id, uri, version } => {
            let request = format_request(id, &uri);
            pending.formats.insert(id, (uri, version));
            request
        }
        Command::Completion {
            id,
            uri,
            version,
            position,
        } => {
            let request_state = PendingHover {
                uri,
                version,
                position,
                retries: 0,
            };
            let request = completion_request(id, &request_state);
            pending.completions.insert(id, request_state);
            request
        }
        Command::Shutdown => return Ok(()),
    };
    if std::env::var_os("LIGHTLINE_LSP_TRACE").is_some() {
        eprintln!(
            "{} sending {}",
            config.display_name,
            message
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("response")
        );
    }
    write_packet(stdin, &message)
}

fn is_initialize_response(message: &Value) -> bool {
    message.get("method").is_none() && message.get("id").and_then(Value::as_u64) == Some(1)
}

pub fn file_uri(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let normalized = if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(local) = raw.strip_prefix(r"\\?\") {
        local.to_owned()
    } else {
        raw.into_owned()
    };
    let mut slashes = normalized.replace('\\', "/");
    if slashes.as_bytes().get(1) == Some(&b':') {
        let drive = slashes[..1].to_ascii_lowercase();
        slashes.replace_range(..1, &drive);
    }
    let prefix = if slashes.starts_with("//") {
        "file:"
    } else {
        "file:///"
    };
    let mut uri = prefix.to_owned();
    for byte in slashes.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'_' | b'.' | b'~') {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// Compare file URIs even when a server chooses different percent escaping.
pub fn same_file_uri(left: &str, right: &str) -> bool {
    fn decoded(uri: &str) -> Vec<u8> {
        let bytes = uri.as_bytes();
        let mut result = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%'
                && let (Some(high), Some(low)) = (bytes.get(index + 1), bytes.get(index + 2))
                && let (Some(high), Some(low)) =
                    ((*high as char).to_digit(16), (*low as char).to_digit(16))
            {
                result.push((high * 16 + low) as u8);
                index += 3;
            } else {
                result.push(bytes[index]);
                index += 1;
            }
        }
        result
    }
    let left = decoded(left);
    let right = decoded(right);
    #[cfg(windows)]
    {
        left.eq_ignore_ascii_case(&right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

pub fn utf16_to_byte(line: &str, column: u32) -> usize {
    let mut units = 0;
    for (byte, ch) in line.char_indices() {
        if units >= column {
            return byte;
        }
        let next = units + ch.len_utf16() as u32;
        if next > column {
            return byte;
        }
        units = next;
    }
    line.len()
}

fn parse_position(value: &Value) -> Option<Position> {
    Some(Position {
        line: value.get("line")?.as_u64()?.try_into().ok()?,
        character: value.get("character")?.as_u64()?.try_into().ok()?,
    })
}

fn parse_diagnostics(message: &Value) -> Option<(String, Option<i32>, Vec<Diagnostic>)> {
    let params = message.get("params")?;
    let uri = params.get("uri")?.as_str()?.to_owned();
    let version = params
        .get("version")
        .and_then(Value::as_i64)
        .and_then(|v| v.try_into().ok());
    let items = params
        .get("diagnostics")?
        .as_array()?
        .iter()
        .take(300)
        .filter_map(|item| {
            let range = item.get("range")?;
            Some(Diagnostic {
                range: Range {
                    start: parse_position(range.get("start")?)?,
                    end: parse_position(range.get("end")?)?,
                },
                severity: item
                    .get("severity")
                    .and_then(Value::as_u64)
                    .unwrap_or(3)
                    .min(4) as u8,
                message: item.get("message")?.as_str()?.to_owned(),
            })
        })
        .collect();
    Some((uri, version, items))
}

fn hover_text(result: &Value) -> Option<String> {
    let contents = result.get("contents")?;
    let raw = if let Some(text) = contents.as_str() {
        text.to_owned()
    } else if let Some(value) = contents.get("value").and_then(Value::as_str) {
        value.to_owned()
    } else {
        let parts = contents.as_array()?;
        parts
            .iter()
            .filter_map(|part| {
                part.as_str()
                    .or_else(|| part.get("value").and_then(Value::as_str))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let clean = raw
        .lines()
        .filter(|line| !line.trim_start().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace('`', "");
    let clean: String = clean.chars().take(800).collect();
    (!clean.trim().is_empty()).then_some(clean)
}

fn parse_range(value: &Value) -> Option<Range> {
    Some(Range {
        start: parse_position(value.get("start")?)?,
        end: parse_position(value.get("end")?)?,
    })
}

fn parse_location(value: &Value) -> Option<Location> {
    // A LocationLink carries targetUri/targetSelectionRange instead of uri/range.
    if let Some(uri) = value.get("targetUri").and_then(Value::as_str) {
        let range = value
            .get("targetSelectionRange")
            .and_then(parse_range)
            .or_else(|| value.get("targetRange").and_then(parse_range))?;
        return Some(Location {
            uri: uri.to_owned(),
            range,
        });
    }
    let uri = value.get("uri")?.as_str()?;
    let range = parse_range(value.get("range")?)?;
    Some(Location {
        uri: uri.to_owned(),
        range,
    })
}

// textDocument/definition answers with a single Location, an array of
// Locations, an array of LocationLinks, or null when there is no definition.
fn parse_locations(message: &Value) -> Vec<Location> {
    let result = match message.get("result") {
        Some(Value::Null) | None => return Vec::new(),
        Some(value) => value,
    };
    if let Some(items) = result.as_array() {
        return items.iter().filter_map(parse_location).take(64).collect();
    }
    if result.is_object() {
        return parse_location(result).into_iter().collect();
    }
    Vec::new()
}

// textDocument/formatting answers with an array of TextEdits or null.
fn parse_text_edits(message: &Value) -> Vec<TextEdit> {
    let Some(items) = message.get("result").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let range = parse_range(item.get("range")?)?;
            let text = item.get("newText")?.as_str()?.to_owned();
            Some(TextEdit { range, text })
        })
        .take(20_000)
        .collect()
}

// textDocument/completion answers with either a bare array of items or a
// { isIncomplete, items } object; each item carries a label plus optional
// kind/detail and an insertText or textEdit.
fn parse_completion(message: &Value) -> Vec<CompletionItem> {
    let result = match message.get("result") {
        Some(Value::Null) | None => return Vec::new(),
        Some(value) => value,
    };
    let Some(items) = result.get("items").and_then(Value::as_array).or_else(|| result.as_array())
    else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?;
            let text_edit = item.get("textEdit").filter(|edit| edit.is_object());
            let insert = text_edit
                .and_then(|edit| edit.get("newText").and_then(Value::as_str))
                .or_else(|| item.get("insertText").and_then(Value::as_str))
                .unwrap_or(label);
            let insert = strip_snippet(insert);
            // A textEdit may be a plain {range,newText} or an
            // InsertReplaceEdit with separate insert/replace ranges.
            let edit_range = text_edit.and_then(|edit| {
                edit.get("range")
                    .or_else(|| edit.get("replace"))
                    .or_else(|| edit.get("insert"))
            });
            let (edit_start, edit_end) = edit_range.map_or((None, None), |range| {
                (
                    range.get("start").and_then(parse_position),
                    range.get("end").and_then(parse_position),
                )
            });
            Some(CompletionItem {
                label: label.to_owned(),
                kind: item.get("kind").and_then(Value::as_u64).unwrap_or(0).min(25) as u8,
                detail: item.get("detail").and_then(Value::as_str).map(str::to_owned),
                insert,
                edit_start,
                edit_end,
            })
        })
        .take(300)
        .collect()
}

// Drops snippet placeholders so a plain-text insert never leaves `${...}` or
// `$1` tab stops in the document: keeps the text after a `${n:placeholder}`
// colon, drops a bare `${n}`/`$n` tab stop.
fn strip_snippet(text: &str) -> String {
    if !text.contains('$') {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '$' {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        if chars.get(index + 1) == Some(&'{') {
            let mut depth = 1;
            let mut cursor = index + 2;
            let start = cursor;
            while cursor < chars.len() && depth > 0 {
                match chars[cursor] {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                if depth > 0 {
                    cursor += 1;
                }
            }
            let inner: String = chars[start..cursor.min(chars.len())].iter().collect();
            let body = inner.split_once(':').map_or("", |(_, value)| value);
            out.push_str(&strip_snippet(body));
            index = (cursor + 1).min(chars.len());
        } else {
            let mut cursor = index + 1;
            while cursor < chars.len() && chars[cursor].is_ascii_digit() {
                cursor += 1;
            }
            // A lone `$` with no digits is a literal dollar sign.
            if cursor == index + 1 {
                out.push('$');
            }
            index = cursor;
        }
    }
    out
}

/// Reverse of [`file_uri`]: turn a file:// URI back into a native path.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut text = percent_decode(rest.as_bytes());
    // Drop the leading slash on /C:/... Windows paths.
    #[cfg(windows)]
    if text.first() == Some(&b'/')
        && text.get(2) == Some(&b':')
        && text[1].is_ascii_alphabetic()
    {
        text.remove(0);
    }
    let path = String::from_utf8(text).ok()?;
    let path = path.replace('/', std::path::MAIN_SEPARATOR_STR);
    Some(PathBuf::from(path))
}

fn percent_decode(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let (Some(high), Some(low)) = (bytes.get(index + 1), bytes.get(index + 2))
            && let (Some(high), Some(low)) =
                ((*high as char).to_digit(16), (*low as char).to_digit(16))
        {
            result.push((high * 16 + low) as u8);
            index += 3;
        } else {
            result.push(bytes[index]);
            index += 1;
        }
    }
    result
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
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "LSP header too large",
            ));
        }
    }
    let length = length
        .filter(|n| *n <= 16 * 1024 * 1024)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid LSP content length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map(Some).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_server_stderr_becomes_an_install_hint() {
        let config = server_config(Language::Python, None);
        let message = stop_message(
            &config,
            "Pyright closed its output",
            "'pyright-langserver' is not recognized as an internal or external command,\r\noperable program or batch file.\r\n",
        );
        assert!(message.contains("Node.js"));
        assert!(message.contains("automatically"));
        assert!(!message.contains("not recognized"));
        let plain = stop_message(&config, "Pyright closed its output", "  ");
        assert_eq!(plain, "Pyright closed its output");
        let detailed = stop_message(&config, "Pyright closed its output", "boom");
        assert_eq!(detailed, "Pyright closed its output: boom");
        let rust = server_config(Language::Rust, None);
        let rust_message = stop_message(
            &rust,
            "rust-analyzer closed its output",
            "'rust-analyzer' is not recognized as an internal or external command",
        );
        assert!(rust_message.contains("rustup component add"));
    }

    #[test]
    fn find_in_path_with_locates_the_first_existing_candidate() {
        let dir = std::env::temp_dir().join(format!("lightline-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("pyright-langserver.cmd");
        std::fs::write(&script, "").unwrap();
        let path_var = std::env::join_paths([&dir, &std::env::temp_dir()]).unwrap();
        assert_eq!(
            find_in_path_with(&path_var, &["pyright-langserver.cmd", "pyright-langserver"]),
            Some(script)
        );
        assert_eq!(find_in_path_with(&path_var, &["definitely-missing.cmd"]), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn packets_and_unicode_positions_round_trip() {
        let payload = json!({"method":"textDocument/didChange","text":"a🦀é"});
        let mut bytes = Vec::new();
        write_packet(&mut bytes, &payload).unwrap();
        assert_eq!(
            read_packet(&mut BufReader::new(bytes.as_slice())).unwrap(),
            Some(payload)
        );
        assert_eq!(utf16_to_byte("a🦀é", 1), 1);
        assert_eq!(utf16_to_byte("a🦀é", 3), 5);
        assert_eq!(utf16_to_byte("a🦀é", 4), 7);
    }

    #[test]
    fn windows_paths_become_file_uris() {
        assert_eq!(
            file_uri(Path::new(r"\\?\D:\Code Work\src\main.rs")),
            "file:///d:/Code%20Work/src/main.rs"
        );
        assert_eq!(
            file_uri(Path::new(r"\\server\share\main.rs")),
            "file://server/share/main.rs"
        );
    }

    #[test]
    fn file_uri_comparison_accepts_server_percent_escaping() {
        assert!(same_file_uri(
            "file:///d:/Code%20Work/main.py",
            "file:///d%3A/Code%20Work/main.py"
        ));
    }

    #[test]
    fn diagnostics_keep_range_and_version() {
        let message = json!({"params":{"uri":"file:///D:/x.rs","version":3,"diagnostics":[{"range":{"start":{"line":1,"character":2},"end":{"line":1,"character":5}},"severity":1,"message":"problem"}]}});
        let (_, version, diagnostics) = parse_diagnostics(&message).unwrap();
        assert_eq!(version, Some(3));
        assert_eq!(diagnostics[0].range.start.character, 2);
        assert_eq!(diagnostics[0].message, "problem");
    }

    #[test]
    fn server_request_id_one_is_not_an_initialize_response() {
        assert!(!is_initialize_response(
            &json!({"jsonrpc":"2.0","id":1,"method":"workspace/configuration","params":{"items":[]}})
        ));
        assert!(is_initialize_response(
            &json!({"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}})
        ));
    }

    #[test]
    fn python_interpreter_settings_include_venv_details() {
        let settings = python_settings(Some(Path::new(
            r"D:\Projects\demo\.venv\Scripts\python.exe",
        )));
        assert_eq!(
            settings
                .pointer("/python/pythonPath")
                .and_then(Value::as_str),
            Some(r"D:\Projects\demo\.venv\Scripts\python.exe")
        );
        assert_eq!(
            settings.pointer("/python/venv").and_then(Value::as_str),
            Some(".venv")
        );
        assert_eq!(
            settings
                .pointer("/python.analysis/diagnosticMode")
                .and_then(Value::as_str),
            Some("openFilesOnly")
        );
    }

    #[test]
    fn configuration_requests_get_section_values() {
        let config = server_config(Language::Python, None);
        let request =
            json!({"params":{"items":[{"section":"python.analysis"},{"section":"missing"}]}});
        let response = configuration_response(&request, &config);
        assert!(response[0].is_object());
        assert!(response[1].is_null());
    }

    #[test]
    fn definition_results_accept_locations_and_links() {
        let single = json!({"result":{"uri":"file:///a.rs","range":{
            "start":{"line":3,"character":5},"end":{"line":3,"character":8}}}});
        let parsed = parse_locations(&single);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].uri, "file:///a.rs");
        assert_eq!(parsed[0].range.start.line, 3);

        let links = json!({"result":[{
            "targetUri":"file:///b.rs",
            "targetRange":{"start":{"line":1,"character":0},"end":{"line":1,"character":4}},
            "targetSelectionRange":{"start":{"line":1,"character":2},"end":{"line":1,"character":4}}
        }]});
        let parsed = parse_locations(&links);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].uri, "file:///b.rs");
        assert_eq!(parsed[0].range.start.character, 2);

        assert!(parse_locations(&json!({"result":null})).is_empty());
    }

    #[test]
    fn formatting_parses_text_edits() {
        let message = json!({"result":[
            {"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":2}},"newText":"  "},
            {"range":{"start":{"line":1,"character":0},"end":{"line":1,"character":1}},"newText":""}
        ]});
        let edits = parse_text_edits(&message);
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].text, "  ");
        assert_eq!(edits[1].range.end.line, 1);
        assert!(parse_text_edits(&json!({"result":null})).is_empty());
    }

    #[test]
    fn completion_parses_lists_and_snippets() {
        // Bare array form, a snippet insertText, and an object form with a
        // textEdit range are all accepted.
        let array = parse_completion(&json!({"result":[
            {"label":"println","kind":3,"insertText":"println!($1)"},
            {"label":"Vec","kind":7,"detail":"struct Vec","insertText":"Vec"}
        ]}));
        assert_eq!(array.len(), 2);
        assert_eq!(array[0].kind, 3);
        assert_eq!(array[0].insert, "println!()");
        assert_eq!(array[1].detail.as_deref(), Some("struct Vec"));
        assert!(array[1].edit_start.is_none());

        let object = parse_completion(&json!({"result":{"items":[
            {"label":"push","textEdit":{
                "newText":"push($0)",
                "range":{"start":{"line":2,"character":4},"end":{"line":2,"character":7}}
            }}
        ]}}));
        assert_eq!(object.len(), 1);
        assert_eq!(object[0].insert, "push()");
        assert_eq!(object[0].edit_start.unwrap().line, 2);
        assert_eq!(object[0].edit_end.unwrap().character, 7);

        assert!(parse_completion(&json!({"result":null})).is_empty());
        assert_eq!(strip_snippet("${1:foo}bar"), "foobar");
        assert_eq!(strip_snippet("a$1b"), "ab");
        assert_eq!(strip_snippet("cost $x"), "cost $x");
    }

    #[cfg(windows)]
    #[test]
    fn file_uri_round_trips_back_to_a_path() {
        let path = Path::new(r"D:\Projects\demo\src\main.rs");
        let uri = file_uri(path);
        let back = uri_to_path(&uri).unwrap();
        assert_eq!(back, PathBuf::from(r"d:\Projects\demo\src\main.rs"));
    }
}
