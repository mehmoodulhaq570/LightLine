//! How a language from an extension is edited: its comment tokens, the
//! brackets and quotes typed in pairs, and when Enter indents or a typed
//! word outdents. Read from the language's `config.toml` (Zed's language
//! config) when the installed languages are listed, so typing only looks at
//! what is already parsed.
//!
//! The keys LightLine reads, as Zed's own language extensions write them
//! (zed-extensions/lua, zig, elixir, toml):
//!
//! ```toml
//! line_comments = ["-- ", "--- "]
//! block_comment = ["--[[ ", " ]]"]       # or { start = "/*", end = "*/" }
//! autoclose_before = ";:.,=}])>"
//! brackets = [
//!     { start = "{", end = "}", close = true, newline = true },
//!     { start = "\"", end = "\"", close = true, newline = false, not_in = ["string"] },
//! ]
//! increase_indent_pattern = "^\\s*(if|do|function)\\b"
//! decrease_indent_pattern = "^\\s*(else|end)\\b"
//! tab_size = 2
//! hard_tabs = false
//! ```
//!
//! A language without `brackets` keeps LightLine's own pairs, matching and
//! Enter rule; one without comment tokens has nothing to toggle.

use regex::Regex;
use serde::Deserialize;

/// `block_comment`: the old two-string form or the newer table.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum BlockCommentEntry {
    Pair(Vec<String>),
    Table { start: String, end: String },
}

/// One entry of `brackets`.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BracketEntry {
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub close: bool,
    #[serde(default)]
    pub newline: bool,
    #[serde(default)]
    pub not_in: Vec<String>,
}

/// A pair typed together: typing `open` also types `close` after the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoPair {
    pub open: char,
    pub close: char,
    /// Not inside a string or a comment (`not_in`).
    pub not_in_string: bool,
    pub not_in_comment: bool,
}

/// The editing rules of one language.
#[derive(Debug, Clone, Default)]
pub struct EditingRules {
    /// Line comment prefixes; the first is the one Toggle Comment types.
    pub line_comments: Vec<String>,
    pub block_comment: Option<(String, String)>,
    /// None: the language lists no brackets, so LightLine's own apply.
    pub brackets: Option<Brackets>,
    pub increase_indent: Option<Regex>,
    pub decrease_indent: Option<Regex>,
    pub tab_size: Option<usize>,
    pub hard_tabs: Option<bool>,
}

/// The `brackets` a language lists, in the forms the editor uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Brackets {
    /// Typed in pairs (`close = true`), one character each.
    pub auto_pairs: Vec<AutoPair>,
    /// After the caret, a pair is only closed before whitespace, the end of
    /// the line, or one of these.
    pub autoclose_before: String,
    /// Matched and colored by depth: which of `()`, `[]` and `{}` the
    /// language lists.
    pub matched: Vec<(char, char)>,
    /// Enter after one of these indents the new line (`newline = true`).
    pub indent_after: Vec<char>,
}

fn single(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
}

impl EditingRules {
    /// Builds the rules from a language config's fields. Patterns that
    /// aren't valid regular expressions are left out.
    #[allow(clippy::too_many_arguments)]
    pub fn from_config(
        line_comments: &[String],
        block_comment: Option<&BlockCommentEntry>,
        brackets: Option<&[BracketEntry]>,
        autoclose_before: &str,
        increase_indent_pattern: Option<&str>,
        decrease_indent_pattern: Option<&str>,
        tab_size: Option<u32>,
        hard_tabs: Option<bool>,
    ) -> Self {
        let line_comments = line_comments
            .iter()
            .filter(|prefix| !prefix.trim().is_empty())
            .cloned()
            .collect();
        let block_comment = block_comment.and_then(|entry| match entry {
            BlockCommentEntry::Pair(pair) if pair.len() == 2 => {
                Some((pair[0].clone(), pair[1].clone()))
            }
            BlockCommentEntry::Pair(_) => None,
            BlockCommentEntry::Table { start, end } => Some((start.clone(), end.clone())),
        });
        let block_comment =
            block_comment.filter(|(start, end)| !start.trim().is_empty() && !end.trim().is_empty());
        let brackets = brackets.map(|entries| {
            let mut set = Brackets {
                autoclose_before: autoclose_before.to_string(),
                ..Brackets::default()
            };
            for entry in entries {
                let (Some(open), Some(close)) = (single(&entry.start), single(&entry.end)) else {
                    continue;
                };
                if entry.close {
                    set.auto_pairs.push(AutoPair {
                        open,
                        close,
                        not_in_string: entry.not_in.iter().any(|scope| scope.contains("string")),
                        not_in_comment: entry.not_in.iter().any(|scope| scope.contains("comment")),
                    });
                }
                // Matching works on the text, not the syntax tree, so `<` and
                // `>` (also comparisons) are left out.
                if matches!((open, close), ('(', ')') | ('[', ']') | ('{', '}')) {
                    set.matched.push((open, close));
                    if entry.newline {
                        set.indent_after.push(open);
                    }
                }
            }
            set
        });
        let pattern = |text: Option<&str>| text.and_then(|text| Regex::new(text).ok());
        Self {
            line_comments,
            block_comment,
            brackets,
            increase_indent: pattern(increase_indent_pattern),
            decrease_indent: pattern(decrease_indent_pattern),
            tab_size: tab_size
                .filter(|size| (1..=16).contains(size))
                .map(|size| size as usize),
            hard_tabs,
        }
    }

    /// The pair typing `ch` opens, if the language closes it.
    pub fn auto_pair(&self, ch: char) -> Option<&AutoPair> {
        self.brackets
            .as_ref()?
            .auto_pairs
            .iter()
            .find(|pair| pair.open == ch)
    }

    /// True when `ch` closes one of the language's pairs, so typing it on
    /// the same character steps over it.
    pub fn closes(&self, ch: char) -> bool {
        self.brackets
            .as_ref()
            .is_some_and(|brackets| brackets.auto_pairs.iter().any(|pair| pair.close == ch))
    }

    /// Whether Enter after `before` (the text before the caret on its line)
    /// starts the new line one level deeper. None: the language has no rule
    /// of its own, so LightLine's applies.
    pub fn indents_after(&self, before: &str) -> Option<bool> {
        if self
            .increase_indent
            .as_ref()
            .is_some_and(|pattern| pattern.is_match(before))
        {
            return Some(true);
        }
        let brackets = self.brackets.as_ref()?;
        let last = before.trim_end().chars().last();
        Some(last.is_some_and(|ch| brackets.indent_after.contains(&ch)))
    }

    /// True when `line` should sit one level shallower than the block it
    /// is in (`end`, `else`...).
    pub fn outdents(&self, line: &str) -> bool {
        self.decrease_indent
            .as_ref()
            .is_some_and(|pattern| pattern.is_match(line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(start: &str, end: &str, close: bool, newline: bool, not_in: &[&str]) -> BracketEntry {
        BracketEntry {
            start: start.into(),
            end: end.into(),
            close,
            newline,
            not_in: not_in.iter().map(|scope| scope.to_string()).collect(),
        }
    }

    #[test]
    fn brackets_split_into_pairs_matching_and_indent() {
        let entries = [
            entry("{", "}", true, true, &[]),
            entry("(", ")", true, false, &[]),
            entry("\"", "\"", true, false, &["string"]),
            entry("'", "'", true, false, &["comment", "string"]),
            entry("<<", ">>", false, true, &[]),
            entry("<", ">", false, true, &[]),
            entry("\"\"\"", "\n\"\"\"", true, true, &["string"]),
        ];
        let rules = EditingRules::from_config(
            &[],
            None,
            Some(&entries),
            ";:.,=}])>",
            None,
            None,
            None,
            None,
        );
        let brackets = rules.brackets.as_ref().unwrap();
        assert_eq!(brackets.matched, [('{', '}'), ('(', ')')]);
        assert_eq!(brackets.indent_after, ['{']);
        assert_eq!(brackets.auto_pairs.len(), 4);
        let quote = rules.auto_pair('"').unwrap();
        assert!(quote.not_in_string && !quote.not_in_comment);
        let single = rules.auto_pair('\'').unwrap();
        assert!(single.not_in_string && single.not_in_comment);
        assert!(rules.auto_pair('[').is_none());
        assert!(rules.closes('}') && rules.closes('"') && !rules.closes(']'));
    }

    #[test]
    fn enter_indents_after_newline_brackets_or_the_pattern() {
        let entries = [
            entry("{", "}", true, true, &[]),
            entry("(", ")", true, false, &[]),
        ];
        let rules = EditingRules::from_config(
            &[],
            None,
            Some(&entries),
            "",
            Some(r"^\s*(if|elseif|else|do|while|for|repeat|((local|global)\s+)?function)\b"),
            Some(r"^\s*(elseif|else|end|until)\b"),
            None,
            None,
        );
        assert_eq!(rules.indents_after("local t = {"), Some(true));
        assert_eq!(rules.indents_after("foo("), Some(false));
        assert_eq!(rules.indents_after("  if x then"), Some(true));
        assert_eq!(rules.indents_after("local function f()"), Some(true));
        assert_eq!(rules.indents_after("print(x)"), Some(false));
        assert!(rules.outdents("    end"));
        assert!(rules.outdents("  else"));
        assert!(!rules.outdents("    ending = 1"));
        assert!(!rules.outdents("    print(x)"));
        // No brackets and no pattern: LightLine's own rule.
        let plain = EditingRules::default();
        assert_eq!(plain.indents_after("x {"), None);
        assert!(!plain.outdents("end"));
    }

    #[test]
    fn comments_and_indentation_settings_are_checked() {
        let rules = EditingRules::from_config(
            &["# ".into(), "  ".into()],
            Some(&BlockCommentEntry::Pair(vec!["/* ".into(), " */".into()])),
            None,
            "",
            Some("("),
            None,
            Some(0),
            Some(true),
        );
        assert_eq!(rules.line_comments, ["# "]);
        assert_eq!(
            rules.block_comment,
            Some(("/* ".to_string(), " */".to_string()))
        );
        // An invalid pattern and a zero tab size are dropped.
        assert!(rules.increase_indent.is_none());
        assert_eq!(rules.tab_size, None);
        assert_eq!(rules.hard_tabs, Some(true));
        let table = EditingRules::from_config(
            &[],
            Some(&BlockCommentEntry::Table {
                start: "/*".into(),
                end: "*/".into(),
            }),
            None,
            "",
            None,
            None,
            Some(2),
            None,
        );
        assert_eq!(table.block_comment, Some(("/*".into(), "*/".into())));
        assert_eq!(table.tab_size, Some(2));
    }
}
