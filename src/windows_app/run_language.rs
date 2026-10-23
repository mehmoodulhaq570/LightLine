//! The language Run treats the active file as: what its name says, or the
//! one chosen for it in the language picker, which opens from the language
//! name in the status bar (#52).
//!
//! A choice is remembered per file in `%APPDATA%\LightLine\run-languages.json`,
//! so it outlives the tab and the session. That file is read once, when a
//! file's language is first needed.

use super::app::{is_c_family_path, is_cpp_path};
use super::*;
use lightline::runner::Language as RunnerLanguage;
use lightline::settings::Settings;
use std::fs;

// At most this many choices are kept; the oldest go first.
const REMEMBERED: usize = 500;

/// Languages chosen for files, by path, oldest first.
#[derive(Default)]
pub(super) struct ChosenLanguages {
    // None until read from disk.
    chosen: Option<Vec<(PathBuf, RunLanguage)>>,
}

fn chosen_path() -> Option<PathBuf> {
    Settings::settings_dir().map(|dir| dir.join("run-languages.json"))
}

impl ChosenLanguages {
    fn loaded(&mut self) -> &mut Vec<(PathBuf, RunLanguage)> {
        self.chosen.get_or_insert_with(|| {
            chosen_path()
                .and_then(|path| fs::read_to_string(path).ok())
                .map(|text| parse_chosen(&text))
                .unwrap_or_default()
        })
    }

    fn get(&mut self, path: &Path) -> Option<RunLanguage> {
        self.loaded()
            .iter()
            .rev()
            .find(|(file, _)| same_file(file, path))
            .map(|(_, language)| *language)
    }

    // Remembers `language` for `path` (None forgets it) and saves.
    fn set(&mut self, path: &Path, language: Option<RunLanguage>) -> std::io::Result<()> {
        let chosen = self.loaded();
        chosen.retain(|(file, _)| !same_file(file, path));
        if let Some(language) = language {
            chosen.push((plain(path), language));
        }
        if chosen.len() > REMEMBERED {
            chosen.drain(..chosen.len() - REMEMBERED);
        }
        let text = serialize_chosen(chosen);
        let Some(file) = chosen_path() else {
            return Ok(());
        };
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(file, text)
    }
}

// The plain form of a path (no `\\?\` prefix, `\` between names), as the
// file keeps it.
fn plain(path: &Path) -> PathBuf {
    PathBuf::from(display_path(path).replace('/', "\\"))
}

// Windows paths: the same file whatever the case or the form.
fn same_file(a: &Path, b: &Path) -> bool {
    plain(a)
        .as_os_str()
        .eq_ignore_ascii_case(plain(b).as_os_str())
}

// A list, oldest first: [{"path": "C:\\scripts\\hello", "language": "Python"}].
fn parse_chosen(text: &str) -> Vec<(PathBuf, RunLanguage)> {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str(text) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let path = item.get("path")?.as_str()?;
            let language = RunLanguage::from_label(item.get("language")?.as_str()?)?;
            Some((PathBuf::from(path), language))
        })
        .collect()
}

fn serialize_chosen(chosen: &[(PathBuf, RunLanguage)]) -> String {
    let items: Vec<serde_json::Value> = chosen
        .iter()
        .map(|(path, language)| {
            serde_json::json!({"path": path.to_string_lossy(), "language": language.label()})
        })
        .collect();
    serde_json::to_string_pretty(&items).unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RunLanguage {
    Python,
    C,
    Cpp,
    JavaScript,
    TypeScript,
    Go,
    Rust,
}

impl RunLanguage {
    const ALL: [Self; 7] = [
        Self::Python,
        Self::C,
        Self::Cpp,
        Self::JavaScript,
        Self::TypeScript,
        Self::Go,
        Self::Rust,
    ];

    fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.label() == label)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Python => "Python",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Go => "Go",
            Self::Rust => "Rust",
        }
    }

    /// The language a file's name says, if Run supports it.
    pub(super) fn for_path(path: &Path) -> Option<Self> {
        let python = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("py"));
        if python {
            Some(Self::Python)
        } else if is_cpp_path(path) {
            Some(Self::Cpp)
        } else if is_c_family_path(path) {
            Some(Self::C)
        } else {
            RunnerLanguage::for_path(path).map(|language| match language {
                RunnerLanguage::JavaScript => Self::JavaScript,
                RunnerLanguage::TypeScript => Self::TypeScript,
                RunnerLanguage::Go => Self::Go,
                RunnerLanguage::Rust => Self::Rust,
            })
        }
    }

    /// The runner for the languages runner.rs runs; Python and C/C++ have
    /// their own (panels.rs).
    pub(super) fn runner(self) -> Option<RunnerLanguage> {
        match self {
            Self::JavaScript => Some(RunnerLanguage::JavaScript),
            Self::TypeScript => Some(RunnerLanguage::TypeScript),
            Self::Go => Some(RunnerLanguage::Go),
            Self::Rust => Some(RunnerLanguage::Rust),
            Self::Python | Self::C | Self::Cpp => None,
        }
    }

    /// The Command Palette filter that lists this language's own commands
    /// (interpreter, debugpy, run), if it has any.
    fn commands(self) -> Option<&'static str> {
        match self {
            Self::Python => Some("python"),
            Self::C | Self::Cpp => Some("c/c++"),
            Self::Rust => Some("rust"),
            Self::JavaScript | Self::TypeScript | Self::Go => None,
        }
    }
}

/// A row of the language picker.
#[derive(Clone, Copy)]
pub(super) enum LanguagePick {
    /// Go back to the language the file's name says.
    Detect,
    Language(RunLanguage),
    /// Open the Command Palette at the language's own commands.
    Commands(RunLanguage),
}

impl App {
    /// The language chosen for the active file, if any: kept with the tab,
    /// and for a saved file, in run-languages.json too.
    fn chosen_language(&self) -> Option<RunLanguage> {
        self.tab().run_language.or_else(|| {
            let path = self.doc().path.as_deref()?;
            self.chosen_languages.borrow_mut().get(path)
        })
    }

    /// The language Run treats the active file as.
    pub(super) fn run_language(&self) -> Option<RunLanguage> {
        self.chosen_language()
            .or_else(|| self.doc().path.as_deref().and_then(RunLanguage::for_path))
    }

    /// The status bar's language name: the chosen language, else what the
    /// file's name says.
    pub(super) fn language_name(&self) -> String {
        match self.chosen_language() {
            Some(language) => language.label().into(),
            None => super::render::language_label(self.doc().path.as_deref()),
        }
    }

    // Keeps `language` (None: what the name says) for the active file.
    fn choose_language(&mut self, language: Option<RunLanguage>) {
        self.tab_mut().run_language = language;
        let Some(path) = self.doc().path.clone() else {
            return;
        };
        if let Err(error) = self.chosen_languages.borrow_mut().set(&path, language) {
            self.status = format!("{} (not remembered: {error})", self.status);
        }
    }

    /// Opens the language picker in the Quick Open box.
    pub(super) fn show_language_pick(&mut self, hwnd: HWND) {
        self.quick_open = true;
        self.language_pick = true;
        self.history_pick = None;
        self.panel_focus = false;
        self.terminal_focus = false;
        self.quick_query.clear();
        self.quick_first = 0;
        self.quick_loading = false;
        let current = self.run_language();
        self.quick_selected = self
            .language_picks()
            .iter()
            .position(|(_, pick)| {
                matches!(pick, LanguagePick::Language(language) if Some(*language) == current)
            })
            .unwrap_or(0);
        self.quick_select(self.quick_selected);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// The picker's rows that match what's typed, with their labels.
    pub(super) fn language_picks(&self) -> Vec<(String, LanguagePick)> {
        let query = self.quick_query.to_lowercase();
        let detected = self.doc().path.as_deref().and_then(RunLanguage::for_path);
        let current = self.run_language();
        let mut rows = vec![(
            match detected {
                Some(language) => format!("Auto Detect ({})", language.label()),
                None => "Auto Detect (not runnable)".into(),
            },
            LanguagePick::Detect,
        )];
        for language in RunLanguage::ALL {
            let label = if Some(language) == current {
                format!("{}  (current)", language.label())
            } else {
                language.label().into()
            };
            rows.push((label, LanguagePick::Language(language)));
        }
        if let Some(language) = current.filter(|language| language.commands().is_some()) {
            rows.push((
                format!("{} commands...", language.label()),
                LanguagePick::Commands(language),
            ));
        }
        rows.into_iter()
            .filter(|(label, _)| label.to_lowercase().contains(&query))
            .collect()
    }

    pub(super) fn pick_language(&mut self, hwnd: HWND, index: usize) {
        let Some(pick) = self.language_picks().get(index).map(|(_, pick)| *pick) else {
            return;
        };
        self.quick_open = false;
        self.language_pick = false;
        self.backbuffer = None;
        let detected = self.doc().path.as_deref().and_then(RunLanguage::for_path);
        match pick {
            LanguagePick::Commands(language) => {
                self.show_quick_open(hwnd);
                self.quick_query = format!(">{}", language.commands().unwrap_or_default());
            }
            LanguagePick::Detect => {
                self.status = match detected {
                    Some(language) => format!("Run treats this file as {}", language.label()),
                    None => "Run can't run this file: pick its language to run it".into(),
                };
                self.choose_language(None);
            }
            LanguagePick::Language(language) => {
                self.status = format!("Run treats this file as {}", language.label());
                // The file's own language needs no choice remembered.
                self.choose_language((Some(language) != detected).then_some(language));
            }
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_round_trip_and_unknown_entries_are_dropped() {
        // Oldest first, which isn't alphabetical.
        let chosen = vec![
            (PathBuf::from(r"C:\src\main.h"), RunLanguage::Cpp),
            (PathBuf::from(r"C:\scripts\hello"), RunLanguage::Python),
        ];
        let text = serialize_chosen(&chosen);
        assert_eq!(parse_chosen(&text), chosen);
        let mixed = r#"[
            {"path": "C:\\a", "language": "Cobol"},
            {"path": "C:\\b", "language": "Go"},
            {"path": "C:\\c"},
            "C:\\d"
        ]"#;
        assert_eq!(
            parse_chosen(mixed),
            [(PathBuf::from(r"C:\b"), RunLanguage::Go)]
        );
        assert!(parse_chosen("not json").is_empty());
    }

    #[test]
    fn a_file_is_the_same_whatever_its_case_or_form() {
        assert!(same_file(
            Path::new(r"C:\Src\Hello"),
            Path::new(r"c:\src\hello")
        ));
        assert!(same_file(
            Path::new(r"\\?\C:\src\hello"),
            Path::new("C:/src/hello")
        ));
        assert!(!same_file(
            Path::new(r"C:\src\hello"),
            Path::new(r"C:\src\hello2")
        ));
    }
}
