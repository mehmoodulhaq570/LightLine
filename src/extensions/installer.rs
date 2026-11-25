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

/// Whether `path` names a folder inside a repository that is safe to hand
/// to git and to join onto a local folder: relative, `/`-separated, with
/// only plain names (no `.`, `..`, drive, `\` or `:`).
pub fn valid_folder_path(path: &str) -> bool {
    path.len() <= 256
        && !path.is_empty()
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.starts_with('-')
                && part
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        })
}

fn validate_download(dir: &Path, id: &str) -> Result<(), String> {
    let manifest = super::zed_manifest::Manifest::load(dir).ok_or("Invalid extension.toml")?;
    if manifest.id != id || manifest.name.trim().is_empty() || manifest.version.trim().is_empty() {
        return Err("Extension manifest does not match the requested extension".into());
    }
    let mut usable = false;
    if super::zed_manifest::is_icon_theme(dir) {
        crate::icon_theme::IconTheme::load(dir)
            .ok_or("Invalid icon theme files")?
            .validate_assets()?;
        usable = true;
    } else if super::zed_manifest::is_color_theme(dir) {
        crate::color_theme::ZedColorTheme::load(dir).ok_or("Invalid color theme files")?;
        usable = true;
    }
    if super::language::is_language_extension(dir) {
        super::language::validate(dir)?;
        usable = true;
    }
    if super::snippets::is_snippet_pack(dir) {
        super::snippets::validate(dir)?;
        usable = true;
    }
    // Its server is started from PATH; nothing in it runs.
    if super::tools::is_tool_extension(dir) {
        usable = true;
    }
    if !usable {
        return Err(
            "Unsupported extension: only themes, snippets, language servers and languages with \
             a Tree-sitter grammar can be installed"
                .into(),
        );
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

/// Installs `git_url` at `commit` into `<extensions_dir>/<id>`, replacing
/// anything already there. With a `folder`, the extension is that folder of
/// the repository, and only it is downloaded. Returns the installed
/// extension's directory.
pub fn install(
    id: &str,
    git_url: &str,
    commit: &str,
    folder: Option<&str>,
) -> Result<PathBuf, String> {
    validate_id(id)?;
    let _guard = INSTALL_LOCK.lock().map_err(|e| e.to_string())?;
    let dir =
        crate::workflow::extensions_dir().ok_or("could not determine the extensions directory")?;
    install_in(&dir, id, git_url, commit, folder)
}

pub(crate) fn install_in(
    dir: &Path,
    id: &str,
    git_url: &str,
    commit: &str,
    folder: Option<&str>,
) -> Result<PathBuf, String> {
    validate_id(id)?;
    if folder.is_some_and(|folder| !valid_folder_path(folder)) {
        return Err("Invalid extension folder".into());
    }
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
    // The whole sparse checkout, when only a folder of it is the extension.
    let download = dir.join(format!(".download-{id}"));
    if download.exists() {
        std::fs::remove_dir_all(&download).map_err(|e| e.to_string())?;
    }
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
    let result = (|| {
        match folder {
            Some(folder) => {
                fetch_folder(&download, git_url, commit, folder)?;
                let inside = download.join(folder);
                if !inside.join("extension.toml").is_file() {
                    return Err(format!("{folder} isn't an extension at {commit}"));
                }
                std::fs::rename(&inside, &staging).map_err(|e| e.to_string())?;
                std::fs::remove_dir_all(&download).map_err(|e| e.to_string())?;
            }
            None => fetch_commit(&staging, git_url, commit)?,
        }
        // A language's grammar is its own repository, built here so a
        // failed build leaves the installed copy alone.
        if super::language::is_language_extension(&staging) {
            super::language::build_grammars(&staging, fetch_commit)?;
        }
        validate_download(&staging, id)?;
        promote(&staging, &target, &backup)?;
        Ok(target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
        let _ = std::fs::remove_dir_all(&download);
    }
    result
}

// Runs git with `args`, without a console window, inside the job that ends
// with LightLine.
fn git(args: &[&std::ffi::OsStr], failure: impl FnOnce() -> String) -> Result<(), String> {
    let mut command = Command::new("git");
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let status =
        crate::jobs::status(&mut command).map_err(|error| format!("could not run git: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(failure())
    }
}

// Only `folder` of `git_url` at `commit`, checked out under `root`: a
// shallow, blob-less fetch of the commit, then a sparse checkout, so the
// other folders' files are never downloaded. The sparse-checkout file and
// partial-clone settings are written directly rather than through `git
// sparse-checkout`, whose options changed between git versions.
fn fetch_folder(root: &Path, git_url: &str, commit: &str, folder: &str) -> Result<(), String> {
    let failed = || format!("could not download {folder} of {git_url} at {commit}");
    let at = root.as_os_str();
    let config = |key: &str, value: &str| {
        git(
            &[
                "-C".as_ref(),
                at,
                "config".as_ref(),
                key.as_ref(),
                value.as_ref(),
            ],
            failed,
        )
    };
    git(&["init".as_ref(), "-q".as_ref(), at], failed)?;
    config("remote.origin.url", git_url)?;
    config("remote.origin.promisor", "true")?;
    config("remote.origin.partialclonefilter", "blob:none")?;
    config("extensions.partialClone", "origin")?;
    config("core.sparseCheckout", "true")?;
    let info = root.join(".git").join("info");
    std::fs::create_dir_all(&info).map_err(|e| e.to_string())?;
    std::fs::write(info.join("sparse-checkout"), format!("/{folder}/\n"))
        .map_err(|e| e.to_string())?;
    git(
        &[
            "-C".as_ref(),
            at,
            "fetch".as_ref(),
            "-q".as_ref(),
            "--depth".as_ref(),
            "1".as_ref(),
            "--filter=blob:none".as_ref(),
            "origin".as_ref(),
            commit.as_ref(),
        ],
        failed,
    )?;
    git(
        &[
            "-C".as_ref(),
            at,
            "-c".as_ref(),
            "advice.detachedHead=false".as_ref(),
            "checkout".as_ref(),
            "-q".as_ref(),
            "FETCH_HEAD".as_ref(),
        ],
        failed,
    )
}

// Exactly `commit` of `git_url` into `folder`, with no other history: the
// version the registry pins, whatever the repository's branches say now.
fn fetch_commit(folder: &Path, git_url: &str, commit: &str) -> Result<(), String> {
    let git = |args: &[&std::ffi::OsStr]| {
        git(args, || format!("could not download {git_url} at {commit}"))
    };
    let folder = folder.as_os_str();
    git(&["init".as_ref(), "-q".as_ref(), folder])?;
    git(&[
        "-C".as_ref(),
        folder,
        "fetch".as_ref(),
        "-q".as_ref(),
        "--depth".as_ref(),
        "1".as_ref(),
        "--".as_ref(),
        git_url.as_ref(),
        commit.as_ref(),
    ])?;
    git(&[
        "-C".as_ref(),
        folder,
        "-c".as_ref(),
        "advice.detachedHead=false".as_ref(),
        "checkout".as_ref(),
        "-q".as_ref(),
        "FETCH_HEAD".as_ref(),
    ])
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
    #[ignore = "hits the real network; run explicitly with --ignored"]
    fn installs_exactly_the_commit_the_registry_pins() {
        let resolved = crate::extensions::zed_registry::resolve("dracula").unwrap();
        let dir = fixture("pinned");
        let installed = install_in(
            &dir,
            "dracula",
            &resolved.git_url,
            &resolved.commit,
            resolved.path.as_deref(),
        )
        .unwrap();
        assert!(installed.join("extension.toml").is_file());
        let head = Command::new("git")
            .arg("-C")
            .arg(&installed)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&head.stdout).trim(),
            resolved.commit
        );
        fs::remove_dir_all(dir).unwrap();
    }

    // Zed's html extension lives inside Zed's own (very large) repository:
    // only its folder may be downloaded.
    #[test]
    #[ignore = "hits the real network; run explicitly with --ignored"]
    fn a_folder_of_zeds_own_repository_downloads_alone() {
        let resolved = crate::extensions::zed_registry::resolve("html").unwrap();
        let folder = resolved.path.clone().expect("html has a path");
        let dir = fixture("zed-folder");
        let started = std::time::Instant::now();
        fetch_folder(&dir, &resolved.git_url, &resolved.commit, &folder).unwrap();
        eprintln!("{folder} in {:.1} s", started.elapsed().as_secs_f32());
        assert!(dir.join(&folder).join("extension.toml").is_file());
        let top: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != ".git")
            .collect();
        assert_eq!(top, [folder.split('/').next().unwrap()]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "hits the real network; run explicitly with --ignored"]
    fn real_snippet_packs_install_and_offer_their_snippets() {
        use crate::extensions::snippets;
        let dir = fixture("snippet-packs");
        // One file for one language, and one file per language in folders.
        for id in ["fastapi-snippets", "comment-block-snippets"] {
            let resolved = crate::extensions::zed_registry::resolve(id).unwrap();
            let installed = install_in(
                &dir,
                id,
                &resolved.git_url,
                &resolved.commit,
                resolved.path.as_deref(),
            )
            .unwrap();
            assert!(snippets::validate(&installed).unwrap() > 0, "{id}");
        }
        assert_eq!(
            snippets::pack_languages(&dir.join("fastapi-snippets")),
            [Some("python".to_string())]
        );
        assert!(
            snippets::pack_languages(&dir.join("comment-block-snippets"))
                .contains(&Some("shell script".to_string()))
        );
        let python = snippets::installed_in(&dir, &["python".to_string()]);
        assert!(
            python
                .iter()
                .any(|s| s.prefixes.iter().any(|p| p == "fastapp"))
        );
        assert!(python.iter().any(|s| s.prefixes[0].starts_with("com-")));
        let lua = snippets::installed_in(&dir, &["lua".to_string()]);
        assert!(!lua.is_empty());
        assert!(
            lua.iter()
                .all(|s| !s.prefixes.iter().any(|p| p == "fastapp"))
        );
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
                "0123456789012345678901234567890123456789",
                None,
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "old");
        assert!(!dir.join(".staging-theme").exists());
        assert!(!dir.join(".backup-theme").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn folder_paths_cannot_escape_the_repository() {
        for path in [
            "",
            "/themes/a",
            "themes/",
            "themes//a",
            "./themes",
            "themes/../..",
            "..",
            "C:/themes",
            "themes\\a",
            "themes/a b",
            "-c/x",
            "themes/a:b",
        ] {
            assert!(!valid_folder_path(path), "{path}");
        }
        for path in ["themes/lightline-themes", "extensions/html", "a", "a_b/c.d"] {
            assert!(valid_folder_path(path), "{path}");
        }
        let dir = fixture("bad-folder");
        let error = install_in(&dir, "theme", "https://github.com/a/b", "a", Some("../x"));
        assert_eq!(error.unwrap_err(), "Invalid extension folder");
        fs::remove_dir_all(dir).unwrap();
    }

    // A repository holding several extensions in folders, as
    // lightline-extensions does: installing one brings only its files.
    #[test]
    fn one_folder_of_a_repository_installs_alone() {
        if !crate::workflow::command_available("git") {
            return;
        }
        let dir = fixture("folder");
        let repo = dir.join("repo");
        let theme = r##"{"name": "T", "themes": [{"name": "T Light", "appearance": "light",
            "style": {"editor.background": "#ffffff"}}]}"##;
        for (path, text) in [
            (
                "themes/one/extension.toml",
                "id = \"one\"\nname = \"One\"\nversion = \"1.0.0\"\n",
            ),
            ("themes/one/themes/one.json", theme),
            (
                "themes/two/extension.toml",
                "id = \"two\"\nname = \"Two\"\nversion = \"1.0.0\"\n",
            ),
            ("themes/two/themes/two.json", theme),
            ("README.md", "not part of any extension\n"),
        ] {
            let file = repo.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        }
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        };
        run(&["init", "-q"]);
        run(&["config", "uploadpack.allowFilter", "true"]);
        run(&["add", "."]);
        run(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "-m",
            "two themes",
        ]);
        let head = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let commit = String::from_utf8_lossy(&head.stdout).trim().to_owned();
        let extensions = dir.join("extensions");
        let url = repo.display().to_string();

        let installed = install_in(&extensions, "one", &url, &commit, Some("themes/one")).unwrap();
        assert_eq!(installed, extensions.join("one"));
        let mut files: Vec<String> = fs::read_dir(&installed)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert_eq!(files, ["extension.toml", "themes"]);
        let mut left: Vec<String> = fs::read_dir(&extensions)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| !name.ends_with(".lock"))
            .collect();
        left.sort();
        assert_eq!(left, ["one"], "no download or staging folder is left");

        // A folder that isn't there, or holds another extension, changes
        // nothing.
        for (id, folder) in [("one", "themes/three"), ("one", "themes/two")] {
            assert!(install_in(&extensions, id, &url, &commit, Some(folder)).is_err());
            assert!(installed.join("themes/one.json").is_file());
            assert!(!extensions.join(format!(".download-{id}")).exists());
            assert!(!extensions.join(format!(".staging-{id}")).exists());
        }
        fs::remove_dir_all(dir).unwrap();
    }

    // The Extensions panel's Update is an install over the installed copy:
    // the new version replaces it, a theme keeps its variant's name (which
    // is how the theme in use is applied again), and a failed update leaves
    // the version that was there.
    #[test]
    fn an_update_replaces_the_installed_copy_or_leaves_it_alone() {
        if !crate::workflow::command_available("git") {
            return;
        }
        let dir = fixture("update");
        let repo = dir.join("repo");
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        };
        let write = |path: &str, text: &str| {
            let file = repo.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        };
        let commit = |message: &str| {
            run(&["add", "-A"]);
            run(&[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-q",
                "-m",
                message,
            ]);
            let head = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap();
            String::from_utf8_lossy(&head.stdout).trim().to_owned()
        };
        let version = |version: &str, background: &str| {
            write(
                "themes/one/extension.toml",
                &format!("id = \"one\"\nname = \"One\"\nversion = \"{version}\"\n"),
            );
            write(
                "themes/one/themes/one.json",
                &format!(
                    r##"{{"name": "One", "themes": [{{"name": "One Dark", "appearance": "dark",
                    "style": {{"editor.background": "{background}"}}}}]}}"##
                ),
            );
        };
        fs::create_dir_all(&repo).unwrap();
        run(&["init", "-q"]);
        run(&["config", "uploadpack.allowFilter", "true"]);
        version("1.0.0", "#000000");
        let first = commit("1.0.0");
        version("1.1.0", "#102030");
        let second = commit("1.1.0");
        write("themes/one/themes/one.json", "broken");
        let broken = commit("broken");

        let extensions = dir.join("extensions");
        let url = repo.display().to_string();
        let installed_version = || {
            crate::extensions::zed_manifest::Manifest::load(&extensions.join("one"))
                .unwrap()
                .version
        };
        let background = || {
            let themes = crate::color_theme::installed(&extensions);
            let (id, theme) = themes
                .iter()
                .find(|(_, theme)| theme.name == "One Dark")
                .expect("the variant keeps its name");
            assert_eq!(id, "one");
            let color = theme.style_color("editor.background").unwrap();
            (color.r, color.g, color.b)
        };
        install_in(&extensions, "one", &url, &first, Some("themes/one")).unwrap();
        assert_eq!(installed_version(), "1.0.0");
        assert_eq!(background(), (0, 0, 0));

        install_in(&extensions, "one", &url, &second, Some("themes/one")).unwrap();
        assert_eq!(installed_version(), "1.1.0");
        assert_eq!(background(), (0x10, 0x20, 0x30));

        // A broken theme file, and a commit that isn't there.
        for commit in [broken.as_str(), "0123456789012345678901234567890123456789"] {
            assert!(install_in(&extensions, "one", &url, commit, Some("themes/one")).is_err());
            assert_eq!(installed_version(), "1.1.0");
            assert_eq!(background(), (0x10, 0x20, 0x30));
            for leftover in [".staging-one", ".backup-one", ".download-one"] {
                assert!(!extensions.join(leftover).exists(), "{leftover}");
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
