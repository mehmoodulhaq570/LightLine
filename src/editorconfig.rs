//! `.editorconfig` files (<https://editorconfig.org>): the rules a project
//! sets for a file's indentation, line endings and whitespace.
//!
//! `rules_for` walks up from the file's folder, reading each `.editorconfig`
//! until one says `root = true`, and applies their matching sections from the
//! outermost file in, so nearer files and later sections win. It is read when
//! a file needs it (opened, saved) and never while typing; the editor caches
//! the result per tab and drops it when the watcher reports a change.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

pub const FILE_NAME: &str = ".editorconfig";

// Bigger files than this aren't project settings; they're skipped.
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndentStyle {
    Space,
    Tab,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndOfLine {
    Lf,
    Crlf,
}

/// What the `.editorconfig` files say for one file. Every field is None when
/// no file sets it (or a file `unset` it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rules {
    pub indent_style: Option<IndentStyle>,
    /// Columns per indent level, with `indent_size = tab` already resolved
    /// to the tab width.
    pub indent_size: Option<usize>,
    pub tab_width: Option<usize>,
    /// `cr` alone isn't supported, so it's left as None.
    pub end_of_line: Option<EndOfLine>,
    pub charset: Option<String>,
    pub insert_final_newline: Option<bool>,
    pub trim_trailing_whitespace: Option<bool>,
    /// The supported properties as the files set them, in a fixed order,
    /// for showing to the user.
    pub shown: Vec<(&'static str, String)>,
    /// The nearest file with a section that matched, and those sections'
    /// headers (e.g. `[*.js]`, `[*]`).
    pub source: Option<(PathBuf, Vec<String>)>,
    /// Every `.editorconfig` read, matched or not: watched for changes.
    pub files: Vec<PathBuf>,
}

impl Rules {
    /// True when no section matched the file.
    pub fn is_empty(&self) -> bool {
        self.source.is_none()
    }

    /// True when the files say how to indent.
    pub fn sets_indent(&self) -> bool {
        self.indent_style.is_some() || self.indent_size.is_some()
    }
}

const SUPPORTED: [&str; 7] = [
    "indent_style",
    "indent_size",
    "tab_width",
    "end_of_line",
    "charset",
    "insert_final_newline",
    "trim_trailing_whitespace",
];

/// One `.editorconfig` file, parsed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ConfigFile {
    pub root: bool,
    /// Each section's header (without brackets) and its `key = value` pairs,
    /// keys lowercase, in file order.
    pub sections: Vec<(String, Vec<(String, String)>)>,
}

/// Reads an `.editorconfig` file's text. Lines that aren't a section, a
/// pair or a comment are ignored, as the format asks.
pub fn parse(text: &str) -> ConfigFile {
    let mut file = ConfigFile::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            if let Some(end) = header.rfind(']') {
                file.sections.push((header[..end].to_owned(), Vec::new()));
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim().to_owned());
        if key.is_empty() || key.len() > 1024 || value.len() > 4096 {
            continue;
        }
        match file.sections.last_mut() {
            Some((_, pairs)) => pairs.push((key, value)),
            None if key == "root" => file.root = value.eq_ignore_ascii_case("true"),
            None => {}
        }
    }
    file
}

/// Whether section header `glob` matches `relative`, the file's path from
/// the `.editorconfig`'s folder with `/` separators. A glob without a `/`
/// matches the file's name in any folder. Case doesn't matter (Windows).
pub fn glob_matches(glob: &str, relative: &str) -> bool {
    let pattern = if glob.contains('/') {
        glob.strip_prefix('/').unwrap_or(glob).to_owned()
    } else {
        format!("**/{glob}")
    };
    let Some(body) = glob_to_regex(&pattern) else {
        return false;
    };
    // `**/` may also match nothing: `**/x` matches `x` at the top.
    let body = body.replace(".*/", "(?:.*/)?");
    Regex::new(&format!("(?is)^{body}$")).is_ok_and(|regex| regex.is_match(relative))
}

// The regular expression for an EditorConfig glob: `*` (not across `/`),
// `**`, `?`, `[set]`, `[!set]`, `{a,b}`, `{1..9}` and `\` escapes. None
// when it can't be read.
fn glob_to_regex(glob: &str) -> Option<String> {
    let chars: Vec<char> = glob.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                out.push_str(".*");
                i += 2;
                continue;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '\\' if i + 1 < chars.len() => {
                i += 1;
                out.push_str(&regex::escape(&chars[i].to_string()));
            }
            '[' => match chars[i + 1..].iter().position(|&c| c == ']') {
                Some(length) if length > 0 => {
                    let set: String = chars[i + 1..i + 1 + length].iter().collect();
                    let (negate, set) = match set.strip_prefix('!') {
                        Some(rest) => (true, rest.to_owned()),
                        None => (false, set),
                    };
                    let set = set.replace('\\', "\\\\").replace('[', "\\[");
                    out.push_str(&format!("[{}{set}]", if negate { "^" } else { "" }));
                    i += length + 2;
                    continue;
                }
                _ => out.push_str("\\["),
            },
            '{' => {
                let close = matching_brace(&chars, i);
                match close {
                    Some(close) => {
                        let inner: String = chars[i + 1..close].iter().collect();
                        out.push_str(&braces(&inner)?);
                        i = close + 1;
                        continue;
                    }
                    None => out.push_str("\\{"),
                }
            }
            c => out.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    Some(out)
}

fn matching_brace(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut i = open;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

// `{a,b}` → `(?:a|b)`; `{1..9}` → the numbers; `{word}` stays literal.
fn braces(inner: &str) -> Option<String> {
    if let Some((from, to)) = inner.split_once("..")
        && let (Ok(from), Ok(to)) = (from.parse::<i64>(), to.parse::<i64>())
    {
        let (low, high) = (from.min(to), from.max(to));
        if high - low > 10_000 {
            return None;
        }
        let numbers: Vec<String> = (low..=high).map(|n| n.to_string()).collect();
        return Some(format!("(?:{})", numbers.join("|")));
    }
    // Split on top-level commas only.
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0, 0);
    let chars: Vec<char> = inner.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(chars[start..i].iter().collect::<String>());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if parts.is_empty() {
        return Some(regex::escape(&format!("{{{inner}}}")));
    }
    parts.push(chars[start..].iter().collect());
    let alternatives: Option<Vec<String>> = parts.iter().map(|part| glob_to_regex(part)).collect();
    Some(format!("(?:{})", alternatives?.join("|")))
}

/// The rules for `path`, read from the `.editorconfig` files in its folder
/// and the folders above it.
pub fn rules_for(path: &Path) -> Rules {
    let mut found: Vec<(PathBuf, ConfigFile)> = Vec::new();
    for dir in path.ancestors().skip(1) {
        let file = dir.join(FILE_NAME);
        let Ok(metadata) = fs::metadata(&file) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let config = parse(&text);
        let root = config.root;
        found.push((file, config));
        if root {
            break;
        }
    }
    resolve(path, &found)
}

/// The rules `files` (nearest first, each with its parsed contents) give
/// `path`.
pub fn resolve(path: &Path, files: &[(PathBuf, ConfigFile)]) -> Rules {
    let mut values: Vec<(String, String)> = Vec::new();
    let mut source = None;
    for (file, config) in files.iter().rev() {
        let Some(relative) = file
            .parent()
            .and_then(|dir| path.strip_prefix(dir).ok())
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        else {
            continue;
        };
        let mut matched = Vec::new();
        for (glob, pairs) in &config.sections {
            if !glob_matches(glob, &relative) {
                continue;
            }
            matched.push(format!("[{glob}]"));
            for (key, value) in pairs {
                values.retain(|(known, _)| known != key);
                values.push((key.clone(), value.clone()));
            }
        }
        if !matched.is_empty() {
            // Nearest file wins; headers listed most specific (last) first.
            matched.reverse();
            source = Some((file.clone(), matched));
        }
    }
    let get = |key: &str| {
        values
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, value)| value.to_ascii_lowercase())
            .filter(|value| value != "unset")
    };
    let number = |key: &str| {
        get(key)
            .and_then(|value| value.parse::<usize>().ok())
            .map(|n| n.clamp(1, 16))
    };
    let flag = |key: &str| match get(key).as_deref() {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    };
    let indent_style = match get("indent_style").as_deref() {
        Some("space") => Some(IndentStyle::Space),
        Some("tab") => Some(IndentStyle::Tab),
        _ => None,
    };
    let tab_width = number("tab_width").or_else(|| number("indent_size"));
    let indent_size = match get("indent_size").as_deref() {
        Some("tab") => tab_width,
        Some(_) => number("indent_size"),
        None if indent_style == Some(IndentStyle::Tab) => tab_width,
        None => None,
    };
    let end_of_line = match get("end_of_line").as_deref() {
        Some("lf") => Some(EndOfLine::Lf),
        Some("crlf") => Some(EndOfLine::Crlf),
        _ => None,
    };
    let shown = SUPPORTED
        .iter()
        .filter_map(|&key| get(key).map(|value| (key, value)))
        .collect();
    Rules {
        indent_style,
        indent_size,
        tab_width,
        end_of_line,
        charset: get("charset"),
        insert_final_newline: flag("insert_final_newline"),
        trim_trailing_whitespace: flag("trim_trailing_whitespace"),
        shown,
        source,
        files: files.iter().map(|(file, _)| file.clone()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_root_sections_and_comments() {
        let file = parse(
            "# top\nroot = true\n\n[*]\nindent_style = space\n; note\nIndent_Size=2\n\n[*.md]\ntrim_trailing_whitespace = false\nnot a pair\n",
        );
        assert!(file.root);
        assert_eq!(
            file.sections,
            vec![
                (
                    "*".to_owned(),
                    vec![
                        ("indent_style".into(), "space".into()),
                        ("indent_size".into(), "2".into())
                    ]
                ),
                (
                    "*.md".to_owned(),
                    vec![("trim_trailing_whitespace".into(), "false".into())]
                ),
            ]
        );
    }

    #[test]
    fn globs_match_like_editorconfig() {
        assert!(glob_matches("*", "src/app.js"));
        assert!(glob_matches("*.js", "src/app.js"));
        assert!(glob_matches("*.js", "app.js"));
        assert!(!glob_matches("*.js", "app.json"));
        assert!(glob_matches("*.{js,ts}", "src/deep/x.ts"));
        assert!(!glob_matches("*.{js,ts}", "x.rs"));
        assert!(glob_matches("lib/*.py", "lib/a.py"));
        assert!(!glob_matches("lib/*.py", "lib/sub/a.py"));
        assert!(!glob_matches("lib/*.py", "src/lib/a.py"));
        assert!(glob_matches("/lib/**.py", "lib/sub/a.py"));
        assert!(glob_matches("{Makefile,*.mk}", "build/Makefile"));
        assert!(glob_matches("file[0-9].txt", "file7.txt"));
        assert!(!glob_matches("file[!0-9].txt", "file7.txt"));
        assert!(glob_matches("v{1..3}.txt", "v2.txt"));
        assert!(!glob_matches("v{1..3}.txt", "v4.txt"));
        assert!(glob_matches("a?c", "abc"));
        assert!(glob_matches("*.JS", "app.js"));
        assert!(glob_matches("{single}", "{single}"));
    }

    fn config(text: &str) -> ConfigFile {
        parse(text)
    }

    #[test]
    fn nearer_files_and_later_sections_win() {
        let files = vec![
            (
                PathBuf::from("/p/web/.editorconfig"),
                config("[*.js]\nindent_size = 2\n"),
            ),
            (
                PathBuf::from("/p/.editorconfig"),
                config(
                    "root = true\n[*]\nindent_style = space\nindent_size = 4\nend_of_line = lf\n[*.js]\ninsert_final_newline = true\n",
                ),
            ),
        ];
        let rules = resolve(Path::new("/p/web/src/app.js"), &files);
        assert_eq!(rules.indent_style, Some(IndentStyle::Space));
        assert_eq!(rules.indent_size, Some(2));
        assert_eq!(rules.tab_width, Some(2));
        assert_eq!(rules.end_of_line, Some(EndOfLine::Lf));
        assert_eq!(rules.insert_final_newline, Some(true));
        assert_eq!(rules.trim_trailing_whitespace, None);
        let (file, headers) = rules.source.clone().unwrap();
        assert_eq!(file, PathBuf::from("/p/web/.editorconfig"));
        assert_eq!(headers, vec!["[*.js]".to_owned()]);
        assert_eq!(
            rules.shown,
            vec![
                ("indent_style", "space".into()),
                ("indent_size", "2".into()),
                ("end_of_line", "lf".into()),
                ("insert_final_newline", "true".into())
            ]
        );
        // A file no section matches gets nothing.
        let other = resolve(Path::new("/elsewhere/a.js"), &files);
        assert!(other.is_empty() && !other.sets_indent());
    }

    #[test]
    fn tabs_and_unset() {
        let files = vec![(
            PathBuf::from("/p/.editorconfig"),
            config(
                "[*]\nindent_style = tab\ntab_width = 8\ntrim_trailing_whitespace = true\n[*.md]\ntrim_trailing_whitespace = unset\n",
            ),
        )];
        let rules = resolve(Path::new("/p/main.go"), &files);
        assert_eq!(rules.indent_style, Some(IndentStyle::Tab));
        // indent_size defaults to the tab width with tabs.
        assert_eq!(rules.indent_size, Some(8));
        assert_eq!(rules.trim_trailing_whitespace, Some(true));
        assert_eq!(
            resolve(Path::new("/p/README.md"), &files).trim_trailing_whitespace,
            None
        );
        // indent_size = tab means the tab width; sizes are kept to 1..=16.
        let files = vec![(
            PathBuf::from("/p/.editorconfig"),
            config("[*]\nindent_size = tab\ntab_width = 40\nend_of_line = cr\n"),
        )];
        let rules = resolve(Path::new("/p/x.c"), &files);
        assert_eq!(rules.indent_size, Some(16));
        assert_eq!(rules.end_of_line, None);
    }

    #[test]
    fn rules_for_reads_up_to_the_root_file() {
        let base = std::env::temp_dir().join(format!("ll-editorconfig-{}", std::process::id()));
        let inner = base.join("project").join("src");
        fs::create_dir_all(&inner).unwrap();
        fs::write(
            base.join(FILE_NAME),
            "[*]\nindent_size = 8\ncharset = latin1\n",
        )
        .unwrap();
        fs::write(
            base.join("project").join(FILE_NAME),
            "root = true\n[*.rs]\nindent_size = 4\n",
        )
        .unwrap();
        let rules = rules_for(&inner.join("main.rs"));
        // The outer file is past `root = true`: its charset isn't read.
        assert_eq!(rules.indent_size, Some(4));
        assert_eq!(rules.charset, None);
        assert_eq!(rules.files, vec![base.join("project").join(FILE_NAME)]);
        assert!(rules_for(&inner.join("notes.txt")).is_empty());
        fs::remove_dir_all(&base).unwrap();
    }
}
