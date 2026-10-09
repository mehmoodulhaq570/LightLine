//! Tells the UI thread when an open file changes on disk or a folder the
//! Explorer shows gains or loses entries. Each watched folder has a thread
//! blocked in `ReadDirectoryChangesW`, so nothing runs until Windows reports
//! a change; the UI is woken once per burst and reads the events with `poll`.

use std::collections::{HashMap, HashSet};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::null;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ACTION_ADDED, FILE_ACTION_REMOVED, FILE_ACTION_RENAMED_NEW_NAME,
    FILE_ACTION_RENAMED_OLD_NAME, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
    FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_NOTIFY_INFORMATION,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadDirectoryChangesW,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects,
};

/// Events emitted by the file watcher to the UI thread.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WatchEvent {
    /// A watched file was written, replaced or deleted.
    FileChanged(PathBuf),
    /// A watched folder gained, lost or renamed an entry.
    DirectoryChanged(PathBuf),
}

type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct State {
    // Lowercase path -> the path as the caller gave it: Windows names are
    // case-insensitive and change notifications use the case on disk.
    files: HashMap<String, PathBuf>,
    // Folders whose entries the caller lists.
    listed: HashSet<PathBuf>,
    pending: Vec<WatchEvent>,
}

impl State {
    fn push(&mut self, event: WatchEvent) {
        if !self.pending.contains(&event) {
            self.pending.push(event);
        }
    }
}

fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

/// A handle to the watcher. Dropping it stops every watch thread.
pub struct FileWatcher {
    state: Arc<Mutex<State>>,
    wake: Wake,
    watches: Mutex<HashMap<PathBuf, DirectoryWatch>>,
}

impl FileWatcher {
    /// `wake` is called (from a watch thread) when events become available
    /// to `poll` after none were waiting.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            state: Arc::default(),
            wake: Arc::new(wake),
            watches: Mutex::default(),
        }
    }

    /// Reports entries added to, removed from or renamed in `path`.
    pub fn watch_directory(&self, path: PathBuf) {
        self.state.lock().unwrap().listed.insert(path.clone());
        self.ensure_watch(path);
    }

    /// Stops listing every folder (the workspace was closed or replaced).
    pub fn unwatch_directories(&self) {
        self.state.lock().unwrap().listed.clear();
        self.drop_unneeded_watches();
    }

    /// Reports changes to `path`, such as an edit made outside LightLine.
    pub fn watch_file(&self, path: PathBuf) {
        let Some(parent) = path.parent().map(Path::to_path_buf) else {
            return;
        };
        self.state.lock().unwrap().files.insert(key(&path), path);
        self.ensure_watch(parent);
    }

    /// Stops reporting changes to `path` (e.g. when its tab is closed).
    pub fn unwatch_file(&self, path: PathBuf) {
        self.state.lock().unwrap().files.remove(&key(&path));
        self.drop_unneeded_watches();
    }

    /// The events since the last call, oldest first. Non-blocking.
    pub fn poll(&self) -> Vec<WatchEvent> {
        std::mem::take(&mut self.state.lock().unwrap().pending)
    }

    fn ensure_watch(&self, directory: PathBuf) {
        let mut watches = self.watches.lock().unwrap();
        if watches.contains_key(&directory) {
            return;
        }
        if let Some(watch) =
            DirectoryWatch::start(directory.clone(), self.state.clone(), self.wake.clone())
        {
            watches.insert(directory, watch);
        }
    }

    // Stops the threads of folders that are neither listed nor hold a
    // watched file.
    fn drop_unneeded_watches(&self) {
        let needed: HashSet<PathBuf> = {
            let state = self.state.lock().unwrap();
            state
                .files
                .values()
                .filter_map(|path| path.parent().map(Path::to_path_buf))
                .chain(state.listed.iter().cloned())
                .collect()
        };
        let stopped: Vec<DirectoryWatch> = {
            let mut watches = self.watches.lock().unwrap();
            let gone: Vec<PathBuf> = watches
                .keys()
                .filter(|directory| !needed.contains(*directory))
                .cloned()
                .collect();
            gone.iter()
                .filter_map(|directory| watches.remove(directory))
                .collect()
        };
        drop(stopped);
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        // Signal every thread first, then wait for them together.
        let watches = std::mem::take(&mut *self.watches.lock().unwrap());
        for watch in watches.values() {
            watch.stop.signal();
        }
        drop(watches);
    }
}

struct OwnedHandle(HANDLE);
// The kernel handles here are owned and only closed once.
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}

impl OwnedHandle {
    fn signal(&self) {
        unsafe { SetEvent(self.0) };
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct DirectoryWatch {
    stop: Arc<OwnedHandle>,
    thread: Option<JoinHandle<()>>,
}

impl DirectoryWatch {
    fn start(directory: PathBuf, state: Arc<Mutex<State>>, wake: Wake) -> Option<Self> {
        let stop = unsafe { CreateEventW(null(), 1, 0, null()) };
        if stop.is_null() {
            return None;
        }
        let stop = Arc::new(OwnedHandle(stop));
        let thread_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("file-watcher".into())
            .spawn(move || watch_directory(&directory, &state, &wake, &thread_stop))
            .ok()?;
        Some(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for DirectoryWatch {
    fn drop(&mut self) {
        self.stop.signal();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

// Waits for changes in `directory` until `stop` is signalled or the folder
// can no longer be read (deleted, or its drive removed).
fn watch_directory(directory: &Path, state: &Mutex<State>, wake: &Wake, stop: &OwnedHandle) {
    let handle = unsafe {
        CreateFileW(
            wide(directory).as_ptr(),
            FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return;
    }
    let handle = OwnedHandle(handle);
    let ready = unsafe { CreateEventW(null(), 1, 0, null()) };
    if ready.is_null() {
        return;
    }
    let ready = OwnedHandle(ready);
    // u32s keep the records DWORD-aligned, as ReadDirectoryChangesW requires.
    let mut buffer = vec![0u32; 16 * 1024];
    let filter = FILE_NOTIFY_CHANGE_FILE_NAME
        | FILE_NOTIFY_CHANGE_DIR_NAME
        | FILE_NOTIFY_CHANGE_LAST_WRITE
        | FILE_NOTIFY_CHANGE_SIZE;
    loop {
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        overlapped.hEvent = ready.0;
        let started = unsafe {
            ReadDirectoryChangesW(
                handle.0,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 4) as u32,
                0,
                filter,
                std::ptr::null_mut(),
                &mut overlapped,
                None,
            )
        };
        if started == 0 {
            return;
        }
        let handles = [ready.0, stop.0];
        let woke = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
        let mut bytes = 0;
        if woke != WAIT_OBJECT_0 {
            // Stopping: the buffer and `overlapped` must outlive the read.
            unsafe {
                CancelIoEx(handle.0, &overlapped);
                GetOverlappedResult(handle.0, &overlapped, &mut bytes, 1);
            }
            return;
        }
        if unsafe { GetOverlappedResult(handle.0, &overlapped, &mut bytes, 0) } == 0 {
            return;
        }
        let changes = if bytes == 0 {
            // More changes than the buffer holds: treat everything as changed.
            None
        } else {
            Some(parse_changes(&buffer, bytes as usize))
        };
        let mut state = state.lock().unwrap();
        let was_empty = state.pending.is_empty();
        record(&mut state, directory, changes);
        if was_empty && !state.pending.is_empty() {
            drop(state);
            wake();
        }
    }
}

// (action, name) for each record in the first `bytes` of `buffer`.
fn parse_changes(buffer: &[u32], bytes: usize) -> Vec<(u32, String)> {
    let base = buffer.as_ptr().cast::<u8>();
    let mut changes = Vec::new();
    let mut offset = 0;
    let header = std::mem::offset_of!(FILE_NOTIFY_INFORMATION, FileName);
    while offset + header <= bytes {
        // Each record starts on a DWORD boundary inside `buffer`.
        let record = unsafe { &*base.add(offset).cast::<FILE_NOTIFY_INFORMATION>() };
        let length = record.FileNameLength as usize / 2;
        if offset + header + length * 2 > bytes {
            break;
        }
        let name =
            unsafe { std::slice::from_raw_parts(base.add(offset + header).cast::<u16>(), length) };
        changes.push((record.Action, String::from_utf16_lossy(name)));
        if record.NextEntryOffset == 0 {
            break;
        }
        offset += record.NextEntryOffset as usize;
    }
    changes
}

// Turns the changes in `directory` into events for the files and folders
// being watched; `None` means the changes were lost and anything may differ.
fn record(state: &mut State, directory: &Path, changes: Option<Vec<(u32, String)>>) {
    let listed = state.listed.contains(directory);
    let Some(changes) = changes else {
        let files: Vec<PathBuf> = state
            .files
            .values()
            .filter(|path| path.parent() == Some(directory))
            .cloned()
            .collect();
        for path in files {
            state.push(WatchEvent::FileChanged(path));
        }
        if listed {
            state.push(WatchEvent::DirectoryChanged(directory.to_path_buf()));
        }
        return;
    };
    for (action, name) in changes {
        let path = directory.join(&name);
        if let Some(file) = state.files.get(&key(&path)).cloned() {
            state.push(WatchEvent::FileChanged(file));
        }
        let entries_changed = matches!(
            action,
            FILE_ACTION_ADDED
                | FILE_ACTION_REMOVED
                | FILE_ACTION_RENAMED_OLD_NAME
                | FILE_ACTION_RENAMED_NEW_NAME
        );
        if listed && entries_changed {
            state.push(WatchEvent::DirectoryChanged(directory.to_path_buf()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lightline-watcher-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // Polls until `want` shows up, collecting everything seen.
    fn wait_for(watcher: &FileWatcher, want: &WatchEvent) -> Vec<WatchEvent> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = Vec::new();
        while Instant::now() < deadline {
            seen.extend(watcher.poll());
            if seen.contains(want) {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        seen
    }

    #[test]
    fn file_edits_and_folder_entries_are_reported_and_wake_once() {
        let dir = temp_dir("events");
        let file = dir.join("Test.txt");
        std::fs::write(&file, "initial").unwrap();
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = wakes.clone();
        let watcher = FileWatcher::start(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        // Registered with different case than on disk.
        let registered = dir.join("test.TXT");
        watcher.watch_file(registered.clone());
        watcher.watch_directory(dir.clone());
        thread::sleep(Duration::from_millis(100));

        std::fs::write(&file, "modified").unwrap();
        let seen = wait_for(&watcher, &WatchEvent::FileChanged(registered.clone()));
        assert!(
            seen.contains(&WatchEvent::FileChanged(registered.clone())),
            "{seen:?}"
        );
        // An edit isn't an entry change.
        assert!(
            !seen.contains(&WatchEvent::DirectoryChanged(dir.clone())),
            "{seen:?}"
        );
        assert!(wakes.load(Ordering::SeqCst) >= 1);

        // Many entries at once: one wake until the events are read. (A write
        // can arrive as two notifications; let the second land first.)
        thread::sleep(Duration::from_millis(200));
        watcher.poll();
        let before = wakes.load(Ordering::SeqCst);
        for index in 0..20 {
            std::fs::write(dir.join(format!("new-{index}.txt")), "x").unwrap();
        }
        thread::sleep(Duration::from_millis(300));
        assert_eq!(wakes.load(Ordering::SeqCst), before + 1);
        let seen = watcher.poll();
        assert_eq!(seen, [WatchEvent::DirectoryChanged(dir.clone())]);

        drop(watcher);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unwatched_paths_stay_quiet_and_drop_stops_threads() {
        let dir = temp_dir("quiet");
        let watcher = FileWatcher::start(|| {});
        watcher.watch_file(dir.join("open.txt"));
        watcher.unwatch_file(dir.join("open.txt"));
        assert!(watcher.watches.lock().unwrap().is_empty());
        watcher.watch_directory(dir.clone());
        watcher.unwatch_directories();
        std::fs::write(dir.join("other.txt"), "x").unwrap();
        thread::sleep(Duration::from_millis(200));
        assert!(watcher.poll().is_empty());
        // A folder that doesn't exist is simply not watched.
        watcher.watch_file(dir.join("missing").join("file.txt"));
        drop(watcher);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
