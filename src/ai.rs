//! Chatting with a language model through the OpenAI-compatible HTTP API
//! that Ollama, LM Studio and llama.cpp's server all offer. Ollama on this
//! PC is the default, so code never leaves the machine unless the user
//! points `aiEndpoint` somewhere else.
//!
//! Nothing here runs on its own: a request is made only when the user
//! connects, picks a model or sends a message, always on a worker thread,
//! and an answer streams back piece by piece so it can be shown (and
//! stopped) as it's written.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Ollama's address on this PC.
pub const DEFAULT_ENDPOINT: &str = "http://localhost:11434";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const LIST_TIMEOUT: Duration = Duration::from_secs(10);
// The first answer can wait while the server loads the model into memory.
const FIRST_TOKEN_TIMEOUT: Duration = Duration::from_secs(300);
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

/// The models the server at `endpoint` offers, sorted by name.
pub fn list_models(endpoint: &str) -> Result<Vec<String>, String> {
    let mut response = ureq::get(&api_url(endpoint, "models"))
        .config()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(LIST_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|error| unreachable(endpoint, &error))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_to_string()
        .map_err(|error| format!("Couldn't read the model list ({error})"))?;
    if status != 200 {
        return Err(server_error(status, &body));
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

/// Models that can chat: embedding models (which only turn text into
/// numbers) are listed by servers too, but can't answer.
pub fn chat_models(models: &[String]) -> Vec<String> {
    models
        .iter()
        .filter(|model| !model.to_ascii_lowercase().contains("embed"))
        .cloned()
        .collect()
}

/// Whether Ollama runs `model` on its own servers rather than on this PC:
/// its "cloud" models, such as "gpt-oss:120b-cloud" or "nemotron-3:cloud".
pub fn is_cloud_model(model: &str) -> bool {
    model
        .rsplit(':')
        .next()
        .is_some_and(|tag| tag == "cloud" || tag.ends_with("-cloud"))
}

/// A model to start with, never a cloud one: questions and code shouldn't
/// leave the PC unless the user picks such a model. A chat model made for
/// code is preferred; "base" models only complete text, so they come last.
pub fn pick_model(models: &[String]) -> Option<String> {
    let rank = |model: &str| {
        let model = model.to_ascii_lowercase();
        let chat = !model.contains("base");
        u8::from(chat) + u8::from(chat && model.contains("code"))
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

fn request_body(model: &str, messages: &[Message]) -> String {
    let messages: Vec<Value> = messages
        .iter()
        .map(|message| json!({"role": message.role.as_str(), "content": message.content}))
        .collect();
    json!({"model": model, "messages": messages, "stream": true}).to_string()
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
/// the answer to `on_text` as it arrives. Returns early (Ok) once `cancel`
/// is set.
pub fn stream_chat(
    endpoint: &str,
    model: &str,
    messages: &[Message],
    cancel: &AtomicBool,
    mut on_text: impl FnMut(&str),
) -> Result<(), String> {
    let response = ureq::post(&api_url(endpoint, "chat/completions"))
        .config()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(FIRST_TOKEN_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .header("Content-Type", "application/json")
        .send(request_body(model, messages))
        .map_err(|error| unreachable(endpoint, &error))?;
    let status = response.status().as_u16();
    let mut body = response.into_body();
    if status != 200 {
        let text = body
            .with_config()
            .limit(64 * 1024)
            .read_to_string()
            .unwrap_or_default();
        return Err(server_error(status, &text));
    }
    let mut received = 0;
    for line in BufReader::new(body.into_reader()).lines() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let line = line.map_err(|error| format!("The answer was cut off ({error})"))?;
        match parse_event(&line) {
            Event::Text(text) => {
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
    endpoint: &str,
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
    stream_chat(endpoint, model, &messages, cancel, |chunk| {
        output.push_str(chunk);
    })?;
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
        assert_eq!(pick_model(&models).as_deref(), Some("qwen2.5-coder:7b"));
        assert_eq!(
            pick_model(&["nomic-embed-text".to_string()]),
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
        assert_eq!(pick_model(&installed).as_deref(), Some("qwen2.5-coder:7b"));
        assert!(is_cloud_model("gpt-oss:120b-cloud"));
        assert!(is_cloud_model("nemotron-3-super:cloud"));
        assert!(!is_cloud_model("qwen2.5-coder:7b"));
        assert!(!is_cloud_model("llama3"));
        assert_eq!(
            pick_model(&["gpt-oss:20b-cloud".to_string()]),
            None,
            "a cloud model is only used when chosen"
        );
        assert_eq!(
            pick_model(&["qwen2.5-coder:1.5b-base".to_string(), "llama3.2:1b".into()]).as_deref(),
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
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
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
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });
        address
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
            &endpoint,
            "m",
            &[Message::new(Role::User, "hi")],
            &cancel,
            |text| pieces.push(text.to_string()),
        )
        .unwrap();
        assert_eq!(pieces, ["Hel", "lo"]);
    }

    #[test]
    fn a_missing_model_reports_the_servers_message() {
        let endpoint = serve_once(
            "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nConnection: close\r\n\
             Content-Length: 43\r\n\r\n{\"error\":{\"message\":\"model 'm' not found\"}}",
        );
        let cancel = AtomicBool::new(false);
        let error = stream_chat(&endpoint, "m", &[], &cancel, |_| {}).unwrap_err();
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
        let error = list_models(&endpoint).unwrap_err();
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
        let completion = complete_inline(&endpoint, "m", "let x = 10;\n", "", "rust", &cancel).unwrap();
        assert_eq!(completion, "let y = 42;");
    }
}
