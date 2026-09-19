use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub enum LaunchRequest {
    Shell {
        cwd: PathBuf,
        no_profile: bool,
    },
    Python {
        interpreter: PathBuf,
        script: PathBuf,
        cwd: PathBuf,
        no_profile: bool,
    },
}

impl LaunchRequest {
    pub fn shell(cwd: PathBuf) -> Self {
        Self::Shell {
            cwd,
            no_profile: false,
        }
    }

    pub fn python(interpreter: PathBuf, script: PathBuf, cwd: PathBuf) -> Self {
        Self::Python {
            interpreter,
            script,
            cwd,
            no_profile: false,
        }
    }

    pub fn without_profile(mut self) -> Self {
        match &mut self {
            Self::Shell { no_profile, .. } | Self::Python { no_profile, .. } => *no_profile = true,
        }
        self
    }
}

#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub cwd: PathBuf,
    // Child-only overrides. None removes an inherited value (case-insensitively on Windows).
    pub environment: Vec<(OsString, Option<OsString>)>,
}

// This helper does filesystem discovery. TerminalService calls it on its owner thread.
pub fn resolve_powershell() -> Result<PathBuf, String> {
    let paths = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&paths) {
        // Empty or relative PATH entries must not turn a workspace file into the shell.
        // WindowsApps execution aliases are deliberately eligible.
        if directory.is_absolute() {
            let path = directory.join("pwsh.exe");
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    for root in ["ProgramW6432", "ProgramFiles"] {
        if let Some(root) = std::env::var_os(root) {
            let directory = PathBuf::from(root).join("PowerShell");
            if let Ok(entries) = std::fs::read_dir(directory) {
                let mut candidates: Vec<_> = entries
                    .flatten()
                    .map(|entry| entry.path().join("pwsh.exe"))
                    .filter(|path| path.is_file())
                    .collect();
                candidates.sort();
                if let Some(path) = candidates.pop() {
                    return Ok(path);
                }
            }
        }
    }
    let root = std::env::var_os("SystemRoot").or_else(|| std::env::var_os("WINDIR"));
    if let Some(root) = root {
        let path = PathBuf::from(root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        if path.is_absolute() && path.is_file() {
            return Ok(path);
        }
    }
    Err("PowerShell was not found in PATH, installed PowerShell directories, or System32".into())
}

pub fn prepare_launch(request: &LaunchRequest) -> Result<LaunchSpec, String> {
    let executable = resolve_powershell()?;
    prepare_with_shell(request, executable)
}

fn prepare_with_shell(request: &LaunchRequest, executable: PathBuf) -> Result<LaunchSpec, String> {
    let (cwd, no_profile) = match request {
        LaunchRequest::Shell { cwd, no_profile }
        | LaunchRequest::Python {
            cwd, no_profile, ..
        } => (cwd, *no_profile),
    };
    let cwd = absolute_existing(cwd, true)?;
    let mut spec = LaunchSpec {
        executable,
        arguments: vec![OsString::from("-NoLogo")],
        cwd,
        environment: Vec::new(),
    };
    if no_profile {
        spec.arguments.push(OsString::from("-NoProfile"));
    }
    if let LaunchRequest::Python {
        interpreter,
        script,
        ..
    } = request
    {
        reject_pythonw(interpreter)?;
        let interpreter = absolute_existing(interpreter, false)?;
        let script = absolute_existing(script, false)?;
        let command = python_command(&interpreter, &script, &spec.cwd)?;
        spec.arguments.extend([
            OsString::from("-NoExit"),
            OsString::from("-EncodedCommand"),
            OsString::from(encode_powershell_command(&command)),
        ]);
        spec.environment = python_environment(&interpreter, std::env::var_os("PATH").as_deref())?;
    }
    Ok(spec)
}

fn absolute_existing(path: &Path, directory: bool) -> Result<PathBuf, String> {
    if (directory && !path.is_dir()) || (!directory && !path.is_file()) {
        return Err(format!(
            "{} does not exist or has the wrong type",
            path.display()
        ));
    }
    reject_nul(path.as_os_str())?;
    std::path::absolute(path)
        .map_err(|error| format!("Could not resolve {}: {error}", path.display()))
}

fn reject_pythonw(path: &Path) -> Result<(), String> {
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if stem.starts_with("pythonw") {
        Err("pythonw cannot run in an interactive terminal; select python.exe".into())
    } else {
        Ok(())
    }
}

pub(super) fn os_wide(value: &OsStr) -> Vec<u16> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        value.encode_wide().collect()
    }
    #[cfg(not(windows))]
    {
        value.to_string_lossy().encode_utf16().collect()
    }
}

pub(super) fn reject_nul(value: &OsStr) -> Result<(), String> {
    if os_wide(value).contains(&0) {
        Err("Paths and arguments cannot contain NUL".into())
    } else {
        Ok(())
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        result.push(ALPHABET[((word >> 18) & 63) as usize] as char);
        result.push(ALPHABET[((word >> 12) & 63) as usize] as char);
        result.push(if chunk.len() > 1 {
            ALPHABET[((word >> 6) & 63) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            ALPHABET[(word & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}

pub fn encode_powershell_command(command: &str) -> String {
    base64(
        &command
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
}

// PowerShell treats several smart quotes as syntax too. Encoding each path as
// data avoids *all* quote/dollar/backtick/semicolon interpolation, not just ASCII '.
pub fn powershell_path_expression(path: &Path) -> Result<String, String> {
    reject_nul(path.as_os_str())?;
    let bytes: Vec<_> = os_wide(path.as_os_str())
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    Ok(format!(
        "([System.Text.Encoding]::Unicode.GetString([System.Convert]::FromBase64String('{}')))",
        base64(&bytes)
    ))
}

pub fn python_command(interpreter: &Path, script: &Path, cwd: &Path) -> Result<String, String> {
    reject_pythonw(interpreter)?;
    let cwd = powershell_path_expression(cwd)?;
    let interpreter = powershell_path_expression(interpreter)?;
    let script = powershell_path_expression(script)?;
    // -ErrorAction Stop prevents running from a different directory if cd fails.
    // No execution-policy changes, activation script, or Invoke-Expression.
    Ok(format!(
        "Set-Location -LiteralPath {cwd} -ErrorAction Stop; & {interpreter} '-u' {script}"
    ))
}

fn python_environment(
    interpreter: &Path,
    inherited_path: Option<&OsStr>,
) -> Result<Vec<(OsString, Option<OsString>)>, String> {
    let directory = interpreter
        .parent()
        .ok_or("Python interpreter needs a parent directory")?;
    let is_scripts = directory
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("Scripts"));
    let scripts = if is_scripts {
        directory.to_path_buf()
    } else {
        directory.join("Scripts")
    };
    let mut paths = vec![directory.to_path_buf()];
    if scripts != directory {
        paths.push(scripts);
    }
    if let Some(path) = inherited_path {
        paths.extend(std::env::split_paths(path));
    }
    let path =
        std::env::join_paths(paths).map_err(|error| format!("Invalid child PATH: {error}"))?;
    let venv = if is_scripts {
        directory
            .parent()
            .filter(|root| root.join("pyvenv.cfg").is_file())
            .map(|root| root.as_os_str().to_os_string())
    } else {
        None
    };
    Ok(vec![
        (OsString::from("PATH"), Some(path)),
        (OsString::from("VIRTUAL_ENV"), venv),
        (OsString::from("PYTHONHOME"), None),
    ])
}

// Quote a single CreateProcess/CommandLineToArgvW argument, including trailing backslashes.
pub fn quote_windows_argument(argument: &OsStr) -> Result<Vec<u16>, String> {
    reject_nul(argument)?;
    let mut result = vec![u16::from(b'"')];
    let mut slashes = 0;
    for unit in os_wide(argument) {
        if unit == u16::from(b'\\') {
            slashes += 1;
            continue;
        }
        if unit == u16::from(b'"') {
            result.extend(std::iter::repeat_n(u16::from(b'\\'), slashes * 2 + 1));
        } else {
            result.extend(std::iter::repeat_n(u16::from(b'\\'), slashes));
        }
        slashes = 0;
        result.push(unit);
    }
    result.extend(std::iter::repeat_n(u16::from(b'\\'), slashes * 2));
    result.push(u16::from(b'"'));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(encoded: &str) -> String {
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut bytes = Vec::new();
        for chunk in encoded.as_bytes().chunks(4) {
            let mut value = 0u32;
            for b in chunk {
                value = (value << 6) | alphabet.iter().position(|v| v == b).unwrap_or(0) as u32;
            }
            bytes.push((value >> 16) as u8);
            if chunk[2] != b'=' {
                bytes.push((value >> 8) as u8);
            }
            if chunk[3] != b'=' {
                bytes.push(value as u8);
            }
        }
        String::from_utf16(
            &bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn terminal_python_paths_are_data_not_powershell_source() {
        let malicious = Path::new("C:\\space ' ‘ ’ “ ” `$(); & 界\\script.py");
        let expression = powershell_path_expression(malicious).unwrap();
        let encoded = expression.split('\'').nth(1).unwrap();
        assert_eq!(decode(encoded), malicious.to_string_lossy());
        let command = python_command(
            Path::new("C:\\Python\\python.exe"),
            malicious,
            Path::new("C:\\work [1]"),
        )
        .unwrap();
        assert!(!command.contains("Invoke-Expression"));
        assert!(!command.contains("script.py"));
        assert!(command.contains("-LiteralPath"));
        assert!(command.contains("'-u'"));
        assert_eq!(decode(&encode_powershell_command(&command)), command);
        assert!(python_command(Path::new("pythonw.exe"), malicious, Path::new(".")).is_err());
    }

    #[test]
    fn terminal_windows_argument_quoting() {
        let quote =
            |s: &str| String::from_utf16(&quote_windows_argument(OsStr::new(s)).unwrap()).unwrap();
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote("a b\\"), "\"a b\\\\\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert!(quote_windows_argument(OsStr::new("a\0b")).is_err());
    }

    #[test]
    fn terminal_no_profile_is_explicit_recovery() {
        let request = LaunchRequest::shell(PathBuf::from("."));
        assert!(matches!(
            request,
            LaunchRequest::Shell {
                no_profile: false,
                ..
            }
        ));
        assert!(matches!(
            request.without_profile(),
            LaunchRequest::Shell {
                no_profile: true,
                ..
            }
        ));
    }

    #[test]
    fn terminal_venv_requires_config_next_to_scripts() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("lightline-venv-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(root.join("Scripts")).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let interpreter = root.join("Scripts").join("python.exe");
        assert_eq!(python_environment(&interpreter, None).unwrap()[1].1, None);
        std::fs::write(root.join("pyvenv.cfg"), "home = C:\\Python\n").unwrap();
        let environment = python_environment(&interpreter, None).unwrap();
        assert_eq!(environment[1].1, Some(root.clone().into_os_string()));
        assert_eq!(
            std::env::split_paths(environment[0].1.as_ref().unwrap()).collect::<Vec<_>>(),
            [root.join("Scripts")]
        );
    }

    #[test]
    fn terminal_unverified_venv_is_not_inherited() {
        let root = std::env::temp_dir().join("lightline-no-venv-config-for-test");
        let interpreter = root.join("python.exe");
        let environment = python_environment(&interpreter, None).unwrap();
        assert_eq!(environment[1], (OsString::from("VIRTUAL_ENV"), None));
        let path = environment[0].1.as_ref().unwrap();
        assert_eq!(
            std::env::split_paths(path).collect::<Vec<_>>(),
            [root.clone(), root.join("Scripts")]
        );
    }
}
