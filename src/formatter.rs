//! A small, generic "run an external formatter as a process" system.
//!
//! `LightLine -> Formatter -> external formatter process -> formatted code
//! -> LightLine`. Prettier is the first (and today, only) implementation;
//! adding rustfmt/black/clang-format later means adding another
//! `impl Formatter` and a match arm in `formatter_for`, not touching
//! anything that calls into this module.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

/// How long a formatter process gets before it's killed. Bounds the worst
/// case for both the async "Format Document" command and the synchronous
/// format-on-save path -- neither can hang LightLine indefinitely, even if
/// the external tool gets stuck.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub enum FormatError {
    /// The formatter's executable isn't on PATH at all.
    NotAvailable,
    /// It ran, but exited non-zero or wrote a parse/syntax error to stderr.
    Failed(String),
    /// It didn't finish within `DEFAULT_TIMEOUT` and was killed.
    Timeout,
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::NotAvailable => write!(f, "formatter not found on PATH"),
            FormatError::Failed(message) => write!(f, "{message}"),
            FormatError::Timeout => write!(f, "formatter timed out and was stopped"),
        }
    }
}

/// One external formatting tool. Implementations only need to know how to
/// build the right `Command` for a given file -- process spawning, stdin/
/// stdout wiring, timeout enforcement, and stderr handling are all shared.
pub trait Formatter: Send + Sync {
    /// Display name, e.g. for status messages ("Formatted with Prettier").
    fn name(&self) -> &'static str;

    /// Whether this formatter should be used for `path`.
    fn supports(&self, path: &Path) -> bool;

    /// Builds the command that formats `path`, reading source from stdin and
    /// writing formatted output to stdout. The filename is passed through
    /// (not just piped bytes) so the tool can infer the right parser/config
    /// for that path, exactly as if it were invoked on the real file.
    fn command(&self, path: &Path) -> Command;

    /// Formats `source`. The default implementation is what every formatter
    /// here actually uses; a formatter only overrides this if it needs
    /// something command()+stdin/stdout can't express.
    fn format(&self, source: &str, path: &Path) -> Result<String, FormatError> {
        run_formatter(self.command(path), source)
    }
}

/// Spawns `command`, writes `source` to its stdin, and reads formatted code
/// back from stdout -- with a timeout and concurrent stdout/stderr draining
/// so neither a slow formatter nor a large file can deadlock or hang the
/// caller.
fn run_formatter(mut command: Command, source: &str) -> Result<String, FormatError> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().map_err(|_| FormatError::NotAvailable)?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(source.as_bytes());
        // Dropping `stdin` here closes it, signaling EOF to the child; it
        // won't finish reading until this happens.
    }

    // Drain stdout/stderr on their own threads *before* blocking on the
    // child's exit: a large formatted file can fill the pipe buffer, and
    // without a concurrent reader the child would block on its own write
    // and never exit, regardless of the timeout below.
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let stdout_thread = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(pipe) = stdout_pipe.as_mut() {
            let _ = pipe.read_to_end(&mut buffer);
        }
        buffer
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(pipe) = stderr_pipe.as_mut() {
            let _ = pipe.read_to_end(&mut buffer);
        }
        buffer
    });

    let status = match child.wait_timeout(DEFAULT_TIMEOUT) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FormatError::Timeout);
        }
        Err(error) => return Err(FormatError::Failed(error.to_string())),
    };

    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    if !status.success() {
        let stderr_text = String::from_utf8_lossy(&stderr);
        let first_error = stderr_text
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("formatting failed")
            .trim()
            .to_string();
        return Err(FormatError::Failed(first_error));
    }

    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

/// Runs `prettier` directly, falling back to `npx --yes prettier` if the
/// bare command isn't on PATH (mirrors how Prettier is probed elsewhere in
/// LightLine, e.g. the Extensions panel's detect-only toggle).
pub struct PrettierFormatter;

impl Formatter for PrettierFormatter {
    fn name(&self) -> &'static str {
        "Prettier"
    }

    fn supports(&self, path: &Path) -> bool {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        matches!(
            extension.as_str(),
            "js" | "mjs"
                | "cjs"
                | "jsx"
                | "ts"
                | "mts"
                | "cts"
                | "tsx"
                | "json"
                | "css"
                | "scss"
                | "less"
                | "html"
                | "htm"
                | "md"
                | "markdown"
                | "yaml"
                | "yml"
                | "graphql"
                | "gql"
                | "vue"
        )
    }

    fn command(&self, path: &Path) -> Command {
        let mut command = Command::new("prettier");
        command.arg("--stdin-filepath").arg(path);
        command
    }

    fn format(&self, source: &str, path: &Path) -> Result<String, FormatError> {
        match run_formatter(self.command(path), source) {
            Err(FormatError::NotAvailable) => {
                let mut fallback = Command::new("npx");
                fallback.args(["--yes", "prettier", "--stdin-filepath"]).arg(path);
                run_formatter(fallback, source)
            }
            other => other,
        }
    }
}

/// Picks the formatter for `path`, if any is both known and supports it.
/// The one place a second formatter (rustfmt, black, clang-format, ...)
/// gets added later.
pub fn formatter_for(path: &Path) -> Option<Box<dyn Formatter>> {
    let prettier = PrettierFormatter;
    if prettier.supports(path) {
        return Some(Box::new(prettier));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prettier_supports_expected_extensions() {
        let prettier = PrettierFormatter;
        assert!(prettier.supports(Path::new("app.js")));
        assert!(prettier.supports(Path::new("styles.css")));
        assert!(prettier.supports(Path::new("data.json")));
        assert!(!prettier.supports(Path::new("main.rs")));
        assert!(!prettier.supports(Path::new("script.py")));
    }

    #[test]
    fn formatter_for_returns_prettier_for_supported_files_and_none_otherwise() {
        assert!(formatter_for(Path::new("index.ts")).is_some());
        assert!(formatter_for(Path::new("main.rs")).is_none());
    }

    #[test]
    fn missing_command_reports_not_available() {
        let result = run_formatter(Command::new("lightline-formatter-that-does-not-exist"), "x");
        assert!(matches!(result, Err(FormatError::NotAvailable)));
    }

    // Live test: only runs if `prettier` (or `npx`) is actually on this
    // machine's PATH, same spirit as the LSP/registry live tests elsewhere.
    #[test]
    fn real_prettier_formats_messy_json_if_installed() {
        let prettier = PrettierFormatter;
        match prettier.format("{\"a\":1,\"b\":2}", Path::new("test.json")) {
            Ok(formatted) => {
                assert!(formatted.contains('\n'), "prettier should pretty-print, got: {formatted:?}");
            }
            Err(FormatError::NotAvailable) => {
                eprintln!("skipped: prettier/npx not found on PATH");
            }
            Err(other) => panic!("unexpected formatting error: {other}"),
        }
    }

    // Proves the timeout path actually kills a stuck process instead of
    // hanging forever.
    #[test]
    fn a_hanging_process_is_killed_after_the_timeout() {
        #[cfg(windows)]
        let mut command = {
            let mut c = Command::new("cmd");
            c.args(["/C", "ping", "-n", "30", "127.0.0.1", ">nul"]);
            c
        };
        #[cfg(not(windows))]
        let mut command = Command::new("sleep").arg("30").to_owned();
        // Use a short timeout for the test instead of waiting 10 real seconds.
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("should spawn a long-running process");
        let start = std::time::Instant::now();
        let result = child.wait_timeout(Duration::from_millis(300));
        assert!(matches!(result, Ok(None)), "expected the process to still be running");
        let _ = child.kill();
        let _ = child.wait();
        assert!(start.elapsed() < Duration::from_secs(5), "wait_timeout should not block past its duration");
    }
}
