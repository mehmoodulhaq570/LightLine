//! Downloads an extension's files into the local extensions directory.
//! Shells out to `git clone` rather than adding an HTTP-tarball path or a
//! libgit2 dependency — every extension in the Zed registry is a git
//! submodule pointing at an ordinary repo, and `git` is a reasonable
//! dependency for a developer tool to expect.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

static INSTALL_LOCK: Mutex<()> = Mutex::new(());

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
        || !id.as_bytes()[0].is_ascii_alphanumeric()
        || matches!(
            id,
            "con"
                | "prn"
                | "aux"
                | "nul"
                | "com1"
                | "com2"
                | "com3"
                | "com4"
                | "com5"
                | "com6"
                | "com7"
                | "com8"
                | "com9"
                | "lpt1"
                | "lpt2"
                | "lpt3"
                | "lpt4"
                | "lpt5"
                | "lpt6"
                | "lpt7"
                | "lpt8"
                | "lpt9"
        )
    {
        return Err("Invalid extension ID".into());
    }
    Ok(())
}

fn validate_download(dir: &Path, id: &str) -> Result<(), String> {
    let manifest = super::zed_manifest::Manifest::load(dir).ok_or("Invalid extension.toml")?;
    if manifest.id != id || manifest.name.trim().is_empty() || manifest.version.trim().is_empty() {
        return Err("Extension manifest does not match the requested extension".into());
    }
    if super::zed_manifest::is_icon_theme(dir) {
        crate::icon_theme::IconTheme::load(dir)
            .ok_or("Invalid icon theme files")?
            .validate_assets()?;
    } else if super::zed_manifest::is_color_theme(dir) {
        crate::color_theme::ZedColorTheme::load(dir).ok_or("Invalid color theme files")?;
    } else {
        return Err("Unsupported extension: only icon and color themes can be installed".into());
    }
    Ok(())
}

fn promote(staging: &Path, target: &Path, backup: &Path) -> Result<(), String> {
    let existed = target.exists();
    if existed {
        std::fs::rename(target, backup).map_err(|e| e.to_string())?;
    }
    if let Err(error) = std::fs::rename(staging, target) {
        if existed && let Err(rollback) = std::fs::rename(backup, target) {
            return Err(format!(
                "Update failed: {error}; rollback failed: {rollback}. Previous copy retained at {}",
                backup.display()
            ));
        }
        return Err(format!(
            "Update failed; previous installation preserved: {error}"
        ));
    }
    if existed {
        let _ = std::fs::remove_dir_all(backup);
    }
    Ok(())
}
use std::process::Command;

const MATERIAL_ICON_THEME_URL: &str = "https://github.com/zed-extensions/material-icon-theme.git";
const MATERIAL_ICON_THEME_TAG: &str = "v1.3.1";

/// Ensures the Material Icon Theme extension is present in the local
/// extensions directory, cloning it from its real upstream repository if
/// it's missing. Silently does nothing if `git` isn't on PATH or the clone
/// fails — callers already treat a missing/unloadable theme as "fall back
/// gracefully," not a fatal error, so a failed install here shouldn't be one
/// either.
///
/// This is the one extension LightLine installs itself, unprompted, so
/// there's always at least one icon theme available — everything else goes
/// through `install()` below, driven by the registry and a user's Install
/// click. This runs synchronously on the caller's thread: on a fresh install
/// that means a one-time blocking `git clone` before the window first
/// paints; every subsequent launch is a single fast file-existence check.
pub fn ensure_material_icon_theme() {
    let target_exists = crate::workflow::extensions_dir()
        .map(|dir| dir.join("material-icon-theme"))
        .is_some_and(|target| target.join("extension.toml").is_file());
    if target_exists {
        return;
    }
    let _ = install(
        "material-icon-theme",
        MATERIAL_ICON_THEME_URL,
        Some(MATERIAL_ICON_THEME_TAG),
    );
}

/// Clones `git_url` (optionally at `tag`) into
/// `<extensions_dir>/<id>`, replacing anything already there. Returns the
/// installed extension's directory on success.
pub fn install(id: &str, git_url: &str, tag: Option<&str>) -> Result<PathBuf, String> {
    validate_id(id)?;
    let _guard = INSTALL_LOCK.lock().map_err(|e| e.to_string())?;
    let dir =
        crate::workflow::extensions_dir().ok_or("could not determine the extensions directory")?;
    install_in(&dir, id, git_url, tag)
}

fn install_in(dir: &Path, id: &str, git_url: &str, tag: Option<&str>) -> Result<PathBuf, String> {
    validate_id(id)?;
    let target = dir.join(id);
    if !crate::workflow::command_available("git") {
        return Err("git was not found on PATH".into());
    }
    std::fs::create_dir_all(dir).map_err(|error| format!("could not create {dir:?}: {error}"))?;
    let mut lock_options = std::fs::OpenOptions::new();
    lock_options
        .read(true)
        .write(true)
        .create(true)
        .truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        lock_options.share_mode(0);
    }
    // Windows releases this exclusive handle on a crash, so a later launch
    // can recover the stable backup instead of losing it under an old PID.
    let _lock = lock_options
        .open(dir.join(format!(".update-{id}.lock")))
        .map_err(|e| format!("Another update may be running: {e}"))?;
    let staging = dir.join(format!(".staging-{id}"));
    let backup = dir.join(format!(".backup-{id}"));
    if backup.exists() && !target.exists() {
        std::fs::rename(&backup, &target)
            .map_err(|e| format!("Could not restore previous extension: {e}"))?;
    }
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    if backup.exists() {
        return Err(format!(
            "Previous extension backup retained at {}; remove it before updating again",
            backup.display()
        ));
    }
    let clone = |tag: Option<&str>| -> Result<bool, String> {
        let mut command = Command::new("git");
        command.args(["clone", "--depth", "1"]);
        if let Some(tag) = tag {
            command.args(["--branch", tag]);
        }
        command.arg("--").arg(git_url).arg(&staging);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let status = command
            .status()
            .map_err(|error| format!("could not run git: {error}"))?;
        Ok(status.success())
    };
    // Not every registry entry tags its releases as "v<version>" (the
    // registry's own version number can just track upstream's default
    // branch) -- confirmed against a real extension, dracula/zed, which has
    // no git tags at all. Fall back to the default branch rather than
    // failing the install over a naming convention the registry doesn't
    // actually guarantee.
    let result = (|| {
        if !clone(tag)? {
            if tag.is_none() {
                return Err(format!("git clone of {git_url} failed"));
            }
            if staging.exists() {
                std::fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
            }
            if !clone(None)? {
                return Err(format!("git clone of {git_url} failed"));
            }
        }
        validate_download(&staging, id)?;
        promote(&staging, &target, &backup)?;
        Ok(target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

/// Removes an installed extension's local files. Idempotent: missing is not
/// an error.
pub fn uninstall(id: &str) -> Result<(), String> {
    validate_id(id)?;
    let _guard = INSTALL_LOCK.lock().map_err(|e| e.to_string())?;
    let Some(dir) = crate::workflow::extensions_dir() else {
        return Ok(());
    };
    let target = dir.join(id);
    if !target.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&target)
        .map_err(|error| format!("could not remove {}: {error}", target.display()))
}

/// True when `id` has already been installed locally (regardless of type).
pub fn is_installed(id: &str) -> bool {
    if validate_id(id).is_err() {
        return false;
    }
    crate::workflow::extensions_dir()
        .map(|dir| dir.join(id))
        .is_some_and(|dir| dir.join("extension.toml").is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn fixture(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lightline-extension-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    #[test]
    fn ids_cannot_escape_or_alias_windows_device_paths() {
        for id in [
            "",
            "..",
            "../other",
            "a/b",
            "a\\b",
            "C:\\other",
            "a:",
            "a.",
            "a ",
            "CON",
            "con",
            "lpt1",
            "-theme",
            ".theme",
        ] {
            assert!(validate_id(id).is_err(), "{id}");
            assert!(!is_installed(id));
            assert!(uninstall(id).is_err());
        }
        for id in ["material-icon-theme", "dracula", "theme_2"] {
            assert!(validate_id(id).is_ok());
        }
    }
    #[test]
    fn failed_promotion_rolls_back_the_existing_copy() {
        let dir = fixture("rollback");
        let target = dir.join("theme");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("old.txt"), "working copy").unwrap();
        assert!(promote(&dir.join("missing"), &target, &dir.join("backup")).is_err());
        assert_eq!(
            fs::read_to_string(target.join("old.txt")).unwrap(),
            "working copy"
        );
        assert!(!dir.join("backup").exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn successful_promotion_replaces_only_after_staging_is_ready() {
        let dir = fixture("promote");
        let target = dir.join("theme");
        let staging = dir.join("staging");
        fs::create_dir(&target).unwrap();
        fs::create_dir(&staging).unwrap();
        fs::write(target.join("version"), "old").unwrap();
        fs::write(staging.join("version"), "new").unwrap();
        promote(&staging, &target, &dir.join("backup")).unwrap();
        assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "new");
        assert!(!staging.exists());
        assert!(!dir.join("backup").exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn invalid_download_is_rejected_before_replacement() {
        let dir = fixture("validate");
        fs::write(
            dir.join("extension.toml"),
            "id='theme'\nname='Theme'\nversion='1'\n",
        )
        .unwrap();
        assert!(validate_download(&dir, "other").is_err());
        assert!(validate_download(&dir, "theme").is_err());
        fs::create_dir(dir.join("themes")).unwrap();
        fs::write(dir.join("themes/broken.json"), "broken").unwrap();
        assert!(validate_download(&dir, "theme").is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn local_clone_failure_keeps_old_install_and_cleans_staging() {
        if !crate::workflow::command_available("git") {
            return;
        }
        let dir = fixture("clone-failure");
        let target = dir.join("theme");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("version"), "old").unwrap();
        assert!(
            install_in(
                &dir,
                "theme",
                dir.join("missing-repository").to_str().unwrap(),
                None
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "old");
        assert!(!dir.join(".staging-theme").exists());
        assert!(!dir.join(".backup-theme").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
