//! Downloads a checker's program (extensions/tools.rs) when the user agrees
//! to it, as Pyright is: typos-lsp, codebook-lsp and harper-ls are each one
//! `.exe` in a zip on their official GitHub release page.
//!
//! Each is pinned to one release and the SHA-256 of its zip, as GitHub
//! lists it: a file that doesn't match is refused before it's unpacked.
//! The program goes into `%APPDATA%\LightLine\tools\<command>-<version>\`
//! and nothing else on the PC changes. Commands are looked up there after
//! PATH (`language_servers::find_command`). Nothing is fetched unless the
//! user says Download.
//!
//! To update one, change its version, addresses and hashes below: the
//! release's assets list each file's `sha256:` digest.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// One pinned release of a checker's program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Release {
    /// The extension's server name (`language_servers::KNOWN`).
    pub server: &'static str,
    /// The program, without `.exe`.
    pub command: &'static str,
    pub version: &'static str,
    /// Where it comes from, for the question asked first.
    pub site: &'static str,
    /// The zip for this PC and its SHA-256, by CPU.
    x64: Option<(&'static str, &'static str)>,
    arm64: Option<(&'static str, &'static str)>,
    /// About how big the download is, in MB, for the question.
    pub megabytes: u32,
}

impl Release {
    // The zip for this PC's CPU and its SHA-256.
    fn asset(&self) -> Option<(&'static str, &'static str)> {
        if cfg!(target_arch = "aarch64") {
            self.arm64
        } else if cfg!(target_arch = "x86_64") {
            self.x64
        } else {
            None
        }
    }

    // `%APPDATA%\LightLine\tools\<command>-<version>`.
    fn folder(&self) -> Option<PathBuf> {
        Some(
            PathBuf::from(std::env::var_os("APPDATA")?)
                .join("LightLine")
                .join("tools")
                .join(format!("{}-{}", self.command, self.version)),
        )
    }

    /// The downloaded program, if it's there.
    pub fn installed(&self) -> Option<PathBuf> {
        let exe = self.folder()?.join(format!("{}.exe", self.command));
        exe.is_file().then_some(exe)
    }
}

const RELEASES: &[Release] = &[
    Release {
        server: "typos",
        command: "typos-lsp",
        version: "0.1.58",
        site: "github.com/tekumara/typos-lsp",
        x64: Some((
            "https://github.com/tekumara/typos-lsp/releases/download/v0.1.58/typos-lsp-v0.1.58-x86_64-pc-windows-msvc.zip",
            "626fc52b7f056c9b84714fecb8cc6af926a11cca5b7e17126d809d8615ccab95",
        )),
        arm64: Some((
            "https://github.com/tekumara/typos-lsp/releases/download/v0.1.58/typos-lsp-v0.1.58-aarch64-pc-windows-msvc.zip",
            "b9b58f2ba525afcb46520f815a806435d96d16b231ede568b95bbbd68ae95e46",
        )),
        megabytes: 4,
    },
    Release {
        server: "codebook",
        command: "codebook-lsp",
        version: "0.4.0",
        site: "github.com/blopker/codebook",
        x64: Some((
            "https://github.com/blopker/codebook/releases/download/v0.4.0/codebook-lsp-x86_64-pc-windows-msvc.zip",
            "51f2c55bb266b502e57eccbb5f016989c7687f3b2d947b85ac5f9cd98fb0866c",
        )),
        arm64: Some((
            "https://github.com/blopker/codebook/releases/download/v0.4.0/codebook-lsp-aarch64-pc-windows-msvc.zip",
            "4461ec81dd18a9785fada9573b58f6d4220874cf65e49e6a85dac8f0b92bc667",
        )),
        megabytes: 9,
    },
    Release {
        server: "harper-ls",
        command: "harper-ls",
        version: "3.2.0",
        site: "github.com/Automattic/harper",
        x64: Some((
            "https://github.com/Automattic/harper/releases/download/v3.2.0/harper-ls-x86_64-pc-windows-msvc.zip",
            "a9e370166ace4e92a592f954baa2fedf4528e6daf59e64a9973fc9f3d7d30d5e",
        )),
        // No Windows ARM64 build is published.
        arm64: None,
        megabytes: 14,
    },
];

/// The pinned release of `server`'s program, when one can be downloaded for
/// this PC.
pub fn release_for(server: &str) -> Option<Release> {
    RELEASES
        .iter()
        .find(|release| release.server == server && release.asset().is_some())
        .copied()
}

/// The downloaded program for `command` (`typos-lsp`), for command lookup.
pub fn installed_command(command: &str) -> Option<PathBuf> {
    let name = command
        .strip_suffix(".exe")
        .unwrap_or(command)
        .to_ascii_lowercase();
    RELEASES
        .iter()
        .find(|release| release.command == name)
        .and_then(Release::installed)
}

const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Downloads and unpacks `release`, returning the program. The zip is
/// checked against its pinned hash before anything is unpacked. Runs on a
/// worker.
pub fn download(release: &Release) -> Result<PathBuf, String> {
    let (url, sha256) = release
        .asset()
        .ok_or("no download for this PC's processor")?;
    let folder = release
        .folder()
        .ok_or("could not find LightLine's folder")?;
    let mut response = ureq::get(url)
        .config()
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .call()
        .map_err(|error| format!("could not download it: {error}"))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|error| format!("could not download it: {error}"))?;
    let actual = crate::sha256::hex(&bytes);
    if actual != sha256 {
        return Err(format!(
            "the download didn't match the file LightLine expects (SHA-256 {actual}), so it \
             wasn't used"
        ));
    }
    install_zip(&bytes, &folder, release.command)
}

// Unpacks `zip` (already checked) into a staging folder beside `folder`,
// finds `<command>.exe` in it, and moves just that into `folder`.
fn install_zip(zip: &[u8], folder: &Path, command: &str) -> Result<PathBuf, String> {
    let parent = folder.parent().ok_or("bad tools folder")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let staging = parent.join(format!(".staging-{command}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let result = (|| {
        let archive = staging.join("download.zip");
        std::fs::write(&archive, zip).map_err(|error| error.to_string())?;
        unzip(&archive, &staging)?;
        let exe = find_file(&staging, &format!("{command}.exe"), 4)
            .ok_or_else(|| format!("{command}.exe wasn't in the download"))?;
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
        let target = folder.join(format!("{command}.exe"));
        std::fs::rename(&exe, &target).map_err(|error| error.to_string())?;
        Ok(target)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

// Windows' own tar (bsdtar, in Windows 10 1803 and later) unpacks zips;
// it refuses entries that would land outside `into`.
fn unzip(archive: &Path, into: &Path) -> Result<(), String> {
    let mut command = std::process::Command::new("tar");
    command.arg("-xf").arg(archive).arg("-C").arg(into);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = crate::jobs::output(&mut command)
        .map_err(|error| format!("could not unpack it: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "could not unpack it: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn find_file(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut folders = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            folders.push(path);
        } else if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
        {
            return Some(path);
        }
    }
    if depth == 0 {
        return None;
    }
    folders
        .iter()
        .find_map(|folder| find_file(folder, name, depth - 1))
}

/// Removes the downloaded programs of `servers` (an uninstalled
/// extension's). A program still running is retried for a few seconds, as
/// its server stops; runs on a worker.
pub fn remove(servers: &[String]) {
    for release in RELEASES
        .iter()
        .filter(|release| servers.iter().any(|server| server == release.server))
    {
        let Some(folder) = release.folder() else {
            continue;
        };
        for _ in 0..25 {
            if !folder.exists() || std::fs::remove_dir_all(&folder).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_release_is_pinned_to_a_hash_and_its_known_command() {
        for release in RELEASES {
            let command = crate::extensions::language_servers::known_command(release.server)
                .unwrap_or_else(|| panic!("{} has no known command", release.server));
            assert_eq!(command.command, release.command, "{}", release.server);
            for (url, sha256) in release.x64.iter().chain(release.arm64.iter()) {
                assert!(url.starts_with("https://github.com/"), "{url}");
                assert!(url.contains(release.version), "{url}");
                assert_eq!(sha256.len(), 64, "{url}");
                assert!(
                    sha256
                        .bytes()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                );
            }
        }
        assert!(release_for("cspell").is_none(), "cspell needs Node.js");
        if cfg!(target_arch = "x86_64") {
            assert_eq!(
                release_for("harper-ls").map(|r| r.command),
                Some("harper-ls")
            );
        }
    }

    #[test]
    fn a_zip_is_unpacked_and_only_the_program_kept() {
        if !crate::workflow::command_available("tar") {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "lightline-tool-download-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // A zip made with tar itself: a nested program and a readme.
        let source = dir.join("source");
        std::fs::create_dir_all(source.join("target/release")).unwrap();
        std::fs::write(source.join("target/release/demo-lsp.exe"), b"MZ demo").unwrap();
        std::fs::write(source.join("README.md"), b"readme").unwrap();
        let zip = dir.join("demo.zip");
        let made = crate::jobs::output(
            std::process::Command::new("tar")
                .arg("-a")
                .arg("-cf")
                .arg(&zip)
                .arg("-C")
                .arg(&source)
                .arg("."),
        )
        .unwrap();
        assert!(made.status.success(), "{made:?}");
        let folder = dir.join("tools").join("demo-lsp-1.0");
        let exe = install_zip(&std::fs::read(&zip).unwrap(), &folder, "demo-lsp").unwrap();
        assert_eq!(exe, folder.join("demo-lsp.exe"));
        assert_eq!(std::fs::read(&exe).unwrap(), b"MZ demo");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        // The staging folder is gone.
        assert_eq!(std::fs::read_dir(dir.join("tools")).unwrap().count(), 1);
        assert!(install_zip(b"not a zip", &dir.join("tools/x-1"), "x").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "hits the real network; run explicitly with --ignored"]
    fn the_pinned_downloads_match_their_hashes() {
        for release in RELEASES {
            let Some((url, sha256)) = release.asset() else {
                continue;
            };
            let bytes = ureq::get(url)
                .call()
                .unwrap()
                .body_mut()
                .with_config()
                .limit(MAX_BYTES)
                .read_to_vec()
                .unwrap();
            assert_eq!(crate::sha256::hex(&bytes), sha256, "{url}");
        }
    }
}
