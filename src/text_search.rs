//! Matching for Find (Ctrl+F) and the Search panel: plain text or a regular
//! expression, with Match Case and Whole Word. Every mode becomes one
//! `Regex`, which searches a plain string as fast as `str::find` does.
//!
//! Matches are found within one line at a time, like the rest of the
//! editor's find, so a pattern can't match across a line break. Empty
//! matches (`^`, `a*` where there's no `a`) are skipped: there'd be nothing
//! to select or replace.

use regex::{Regex, RegexBuilder};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    pub regex: bool,
    pub match_case: bool,
    pub whole_word: bool,
}

impl Default for SearchOptions {
    // Match Case starts on: before these options, every search matched case.
    fn default() -> Self {
        SearchOptions {
            regex: false,
            match_case: true,
            whole_word: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Matcher {
    regex: Regex,
    // `$1` and `${name}` in the replacement mean groups only in regex mode.
    expands: bool,
    // False when a whole file's text can't rule out a match on one of its
    // lines (`\A` and `\z` mean the start and end of each line).
    whole_text_check: bool,
}

impl Matcher {
    /// Err is a short message for a pattern that isn't a valid regular
    /// expression (or an empty query).
    pub fn new(query: &str, options: SearchOptions) -> Result<Matcher, String> {
        if query.is_empty() {
            return Err("Nothing to find".into());
        }
        let pattern = if options.regex {
            query.to_owned()
        } else {
            regex::escape(query)
        };
        let pattern = if options.whole_word {
            format!(r"\b(?:{pattern})\b")
        } else {
            pattern
        };
        // `^` and `$` at every line break (CRLF included), so a check of a
        // whole file's text agrees with matching its lines one by one.
        let regex = RegexBuilder::new(&pattern)
            .case_insensitive(!options.match_case)
            .multi_line(true)
            .crlf(true)
            .size_limit(1 << 22)
            .build()
            .map_err(|error| match error {
                regex::Error::Syntax(_) => "Invalid regular expression".to_string(),
                _ => "Regular expression too large".to_string(),
            })?;
        Ok(Matcher {
            regex,
            expands: options.regex,
            whole_text_check: !(options.regex && (query.contains(r"\A") || query.contains(r"\z"))),
        })
    }

    /// False only when no line of `text` (a whole file) can have a match:
    /// one scan that rules out most files in a workspace search.
    pub fn may_match(&self, text: &str) -> bool {
        !self.whole_text_check || self.regex.is_match(text)
    }

    /// The first match in `line` starting at or after byte `from`, as
    /// (start, end). The rest of the line still counts for `\b`, `^` and
    /// look-arounds.
    pub fn find_at(&self, line: &str, from: usize) -> Option<(usize, usize)> {
        let mut at = from;
        while at <= line.len() {
            let found = self.regex.find_at(line, at)?;
            if found.end() > found.start() {
                return Some((found.start(), found.end()));
            }
            // An empty match: try again one character on.
            at = found.end() + line[found.end()..].chars().next()?.len_utf8();
        }
        None
    }

    /// The last match in `line` that starts before byte `before`.
    pub fn find_before(&self, line: &str, before: usize) -> Option<(usize, usize)> {
        self.matches(line)
            .take_while(|(start, _)| *start < before)
            .last()
    }

    /// Every non-overlapping match in `line`, left to right.
    pub fn matches<'a>(&'a self, line: &'a str) -> impl Iterator<Item = (usize, usize)> + 'a {
        self.regex
            .find_iter(line)
            .filter(|found| found.end() > found.start())
            .map(|found| (found.start(), found.end()))
    }

    /// True when the whole of `text` is one match: whether a selection is a
    /// match that Replace may change.
    pub fn matches_exactly(&self, text: &str) -> bool {
        self.find_at(text, 0) == Some((0, text.len()))
    }

    /// What replaces the match at `start..end` of `line`: `with` as typed,
    /// or in regex mode with `$1`, `${name}` filled in from the match.
    pub fn replacement(&self, line: &str, start: usize, end: usize, with: &str) -> String {
        if !self.expands {
            return with.to_owned();
        }
        match self.regex.captures_at(line, start) {
            Some(captures) if captures.get(0).is_some_and(|m| m.range() == (start..end)) => {
                let mut out = String::new();
                captures.expand(with, &mut out);
                out
            }
            _ => with.to_owned(),
        }
    }

    /// `line` with every match replaced, and how many there were.
    pub fn replace_line(&self, line: &str, with: &str) -> (String, usize) {
        let mut out = String::with_capacity(line.len());
        let mut last = 0;
        let mut count = 0;
        for (start, end) in self.matches(line) {
            out.push_str(&line[last..start]);
            out.push_str(&self.replacement(line, start, end, with));
            last = end;
            count += 1;
        }
        out.push_str(&line[last..]);
        (out, count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(regex: bool, match_case: bool, whole_word: bool) -> SearchOptions {
        SearchOptions {
            regex,
            match_case,
            whole_word,
        }
    }

    #[test]
    fn plain_text_is_not_a_pattern() {
        let m = Matcher::new("a.b", SearchOptions::default()).unwrap();
        assert_eq!(m.find_at("axb a.b", 0), Some((4, 7)));
    }

    #[test]
    fn match_case_and_whole_word() {
        let m = Matcher::new("foo", opts(false, false, false)).unwrap();
        assert_eq!(m.matches("Foo food FOO").count(), 3);
        let m = Matcher::new("foo", opts(false, false, true)).unwrap();
        assert_eq!(
            m.matches("Foo food FOO").collect::<Vec<_>>(),
            [(0, 3), (9, 12)]
        );
        let m = Matcher::new("foo", opts(false, true, false)).unwrap();
        assert_eq!(m.matches("Foo food FOO").collect::<Vec<_>>(), [(4, 7)]);
    }

    #[test]
    fn regex_finds_forward_and_backward() {
        let m = Matcher::new(r"\d+", opts(true, true, false)).unwrap();
        let line = "a1 b22 c333";
        assert_eq!(m.find_at(line, 0), Some((1, 2)));
        assert_eq!(m.find_at(line, 2), Some((4, 6)));
        assert_eq!(m.find_before(line, 8), Some((4, 6)));
        assert_eq!(m.find_before(line, 1), None);
        // `\b` still sees the text before `from`.
        let word = Matcher::new("bc", opts(false, true, true)).unwrap();
        assert_eq!(word.find_at("abc bc", 1), Some((4, 6)));
    }

    #[test]
    fn empty_matches_are_skipped() {
        let m = Matcher::new("x*", opts(true, true, false)).unwrap();
        assert_eq!(m.find_at("abxxc", 0), Some((2, 4)));
        assert_eq!(m.find_at("abc", 0), None);
        let m = Matcher::new("^", opts(true, true, false)).unwrap();
        assert_eq!(m.find_at("anything", 0), None);
    }

    #[test]
    fn regex_replacements_fill_in_groups() {
        let m = Matcher::new(r"(\w+)@(\w+)", opts(true, true, false)).unwrap();
        assert_eq!(
            m.replace_line("me@home, you@work", "$2:$1"),
            ("home:me, work:you".to_string(), 2)
        );
        // In plain mode `$1` is just text.
        let plain = Matcher::new("x", SearchOptions::default()).unwrap();
        assert_eq!(plain.replace_line("axb", "$1"), ("a$1b".to_string(), 1));
        assert!(m.matches_exactly("a@b"));
        assert!(!m.matches_exactly("a@b "));
    }

    #[test]
    fn a_whole_file_check_agrees_with_its_lines() {
        let regex = |q| Matcher::new(q, opts(true, true, false)).unwrap();
        let text = "first\r\nsecond line\r\n";
        assert!(regex("^second").may_match(text));
        assert!(regex("first$").may_match(text));
        assert!(!regex("^line").may_match(text));
        // `\A` is each line's start when lines are matched one by one.
        assert!(regex(r"\Asecond").may_match(text));
        assert_eq!(regex(r"\Asecond").find_at("second line", 0), Some((0, 6)));
    }

    #[test]
    fn bad_patterns_and_empty_queries_are_errors() {
        assert_eq!(
            Matcher::new("(", opts(true, true, false)).unwrap_err(),
            "Invalid regular expression"
        );
        assert!(Matcher::new("(", SearchOptions::default()).is_ok());
        assert!(Matcher::new("", SearchOptions::default()).is_err());
    }
}
