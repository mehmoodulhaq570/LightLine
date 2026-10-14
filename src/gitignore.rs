//! The files Git ignores, so Quick Open and search leave out build output,
//! logs and the like. Follows the rules of gitignore(5): each folder's
//! `.gitignore` applies below it, the repository's `.git/info/exclude` to all
//! of it, a later pattern wins over an earlier one and a deeper file over its
//! parents, `!` brings a file back, a trailing `/` matches only folders, and
//! a pattern with a `/` before its end is relative to its `.gitignore`.
//! Matching ignores case, as Git does on Windows. The user's own excludes
//! file (core.excludesFile, by default `~/.config/git/ignore`) applies too.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Byte(u8),
    // `?`: any one character but `/`.
    One,
    // `*`: any run of characters without `/`.
    Star,
    // `**/` at the start or `/**/` in the middle: any number of whole
    // folders, including none.
    Folders,
    // `/**` at the end: everything inside.
    Inside,
    // `[...]`: the byte ranges it allows, and whether it's negated.
    Class(Vec<(u8, u8)>, bool),
}

#[derive(Debug, Clone)]
struct Pattern {
    tokens: Vec<Token>,
    negated: bool,
    folders_only: bool,
    // Whether it's matched against the path from its `.gitignore`'s folder
    // rather than against the name alone.
    anchored: bool,
}

impl Pattern {
    fn parse(line: &str) -> Option<Self> {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let mut text = trim_unescaped_spaces(line);
        let negated = text.starts_with('!');
        if negated {
            text = &text[1..];
        }
        // `\#` and `\!` stand for themselves.
        if text.starts_with("\\#") || text.starts_with("\\!") {
            text = &text[1..];
        }
        let folders_only = text.ends_with('/') && !text.ends_with("\\/");
        let text = text.trim_end_matches('/');
        if text.is_empty() {
            return None;
        }
        let anchored = text.contains('/');
        let text = text.strip_prefix('/').unwrap_or(text);
        Some(Self {
            tokens: tokenize(&text.to_ascii_lowercase()),
            negated,
            folders_only,
            anchored,
        })
    }

    // `path` is relative to the pattern's `.gitignore`, with `/` between
    // folders, in lowercase; `name` is its last part.
    fn matches(&self, path: &str, name: &str, is_dir: bool) -> bool {
        if self.folders_only && !is_dir {
            return false;
        }
        let subject = if self.anchored { path } else { name };
        glob(&self.tokens, subject.as_bytes())
    }
}

// Trailing spaces are dropped unless the last one is escaped with `\`.
fn trim_unescaped_spaces(line: &str) -> &str {
    let trimmed = line.trim_end_matches(' ');
    if trimmed.ends_with('\\') && trimmed.len() < line.len() {
        &line[..trimmed.len() + 1]
    } else {
        trimmed
    }
}

fn tokenize(pattern: &str) -> Vec<Token> {
    let bytes = pattern.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let at_segment_start = i == 0 || bytes[i - 1] == b'/';
        match bytes[i] {
            b'*' if bytes.get(i + 1) == Some(&b'*') && at_segment_start => {
                let after = bytes.get(i + 2).copied();
                if after == Some(b'/') {
                    tokens.push(Token::Folders);
                    i += 3;
                    continue;
                }
                if after.is_none() {
                    if tokens.last() == Some(&Token::Byte(b'/')) {
                        tokens.pop();
                        tokens.push(Token::Inside);
                    } else {
                        // A lone `**` matches everything.
                        tokens.push(Token::Folders);
                        tokens.push(Token::Star);
                    }
                    i += 2;
                    continue;
                }
                tokens.push(Token::Star);
                i += 2;
            }
            b'*' => {
                if tokens.last() != Some(&Token::Star) {
                    tokens.push(Token::Star);
                }
                i += 1;
            }
            b'?' => {
                tokens.push(Token::One);
                i += 1;
            }
            b'[' => match class(&bytes[i..]) {
                Some((token, used)) => {
                    tokens.push(token);
                    i += used;
                }
                None => {
                    tokens.push(Token::Byte(b'['));
                    i += 1;
                }
            },
            b'\\' if i + 1 < bytes.len() => {
                tokens.push(Token::Byte(bytes[i + 1]));
                i += 2;
            }
            byte => {
                tokens.push(Token::Byte(byte));
                i += 1;
            }
        }
    }
    tokens
}

// A `[...]` class at the start of `bytes`, and how many bytes it took.
fn class(bytes: &[u8]) -> Option<(Token, usize)> {
    let mut i = 1;
    let negated = matches!(bytes.get(i), Some(b'!' | b'^'));
    if negated {
        i += 1;
    }
    let mut ranges = Vec::new();
    let mut first = true;
    loop {
        let byte = *bytes.get(i)?;
        if byte == b']' && !first {
            return Some((Token::Class(ranges, negated), i + 1));
        }
        first = false;
        let byte = if byte == b'\\' {
            i += 1;
            *bytes.get(i)?
        } else {
            byte
        };
        if bytes.get(i + 1) == Some(&b'-') && bytes.get(i + 2).is_some_and(|&end| end != b']') {
            ranges.push((byte, bytes[i + 2]));
            i += 3;
        } else {
            ranges.push((byte, byte));
            i += 1;
        }
    }
}

fn glob(tokens: &[Token], text: &[u8]) -> bool {
    let Some((token, rest)) = tokens.split_first() else {
        return text.is_empty();
    };
    match token {
        Token::Byte(byte) => text.first() == Some(byte) && glob(rest, &text[1..]),
        Token::One => text.first().is_some_and(|&b| b != b'/') && glob(rest, &text[1..]),
        Token::Class(ranges, negated) => {
            text.first().is_some_and(|&b| {
                b != b'/' && ranges.iter().any(|&(low, high)| low <= b && b <= high) != *negated
            }) && glob(rest, &text[1..])
        }
        Token::Star => {
            let mut i = 0;
            loop {
                if glob(rest, &text[i..]) {
                    return true;
                }
                if i == text.len() || text[i] == b'/' {
                    return false;
                }
                i += 1;
            }
        }
        Token::Folders => {
            // None, or up to and including any later `/`.
            if glob(rest, text) {
                return true;
            }
            text.iter()
                .enumerate()
                .filter(|&(_, &b)| b == b'/')
                .any(|(slash, _)| glob(rest, &text[slash + 1..]))
        }
        Token::Inside => text.first() == Some(&b'/') && text.len() > 1,
    }
}

/// The patterns of one ignore file, and the folder they apply below
/// (relative to the root, "" for the root itself, else ending in `/`).
#[derive(Debug)]
struct RuleSet {
    base: String,
    patterns: Vec<Pattern>,
}

/// The rules in force in one folder of the walk: its own and its parents'.
#[derive(Debug, Clone, Default)]
pub struct Rules {
    sets: Vec<Arc<RuleSet>>,
}

impl Rules {
    /// The rules at the top of a walk of `root`: the user's excludes file,
    /// the repository's `.git/info/exclude` and `root`'s own `.gitignore`,
    /// each winning over the one before.
    pub fn for_root(root: &Path) -> Self {
        let mut rules = Self::default();
        if let Some(text) = user_excludes_file().and_then(|path| fs::read_to_string(path).ok()) {
            rules.add(String::new(), &text);
        }
        if let Ok(text) = fs::read_to_string(root.join(".git").join("info").join("exclude")) {
            rules.add(String::new(), &text);
        }
        rules.within(root, "")
    }

    /// The rules for folder `dir`, at `relative` (from the root, ending in
    /// `/`) below this one: these, then its `.gitignore`, if any.
    pub fn within(&self, dir: &Path, relative: &str) -> Self {
        let mut rules = self.clone();
        if let Ok(text) = fs::read_to_string(dir.join(".gitignore")) {
            rules.add(relative.to_string(), &text);
        }
        rules
    }

    fn add(&mut self, base: String, text: &str) {
        let patterns: Vec<Pattern> = text.lines().filter_map(Pattern::parse).collect();
        if !patterns.is_empty() {
            self.sets.push(Arc::new(RuleSet {
                base: base.to_ascii_lowercase(),
                patterns,
            }));
        }
    }

    /// Whether the entry at `relative` (from the root, `/` between folders)
    /// is ignored. Its parent folders are taken to be kept: the walk doesn't
    /// go into an ignored folder.
    pub fn ignores(&self, relative: &str, is_dir: bool) -> bool {
        if self.sets.is_empty() {
            return false;
        }
        let relative = relative.to_ascii_lowercase();
        let name = relative.rsplit('/').next().unwrap_or(&relative);
        let mut ignored = false;
        for set in &self.sets {
            let Some(path) = relative.strip_prefix(set.base.as_str()) else {
                continue;
            };
            for pattern in &set.patterns {
                if pattern.negated == ignored && pattern.matches(path, name, is_dir) {
                    ignored = !pattern.negated;
                }
            }
        }
        ignored
    }
}

/// The user's excludes file: core.excludesFile from `~/.gitconfig`, else
/// Git's default, `$XDG_CONFIG_HOME/git/ignore` or `~/.config/git/ignore`.
fn user_excludes_file() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)?;
    let configured = fs::read_to_string(home.join(".gitconfig"))
        .ok()
        .and_then(|config| excludes_file_setting(&config))
        .map(|path| match path.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(path),
        });
    configured.or_else(|| {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        Some(config.join("git").join("ignore"))
    })
}

// `excludesfile` in the `[core]` section of a Git config file.
fn excludes_file_setting(config: &str) -> Option<String> {
    let mut in_core = false;
    let mut found = None;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_core = line
                .trim_matches(['[', ']'])
                .trim()
                .eq_ignore_ascii_case("core");
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if in_core && key.trim().eq_ignore_ascii_case("excludesfile") {
            let value = value.trim().trim_matches('"').replace("\\\\", "\\");
            if !value.is_empty() {
                found = Some(value);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(text: &str) -> Rules {
        let mut rules = Rules::default();
        rules.add(String::new(), text);
        rules
    }

    #[test]
    fn names_match_at_any_depth_and_slashes_anchor() {
        let r = rules("*.log\nbuild/\n/dist\ndocs/*.tmp\n");
        assert!(r.ignores("app.log", false));
        assert!(r.ignores("src/deep/app.LOG", false), "case is ignored");
        assert!(r.ignores("build", true));
        assert!(r.ignores("src/build", true));
        assert!(!r.ignores("build", false), "build/ matches folders only");
        assert!(r.ignores("dist", true));
        assert!(!r.ignores("src/dist", true), "/dist is only at the top");
        assert!(r.ignores("docs/a.tmp", false));
        assert!(!r.ignores("src/docs/a.tmp", false));
        assert!(!r.ignores("docs/sub/a.tmp", false), "* doesn't cross /");
        assert!(!r.ignores("main.rs", false));
    }

    #[test]
    fn negation_and_order() {
        let r = rules("*.env\n!example.env\n");
        assert!(r.ignores(".env", false));
        assert!(r.ignores("prod.env", false));
        assert!(!r.ignores("example.env", false));
        let r = rules("!keep.txt\n*.txt\n");
        assert!(r.ignores("keep.txt", false), "the later pattern wins");
    }

    #[test]
    fn double_stars() {
        let r = rules("**/cache\nlogs/**\na/**/z.txt\n");
        assert!(r.ignores("cache", true));
        assert!(r.ignores("x/y/cache", false));
        assert!(r.ignores("logs/today.txt", false));
        assert!(r.ignores("logs/2026/today.txt", false));
        assert!(!r.ignores("logs", true), "logs/** is what's inside");
        assert!(r.ignores("a/z.txt", false));
        assert!(r.ignores("a/b/c/z.txt", false));
        assert!(!r.ignores("b/a/z.txt", false));
    }

    #[test]
    fn classes_escapes_and_comments() {
        let r = rules("# comment\n\nfile[0-9].txt\n\\#hash\nspace\\ \n?.o\n[!a]x\n");
        assert!(r.ignores("file7.txt", false));
        assert!(!r.ignores("filex.txt", false));
        assert!(r.ignores("#hash", false));
        assert!(r.ignores("space ", false));
        assert!(r.ignores("a.o", false));
        assert!(!r.ignores("ab.o", false));
        assert!(r.ignores("bx", false));
        assert!(!r.ignores("ax", false));
        assert!(!r.ignores("# comment", false));
    }

    #[test]
    fn the_excludes_file_comes_from_the_core_section() {
        // Git config doubles backslashes inside quotes.
        let config = r#"[user]
	name = x
[core]
	autocrlf = true
	excludesFile = "C:\\Users\\me\\.gitignore_global"
"#;
        assert_eq!(
            excludes_file_setting(config).as_deref(),
            Some(r"C:\Users\me\.gitignore_global")
        );
        assert_eq!(excludes_file_setting("[user]\nexcludesfile = x\n"), None);
    }

    #[test]
    fn deeper_files_apply_below_their_folder_and_win() {
        let mut r = rules("*.gen\n");
        r.add("tools/".into(), "!keep.gen\nlocal/\n");
        assert!(r.ignores("x.gen", false));
        assert!(r.ignores("src/keep.gen", false));
        assert!(!r.ignores("tools/keep.gen", false));
        assert!(r.ignores("tools/local", true));
        assert!(
            !r.ignores("local", true),
            "tools/.gitignore applies only below tools/"
        );
    }
}
