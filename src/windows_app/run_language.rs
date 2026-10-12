//! The language Run treats the active file as: what its name says, or the
//! one chosen for it in the language picker, which opens from the language
//! name in the status bar (#52).

use super::app::{is_c_family_path, is_cpp_path};
use super::*;
use lightline::runner::Language as RunnerLanguage;

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
    /// The language Run treats the active file as.
    pub(super) fn run_language(&self) -> Option<RunLanguage> {
        self.tab()
            .run_language
            .or_else(|| self.doc().path.as_deref().and_then(RunLanguage::for_path))
    }

    /// The status bar's language name: the chosen language, else what the
    /// file's name says.
    pub(super) fn language_name(&self) -> String {
        match self.tab().run_language {
            Some(language) => language.label().into(),
            None => super::render::language_label(self.doc().path.as_deref()),
        }
    }

    /// Opens the language picker in the Quick Open box.
    pub(super) fn show_language_pick(&mut self, hwnd: HWND) {
        self.quick_open = true;
        self.language_pick = true;
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
                self.tab_mut().run_language = None;
                self.status = match detected {
                    Some(language) => format!("Run treats this file as {}", language.label()),
                    None => "Run can't run this file: pick its language to run it".into(),
                };
            }
            LanguagePick::Language(language) => {
                // The file's own language needs no choice remembered.
                self.tab_mut().run_language = (Some(language) != detected).then_some(language);
                self.status = format!("Run treats this file as {}", language.label());
            }
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}
