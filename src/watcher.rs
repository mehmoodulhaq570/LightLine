use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

/// Events emitted by the file watcher to the UI thread.
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// A file that is open in the editor was modified externally.
    FileChanged(PathBuf),
    /// A directory's contents changed (files added/removed).
    DirectoryChanged(PathBuf),
}

/// Commands the UI thread sends to the watcher thread.
enum WatchCommand {
    /// Start watching the given directory tree.
    WatchDirectory(PathBuf),
    /// Add a specific file to the watch set.
    WatchFile(PathBuf),
    /// Remove a file from the watch set.
    UnwatchFile(PathBuf),
    /// Shut down the watcher thread.
    Shutdown,
}

/// A handle to the background file watcher. Drop this to stop watching.
pub struct FileWatcher {
    commands: Sender<WatchCommand>,
    events: Receiver<WatchEvent>,
    alive: Arc<AtomicBool>,
}

impl FileWatcher {
    /// Start the watcher on a background thread. The returned handle can be
    /// used to send commands and poll for events.
    pub fn start() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (evt_tx, evt_rx) = mpsc::channel();
        let alive = Arc::new(AtomicBool::new(true));
        let thread_alive = alive.clone();

        thread::Builder::new()
            .name("file-watcher".into())
            .spawn(move || {
                watcher_thread(cmd_rx, evt_tx, thread_alive);
            })
            .ok();

        Self {
            commands: cmd_tx,
            events: evt_rx,
            alive,
        }
    }

    /// Begin watching a workspace directory for changes.
    pub fn watch_directory(&self, path: PathBuf) {
        let _ = self.commands.send(WatchCommand::WatchDirectory(path));
    }

    /// Track an open file for external modification detection.
    pub fn watch_file(&self, path: PathBuf) {
        let _ = self.commands.send(WatchCommand::WatchFile(path));
    }

    /// Stop tracking a file (e.g. when the tab is closed).
    pub fn unwatch_file(&self, path: PathBuf) {
        let _ = self.commands.send(WatchCommand::UnwatchFile(path));
    }

    /// Poll for any pending watch events. Non-blocking.
    pub fn poll(&self) -> Vec<WatchEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            events.push(event);
        }
        events
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = self.commands.send(WatchCommand::Shutdown);
    }
}

/// The background watcher thread. Uses polling (stat-based) rather than
/// `ReadDirectoryChangesW` for simplicity and reliability across network
/// drives and edge cases. Checks every 2 seconds.
fn watcher_thread(
    commands: Receiver<WatchCommand>,
    events: Sender<WatchEvent>,
    alive: Arc<AtomicBool>,
) {
    let mut watched_files: HashSet<PathBuf> = HashSet::new();
    let mut watched_dirs: HashSet<PathBuf> = HashSet::new();
    let mut file_stamps: std::collections::HashMap<PathBuf, std::time::SystemTime> =
        std::collections::HashMap::new();
    let mut dir_stamps: std::collections::HashMap<PathBuf, std::time::SystemTime> =
        std::collections::HashMap::new();
    let poll_interval = Duration::from_secs(2);

    while alive.load(Ordering::Relaxed) {
        // Process any pending commands.
        loop {
            match commands.try_recv() {
                Ok(WatchCommand::WatchDirectory(path)) => {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        dir_stamps.insert(
                            path.clone(),
                            meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                        );
                    }
                    watched_dirs.insert(path);
                }
                Ok(WatchCommand::WatchFile(path)) => {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        file_stamps.insert(
                            path.clone(),
                            meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                        );
                    }
                    watched_files.insert(path);
                }
                Ok(WatchCommand::UnwatchFile(path)) => {
                    watched_files.remove(&path);
                    file_stamps.remove(&path);
                }
                Ok(WatchCommand::Shutdown) => return,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }

        // Check watched files for modifications.
        let files: Vec<PathBuf> = watched_files.iter().cloned().collect();
        for path in files {
            if let Ok(meta) = std::fs::metadata(&path) {
                let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                let previous = file_stamps.get(&path).copied();
                if previous.is_some_and(|prev| modified > prev) {
                    file_stamps.insert(path.clone(), modified);
                    let _ = events.send(WatchEvent::FileChanged(path));
                } else if previous.is_none() {
                    file_stamps.insert(path, modified);
                }
            }
        }

        // Check watched directories for changes.
        let dirs: Vec<PathBuf> = watched_dirs.iter().cloned().collect();
        for dir in dirs {
            if let Ok(meta) = std::fs::metadata(&dir) {
                let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                let previous = dir_stamps.get(&dir).copied();
                if previous.is_some_and(|prev| modified > prev) {
                    dir_stamps.insert(dir.clone(), modified);
                    let _ = events.send(WatchEvent::DirectoryChanged(dir));
                } else if previous.is_none() {
                    dir_stamps.insert(dir, modified);
                }
            }
        }

        // Sleep for the polling interval, waking early if a command arrives.
        match commands.recv_timeout(poll_interval) {
            Ok(WatchCommand::Shutdown) => return,
            Ok(WatchCommand::WatchDirectory(path)) => {
                if let Ok(meta) = std::fs::metadata(&path) {
                    dir_stamps.insert(
                        path.clone(),
                        meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                    );
                }
                watched_dirs.insert(path);
            }
            Ok(WatchCommand::WatchFile(path)) => {
                if let Ok(meta) = std::fs::metadata(&path) {
                    file_stamps.insert(
                        path.clone(),
                        meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                    );
                }
                watched_files.insert(path);
            }
            Ok(WatchCommand::UnwatchFile(path)) => {
                watched_files.remove(&path);
                file_stamps.remove(&path);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watcher_detects_file_modification() {
        let dir =
            std::env::temp_dir().join(format!("lightline-watcher-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("test.txt");
        std::fs::write(&file, "initial").unwrap();

        let watcher = FileWatcher::start();
        watcher.watch_file(file.clone());

        // Wait for the initial stamp to be recorded.
        thread::sleep(Duration::from_millis(200));

        // Modify the file after a delay so the timestamp changes.
        thread::sleep(Duration::from_secs(1));
        std::fs::write(&file, "modified").unwrap();

        // Wait for the poller to detect the change.
        thread::sleep(Duration::from_secs(3));

        let events = watcher.poll();
        let changed = events
            .iter()
            .any(|e| matches!(e, WatchEvent::FileChanged(p) if p == &file));
        assert!(changed, "Expected FileChanged event for {:?}", file);

        drop(watcher);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn watcher_stops_on_drop() {
        let watcher = FileWatcher::start();
        watcher.watch_file(PathBuf::from("nonexistent.txt"));
        drop(watcher);
        // No panic or hang = success.
    }
}
