//! Files attached to an AI question (design/ai-project-access-design.html,
//! part 1): picked by typing @ in the question box, or added with "Add to
//! AI Chat" in the Explorer or the editor's menu. Each shows as a chip above
//! the box, so what will be sent is visible before Enter; on Enter its text
//! goes with the question, cut like selected code.
//!
//! Nothing is read until the question is sent, and then at most MAX_READ
//! bytes of a file (or the first lines of an open tab), so a huge file
//! can't stall the window. Files that look like secrets ask first.

use super::*;
use lightline::ai;

/// Attached files in all, per question.
const MAX_ATTACHED_BYTES: usize = 96 * 1024;
/// At most this much of a file is read; past it the file is "cut".
const MAX_READ: u64 = 256 * 1024;
/// Rows the @ list shows.
pub(super) const MENTION_ROWS: usize = 6;

/// A file waiting to go with the next question.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Attachment {
    pub(super) path: PathBuf,
    /// Its path within the workspace, as shown on its chip.
    pub(super) name: String,
    pub(super) secret: bool,
}

/// A file's chip on a question that was sent: what it says, e.g.
/// "ai.rs · 1,320 lines, cut to 400".
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Chip {
    pub(super) path: PathBuf,
    pub(super) label: String,
    /// Amber: a secrets file, or one that was left out.
    pub(super) warn: bool,
}

/// A file as sent with a question.
pub(super) struct SentFile {
    pub(super) chip: Chip,
    pub(super) name: String,
    /// What the model gets; empty when the file was left out.
    pub(super) text: String,
    pub(super) language: String,
    pub(super) cut: bool,
}

/// Whether `path` looks like it holds secrets: environment files, keys and
/// credentials. These are never sent without asking.
pub(super) fn looks_secret(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    name.starts_with(".env")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name.starts_with("credentials")
        || [".pem", ".key", ".pfx", ".p12"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|&byte| byte == 0)
}

/// The @ word being typed at the end of `input`, without its @: "ai" for
/// "How does @ai". None when the last word doesn't start with @.
pub(super) fn mention_token(input: &str) -> Option<&str> {
    let start = input.rfind(char::is_whitespace).map_or(0, |at| {
        at + input[at..].chars().next().map_or(1, char::len_utf8)
    });
    input[start..].strip_prefix('@')
}

/// How well a file's lowercase relative path `key` matches `query`, best
/// first: its name starts with it (0), a word in its name does, as `ai` in
/// `my_ai.rs` (1), its name contains it (2), only its folders do (3).
fn mention_rank(key: &str, query: &str) -> u8 {
    let name = key.rsplit(['\\', '/']).next().unwrap_or(key);
    if name.starts_with(query) {
        0
    } else if name
        .match_indices(query)
        .any(|(at, _)| !name[..at].ends_with(|c: char| c.is_ascii_alphanumeric()))
    {
        1
    } else if name.contains(query) {
        2
    } else {
        3
    }
}

/// `count` with thousands separated: 1,320.
fn thousands(count: usize) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Cuts `text` like selected code and fits it into `budget`, giving the
/// chip and what to send. `lines` is the file's line count when all of it
/// was read.
fn sent_file(
    path: &Path,
    name: &str,
    text: &str,
    lines: Option<usize>,
    secret: bool,
    budget: &mut usize,
) -> SentFile {
    let (clipped, mut cut) = super::ai_chat::clip_context(text);
    cut |= lines.is_none();
    let kept = clipped.lines().count();
    let short = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string());
    let language = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if clipped.len() > *budget {
        return SentFile {
            chip: Chip {
                path: path.to_path_buf(),
                label: format!("{short} \u{00b7} left out (over 96 KB in all)"),
                warn: true,
            },
            name: name.to_string(),
            text: String::new(),
            language,
            cut: true,
        };
    }
    *budget -= clipped.len();
    let label = match (lines, cut) {
        (Some(lines), false) => {
            let noun = if lines == 1 { "line" } else { "lines" };
            format!("{short} \u{00b7} {} {noun}", thousands(lines))
        }
        (Some(lines), true) => format!(
            "{short} \u{00b7} {} lines, cut to {}",
            thousands(lines),
            thousands(kept)
        ),
        (None, _) => {
            let noun = if kept == 1 { "line" } else { "lines" };
            format!("{short} \u{00b7} cut to {} {noun}", thousands(kept))
        }
    };
    SentFile {
        chip: Chip {
            path: path.to_path_buf(),
            label,
            warn: secret,
        },
        name: name.to_string(),
        text: clipped,
        language,
        cut,
    }
}

impl App {
    /// The @ word being typed, while the list for it is open.
    pub(super) fn ai_mention_query(&self) -> Option<&str> {
        if self.ai.mention_dismissed || !self.ai.focused {
            return None;
        }
        mention_token(&self.ai.input)
    }

    /// Project files matching the @ word, best first (see `mention_rank`).
    /// Files already attached are left out.
    pub(super) fn ai_mention_matches(&self) -> Vec<PathBuf> {
        let Some(query) = self.ai_mention_query() else {
            return Vec::new();
        };
        let query = query.to_ascii_lowercase();
        let mut found: Vec<(u8, usize, &PathBuf)> = self
            .quick_files
            .iter()
            .filter(|(path, key)| {
                key.contains(&query)
                    && !self
                        .ai
                        .attachments
                        .iter()
                        .any(|attached| &attached.path == path)
            })
            .take(2000)
            .map(|(path, key)| (mention_rank(key, &query), key.len(), path))
            .collect();
        found.sort_by_key(|&(rank, length, _)| (rank, length));
        found
            .into_iter()
            .take(MENTION_ROWS)
            .map(|(_, _, path)| path.clone())
            .collect()
    }

    /// `path` as shown: within the workspace if it's inside it.
    pub(super) fn ai_file_name(&self, path: &Path) -> String {
        let plain = |path: &Path| PathBuf::from(display_path(path));
        let path = plain(path);
        self.workspace_root
            .as_deref()
            .map(plain)
            .and_then(|root| path.strip_prefix(&root).ok().map(Path::to_path_buf))
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// Adds `path` to the next question. A folder, a binary file or one
    /// already attached is refused with a word in the status bar.
    pub(super) fn ai_attach(&mut self, hwnd: HWND, path: PathBuf) {
        let name = self.ai_file_name(&path);
        if path.is_dir() {
            self.status = format!("{name} is a folder; attach the files in it");
        } else if self
            .ai
            .attachments
            .iter()
            .any(|attached| attached.path == path)
        {
            self.status = format!("{name} is already attached");
        } else if self.ai.attachments.len() >= 20 {
            self.status = "A question can carry 20 files at most".into();
        } else if !self.ai_attachable(&path) {
            self.status = format!("{name} isn't text, so it can't go with a question");
        } else {
            let secret = looks_secret(&path);
            self.ai.attachments.push(Attachment { path, name, secret });
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    // An open tab is text; a file on disk is, unless its start holds a
    // zero byte. Reads at most 8 KB.
    fn ai_attachable(&self, path: &Path) -> bool {
        if self.tabs.iter().any(|tab| {
            tab.document
                .path
                .as_deref()
                .is_some_and(|open| Self::same_path(open, path))
        }) {
            return true;
        }
        use std::io::Read;
        let mut start = Vec::new();
        std::fs::File::open(path)
            .and_then(|file| file.take(8192).read_to_end(&mut start))
            .is_ok_and(|_| !is_binary(&start))
    }

    /// "Add to AI Chat" from a menu: opens the panel and attaches `path`.
    pub(super) fn ai_attach_from_menu(&mut self, hwnd: HWND, path: PathBuf) {
        if !self.ai_assistant_visible {
            self.ai_assistant_visible = true;
            self.keep_active_tab_visible(hwnd);
        }
        self.ai_attach(hwnd, path);
        self.focus_ai_input();
    }

    pub(super) fn ai_remove_attachment(&mut self, hwnd: HWND, index: usize) {
        if index < self.ai.attachments.len() {
            self.ai.attachments.remove(index);
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Attaches the @ list's chosen file, taking its @ word out of the box.
    pub(super) fn ai_accept_mention(&mut self, hwnd: HWND, choice: Option<PathBuf>) {
        let Some(path) = choice.or_else(|| {
            self.ai_mention_matches()
                .into_iter()
                .nth(self.ai.mention_selected)
        }) else {
            return;
        };
        if let Some(token) = mention_token(&self.ai.input) {
            let keep = self.ai.input.len() - token.len() - 1;
            self.ai.input.truncate(keep);
        }
        self.ai.mention_selected = 0;
        self.ai_attach(hwnd, path);
    }

    /// Keys for the open @ list: Up and Down choose, Enter or Tab attach,
    /// Esc closes it (the @ stays). None when the list isn't open or the key
    /// isn't one of these.
    pub(super) fn ai_mention_key(&mut self, hwnd: HWND, key: u32, ctrl: bool) -> Option<bool> {
        self.ai_mention_query()?;
        if ctrl {
            return None;
        }
        let count = self.ai_mention_matches().len();
        match key {
            k if k == VK_ESCAPE as u32 => {
                self.ai.mention_dismissed = true;
            }
            k if k == VK_UP as u32 && count > 0 => {
                self.ai.mention_selected = (self.ai.mention_selected + count - 1) % count;
            }
            k if k == VK_DOWN as u32 && count > 0 => {
                self.ai.mention_selected = (self.ai.mention_selected + 1) % count;
            }
            k if (k == VK_RETURN as u32 || k == VK_TAB as u32) && count > 0 => {
                self.ai_accept_mention(hwnd, None);
            }
            _ => return None,
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
        Some(true)
    }

    /// The text box changed: the @ list opens again and starts at the top,
    /// and the project's file list is read if it hasn't been yet.
    pub(super) fn ai_input_changed(&mut self) {
        self.ai.mention_dismissed = false;
        self.ai.mention_selected = 0;
        if mention_token(&self.ai.input).is_some()
            && self.quick_files.is_empty()
            && !self.quick_loading
        {
            self.load_quick_files();
        }
    }

    /// The attached files, read and cut, ready to send; the attachments are
    /// used up. None when the user chose not to send a secrets file.
    pub(super) fn ai_files_for_question(&mut self, hwnd: HWND) -> Option<Vec<SentFile>> {
        if self.ai.attachments.is_empty() {
            return Some(Vec::new());
        }
        let secrets: Vec<&str> = self
            .ai
            .attachments
            .iter()
            .filter(|attached| attached.secret)
            .map(|attached| attached.name.as_str())
            .collect();
        if !secrets.is_empty() {
            let endpoint = &self.settings.ai_endpoint;
            let to = if ai::is_local(endpoint) {
                "the model on this PC".to_string()
            } else {
                ai::host(endpoint)
            };
            let message = format!(
                "{} may hold passwords or keys. Send {} to {to}?",
                secrets.join(", "),
                if secrets.len() == 1 { "it" } else { "them" }
            );
            let answer = dialog::show_dialog(
                hwnd,
                "Send a secrets file?",
                &message,
                dialog::DialogIcon::Question,
                &[
                    dialog::DialogButton {
                        label: "Send",
                        id: dialog::DLG_YES,
                        is_default: false,
                        is_cancel: false,
                    },
                    dialog::DialogButton {
                        label: "Don't Send",
                        id: dialog::DLG_NO,
                        is_default: true,
                        is_cancel: true,
                    },
                ],
            );
            if answer != dialog::DLG_YES {
                self.status = "Not sent. Remove the file's chip, or send it after all.".into();
                return None;
            }
        }
        let mut budget = MAX_ATTACHED_BYTES;
        let attachments = std::mem::take(&mut self.ai.attachments);
        let files = attachments
            .iter()
            .map(|attached| {
                let (text, lines) = self.ai_read_attachment(&attached.path);
                sent_file(
                    &attached.path,
                    &attached.name,
                    &text,
                    lines,
                    attached.secret,
                    &mut budget,
                )
            })
            .collect();
        Some(files)
    }

    // The text to send for `path`, and its line count when all of it was
    // read: an open tab's first lines (unsaved edits included), else at
    // most MAX_READ bytes of the file.
    fn ai_read_attachment(&self, path: &Path) -> (String, Option<usize>) {
        if let Some(tab) = self.tabs.iter().find(|tab| {
            !tab.unloaded
                && tab
                    .document
                    .path
                    .as_deref()
                    .is_some_and(|open| Self::same_path(open, path))
        }) {
            let doc = &tab.document;
            let count = doc.line_count();
            // One line more than is ever sent shows whether it was cut.
            let wanted = count.min(super::ai_chat::MAX_CONTEXT_LINES + 1);
            let mut text = String::new();
            for line in 0..wanted {
                if line > 0 {
                    text.push('\n');
                }
                text.push_str(doc.line(line));
            }
            return (text, Some(count));
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        let read = std::fs::File::open(path)
            .and_then(|file| file.take(MAX_READ).read_to_end(&mut bytes))
            .is_ok();
        if !read {
            return (String::new(), Some(0));
        }
        let whole = size <= MAX_READ;
        let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
        let lines = whole.then(|| text.lines().count());
        (text, lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_at_word_is_the_last_word_starting_with_at() {
        assert_eq!(mention_token("How does @ai"), Some("ai"));
        assert_eq!(mention_token("@"), Some(""));
        assert_eq!(mention_token("see @src/ai.rs"), Some("src/ai.rs"));
        assert_eq!(mention_token("mail me@example.com"), None);
        assert_eq!(mention_token("How does @ai work"), None);
        assert_eq!(mention_token("line one\n@ma"), Some("ma"));
        assert_eq!(mention_token("caf\u{e9} @x"), Some("x"));
        assert_eq!(mention_token(""), None);
    }

    #[test]
    fn secrets_files_are_recognized_by_name() {
        for name in [
            ".env",
            ".env.local",
            "server.pem",
            "tls.KEY",
            "id_rsa",
            "id_ed25519.pub",
            "credentials.json",
            "cert.pfx",
        ] {
            assert!(looks_secret(Path::new(name)), "{name}");
        }
        for name in ["main.rs", "environment.ts", "keyboard.rs", "README.md"] {
            assert!(!looks_secret(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn chips_say_how_much_was_sent() {
        let mut budget = MAX_ATTACHED_BYTES;
        let small = sent_file(
            Path::new("src/main.rs"),
            "src/main.rs",
            "fn main() {}\n",
            Some(1),
            false,
            &mut budget,
        );
        assert_eq!(small.chip.label, "main.rs \u{00b7} 1 line");
        assert!(!small.cut && !small.chip.warn);
        assert_eq!(small.language, "rs");

        let long: String = (0..1320).map(|n| format!("let x{n} = {n};\n")).collect();
        let cut = sent_file(
            Path::new("ai.rs"),
            "src/ai.rs",
            &long,
            Some(1320),
            false,
            &mut budget,
        );
        assert_eq!(cut.chip.label, "ai.rs \u{00b7} 1,320 lines, cut to 400");
        assert!(cut.cut);
        assert_eq!(cut.text.lines().count(), 400);

        let huge = sent_file(
            Path::new("big.log"),
            "big.log",
            "x\n",
            None,
            false,
            &mut budget,
        );
        assert_eq!(huge.chip.label, "big.log \u{00b7} cut to 1 line");

        let secret = sent_file(
            Path::new(".env"),
            ".env",
            "A=1\n",
            Some(1),
            true,
            &mut budget,
        );
        assert!(secret.chip.warn);
    }

    #[test]
    fn attachments_past_96_kb_in_all_are_left_out() {
        let mut budget = MAX_ATTACHED_BYTES;
        let chunk: String = (0..300).map(|_| "x".repeat(70) + "\n").collect();
        let mut left_out = 0;
        for n in 0..6 {
            let file = sent_file(
                Path::new("a.txt"),
                &format!("a{n}.txt"),
                &chunk,
                Some(300),
                false,
                &mut budget,
            );
            if file.text.is_empty() {
                left_out += 1;
                assert!(file.chip.label.contains("left out"), "{}", file.chip.label);
                assert!(file.chip.warn);
            }
        }
        assert_eq!(left_out, 2);
    }

    #[test]
    fn names_starting_with_the_at_word_come_first() {
        assert_eq!(mention_rank("src\\ai_demo.rs", "ai"), 0);
        assert_eq!(mention_rank("src\\my_ai.rs", "ai"), 1);
        assert_eq!(mention_rank("src\\main.rs", "ai"), 2);
        assert_eq!(mention_rank("ai\\lib.rs", "ai"), 3);
        assert_eq!(mention_rank(".env", ".env"), 0);
    }

    #[test]
    fn counts_get_thousands_separators() {
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(400), "400");
        assert_eq!(thousands(1320), "1,320");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}
