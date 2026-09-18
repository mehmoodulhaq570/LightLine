use std::fs;
use std::io::{BufRead, BufReader};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
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
            let _ = out_tx.send(line);
        }
    });
    let err_tx = lines.clone();
    let err_reader = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = err_tx.send(line);
        }
    });
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            #[cfg(windows)]
            let _ = background_command("taskkill")
                .args(["/T", "/F", "/PID", &child.id().to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = child.kill();
            break child
                .wait()
                .map_err(|error| format!("Could not stop cargo: {error}"))?;
        }
        match child
            .try_wait()
            .map_err(|error| format!("Could not wait for cargo: {error}"))?
        {
            Some(status) => break status,
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
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
