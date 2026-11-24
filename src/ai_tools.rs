//! The project tools the AI Assistant's model may call
//! (design/ai-project-access-design.html, parts 2 and 3): search the project, list
//! its files, read a file, and see the problems language servers report;
//! and (part 3) propose an edit or a new file. Nothing here writes: a
//! proposal is recorded for the user to review and apply, and later reads
//! in the same question see it.
//!
//! What may be read is decided once per question: the files
//! `workflow::workspace_files` lists (so nothing in `.git`, build or
//! dependency folders, or anything `.gitignore` hides), minus files that
//! look like they hold secrets. A path the model gives is looked up in that
//! list, so `..`, absolute paths and links that lead out never match.
//!
//! Everything here runs on the question's worker thread.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, OnceLock};

/// Tool calls per question; past this the model answers with what it has.
pub const MAX_STEPS: usize = 25;
const MAX_SEARCH_HITS: usize = 50;
const MAX_LISTED: usize = 500;
const MAX_READ_LINES: usize = 400;
const MAX_READ_BYTES: usize = 24 * 1024;
const MAX_PROBLEMS: usize = 100;
/// A file bigger than this isn't read whole; its first part is.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// A proposed new file can be at most this big.
const MAX_NEW_FILE_BYTES: usize = 256 * 1024;
/// A file is edited only when all of it was read.
const MAX_EDITED_FILE_BYTES: usize = MAX_FILE_BYTES as usize;

/// Whether `path` looks like it holds secrets: environment files, keys and
/// credentials. The model can't read these, and attaching one asks first.
pub fn looks_secret(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    name.starts_with(".env")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name.starts_with("credentials")
        || [".pem", ".key", ".pfx", ".p12"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

/// A tool, as described to the model.
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

/// The four project tools.
pub fn definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "search",
            description: "Search the files of the user's project for text. Call this to find \
                          where something is defined or used before answering a question about \
                          the project. Returns up to 50 matching lines as path:line: text.",
            schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "The text to find"},
                    "regex": {"type": "boolean", "description": "Treat query as a regular expression (default false)"},
                    "path": {"type": "string", "description": "Only search under this folder of the project"}
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "list_files",
            description: "List the files of the user's project, optionally under one folder and \
                          matching a pattern such as *.rs. Call this to see how the project is \
                          laid out. Returns up to 500 paths.",
            schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "A folder of the project; omit for all of it"},
                    "pattern": {"type": "string", "description": "A file name pattern with * and ?, e.g. *.rs"}
                }
            }),
        },
        ToolDef {
            name: "read_file",
            description: "Read a file of the user's project, or some of its lines. Call this \
                          before answering about code you haven't seen. Returns at most 400 \
                          lines, each starting with its line number. An open file is read as \
                          it is in the editor, unsaved edits included.",
            schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "The file's path within the project, e.g. src/main.rs"},
                    "start_line": {"type": "integer", "description": "First line to read, from 1"},
                    "end_line": {"type": "integer", "description": "Last line to read"}
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "problems",
            description: "List the errors and warnings the editor's language servers report for \
                          the files open in the editor, or for one of them. Call this when the \
                          user asks about errors or why something doesn't build.",
            schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Only this file of the project"}
                }
            }),
        },
        ToolDef {
            name: "propose_edit",
            description: "Propose a change to a file of the user's project: replace the text \
                          `find` with `replace`. `find` must appear exactly once in the file, \
                          so copy it from read_file (without the line numbers) and include \
                          enough lines to make it unique. Nothing changes until the user \
                          reviews and applies it. Call it once per change; later reads show \
                          the file with your proposals.",
            schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "The file's path within the project"},
                    "find": {"type": "string", "description": "The exact text to replace"},
                    "replace": {"type": "string", "description": "The text to put in its place"}
                },
                "required": ["path", "find", "replace"]
            }),
        },
        ToolDef {
            name: "propose_new_file",
            description: "Propose a new file in the user's project, with all of its contents. \
                          The path must not exist yet. Nothing is created until the user \
                          reviews and applies it.",
            schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "The new file's path within the project, e.g. tests/sum_test.rs"},
                    "content": {"type": "string", "description": "The whole file"}
                },
                "required": ["path", "content"]
            }),
        },
    ]
}

/// A change the model proposed to one file: everything it proposed for
/// that file in one question.
#[derive(Clone, Debug, PartialEq)]
pub struct FileProposal {
    pub path: PathBuf,
    /// Its path within the project, as shown.
    pub name: String,
    pub new_file: bool,
    /// The text the proposals start from: the editor's or the file's.
    pub original: String,
    /// The text with every proposal made.
    pub proposed: String,
    /// Each edit, in order: the text found and what replaces it.
    pub edits: Vec<(String, String)>,
    /// Lines added and removed, from `original` to `proposed`.
    pub added: usize,
    pub removed: usize,
}

/// Lines added and removed from `before` to `after`.
pub fn line_counts(before: &str, after: &str) -> (usize, usize) {
    let rows = crate::workflow::text_diff_rows(before, after);
    let mut added = 0;
    let mut removed = 0;
    for row in rows.iter().filter(|row| row.changed) {
        match (row.before_number, row.after_number) {
            (Some(_), Some(_)) => {
                added += 1;
                removed += 1;
            }
            (None, Some(_)) => added += 1,
            (Some(_), None) => removed += 1,
            (None, None) => {}
        }
    }
    (added, removed)
}

/// Applies `edits` to `text` in order, each `find` matching exactly once.
/// Err names the first edit that no longer fits.
pub fn apply_edits(text: &str, edits: &[(String, String)]) -> Result<String, String> {
    let mut text = text.to_string();
    for (number, (find, replace)) in edits.iter().enumerate() {
        match text.matches(find.as_str()).count() {
            1 => text = text.replacen(find.as_str(), replace, 1),
            0 => return Err(format!("change {} no longer matches the file", number + 1)),
            _ => {
                return Err(format!(
                    "change {} matches more than one place now",
                    number + 1
                ));
            }
        }
    }
    Ok(text)
}

/// A problem a language server reports, as the editor holds it.
#[derive(Clone, Debug)]
pub struct Problem {
    pub path: PathBuf,
    /// From 1.
    pub line: usize,
    pub severity: &'static str,
    pub message: String,
}

/// What a tool call gave: the text for the model, a line for the chat, and
/// for a read, the file and line it started at (clicking the line opens it).
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub content: String,
    pub summary: String,
    pub is_error: bool,
    pub opened: Option<(PathBuf, usize)>,
    /// For a proposal: the file's proposals so far.
    pub proposal: Option<Box<FileProposal>>,
}

impl Outcome {
    fn error(content: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            summary: summary.into(),
            is_error: true,
            opened: None,
            proposal: None,
        }
    }
}

/// The project as one question sees it.
pub struct Project {
    root: PathBuf,
    /// Open files with unsaved edits, by lowercase relative path: these are
    /// read from the editor's text rather than from disk.
    edited: HashMap<String, String>,
    problems: Vec<Problem>,
    /// The files that may be read, with their lowercase relative paths,
    /// listed the first time a tool needs them.
    files: OnceLock<Vec<(PathBuf, String)>>,
    /// What the model proposed this question, by key.
    proposals: Mutex<HashMap<String, FileProposal>>,
}

// `path` relative to `root`, lowercase, with `/`: the key paths are looked
// up by. None outside `root`.
fn key(root: &Path, path: &Path) -> Option<String> {
    let plain = |path: &Path| {
        let text = path.to_string_lossy();
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_string();
        PathBuf::from(text)
    };
    let relative = plain(path).strip_prefix(plain(root)).ok()?.to_path_buf();
    Some(
        relative
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase(),
    )
}

// What the model wrote for a path, as a key: `/` between folders, no
// leading `./` or `/`, lowercase.
fn asked_key(asked: &str) -> String {
    let mut text = asked.trim().replace('\\', "/");
    while let Some(rest) = text.strip_prefix("./") {
        text = rest.to_string();
    }
    text.trim_start_matches('/')
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

// Whether `name` matches `pattern`, with `*` for any run and `?` for one
// character, ignoring case.
fn wildcard(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let name: Vec<char> = name.to_ascii_lowercase().chars().collect();
    let (mut p, mut n, mut star, mut mark) = (0, 0, None, 0);
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            mark = n;
            p += 1;
        } else if let Some(at) = star {
            p = at + 1;
            mark += 1;
            n = mark;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

fn input_str<'a>(input: &'a Value, field: &str) -> Option<&'a str> {
    input
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
}

fn input_line(input: &Value, field: &str) -> Option<usize> {
    let value = input.get(field)?;
    value
        .as_u64()
        .or_else(|| value.as_f64().map(|line| line.max(0.0) as u64))
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
        .map(|line| line as usize)
}

impl Project {
    /// `edited` are open files with unsaved edits and their text.
    pub fn new(root: PathBuf, edited: Vec<(PathBuf, String)>, problems: Vec<Problem>) -> Self {
        let edited = edited
            .into_iter()
            .filter_map(|(path, text)| Some((key(&root, &path)?, text)))
            .collect();
        Self {
            root,
            edited,
            problems,
            files: OnceLock::new(),
            proposals: Mutex::new(HashMap::new()),
        }
    }

    fn files(&self) -> &[(PathBuf, String)] {
        self.files.get_or_init(|| {
            crate::workflow::workspace_files(&self.root)
                .into_iter()
                .filter(|path| !looks_secret(path))
                .filter_map(|path| Some((key(&self.root, &path)?, path)))
                .map(|(key, path)| (path, key))
                .collect()
        })
    }

    // The readable file the model means by `asked`.
    fn find(&self, asked: &str) -> Result<(PathBuf, String), Outcome> {
        let wanted = asked_key(asked);
        let shown = wanted.clone();
        if looks_secret(Path::new(&wanted)) {
            return Err(Outcome::error(
                format!("{asked} may hold secrets, so it can't be read. Don't ask for it again."),
                format!("Not allowed: {shown}"),
            ));
        }
        self.files()
            .iter()
            .find(|(_, key)| *key == wanted)
            .map(|(path, key)| (path.clone(), key.clone()))
            .ok_or_else(|| {
                Outcome::error(
                    format!(
                        "{asked} isn't a file of the project that can be read (paths are relative \
                         to the project's folder; files outside it, in .git, or hidden by \
                         .gitignore can't be read). Use list_files or search to find the right path."
                    ),
                    format!("Not found or not allowed: {shown}"),
                )
            })
    }

    /// Runs tool `name` with the model's `input`.
    pub fn run(&self, name: &str, input: &Value, cancel: &AtomicBool) -> Outcome {
        match name {
            "search" => self.search(input, cancel),
            "list_files" => self.list(input),
            "read_file" => self.read(input),
            "problems" => self.problems(input),
            "propose_edit" => self.propose_edit(input),
            "propose_new_file" => self.propose_new_file(input),
            other => Outcome::error(
                format!("There's no tool called {other}."),
                format!("Unknown tool {other}"),
            ),
        }
    }

    fn search(&self, input: &Value, cancel: &AtomicBool) -> Outcome {
        let Some(query) = input_str(input, "query") else {
            return Outcome::error("search needs a query.", "Search without a query");
        };
        let regex = input.get("regex").and_then(Value::as_bool).unwrap_or(false);
        let options = crate::text_search::SearchOptions {
            regex,
            ..Default::default()
        };
        let matcher = match crate::text_search::Matcher::new(query, options) {
            Ok(matcher) => matcher,
            Err(error) => {
                return Outcome::error(
                    format!("The query couldn't be used: {error}"),
                    format!("Search for \u{201c}{query}\u{201d} failed"),
                );
            }
        };
        let under = input_str(input, "path").map(|path| format!("{}/", asked_key(path)));
        let readable: std::collections::HashSet<&str> =
            self.files().iter().map(|(_, key)| key.as_str()).collect();
        let hits =
            crate::workflow::search_workspace_where(&self.root, &matcher, cancel, &|_, _| true);
        let mut lines = Vec::new();
        let mut files = std::collections::HashSet::new();
        for hit in hits {
            let Some(key) = key(&self.root, &hit.path) else {
                continue;
            };
            if !readable.contains(key.as_str())
                || under.as_ref().is_some_and(|under| !key.starts_with(under))
            {
                continue;
            }
            files.insert(key.clone());
            let preview: String = hit.preview.trim().chars().take(200).collect();
            lines.push(format!("{key}:{}: {preview}", hit.line + 1));
            if lines.len() >= MAX_SEARCH_HITS {
                break;
            }
        }
        let summary = format!(
            "Searched for \u{201c}{query}\u{201d} \u{00b7} {} in {} {}",
            match lines.len() {
                1 => "1 match".to_string(),
                count => format!("{count} matches"),
            },
            files.len(),
            if files.len() == 1 { "file" } else { "files" }
        );
        let content = if lines.is_empty() {
            format!("No matches for {query}.")
        } else {
            lines.join("\n")
        };
        Outcome {
            content,
            summary,
            is_error: false,
            opened: None,
            proposal: None,
        }
    }

    fn list(&self, input: &Value) -> Outcome {
        let under = input_str(input, "path").map(asked_key);
        let pattern = input_str(input, "pattern");
        let prefix = under.as_ref().map(|under| format!("{under}/"));
        let mut keys: Vec<&str> = self
            .files()
            .iter()
            .map(|(_, key)| key.as_str())
            .filter(|key| prefix.as_ref().is_none_or(|prefix| key.starts_with(prefix)))
            .filter(|key| {
                pattern.is_none_or(|pattern| {
                    let name = key.rsplit('/').next().unwrap_or(key);
                    wildcard(pattern, name)
                })
            })
            .collect();
        keys.sort_unstable();
        let total = keys.len();
        keys.truncate(MAX_LISTED);
        let place = under.clone().unwrap_or_else(|| "the project".into());
        let summary = format!(
            "Listed {place}{} \u{00b7} {total} {}",
            pattern.map(|p| format!(" ({p})")).unwrap_or_default(),
            if total == 1 { "file" } else { "files" }
        );
        let mut content = if keys.is_empty() {
            format!("No files in {place}.")
        } else {
            keys.join("\n")
        };
        if total > MAX_LISTED {
            content.push_str(&format!(
                "\n({} more not shown; narrow it with path or pattern)",
                total - MAX_LISTED
            ));
        }
        Outcome {
            content,
            summary,
            is_error: false,
            opened: None,
            proposal: None,
        }
    }

    // The text of readable file `path` (`key`): the editor's when it has
    // unsaved edits, else the file's first MAX_FILE_BYTES. The bool says
    // whether that's all of it.
    fn text_of(&self, path: &Path, key: &str) -> Result<(String, bool), Outcome> {
        if let Some(text) = self.edited.get(key) {
            return Ok((text.clone(), true));
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        let read = std::fs::File::open(path)
            .and_then(|file| file.take(MAX_FILE_BYTES).read_to_end(&mut bytes));
        if let Err(error) = read {
            return Err(Outcome::error(
                format!("{key} couldn't be read: {error}"),
                format!("Couldn't read {key}"),
            ));
        }
        if bytes.iter().take(8192).any(|&byte| byte == 0) {
            return Err(Outcome::error(
                format!("{key} isn't a text file."),
                format!("Not text: {key}"),
            ));
        }
        let whole = (bytes.len() as u64) < MAX_FILE_BYTES;
        Ok((String::from_utf8_lossy(&bytes).replace("\r\n", "\n"), whole))
    }

    fn proposal_for(&self, key: &str) -> Option<FileProposal> {
        self.proposals.lock().ok()?.get(key).cloned()
    }

    fn read(&self, input: &Value) -> Outcome {
        let Some(asked) = input_str(input, "path") else {
            return Outcome::error("read_file needs a path.", "Read without a path");
        };
        // A file with proposals reads as proposed, new files included.
        let proposed = self.proposal_for(&asked_key(asked));
        let (path, key, text) = match proposed {
            Some(proposal) => {
                let key = asked_key(asked);
                (proposal.path, key, proposal.proposed)
            }
            None => {
                let (path, key) = match self.find(asked) {
                    Ok(found) => found,
                    Err(outcome) => return outcome,
                };
                match self.text_of(&path, &key) {
                    Ok((text, _)) => (path, key, text),
                    Err(outcome) => return outcome,
                }
            }
        };
        let lines: Vec<&str> = text.lines().collect();
        let total = lines.len();
        let first = input_line(input, "start_line").unwrap_or(1).max(1);
        if total > 0 && first > total {
            return Outcome::error(
                format!("{key} has only {total} lines."),
                format!("Read {key} past its end"),
            );
        }
        let last = input_line(input, "end_line")
            .unwrap_or(total)
            .clamp(first, total.max(first));
        let mut out = String::new();
        let mut shown_last = first.saturating_sub(1);
        for number in first..=last.min(total) {
            let line = lines[number - 1];
            let row = format!("{number}| {line}\n");
            if number - first >= MAX_READ_LINES || out.len() + row.len() > MAX_READ_BYTES {
                break;
            }
            out.push_str(&row);
            shown_last = number;
        }
        let count = shown_last + 1 - first.min(shown_last + 1);
        let whole = first == 1 && shown_last >= total;
        let header = if total == 0 {
            format!("{key} is empty.")
        } else if whole {
            format!("{key} ({total} lines):")
        } else {
            format!("{key}, lines {first}\u{2013}{shown_last} of {total}:")
        };
        let header = if self.proposal_for(&key).is_some() {
            format!("{header} (with your proposed changes, not applied yet)")
        } else {
            header
        };
        let mut content = format!("{header}\n{out}");
        if shown_last < last.min(total) {
            content.push_str(&format!(
                "(cut at {} lines; read from line {} for more)",
                MAX_READ_LINES,
                shown_last + 1
            ));
        }
        let range = if whole || total == 0 {
            String::new()
        } else {
            format!(" {first}\u{2013}{shown_last}")
        };
        Outcome {
            content,
            summary: format!(
                "Read {key}{range} \u{00b7} {count} {}",
                if count == 1 { "line" } else { "lines" }
            ),
            is_error: false,
            opened: Some((path, first)),
            proposal: None,
        }
    }

    // Records `proposal` and reports it.
    fn proposed(&self, proposal: FileProposal, summary: String) -> Outcome {
        let key = asked_key(&proposal.name);
        if let Ok(mut proposals) = self.proposals.lock() {
            proposals.insert(key, proposal.clone());
        }
        Outcome {
            content: "Proposed. Nothing changes until the user reviews and applies it; \
                      read_file now shows the file with your proposals."
                .into(),
            summary,
            is_error: false,
            opened: None,
            proposal: Some(Box::new(proposal)),
        }
    }

    fn propose_edit(&self, input: &Value) -> Outcome {
        let (Some(asked), Some(find)) = (
            input_str(input, "path"),
            input.get("find").and_then(Value::as_str),
        ) else {
            return Outcome::error(
                "propose_edit needs path, find and replace.",
                "Edit without path or find",
            );
        };
        let Some(replace) = input.get("replace").and_then(Value::as_str) else {
            return Outcome::error("propose_edit needs replace.", "Edit without replace");
        };
        if find.is_empty() {
            return Outcome::error("find can't be empty.", "Edit with nothing to find");
        }
        let earlier = self.proposal_for(&asked_key(asked));
        let (path, key, base, original, mut edits, new_file) = match earlier {
            Some(earlier) => (
                earlier.path,
                asked_key(asked),
                earlier.proposed,
                earlier.original,
                earlier.edits,
                earlier.new_file,
            ),
            None => {
                let (path, key) = match self.find(asked) {
                    Ok(found) => found,
                    Err(outcome) => return outcome,
                };
                let (text, whole) = match self.text_of(&path, &key) {
                    Ok(read) => read,
                    Err(outcome) => return outcome,
                };
                if !whole || text.len() > MAX_EDITED_FILE_BYTES {
                    return Outcome::error(
                        format!("{key} is too big to change here."),
                        format!("Too big to change: {key}"),
                    );
                }
                (path, key, text.clone(), text, Vec::new(), false)
            }
        };
        match base.matches(find).count() {
            1 => {}
            0 => {
                return Outcome::error(
                    format!(
                        "find doesn't appear in {key}. Read the file again and copy the text \
                         exactly, without the line numbers."
                    ),
                    format!("Change to {key} didn't match"),
                );
            }
            count => {
                return Outcome::error(
                    format!(
                        "find appears {count} times in {key}. Include more of the surrounding \
                         lines so it matches exactly once."
                    ),
                    format!("Change to {key} matched {count} places"),
                );
            }
        }
        let proposed = base.replacen(find, replace, 1);
        edits.push((find.to_string(), replace.to_string()));
        let (added, removed) = line_counts(&original, &proposed);
        let summary = format!("Proposed a change to {key} \u{00b7} +{added} \u{2212}{removed}");
        self.proposed(
            FileProposal {
                path,
                name: key,
                new_file,
                original,
                proposed,
                edits,
                added,
                removed,
            },
            summary,
        )
    }

    fn propose_new_file(&self, input: &Value) -> Outcome {
        let (Some(asked), Some(content)) = (
            input_str(input, "path"),
            input.get("content").and_then(Value::as_str),
        ) else {
            return Outcome::error(
                "propose_new_file needs path and content.",
                "New file without path or content",
            );
        };
        let key = asked_key(asked);
        let bad = key.is_empty()
            || key.contains(':')
            || asked.trim().starts_with(['/', '\\'])
            || key
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".." || part == ".git")
            || looks_secret(Path::new(&key));
        if bad {
            return Outcome::error(
                format!(
                    "{asked} can't be created: give a path inside the project, like src/new.rs."
                ),
                format!("Not allowed: {asked}"),
            );
        }
        let path = self
            .root
            .join(key.replace('/', std::path::MAIN_SEPARATOR_STR));
        if path.exists() || self.proposal_for(&key).is_some() {
            return Outcome::error(
                format!("{key} already exists; use propose_edit to change it."),
                format!("Already exists: {key}"),
            );
        }
        if content.len() > MAX_NEW_FILE_BYTES {
            return Outcome::error(
                format!("{key} is too big to propose (256 KB at most)."),
                format!("Too big: {key}"),
            );
        }
        let (added, _) = line_counts("", content);
        let summary = format!("Proposed a new file {key} \u{00b7} +{added}");
        self.proposed(
            FileProposal {
                path,
                name: key,
                new_file: true,
                original: String::new(),
                proposed: content.to_string(),
                edits: Vec::new(),
                added,
                removed: 0,
            },
            summary,
        )
    }

    fn problems(&self, input: &Value) -> Outcome {
        let only = input_str(input, "path").map(asked_key);
        let mut lines = Vec::new();
        for problem in &self.problems {
            let Some(key) = key(&self.root, &problem.path) else {
                continue;
            };
            if only.as_ref().is_some_and(|only| *only != key) || looks_secret(&problem.path) {
                continue;
            }
            let message = problem.message.lines().next().unwrap_or("").trim();
            lines.push(format!(
                "{key}:{}: {}: {message}",
                problem.line, problem.severity
            ));
            if lines.len() >= MAX_PROBLEMS {
                break;
            }
        }
        let place = only.clone().unwrap_or_else(|| "open files".into());
        Outcome {
            summary: format!(
                "Problems in {place} \u{00b7} {}",
                match lines.len() {
                    0 => "none".to_string(),
                    count => count.to_string(),
                }
            ),
            content: if lines.is_empty() {
                format!("No errors or warnings reported in {place}.")
            } else {
                lines.join("\n")
            },
            is_error: false,
            opened: None,
            proposal: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("lightline-ai-tools-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn write(&self, relative: &str, text: &str) -> PathBuf {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            path
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn project(folder: &Folder) -> Project {
        Project::new(folder.0.clone(), Vec::new(), Vec::new())
    }

    fn run(project: &Project, name: &str, input: Value) -> Outcome {
        project.run(name, &input, &AtomicBool::new(false))
    }

    #[test]
    fn reads_number_lines_and_say_what_they_cover() {
        let folder = Folder::new("read");
        let text: String = (1..=10).map(|n| format!("line {n}\n")).collect();
        let path = folder.write("src/main.rs", &text);
        let project = project(&folder);
        let whole = run(&project, "read_file", json!({"path": "src/main.rs"}));
        assert!(!whole.is_error, "{}", whole.content);
        assert!(
            whole
                .content
                .starts_with("src/main.rs (10 lines):\n1| line 1\n")
        );
        assert_eq!(whole.summary, "Read src/main.rs \u{00b7} 10 lines");
        assert_eq!(whole.opened, Some((path, 1)));

        let part = run(
            &project,
            "read_file",
            json!({"path": "./SRC\\main.rs", "start_line": 3, "end_line": 4}),
        );
        assert_eq!(
            part.content,
            "src/main.rs, lines 3\u{2013}4 of 10:\n3| line 3\n4| line 4\n"
        );
        assert_eq!(part.summary, "Read src/main.rs 3\u{2013}4 \u{00b7} 2 lines");
        let past = run(
            &project,
            "read_file",
            json!({"path": "src/main.rs", "start_line": 50}),
        );
        assert!(past.is_error);
    }

    #[test]
    fn long_files_are_cut_at_400_lines() {
        let folder = Folder::new("long");
        let text: String = (1..=1000).map(|n| format!("x{n}\n")).collect();
        folder.write("big.txt", &text);
        let read = run(&project(&folder), "read_file", json!({"path": "big.txt"}));
        assert!(read.content.contains("400| x400\n"));
        assert!(!read.content.contains("401| x401"));
        assert!(read.content.contains("read from line 401 for more"));
        assert_eq!(read.summary, "Read big.txt 1\u{2013}400 \u{00b7} 400 lines");
    }

    #[test]
    fn nothing_outside_the_project_or_hidden_or_secret_can_be_read() {
        let folder = Folder::new("rules");
        folder.write("src/main.rs", "fn main() {}\n");
        folder.write(".env", "TOKEN=x\n");
        folder.write("keys/server.pem", "-----BEGIN-----\n");
        folder.write(".gitignore", "secret_notes.txt\nbuild/\n");
        folder.write("secret_notes.txt", "hidden\n");
        folder.write("build/out.txt", "built\n");
        folder.write(".git/config", "[core]\n");
        let outside = Folder::new("outside");
        outside.write("other.txt", "not ours\n");
        let project = project(&folder);
        let absolute = outside.0.join("other.txt");
        for path in [
            ".env",
            "keys/server.pem",
            "secret_notes.txt",
            "build/out.txt",
            ".git/config",
            "../lightline-ai-tools-outside/other.txt",
            &absolute.to_string_lossy(),
            "src/missing.rs",
        ] {
            let read = run(&project, "read_file", json!({"path": path}));
            assert!(read.is_error, "{path} was readable: {}", read.content);
            assert!(read.opened.is_none());
        }
        assert!(!run(&project, "read_file", json!({"path": "src/main.rs"})).is_error);
        let listed = run(&project, "list_files", json!({}));
        assert_eq!(listed.content, ".gitignore\nsrc/main.rs");
        let found = run(&project, "search", json!({"query": "TOKEN"}));
        assert_eq!(found.content, "No matches for TOKEN.");
    }

    #[test]
    fn search_finds_lines_and_can_be_limited_to_a_folder() {
        let folder = Folder::new("search");
        folder.write("src/a.rs", "fn stream_chat() {}\nfn other() {}\n");
        folder.write("src/b.rs", "// calls stream_chat\n");
        folder.write("docs/c.md", "stream_chat is documented\n");
        let project = project(&folder);
        let all = run(&project, "search", json!({"query": "stream_chat"}));
        assert_eq!(
            all.summary,
            "Searched for \u{201c}stream_chat\u{201d} \u{00b7} 3 matches in 3 files"
        );
        assert!(all.content.contains("src/a.rs:1: fn stream_chat() {}"));
        let src = run(
            &project,
            "search",
            json!({"query": "stream_\\w+", "regex": true, "path": "src"}),
        );
        assert_eq!(src.content.lines().count(), 2, "{}", src.content);
        let bad = run(&project, "search", json!({"query": "(", "regex": true}));
        assert!(bad.is_error);
        assert!(run(&project, "search", json!({})).is_error);
    }

    #[test]
    fn listing_takes_a_folder_and_a_pattern() {
        let folder = Folder::new("list");
        folder.write("src/a.rs", "");
        folder.write("src/b.txt", "");
        folder.write("src/deep/c.rs", "");
        folder.write("README.md", "");
        let project = project(&folder);
        let rs = run(
            &project,
            "list_files",
            json!({"path": "src", "pattern": "*.rs"}),
        );
        assert_eq!(rs.content, "src/a.rs\nsrc/deep/c.rs");
        assert_eq!(rs.summary, "Listed src (*.rs) \u{00b7} 2 files");
        assert!(wildcard("*.r?", "main.rs"));
        assert!(!wildcard("*.rs", "main.rsx"));
        assert!(wildcard("READ*", "readme.md"));
    }

    #[test]
    fn unsaved_edits_are_read_from_the_editor() {
        let folder = Folder::new("edited");
        let path = folder.write("src/main.rs", "on disk\n");
        let project = Project::new(
            folder.0.clone(),
            vec![(path, "in the editor\n".into())],
            Vec::new(),
        );
        let read = run(&project, "read_file", json!({"path": "src/main.rs"}));
        assert!(
            read.content.contains("1| in the editor"),
            "{}",
            read.content
        );
    }

    #[test]
    fn edits_are_proposed_not_made_and_later_reads_see_them() {
        let folder = Folder::new("propose");
        let path = folder.write("src/main.rs", "fn main() {\n    run();\n}\n");
        let project = project(&folder);
        let first = run(
            &project,
            "propose_edit",
            json!({"path": "src/main.rs", "find": "    run();", "replace": "    setup();\n    run();"}),
        );
        assert!(!first.is_error, "{}", first.content);
        assert_eq!(
            first.summary,
            "Proposed a change to src/main.rs \u{00b7} +1 \u{2212}0"
        );
        // The file itself is untouched.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "fn main() {\n    run();\n}\n"
        );
        let read = run(&project, "read_file", json!({"path": "src/main.rs"}));
        assert!(read.content.contains("2|     setup();"), "{}", read.content);
        assert!(read.content.contains("not applied yet"));
        // A second change builds on the first.
        let second = run(
            &project,
            "propose_edit",
            json!({"path": "src/main.rs", "find": "fn main()", "replace": "pub fn main()"}),
        );
        let proposal = second.proposal.unwrap();
        assert_eq!(proposal.original, "fn main() {\n    run();\n}\n");
        assert_eq!(
            proposal.proposed,
            "pub fn main() {\n    setup();\n    run();\n}\n"
        );
        assert_eq!(proposal.edits.len(), 2);
        assert_eq!((proposal.added, proposal.removed), (2, 1));
        assert_eq!(
            apply_edits(&proposal.original, &proposal.edits).unwrap(),
            proposal.proposed
        );
    }

    #[test]
    fn an_edit_must_match_exactly_once() {
        let folder = Folder::new("match");
        folder.write("a.rs", "x();\nx();\n");
        let project = project(&folder);
        let twice = run(
            &project,
            "propose_edit",
            json!({"path": "a.rs", "find": "x();", "replace": "y();"}),
        );
        assert!(
            twice.is_error && twice.content.contains("2 times"),
            "{}",
            twice.content
        );
        let none = run(
            &project,
            "propose_edit",
            json!({"path": "a.rs", "find": "z();", "replace": "y();"}),
        );
        assert!(none.is_error && none.content.contains("doesn't appear"));
        let secret = run(
            &project,
            "propose_edit",
            json!({"path": ".env", "find": "a", "replace": "b"}),
        );
        assert!(secret.is_error);
        assert!(apply_edits("x();\n", &[("z".into(), "y".into())]).is_err());
    }

    #[test]
    fn new_files_stay_inside_the_project_and_dont_replace_any() {
        let folder = Folder::new("newfile");
        folder.write("src/main.rs", "fn main() {}\n");
        let project = project(&folder);
        let made = run(
            &project,
            "propose_new_file",
            json!({"path": "tests/sum_test.rs", "content": "#[test]\nfn t() {}\n"}),
        );
        assert!(!made.is_error, "{}", made.content);
        assert_eq!(
            made.summary,
            "Proposed a new file tests/sum_test.rs \u{00b7} +2"
        );
        assert!(!folder.0.join("tests").exists());
        let proposal = made.proposal.unwrap();
        assert!(proposal.new_file);
        assert_eq!(proposal.path, folder.0.join("tests").join("sum_test.rs"));
        // It reads back as proposed.
        let read = run(&project, "read_file", json!({"path": "tests/sum_test.rs"}));
        assert!(read.content.contains("2| fn t() {}"), "{}", read.content);
        for bad in [
            "src/main.rs",
            "../out.rs",
            "C:/x.rs",
            "/x.rs",
            ".git/hooks/x",
            ".env",
            "a//b.rs",
        ] {
            let refused = run(
                &project,
                "propose_new_file",
                json!({"path": bad, "content": "x"}),
            );
            assert!(refused.is_error, "{bad} was allowed");
        }
    }

    #[test]
    fn problems_list_what_the_editor_reports() {
        let folder = Folder::new("problems");
        let path = folder.write("src/main.rs", "fn main() {}\n");
        let problems = vec![Problem {
            path,
            line: 3,
            severity: "error",
            message: "expected `;`\nmore detail".into(),
        }];
        let project = Project::new(folder.0.clone(), Vec::new(), problems);
        let all = run(&project, "problems", json!({}));
        assert_eq!(all.content, "src/main.rs:3: error: expected `;`");
        assert_eq!(all.summary, "Problems in open files \u{00b7} 1");
        let other = run(&project, "problems", json!({"path": "src/lib.rs"}));
        assert_eq!(other.summary, "Problems in src/lib.rs \u{00b7} none");
        assert!(run(&project, "nope", json!({})).is_error);
    }
}
