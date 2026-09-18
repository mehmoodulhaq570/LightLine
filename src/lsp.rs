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
    Stopped {
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

fn hover_request(id: u64, hover: &PendingHover) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"textDocument/hover","params":{
        "textDocument":{"uri":hover.uri},
        "position":{"line":hover.position.line,"character":hover.position.character}
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

fn run_server(
    root: PathBuf,
    config: ServerConfig,
    commands: Receiver<Command>,
    events: Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    #[cfg(windows)]
    let mut process = if config.language == Language::Python {
        let mut command = ProcessCommand::new("cmd.exe");
        command.args(["/D", "/C", "pyright-langserver"]);
        command
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
                ". Install Pyright with `npm install -g pyright` so pyright-langserver is on PATH"
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
    let mut hovers: HashMap<u64, PendingHover> = HashMap::new();
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
                    if send_command(&mut stdin, &config, command, &mut hovers).is_err() {
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
            } else if let Some(id) = message.get("id").and_then(Value::as_u64)
                && let Some(mut hover) = hovers.remove(&id)
            {
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
                hovers.insert(id, hover);
            } else {
                index += 1;
            }
        }
        match commands.recv_timeout(Duration::from_millis(30)) {
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(command) if ready => {
                if send_command(&mut stdin, &config, command, &mut hovers).is_err() {
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
    if let Some(mut error) = failure {
        if let Ok(stderr) = stderr.lock()
            && !stderr.trim().is_empty()
        {
            error.push_str(": ");
            error.push_str(stderr.trim());
        }
        emit(stopped(config.language, error), &events, &wake);
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
    hovers: &mut HashMap<u64, PendingHover>,
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
            hovers.insert(id, hover);
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
}
