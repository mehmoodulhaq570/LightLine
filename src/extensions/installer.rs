//! Downloads an extension's files into the local extensions directory.
//! Shells out to `git clone` rather than adding an HTTP-tarball path or a
//! libgit2 dependency — every extension in the Zed registry is a git
//! submodule pointing at an ordinary repo, and `git` is a reasonable
//! dependency for a developer tool to expect.

use std::path::PathBuf;
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
    let dir =
        crate::workflow::extensions_dir().ok_or("could not determine the extensions directory")?;
    let target = dir.join(id);
    if !crate::workflow::command_available("git") {
        return Err("git was not found on PATH".into());
    }
    std::fs::create_dir_all(&dir).map_err(|error| format!("could not create {dir:?}: {error}"))?;
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|error| {
            format!("could not remove the existing install at {target:?}: {error}")
        })?;
    }
    let clone = |tag: Option<&str>| -> Result<bool, String> {
        let mut command = Command::new("git");
        command.args(["clone", "--depth", "1"]);
        if let Some(tag) = tag {
            command.args(["--branch", tag]);
        }
        command.arg(git_url).arg(&target);
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
    if !clone(tag)? {
        if tag.is_none() {
            return Err(format!("git clone of {git_url} failed"));
        }
        std::fs::remove_dir_all(&target).ok();
        if !clone(None)? {
            return Err(format!("git clone of {git_url} failed"));
        }
    }
    Ok(target)
}

/// Removes an installed extension's local files. Idempotent: missing is not
/// an error.
pub fn uninstall(id: &str) -> Result<(), String> {
    let Some(dir) = crate::workflow::extensions_dir() else {
        return Ok(());
    };
    let target = dir.join(id);
    if !target.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&target)
        .map_err(|error| format!("could not remove {target:?}: {error}"))
}

/// True when `id` has already been installed locally (regardless of type).
pub fn is_installed(id: &str) -> bool {
    crate::workflow::extensions_dir()
        .map(|dir| dir.join(id))
        .is_some_and(|dir| dir.join("extension.toml").is_file())
}
