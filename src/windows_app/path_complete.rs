// File path suggestions (`pathCompletion`), like VS Code's Path IntelliSense:
// typing `./` or `../` in a string opens the completion box with the files
// and folders there, using the Explorer's icons; each character typed after
// narrows it, and choosing a folder adds `/` and lists what's inside.
//
// The folder is read on a worker through the Explorer's own cache
// (`load_directory`), which the file watcher keeps up to date, so typing
// more letters only filters what was read. What the workspace search
// leaves out is left out here too: the `.gitignore` rules (read once per
// folder), and `.git`, `target`, `node_modules`, `.venv`, `__pycache__`.
// The letters typed are highlighted, and a footer says which folder is
// listed and how many of its entries match. Nothing runs unless the text
// before the caret is a relative path in a string.

use super::app::CompletionPopup;
use super::*;

// A huge folder can't make the box slow to fill.
const MAX_ITEMS: usize = 500;
// LSP CompletionItemKind for a file and a folder.
pub(super) const FILE_KIND: u8 = 17;
pub(super) const FOLDER_KIND: u8 = 19;

/// What a list of path suggestions shows besides its items.
pub(super) struct PathList {
    /// The folder listed.
    pub(super) folder: PathBuf,
    /// What was typed of the entry's name, lower case, to highlight.
    pub(super) typed: String,
    /// "./src/ · 4 of 23".
    pub(super) footer: String,
}

/// Names the workspace search never looks in.
const SKIPPED: [&str; 5] = [".git", "target", "node_modules", ".venv", "__pycache__"];

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
/// the prefix does, and none that `hidden` (name, is a folder) leaves out.
pub(super) fn path_items(
    entries: &[ExplorerEntry],
    prefix: &str,
    hidden: &dyn Fn(&str, bool) -> bool,
) -> Vec<LspCompletionItem> {
    let needle = prefix.to_lowercase();
    entries
        .iter()
        .filter_map(|entry| {
            let name = entry.path.file_name()?.to_string_lossy().into_owned();
            let shown = name.to_lowercase().starts_with(&needle)
                && (!name.starts_with('.') || prefix.starts_with('.'))
                && !SKIPPED.contains(&name.as_str())
                && !hidden(&name, entry.is_dir);
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

/// The repository `dir` is in (the nearest folder above with a `.git`),
/// and `dir`'s path from there, ending in `/` ("" for the root itself).
fn repository_of(dir: &Path) -> Option<(PathBuf, String)> {
    let root = dir
        .ancestors()
        .find(|folder| folder.join(".git").exists())?;
    let relative = dir.strip_prefix(root).ok()?;
    let mut path = String::new();
    for part in relative.components() {
        path.push_str(&part.as_os_str().to_string_lossy());
        path.push('/');
    }
    Some((root.to_path_buf(), path))
}

impl App {
    // The .gitignore rules in force in `dir`, read once per folder: its
    // repository's, then each .gitignore down to it.
    fn path_rules_for(&mut self, dir: &Path) -> Option<&(lightline::gitignore::Rules, String)> {
        if !self.path_rules.contains_key(dir) {
            let (root, relative) = repository_of(dir)?;
            let mut rules = lightline::gitignore::Rules::for_root(&root);
            let mut walked = String::new();
            let mut folder = root.clone();
            for part in relative.split('/').filter(|part| !part.is_empty()) {
                folder.push(part);
                walked.push_str(part);
                walked.push('/');
                rules = rules.within(&folder, &walked);
            }
            self.path_rules.insert(dir.to_path_buf(), (rules, relative));
        }
        self.path_rules.get(dir)
    }

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
        if !self.directory_cache.contains_key(&dir) {
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
        }
        let rules = self.path_rules_for(&dir).cloned();
        let hidden = |name: &str, is_dir: bool| {
            rules.as_ref().is_some_and(|(rules, relative)| {
                rules.ignores(&format!("{relative}{name}"), is_dir)
            })
        };
        let entries = &self.directory_cache[&dir];
        let items = path_items(entries, &context.prefix, &hidden);
        let items_len = items.len();
        // Everything the folder would list: the footer's "of N".
        let listed = entries
            .iter()
            .filter(|entry| {
                entry.path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    !SKIPPED.contains(&name.as_ref()) && !hidden(&name, entry.is_dir)
                })
            })
            .count();
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
                folder: Some(PathList {
                    folder: dir,
                    typed: context.prefix.to_lowercase(),
                    footer: format!("{} · {} of {listed}", context.dir, items_len),
                }),
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
    fn skipped_and_ignored_entries_are_left_out() {
        let entry = |name: &str, is_dir: bool| ExplorerEntry {
            path: PathBuf::from("/p").join(name),
            is_dir,
        };
        let entries = [
            entry("node_modules", true),
            entry("dist", true),
            entry("src", true),
            entry("app.log", false),
        ];
        let hidden = |name: &str, _: bool| name == "dist" || name.ends_with(".log");
        let names: Vec<String> = path_items(&entries, "", &hidden)
            .into_iter()
            .map(|item| item.label)
            .collect();
        assert_eq!(names, ["src"]);
    }

    #[test]
    fn finds_the_repository_and_the_path_in_it() {
        let base = std::env::temp_dir().join(format!("ll-paths-{}", std::process::id()));
        let inner = base.join("web").join("src");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::create_dir_all(base.join(".git")).unwrap();
        assert_eq!(
            repository_of(&inner),
            Some((base.clone(), "web/src/".into()))
        );
        assert_eq!(repository_of(&base), Some((base.clone(), String::new())));
        std::fs::remove_dir_all(&base).unwrap();
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
            labels(path_items(&entries, "con", &|_, _| false)),
            [
                ("controllers".into(), "controllers/".into(), FOLDER_KIND),
                ("Config.js".into(), "Config.js".into(), FILE_KIND),
            ]
        );
        assert_eq!(labels(path_items(&entries, ".c", &|_, _| false)).len(), 1);
        assert_eq!(labels(path_items(&entries, "", &|_, _| false)).len(), 3);
    }
}
