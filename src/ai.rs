//! Chatting with a language model through the OpenAI-compatible HTTP API
//! that Ollama, LM Studio and llama.cpp's server all offer, and cloud
//! services such as OpenAI, OpenRouter and Groq; and through Anthropic's own
//! Messages API for Claude (`is_anthropic`), which has its own request and
//! streamed events. Ollama on this PC is the default, so code never leaves
//! the machine unless the user points `aiEndpoint` somewhere else.
//!
//! A cloud service wants an API key. It's kept in Windows Credential Manager
//! under the service's host name (or read from OPENAI_API_KEY, or
//! ANTHROPIC_API_KEY for Claude), sent only to that host, and kept out of
//! every error message.
//!
//! Nothing here runs on its own: a request is made only when the user
//! connects, picks a model or sends a message, always on a worker thread,
//! and an answer streams back piece by piece so it can be shown (and
//! stopped) as it's written.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

/// Ollama's address on this PC.
pub const DEFAULT_ENDPOINT: &str = "http://localhost:11434";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const LIST_TIMEOUT: Duration = Duration::from_secs(10);
// The first answer can wait while the server loads the model into memory.
const FIRST_TOKEN_TIMEOUT: Duration = Duration::from_secs(300);
// An answer that goes quiet this long once it has started is stuck.
const STALL_TIMEOUT: Duration = Duration::from_secs(120);
// How often a quiet answer checks whether it was stopped.
const CANCEL_CHECK: Duration = Duration::from_millis(100);
// An answer longer than this is cut off.
const MAX_ANSWER_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

/// Where requests go: the server, and the API key it wants, if any.
#[derive(Clone)]
pub struct Server {
    pub endpoint: String,
    key: Option<String>,
    // Claude's own API rather than OpenAI's chat format.
    claude: bool,
}

impl Server {
    pub fn new(endpoint: impl Into<String>, key: Option<String>) -> Self {
        let endpoint = endpoint.into();
        Self {
            claude: is_anthropic(&endpoint),
            endpoint,
            key: key
                .map(|key| key.trim().to_string())
                .filter(|key| !key.is_empty()),
        }
    }

    // A server on this PC that speaks Claude's API, for tests.
    #[cfg(test)]
    fn claude_at(endpoint: impl Into<String>, key: Option<String>) -> Self {
        Self {
            claude: true,
            ..Self::new(endpoint, key)
        }
    }

    fn authorize<B>(&self, request: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        if self.claude {
            let request = request.header("anthropic-version", ANTHROPIC_VERSION);
            return match &self.key {
                Some(key) => request.header("x-api-key", key),
                None => request,
            };
        }
        match &self.key {
            Some(key) => request.header("Authorization", format!("Bearer {key}")),
            None => request,
        }
    }

    // `text` with the key blanked out, in case a server repeats it.
    fn redact(&self, text: String) -> String {
        match &self.key {
            Some(key) if text.contains(key.as_str()) => text.replace(key.as_str(), "[API key]"),
            _ => text,
        }
    }

    // An answer other than 200 OK, as a message for the panel.
    fn status_error(&self, status: u16, body: &str) -> String {
        let host = host(&self.endpoint);
        let message = match status {
            // The way to a new key (the provider card) is named by whoever
            // shows this.
            401 if self.key.is_some() => format!(
                "{host} didn't accept the API key. Check that it's complete and still active."
            ),
            401 => format!("{host} needs an API key."),
            429 => format!(
                "{host} is limiting requests, or the account's quota is used up. Wait a \
                 minute and try again, or check the plan and billing with the service."
            ),
            _ => server_error(status, body),
        };
        self.redact(message)
    }
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("endpoint", &self.endpoint)
            .field("key", &self.key.as_ref().map(|_| "<hidden>"))
            .finish()
    }
}

/// The host (and port, if given) of `endpoint`, e.g. "api.openai.com".
pub fn host(endpoint: &str) -> String {
    let rest = endpoint.trim();
    let rest = rest.split_once("://").map_or(rest, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    authority.to_ascii_lowercase()
}

/// Whether `endpoint` is on this PC, where no key is needed and nothing
/// leaves the machine.
pub fn is_local(endpoint: &str) -> bool {
    let host = host(endpoint);
    let name = if let Some(bracketed) = host.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or(bracketed)
    } else {
        host.rsplit_once(':')
            .map_or(host.as_str(), |(name, _)| name)
    };
    name == "localhost" || name == "::1" || name.starts_with("127.")
}

/// Where a key comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    /// Windows Credential Manager, set with AI: Set API Key.
    Saved,
    /// The OPENAI_API_KEY environment variable.
    Environment,
}

/// The environment variable read when no key is saved.
pub const KEY_VARIABLE: &str = "OPENAI_API_KEY";
/// The one read for Claude instead, as Anthropic's own tools do.
pub const ANTHROPIC_KEY_VARIABLE: &str = "ANTHROPIC_API_KEY";

/// The environment variable that holds a key for `endpoint`.
pub fn key_variable(endpoint: &str) -> &'static str {
    if is_anthropic(endpoint) {
        ANTHROPIC_KEY_VARIABLE
    } else {
        KEY_VARIABLE
    }
}

// Claude's own API, the Messages API, which isn't OpenAI's chat format: its
// own address, headers, request and streamed events.
const ANTHROPIC_HOST: &str = "api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2026-10-21";
// Lets a request Claude's safety checks decline be answered by another
// Claude model where one can.
const ANTHROPIC_FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// The Claude model a new connection starts with.
pub const CLAUDE_DEFAULT_MODEL: &str = "claude-opus-5-5";

/// Whether `endpoint` is Anthropic's Claude API.
pub fn is_anthropic(endpoint: &str) -> bool {
    host(endpoint) == ANTHROPIC_HOST
}

// The Credential Manager entry holding the key for `endpoint`'s host.
#[cfg(windows)]
fn key_target(endpoint: &str) -> String {
    format!("LightLine AI API key: {}", host(endpoint))
}

/// The key to send to `endpoint`: the one saved for its host, else
/// OPENAI_API_KEY (ANTHROPIC_API_KEY for Claude). None for a server on
/// this PC.
pub fn find_key(endpoint: &str) -> Option<(String, KeySource)> {
    if is_local(endpoint) {
        return None;
    }
    #[cfg(windows)]
    if let Some(key) = crate::credentials::read(&key_target(endpoint)) {
        return Some((key, KeySource::Saved));
    }
    std::env::var(key_variable(endpoint))
        .ok()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .map(|key| (key, KeySource::Environment))
}

/// The server at `endpoint`, with the key it should get.
pub fn server(endpoint: &str) -> Server {
    Server::new(endpoint, find_key(endpoint).map(|(key, _)| key))
}

/// Saves `key` for `endpoint`'s host.
#[cfg(windows)]
pub fn save_key(endpoint: &str, key: &str) -> std::io::Result<()> {
    crate::credentials::write(&key_target(endpoint), "LightLine", key.trim())
}

/// Removes the key saved for `endpoint`'s host. False when there was none.
#[cfg(windows)]
pub fn remove_key(endpoint: &str) -> std::io::Result<bool> {
    crate::credentials::delete(&key_target(endpoint))
}

// `endpoint` + "/v1/" + `path`. An endpoint given with its "/v1" (as LM
// Studio shows it) works too.
fn api_url(endpoint: &str, path: &str) -> String {
    let base = endpoint.trim().trim_end_matches('/');
    let base = base.strip_suffix("/v1").unwrap_or(base);
    format!("{base}/v1/{path}")
}

/// Starts the message for a server that couldn't be reached at all, so the
/// panel can add how to start one.
pub const NOTHING_ANSWERED: &str = "Nothing answered at";

fn unreachable(endpoint: &str, error: &ureq::Error) -> String {
    use ureq::Timeout;
    match error {
        // Refused connections are retried by Windows for a few seconds, so
        // a server that isn't running often shows up as a connect timeout.
        ureq::Error::ConnectionFailed
        | ureq::Error::HostNotFound
        | ureq::Error::Timeout(Timeout::Connect | Timeout::Resolve) => {
            format!("{NOTHING_ANSWERED} {endpoint}.")
        }
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::ConnectionRefused => {
            format!("{NOTHING_ANSWERED} {endpoint}.")
        }
        ureq::Error::Timeout(_) => format!("{endpoint} didn't answer in time."),
        _ => format!("Couldn't reach {endpoint} ({error})."),
    }
}

// The message in an OpenAI-style error: {"error": {"message": "..."}} or
// {"error": "..."}.
fn error_message(value: &Value) -> Option<String> {
    let error = value.get("error")?;
    error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .map(str::to_owned)
}

fn server_error(status: u16, body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .as_ref()
        .and_then(error_message)
        .unwrap_or_else(|| format!("The server answered with HTTP {status}"))
}

/// The models `server` offers, sorted by name.
pub fn list_models(server: &Server) -> Result<Vec<String>, String> {
    let endpoint = server.endpoint.as_str();
    // Claude's list comes 20 to a page unless asked for more.
    let path = if server.claude {
        "models?limit=1000"
    } else {
        "models"
    };
    let request = ureq::get(&api_url(endpoint, path))
        .config()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(LIST_TIMEOUT))
        .http_status_as_error(false)
        .build();
    let mut response = server
        .authorize(request)
        .call()
        .map_err(|error| server.redact(unreachable(endpoint, &error)))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .with_config()
        .limit(4 * 1024 * 1024)
        .read_to_string()
        .map_err(|error| server.redact(format!("Couldn't read the model list ({error})")))?;
    if status != 200 {
        return Err(server.status_error(status, &body));
    }
    parse_models(&body)
}

fn parse_models(body: &str) -> Result<Vec<String>, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| "The server's model list isn't valid JSON".to_string())?;
    let mut models: Vec<String> = value
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("id")?.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    models.sort();
    models.dedup();
    Ok(models)
}

// Parts of the names of models that can't chat: embedding models (which
// only turn text into numbers), and OpenAI's models for speech, images,
// moderation, plain completion and its other APIs.
const NOT_CHAT: [&str; 15] = [
    "embed",
    "whisper",
    "tts",
    "dall-e",
    "gpt-image",
    "moderation",
    "davinci",
    "babbage",
    "audio",
    "realtime",
    "transcribe",
    "search",
    "computer-use",
    "codex",
    "sora",
];

/// Models that can chat. Servers list the others too, but they can't
/// answer.
pub fn chat_models(models: &[String]) -> Vec<String> {
    models
        .iter()
        .filter(|model| {
            let model = model.to_ascii_lowercase();
            !NOT_CHAT.iter().any(|part| model.contains(part))
        })
        .cloned()
        .collect()
}

/// How the panel names `model`: Claude's ids read as their product names
/// ("claude-opus-5-5" is "Claude Opus 5.5", and a dated snapshot leaves its
/// date out); any other id is shown as it is.
pub fn model_label(model: &str) -> String {
    let Some(rest) = model.strip_prefix("claude-") else {
        return model.to_string();
    };
    let parts: Vec<&str> = rest.split('-').collect();
    let Some(family) = parts
        .first()
        .filter(|family| !family.is_empty() && family.chars().all(|ch| ch.is_ascii_alphabetic()))
    else {
        return model.to_string();
    };
    // The version: one or two short numbers; a long one is a date.
    let numbers: Vec<&str> = parts[1..]
        .iter()
        .take_while(|part| {
            !part.is_empty() && part.len() <= 2 && part.chars().all(|ch| ch.is_ascii_digit())
        })
        .copied()
        .collect();
    let dated = parts[1 + numbers.len()..]
        .iter()
        .all(|part| part.len() == 8 && part.chars().all(|ch| ch.is_ascii_digit()));
    if numbers.is_empty() || numbers.len() > 2 || !dated {
        return model.to_string();
    }
    let mut name = family.to_string();
    name[..1].make_ascii_uppercase();
    format!("Claude {name} {}", numbers.join("."))
}

/// Whether Ollama runs `model` on its own servers rather than on this PC:
/// its "cloud" models, such as "gpt-oss:120b-cloud" or "nemotron-3:cloud".
pub fn is_cloud_model(model: &str) -> bool {
    model
        .rsplit(':')
        .next()
        .is_some_and(|tag| tag == "cloud" || tag.ends_with("-cloud"))
}

/// A model to start with, never an Ollama cloud one: questions and code
/// shouldn't leave the PC unless the user picks such a model. A chat model
/// made for code is preferred; "base" models only complete text, so they
/// come last. From a cloud service (`remote`), a small, fast model ("mini")
/// is preferred, and a dated snapshot or a preview comes after the model
/// it's a version of.
pub fn pick_model(models: &[String], remote: bool) -> Option<String> {
    // From Claude's API, its current Opus.
    if let Some(claude) = models.iter().find(|model| *model == CLAUDE_DEFAULT_MODEL) {
        return Some(claude.clone());
    }
    let rank = |model: &str| {
        let model = model.to_ascii_lowercase();
        let chat = !model.contains("base");
        let mut score = 2 * u8::from(chat) + 2 * u8::from(chat && model.contains("code"));
        if remote {
            let dated = model
                .split('-')
                .any(|part| part.len() == 4 && part.starts_with("20"));
            let current = !dated && !model.contains("preview");
            score += 2 * u8::from(model.contains("mini")) + u8::from(current);
        }
        score
    };
    let mut best: Option<(u8, String)> = None;
    for model in chat_models(models) {
        if is_cloud_model(&model) {
            continue;
        }
        let score = rank(&model);
        if best.as_ref().is_none_or(|(top, _)| score > *top) {
            best = Some((score, model));
        }
    }
    best.map(|(_, model)| model)
}

fn openai_body(model: &str, messages: &[Message]) -> Value {
    let messages: Vec<Value> = messages
        .iter()
        .map(|message| json!({"role": message.role.as_str(), "content": message.content}))
        .collect();
    json!({"model": model, "messages": messages, "stream": true})
}

#[cfg(test)]
fn request_body(model: &str, messages: &[Message]) -> String {
    openai_body(model, messages).to_string()
}

/// What an answer is for: a chat answer may think a while; an inline
/// completion should come quickly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Purpose {
    Chat,
    Inline,
}

// Claude models of the current API generation: adaptive thinking, the
// `effort` setting and up to 128K tokens of output. Older ones take fewer
// tokens and refuse `effort`.
fn claude_current(model: &str) -> bool {
    const CURRENT: [&str; 9] = [
        "claude-fable",
        "claude-mythos",
        "claude-opus-5",
        "claude-sonnet-5",
        "claude-haiku-5",
        "claude-opus-4-6",
        "claude-opus-4-7",
        "claude-opus-4-8",
        "claude-sonnet-4-6",
    ];
    let model = model.to_ascii_lowercase();
    CURRENT.iter().any(|family| model.starts_with(family))
}

// Models whose declined answers another Claude model can take over.
fn claude_has_fallbacks(model: &str) -> bool {
    matches!(
        model,
        "claude-fable-5-2"
            | "claude-fable-5-1"
            | "claude-mythos-5-2"
            | "claude-mythos-5-1"
            | "claude-opus-5-5"
            | "claude-opus-5"
            | "claude-sonnet-5-5"
    )
}

/// What Claude's API takes for `messages`. Its system prompt is a field of
/// its own, turns alternate between user and assistant starting with the
/// user, and none may be empty: system messages are joined into `system`,
/// neighbouring turns of one role are joined, and empty ones (an answer
/// that was stopped before it began) and a leading assistant turn dropped.
#[cfg(test)]
fn claude_request_body(model: &str, messages: &[Message], purpose: Purpose) -> String {
    claude_body(model, messages, purpose).to_string()
}

fn claude_body(model: &str, messages: &[Message], purpose: Purpose) -> Value {
    let system: Vec<&str> = messages
        .iter()
        .filter(|message| message.role == Role::System && !message.content.trim().is_empty())
        .map(|message| message.content.as_str())
        .collect();
    let mut turns: Vec<(Role, String)> = Vec::new();
    for message in messages {
        if message.role == Role::System || message.content.trim().is_empty() {
            continue;
        }
        match turns.last_mut() {
            Some((role, text)) if *role == message.role => {
                text.push_str("\n\n");
                text.push_str(&message.content);
            }
            _ => turns.push((message.role, message.content.clone())),
        }
    }
    let first_user = turns
        .iter()
        .position(|(role, _)| *role == Role::User)
        .unwrap_or(turns.len());
    let turns: Vec<Value> = turns[first_user..]
        .iter()
        .map(|(role, text)| json!({"role": role.as_str(), "content": text}))
        .collect();
    let current = claude_current(model);
    let mut body = json!({
        "model": model,
        "max_tokens": if current { 64_000 } else { 4_096 },
        "stream": true,
        "messages": turns,
    });
    if !system.is_empty() {
        body["system"] = json!(system.join("\n\n"));
    }
    if current {
        let effort = match purpose {
            Purpose::Chat => "medium",
            Purpose::Inline => "low",
        };
        body["output_config"] = json!({"effort": effort});
    }
    if claude_has_fallbacks(model) {
        body["fallbacks"] = json!("default");
    }
    body
}

/// Shown when Claude's safety checks decline to answer.
pub const CLAUDE_DECLINED: &str = "Claude declined to answer this request.";

// One line of Claude's streamed answer: "data: {json}" whose "type" says
// what it is. Text arrives as content_block_delta / text_delta; thinking
// and the rest are skipped; message_stop ends it.
fn parse_claude_event(line: &str) -> Event {
    let Some(data) = line.strip_prefix("data:") else {
        return Event::Nothing;
    };
    let Ok(value) = serde_json::from_str::<Value>(data.trim()) else {
        return Event::Nothing;
    };
    match value.get("type").and_then(Value::as_str) {
        Some("content_block_delta") => {
            match (
                value.pointer("/delta/type").and_then(Value::as_str),
                value.pointer("/delta/text").and_then(Value::as_str),
            ) {
                (Some("text_delta"), Some(text)) if !text.is_empty() => {
                    Event::Text(text.to_owned())
                }
                _ => Event::Nothing,
            }
        }
        Some("message_delta")
            if value.pointer("/delta/stop_reason").and_then(Value::as_str) == Some("refusal") =>
        {
            Event::Error(CLAUDE_DECLINED.into())
        }
        Some("message_stop") => Event::Done,
        Some("error") => Event::Error(
            error_message(&value).unwrap_or_else(|| "Claude's API reported an error".into()),
        ),
        _ => Event::Nothing,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Text(String),
    Done,
    Error(String),
    Nothing,
}

// One line of the streamed answer (server-sent events): "data: {json}"
// carrying the next piece of text, until "data: [DONE]".
fn parse_event(line: &str) -> Event {
    let Some(data) = line.strip_prefix("data:") else {
        return Event::Nothing;
    };
    let data = data.trim();
    if data == "[DONE]" {
        return Event::Done;
    }
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Event::Nothing;
    };
    if let Some(message) = error_message(&value) {
        return Event::Error(message);
    }
    match value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
    {
        Some(text) if !text.is_empty() => Event::Text(text.to_owned()),
        _ => Event::Nothing,
    }
}

/// Asks `model` at `endpoint` to answer `messages`, handing each piece of
/// the answer to `on_text` as it arrives. While the answer is quiet,
/// `on_text` also gets an empty piece every `CANCEL_CHECK`, so a caller that
/// batches pieces can pass on what it holds. Returns early (Ok) once
/// `cancel` is set.
pub fn stream_chat(
    server: &Server,
    model: &str,
    messages: &[Message],
    cancel: &AtomicBool,
    on_text: impl FnMut(&str),
) -> Result<(), String> {
    stream_answer(server, model, messages, Purpose::Chat, cancel, on_text)
        .map_err(|error| server.redact(error))
}

fn stream_answer(
    server: &Server,
    model: &str,
    messages: &[Message],
    purpose: Purpose,
    cancel: &AtomicBool,
    on_text: impl FnMut(&str),
) -> Result<(), String> {
    let body = if server.claude {
        claude_body(model, messages, purpose)
    } else {
        openai_body(model, messages)
    };
    let mut reader = Reader::new(server.claude);
    stream_turn(server, model, body, cancel, &mut reader, on_text)
}

/// A tool call the model made.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Its arguments, or the text that wasn't valid JSON.
    pub input: Result<Value, String>,
}

/// What running a tool gave, for the model.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolReply {
    pub content: String,
    pub is_error: bool,
}

/// How a question with tools went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolsEnded {
    /// The server or model refused tools, so it was asked without them.
    pub tools_refused: bool,
    pub steps: usize,
}

// What one streamed answer held besides its text.
struct Turn {
    calls: Vec<ToolCall>,
    /// The assistant message to send back with the tools' results.
    assistant: Value,
    /// It stopped at its length limit, maybe in the middle of a call.
    cut_off: bool,
}

// Reads one streamed answer: its text as it comes, and the tool calls and
// (for Claude) every content block, thinking included, to send back.
enum Reader {
    OpenAi {
        text: String,
        // id, name, arguments as they arrive.
        calls: Vec<(String, String, String)>,
        finish: Option<String>,
    },
    Claude {
        blocks: Vec<Value>,
        partial: std::collections::HashMap<usize, String>,
        stop: Option<String>,
    },
}

impl Reader {
    fn new(claude: bool) -> Self {
        if claude {
            Reader::Claude {
                blocks: Vec::new(),
                partial: std::collections::HashMap::new(),
                stop: None,
            }
        } else {
            Reader::OpenAi {
                text: String::new(),
                calls: Vec::new(),
                finish: None,
            }
        }
    }

    fn line(&mut self, line: &str) -> Event {
        let data = line
            .strip_prefix("data:")
            .and_then(|data| serde_json::from_str::<Value>(data.trim()).ok());
        match self {
            Reader::OpenAi {
                text,
                calls,
                finish,
            } => {
                let event = parse_event(line);
                if let Event::Text(piece) = &event {
                    text.push_str(piece);
                }
                if let Some(value) = &data {
                    let deltas = value
                        .pointer("/choices/0/delta/tool_calls")
                        .and_then(Value::as_array);
                    for delta in deltas.into_iter().flatten() {
                        let index = delta
                            .get("index")
                            .and_then(Value::as_u64)
                            .map_or(calls.len().saturating_sub(1), |index| index as usize);
                        while calls.len() <= index {
                            calls.push(Default::default());
                        }
                        let call = &mut calls[index];
                        if let Some(id) = delta.get("id").and_then(Value::as_str) {
                            call.0 = id.to_owned();
                        }
                        if let Some(name) = delta.pointer("/function/name").and_then(Value::as_str)
                        {
                            call.1.push_str(name);
                        }
                        if let Some(arguments) =
                            delta.pointer("/function/arguments").and_then(Value::as_str)
                        {
                            call.2.push_str(arguments);
                        }
                    }
                    if let Some(reason) = value
                        .pointer("/choices/0/finish_reason")
                        .and_then(Value::as_str)
                    {
                        *finish = Some(reason.to_owned());
                    }
                }
                event
            }
            Reader::Claude {
                blocks,
                partial,
                stop,
            } => {
                let event = parse_claude_event(line);
                let Some(value) = &data else {
                    return event;
                };
                let index = value
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|index| index as usize);
                match (value.get("type").and_then(Value::as_str), index) {
                    (Some("content_block_start"), Some(index)) => {
                        while blocks.len() <= index {
                            blocks.push(Value::Null);
                        }
                        let block = value.get("content_block").cloned().unwrap_or(Value::Null);
                        if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                            partial.insert(index, String::new());
                        }
                        blocks[index] = block;
                    }
                    (Some("content_block_delta"), Some(index)) if index < blocks.len() => {
                        let block = &mut blocks[index];
                        let delta = value.get("delta").cloned().unwrap_or(Value::Null);
                        let append = |block: &mut Value, field: &str, piece: &str| {
                            let joined = format!(
                                "{}{piece}",
                                block.get(field).and_then(Value::as_str).unwrap_or("")
                            );
                            block[field] = Value::String(joined);
                        };
                        match delta.get("type").and_then(Value::as_str) {
                            Some("text_delta") => {
                                let piece = delta.get("text").and_then(Value::as_str);
                                append(block, "text", piece.unwrap_or(""));
                            }
                            Some("thinking_delta") => {
                                let piece = delta.get("thinking").and_then(Value::as_str);
                                append(block, "thinking", piece.unwrap_or(""));
                            }
                            Some("signature_delta") => {
                                if let Some(signature) = delta.get("signature") {
                                    block["signature"] = signature.clone();
                                }
                            }
                            Some("input_json_delta") => {
                                let piece = delta.get("partial_json").and_then(Value::as_str);
                                partial
                                    .entry(index)
                                    .or_default()
                                    .push_str(piece.unwrap_or(""));
                            }
                            _ => {}
                        }
                    }
                    (Some("message_delta"), _) => {
                        if let Some(reason) =
                            value.pointer("/delta/stop_reason").and_then(Value::as_str)
                        {
                            *stop = Some(reason.to_owned());
                        }
                    }
                    _ => {}
                }
                event
            }
        }
    }

    fn finish(self) -> Turn {
        match self {
            Reader::OpenAi {
                text,
                calls,
                finish,
            } => {
                let calls: Vec<ToolCall> = calls
                    .into_iter()
                    .enumerate()
                    .filter(|(_, (_, name, _))| !name.is_empty())
                    .map(|(index, (id, name, arguments))| ToolCall {
                        id: if id.is_empty() {
                            format!("call_{index}")
                        } else {
                            id
                        },
                        name,
                        input: parse_arguments(&arguments),
                    })
                    .collect();
                let tool_calls: Vec<Value> = calls
                    .iter()
                    .map(|call| {
                        let arguments = match &call.input {
                            Ok(value) => value.to_string(),
                            Err(raw) => raw.clone(),
                        };
                        json!({
                            "id": call.id,
                            "type": "function",
                            "function": {"name": call.name, "arguments": arguments},
                        })
                    })
                    .collect();
                let content = if text.is_empty() {
                    Value::Null
                } else {
                    Value::String(text)
                };
                Turn {
                    assistant: json!({
                        "role": "assistant",
                        "content": content,
                        "tool_calls": tool_calls,
                    }),
                    calls,
                    cut_off: finish.as_deref() == Some("length"),
                }
            }
            Reader::Claude {
                mut blocks,
                partial,
                stop,
            } => {
                let mut calls = Vec::new();
                for (index, block) in blocks.iter_mut().enumerate() {
                    if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                        continue;
                    }
                    let input =
                        parse_arguments(partial.get(&index).map(String::as_str).unwrap_or(""));
                    block["input"] = input.clone().unwrap_or_else(|_| json!({}));
                    calls.push(ToolCall {
                        id: block
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        name: block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        input,
                    });
                }
                blocks.retain(|block| !block.is_null());
                Turn {
                    assistant: json!({"role": "assistant", "content": blocks}),
                    calls,
                    cut_off: stop.as_deref() == Some("max_tokens"),
                }
            }
        }
    }
}

// A tool call's arguments: an object, `{}` when there were none, else the
// text as it came.
fn parse_arguments(raw: &str) -> Result<Value, String> {
    if raw.trim().is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(value) if value.is_object() => Ok(value),
        _ => Err(raw.to_owned()),
    }
}

// The tools as each API describes them. Claude's stream each argument as
// it's written (eager_input_streaming), so they're checked before use.
fn tools_json(claude: bool, tools: &[crate::ai_tools::ToolDef]) -> Value {
    let tools: Vec<Value> = tools
        .iter()
        .map(|tool| {
            if claude {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.schema,
                    "eager_input_streaming": true,
                })
            } else {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.schema,
                    },
                })
            }
        })
        .collect();
    Value::Array(tools)
}

/// Like `stream_chat`, offering `tools` the model may call. Each call goes
/// to `run_tool` and its reply back to the model, until it answers without
/// one, `ai_tools::MAX_STEPS` calls are made (then it must answer with what
/// it has), or `cancel` is set. A server or model that refuses tools is
/// asked again without them. Text from every round reaches `on_text`.
pub fn chat_with_tools(
    server: &Server,
    model: &str,
    messages: &[Message],
    tools: &[crate::ai_tools::ToolDef],
    cancel: &AtomicBool,
    mut on_text: impl FnMut(&str),
    mut run_tool: impl FnMut(&ToolCall) -> ToolReply,
) -> Result<ToolsEnded, String> {
    let mut ended = ToolsEnded::default();
    let mut rounds: Vec<Value> = Vec::new();
    let mut use_tools = !tools.is_empty();
    // A paragraph break goes between one round's words and the next's.
    let mut wrote = false;
    let mut needs_break = false;
    loop {
        let last_round = ended.steps >= crate::ai_tools::MAX_STEPS;
        let mut body = if server.claude {
            claude_body(model, messages, Purpose::Chat)
        } else {
            openai_body(model, messages)
        };
        if let Some(list) = body.get_mut("messages").and_then(Value::as_array_mut) {
            list.extend(rounds.iter().cloned());
        }
        if use_tools {
            body["tools"] = tools_json(server.claude, tools);
            if last_round {
                body["tool_choice"] = if server.claude {
                    json!({"type": "none"})
                } else {
                    json!("none")
                };
            }
        }
        let mut reader = Reader::new(server.claude);
        let streamed = stream_turn(server, model, body, cancel, &mut reader, |piece| {
            if !piece.is_empty() {
                if needs_break && wrote {
                    on_text("\n\n");
                }
                needs_break = false;
                wrote = true;
            }
            on_text(piece);
        });
        match streamed {
            Err(error)
                if use_tools
                    && rounds.is_empty()
                    && error.to_ascii_lowercase().contains("tool") =>
            {
                use_tools = false;
                ended.tools_refused = true;
                continue;
            }
            Err(error) => return Err(server.redact(error)),
            Ok(()) => {}
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(ended);
        }
        let turn = reader.finish();
        if turn.calls.is_empty() || !use_tools || last_round {
            return Ok(ended);
        }
        if turn.cut_off {
            return Err("The answer was cut off while it was asking to look at the project".into());
        }
        rounds.push(turn.assistant);
        let mut results = Vec::new();
        for call in &turn.calls {
            if cancel.load(Ordering::Relaxed) {
                return Ok(ended);
            }
            ended.steps += 1;
            let reply = match &call.input {
                Ok(_) => run_tool(call),
                Err(raw) => ToolReply {
                    content: json!({ "INVALID_JSON": raw }).to_string(),
                    is_error: true,
                },
            };
            results.push((call, reply));
        }
        if server.claude {
            let blocks: Vec<Value> = results
                .iter()
                .map(|(call, reply)| {
                    let mut block = json!({
                        "type": "tool_result",
                        "tool_use_id": call.id,
                        "content": reply.content,
                    });
                    if reply.is_error {
                        block["is_error"] = json!(true);
                    }
                    block
                })
                .collect();
            rounds.push(json!({"role": "user", "content": blocks}));
        } else {
            rounds.extend(results.iter().map(|(call, reply)| {
                json!({"role": "tool", "tool_call_id": call.id, "content": reply.content})
            }));
        }
        needs_break = true;
    }
}

// Sends one request whose answer streams back, giving each piece of text to
// `on_text` and every line to `reader`.
fn stream_turn(
    server: &Server,
    model: &str,
    body: Value,
    cancel: &AtomicBool,
    reader: &mut Reader,
    mut on_text: impl FnMut(&str),
) -> Result<(), String> {
    let endpoint = server.endpoint.as_str();
    let claude = server.claude;
    let path = if claude {
        "messages"
    } else {
        "chat/completions"
    };
    let body = body.to_string();
    let mut request = ureq::post(&api_url(endpoint, path))
        .config()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(FIRST_TOKEN_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .header("Content-Type", "application/json");
    if claude && claude_has_fallbacks(model) {
        request = request.header("anthropic-beta", ANTHROPIC_FALLBACK_BETA);
    }
    let response = server
        .authorize(request)
        .send(body)
        .map_err(|error| unreachable(endpoint, &error))?;
    let status = response.status().as_u16();
    let mut body = response.into_body();
    if status != 200 {
        let text = body
            .with_config()
            .limit(64 * 1024)
            .read_to_string()
            .unwrap_or_default();
        return Err(server.status_error(status, &text));
    }
    // Read on a thread of its own, so that a server that goes quiet can
    // still be stopped. Once nobody is listening, the reader stops at the
    // next line and drops the connection, which ends the answer server-side.
    let (lines_tx, lines) = std::sync::mpsc::sync_channel(64);
    std::thread::spawn(move || {
        for line in BufReader::new(body.into_reader()).lines() {
            let failed = line.is_err();
            if lines_tx.send(line).is_err() || failed {
                break;
            }
        }
    });
    let mut received = 0;
    let mut quiet_limit = FIRST_TOKEN_TIMEOUT;
    let mut last_line = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let line = match lines.recv_timeout(CANCEL_CHECK) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) if last_line.elapsed() >= quiet_limit => {
                return Err("The server stopped answering".into());
            }
            Err(RecvTimeoutError::Timeout) => {
                on_text("");
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        last_line = Instant::now();
        let line = line.map_err(|error| format!("The answer was cut off ({error})"))?;
        match reader.line(&line) {
            Event::Text(text) => {
                quiet_limit = STALL_TIMEOUT;
                received += text.len();
                on_text(&text);
                if received > MAX_ANSWER_BYTES {
                    return Err("The answer was too long and was cut off".into());
                }
            }
            Event::Done => break,
            Event::Error(message) => return Err(message),
            Event::Nothing => {}
        }
    }
    Ok(())
}

/// Inline code completion: predicts the code snippet that should be inserted at the cursor.
pub fn complete_inline(
    server: &Server,
    model: &str,
    prefix: &str,
    suffix: &str,
    language: &str,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let prompt = format!(
        "You are an inline code completion model for {language}.\n\
         Given the code before and after the cursor, output ONLY the code that should be inserted directly at the cursor.\n\
         Do not include explanations, greetings, or markdown code fences.\n\n\
         [BEFORE CURSOR]\n{prefix}\n[AFTER CURSOR]\n{suffix}\n[COMPLETION]"
    );
    let messages = [
        Message::new(
            Role::System,
            "You are an expert code completion engine. Output only the completion code, nothing else.",
        ),
        Message::new(Role::User, prompt),
    ];
    let mut output = String::new();
    stream_answer(server, model, &messages, Purpose::Inline, cancel, |chunk| {
        output.push_str(chunk);
    })
    .map_err(|error| server.redact(error))?;
    Ok(clean_inline_completion(&output))
}

/// Strip any accidental markdown fences or thinking tags from inline completion.
pub fn clean_inline_completion(raw: &str) -> String {
    let (visible, _) = visible_answer(raw);
    let trimmed = visible.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        let after_lang = rest.split_once('\n').map_or(rest, |(_, code)| code);
        let code = after_lang.strip_suffix("```").unwrap_or(after_lang);
        code.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// The part of an answer to show. Reasoning models first "think out loud"
/// inside <think>...</think>; that's hidden, and `true` is returned while
/// the model is still thinking.
pub fn visible_answer(text: &str) -> (&str, bool) {
    let trimmed = text.trim_start();
    let Some(rest) = trimmed.strip_prefix("<think>") else {
        return (text, false);
    };
    match rest.find("</think>") {
        Some(end) => (rest[end + "</think>".len()..].trim_start(), false),
        None => ("", true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn claude_models_are_named_as_products() {
        assert_eq!(model_label("claude-opus-5-5"), "Claude Opus 5.5");
        assert_eq!(model_label("claude-haiku-5-5"), "Claude Haiku 5.5");
        assert_eq!(
            model_label("claude-sonnet-4-5-20250929"),
            "Claude Sonnet 4.5"
        );
        assert_eq!(model_label("claude-opus-4-20250514"), "Claude Opus 4");
        // Anything else stays as its id.
        assert_eq!(model_label("qwen2.5-coder:7b"), "qwen2.5-coder:7b");
        assert_eq!(
            model_label("claude-3-5-sonnet-latest"),
            "claude-3-5-sonnet-latest"
        );
        assert_eq!(
            model_label("claude-opus-5-5-preview"),
            "claude-opus-5-5-preview"
        );
        assert_eq!(model_label("claude-"), "claude-");
    }

    #[test]
    fn urls_accept_the_endpoint_with_or_without_v1() {
        assert_eq!(
            api_url("http://localhost:11434", "models"),
            "http://localhost:11434/v1/models"
        );
        assert_eq!(
            api_url("http://localhost:1234/v1/", "chat/completions"),
            "http://localhost:1234/v1/chat/completions"
        );
    }

    #[test]
    fn models_are_listed_and_a_coding_model_is_preferred() {
        let body = r#"{"object":"list","data":[
            {"id":"llama3.2:latest","object":"model"},
            {"id":"nomic-embed-text:latest","object":"model"},
            {"id":"qwen2.5-coder:7b","object":"model"}]}"#;
        let models = parse_models(body).unwrap();
        assert_eq!(models.len(), 3);
        assert_eq!(
            chat_models(&models),
            vec!["llama3.2:latest".to_string(), "qwen2.5-coder:7b".into()]
        );
        assert_eq!(
            pick_model(&models, false).as_deref(),
            Some("qwen2.5-coder:7b")
        );
        assert_eq!(
            pick_model(&["nomic-embed-text".to_string()], false),
            None,
            "an embedding model can't chat"
        );
        assert!(parse_models("not json").is_err());
    }

    #[test]
    fn cloud_models_are_never_picked_and_base_models_come_last() {
        // Ollama's own names, as listed on a real install.
        let installed: Vec<String> = [
            "gemma4:31b-cloud",
            "gpt-oss:120b-cloud",
            "llama3.2:1b",
            "nemotron-3-super:cloud",
            "nomic-embed-text:latest",
            "qwen2.5-coder:1.5b-base",
            "qwen2.5-coder:7b",
            "qwen2.5:0.5b",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            pick_model(&installed, false).as_deref(),
            Some("qwen2.5-coder:7b")
        );
        assert!(is_cloud_model("gpt-oss:120b-cloud"));
        assert!(is_cloud_model("nemotron-3-super:cloud"));
        assert!(!is_cloud_model("qwen2.5-coder:7b"));
        assert!(!is_cloud_model("llama3"));
        assert_eq!(
            pick_model(&["gpt-oss:20b-cloud".to_string()], false),
            None,
            "a cloud model is only used when chosen"
        );
        assert_eq!(
            pick_model(
                &["qwen2.5-coder:1.5b-base".to_string(), "llama3.2:1b".into()],
                false
            )
            .as_deref(),
            Some("llama3.2:1b")
        );
    }

    #[test]
    fn stream_lines_become_text_done_or_errors() {
        assert_eq!(
            parse_event(r#"data: {"choices":[{"delta":{"content":"Hi"}}]}"#),
            Event::Text("Hi".into())
        );
        assert_eq!(parse_event("data: [DONE]"), Event::Done);
        assert_eq!(parse_event(""), Event::Nothing);
        assert_eq!(parse_event(": keep-alive"), Event::Nothing);
        assert_eq!(
            parse_event(r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#),
            Event::Nothing
        );
        assert_eq!(
            parse_event(r#"data: {"error":{"message":"model not found"}}"#),
            Event::Error("model not found".into())
        );
    }

    #[test]
    fn server_errors_use_the_servers_own_message() {
        assert_eq!(
            server_error(
                404,
                r#"{"error":{"message":"model \"x\" not found, try pulling it first"}}"#
            ),
            "model \"x\" not found, try pulling it first"
        );
        assert_eq!(
            server_error(500, "oops"),
            "The server answered with HTTP 500"
        );
    }

    #[test]
    fn the_request_carries_the_model_messages_and_streaming() {
        let body = request_body(
            "m",
            &[
                Message::new(Role::System, "be brief"),
                Message::new(Role::User, "hi \"there\""),
            ],
        );
        let value: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["model"], "m");
        assert_eq!(value["stream"], true);
        assert_eq!(value["messages"][0]["role"], "system");
        assert_eq!(value["messages"][1]["content"], "hi \"there\"");
    }

    #[test]
    fn thinking_is_hidden_until_the_answer_starts() {
        assert_eq!(visible_answer("Hello"), ("Hello", false));
        assert_eq!(visible_answer("<think>hmm"), ("", true));
        assert_eq!(
            visible_answer("<think>hmm</think>\n\nHello"),
            ("Hello", false)
        );
    }

    // Serves one canned HTTP response on a local port. The whole request is
    // read first: closing a socket with unread data resets the connection
    // on Windows, which the client would see as a failure.
    fn serve_once(response: &'static str) -> String {
        serve_and_record(response).0
    }

    // serve_once, also handing over the request the server got.
    fn serve_and_record(response: &'static str) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let (got, request_text) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let Ok(read) = stream.read(&mut chunk) else {
                    return;
                };
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())?
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let _ = got.send(String::from_utf8_lossy(&request).into_owned());
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });
        (address, request_text)
    }

    const MODEL_LIST: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
        Connection: close\r\nContent-Length: 21\r\n\r\n{\"data\":[{\"id\":\"m\"}]}";

    #[test]
    fn the_key_is_sent_only_when_one_is_set() {
        let (endpoint, request) = serve_and_record(MODEL_LIST);
        let server = Server::new(endpoint, Some("sk-test-123".into()));
        assert_eq!(list_models(&server).unwrap(), ["m"]);
        let request = request.recv().unwrap().to_ascii_lowercase();
        assert!(
            request.contains("authorization: bearer sk-test-123"),
            "{request}"
        );

        let (endpoint, request) = serve_and_record(MODEL_LIST);
        assert_eq!(list_models(&Server::new(endpoint, None)).unwrap(), ["m"]);
        assert!(
            !request
                .recv()
                .unwrap()
                .to_ascii_lowercase()
                .contains("authorization")
        );

        let (endpoint, request) = serve_and_record(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
             data: [DONE]\n\n",
        );
        let cancel = AtomicBool::new(false);
        let server = Server::new(endpoint, Some("sk-test-123".into()));
        stream_chat(&server, "m", &[], &cancel, |_| {}).unwrap();
        assert!(
            request
                .recv()
                .unwrap()
                .to_ascii_lowercase()
                .contains("authorization: bearer sk-test-123")
        );
        assert!(!format!("{server:?}").contains("sk-test-123"));
    }

    #[test]
    fn claude_is_recognized_by_its_host_and_reads_its_own_key_variable() {
        assert!(is_anthropic("https://api.anthropic.com"));
        assert!(is_anthropic("https://API.anthropic.com/v1/"));
        assert!(!is_anthropic("https://api.openai.com"));
        assert!(!is_anthropic("https://openrouter.ai/api"));
        assert_eq!(
            key_variable("https://api.anthropic.com"),
            "ANTHROPIC_API_KEY"
        );
        assert_eq!(key_variable("https://api.openai.com"), "OPENAI_API_KEY");
    }

    #[test]
    fn claude_gets_its_own_request() {
        let messages = [
            Message::new(Role::System, "Be brief."),
            // An earlier answer that was stopped before it began.
            Message::new(Role::Assistant, ""),
            Message::new(Role::User, "first"),
            Message::new(Role::User, "and more"),
            Message::new(Role::Assistant, "an answer"),
            Message::new(Role::User, "next"),
        ];
        let body: Value = serde_json::from_str(&claude_request_body(
            "claude-opus-5-5",
            &messages,
            Purpose::Chat,
        ))
        .unwrap();
        assert_eq!(body["model"], "claude-opus-5-5");
        assert_eq!(body["system"], "Be brief.");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_tokens"], 64_000);
        assert_eq!(body["output_config"]["effort"], "medium");
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(
            body["messages"],
            json!([
                {"role": "user", "content": "first\n\nand more"},
                {"role": "assistant", "content": "an answer"},
                {"role": "user", "content": "next"},
            ])
        );
        // Sampling settings are refused by the API version LightLine sends.
        assert!(body.get("temperature").is_none());

        let inline: Value = serde_json::from_str(&claude_request_body(
            "claude-sonnet-5-5",
            &messages,
            Purpose::Inline,
        ))
        .unwrap();
        assert_eq!(inline["output_config"]["effort"], "low");

        // Older models take less output and no effort setting.
        let old: Value = serde_json::from_str(&claude_request_body(
            "claude-haiku-4-5",
            &messages,
            Purpose::Chat,
        ))
        .unwrap();
        assert_eq!(old["max_tokens"], 4_096);
        assert!(old.get("output_config").is_none());
        assert!(old.get("fallbacks").is_none());
    }

    #[test]
    fn claudes_stream_becomes_text_done_or_errors() {
        let text = r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Hi"}}"#;
        assert_eq!(parse_claude_event(text), Event::Text("Hi".into()));
        let thinking = r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":""}}"#;
        assert_eq!(parse_claude_event(thinking), Event::Nothing);
        assert_eq!(parse_claude_event("event: ping"), Event::Nothing);
        assert_eq!(
            parse_claude_event(r#"data: {"type":"message_stop"}"#),
            Event::Done
        );
        let refused = r#"data: {"type":"message_delta","delta":{"stop_reason":"refusal"}}"#;
        assert_eq!(
            parse_claude_event(refused),
            Event::Error(CLAUDE_DECLINED.into())
        );
        let finished = r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#;
        assert_eq!(parse_claude_event(finished), Event::Nothing);
        let overloaded =
            r#"data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        assert_eq!(
            parse_claude_event(overloaded),
            Event::Error("Overloaded".into())
        );
    }

    #[test]
    fn claude_answers_stream_with_its_headers() {
        let (endpoint, request) = serve_and_record(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
             event: message_start\ndata: {\"type\":\"message_start\",\"message\":{}}\n\n\
             event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
             event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" there\"}}\n\n\
             event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        let server = Server::claude_at(endpoint, Some("sk-ant-test".into()));
        let cancel = AtomicBool::new(false);
        let mut answer = String::new();
        let messages = [Message::new(Role::User, "hi")];
        stream_chat(&server, "claude-opus-5-5", &messages, &cancel, |piece| {
            answer.push_str(piece)
        })
        .unwrap();
        assert_eq!(answer, "Hello there");
        let request = request.recv().unwrap();
        let lower = request.to_ascii_lowercase();
        assert!(lower.starts_with("post /v1/messages "), "{request}");
        assert!(lower.contains("x-api-key: sk-ant-test"), "{request}");
        assert!(lower.contains("anthropic-version: 2026-10-21"), "{request}");
        assert!(
            lower.contains("anthropic-beta: server-side-fallback-2026-07-01"),
            "{request}"
        );
        assert!(!lower.contains("authorization"), "{request}");
    }

    #[test]
    fn claudes_errors_and_model_list_are_read() {
        let (endpoint, request) = serve_and_record(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\
             Content-Length: 109\r\n\r\n{\"data\":[{\"type\":\"model\",\"id\":\"claude-sonnet-5-5\"},\
             {\"type\":\"model\",\"id\":\"claude-opus-5-5\"}],\"has_more\":false}",
        );
        let server = Server::claude_at(endpoint, Some("sk-ant-test".into()));
        let models = list_models(&server).unwrap();
        assert_eq!(models, ["claude-opus-5-5", "claude-sonnet-5-5"]);
        assert!(
            request
                .recv()
                .unwrap()
                .starts_with("GET /v1/models?limit=1000 ")
        );
        // Claude's Opus is where a new connection starts.
        assert_eq!(
            pick_model(&models, true).as_deref(),
            Some(CLAUDE_DEFAULT_MODEL)
        );

        let endpoint = serve_once(
            "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nConnection: close\r\n\
             Content-Length: 86\r\n\r\n{\"type\":\"error\",\"error\":{\"type\":\"authentication_error\",\"message\":\"invalid x-api-key\"}}",
        );
        let server = Server::claude_at(endpoint, Some("sk-ant-test".into()));
        let error = list_models(&server).unwrap_err();
        assert!(error.contains("didn't accept the API key"), "{error}");

        let endpoint = serve_once(
            "HTTP/1.1 529 Overloaded\r\nContent-Type: application/json\r\nConnection: close\r\n\
             Content-Length: 75\r\n\r\n{\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}",
        );
        let cancel = AtomicBool::new(false);
        let server = Server::claude_at(endpoint, Some("sk-ant-test".into()));
        let error = stream_chat(&server, "claude-opus-5-5", &[], &cancel, |_| {}).unwrap_err();
        assert_eq!(error, "Overloaded");
    }

    // A server that answers each connection with the next of `responses`,
    // handing over each request's body.
    fn serve_sequence(responses: Vec<String>) -> (String, std::sync::mpsc::Receiver<Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let (got, bodies) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for response in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut request = Vec::new();
                let mut chunk = [0u8; 8192];
                let body_start = loop {
                    let Ok(read) = stream.read(&mut chunk) else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..read]);
                    let text = String::from_utf8_lossy(&request);
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())?
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break end + 4;
                        }
                    }
                };
                let body = serde_json::from_slice(&request[body_start..]).unwrap_or(Value::Null);
                let _ = got.send(body);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (address, bodies)
    }

    fn sse(events: &[Value]) -> String {
        let mut out = String::from(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        );
        for event in events {
            out.push_str(&format!("data: {event}\n\n"));
        }
        out
    }

    fn openai_tool_call(id: &str, arguments: [&str; 2]) -> String {
        let mut text = sse(&[
            json!({"choices": [{"delta": {"content": "Let me look."}}]}),
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": id, "type": "function",
                "function": {"name": "read_file", "arguments": arguments[0]}}]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0,
                "function": {"arguments": arguments[1]}}]}, "finish_reason": "tool_calls"}]}),
        ]);
        text.push_str("data: [DONE]\n\n");
        text
    }

    fn openai_text(text: &str) -> String {
        let mut out = sse(&[json!({"choices": [{"delta": {"content": text}}]})]);
        out.push_str("data: [DONE]\n\n");
        out
    }

    fn tools() -> Vec<crate::ai_tools::ToolDef> {
        crate::ai_tools::definitions()
    }

    #[test]
    fn tool_calls_go_round_in_openais_format() {
        let (endpoint, bodies) = serve_sequence(vec![
            openai_tool_call("call_1", ["{\"path\":", "\"src/a.rs\"}"]),
            openai_text("It's in a.rs."),
        ]);
        let server = Server::new(endpoint, None);
        let cancel = AtomicBool::new(false);
        let mut answer = String::new();
        let mut asked = Vec::new();
        let ended = chat_with_tools(
            &server,
            "m",
            &[Message::new(Role::User, "where?")],
            &tools(),
            &cancel,
            |piece| answer.push_str(piece),
            |call| {
                asked.push(call.clone());
                ToolReply {
                    content: "1| fn a() {}".into(),
                    is_error: false,
                }
            },
        )
        .unwrap();
        assert_eq!(answer, "Let me look.\n\nIt's in a.rs.");
        assert_eq!(ended.steps, 1);
        assert_eq!(asked[0].name, "read_file");
        assert_eq!(asked[0].input, Ok(json!({"path": "src/a.rs"})));
        let first = bodies.recv().unwrap();
        assert_eq!(first["tools"][2]["function"]["name"], "read_file");
        let second = bodies.recv().unwrap();
        let messages = second["messages"].as_array().unwrap();
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(
            messages[1]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"src/a.rs\"}"
        );
        assert_eq!(
            messages[2],
            json!({"role": "tool", "tool_call_id": "call_1", "content": "1| fn a() {}"})
        );
    }

    #[test]
    fn claudes_tool_use_sends_back_every_block_thinking_included() {
        let first = sse(&[
            json!({"type": "message_start", "message": {}}),
            json!({"type": "content_block_start", "index": 0,
                "content_block": {"type": "thinking", "thinking": ""}}),
            json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "thinking_delta", "thinking": "Find it."}}),
            json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "signature_delta", "signature": "sig-1"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "content_block_start", "index": 1,
                "content_block": {"type": "tool_use", "id": "toolu_1", "name": "search", "input": {}}}),
            json!({"type": "content_block_delta", "index": 1,
                "delta": {"type": "input_json_delta", "partial_json": "{\"query\": \"str"}}),
            json!({"type": "content_block_delta", "index": 1,
                "delta": {"type": "input_json_delta", "partial_json": "eam_chat\"}"}}),
            json!({"type": "content_block_stop", "index": 1}),
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
            json!({"type": "message_stop"}),
        ]);
        let second = sse(&[
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Found it."}}),
            json!({"type": "message_stop"}),
        ]);
        let (endpoint, bodies) = serve_sequence(vec![first, second]);
        let server = Server::claude_at(endpoint, Some("sk-ant-test".into()));
        let cancel = AtomicBool::new(false);
        let mut answer = String::new();
        chat_with_tools(
            &server,
            "claude-opus-5-5",
            &[
                Message::new(Role::System, "Be brief."),
                Message::new(Role::User, "where?"),
            ],
            &tools(),
            &cancel,
            |piece| answer.push_str(piece),
            |call| {
                assert_eq!(call.input, Ok(json!({"query": "stream_chat"})));
                ToolReply {
                    content: "src/a.rs:1: fn stream_chat()".into(),
                    is_error: false,
                }
            },
        )
        .unwrap();
        assert_eq!(answer, "Found it.");
        let first = bodies.recv().unwrap();
        assert_eq!(first["tools"][0]["name"], "search");
        assert_eq!(first["tools"][0]["eager_input_streaming"], true);
        assert!(first.get("tool_choice").is_none());
        let second = bodies.recv().unwrap();
        let messages = second["messages"].as_array().unwrap();
        assert_eq!(
            messages[1],
            json!({"role": "assistant", "content": [
                {"type": "thinking", "thinking": "Find it.", "signature": "sig-1"},
                {"type": "tool_use", "id": "toolu_1", "name": "search", "input": {"query": "stream_chat"}},
            ]})
        );
        assert_eq!(
            messages[2],
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": "src/a.rs:1: fn stream_chat()"},
            ]})
        );
        assert_eq!(second["system"], "Be brief.");
    }

    #[test]
    fn a_model_that_refuses_tools_is_asked_again_without_them() {
        let refused = "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\n\
            Connection: close\r\nContent-Length: 75\r\n\r\n\
            {\"error\":{\"message\":\"registry.ollama.ai/library/x does not support tools\"}}"
            .to_string();
        let (endpoint, bodies) = serve_sequence(vec![refused, openai_text("Plain answer.")]);
        let server = Server::new(endpoint, None);
        let cancel = AtomicBool::new(false);
        let mut answer = String::new();
        let ended = chat_with_tools(
            &server,
            "x",
            &[Message::new(Role::User, "hi")],
            &tools(),
            &cancel,
            |piece| answer.push_str(piece),
            |_| unreachable!("no tools without tools"),
        )
        .unwrap();
        assert!(ended.tools_refused);
        assert_eq!(answer, "Plain answer.");
        assert!(bodies.recv().unwrap().get("tools").is_some());
        assert!(bodies.recv().unwrap().get("tools").is_none());
    }

    #[test]
    fn broken_tool_input_isnt_run_and_the_model_is_told() {
        let (endpoint, bodies) = serve_sequence(vec![
            openai_tool_call("call_9", ["{\"path\": \"a", ".rs"]),
            openai_text("Sorry."),
        ]);
        let server = Server::new(endpoint, None);
        let cancel = AtomicBool::new(false);
        chat_with_tools(
            &server,
            "m",
            &[Message::new(Role::User, "hi")],
            &tools(),
            &cancel,
            |_| {},
            |_| unreachable!("a call with broken input never runs"),
        )
        .unwrap();
        let _ = bodies.recv();
        let second = bodies.recv().unwrap();
        let reply = &second["messages"][2]["content"];
        let reply: Value = serde_json::from_str(reply.as_str().unwrap()).unwrap();
        assert_eq!(reply["INVALID_JSON"], "{\"path\": \"a.rs");
    }

    #[test]
    fn after_25_steps_the_model_must_answer() {
        let mut responses: Vec<String> = (0..crate::ai_tools::MAX_STEPS)
            .map(|n| openai_tool_call(&format!("call_{n}"), ["{\"path\":", "\"a.rs\"}"]))
            .collect();
        responses.push(openai_text("Done looking."));
        let (endpoint, bodies) = serve_sequence(responses);
        let server = Server::new(endpoint, None);
        let cancel = AtomicBool::new(false);
        let mut runs = 0;
        let ended = chat_with_tools(
            &server,
            "m",
            &[Message::new(Role::User, "hi")],
            &tools(),
            &cancel,
            |_| {},
            |_| {
                runs += 1;
                ToolReply {
                    content: "ok".into(),
                    is_error: false,
                }
            },
        )
        .unwrap();
        assert_eq!(runs, crate::ai_tools::MAX_STEPS);
        assert_eq!(ended.steps, crate::ai_tools::MAX_STEPS);
        let all: Vec<Value> = bodies.try_iter().collect();
        assert_eq!(all.len(), crate::ai_tools::MAX_STEPS + 1);
        assert!(all[0].get("tool_choice").is_none());
        assert_eq!(all.last().unwrap()["tool_choice"], "none");
    }

    #[test]
    fn the_key_never_shows_in_errors() {
        // OpenAI repeats part of a wrong key; some servers echo all of it.
        let endpoint = serve_once(
            "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nConnection: close\r\n\
             Content-Length: 61\r\n\r\n{\"error\":{\"message\":\"Incorrect API key provided: sk-abc123\"}}",
        );
        let server = Server::new(endpoint.clone(), Some("sk-abc123".into()));
        let error = list_models(&server).unwrap_err();
        assert!(!error.contains("sk-abc"), "{error}");
        assert!(error.contains("didn't accept the API key"), "{error}");

        let endpoint = serve_once(
            "HTTP/1.1 500 Oops\r\nContent-Type: application/json\r\nConnection: close\r\n\
             Content-Length: 41\r\n\r\n{\"error\":{\"message\":\"bad key sk-abc123\"}}",
        );
        let cancel = AtomicBool::new(false);
        let server = Server::new(endpoint, Some("sk-abc123".into()));
        let error = stream_chat(&server, "m", &[], &cancel, |_| {}).unwrap_err();
        assert_eq!(error, "bad key [API key]");
    }

    #[test]
    fn rejected_keys_and_rate_limits_get_plain_messages() {
        let server = Server::new("https://api.openai.com/v1", None);
        assert!(
            server
                .status_error(401, "")
                .starts_with("api.openai.com needs an API key")
        );
        let server = Server::new("https://api.openai.com/v1", Some("k".into()));
        assert!(
            server
                .status_error(
                    429,
                    r#"{"error":{"message":"You exceeded your current quota"}}"#
                )
                .starts_with("api.openai.com is limiting requests, or the account's quota")
        );
    }

    #[test]
    fn hosts_and_local_servers_are_recognized() {
        assert_eq!(host("https://api.openai.com/v1"), "api.openai.com");
        assert_eq!(host("http://localhost:11434"), "localhost:11434");
        assert_eq!(host("https://user@openrouter.ai/api"), "openrouter.ai");
        assert!(is_local("http://localhost:11434"));
        assert!(is_local("http://127.0.0.1:1234/v1"));
        assert!(is_local("http://[::1]:8080"));
        assert!(!is_local("https://api.openai.com"));
        assert!(!is_local("https://localhost.example.com"));
        assert_eq!(
            find_key("http://localhost:11434"),
            None,
            "no key for this PC"
        );
    }

    #[test]
    fn only_chat_models_are_listed_and_a_small_one_is_picked_from_a_service() {
        // Part of what api.openai.com/v1/models lists.
        let listed: Vec<String> = [
            "babbage-002",
            "chatgpt-4o-latest",
            "codex-mini-latest",
            "dall-e-3",
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-4.1-mini-2025-04-14",
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4o-mini-search-preview",
            "gpt-4o-mini-transcribe",
            "gpt-4o-mini-tts",
            "gpt-4o-realtime-preview",
            "gpt-image-1",
            "o4-mini",
            "omni-moderation-latest",
            "text-embedding-3-small",
            "tts-1",
            "whisper-1",
        ]
        .map(String::from)
        .to_vec();
        let chat = chat_models(&listed);
        assert_eq!(
            chat,
            [
                "chatgpt-4o-latest",
                "gpt-4.1",
                "gpt-4.1-mini",
                "gpt-4.1-mini-2025-04-14",
                "gpt-4o",
                "gpt-4o-mini",
                "o4-mini"
            ]
        );
        assert_eq!(pick_model(&chat, true).as_deref(), Some("gpt-4.1-mini"));
    }

    #[test]
    fn an_answer_streams_in_pieces() {
        let endpoint = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
             data: [DONE]\n\n",
        );
        let mut pieces = Vec::new();
        let cancel = AtomicBool::new(false);
        stream_chat(
            &Server::new(endpoint, None),
            "m",
            &[Message::new(Role::User, "hi")],
            &cancel,
            |text| {
                if !text.is_empty() {
                    pieces.push(text.to_string());
                }
            },
        )
        .unwrap();
        assert_eq!(pieces, ["Hel", "lo"]);
    }

    #[test]
    fn a_server_that_goes_quiet_can_still_be_stopped() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
                  data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            );
            let _ = stream.flush();
            // Keep the connection open without sending anything more.
            std::thread::sleep(std::time::Duration::from_secs(10));
        });
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let stopper = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            stopper.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let mut pieces = Vec::new();
        stream_chat(
            &Server::new(endpoint, None),
            "m",
            &[Message::new(Role::User, "hi")],
            &cancel,
            |text| pieces.push(text.to_string()),
        )
        .unwrap();
        // The piece, then empty ones while the answer is quiet: what lets a
        // caller pass on held text without waiting for the next piece.
        assert_eq!(pieces[0], "Hel");
        assert!(pieces.len() > 1 && pieces[1..].iter().all(String::is_empty));
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
    }

    #[test]
    fn a_missing_model_reports_the_servers_message() {
        let endpoint = serve_once(
            "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nConnection: close\r\n\
             Content-Length: 43\r\n\r\n{\"error\":{\"message\":\"model 'm' not found\"}}",
        );
        let cancel = AtomicBool::new(false);
        let error =
            stream_chat(&Server::new(endpoint, None), "m", &[], &cancel, |_| {}).unwrap_err();
        assert_eq!(error, "model 'm' not found");
    }

    #[test]
    fn an_unreachable_server_says_so() {
        // Bind and drop a listener to find a port nothing listens on.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let endpoint = format!("http://127.0.0.1:{port}");
        let error = list_models(&Server::new(endpoint.clone(), None)).unwrap_err();
        assert_eq!(error, format!("{NOTHING_ANSWERED} {endpoint}."));
    }

    #[test]
    fn complete_inline_extracts_clean_code() {
        let endpoint = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"```rust\\nlet y = 42;\\n```\"}}]}\n\n\
             data: [DONE]\n\n",
        );
        let cancel = AtomicBool::new(false);
        let server = Server::new(endpoint, None);
        let completion =
            complete_inline(&server, "m", "let x = 10;\n", "", "rust", &cancel).unwrap();
        assert_eq!(completion, "let y = 42;");
    }
}
