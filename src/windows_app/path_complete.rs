// File path suggestions (`pathCompletion`), like VS Code's Path IntelliSense:
// typing `./` or `../` in a string opens the completion box with the files
// and folders there, using the Explorer's icons; each character typed after
// narrows it, and choosing a folder adds `/` and lists what's inside.
//
// The folder is read on a worker through the Explorer's own cache
// (`load_directory`), which the file watcher keeps up to date, so typing
// more letters only filters what was read. Nothing runs unless the text
// before the caret is a relative path in a string.

use super::app::CompletionPopup;
use super::*;

// A huge folder can't make the box slow to fill.
const MAX_ITEMS: usize = 500;
// LSP CompletionItemKind for a file and a folder.
pub(super) const FILE_KIND: u8 = 17;
pub(super) const FOLDER_KIND: u8 = 19;

/// A relative path being typed in a string: where its last part starts on
/// the line, the folder part (`./src/`) and the last part typed so far.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct PathContext {
    pub(super) start: usize,
    pub(super) dir: String,
    pub(super) prefix: String,
}

/// The path typed before `caret` on `line`, when it's a string starting
/// with `./` or `../`.
pub(super) fn path_context(line: &str, caret: usize) -> Option<PathContext> {
    let before = line.get(..caret)?;
    let quote = before.rfind(['"', '\'', '`'])?;
    let typed = &before[quote + 1..];
    if !(typed.starts_with("./") || typed.starts_with("../")) || typed.contains(['\\', '\t', '\n'])
    {
        return None;
    }
    let slash = typed.rfind('/')?;
    Some(PathContext {
        start: quote + 1 + slash + 1,
        dir: typed[..=slash].to_owned(),
        prefix: typed[slash + 1..].to_owned(),
    })
}

/// `dir` (`./src/`, `../lib/`) from the folder `base`, with `.` and `..`
/// resolved.
pub(super) fn resolve_dir(base: &Path, dir: &str) -> PathBuf {
    let mut path = base.to_path_buf();
    for part in dir.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                path.pop();
            }
            name => path.push(name),
        }
    }
    path
}

/// The entries of a folder listing that start with `prefix` (case doesn't
/// matter), folders first as listed; files starting with a dot only when
/// the prefix does.
pub(super) fn path_items(entries: &[ExplorerEntry], prefix: &str) -> Vec<LspCompletionItem> {
    let needle = prefix.to_lowercase();
    entries
        .iter()
        .filter_map(|entry| {
            let name = entry.path.file_name()?.to_string_lossy().into_owned();
            let shown = name.to_lowercase().starts_with(&needle)
                && (!name.starts_with('.') || prefix.starts_with('.'));
            shown.then(|| LspCompletionItem {
                insert: if entry.is_dir {
                    format!("{name}/")
                } else {
                    name.clone()
                },
                label: name,
                kind: if entry.is_dir { FOLDER_KIND } else { FILE_KIND },
                detail: Some(if entry.is_dir { "folder" } else { "file" }.into()),
                snippet: false,
                edit_start: None,
                edit_end: None,
            })
        })
        .take(MAX_ITEMS)
        .collect()
}

impl App {
    /// After typing, Ctrl+Space or choosing a folder: when the caret is in a
    /// relative path in a string, shows the folder's entries (reading the
    /// folder first if needed). True when it's such a path.
    pub(super) fn suggest_paths(&mut self, hwnd: HWND) -> bool {
        if !self.settings.path_completion
            || self.selection_range().is_some()
            || !self.view().extra.is_empty()
            || self.tab().read_only()
        {
            return false;
        }
        let Some(base) = self.doc().path.as_deref().and_then(Path::parent) else {
            return false;
        };
        let cursor = self.view().cursor;
        let Some(context) = path_context(self.doc().line(cursor.line), cursor.byte) else {
            return false;
        };
        let dir = resolve_dir(base, &context.dir);
        let Some(entries) = self.directory_cache.get(&dir) else {
            // Read on a worker; shown when it arrives if the caret is still
            // where it was (path_listing_arrived).
            self.path_pending = Some((
                self.doc().id(),
                self.doc().change_serial(),
                cursor,
                dir.clone(),
            ));
            self.load_directory(&dir);
            return true;
        };
        let items = path_items(entries, &context.prefix);
        let had = self.completion.is_some();
        if items.is_empty() {
            self.completion = None;
        } else {
            let rect = self.caret_rect(hwnd);
            self.completion = Some(CompletionPopup {
                items,
                selected: 0,
                replace_start: Pos {
                    line: cursor.line,
                    byte: context.start,
                },
                replace_end: cursor,
                x: rect.left,
                y: rect.bottom,
                folder: Some(dir),
            });
        }
        if had || self.completion.is_some() {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
        true
    }

    /// A folder listing arrived: shows it if it's the one a path being
    /// typed was waiting for and nothing moved since.
    pub(super) fn path_listing_arrived(&mut self, hwnd: HWND, dir: &Path) {
        let Some((document, serial, cursor, waiting)) = &self.path_pending else {
            return;
        };
        if waiting != dir {
            return;
        }
        let still = *document == self.doc().id()
            && *serial == self.doc().change_serial()
            && *cursor == self.view().cursor;
        self.path_pending = None;
        if still {
            self.suggest_paths(hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_relative_paths_in_strings() {
        let line = r#"import { a } from "./src/con""#;
        let caret = line.len() - 1;
        assert_eq!(
            path_context(line, caret),
            Some(PathContext {
                start: line.find("con").unwrap(),
                dir: "./src/".into(),
                prefix: "con".into(),
            })
        );
        let line = "open('../data/')";
        assert_eq!(
            path_context(line, 14).map(|c| (c.dir, c.prefix)),
            Some(("../data/".into(), String::new()))
        );
        // Not a relative path, or not in a string.
        assert!(path_context("x = \"src/a\"", 10).is_none());
        assert!(path_context("let p = ./src/", 14).is_none());
        assert!(path_context("\"./a\" + b", 8).is_none());
        assert!(path_context(r#"".\src\a""#, 7).is_none());
    }

    #[test]
    fn resolves_dot_and_dot_dot() {
        let base = Path::new("/p/web/src");
        assert_eq!(resolve_dir(base, "./"), PathBuf::from("/p/web/src"));
        assert_eq!(
            resolve_dir(base, "./ui/"),
            Path::new("/p/web/src").join("ui")
        );
        assert_eq!(
            resolve_dir(base, "../lib/"),
            Path::new("/p/web").join("lib")
        );
    }

    #[test]
    fn items_match_the_prefix_and_hide_dot_files() {
        let entry = |name: &str, is_dir: bool| ExplorerEntry {
            path: PathBuf::from("/p").join(name),
            is_dir,
        };
        let entries = [
            entry("controllers", true),
            entry(".config", true),
            entry("Config.js", false),
            entry("main.js", false),
        ];
        let labels = |items: Vec<LspCompletionItem>| {
            items
                .into_iter()
                .map(|item| (item.label, item.insert, item.kind))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(path_items(&entries, "con")),
            [
                ("controllers".into(), "controllers/".into(), FOLDER_KIND),
                ("Config.js".into(), "Config.js".into(), FILE_KIND),
            ]
        );
        assert_eq!(labels(path_items(&entries, ".c")).len(), 1);
        assert_eq!(labels(path_items(&entries, "")).len(), 3);
    }
}
