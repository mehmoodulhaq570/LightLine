use std::fs;
use serde_json::Value;
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

const MAX_FILES: usize = 3_000;
const MAX_DEPTH: usize = 10;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_RESULTS: usize = 200;

fn recent_path() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("APPDATA")?)
            .join("LightLine")
            .join("recent-workspaces.txt"),
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
    pub path: PathBuf,
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
        cursor: value
            .get("cursor")
            .and_then(pair)
            .unwrap_or((0, 0)),
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
                    let path = item.get("path").and_then(Value::as_str).map(PathBuf::from)?;
                    if !path.is_file() {
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
                            views.first().map(session_view_from).unwrap_or_else(|| default.clone()),
                            views.get(1).map(session_view_from).unwrap_or_else(|| default.clone()),
                        ],
                        None => [default.clone(), default],
                    };
                    Some(SessionTab { path, views })
                })
                .take(64)
                .collect()
        })
        .unwrap_or_default();
    Session {
        root,
        active,
        tabs,
    }
}

pub fn save_session(session: &Session) {
    let Some(path) = session_path() else { return };
    let value = serde_json::json!({
        "root": session.root.as_ref().map(|root| root.to_string_lossy().to_string()),
        "active": session.active,
        "tabs": session
            .tabs
            .iter()
            .map(|tab| serde_json::json!({
                "path": tab.path.to_string_lossy().to_string(),
                "views": tab.views.iter().map(session_view_to_json).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
    });
    if let Some(parent) = path.parent()
        && fs::create_dir_all(parent).is_ok()
    {
        let _ = fs::write(path, value.to_string());
    }
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub path: PathBuf,
    pub line: usize,
    pub byte: usize,
    pub preview: String,
    pub context: Vec<(usize, String)>,
}

#[derive(Clone, Debug)]
pub struct Change {
    pub path: PathBuf,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct DiffRow {
    pub changed: bool,
    pub before_number: Option<usize>,
    pub before: String,
    pub after_number: Option<usize>,
    pub after: String,
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

pub fn search_workspace_with_cancel(
    root: &Path,
    query: &str,
    cancel: &AtomicBool,
) -> Vec<SearchHit> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for path in workspace_files_inner(root, Some(cancel)) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if !fs::metadata(&path).is_ok_and(|metadata| metadata.len() <= MAX_FILE_BYTES) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<&str> = content.lines().collect();
        for (line_index, line) in lines.iter().enumerate() {
            if line_index % 128 == 0 && cancel.load(Ordering::Relaxed) {
                return hits;
            }
            if let Some(byte) = line.find(query) {
                hits.push(SearchHit {
                    path: path.clone(),
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
                    return hits;
                }
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
    let status = wait_for_child(&mut child, "cargo", cancel)?;
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
    let status = wait_for_child(&mut child, "Python", cancel)?;
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

fn stream_reader(mut reader: impl Read + Send + 'static, output: Sender<String>) -> std::thread::JoinHandle<()> {
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
    name: &str,
    cancel: &AtomicBool,
) -> Result<std::process::ExitStatus, String> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            #[cfg(windows)]
            let _ = background_command("taskkill")
                .args(["/T", "/F", "/PID", &child.id().to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = child.kill();
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

pub fn git_changes(root: &Path) -> Result<Vec<Change>, String> {
    let output = background_command("git")
        .args([
            "-c",
            "core.quotePath=false",
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
        ])
        .current_dir(root)
        .output()
        .map_err(|error| format!("Could not start Git: {error}"))?;
    if !output.status.success() {
        return Err("This workspace is not inside a Git repository.".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let name = line.get(3..)?;
            Some(Change {
                path: PathBuf::from(name.rsplit(" -> ").next().unwrap_or(name)),
                status: line.get(..2)?.to_owned(),
            })
        })
        .collect())
}

pub fn git_diff(root: &Path, path: &Path) -> Result<Vec<DiffRow>, String> {
    let tracked = background_command("git")
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(path)
        .current_dir(root)
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
            })
            .collect());
    }
    let output = background_command("git")
        .args(["diff", "--no-ext-diff", "--unified=2", "HEAD", "--"])
        .arg(path)
        .current_dir(root)
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
            rows.push(DiffRow {
                changed: true,
                before_number: removed.get(index).map(|_| *old_number + index),
                before: removed.get(index).cloned().unwrap_or_default(),
                after_number: added.get(index).map(|_| *new_number + index),
                after: added.get(index).cloned().unwrap_or_default(),
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

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(
            python_project_root(&file, Some(&workspace)),
            workspace
        );
        assert_eq!(python_project_root(&file, None), root);
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
        fs::write(
            &file,
            "name = input('name? ')\nprint('hello ' + name)\n",
        )
        .unwrap();
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
        let changes = git_changes(&root).unwrap();
        assert!(
            changes
                .iter()
                .any(|change| change.path == Path::new("a.txt"))
        );
        assert!(
            changes
                .iter()
                .any(|change| change.path == Path::new("new.txt"))
        );
        let tracked = git_diff(&root, Path::new("a.txt")).unwrap();
        assert!(
            tracked
                .iter()
                .any(|row| row.before == "before" && row.after == "after")
        );
        let new = git_diff(&root, Path::new("new.txt")).unwrap();
        assert_eq!(new[0].after, "new file");
        fs::remove_dir_all(root).unwrap();
    }
}
