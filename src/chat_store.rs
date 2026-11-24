//! Earlier AI Assistant chats, kept on this PC only
//! (design/ai-chat-design.html, part 3). Each workspace has a folder under
//! `%APPDATA%\LightLine\chats\`, named by a hash of the workspace's path,
//! holding one JSON file per chat and an `index.json` with each chat's id,
//! title and time, so the list is one small read. The newest MAX_CHATS are
//! kept. Every write is atomic (a temporary file, then a rename), and the
//! whole folder is written by one save at a time.
//!
//! Nothing here runs at startup: the index is read when the list is opened,
//! and a chat when it's opened; a chat is written when an answer finishes.

use serde_json::{Value, json};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Chats kept per workspace; older ones are deleted.
pub const MAX_CHATS: usize = 50;
/// A chat's file is at most this big: the oldest questions and answers are
/// left out of the file past it.
const MAX_CHAT_BYTES: usize = 2 * 1024 * 1024;

// One save (chat and index) at a time.
static WRITING: Mutex<()> = Mutex::new(());

/// A chat in the list: enough to show it and open it.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatSummary {
    pub id: String,
    pub title: String,
    /// When it last changed, in milliseconds since 1970.
    pub updated: u64,
}

/// A step of an answer, as kept.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StoredStep {
    pub summary: String,
    /// The tool's kind: "search", "read", "list", "problems", "change" or
    /// "other".
    pub kind: String,
    pub error: bool,
    /// Where in the answer's text it came.
    pub at: usize,
    /// For a read: the file and line it opens.
    pub opened: Option<(PathBuf, usize)>,
}

/// A file's chip on a question, as kept.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StoredChip {
    pub path: PathBuf,
    pub label: String,
    pub warn: bool,
}

/// A question or an answer, as kept.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StoredEntry {
    /// "user" or "assistant".
    pub role: String,
    /// The question as shown, or the answer.
    pub text: String,
    /// What the model was sent for a question (its code included), so a
    /// reopened chat can go on.
    pub prompt: String,
    pub context: Option<String>,
    pub chips: Vec<StoredChip>,
    pub steps: Vec<StoredStep>,
    /// The model that answered.
    pub model: String,
    pub error: Option<String>,
    /// How long the answer took, in milliseconds.
    pub took_ms: Option<u64>,
}

/// A whole chat.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StoredChat {
    pub id: String,
    pub title: String,
    pub updated: u64,
    pub entries: Vec<StoredEntry>,
}

/// The folder for `workspace`'s chats (or for chats with no folder open),
/// under `base` (`%APPDATA%\LightLine\chats`).
pub fn workspace_dir(base: &Path, workspace: Option<&Path>) -> PathBuf {
    match workspace {
        Some(root) => {
            let key = root.to_string_lossy().replace('/', "\\").to_lowercase();
            let key = key.trim_end_matches('\\');
            base.join(format!("{:016x}", fnv1a(key.as_bytes())))
        }
        None => base.join("no-folder"),
    }
}

/// `%APPDATA%\LightLine\chats`.
pub fn base_dir() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("APPDATA")?)
            .join("LightLine")
            .join("chats"),
    )
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A chat's title: its first question's first line, at most 80 characters.
pub fn title_for(question: &str) -> String {
    let line = question
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty());
    let line = line.unwrap_or("Chat");
    if line.chars().count() > 80 {
        let cut: String = line.chars().take(79).collect();
        format!("{}\u{2026}", cut.trim_end())
    } else {
        line.to_string()
    }
}

// Only ids LightLine made (digits) name files, so an index entry can't
// point outside the folder.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|byte| byte.is_ascii_digit())
}

/// The chats in `dir`, newest first. A missing or unreadable index is an
/// empty list.
pub fn list(dir: &Path) -> Vec<ChatSummary> {
    let Ok(text) = fs::read_to_string(dir.join("index.json")) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let mut chats: Vec<ChatSummary> = value
        .get("chats")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item.get("id")?.as_str()?.to_string();
                    valid_id(&id).then(|| ChatSummary {
                        id,
                        title: item
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or("Chat")
                            .to_string(),
                        updated: item.get("updated").and_then(Value::as_u64).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    chats.sort_by_key(|chat| std::cmp::Reverse(chat.updated));
    chats
}

/// Chat `id` from `dir`; None when it's gone or unreadable.
pub fn load(dir: &Path, id: &str) -> Option<StoredChat> {
    if !valid_id(id) {
        return None;
    }
    let text = fs::read_to_string(dir.join(format!("{id}.json"))).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    Some(chat_from_value(&value, id))
}

/// Writes `chat` to `dir` and puts it first in the index; past MAX_CHATS,
/// the oldest are deleted.
pub fn save(dir: &Path, chat: &StoredChat) -> io::Result<()> {
    if !valid_id(&chat.id) {
        return Err(io::Error::other("not a chat id"));
    }
    let _writing = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    fs::create_dir_all(dir)?;
    let mut value = chat_to_value(chat);
    let mut text = serde_json::to_string(&value).map_err(io::Error::other)?;
    // Too big: the oldest questions and answers go first.
    while text.len() > MAX_CHAT_BYTES {
        let Some(entries) = value.get_mut("entries").and_then(Value::as_array_mut) else {
            break;
        };
        if entries.len() <= 2 {
            break;
        }
        entries.drain(..2);
        text = serde_json::to_string(&value).map_err(io::Error::other)?;
    }
    write_atomic(&dir.join(format!("{}.json", chat.id)), &text)?;

    let mut chats = list(dir);
    chats.retain(|summary| summary.id != chat.id);
    chats.insert(
        0,
        ChatSummary {
            id: chat.id.clone(),
            title: chat.title.clone(),
            updated: chat.updated,
        },
    );
    chats.sort_by_key(|chat| std::cmp::Reverse(chat.updated));
    for old in chats.split_off(MAX_CHATS.min(chats.len())) {
        let _ = fs::remove_file(dir.join(format!("{}.json", old.id)));
    }
    write_index(dir, &chats)
}

/// Takes chat `id` out of `dir`.
pub fn delete(dir: &Path, id: &str) -> io::Result<()> {
    if !valid_id(id) {
        return Ok(());
    }
    let _writing = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut chats = list(dir);
    chats.retain(|summary| summary.id != id);
    match fs::remove_file(dir.join(format!("{id}.json"))) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    write_index(dir, &chats)
}

/// Deletes every kept chat, of every workspace.
pub fn delete_all(base: &Path) -> io::Result<()> {
    let _writing = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match fs::remove_dir_all(base) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn write_index(dir: &Path, chats: &[ChatSummary]) -> io::Result<()> {
    let value = json!({
        "format": 1,
        "chats": chats.iter().map(|chat| json!({
            "id": chat.id,
            "title": chat.title,
            "updated": chat.updated,
        })).collect::<Vec<_>>(),
    });
    let text = serde_json::to_string_pretty(&value).map_err(io::Error::other)?;
    write_atomic(&dir.join("index.json"), &text)
}

fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, text)?;
    fs::rename(&temporary, path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

fn chat_to_value(chat: &StoredChat) -> Value {
    json!({
        "format": 1,
        "title": chat.title,
        "updated": chat.updated,
        "entries": chat.entries.iter().map(|entry| json!({
            "role": entry.role,
            "text": entry.text,
            "prompt": entry.prompt,
            "context": entry.context,
            "chips": entry.chips.iter().map(|chip| json!({
                "path": chip.path.to_string_lossy(),
                "label": chip.label,
                "warn": chip.warn,
            })).collect::<Vec<_>>(),
            "steps": entry.steps.iter().map(|step| json!({
                "summary": step.summary,
                "kind": step.kind,
                "error": step.error,
                "at": step.at,
                "opened": step.opened.as_ref().map(|(path, line)| json!([path.to_string_lossy(), line])),
            })).collect::<Vec<_>>(),
            "model": entry.model,
            "error": entry.error,
            "tookMs": entry.took_ms,
        })).collect::<Vec<_>>(),
    })
}

fn chat_from_value(value: &Value, id: &str) -> StoredChat {
    let text = |item: &Value, key: &str| {
        item.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let entries = value
        .get("entries")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| {
                    matches!(
                        item.get("role").and_then(Value::as_str),
                        Some("user" | "assistant")
                    )
                })
                .map(|item| StoredEntry {
                    role: text(item, "role"),
                    text: text(item, "text"),
                    prompt: text(item, "prompt"),
                    context: item
                        .get("context")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    chips: item
                        .get("chips")
                        .and_then(Value::as_array)
                        .map(|chips| {
                            chips
                                .iter()
                                .map(|chip| StoredChip {
                                    path: PathBuf::from(text(chip, "path")),
                                    label: text(chip, "label"),
                                    warn: chip
                                        .get("warn")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    steps: item
                        .get("steps")
                        .and_then(Value::as_array)
                        .map(|steps| {
                            steps
                                .iter()
                                .map(|step| StoredStep {
                                    summary: text(step, "summary"),
                                    kind: text(step, "kind"),
                                    error: step
                                        .get("error")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false),
                                    at: step.get("at").and_then(Value::as_u64).unwrap_or(0)
                                        as usize,
                                    opened: step.get("opened").and_then(Value::as_array).and_then(
                                        |pair| {
                                            Some((
                                                PathBuf::from(pair.first()?.as_str()?),
                                                pair.get(1)?.as_u64()? as usize,
                                            ))
                                        },
                                    ),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    model: text(item, "model"),
                    error: item
                        .get("error")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    took_ms: item.get("tookMs").and_then(Value::as_u64),
                })
                .collect()
        })
        .unwrap_or_default();
    StoredChat {
        id: id.to_string(),
        title: value
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Chat")
            .to_string(),
        updated: value.get("updated").and_then(Value::as_u64).unwrap_or(0),
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lightline-chats-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn chat(id: u64, title: &str) -> StoredChat {
        StoredChat {
            id: id.to_string(),
            title: title.into(),
            updated: id,
            entries: vec![
                StoredEntry {
                    role: "user".into(),
                    text: title.into(),
                    prompt: format!("{title}\n\nFile src/a.rs:\n```rust\nfn a() {{}}\n```"),
                    chips: vec![StoredChip {
                        path: PathBuf::from("D:\\ws\\src\\a.rs"),
                        label: "a.rs \u{b7} 1 line".into(),
                        warn: false,
                    }],
                    ..Default::default()
                },
                StoredEntry {
                    role: "assistant".into(),
                    text: "Let me look.\n\nIt returns NaN.".into(),
                    steps: vec![StoredStep {
                        summary: "Read src/a.rs \u{b7} 1 line".into(),
                        kind: "read".into(),
                        error: false,
                        at: 12,
                        opened: Some((PathBuf::from("D:\\ws\\src\\a.rs"), 1)),
                    }],
                    model: "claude-opus-5-5".into(),
                    took_ms: Some(1400),
                    ..Default::default()
                },
            ],
        }
    }

    #[test]
    fn a_chat_is_saved_listed_and_loaded_back() {
        let dir = folder("round-trip");
        save(&dir, &chat(100, "First")).unwrap();
        save(&dir, &chat(200, "Second")).unwrap();
        let listed = list(&dir);
        assert_eq!(
            listed.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
            ["Second", "First"]
        );
        assert_eq!(load(&dir, "100"), Some(chat(100, "First")));
        // Saving it again moves it to the top, once.
        let mut again = chat(100, "First");
        again.updated = 300;
        save(&dir, &again).unwrap();
        assert_eq!(list(&dir)[0].id, "100");
        assert_eq!(list(&dir).len(), 2);
        delete(&dir, "100").unwrap();
        assert_eq!(list(&dir).len(), 1);
        assert_eq!(load(&dir, "100"), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_the_newest_chats_are_kept() {
        let dir = folder("limit");
        for id in 1..=(MAX_CHATS as u64 + 3) {
            save(&dir, &chat(id, "Chat")).unwrap();
        }
        let listed = list(&dir);
        assert_eq!(listed.len(), MAX_CHATS);
        assert_eq!(listed.last().unwrap().id, "4");
        assert!(!dir.join("3.json").exists());
        assert!(dir.join("4.json").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ids_never_name_a_file_outside_the_folder() {
        let dir = folder("ids");
        assert_eq!(load(&dir, "..\\..\\settings"), None);
        assert!(save(&dir, &chat(1, "x")).is_ok());
        let mut bad = chat(1, "x");
        bad.id = "../x".into();
        assert!(save(&dir, &bad).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn each_workspace_has_its_own_folder() {
        let base = Path::new("C:\\chats");
        let a = workspace_dir(base, Some(Path::new("D:\\Projects\\App")));
        assert_eq!(
            a,
            workspace_dir(base, Some(Path::new("d:\\projects\\app\\")))
        );
        assert_ne!(
            a,
            workspace_dir(base, Some(Path::new("D:\\Projects\\Other")))
        );
        assert_eq!(workspace_dir(base, None), base.join("no-folder"));
    }

    #[test]
    fn titles_come_from_the_first_question() {
        assert_eq!(
            title_for("\n  What does mean() do?\nMore"),
            "What does mean() do?"
        );
        assert_eq!(title_for(""), "Chat");
        let long = "x".repeat(100);
        assert_eq!(title_for(&long).chars().count(), 80);
    }

    #[test]
    fn deleting_everything_is_fine_when_nothing_is_there() {
        let base = folder("all");
        assert!(delete_all(&base).is_ok());
        save(&workspace_dir(&base, None), &chat(5, "x")).unwrap();
        delete_all(&base).unwrap();
        assert!(!base.exists());
    }
}
