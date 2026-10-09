use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

fn background_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    command
}

// Bounds on the Quick Open / project search file walk. They exist to stop a
// runaway walk, not to trim ordinary projects: at 3,000 files and 10 levels,
// files in medium-sized repositories were silently missing from both.
pub const MAX_FILES: usize = 200_000;
const MAX_DEPTH: usize = 64;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_RESULTS: usize = 200;

fn recent_path() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("APPDATA")?)
            .join("LightLine")
            .join("recent-workspaces.txt"),
    )
}

/// Where locally-installed extensions live, e.g.
/// `extensions_dir().join("material-icon-theme")` for a Zed icon-theme
/// extension a user has placed there. No installer/registry writes here yet —
/// this is only ever read from.
pub fn extensions_dir() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("APPDATA")?)
            .join("LightLine")
            .join("extensions"),
    )
}

pub fn recent_workspaces() -> Vec<PathBuf> {
    let Some(path) = recent_path() else {
        return Vec::new();
    };
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .take(5)
        .collect()
}

pub fn remember_workspace(root: &Path) {
    let Some(path) = recent_path() else { return };
    let mut recent = recent_workspaces();
    recent.retain(|other| other != root);
    recent.insert(0, root.to_path_buf());
    recent.truncate(5);
    if let Some(parent) = path.parent()
        && fs::create_dir_all(parent).is_ok()
    {
        let _ = fs::write(
            path,
            recent
                .iter()
                .map(|path| path.to_string_lossy())
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
}

// A snapshot of the last editing session, stored as JSON beside the recent
// workspace list, so reopening LightLine restores the workspace, the open
// tabs, and each tab's cursor/scroll position rather than starting blank.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionView {
    pub cursor: (usize, usize),
    pub anchor: Option<(usize, usize)>,
    pub first_line: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionTab {
    pub path: Option<PathBuf>,
    // Shared with the tab that produced it, so an unchanged buffer isn't
    // copied again for every snapshot.
    pub recovery: Option<std::sync::Arc<str>>,
    pub stamp: Option<(u64, u32, u64)>,
    pub views: [SessionView; 2],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    pub root: Option<PathBuf>,
    pub active: usize,
    pub tabs: Vec<SessionTab>,
}

fn session_path() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("APPDATA")?)
            .join("LightLine")
            .join("session.json"),
    )
}

fn session_view_from(value: &Value) -> SessionView {
    let pair = |value: &Value| {
        value.as_array().and_then(|items| {
            Some((
                items.first()?.as_u64()? as usize,
                items.get(1)?.as_u64()? as usize,
            ))
        })
    };
    SessionView {
        cursor: value.get("cursor").and_then(pair).unwrap_or((0, 0)),
        anchor: value.get("anchor").and_then(pair),
        first_line: value.get("first").and_then(Value::as_u64).unwrap_or(0) as usize,
    }
}

fn session_view_to_json(view: &SessionView) -> Value {
    serde_json::json!({
        "cursor": [view.cursor.0, view.cursor.1],
        "anchor": view.anchor.map(|(line, byte)| serde_json::json!([line, byte])),
        "first": view.first_line,
    })
}

pub fn load_session() -> Session {
    let text = session_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .unwrap_or_default();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Session::default();
    };
    session_from_value(&value)
}

fn session_from_value(value: &Value) -> Session {
    let root = value
        .get("root")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|path| path.is_dir());
    let active = value.get("active").and_then(Value::as_u64).unwrap_or(0) as usize;
    let tabs = value
        .get("tabs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let path = item.get("path").and_then(Value::as_str).map(PathBuf::from);
                    let recovery = item
                        .get("recovery")
                        .and_then(Value::as_str)
                        .map(std::sync::Arc::from);
                    if recovery.is_none() && !path.as_ref().is_some_and(|p| p.is_file()) {
                        return None;
                    }
                    let views = item.get("views").and_then(Value::as_array);
                    let default = SessionView {
                        cursor: (0, 0),
                        anchor: None,
                        first_line: 0,
                    };
                    let views = match views {
                        Some(views) => [
                            views
                                .first()
                                .map(session_view_from)
                                .unwrap_or_else(|| default.clone()),
                            views
                                .get(1)
                                .map(session_view_from)
                                .unwrap_or_else(|| default.clone()),
                        ],
                        None => [default.clone(), default],
                    };
                    Some(SessionTab {
                        path,
                        recovery,
                        stamp: item
                            .get("stamp")
                            .and_then(|v| serde_json::from_value(v.clone()).ok()),
                        views,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Session { root, active, tabs }
}

type SessionCompletion = std::sync::mpsc::Sender<Result<(), String>>;
#[derive(Default)]
struct SessionMailbox {
    pending: Option<Session>,
    completions: Vec<SessionCompletion>,
    error: Option<String>,
}
type SessionWriter = std::sync::Arc<(std::sync::Mutex<SessionMailbox>, std::sync::Condvar)>;

fn session_writer() -> &'static SessionWriter {
    static WRITER: std::sync::OnceLock<SessionWriter> = std::sync::OnceLock::new();
    WRITER.get_or_init(|| {
        let state = std::sync::Arc::new((
            std::sync::Mutex::new(SessionMailbox::default()),
            std::sync::Condvar::new(),
        ));
        let worker = state.clone();
        std::thread::spawn(move || {
            loop {
                let (lock, wake) = &*worker;
                let mut mailbox = lock.lock().unwrap();
                while mailbox.pending.is_none() {
                    mailbox = wake.wait(mailbox).unwrap();
                }
                let session = mailbox.pending.take().unwrap();
                let completions = std::mem::take(&mut mailbox.completions);
                drop(mailbox);
                let result = save_session(&session);
                lock.lock().unwrap().error = result.as_ref().err().cloned();
                for done in completions {
                    let _ = done.send(result.clone());
                }
            }
        });
        state
    })
}

pub fn queue_session(session: Session, flush: bool) -> Result<(), String> {
    let (lock, wake) = &**session_writer();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut mailbox = lock.lock().map_err(|e| e.to_string())?;
    mailbox.pending = Some(session);
    if flush {
        mailbox.completions.push(tx);
    }
    drop(mailbox);
    wake.notify_one();
    if flush {
        rx.recv().map_err(|e| e.to_string())?
    } else {
        Ok(())
    }
}

pub fn take_session_error() -> Option<String> {
    session_writer().0.lock().ok()?.error.take()
}

pub fn save_session(session: &Session) -> Result<(), String> {
    let path = session_path().ok_or("Could not determine session recovery directory")?;
    save_session_at(&path, session).map_err(|e| e.to_string())
}

fn session_to_value(session: &Session) -> Value {
    serde_json::json!({
        "root": session.root.as_ref().map(|root| root.to_string_lossy().to_string()),
        "active": session.active,
        "tabs": session.tabs.iter().map(|tab| serde_json::json!({
            "path": tab.path.as_ref().map(|p| p.to_string_lossy().to_string()),
            "recovery": tab.recovery.as_deref(),
            "stamp": tab.stamp,
            "views": tab.views.iter().map(session_view_to_json).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

fn save_session_at(path: &Path, session: &Session) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Session path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(session_to_value(session).to_string().as_bytes())?;
        file.sync_all()?;
        drop(file);
        // Another program (a virus scanner, the search indexer) can hold the
        // old file open for a moment; that passes, so try again shortly.
        let mut attempt = 0;
        loop {
            match crate::document::replace_file(&temporary, path) {
                Err(error) if attempt < 5 && matches!(error.raw_os_error(), Some(5 | 32)) => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => break result,
            }
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub path: PathBuf,
    pub line: usize,
    pub byte: usize,
    pub preview: String,
    pub context: Vec<(usize, String)>,
}

/// One changed file, as reported by `git status --porcelain=v2`.
#[derive(Clone, Debug)]
pub struct Change {
    pub path: PathBuf,
    /// Raw Git XY status code, e.g. `".M"` or `"A."`.
    pub status: String,
    /// Previous path, present for renames and copies.
    pub old_path: Option<PathBuf>,
    /// The index differs from HEAD, so the change is staged.
    pub staged: bool,
    /// The working tree differs from the index (or the file is untracked).
    pub unstaged: bool,
    pub untracked: bool,
    /// A merge or rebase conflict is pending on this path.
    pub unmerged: bool,
}

impl Change {
    /// One-word label for the source-control list.
    pub fn label(&self) -> &'static str {
        if self.unmerged {
            return "Conflicted";
        }
        let mut code = self.status.chars();
        let index = code.next().unwrap_or('.');
        let worktree = code.next().unwrap_or('.');
        // Prefer the staged letter; fall back to the working-tree one.
        let marker = if index == '.' { worktree } else { index };
        match marker {
            'A' => "Added",
            'M' => "Modified",
            'D' => "Deleted",
            'R' => "Renamed",
            'C' => "Copied",
            'T' => "Type change",
            'U' => "Conflicted",
            '?' => "Untracked",
            _ => "Changed",
        }
    }
}

/// Which side of the index a diff should be computed against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffScope {
    /// Working tree against HEAD: everything not yet committed.
    Head,
    /// Index against HEAD: the staged changes only.
    Staged,
    /// Working tree against the index: the unstaged changes only.
    Unstaged,
}

/// Everything one `git status` call tells the source-control panel.
#[derive(Clone, Debug, Default)]
pub struct RepoState {
    /// Repository root, which can be above the opened workspace folder.
    pub root: PathBuf,
    /// Current branch, or `None` when HEAD is detached or unborn.
    pub branch: Option<String>,
    /// Short commit hash, used as the label when HEAD is detached.
    pub detached: Option<String>,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub changes: Vec<Change>,
    /// True when at least one path is in a conflict state.
    pub conflicted: bool,
    /// Recent commits, newest first.
    pub history: Vec<CommitEntry>,
    /// The absolute Git directory (`.git`, or a worktree's own directory).
    pub git_dir: Option<PathBuf>,
}

impl RepoState {
    /// What the branch chip shows: branch name, or `@abcd1234` when detached.
    pub fn head_label(&self) -> String {
        if let Some(branch) = &self.branch {
            return branch.clone();
        }
        if let Some(oid) = &self.detached {
            return format!("@{oid}");
        }
        String::new()
    }

    pub fn staged(&self) -> impl Iterator<Item = &Change> {
        self.changes.iter().filter(|change| change.staged)
    }

    /// Files with work-tree changes. A partially staged file appears in both
    /// lists, which is what the two review sections mean.
    pub fn unstaged(&self) -> impl Iterator<Item = &Change> {
        self.changes.iter().filter(|change| change.unstaged)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffHunk {
    pub buffer_start: usize,
    pub buffer_len: usize,
    pub head_start: usize,
    pub head_len: usize,
    pub head_lines: Vec<String>,
    pub buffer_lines: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GutterDiff {
    pub added: HashSet<usize>,
    pub modified: HashSet<usize>,
    pub deleted: HashSet<usize>,
    pub hunks: Vec<DiffHunk>,
}

impl GutterDiff {
    pub fn hunk_at_line(&self, line: usize) -> Option<&DiffHunk> {
        self.hunks.iter().find(|hunk| {
            if hunk.buffer_len == 0 {
                line == hunk.buffer_start
                    || (hunk.buffer_start > 0 && line == hunk.buffer_start - 1)
            } else {
                line >= hunk.buffer_start && line < hunk.buffer_start + hunk.buffer_len
            }
        })
    }
}

#[derive(Clone, Debug)]
pub struct DiffRow {
    pub changed: bool,
    pub before_number: Option<usize>,
    pub before: String,
    pub after_number: Option<usize>,
    pub after: String,
    pub deleted_at: Option<usize>,
}

pub fn workspace_files(root: &Path) -> Vec<PathBuf> {
    workspace_files_inner(root, None)
}

fn workspace_files_inner(root: &Path, cancel: Option<&AtomicBool>) -> Vec<PathBuf> {
    let mut pending = vec![(root.to_path_buf(), 0)];
    let mut files = Vec::new();
    while let Some((directory, depth)) = pending.pop() {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            break;
        }
        if depth > MAX_DEPTH || files.len() >= MAX_FILES {
            break;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                break;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if matches!(
                name.as_ref(),
                ".git" | "target" | "node_modules" | ".venv" | "__pycache__"
            ) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if depth < MAX_DEPTH {
                    pending.push((entry.path(), depth + 1));
                }
            } else if kind.is_file() {
                files.push(entry.path());
                if files.len() >= MAX_FILES {
                    break;
                }
            }
        }
    }
    files.sort();
    files
}

pub fn search_workspace(root: &Path, query: &str) -> Vec<SearchHit> {
    search_workspace_with_cancel(root, query, &AtomicBool::new(false))
}

/// The first `MAX_RESULTS` lines containing `query`, in file order. Files
/// are read on several threads; each takes the next file in order, and none
/// starts a new one once enough lines are found, so the files read are
/// always a leading run of the list.
pub fn search_workspace_with_cancel(
    root: &Path,
    query: &str,
    cancel: &AtomicBool,
) -> Vec<SearchHit> {
    if query.is_empty() {
        return Vec::new();
    }
    let files = workspace_files_inner(root, Some(cancel));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let found = std::sync::atomic::AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |count| count.get().min(8));
    let mut per_file: Vec<(usize, Vec<SearchHit>)> = std::thread::scope(|scope| {
        let searches: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut hits = Vec::new();
                    while !cancel.load(Ordering::Relaxed)
                        && found.load(Ordering::Relaxed) < MAX_RESULTS
                    {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = files.get(index) else {
                            break;
                        };
                        let file_hits = search_file(path, query, cancel);
                        if !file_hits.is_empty() {
                            found.fetch_add(file_hits.len(), Ordering::Relaxed);
                            hits.push((index, file_hits));
                        }
                    }
                    hits
                })
            })
            .collect();
        searches
            .into_iter()
            .flat_map(|search| search.join().unwrap_or_default())
            .collect()
    });
    per_file.sort_by_key(|(index, _)| *index);
    per_file
        .into_iter()
        .flat_map(|(_, hits)| hits)
        .take(MAX_RESULTS)
        .collect()
}

fn search_file(path: &Path, query: &str, cancel: &AtomicBool) -> Vec<SearchHit> {
    if !fs::metadata(path).is_ok_and(|metadata| metadata.len() <= MAX_FILE_BYTES) {
        return Vec::new();
    }
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    // Most files have no match; one scan of the whole text rules them out.
    if !content.contains(query) {
        return Vec::new();
    }
    let lines: Vec<&str> = content.lines().collect();
    let mut hits = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        if line_index % 128 == 0 && cancel.load(Ordering::Relaxed) {
            break;
        }
        if let Some(byte) = line.find(query) {
            hits.push(SearchHit {
                path: path.to_path_buf(),
                line: line_index,
                byte,
                preview: line.trim().chars().take(110).collect(),
                context: lines[line_index.saturating_sub(2)..(line_index + 3).min(lines.len())]
                    .iter()
                    .enumerate()
                    .map(|(offset, text)| {
                        (
                            line_index.saturating_sub(2) + offset + 1,
                            text.chars().take(130).collect(),
                        )
                    })
                    .collect(),
            });
            if hits.len() >= MAX_RESULTS {
                break;
            }
        }
    }
    hits
}
pub fn run_tests_stream(
    root: &Path,
    lines: Sender<String>,
    cancel: &AtomicBool,
    pid: &AtomicU32,
) -> Result<(), String> {
    if !root.join("Cargo.toml").is_file() {
        return Err("This workspace has no Cargo.toml. Run is available for Rust projects.".into());
    }
    let mut child = background_command("cargo")
        .args(["test", "--offline"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start cargo: {error}"))?;
    let tree = crate::jobs::ProcessTree::new(&child);
    pid.store(child.id(), Ordering::Relaxed);
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out_tx = lines.clone();
    let out_reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = out_tx.send(format!("{line}\n"));
        }
    });
    let err_tx = lines.clone();
    let err_reader = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = err_tx.send(format!("{line}\n"));
        }
    });
    let status = wait_for_child(&mut child, &tree, "cargo", cancel)?;
    let _ = out_reader.join();
    let _ = err_reader.join();
    pid.store(0, Ordering::Relaxed);
    if cancel.load(Ordering::Relaxed) {
        Err("Stopped by user".into())
    } else if status.success() {
        Ok(())
    } else {
        Err(format!("Test command exited with {status}"))
    }
}

pub fn run_python_file_stream(
    interpreter: &Path,
    file: &Path,
    root: &Path,
    output: Sender<String>,
    input: Receiver<String>,
    cancel: &AtomicBool,
    pid: &AtomicU32,
) -> Result<(), String> {
    if !interpreter.is_file() {
        return Err(format!(
            "Python interpreter does not exist: {}",
            interpreter.display()
        ));
    }
    if !file.is_file() {
        return Err(format!("Python file does not exist: {}", file.display()));
    }
    let mut child = background_command(&interpreter.to_string_lossy())
        .arg("-u")
        .arg(file)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start Python: {error}"))?;
    let tree = crate::jobs::ProcessTree::new(&child);
    pid.store(child.id(), Ordering::Relaxed);
    let mut stdin = child.stdin.take().unwrap();
    let stdin_writer = std::thread::spawn(move || {
        for text in input {
            if stdin.write_all(text.as_bytes()).is_err() || stdin.flush().is_err() {
                break;
            }
        }
    });
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out_reader = stream_reader(stdout, output.clone());
    let err_reader = stream_reader(stderr, output);
    let status = wait_for_child(&mut child, &tree, "Python", cancel)?;
    let _ = out_reader.join();
    let _ = err_reader.join();
    drop(stdin_writer);
    pid.store(0, Ordering::Relaxed);
    if cancel.load(Ordering::Relaxed) {
        Err("Python run stopped by user".into())
    } else if status.success() {
        Ok(())
    } else {
        Err(format!("Python exited with {status}"))
    }
}

fn stream_reader(
    mut reader: impl Read + Send + 'static,
    output: Sender<String>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buffer = [0; 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let text = String::from_utf8_lossy(&buffer[..count]).into_owned();
                    let _ = output.send(text);
                }
                Err(_) => break,
            }
        }
    })
}

fn wait_for_child(
    child: &mut std::process::Child,
    tree: &crate::jobs::ProcessTree,
    name: &str,
    cancel: &AtomicBool,
) -> Result<std::process::ExitStatus, String> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            // Everything the run started, not just the program itself.
            tree.kill();
            return child
                .wait()
                .map_err(|error| format!("Could not stop {name}: {error}"));
        }
        match child
            .try_wait()
            .map_err(|error| format!("Could not wait for {name}: {error}"))?
        {
            Some(status) => return Ok(status),
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

pub fn python_project_root(file: &Path, workspace_root: Option<&Path>) -> PathBuf {
    file.ancestors()
        .skip(1)
        .take(10)
        .find(|folder| {
            folder.join("pyproject.toml").is_file()
                || folder.join("setup.py").is_file()
                || folder.join("setup.cfg").is_file()
                || folder.join("requirements.txt").is_file()
                || folder.join(".venv").is_dir()
                || folder.join(".git").exists()
        })
        .or(workspace_root)
        .or_else(|| file.parent())
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

pub fn detect_python_interpreter(root: Option<&Path>) -> Option<PathBuf> {
    if let Some(root) = root {
        for folder in root.ancestors().take(10) {
            for candidate in [
                folder.join(".venv").join("Scripts").join("python.exe"),
                folder.join(".venv").join("bin").join("python"),
            ] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    let paths = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&paths) {
        let display = directory.to_string_lossy();
        if display.contains("WindowsApps") {
            continue;
        }
        for name in ["python.exe", "python3.exe"] {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Returns true when `name` (e.g. "cargo", "g++") resolves to a file on PATH,
/// so callers can surface a clear "not installed" message instead of letting
/// the child process spawn fail (or, worse, run and crash midway through).
pub fn command_available(name: &str) -> bool {
    resolve_command(name).is_some()
}

/// The full path `name` runs from, searched on PATH the way a Windows shell
/// does: `.exe`, then the `.cmd`/`.bat` launchers npm installs for tools such
/// as `prettier` and `npx`. `Command::new("prettier")` alone never finds
/// those, because it only tries `.exe`.
pub fn resolve_command(name: &str) -> Option<PathBuf> {
    resolve_command_in(&std::env::var_os("PATH")?, name)
}

fn resolve_command_in(path_var: &std::ffi::OsStr, name: &str) -> Option<PathBuf> {
    let candidates: Vec<String> = if !cfg!(windows) || Path::new(name).extension().is_some() {
        vec![name.to_string()]
    } else {
        // An extensionless file next to a .cmd (npm's shell script for Git
        // Bash) isn't runnable on Windows, so it is never a candidate.
        [".exe", ".cmd", ".bat", ".com"]
            .iter()
            .map(|extension| format!("{name}{extension}"))
            .collect()
    };
    std::env::split_paths(path_var).find_map(|directory| {
        candidates
            .iter()
            .map(|candidate| directory.join(candidate))
            .find(|candidate| candidate.is_file())
    })
}

/// Finds a C or C++ compiler on PATH, preferring the one that matches the
/// source language. Returns `None` when no usable toolchain is installed, so
/// the caller can tell the user instead of spawning a command that can't run.
// MSVC's cl.exe is deliberately not offered here: it takes /Fe: instead of
// -o and only works from inside a Developer Command Prompt environment, so
// detecting it on PATH doesn't mean the -o command line below would work.
pub fn detect_c_compiler(is_cpp: bool) -> Option<&'static str> {
    let ordered = if is_cpp {
        ["g++", "clang++"]
    } else {
        ["gcc", "clang"]
    };
    ordered.into_iter().find(|name| command_available(name))
}

/// Runs a syntax-only compile of a single C/C++ file (no object file or
/// binary produced) and turns gcc/clang's diagnostics into the same
/// `Diagnostic` type LSP servers report, so the existing squiggly-underline
/// rendering can show C/C++ errors too without a persistent language server.
pub fn c_syntax_diagnostics(
    file: &Path,
    compiler: &str,
    is_cpp: bool,
) -> Vec<crate::lsp::Diagnostic> {
    let mut command = background_command(compiler);
    command.arg("-fsyntax-only").arg("-Wall");
    if is_cpp {
        command.arg("-std=c++17");
    }
    command.arg(file);
    let Ok(output) = command.output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter_map(parse_gcc_diagnostic)
        .collect()
}

// Parses a gcc/clang diagnostic line: "<path>:<line>:<col>: <severity>: <message>".
// The path is split off from the right (not the left) because a Windows path
// carries its own drive-letter colon (e.g. "C:\src\main.cpp:5:10: error: ...").
fn parse_gcc_diagnostic(line: &str) -> Option<crate::lsp::Diagnostic> {
    use crate::lsp::{Diagnostic, Position, Range};
    let (marker, severity) = if line.contains(" error: ") {
        (" error: ", 1u8)
    } else if line.contains(" warning: ") {
        (" warning: ", 2u8)
    } else {
        return None;
    };
    let index = line.find(marker)?;
    let prefix = &line[..index];
    let message = &line[index + marker.len()..];
    let mut parts = prefix.rsplitn(3, ':');
    let column: u32 = parts.next()?.trim().parse().ok()?;
    let line_number: u32 = parts.next()?.trim().parse().ok()?;
    if line_number == 0 {
        return None;
    }
    let character = column.saturating_sub(1);
    let line_index = line_number - 1;
    Some(Diagnostic {
        range: Range {
            start: Position {
                line: line_index,
                character,
            },
            end: Position {
                line: line_index,
                character: character + 1,
            },
        },
        severity,
        message: message.trim().to_string(),
    })
}

/// Run Git in `root` and return its standard output.
pub fn git_output(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = git_command(root, args)
        .output()
        .map_err(|error| format!("Could not start Git: {error}"))?;
    if !output.status.success() {
        return Err(git_failure(&output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// GIT_OPTIONAL_LOCKS=0 stops read-only commands such as `git status` from
// rewriting .git/index to refresh its stat cache. LightLine watches that file
// to notice commits made in a terminal, so its own reads must not touch it.
fn git_command(root: &Path, args: &[&str]) -> Command {
    let mut command = background_command("git");
    command
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(root)
        .args(["--no-pager", "-c", "core.quotePath=false"])
        .args(args);
    command
}

/// Run a Git command whose path arguments follow the options.
fn git_paths(root: &Path, args: &[&str], paths: &[PathBuf]) -> Result<(), String> {
    let mut command = git_command(root, args);
    for path in paths {
        command.arg(path);
    }
    let output = command
        .output()
        .map_err(|error| format!("Could not start Git: {error}"))?;
    if !output.status.success() {
        return Err(git_failure(&output));
    }
    Ok(())
}

/// Apply a unified diff patch to the Git index or working tree.
pub fn git_apply_patch(root: &Path, patch: &str, cached: bool) -> Result<(), String> {
    let mut args = vec!["apply", "--unidiff-zero", "--whitespace=nowarn"];
    if cached {
        args.push("--cached");
    }
    args.push("-");
    let mut command = git_command(root, &args);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start Git: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(patch.as_bytes())
            .map_err(|e| format!("Failed to send patch to Git: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Git apply failed: {e}"))?;
    if !output.status.success() {
        return Err(git_failure(&output));
    }
    Ok(())
}

/// Synthesize a unidiff-zero patch representation for a single hunk.
pub fn hunk_unified_diff(path: &Path, hunk: &DiffHunk) -> String {
    let path_str = path.to_string_lossy().replace('\\', "/");
    let mut patch = format!("--- a/{path_str}\n+++ b/{path_str}\n");
    let (head_start, head_len) = (hunk.head_start, hunk.head_len);
    let (buf_start, buf_len) = (hunk.buffer_start, hunk.buffer_len);
    let old_range = if head_len == 0 {
        format!("{head_start},0")
    } else {
        format!("{},{head_len}", head_start + 1)
    };
    let new_range = if buf_len == 0 {
        format!("{buf_start},0")
    } else {
        format!("{},{buf_len}", buf_start + 1)
    };
    patch.push_str(&format!("@@ -{old_range} +{new_range} @@\n"));
    for line in &hunk.head_lines {
        patch.push('-');
        patch.push_str(line);
        patch.push('\n');
    }
    for line in &hunk.buffer_lines {
        patch.push('+');
        patch.push_str(line);
        patch.push('\n');
    }
    patch
}

/// Stages an individual diff hunk into Git's index using a synthesized unified diff.
pub fn stage_hunk(root: &Path, relative_path: &Path, hunk: &DiffHunk) -> Result<(), String> {
    let patch = hunk_unified_diff(relative_path, hunk);
    git_apply_patch(root, &patch, true)
}

/// Unstages an individual diff hunk from Git's index by reverse-applying its unified diff.
pub fn unstage_hunk(root: &Path, relative_path: &Path, hunk: &DiffHunk) -> Result<(), String> {
    let patch = hunk_unified_diff(relative_path, hunk);
    let args = [
        "apply",
        "--reverse",
        "--cached",
        "--unidiff-zero",
        "--whitespace=nowarn",
        "-",
    ];
    let mut command = git_command(root, &args);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start Git: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(patch.as_bytes())
            .map_err(|e| format!("Failed to send patch to Git: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Git apply reverse failed: {e}"))?;
    if !output.status.success() {
        return Err(git_failure(&output));
    }
    Ok(())
}

fn git_failure(output: &std::process::Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return format!("Git failed ({}).", output.status);
    }
    if trimmed.contains("not a git repository") {
        return "This workspace is not inside a Git repository.".into();
    }
    // A rejected commit or a conflicted merge spreads the reason over several
    // lines; keep enough of them to act on without flooding the status bar.
    trimmed.lines().take(4).collect::<Vec<_>>().join(" / ")
}

/// Top level of the repository containing `path`. Opening a subfolder of a
/// repository still needs this, because Git reports paths from the root.
pub fn repo_root(path: &Path) -> Option<PathBuf> {
    let text = git_output(path, &["rev-parse", "--show-toplevel"]).ok()?;
    let line = text.lines().next()?;
    (!line.is_empty()).then(|| PathBuf::from(line))
}

/// Branch, sync state, changed files and recent commits. Paths in `changes`
/// are relative to `state.root`, which can sit above `root` when a subfolder
/// was opened as the workspace.
pub fn repo_state(root: &Path) -> Result<RepoState, String> {
    let text = git_output(
        root,
        &[
            "status",
            "--branch",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
        ],
    )?;
    let mut state = parse_porcelain_v2(&text);
    state.root = repo_root(root).unwrap_or_else(|| root.to_path_buf());
    state.history = log(root, 30).unwrap_or_default();
    state.git_dir = git_output(root, &["rev-parse", "--absolute-git-dir"])
        .ok()
        .and_then(|text| text.lines().next().map(PathBuf::from));
    Ok(state)
}

/// Parse `git status --branch --porcelain=v2 -z`. Records are NUL separated,
/// and a rename carries its original path in the record that follows it.
pub fn parse_porcelain_v2(text: &str) -> RepoState {
    fn field(record: &str, index: usize) -> Option<&str> {
        record.splitn(index + 1, ' ').nth(index).map(str::trim)
    }
    fn change(path: &str, status: &str) -> Change {
        let mut chars = status.chars();
        let index_code = chars.next().unwrap_or('.');
        let work_code = chars.next().unwrap_or('.');
        Change {
            path: PathBuf::from(path),
            status: status.to_owned(),
            old_path: None,
            staged: index_code != '.' && index_code != '?',
            unstaged: (work_code != '.' && work_code != '?') || status.starts_with('?'),
            untracked: status.starts_with('?'),
            unmerged: false,
        }
    }

    let records: Vec<&str> = text
        .split('\0')
        .filter(|record| !record.is_empty())
        .collect();
    let mut state = RepoState::default();
    let mut oid = String::new();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        index += 1;
        if let Some(header) = record.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" => oid = value.to_owned(),
                "branch.head" => {
                    if value != "(detached)" && value != "(initial)" && !value.is_empty() {
                        state.branch = Some(value.to_owned());
                    }
                }
                "branch.upstream" => state.upstream = Some(value.to_owned()),
                "branch.ab" => {
                    for token in value.split(' ') {
                        if let Some(count) = token.strip_prefix('+') {
                            state.ahead = count.parse().unwrap_or(0);
                        } else if let Some(count) = token.strip_prefix('-') {
                            state.behind = count.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let kind = record.chars().next().unwrap_or_default();
        if kind == '?' {
            if let Some(path) = record.get(2..) {
                state.changes.push(change(path, "??"));
            }
            continue;
        }
        // Each entry lists its fixed fields first, so the path is whatever is
        // left after them and may itself contain spaces.
        let path_index = match kind {
            '1' => 8,
            '2' => 9,
            'u' => 12,
            _ => continue,
        };
        let Some(status) = record.get(2..4) else {
            continue;
        };
        let Some(path) = field(record, path_index) else {
            continue;
        };
        let mut item = change(path, status);
        item.unmerged = kind == 'u';
        if kind == '2' {
            item.old_path = records.get(index).map(|path| PathBuf::from(*path));
            index += 1;
        }
        state.changes.push(item);
    }
    if state.branch.is_none() && oid.len() >= 8 && !oid.starts_with('(') {
        state.detached = Some(oid[..8].to_owned());
    }
    state.conflicted = state.changes.iter().any(|change| change.unmerged);
    state
}

/// Add files to the index, or everything when `paths` is empty.
pub fn stage(root: &Path, paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return git_output(root, &["add", "-A"]).map(|_| ());
    }
    git_paths(root, &["add", "--"], paths)
}

/// Drop files from the index without touching the working tree.
pub fn unstage(root: &Path, paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return git_output(root, &["reset"]).map(|_| ());
    }
    git_paths(root, &["restore", "--staged", "--"], paths)
}

/// Throw away working-tree changes. Untracked files are deleted rather than
/// restored, so callers must confirm first: neither kind is recoverable.
pub fn discard(root: &Path, paths: &[PathBuf], untracked: &[PathBuf]) -> Result<(), String> {
    if !paths.is_empty() {
        git_paths(root, &["restore", "--"], paths)?;
    }
    if !untracked.is_empty() {
        git_paths(root, &["clean", "-f", "--"], untracked)?;
    }
    Ok(())
}

pub fn commit(root: &Path, message: &str) -> Result<(), String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("Type a commit message first.".into());
    }
    git_output(root, &["commit", &format!("--message={message}")]).map(|_| ())
}

/// Local branch names with the checked-out one flagged.
pub fn branches(root: &Path) -> Result<Vec<(String, bool)>, String> {
    let text = git_output(root, &["branch", "--format=%(refname:short)\u{1f}%(HEAD)"])?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let (name, head) = line.split_once('\u{1f}')?;
            (!name.is_empty()).then(|| (name.to_owned(), head == "*"))
        })
        .collect())
}

pub fn checkout(root: &Path, branch: &str, create: bool) -> Result<(), String> {
    if create {
        git_output(root, &["switch", "-c", branch]).map(|_| ())
    } else {
        git_output(root, &["switch", "--", branch]).map(|_| ())
    }
}

/// Sync commands run in the terminal panel instead of a hidden child process,
/// so Git's own output and its credential prompt stay visible to the user.
pub fn remote_command(action: RemoteAction) -> &'static str {
    match action {
        RemoteAction::Push => "git push",
        RemoteAction::Pull => "git pull --ff-only",
        RemoteAction::Fetch => "git fetch --all",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteAction {
    Push,
    Pull,
    Fetch,
}

/// Short history, newest first.
pub fn log(root: &Path, limit: usize) -> Result<Vec<CommitEntry>, String> {
    let text = git_output(
        root,
        &[
            "log",
            &format!("--max-count={limit}"),
            "--date=format:%b %d",
            "--pretty=format:%h\u{1f}%an\u{1f}%ad\u{1f}%s",
        ],
    )?;
    Ok(parse_log(&text))
}

/// Parse `%h<US>%an<US>%ad<US>%s` records, one per line.
pub fn parse_log(text: &str) -> Vec<CommitEntry> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split('\u{1f}');
            let (oid, author, date, subject) = (
                fields.next()?,
                fields.next()?,
                fields.next()?,
                fields.next()?,
            );
            (!oid.is_empty()).then(|| CommitEntry {
                oid: oid.to_owned(),
                author: author.to_owned(),
                date: date.to_owned(),
                subject: subject.to_owned(),
            })
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct CommitEntry {
    pub oid: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

pub fn git_diff(root: &Path, path: &Path, scope: DiffScope) -> Result<Vec<DiffRow>, String> {
    let tracked = git_command(root, &["ls-files", "--error-unmatch", "--"])
        .arg(path)
        .output()
        .map_err(|error| format!("Could not start Git: {error}"))?
        .status
        .success();
    if !tracked {
        let text = fs::read_to_string(root.join(path))
            .map_err(|error| format!("Cannot preview this new file: {error}"))?;
        return Ok(text
            .lines()
            .take(2_000)
            .enumerate()
            .map(|(index, line)| DiffRow {
                changed: true,
                before_number: None,
                before: String::new(),
                after_number: Some(index + 1),
                after: line.to_owned(),
                deleted_at: None,
            })
            .collect());
    }
    let mut output = match scope {
        DiffScope::Head => git_command(
            root,
            &["diff", "--no-ext-diff", "--unified=2", "HEAD", "--"],
        ),
        DiffScope::Staged => git_command(
            root,
            &["diff", "--no-ext-diff", "--cached", "--unified=2", "--"],
        ),
        DiffScope::Unstaged => git_command(root, &["diff", "--no-ext-diff", "--unified=2", "--"]),
    };
    let output = output
        .arg(path)
        .output()
        .map_err(|error| format!("Could not start Git: {error}"))?;
    if !output.status.success() {
        return Err("Could not read the Git diff.".into());
    }
    Ok(parse_diff(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_diff(text: &str) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    let (mut old_number, mut new_number) = (0, 0);
    let (mut removed, mut added): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let flush = |rows: &mut Vec<DiffRow>,
                 removed: &mut Vec<String>,
                 added: &mut Vec<String>,
                 old_number: &mut usize,
                 new_number: &mut usize| {
        let count = removed.len().max(added.len());
        for index in 0..count {
            let is_del = removed.get(index).is_some() && added.is_empty();
            rows.push(DiffRow {
                changed: true,
                before_number: removed.get(index).map(|_| *old_number + index),
                before: removed.get(index).cloned().unwrap_or_default(),
                after_number: added.get(index).map(|_| *new_number + index),
                after: added.get(index).cloned().unwrap_or_default(),
                deleted_at: if is_del { Some(*new_number) } else { None },
            });
        }
        *old_number += removed.len();
        *new_number += added.len();
        removed.clear();
        added.clear();
    };
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("@@ -") {
            flush(
                &mut rows,
                &mut removed,
                &mut added,
                &mut old_number,
                &mut new_number,
            );
            let Some((old, rest)) = header.split_once(" +") else {
                continue;
            };
            let Some((new, _)) = rest.split_once(" @@") else {
                continue;
            };
            old_number = old.split(',').next().unwrap_or("0").parse().unwrap_or(0);
            new_number = new.split(',').next().unwrap_or("0").parse().unwrap_or(0);
        } else if let Some(value) = line.strip_prefix('-') {
            if old_number > 0 {
                removed.push(value.to_owned());
            }
        } else if let Some(value) = line.strip_prefix('+')
            && new_number > 0
        {
            added.push(value.to_owned());
        } else if let Some(value) = line.strip_prefix(' ')
            && old_number > 0
            && new_number > 0
        {
            flush(
                &mut rows,
                &mut removed,
                &mut added,
                &mut old_number,
                &mut new_number,
            );
            rows.push(DiffRow {
                changed: false,
                before_number: Some(old_number),
                before: value.to_owned(),
                after_number: Some(new_number),
                after: value.to_owned(),
                deleted_at: None,
            });
            old_number += 1;
            new_number += 1;
        }
        if rows.len() > 2_000 {
            break;
        }
    }
    flush(
        &mut rows,
        &mut removed,
        &mut added,
        &mut old_number,
        &mut new_number,
    );
    rows.truncate(2_000);
    rows
}

/// Reads the HEAD content of `path` from git in `root`.
pub fn git_head_text(root: &Path, path: &Path) -> Result<String, String> {
    let spec = format!("HEAD:{}", path.to_string_lossy().replace('\\', "/"));
    let output = git_command(root, &["show", &spec])
        .output()
        .map_err(|error| format!("Could not start Git: {error}"))?;
    if !output.status.success() {
        return Err("File not found in HEAD".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Upper bound on Myers diff work per call (edit distance x trimmed lines),
/// so a live recompute after a keystroke stays within a few milliseconds
/// even on a large file. Past it, the changed region is marked as a whole.
const GUTTER_DIFF_BUDGET: usize = 2_000_000;

#[derive(Clone, Copy, PartialEq)]
enum LineOp {
    Equal,
    Delete,
    Insert,
}

// Myers' O((N+M)D) shortest edit script. Returns None when the edit
// distance exceeds `max_d`.
fn myers_line_ops(a: &[&str], b: &[&str], max_d: usize) -> Option<Vec<LineOp>> {
    let n = a.len() as isize;
    let m = b.len() as isize;
    let max = (n + m) as usize;
    let offset = max as isize + 1;
    let mut v = vec![0isize; 2 * max + 3];
    // trace[d] holds V for diagonals -d..=d after step d.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut end_d = None;
    'search: for d in 0..=max.min(max_d) as isize {
        let mut k = -d;
        while k <= d {
            let index = (offset + k) as usize;
            let mut x = if k == -d || (k != d && v[index - 1] < v[index + 1]) {
                v[index + 1]
            } else {
                v[index - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[index] = x;
            if x >= n && y >= m {
                trace.push(v[(offset - d) as usize..=(offset + d) as usize].to_vec());
                end_d = Some(d);
                break 'search;
            }
            k += 2;
        }
        trace.push(v[(offset - d) as usize..=(offset + d) as usize].to_vec());
    }
    let end_d = end_d?;

    let mut ops = Vec::with_capacity((n + m) as usize);
    let (mut x, mut y) = (n, m);
    for d in (1..=end_d).rev() {
        let previous = &trace[(d - 1) as usize];
        let at = |k: isize| previous[(k + d - 1) as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && at(k - 1) < at(k + 1)) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = at(prev_k);
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            ops.push(LineOp::Equal);
            x -= 1;
            y -= 1;
        }
        ops.push(if x == prev_x {
            LineOp::Insert
        } else {
            LineOp::Delete
        });
        x = prev_x;
        y = prev_y;
    }
    while x > 0 && y > 0 {
        ops.push(LineOp::Equal);
        x -= 1;
        y -= 1;
    }
    ops.reverse();
    Some(ops)
}

// Marks one changed region: `inserted` buffer lines starting at buffer line
// `at`, replacing `removed` HEAD lines. Pure insertions are added, pure
// removals leave a deletion marker on the following line, and anything
// mixed is modified.
fn mark_gutter_hunk(
    diff: &mut GutterDiff,
    at: usize,
    removed: usize,
    inserted: usize,
    buffer_len: usize,
) {
    if inserted == 0 {
        if removed > 0 && buffer_len > 0 {
            diff.deleted.insert(at.min(buffer_len - 1));
        }
        return;
    }
    let target = if removed == 0 {
        &mut diff.added
    } else {
        &mut diff.modified
    };
    target.extend(at..at + inserted);
}

/// Compares the buffer against its HEAD text for the editor gutter: lines
/// added, lines modified, and lines that follow a deletion. Each changed
/// region is classified on its own, so unchanged lines between two edits are
/// never marked.
pub fn compute_gutter_diff(head_text: &str, buf_lines: &[String]) -> GutterDiff {
    match gutter_work(head_text, buf_lines) {
        GutterWork::Done(diff) => diff,
        GutterWork::Job(job) => job.run(),
    }
}

/// A gutter diff split in two: the lines the buffer shares with its HEAD text
/// at the start and the end are compared in place, without copying, which is
/// cheap enough for the UI thread; what differs in between is copied into a
/// job that can run on another thread. While typing that's a line or two:
/// copying every line of the file for each diff was the costly part.
pub enum GutterWork {
    Done(GutterDiff),
    Job(GutterJob),
}

pub struct GutterJob {
    prefix: usize,
    buffer_len: usize,
    old: Vec<String>,
    new: Vec<String>,
}

impl GutterJob {
    /// How many lines differ, HEAD's and the buffer's together.
    pub fn lines(&self) -> usize {
        self.old.len() + self.new.len()
    }

    pub fn run(self) -> GutterDiff {
        let old: Vec<&str> = self.old.iter().map(String::as_str).collect();
        let new: Vec<&str> = self.new.iter().map(String::as_str).collect();
        diff_between(self.prefix, &old, &new, self.buffer_len)
    }
}

pub fn gutter_work(head_text: &str, buf_lines: &[String]) -> GutterWork {
    // Split exactly the way Document does ("a\n" is two lines, "a" and ""),
    // so a file's final newline never reads as an added line.
    let head_lines: Vec<&str> = head_text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let (n, m) = (head_lines.len(), buf_lines.len());

    let mut prefix = 0;
    while prefix < n && prefix < m && head_lines[prefix] == buf_lines[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < n - prefix
        && suffix < m - prefix
        && head_lines[n - 1 - suffix] == buf_lines[m - 1 - suffix]
    {
        suffix += 1;
    }
    let old = &head_lines[prefix..n - suffix];
    let new = &buf_lines[prefix..m - suffix];
    if old.is_empty() && new.is_empty() {
        return GutterWork::Done(GutterDiff::default());
    }
    GutterWork::Job(GutterJob {
        prefix,
        buffer_len: m,
        old: old.iter().map(|line| (*line).to_owned()).collect(),
        new: new.to_vec(),
    })
}

// Classifies the lines between the shared start (`prefix` lines) and end.
fn diff_between(prefix: usize, old: &[&str], new: &[&str], m: usize) -> GutterDiff {
    let mut diff = GutterDiff::default();
    let max_d = (GUTTER_DIFF_BUDGET / (old.len() + new.len()).max(1)).max(8);
    let Some(ops) = myers_line_ops(old, new, max_d) else {
        mark_gutter_hunk(&mut diff, prefix, old.len(), new.len(), m);
        diff.hunks.push(DiffHunk {
            buffer_start: prefix,
            buffer_len: new.len(),
            head_start: prefix,
            head_len: old.len(),
            head_lines: old.iter().map(|s| s.to_string()).collect(),
            buffer_lines: new.iter().map(|s| s.to_string()).collect(),
        });
        return diff;
    };

    let mut line = prefix;
    let mut head_line = prefix;
    let (mut removed, mut inserted, mut hunk_start) = (0, 0, prefix);
    let mut hunk_head_start = prefix;
    for op in ops {
        match op {
            LineOp::Equal => {
                if removed + inserted > 0 {
                    mark_gutter_hunk(&mut diff, hunk_start, removed, inserted, m);
                    let head_slice =
                        &old[(hunk_head_start - prefix)..(hunk_head_start - prefix + removed)];
                    let buf_slice = &new[(hunk_start - prefix)..(hunk_start - prefix + inserted)];
                    diff.hunks.push(DiffHunk {
                        buffer_start: hunk_start,
                        buffer_len: inserted,
                        head_start: hunk_head_start,
                        head_len: removed,
                        head_lines: head_slice.iter().map(|s| s.to_string()).collect(),
                        buffer_lines: buf_slice.iter().map(|s| s.to_string()).collect(),
                    });
                    removed = 0;
                    inserted = 0;
                }
                line += 1;
                head_line += 1;
                hunk_start = line;
                hunk_head_start = head_line;
            }
            LineOp::Delete => {
                removed += 1;
                head_line += 1;
            }
            LineOp::Insert => {
                inserted += 1;
                line += 1;
            }
        }
    }
    if removed + inserted > 0 {
        mark_gutter_hunk(&mut diff, hunk_start, removed, inserted, m);
        let head_slice = &old[(hunk_head_start - prefix)..(hunk_head_start - prefix + removed)];
        let buf_slice = &new[(hunk_start - prefix)..(hunk_start - prefix + inserted)];
        diff.hunks.push(DiffHunk {
            buffer_start: hunk_start,
            buffer_len: inserted,
            head_start: hunk_head_start,
            head_len: removed,
            head_lines: head_slice.iter().map(|s| s.to_string()).collect(),
            buffer_lines: buf_slice.iter().map(|s| s.to_string()).collect(),
        });
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_snapshot_roundtrips_untitled_missing_and_existing_files() {
        let root = temp_dir("session-recovery");
        let file = root.join("existing.txt");
        fs::write(&file, "original").unwrap();
        let view = SessionView {
            cursor: (0, 3),
            anchor: None,
            first_line: 0,
        };
        let session = Session {
            root: Some(root.clone()),
            active: 2,
            tabs: vec![
                SessionTab {
                    path: Some(file.clone()),
                    recovery: None,
                    stamp: None,
                    views: [view.clone(), view.clone()],
                },
                SessionTab {
                    path: Some(root.join("deleted.txt")),
                    recovery: Some("recover deleted".into()),
                    stamp: Some((1, 2, 3)),
                    views: [view.clone(), view.clone()],
                },
                SessionTab {
                    path: None,
                    recovery: Some("untitled 👩‍💻\ntext".into()),
                    stamp: None,
                    views: [view.clone(), view],
                },
            ],
        };
        let path = root.join("session.json");
        save_session_at(&path, &session).unwrap();
        assert_eq!(
            session_from_value(&serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap()),
            session
        );
        let mut newer = session.clone();
        newer.tabs[2].recovery = Some("newer text".into());
        save_session_at(&path, &newer).unwrap();
        assert_eq!(
            session_from_value(&serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap()),
            newer
        );
        assert_eq!(fs::read_to_string(file).unwrap(), "original");
        assert!(
            !path
                .with_extension(format!("{}.tmp", std::process::id()))
                .exists()
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn old_session_format_remains_readable() {
        let root = temp_dir("old-session");
        let file = root.join("existing.txt");
        fs::write(&file, "original").unwrap();
        let old = serde_json::json!({ "tabs": [{"path": file}], "active": 0 });
        let session = session_from_value(&old);
        assert_eq!(session.tabs.len(), 1);
        assert!(session.tabs[0].recovery.is_none());
        assert!(session.tabs[0].stamp.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    use std::sync::Arc;
    use std::sync::mpsc;

    fn temp_dir(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "lightline-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn find_python() -> Option<PathBuf> {
        for name in ["python", "python3"] {
            let output = Command::new("where").arg(name).output().ok()?;
            if output.status.success() {
                let first = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .map(PathBuf::from);
                if let Some(path) = first
                    && path.is_file()
                {
                    return Some(path);
                }
            }
        }
        None
    }

    fn drain(rx: Receiver<String>) -> String {
        rx.into_iter().collect()
    }

    #[cfg(windows)]
    #[test]
    fn resolve_command_finds_npm_cmd_launchers_but_not_their_shell_scripts() {
        let root = temp_dir("resolve-command");
        let (npm, tools) = (root.join("npm"), root.join("tools"));
        fs::create_dir_all(&npm).unwrap();
        fs::create_dir_all(&tools).unwrap();
        // What `npm install -g prettier` leaves: a shell script and a .cmd.
        fs::write(npm.join("prettier"), "").unwrap();
        fs::write(npm.join("prettier.cmd"), "").unwrap();
        fs::write(tools.join("git.exe"), "").unwrap();
        fs::write(tools.join("only-script"), "").unwrap();
        let path_var = std::env::join_paths([&npm, &tools]).unwrap();
        assert_eq!(
            resolve_command_in(&path_var, "prettier"),
            Some(npm.join("prettier.cmd"))
        );
        assert_eq!(
            resolve_command_in(&path_var, "git"),
            Some(tools.join("git.exe"))
        );
        assert_eq!(resolve_command_in(&path_var, "only-script"), None);
        assert_eq!(resolve_command_in(&path_var, "missing"), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn python_project_root_prefers_the_nearest_project_marker() {
        let root = temp_dir("python-root");
        fs::create_dir_all(root.join("pkg/sub")).unwrap();
        fs::write(root.join("pyproject.toml"), "").unwrap();
        let file = root.join("pkg/sub/script.py");
        fs::write(&file, "print('hi')\n").unwrap();
        assert_eq!(python_project_root(&file, None), root);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn python_project_root_detects_a_virtual_environment_marker() {
        let root = temp_dir("python-root-venv");
        fs::create_dir_all(root.join(".venv/Scripts")).unwrap();
        fs::create_dir_all(root.join("app")).unwrap();
        let file = root.join("app/script.py");
        fs::write(&file, "print('hi')\n").unwrap();
        assert_eq!(python_project_root(&file, None), root);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detect_python_interpreter_prefers_a_project_virtual_environment() {
        let root = temp_dir("python-detect-venv");
        fs::create_dir_all(root.join(".venv").join("Scripts")).unwrap();
        let venv_python = root.join(".venv").join("Scripts").join("python.exe");
        fs::write(&venv_python, "").unwrap();
        fs::create_dir_all(root.join("app")).unwrap();
        assert_eq!(
            detect_python_interpreter(Some(&root.join("app"))),
            Some(venv_python)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_python_file_stream_handles_paths_containing_spaces() {
        let Some(python) = find_python() else {
            return;
        };
        let root = temp_dir("python run with spaces");
        let file = root.join("my script.py");
        fs::write(&file, "print('spaces ok')\n").unwrap();
        let (output_tx, output_rx) = mpsc::channel();
        let (_input_tx, input_rx) = mpsc::channel();
        let result = run_python_file_stream(
            &python,
            &file,
            &root,
            output_tx,
            input_rx,
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        );
        assert!(result.is_ok(), "unexpected error: {result:?}");
        assert!(drain(output_rx).contains("spaces ok"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn python_project_root_falls_back_to_workspace_then_file_parent() {
        let root = temp_dir("python-root-fallback");
        let file = root.join("loose_script.py");
        fs::write(&file, "print('hi')\n").unwrap();
        let workspace = temp_dir("python-workspace");
        let enclosing_marker = file
            .ancestors()
            .skip(1)
            .take(10)
            .find(|dir| dir.join(".git").exists())
            .map(Path::to_owned);
        assert_eq!(
            python_project_root(&file, Some(&workspace)),
            enclosing_marker.clone().unwrap_or(workspace.clone())
        );
        assert_eq!(
            python_project_root(&file, None),
            enclosing_marker.unwrap_or(root.clone())
        );
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&workspace).unwrap();
    }

    #[test]
    fn run_python_file_stream_rejects_a_missing_interpreter_or_file() {
        let (output_tx, output_rx) = mpsc::channel();
        let (_input_tx, input_rx) = mpsc::channel();
        let missing_interpreter = Path::new("Z:/definitely/missing/python.exe");
        let root = temp_dir("python-missing-interpreter");
        let file = root.join("script.py");
        fs::write(&file, "print('hi')\n").unwrap();
        let result = run_python_file_stream(
            missing_interpreter,
            &file,
            &root,
            output_tx,
            input_rx,
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        );
        assert!(result.is_err());
        drop(output_rx);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_python_file_stream_streams_output_and_succeeds() {
        let Some(python) = find_python() else {
            return;
        };
        let root = temp_dir("python-run-success");
        let file = root.join("script.py");
        fs::write(
            &file,
            "print('line one')\nimport sys\nprint('line two', file=sys.stderr)\n",
        )
        .unwrap();
        let (output_tx, output_rx) = mpsc::channel();
        let (_input_tx, input_rx) = mpsc::channel();
        let pid = AtomicU32::new(0);
        let result = run_python_file_stream(
            &python,
            &file,
            &root,
            output_tx,
            input_rx,
            &AtomicBool::new(false),
            &pid,
        );
        assert!(result.is_ok(), "unexpected error: {result:?}");
        assert_eq!(pid.load(Ordering::Relaxed), 0);
        let text = drain(output_rx);
        assert!(text.contains("line one"));
        assert!(text.contains("line two"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_python_file_stream_reports_a_traceback_and_nonzero_exit() {
        let Some(python) = find_python() else {
            return;
        };
        let root = temp_dir("python-run-error");
        let file = root.join("script.py");
        fs::write(&file, "raise ValueError('boom')\n").unwrap();
        let (output_tx, output_rx) = mpsc::channel();
        let (_input_tx, input_rx) = mpsc::channel();
        let result = run_python_file_stream(
            &python,
            &file,
            &root,
            output_tx,
            input_rx,
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        );
        assert!(result.is_err());
        let text = drain(output_rx);
        assert!(text.contains("ValueError"));
        assert!(text.contains("boom"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_python_file_stream_reads_stdin_input() {
        let Some(python) = find_python() else {
            return;
        };
        let root = temp_dir("python-run-input");
        let file = root.join("script.py");
        fs::write(&file, "name = input('name? ')\nprint('hello ' + name)\n").unwrap();
        let (output_tx, output_rx) = mpsc::channel();
        let (input_tx, input_rx) = mpsc::channel();
        input_tx.send("LightLine\n".to_string()).unwrap();
        let result = run_python_file_stream(
            &python,
            &file,
            &root,
            output_tx,
            input_rx,
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        );
        assert!(result.is_ok(), "unexpected error: {result:?}");
        let text = drain(output_rx);
        assert!(text.contains("hello LightLine"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_python_file_stream_can_be_cancelled_while_running() {
        let Some(python) = find_python() else {
            return;
        };
        let root = temp_dir("python-run-cancel");
        let file = root.join("script.py");
        fs::write(
            &file,
            "import time\nfor _ in range(600):\n    time.sleep(0.1)\n",
        )
        .unwrap();
        let (output_tx, output_rx) = mpsc::channel();
        let (_input_tx, input_rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let pid = Arc::new(AtomicU32::new(0));
        let run_cancel = cancel.clone();
        let run_pid = pid.clone();
        let run_root = root.clone();
        let handle = std::thread::spawn(move || {
            run_python_file_stream(
                &python,
                &file,
                &run_root,
                output_tx,
                input_rx,
                &run_cancel,
                &run_pid,
            )
        });
        while pid.load(Ordering::Relaxed) == 0 {
            std::thread::sleep(Duration::from_millis(20));
        }
        cancel.store(true, Ordering::Relaxed);
        let result = handle.join().unwrap();
        assert!(result.is_err());
        assert_eq!(pid.load(Ordering::Relaxed), 0);
        drop(output_rx);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_aligned_change_hunks() {
        let diff = "@@ -4,2 +4,3 @@\n-old one\n-old two\n+new one\n+new two\n+new three\n";
        let rows = parse_diff(diff);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].before_number, Some(4));
        assert_eq!(rows[0].after_number, Some(4));
        assert_eq!(rows[2].before_number, None);
        assert_eq!(rows[2].after_number, Some(6));
    }

    #[test]
    fn keeps_context_separate_from_changed_lines() {
        let rows = parse_diff("@@ -2,3 +2,3 @@\n keep\n-old\n+new\n after\n");
        assert_eq!(rows.len(), 3);
        assert!(!rows[0].changed);
        assert!(rows[1].changed);
        assert_eq!(rows[1].before_number, Some(3));
        assert_eq!(rows[1].after_number, Some(3));
        assert!(!rows[2].changed);
    }

    #[test]
    fn searches_project_files_without_generated_directories() {
        let root = std::env::temp_dir().join(format!(
            "lightline-workflow-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("src/main.rs"), "first\nfind this\n").unwrap();
        fs::write(root.join("target/generated.rs"), "find this\n").unwrap();
        let hits = search_workspace(&root, "find");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 1);
        assert_eq!(hits[0].byte, 0);
        assert!(search_workspace_with_cancel(&root, "find", &AtomicBool::new(true)).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parallel_search_returns_the_first_matches_in_file_order() {
        let root = temp_dir("search-order");
        for index in 0..300 {
            fs::write(root.join(format!("{index:03}.txt")), "a\nhit\nb\nhit\n").unwrap();
        }
        let hits = search_workspace(&root, "hit");
        assert_eq!(hits.len(), MAX_RESULTS);
        // Two per file, from the first files on.
        for (number, hit) in hits.iter().enumerate() {
            assert_eq!(hit.path, root.join(format!("{:03}.txt", number / 2)));
            assert_eq!(hit.line, if number % 2 == 0 { 1 } else { 3 });
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reviews_tracked_and_new_files_in_a_real_repository() {
        if background_command("git").arg("--version").output().is_err() {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "lightline-git-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        assert!(
            background_command("git")
                .args(["init", "--quiet"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("a.txt"), "before\n").unwrap();
        assert!(
            background_command("git")
                .args(["add", "a.txt"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            background_command("git")
                .args([
                    "-c",
                    "user.name=LightLine Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "commit",
                    "--quiet",
                    "-m",
                    "initial"
                ])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        fs::write(root.join("a.txt"), "after\n").unwrap();
        fs::write(root.join("new.txt"), "new file\n").unwrap();
        let state = repo_state(&root).unwrap();
        let changes = &state.changes;
        assert_eq!(state.branch.as_deref(), Some("master"));
        assert!(
            changes
                .iter()
                .any(|change| change.path == Path::new("a.txt") && change.unstaged)
        );
        assert!(
            changes
                .iter()
                .any(|change| change.path == Path::new("new.txt") && change.untracked)
        );
        let tracked = git_diff(&root, Path::new("a.txt"), DiffScope::Head).unwrap();
        assert!(
            tracked
                .iter()
                .any(|row| row.before == "before" && row.after == "after")
        );
        let new = git_diff(&root, Path::new("new.txt"), DiffScope::Head).unwrap();
        assert_eq!(new[0].after, "new file");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_porcelain_v2_branch_and_change_records() {
        let text = "# branch.oid 2a0b1c3d4e5f6a7b8c9d\0\
                    # branch.head main\0\
                    # branch.upstream origin/main\0\
                    # branch.ab +2 -3\0\
                    1 .M N... 100644 100644 100644 aaaa bbbb src/two words.rs\0\
                    2 R. N... 100644 100644 100644 cccc dddd R100 lib/new.rs\0lib/old.rs\0\
                    1 A. N... 000000 100644 100644 0000 eeee added.rs\0\
                    1 M. N... 100644 100644 100644 ffff 0000 staged.rs\0\
                    u UU N... 100644 100644 100644 1111 2222 3333 1 2 3 conflicted.rs\0\
                    ? free.txt\0";
        let state = parse_porcelain_v2(text);
        assert_eq!(state.branch.as_deref(), Some("main"));
        assert_eq!(state.upstream.as_deref(), Some("origin/main"));
        assert_eq!((state.ahead, state.behind), (2, 3));
        assert!(state.conflicted);
        assert_eq!(state.changes.len(), 6);

        // Paths may contain spaces because -z ends records at NUL, not space.
        let spaced = &state.changes[0];
        assert_eq!(spaced.path, Path::new("src/two words.rs"));
        assert!(!spaced.staged && spaced.unstaged);
        assert_eq!(spaced.label(), "Modified");

        let renamed = &state.changes[1];
        assert_eq!(renamed.path, Path::new("lib/new.rs"));
        assert_eq!(renamed.old_path.as_deref(), Some(Path::new("lib/old.rs")));
        assert!(renamed.staged && !renamed.unstaged);
        assert_eq!(renamed.label(), "Renamed");

        assert!(state.changes[2].staged);
        assert_eq!(state.changes[2].label(), "Added");
        assert_eq!(state.changes[3].label(), "Modified");
        assert!(state.changes[3].staged && !state.changes[3].unstaged);
        assert_eq!(state.changes[4].label(), "Conflicted");
        assert!(state.changes[4].unmerged && state.changes[4].staged && state.changes[4].unstaged);
        assert_eq!(state.changes[5].label(), "Untracked");
        // The conflicted path counts as staged because its index differs from
        // HEAD, and as unstaged because the working tree is not resolved.
        assert_eq!(state.staged().count(), 4);
        assert_eq!(state.unstaged().count(), 3);
    }

    #[test]
    fn detached_head_has_no_branch_name() {
        let text = "# branch.oid 0f1e2d3c4b5a6978\0# branch.head (detached)\0";
        let state = parse_porcelain_v2(text);
        assert_eq!(state.branch, None);
        assert_eq!(state.detached.as_deref(), Some("0f1e2d3c"));
        assert_eq!(state.head_label(), "@0f1e2d3c");
    }

    #[test]
    fn unborn_and_partially_staged_records_split_into_both_sections() {
        // `(initial)` is what Git reports before the first commit, when there
        // is no upstream and nothing to compare ahead/behind against.
        let text = "# branch.oid (initial)\0# branch.head main\0\
                    1 MM N... 100644 100644 100644 aaaa bbbb both.rs\0\
                    1 .D N... 100644 100644 000000 cccc dddd gone.rs\0";
        let state = parse_porcelain_v2(text);
        assert_eq!(state.branch.as_deref(), Some("main"));
        assert_eq!(state.head_label(), "main");
        assert_eq!(state.upstream, None);
        assert_eq!((state.ahead, state.behind), (0, 0));
        assert!(!state.conflicted);

        // A file staged and then edited again belongs to both lists, which is
        // what makes the two sections mean what they say.
        let both = &state.changes[0];
        assert!(both.staged && both.unstaged);
        assert_eq!(both.label(), "Modified");
        assert_eq!(state.staged().count(), 1);
        assert_eq!(state.unstaged().count(), 2);

        let deleted = &state.changes[1];
        assert!(!deleted.staged && deleted.unstaged);
        assert_eq!(deleted.label(), "Deleted");
    }

    #[test]
    fn parses_commit_log_records() {
        let rows = parse_log(
            "aaaa\u{1f}Ada\u{1f}Sep 12\u{1f}Add terminal\n\
                              bbbb\u{1f}Ada\u{1f}Sep 13\u{1f}Fix: a bug, then another",
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].oid, "aaaa");
        assert_eq!(rows[0].author, "Ada");
        assert_eq!(rows[1].subject, "Fix: a bug, then another");
        assert!(parse_log("").is_empty());
    }

    fn buffer(text: &str) -> Vec<String> {
        text.split('\n').map(str::to_owned).collect()
    }

    fn sorted(set: &HashSet<usize>) -> Vec<usize> {
        let mut lines: Vec<usize> = set.iter().copied().collect();
        lines.sort_unstable();
        lines
    }

    #[test]
    fn test_compute_gutter_diff_added_modified_deleted() {
        let head = "line1\nline2\nline3\nline4\nline5\n";

        // Identical, including the final newline: no marks at all.
        assert_eq!(
            compute_gutter_diff(head, &buffer(head)),
            GutterDiff::default()
        );

        let added = compute_gutter_diff(
            head,
            &buffer("line1\nline2\nnew line\nline3\nline4\nline5\n"),
        );
        assert_eq!(sorted(&added.added), vec![2]);
        assert!(added.modified.is_empty() && added.deleted.is_empty());

        let modified = compute_gutter_diff(
            head,
            &buffer("line1\nline2 modified\nline3\nline4\nline5\n"),
        );
        assert_eq!(sorted(&modified.modified), vec![1]);
        assert!(modified.added.is_empty() && modified.deleted.is_empty());

        let deleted = compute_gutter_diff(head, &buffer("line1\nline3\nline4\nline5\n"));
        assert_eq!(sorted(&deleted.deleted), vec![1]);
        assert!(deleted.added.is_empty() && deleted.modified.is_empty());
    }

    #[test]
    fn gutter_diff_leaves_unchanged_lines_between_edits_unmarked() {
        let head = "a\nb\nc\nd\ne\n";
        // Two separate one-line edits with equal line counts.
        let diff = compute_gutter_diff(head, &buffer("A\nb\nc\nd\nE\n"));
        assert_eq!(sorted(&diff.modified), vec![0, 4]);
        assert!(diff.added.is_empty() && diff.deleted.is_empty());
    }

    #[test]
    fn gutter_diff_classifies_each_hunk_separately() {
        let head = "a\nb\nc\nd\ne\nf\n";
        // Delete "b" near the top, insert a new line near the bottom.
        let diff = compute_gutter_diff(head, &buffer("a\nc\nd\ne\nnew\nf\n"));
        assert_eq!(sorted(&diff.deleted), vec![1]);
        assert_eq!(sorted(&diff.added), vec![4]);
        assert!(diff.modified.is_empty());
    }

    #[test]
    fn gutter_diff_handles_empty_sides_and_large_rewrites() {
        assert_eq!(
            sorted(&compute_gutter_diff("a\n", &buffer("a\nx\ny\n")).added),
            vec![1, 2]
        );
        assert_eq!(
            sorted(&compute_gutter_diff("x\ny\n", &buffer("")).deleted),
            vec![0]
        );
        let head: String = (0..5000).map(|i| format!("old {i}\n")).collect();
        let new: String = (0..5000).map(|i| format!("new {i}\n")).collect();
        let diff = compute_gutter_diff(&head, &buffer(&new));
        assert_eq!(diff.modified.len(), 5000);
    }

    #[test]
    fn gutter_job_copies_only_the_lines_that_differ() {
        let head: String = (0..2000).map(|i| format!("line {i}\n")).collect();
        let edited = head.replacen("line 1000\n", "line 1000 typed\n", 1);
        let lines = buffer(&edited);
        match gutter_work(&head, &lines) {
            GutterWork::Job(job) => {
                // One line of HEAD against one of the buffer, not 4000.
                assert_eq!(job.lines(), 2);
                let diff = job.run();
                assert_eq!(sorted(&diff.modified), vec![1000]);
                assert_eq!(diff, compute_gutter_diff(&head, &lines));
            }
            GutterWork::Done(_) => panic!("the edited line should differ"),
        }
        assert!(matches!(
            gutter_work(&head, &buffer(&head)),
            GutterWork::Done(diff) if diff == GutterDiff::default()
        ));
    }

    #[test]
    fn hunk_unified_diff_formats_added_modified_deleted() {
        let path = Path::new("src/main.rs");

        // Added hunk
        let added_hunk = DiffHunk {
            buffer_start: 2,
            buffer_len: 1,
            head_start: 2,
            head_len: 0,
            head_lines: vec![],
            buffer_lines: vec!["println!(\"hello\");".into()],
        };
        let patch = hunk_unified_diff(path, &added_hunk);
        assert!(patch.contains("--- a/src/main.rs\n+++ b/src/main.rs\n"));
        assert!(patch.contains("@@ -2,0 +3,1 @@\n+println!(\"hello\");\n"));

        // Modified hunk
        let mod_hunk = DiffHunk {
            buffer_start: 4,
            buffer_len: 1,
            head_start: 4,
            head_len: 1,
            head_lines: vec!["let x = 1;".into()],
            buffer_lines: vec!["let x = 2;".into()],
        };
        let patch = hunk_unified_diff(path, &mod_hunk);
        assert!(patch.contains("@@ -5,1 +5,1 @@\n-let x = 1;\n+let x = 2;\n"));

        // Deleted hunk
        let del_hunk = DiffHunk {
            buffer_start: 3,
            buffer_len: 0,
            head_start: 3,
            head_len: 2,
            head_lines: vec!["old1".into(), "old2".into()],
            buffer_lines: vec![],
        };
        let patch = hunk_unified_diff(path, &del_hunk);
        assert!(patch.contains("@@ -4,2 +3,0 @@\n-old1\n-old2\n"));
    }

    #[test]
    fn gutter_diff_extracts_hunks_with_line_lookup() {
        let head = "line1\nline2\nline3\nline4\nline5\n";
        let diff = compute_gutter_diff(
            head,
            &buffer("line1\nline2 modified\nnew line\nline3\nline5\n"),
        );
        assert!(!diff.hunks.is_empty());
        let hunk = diff.hunk_at_line(1);
        assert!(hunk.is_some());
        assert_eq!(hunk.unwrap().buffer_start, 1);
        assert_eq!(hunk.unwrap().head_lines, vec!["line2"]);
        assert_eq!(
            hunk.unwrap().buffer_lines,
            vec!["line2 modified", "new line"]
        );
    }
}
