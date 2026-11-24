//! The project tools the AI Assistant's model may call
//! (design/ai-project-access-design.html, part 2): search the project, list
//! its files, read a file, and see the problems language servers report.
//! They only read, and only inside the open folder.
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
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;

/// Tool calls per question; past this the model answers with what it has.
pub const MAX_STEPS: usize = 25;
const MAX_SEARCH_HITS: usize = 50;
const MAX_LISTED: usize = 500;
const MAX_READ_LINES: usize = 400;
const MAX_READ_BYTES: usize = 24 * 1024;
const MAX_PROBLEMS: usize = 100;
/// A file bigger than this isn't read whole; its first part is.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

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
    ]
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
}

impl Outcome {
    fn error(content: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            summary: summary.into(),
            is_error: true,
            opened: None,
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
        }
    }

    fn read(&self, input: &Value) -> Outcome {
        let Some(asked) = input_str(input, "path") else {
            return Outcome::error("read_file needs a path.", "Read without a path");
        };
        let (path, key) = match self.find(asked) {
            Ok(found) => found,
            Err(outcome) => return outcome,
        };
        let text = match self.edited.get(&key) {
            Some(text) => text.clone(),
            None => {
                use std::io::Read;
                let mut bytes = Vec::new();
                let read = std::fs::File::open(&path)
                    .and_then(|file| file.take(MAX_FILE_BYTES).read_to_end(&mut bytes));
                if let Err(error) = read {
                    return Outcome::error(
                        format!("{key} couldn't be read: {error}"),
                        format!("Couldn't read {key}"),
                    );
                }
                if bytes.iter().take(8192).any(|&byte| byte == 0) {
                    return Outcome::error(
                        format!("{key} isn't a text file."),
                        format!("Not text: {key}"),
                    );
                }
                String::from_utf8_lossy(&bytes).replace("\r\n", "\n")
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
        }
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
