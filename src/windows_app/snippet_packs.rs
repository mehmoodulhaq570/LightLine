// Snippets from installed snippet packs (lightline::extensions::snippets)
// in the completion list.
//
// Ctrl+Space lists the snippets whose prefix starts with what's typed
// before the caret, with the language server's suggestions when the file
// has one, or alone when it doesn't. A language's snippets are read on a
// worker the first time the list is asked for in it, kept until an
// extension is installed or removed, and never read at startup or while
// typing. Choosing one inserts its body through the snippet engine
// (snippet.rs), so Tab moves through its placeholders.

use super::app::CompletionPopup;
use super::*;
use lightline::extensions::snippets::{self, Snippet};
use std::rc::Rc;

/// LSP CompletionItemKind for a snippet: the list shows it with "~".
pub(super) const SNIPPET_KIND: u8 = 15;

/// A language's installed snippets, or that they're being read.
pub(super) enum SnippetPack {
    Reading,
    Ready(Rc<Vec<Snippet>>),
}

/// A list waiting for snippets to be read: the language key, the document,
/// its change and the caret then, and whether a language server's list is
/// already open to add them to (else they're shown alone).
pub(super) struct SnippetWait {
    key: String,
    document: u64,
    serial: u64,
    cursor: Pos,
    merge: bool,
}

impl App {
    // The names the active file's snippet files may have, and the key its
    // snippets are kept under.
    pub(super) fn snippet_keys(&self) -> (Vec<String>, String) {
        let tab = self.tab();
        let extension = tab
            .syntax
            .as_ref()
            .and_then(Syntax::extension_language)
            .map(str::to_string)
            .or_else(|| tab.extension_choice.as_ref().map(|lang| lang.name.clone()));
        let keys = snippets::language_keys(tab.document.path.as_deref(), extension.as_deref());
        let key = keys.join("|");
        (keys, key)
    }

    /// The active file's snippets if they've been read; otherwise starts
    /// reading them and returns None. A file with no known language has
    /// none.
    pub(super) fn active_snippets(&mut self) -> Option<Rc<Vec<Snippet>>> {
        let (keys, key) = self.snippet_keys();
        if keys.is_empty() {
            return Some(Rc::default());
        }
        match self.snippet_packs.get(&key) {
            Some(SnippetPack::Ready(snippets)) => return Some(snippets.clone()),
            Some(SnippetPack::Reading) => return None,
            None => {}
        }
        self.snippet_packs.insert(key.clone(), SnippetPack::Reading);
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            let snippets = snippets::installed_for(&keys);
            tx.send(WorkerMessage::Snippets(key, snippets));
        });
        None
    }

    /// The active file's snippets that match what's typed before the caret
    /// (empty until they've been read), as completion items, and whether
    /// anything was typed to match them.
    pub(super) fn snippet_items(&mut self) -> (Vec<LspCompletionItem>, bool) {
        let Some(snippets) = self.active_snippets() else {
            return (Vec::new(), false);
        };
        let cursor = self.view().cursor;
        let doc = self.doc();
        let line = doc.line(cursor.line);
        let before = &line[..cursor.byte.min(line.len())];
        let typed = !before.ends_with(char::is_whitespace) && !before.is_empty();
        let end = lsp::Position {
            line: cursor.line as u32,
            character: doc.utf16_column(cursor) as u32,
        };
        let items = snippets::matching(&snippets, before)
            .into_iter()
            .map(|(start, snippet)| {
                let start = lsp::Position {
                    line: cursor.line as u32,
                    character: doc.utf16_column(Pos {
                        line: cursor.line,
                        byte: start,
                    }) as u32,
                };
                LspCompletionItem {
                    label: snippet.prefixes[0].clone(),
                    kind: SNIPPET_KIND,
                    detail: Some(
                        snippet
                            .description
                            .clone()
                            .unwrap_or_else(|| snippet.name.clone()),
                    ),
                    insert: snippet.body.clone(),
                    snippet: true,
                    edit_start: Some(start),
                    edit_end: Some(end),
                }
            })
            .collect();
        (items, typed)
    }

    /// Ctrl+Space where no language server answers: the snippets alone,
    /// once they've been read. False when there's nothing to show, so the
    /// caller can say why.
    pub(super) fn show_snippets_only(&mut self, hwnd: HWND) -> bool {
        let cursor = self.view().cursor;
        if self.active_snippets().is_none() {
            self.wait_for_snippets(false);
            return true;
        }
        let (items, _) = self.snippet_items();
        if items.is_empty() {
            return false;
        }
        let rect = self.caret_rect(hwnd);
        self.completion = Some(CompletionPopup {
            items,
            selected: 0,
            replace_start: cursor,
            replace_end: cursor,
            x: rect.left,
            y: rect.bottom,
            folder: None,
        });
        unsafe { InvalidateRect(hwnd, null(), 0) };
        true
    }

    /// The active file's snippets are being read: the list being opened
    /// (`merge`: a language server's, else snippets alone) gets them when
    /// they arrive, if nothing moved meanwhile.
    pub(super) fn wait_for_snippets(&mut self, merge: bool) {
        self.snippet_wait = Some(SnippetWait {
            key: self.snippet_keys().1,
            document: self.doc().id(),
            serial: self.doc().change_serial(),
            cursor: self.view().cursor,
            merge,
        });
    }

    /// A language's snippets were read: kept, and shown or added to the
    /// list if one was waiting for them.
    pub(super) fn snippets_read(&mut self, hwnd: HWND, key: String, list: Vec<Snippet>) {
        // An install or removal while they were read makes them stale.
        if !matches!(self.snippet_packs.get(&key), Some(SnippetPack::Reading)) {
            return;
        }
        self.snippet_packs
            .insert(key.clone(), SnippetPack::Ready(Rc::new(list)));
        let Some(wait) = self.snippet_wait.take() else {
            return;
        };
        let still = wait.key == key
            && wait.document == self.doc().id()
            && wait.serial == self.doc().change_serial()
            && wait.cursor == self.view().cursor;
        if !still {
            return;
        }
        if !wait.merge {
            if !self.show_snippets_only(hwnd) {
                self.status = "No completions".into();
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            return;
        }
        // The language server answered first: its list, if still open,
        // gets them.
        let open = self
            .completion
            .as_ref()
            .is_some_and(|popup| popup.folder.is_none() && popup.replace_end == wait.cursor);
        let (items, typed) = self.snippet_items();
        if open
            && !items.is_empty()
            && let Some(popup) = self.completion.as_mut()
        {
            merge_snippets(&mut popup.items, items, typed);
            popup.selected = 0;
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    /// An extension was installed or removed: snippets are read again when
    /// next asked for.
    pub(super) fn forget_snippets(&mut self) {
        self.snippet_packs.clear();
        self.snippet_wait = None;
    }
}

/// Adds `snippets` to a language server's `items`: first when something was
/// typed (a prefix is typed on purpose), else after them.
pub(super) fn merge_snippets(
    items: &mut Vec<LspCompletionItem>,
    snippets: Vec<LspCompletionItem>,
    typed: bool,
) {
    if typed {
        items.splice(0..0, snippets);
    } else {
        items.extend(snippets);
    }
}

/// The status bar after a snippet pack is installed.
pub(super) fn installed_note(name: &str, count: usize, languages: &[Option<String>]) -> String {
    let languages: Vec<String> = languages
        .iter()
        .map(|language| match language {
            Some(language) => language_title(language),
            None => "every language".into(),
        })
        .collect();
    let plural = if count == 1 { "" } else { "s" };
    format!(
        "{name} installed: {count} snippet{plural} for {}. Type a prefix and press Ctrl+Space",
        languages.join(", ")
    )
}

/// A language as snippet files name it, written the way Zed shows it:
/// `python` → Python, `tsx` → TSX, `shell script` → Shell Script.
fn language_title(language: &str) -> String {
    match language {
        "tsx" | "json" | "jsonc" | "html" | "css" | "yaml" | "toml" | "sql" | "php" | "erb" => {
            language.to_uppercase()
        }
        "c++" | "cpp" => "C++".into(),
        "csharp" | "c#" => "C#".into(),
        "javascript" => "JavaScript".into(),
        "typescript" => "TypeScript".into(),
        "gdscript" => "GDScript".into(),
        "latex" => "LaTeX".into(),
        _ => language
            .split(' ')
            .map(|word| {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, kind: u8) -> LspCompletionItem {
        LspCompletionItem {
            label: label.into(),
            kind,
            detail: None,
            insert: label.into(),
            snippet: false,
            edit_start: None,
            edit_end: None,
        }
    }

    #[test]
    fn typed_prefixes_put_snippets_first() {
        let server = || vec![item("format", 3), item("for_each", 2)];
        let mut items = server();
        merge_snippets(&mut items, vec![item("fastapp", SNIPPET_KIND)], true);
        assert_eq!(items[0].label, "fastapp");
        let mut items = server();
        merge_snippets(&mut items, vec![item("fastapp", SNIPPET_KIND)], false);
        assert_eq!(items[2].label, "fastapp");
    }

    #[test]
    fn install_note_names_languages_as_zed_does() {
        assert_eq!(
            installed_note("FastAPI Snippets", 25, &[Some("python".into())]),
            "FastAPI Snippets installed: 25 snippets for Python. Type a prefix and press Ctrl+Space"
        );
        assert_eq!(
            installed_note(
                "Blocks",
                1,
                &[
                    Some("shell script".into()),
                    Some("c++".into()),
                    Some("tsx".into()),
                    None
                ]
            ),
            "Blocks installed: 1 snippet for Shell Script, C++, TSX, every language. Type a \
             prefix and press Ctrl+Space"
        );
    }
}
