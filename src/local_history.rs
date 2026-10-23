//! Local History: a copy of a file each time it's saved, kept under
//! `%APPDATA%\LightLine\history` so an earlier version can be compared with
//! the file or brought back, even one Git never saw.
//!
//! Each file has a folder named by a hash of its path, holding `source.txt`
//! (the path itself) and one `<milliseconds since 1970>.ver` per version.
//! Saving copies the file on another thread, so Ctrl+S doesn't wait for it;
//! only the first save of a file with no history yet copies what's on disk
//! first (the version from before any of LightLine's saves), on the spot.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Larger files aren't kept: a copy per save would grow too quickly.
pub const MAX_BYTES: u64 = 1024 * 1024;
/// Versions kept per file; the oldest go first.
pub const MAX_VERSIONS: usize = 50;

// Off until the app turns it on (`localHistory` in settings), so tests that
// save documents don't write into the real history.
static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// One kept version of a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    /// The copy, inside the history folder.
    pub file: PathBuf,
    /// When it was saved, in milliseconds since 1970.
    pub saved_ms: u64,
    pub len: u64,
}

pub fn history_dir() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("APPDATA")?)
            .join("LightLine")
            .join("history"),
    )
}

/// The kept versions of `path`, newest first.
pub fn versions(path: &Path) -> Vec<Version> {
    history_dir().map_or_else(Vec::new, |root| versions_in(&root, path))
}

/// Called just before `path` is written: if it has no history yet, keeps the
/// file as it is on disk, dated when it was last changed.
pub fn before_save(path: &Path) {
    if !enabled() {
        return;
    }
    if let Some(root) = history_dir() {
        let _ = keep_original(&root, path);
    }
}

/// Called once `path` has been written: keeps a copy, on another thread.
pub fn after_save(path: &Path) {
    if !enabled() {
        return;
    }
    let Some(root) = history_dir() else {
        return;
    };
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let _ = remember(&root, &path, now_ms());
    });
}

// FNV-1a: a stable folder name (std's hasher may change between Rust
// versions, which would orphan the history).
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

// Windows paths ignore case, and `\\?\C:\a` is the same file as `C:\a`.
fn key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_lowercase()
}

fn folder(root: &Path, path: &Path) -> PathBuf {
    root.join(format!("{:016x}", fnv1a(&key(path))))
}

fn now_ms() -> u64 {
    ms_since_epoch(SystemTime::now())
}

fn ms_since_epoch(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

fn versions_in(root: &Path, path: &Path) -> Vec<Version> {
    let Ok(entries) = fs::read_dir(folder(root, path)) else {
        return Vec::new();
    };
    let mut versions: Vec<Version> = entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.path();
            if file.extension()? != "ver" {
                return None;
            }
            let saved_ms = file.file_stem()?.to_str()?.parse().ok()?;
            Some(Version {
                len: entry.metadata().ok()?.len(),
                file,
                saved_ms,
            })
        })
        .collect();
    versions.sort_by_key(|version| std::cmp::Reverse(version.saved_ms));
    versions
}

fn is_keepable(root: &Path, path: &Path) -> Option<fs::Metadata> {
    // The history's own files are never kept.
    if key(path).starts_with(&key(root)) {
        return None;
    }
    let metadata = fs::metadata(path).ok()?;
    (metadata.is_file() && metadata.len() <= MAX_BYTES).then_some(metadata)
}

// Runs on the UI thread during Ctrl+S, so once a file has a history this is
// a single check that its folder exists.
fn keep_original(root: &Path, path: &Path) -> io::Result<()> {
    if folder(root, path).exists() {
        return Ok(());
    }
    let Some(metadata) = is_keepable(root, path) else {
        return Ok(());
    };
    let changed = metadata.modified().map_or(0, ms_since_epoch);
    write_version(root, path, &fs::read(path)?, changed)
}

fn remember(root: &Path, path: &Path, saved_ms: u64) -> io::Result<()> {
    if is_keepable(root, path).is_none() {
        return Ok(());
    }
    let bytes = fs::read(path)?;
    // Saving without a change (or a formatter that changed nothing) adds
    // nothing new.
    if let Some(newest) = versions_in(root, path).first()
        && newest.len == bytes.len() as u64
        && fs::read(&newest.file).is_ok_and(|kept| kept == bytes)
    {
        return Ok(());
    }
    write_version(root, path, &bytes, saved_ms)?;
    for old in versions_in(root, path).iter().skip(MAX_VERSIONS) {
        let _ = fs::remove_file(&old.file);
    }
    Ok(())
}

fn write_version(root: &Path, path: &Path, bytes: &[u8], saved_ms: u64) -> io::Result<()> {
    let folder = folder(root, path);
    fs::create_dir_all(&folder)?;
    let source = folder.join("source.txt");
    if !source.exists() {
        fs::write(&source, path.to_string_lossy().as_bytes())?;
    }
    // Two saves in the same millisecond each get their own name.
    let mut stamp = saved_ms;
    loop {
        let file = folder.join(format!("{stamp}.ver"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
        {
            Ok(mut out) => return io::Write::write_all(&mut out, bytes),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => stamp += 1,
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lightline-history-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn keeps_each_saved_version_newest_first() {
        let dir = scratch("versions");
        let root = dir.join("history");
        let file = dir.join("main.rs");
        fs::write(&file, "one").unwrap();
        remember(&root, &file, 1_000).unwrap();
        fs::write(&file, "two").unwrap();
        remember(&root, &file, 2_000).unwrap();
        let versions = versions_in(&root, &file);
        assert_eq!(
            versions.iter().map(|v| v.saved_ms).collect::<Vec<_>>(),
            [2_000, 1_000]
        );
        assert_eq!(fs::read_to_string(&versions[1].file).unwrap(), "one");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unchanged_save_adds_nothing() {
        let dir = scratch("unchanged");
        let root = dir.join("history");
        let file = dir.join("a.txt");
        fs::write(&file, "same").unwrap();
        remember(&root, &file, 1_000).unwrap();
        remember(&root, &file, 2_000).unwrap();
        assert_eq!(versions_in(&root, &file).len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_original_is_kept_only_before_the_first_save() {
        let dir = scratch("original");
        let root = dir.join("history");
        let file = dir.join("a.txt");
        fs::write(&file, "original").unwrap();
        keep_original(&root, &file).unwrap();
        fs::write(&file, "edited").unwrap();
        keep_original(&root, &file).unwrap();
        let versions = versions_in(&root, &file);
        assert_eq!(versions.len(), 1);
        assert_eq!(fs::read_to_string(&versions[0].file).unwrap(), "original");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_newest_versions_are_kept() {
        let dir = scratch("prune");
        let root = dir.join("history");
        let file = dir.join("a.txt");
        for n in 0..MAX_VERSIONS as u64 + 5 {
            fs::write(&file, n.to_string()).unwrap();
            remember(&root, &file, 1_000 + n).unwrap();
        }
        let versions = versions_in(&root, &file);
        assert_eq!(versions.len(), MAX_VERSIONS);
        assert_eq!(versions.last().unwrap().saved_ms, 1_005);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn large_files_and_the_history_itself_are_skipped() {
        let dir = scratch("skip");
        let root = dir.join("history");
        let big = dir.join("big.bin");
        fs::write(&big, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        remember(&root, &big, 1_000).unwrap();
        assert!(versions_in(&root, &big).is_empty());
        fs::create_dir_all(&root).unwrap();
        let inside = root.join("note.txt");
        fs::write(&inside, "x").unwrap();
        remember(&root, &inside, 1_000).unwrap();
        assert!(versions_in(&root, &inside).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn paths_differing_only_in_case_or_prefix_share_a_history() {
        let root = Path::new(r"C:\h");
        assert_eq!(
            folder(root, Path::new(r"\\?\D:\Code\Main.rs")),
            folder(root, Path::new("d:/code/main.rs"))
        );
    }
}
