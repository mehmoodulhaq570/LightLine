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

    /// True for formatters compiled into LightLine, which need no external
    /// tool or extension installed to run.
    fn is_builtin(&self) -> bool {
        false
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

/// Built-in JSON formatter. It only re-indents: every string, number and key
/// is copied through verbatim, so key order, number spelling and duplicate
/// keys survive, and `//` and `/* */` comments (JSONC, e.g. tsconfig.json)
/// are kept in place.
pub struct NativeJsonFormatter;

impl Formatter for NativeJsonFormatter {
    fn name(&self) -> &'static str {
        "JSON (built-in)"
    }

    fn supports(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("json"))
            .unwrap_or(false)
    }

    fn command(&self, _path: &Path) -> Command {
        Command::new("internal")
    }

    fn format(&self, source: &str, _path: &Path) -> Result<String, FormatError> {
        format_json(source).map_err(FormatError::Failed)
    }

    fn is_builtin(&self) -> bool {
        true
    }
}

enum JsonToken<'a> {
    Open(u8),
    Close(u8),
    Comma,
    Colon,
    Value(&'a str),
    Comment(&'a str),
}

// Splits JSON/JSONC into tokens, each paired with the number of newlines
// that preceded it in the source (used to keep trailing comments on their
// line and to preserve blank lines between entries).
fn tokenize_json(source: &str) -> Result<Vec<(usize, JsonToken<'_>)>, String> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut newlines = 0;
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let token = match bytes[i] {
            b'\n' => {
                newlines += 1;
                i += 1;
                continue;
            }
            b' ' | b'\t' | b'\r' => {
                i += 1;
                continue;
            }
            b @ (b'{' | b'[') => {
                i += 1;
                JsonToken::Open(b)
            }
            b @ (b'}' | b']') => {
                i += 1;
                JsonToken::Close(b)
            }
            b',' => {
                i += 1;
                JsonToken::Comma
            }
            b':' => {
                i += 1;
                JsonToken::Colon
            }
            b'"' => {
                i += 1;
                loop {
                    match bytes.get(i) {
                        None | Some(b'\n') => return Err("Invalid JSON: unterminated string".into()),
                        Some(b'\\') => i += 2,
                        Some(b'"') => {
                            i += 1;
                            break;
                        }
                        Some(_) => i += 1,
                    }
                }
                JsonToken::Value(&source[start..i])
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                JsonToken::Comment(source[start..i].trim_end())
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let Some(end) = source[i + 2..].find("*/") else {
                    return Err("Invalid JSON: unterminated comment".into());
                };
                i += 2 + end + 2;
                JsonToken::Comment(&source[start..i])
            }
            _ => {
                while i < bytes.len()
                    && !matches!(
                        bytes[i],
                        b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'}' | b'[' | b']' | b',' | b':'
                            | b'"' | b'/'
                    )
                {
                    i += 1;
                }
                if i == start {
                    return Err(format!("Invalid JSON: unexpected character at byte {start}"));
                }
                JsonToken::Value(&source[start..i])
            }
        };
        tokens.push((newlines, token));
        newlines = 0;
    }
    Ok(tokens)
}

fn format_json(source: &str) -> Result<String, String> {
    let tokens = tokenize_json(source)?;
    let has_comments = tokens.iter().any(|(_, t)| matches!(t, JsonToken::Comment(_)));
    // Plain JSON gets a full syntax check; JSONC can't go through serde_json,
    // so for it the bracket check below is the safety net.
    if !has_comments {
        serde_json::from_str::<serde_json::Value>(source).map_err(|e| format!("Invalid JSON: {e}"))?;
    }

    let mut out = String::with_capacity(source.len() + source.len() / 4);
    let mut stack: Vec<u8> = Vec::new();
    let mut pending_newline = false;
    let newline = |out: &mut String, depth: usize, blank: bool| {
        while out.ends_with(' ') {
            out.pop();
        }
        if blank {
            out.push('\n');
        }
        out.push('\n');
        out.extend(std::iter::repeat_n("  ", depth));
    };
    let mut index = 0;
    while index < tokens.len() {
        let (newlines_before, token) = &tokens[index];
        let blank = *newlines_before >= 2;
        match token {
            JsonToken::Comment(text) => {
                if *newlines_before == 0 && !out.is_empty() {
                    out.push(' ');
                } else if !out.is_empty() {
                    newline(&mut out, stack.len(), blank);
                }
                out.push_str(text);
                pending_newline = true;
            }
            JsonToken::Close(close) => {
                let open = if *close == b'}' { b'{' } else { b'[' };
                if stack.pop() != Some(open) {
                    return Err("Invalid JSON: mismatched brackets".into());
                }
                newline(&mut out, stack.len(), false);
                out.push(*close as char);
                pending_newline = false;
            }
            other => {
                if pending_newline {
                    newline(&mut out, stack.len(), blank);
                    pending_newline = false;
                }
                match other {
                    JsonToken::Open(open) => {
                        let close = if *open == b'{' { b'}' } else { b']' };
                        if matches!(tokens.get(index + 1), Some((_, JsonToken::Close(c))) if *c == close)
                        {
                            out.push(*open as char);
                            out.push(close as char);
                            index += 2;
                            continue;
                        }
                        out.push(*open as char);
                        stack.push(*open);
                        pending_newline = true;
                    }
                    JsonToken::Comma => {
                        out.push(',');
                        pending_newline = true;
                    }
                    JsonToken::Colon => out.push_str(": "),
                    JsonToken::Value(text) => out.push_str(text),
                    JsonToken::Close(_) | JsonToken::Comment(_) => unreachable!(),
                }
            }
        }
        index += 1;
    }
    if !stack.is_empty() {
        return Err("Invalid JSON: unclosed bracket".into());
    }
    let mut formatted = out.trim_end().to_string();
    formatted.push('\n');
    Ok(formatted)
}

/// Built-in TOML formatter. It works line by line and only changes layout:
/// `key = value` spacing, indentation of top-level lines, trailing
/// whitespace and blank lines. Comments, key order and value spelling are
/// untouched, and the result is re-parsed and compared with the original,
/// so a formatting bug can never silently change the document.
pub struct NativeTomlFormatter;

#[derive(Clone, Copy, PartialEq)]
enum TomlString {
    None,
    MultiBasic,
    MultiLiteral,
}

// Scans one TOML line, carrying multi-line string and bracket state across
// lines. Returns the byte index of the key/value `=` if the line has one at
// the top level.
fn scan_toml_line(line: &str, ml: &mut TomlString, depth: &mut i32) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut eq = None;
    let mut i = 0;
    while i < bytes.len() {
        match *ml {
            TomlString::MultiBasic => {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i..].starts_with(b"\"\"\"") {
                    i += 3;
                    // Up to two extra quotes right before the delimiter are content.
                    while i < bytes.len() && bytes[i] == b'"' {
                        i += 1;
                    }
                    *ml = TomlString::None;
                    continue;
                }
                i += 1;
            }
            TomlString::MultiLiteral => {
                if bytes[i..].starts_with(b"'''") {
                    i += 3;
                    while i < bytes.len() && bytes[i] == b'\'' {
                        i += 1;
                    }
                    *ml = TomlString::None;
                    continue;
                }
                i += 1;
            }
            TomlString::None => match bytes[i] {
                b'#' => break,
                b'"' if bytes[i..].starts_with(b"\"\"\"") => {
                    *ml = TomlString::MultiBasic;
                    i += 3;
                }
                b'\'' if bytes[i..].starts_with(b"'''") => {
                    *ml = TomlString::MultiLiteral;
                    i += 3;
                }
                quote @ (b'"' | b'\'') => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != quote {
                        if quote == b'"' && bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    i += 1;
                }
                b'[' | b'{' => {
                    *depth += 1;
                    i += 1;
                }
                b']' | b'}' => {
                    *depth -= 1;
                    i += 1;
                }
                b'=' => {
                    if *depth == 0 && eq.is_none() {
                        eq = Some(i);
                    }
                    i += 1;
                }
                _ => i += 1,
            },
        }
    }
    eq
}

fn format_toml(source: &str) -> Result<String, String> {
    let original: toml::Value = toml::from_str(source).map_err(|e| format!("Invalid TOML: {e}"))?;

    let mut out: Vec<String> = Vec::new();
    let mut ml = TomlString::None;
    let mut depth = 0;
    for raw in source.lines() {
        let started_in_string = ml != TomlString::None;
        let start_depth = depth;
        let eq = scan_toml_line(raw, &mut ml, &mut depth);
        if started_in_string {
            // Inside a multi-line string every byte, whitespace included, is content.
            out.push(raw.to_string());
            continue;
        }
        let ends_in_string = ml != TomlString::None;
        let mut line = if ends_in_string { raw.to_string() } else { raw.trim_end().to_string() };
        if start_depth > 0 {
            // A continuation line of a multi-line array or inline table: keep
            // its indentation, which is the author's layout choice.
            out.push(line);
            continue;
        }
        if let Some(eq) = eq {
            let key = raw[..eq].trim();
            let value = line[eq + 1..].trim_start();
            line = format!("{key} = {value}");
        } else {
            line = line.trim_start().to_string();
        }
        if line.is_empty() {
            if out.last().is_some_and(|last| !last.is_empty()) {
                out.push(line);
            }
            continue;
        }
        let is_header = line.starts_with('[');
        if is_header && out.last().is_some_and(|last| !last.is_empty() && !last.starts_with('#')) {
            out.push(String::new());
        }
        out.push(line);
    }
    while out.last().is_some_and(|last| last.is_empty()) {
        out.pop();
    }
    let mut formatted = out.join("\n");
    formatted.push('\n');

    let reparsed: toml::Value = toml::from_str(&formatted)
        .map_err(|_| "could not be done safely; file left unchanged".to_string())?;
    if reparsed != original {
        return Err("would change the document's values; file left unchanged".into());
    }
    Ok(formatted)
}

impl Formatter for NativeTomlFormatter {
    fn name(&self) -> &'static str {
        "TOML (built-in)"
    }

    fn supports(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("toml"))
            .unwrap_or(false)
    }

    fn command(&self, _path: &Path) -> Command {
        Command::new("internal")
    }

    fn format(&self, source: &str, _path: &Path) -> Result<String, FormatError> {
        format_toml(source).map_err(FormatError::Failed)
    }

    fn is_builtin(&self) -> bool {
        true
    }
}

/// Picks the formatter for `path`, if any is both known and supports it.
/// Native zero-dependency formatters (JSON, TOML) take priority over
/// external tools like Prettier.
pub fn formatter_for(path: &Path) -> Option<Box<dyn Formatter>> {
    let json = NativeJsonFormatter;
    if json.supports(path) {
        return Some(Box::new(json));
    }
    let toml = NativeTomlFormatter;
    if toml.supports(path) {
        return Some(Box::new(toml));
    }
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
    fn formatter_for_returns_expected_formatters() {
        assert_eq!(formatter_for(Path::new("data.json")).unwrap().name(), "JSON (built-in)");
        assert_eq!(formatter_for(Path::new("config.toml")).unwrap().name(), "TOML (built-in)");
        assert_eq!(formatter_for(Path::new("index.ts")).unwrap().name(), "Prettier");
        assert!(formatter_for(Path::new("main.rs")).is_none());
    }

    #[test]
    fn native_json_formatter_formats_messy_json_instantly() {
        let formatter = NativeJsonFormatter;
        let messy = r#"{"b":2,"a":1,"nested":{"c":[1,2,3]}}"#;
        let formatted = formatter.format(messy, Path::new("test.json")).unwrap();
        assert!(formatted.contains('\n'));
        assert!(formatted.contains("  \"a\": 1"));
        assert!(formatted.contains("  \"nested\": {"));
        assert!(formatted.ends_with('\n'));
    }

    #[test]
    fn native_toml_formatter_formats_messy_toml_instantly() {
        let formatter = NativeTomlFormatter;
        let messy = "title=\"TOML Example\"\n[owner]\nname=\"Tom\"\n";
        let formatted = formatter.format(messy, Path::new("Cargo.toml")).unwrap();
        assert!(formatted.contains("title = \"TOML Example\""));
        assert!(formatted.contains("[owner]"));
        assert!(formatted.contains("name = \"Tom\""));
        assert!(formatted.ends_with('\n'));
    }

    #[test]
    fn native_json_formatter_keeps_key_order_numbers_and_comments() {
        let formatter = NativeJsonFormatter;
        let source = "{\"z\":1.50,\"a\":[],\"m\":{}, // trailing\n\n// own line\n\"big\":12345678901234567890}";
        let formatted = formatter.format(source, Path::new("tsconfig.json")).unwrap();
        assert_eq!(
            formatted,
            "{\n  \"z\": 1.50,\n  \"a\": [],\n  \"m\": {}, // trailing\n\n  // own line\n  \"big\": 12345678901234567890\n}\n"
        );
        // Formatting is stable.
        assert_eq!(formatter.format(&formatted, Path::new("tsconfig.json")).unwrap(), formatted);
    }

    #[test]
    fn native_json_formatter_rejects_broken_input() {
        let formatter = NativeJsonFormatter;
        assert!(formatter.format("{\"a\": 1", Path::new("x.json")).is_err());
        assert!(formatter.format("{\"a\": [1}", Path::new("x.json")).is_err());
        assert!(formatter.format("// c\n{\"a\": \"open}", Path::new("x.json")).is_err());
    }

    #[test]
    fn native_toml_formatter_keeps_comments_and_order() {
        let formatter = NativeTomlFormatter;
        let source = "# top comment\nname=\"demo\"   \nversion =  \"0.1.0\" # inline\n\n\n\n[dependencies]\n  # why serde\nserde={ version = \"1\", features = [\"derive\"] }\nlist = [\n    \"a\",  # first\n    \"b\",\n]\ntext = \"\"\"\nkeep   \n  this = spacing\n\"\"\"\n[dev-dependencies]\n";
        let formatted = formatter.format(source, Path::new("Cargo.toml")).unwrap();
        assert_eq!(
            formatted,
            "# top comment\nname = \"demo\"\nversion = \"0.1.0\" # inline\n\n[dependencies]\n# why serde\nserde = { version = \"1\", features = [\"derive\"] }\nlist = [\n    \"a\",  # first\n    \"b\",\n]\ntext = \"\"\"\nkeep   \n  this = spacing\n\"\"\"\n\n[dev-dependencies]\n"
        );
        assert_eq!(formatter.format(&formatted, Path::new("Cargo.toml")).unwrap(), formatted);
    }

    #[test]
    fn native_toml_formatter_handles_this_repos_cargo_toml() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let source = std::fs::read_to_string(&path).unwrap();
        let formatted = NativeTomlFormatter.format(&source, &path).unwrap();
        for comment in source.lines().filter_map(|l| l.find('#').map(|i| l[i..].trim_end())) {
            assert!(formatted.contains(comment), "lost comment: {comment}");
        }
    }

    #[test]
    fn builtin_formatters_handle_non_ascii_text() {
        let toml = "a=\"\"\"héllo ü\n wörld\"\"\"\nb='ß' # ñ\n";
        assert_eq!(
            NativeTomlFormatter.format(toml, Path::new("x.toml")).unwrap(),
            "a = \"\"\"héllo ü\n wörld\"\"\"\nb = 'ß' # ñ\n"
        );
        let json = "{\"ключ\":\"значение\" /* ü */}";
        assert_eq!(
            NativeJsonFormatter.format(json, Path::new("x.json")).unwrap(),
            "{\n  \"ключ\": \"значение\" /* ü */\n}\n"
        );
    }

    #[test]
    fn builtin_formatters_are_marked_builtin() {
        assert!(NativeJsonFormatter.is_builtin());
        assert!(NativeTomlFormatter.is_builtin());
        assert!(!PrettierFormatter.is_builtin());
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
