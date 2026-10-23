//! Run and Debug links on `main` and tests (code lens). They're drawn after
//! the end of the line that starts the function, faint, and run it with a
//! click: the file (Run, Debug) or just that test (Run test). Found from the
//! text itself, so they show without a language server. Setting `codeLens`.
//!
//! Rust: `fn main`, and functions with a test attribute (`cargo test name`).
//! Python: `if __name__ == "__main__":`, and pytest's `test_` functions, in
//! `Test` classes too (`python -m pytest file::Class::test`). Go: `func main`,
//! and `TestXxx` in `_test.go` files (`go test -run ^TestXxx$`). C and C++:
//! `main`.
//!
//! A merge conflict's `<<<<<<<` line gets links too, in any file and with
//! `codeLens` off (merge_conflict.rs).

use super::merge_conflict::{self, Choice};
use super::run_language::RunLanguage;
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Lens {
    Main,
    Test {
        name: String,
        // Python: the class the test is in.
        class: Option<String>,
    },
    Conflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LensAction {
    Run,
    Debug,
    RunTest,
    Accept(Choice),
}

/// A link's left and right edge, and what it does.
type LinkSpan = (i32, i32, LensAction);

/// Where a line's links were drawn: (document, line) to each link's span.
#[derive(Default)]
pub(super) struct LensHits {
    hits: HashMap<(u64, usize), Vec<LinkSpan>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Rust,
    Python,
    Go { test_file: bool },
    C,
}

fn indent_of(text: &str) -> usize {
    text.len() - text.trim_start().len()
}

// The name in "fn name(" / "def name(" / "func name(" after `prefix`.
fn name_after<'a>(text: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    let rest = prefixes
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix))?;
    let end = rest.find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))?;
    (end > 0 && rest[end..].trim_start().starts_with(['(', '<'])).then_some(&rest[..end])
}

fn rust_lens(doc: &Document, line: usize, text: &str) -> Option<Lens> {
    let trimmed = text.trim_start();
    let prefixes = [
        "fn ",
        "pub fn ",
        "async fn ",
        "pub async fn ",
        "pub(crate) fn ",
    ];
    let name = name_after(trimmed, &prefixes)?;
    if name == "main" && indent_of(text) == 0 {
        return Some(Lens::Main);
    }
    // Attributes right above, skipping comments: one naming a test.
    let mut at = line;
    for _ in 0..6 {
        if at == 0 {
            break;
        }
        at -= 1;
        let above = doc.line(at).trim();
        if above.starts_with("//") {
            continue;
        }
        if !above.starts_with("#[") {
            break;
        }
        if above.contains("test]") || above.contains("::test") || above.starts_with("#[test") {
            return Some(Lens::Test {
                name: name.to_string(),
                class: None,
            });
        }
    }
    None
}

fn python_lens(doc: &Document, line: usize, text: &str) -> Option<Lens> {
    let trimmed = text.trim_start();
    let indent = indent_of(text);
    if indent == 0
        && let Some(condition) = trimmed.strip_prefix("if __name__ ==")
    {
        let condition = condition.trim();
        return ["\"__main__\":", "'__main__':"]
            .iter()
            .any(|main| condition.starts_with(main))
            .then_some(Lens::Main);
    }
    let name = name_after(trimmed, &["def ", "async def "])?;
    if !name.starts_with("test") {
        return None;
    }
    if indent == 0 {
        return Some(Lens::Test {
            name: name.to_string(),
            class: None,
        });
    }
    // A method: only in a pytest class (Test..., at the top level).
    let class = (0..line).rev().take(5_000).find_map(|at| {
        let above = doc.line(at);
        let shallower = !above.trim().is_empty() && indent_of(above) < indent;
        shallower.then(|| (indent_of(above), above.trim_start()))
    })?;
    let (class_indent, class_line) = class;
    let class_name = name_after(class_line.trim_end_matches(':'), &["class "]).or_else(|| {
        let rest = class_line.strip_prefix("class ")?;
        let end = rest.find([':', '('])?;
        Some(rest[..end].trim())
    })?;
    (class_indent == 0 && class_name.starts_with("Test")).then(|| Lens::Test {
        name: name.to_string(),
        class: Some(class_name.to_string()),
    })
}

fn go_lens(text: &str, test_file: bool) -> Option<Lens> {
    if indent_of(text) != 0 {
        return None;
    }
    let name = name_after(text, &["func "])?;
    if name == "main" && !test_file {
        return Some(Lens::Main);
    }
    let after = name.strip_prefix("Test")?;
    let named = after.is_empty() || !after.starts_with(|ch: char| ch.is_lowercase());
    (test_file && named && text.contains("*testing.T")).then(|| Lens::Test {
        name: name.to_string(),
        class: None,
    })
}

fn c_lens(text: &str) -> Option<Lens> {
    if indent_of(text) != 0 {
        return None;
    }
    let name = name_after(text, &["int ", "void "])?;
    (name == "main").then_some(Lens::Main)
}

impl App {
    fn lens_kind(&self, tab: &Tab) -> Option<Kind> {
        if !self.settings.code_lens || tab.read_only() || tab.is_placeholder() {
            return None;
        }
        let doc = &tab.document;
        doc.path.as_ref()?;
        if Tab::is_rust(doc) {
            Some(Kind::Rust)
        } else if Tab::is_python(doc) {
            Some(Kind::Python)
        } else if Tab::is_go(doc) {
            let test_file = doc
                .path
                .as_deref()
                .and_then(Path::file_name)
                .is_some_and(|name| name.to_string_lossy().ends_with("_test.go"));
            Some(Kind::Go { test_file })
        } else if Tab::is_c_family_or_header(doc) {
            Some(Kind::C)
        } else {
            None
        }
    }

    /// The lens on `line` of tab `index`, if any.
    pub(super) fn lens_on_line(&self, index: usize, line: usize) -> Option<Lens> {
        let tab = &self.tabs[index];
        if merge_conflict::starts_conflict(tab.document.line(line))
            && !tab.read_only()
            && !tab.is_placeholder()
            && merge_conflict::conflict_at(&tab.document, line).is_some()
        {
            return Some(Lens::Conflict);
        }
        let kind = self.lens_kind(tab)?;
        let doc = &tab.document;
        let text = doc.line(line);
        if text.len() > 1_000 {
            return None;
        }
        match kind {
            Kind::Rust => rust_lens(doc, line, text),
            Kind::Python => python_lens(doc, line, text),
            Kind::Go { test_file } => go_lens(text, test_file),
            Kind::C => c_lens(text),
        }
    }

    /// The links a lens in tab `index` shows, in order. Debug is there for
    /// the languages LightLine debugs: Rust and Python.
    pub(super) fn lens_links(&self, index: usize, lens: &Lens) -> Vec<(&'static str, LensAction)> {
        match lens {
            Lens::Conflict => vec![
                ("Accept Current", LensAction::Accept(Choice::Current)),
                ("Accept Incoming", LensAction::Accept(Choice::Incoming)),
                ("Accept Both", LensAction::Accept(Choice::Both)),
            ],
            Lens::Test { .. } => vec![("\u{25b6} Run test", LensAction::RunTest)],
            Lens::Main => {
                let mut links = vec![("\u{25b6} Run", LensAction::Run)];
                let doc = &self.tabs[index].document;
                if Tab::is_rust(doc) || Tab::is_python(doc) {
                    links.push(("Debug", LensAction::Debug));
                }
                links
            }
        }
    }

    /// Paint records where a line's links went.
    pub(super) fn record_lens_hits(&self, document: u64, line: usize, hits: Vec<LinkSpan>) {
        let mut all = self.lens_hits.borrow_mut();
        if hits.is_empty() {
            all.hits.remove(&(document, line));
        } else {
            all.hits.insert((document, line), hits);
        }
    }

    /// The code pane at `x`.
    pub(super) fn pane_at_x(&self, hwnd: HWND, x: i32) -> usize {
        usize::from(self.split_visible && x >= self.pane_divider(hwnd))
    }

    /// The link at (`x`, `y`) in `pane`, if one was drawn there.
    pub(super) fn lens_at(&self, hwnd: HWND, pane: usize, x: i32, y: i32) -> Option<LensAction> {
        if y < self.editor_top() {
            return None;
        }
        let index = self.tab_for_pane(pane);
        let (line, row) = self.row_at_y(hwnd, pane, y);
        // Links are on a line's last row.
        if row + 1 != self.line_rows(hwnd, pane, line).count() {
            return None;
        }
        self.lens_on_line(index, line)?;
        let document = self.tabs[index].document.id();
        let hits = self.lens_hits.borrow();
        hits.hits
            .get(&(document, line))?
            .iter()
            .find(|(left, right, _)| x >= *left && x < *right)
            .map(|(_, _, action)| *action)
    }

    /// A click on a link: does what it says. True if there was one.
    pub(super) fn lens_click(&mut self, hwnd: HWND, pane: usize, x: i32, y: i32) -> bool {
        let Some(action) = self.lens_at(hwnd, pane, x, y) else {
            return false;
        };
        let (line, _) = self.row_at_y(hwnd, pane, y);
        let lens = self.lens_on_line(self.tab_for_pane(pane), line);
        match (action, lens) {
            (LensAction::Run, _) => self.run_active_file(hwnd),
            (LensAction::Debug, _) => self.start_debug_session(hwnd),
            (LensAction::Accept(choice), _) => self.accept_conflict(hwnd, line, choice),
            (LensAction::RunTest, Some(Lens::Test { name, class })) => {
                self.run_test(hwnd, &name, class.as_deref())
            }
            _ => {}
        }
        true
    }

    // Runs one test of the active file in the Output panel.
    fn run_test(&mut self, hwnd: HWND, name: &str, class: Option<&str>) {
        if self.doc().is_dirty() && !self.save(hwnd, false) {
            self.status = "Save the file before running its tests".into();
            self.refresh(hwnd);
            return;
        }
        let Some(file) = self.doc().path.clone() else {
            return;
        };
        let missing = |tool: &str, hint: &str| format!("{tool} was not found on PATH. {hint}");
        let plan = if Tab::is_rust(self.doc()) {
            let Some(root) = file
                .parent()
                .and_then(|folder| {
                    folder
                        .ancestors()
                        .find(|folder| folder.join("Cargo.toml").is_file())
                })
                .map(Path::to_path_buf)
            else {
                self.show_run_error(hwnd, "This file isn't in a Cargo project".into());
                return;
            };
            let Some(cargo) = workflow::resolve_command("cargo") else {
                self.show_run_error(hwnd, missing("Cargo", "Install Rust from rustup.rs."));
                return;
            };
            lightline::runner::RunPlan {
                cwd: PathBuf::from(display_path(&root)),
                commands: vec![lightline::runner::RunCommand {
                    program: cargo,
                    arguments: vec!["test".into(), name.into()],
                }],
            }
        } else if self.run_language() == Some(RunLanguage::Python) {
            let root = workflow::python_project_root(&file, self.workspace_root.as_deref());
            let Some(interpreter) = self.resolve_python_interpreter(&root) else {
                self.show_run_error(hwnd, "Python interpreter not found. Use Select Python interpreter in the Command Palette.".into());
                return;
            };
            let mut target = display_path(&file);
            if let Some(class) = class {
                target = format!("{target}::{class}");
            }
            target = format!("{target}::{name}");
            lightline::runner::RunPlan {
                cwd: PathBuf::from(display_path(&root)),
                commands: vec![lightline::runner::RunCommand {
                    program: PathBuf::from(display_path(&interpreter)),
                    arguments: vec!["-m".into(), "pytest".into(), target.into()],
                }],
            }
        } else if Tab::is_go(self.doc()) {
            let Some(go) = workflow::resolve_command("go") else {
                self.show_run_error(hwnd, missing("Go", "Install it from go.dev/dl."));
                return;
            };
            let Some(folder) = file.parent() else {
                return;
            };
            lightline::runner::RunPlan {
                cwd: PathBuf::from(display_path(folder)),
                commands: vec![lightline::runner::RunCommand {
                    program: go,
                    arguments: vec![
                        "test".into(),
                        "-run".into(),
                        format!("^{name}$").into(),
                        ".".into(),
                    ],
                }],
            }
        } else {
            return;
        };
        self.status = format!("Running test {name}...");
        self.run_language_plan(hwnd, plan);
    }

    /// Message-loop hook: a line whose lens came or went because a line
    /// above it changed (an attribute typed) is redrawn.
    pub(super) fn check_lenses(&self, hwnd: HWND) {
        if !self.settings.code_lens || self.welcome || self.tabs.is_empty() {
            return;
        }
        let panes = if self.split_visible { 0..2 } else { 0..1 };
        for pane in panes {
            let index = self.tab_for_pane(pane);
            if self.lens_kind(&self.tabs[index]).is_none() {
                continue;
            }
            let doc = &self.tabs[index].document;
            let first = self.view_top(hwnd, pane).0;
            let mut line = first;
            let mut lines = Vec::new();
            for _ in 0..=self.visible_lines(hwnd) {
                if self.lens_on_line(index, line).is_some() {
                    lines.push(line);
                }
                let next = doc.next_visible_line(line);
                if next == line {
                    break;
                }
                line = next;
            }
            let now = (doc.id(), first, lines);
            let before = std::mem::replace(&mut self.lenses_shown.borrow_mut()[pane], now.clone());
            if before.0 != now.0 || before.1 != now.1 {
                continue;
            }
            let changed: Vec<(usize, usize)> = before
                .2
                .iter()
                .filter(|line| !now.2.contains(line))
                .chain(now.2.iter().filter(|line| !before.2.contains(line)))
                .map(|&line| (line, line))
                .collect();
            if !changed.is_empty() {
                for area in self.rows_showing(hwnd, pane, &changed) {
                    unsafe { InvalidateRect(hwnd, &area, 0) };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        let mut doc = Document::default();
        doc.replace(Pos::default(), Pos::default(), text);
        doc
    }

    fn rust(text: &str, line: usize) -> Option<Lens> {
        let doc = doc(text);
        rust_lens(&doc, line, doc.line(line))
    }

    fn python(text: &str, line: usize) -> Option<Lens> {
        let doc = doc(text);
        python_lens(&doc, line, doc.line(line))
    }

    fn test(name: &str, class: Option<&str>) -> Option<Lens> {
        Some(Lens::Test {
            name: name.into(),
            class: class.map(Into::into),
        })
    }

    #[test]
    fn rust_main_and_tests() {
        assert_eq!(rust("fn main() {\n}", 0), Some(Lens::Main));
        assert_eq!(rust("pub fn main() {}", 0), Some(Lens::Main));
        assert_eq!(rust("    fn main() {}", 0), None);
        let tests = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn adds() {}\n    #[tokio::test]\n    // why\n    async fn waits() {}\n    fn helper() {}\n}";
        assert_eq!(rust(tests, 3), test("adds", None));
        assert_eq!(rust(tests, 6), test("waits", None));
        assert_eq!(rust(tests, 7), None);
        assert_eq!(rust("#[derive(Debug)]\nfn make() {}", 1), None);
    }

    #[test]
    fn python_main_and_tests() {
        assert_eq!(
            python("if __name__ == \"__main__\":\n    main()", 0),
            Some(Lens::Main)
        );
        assert_eq!(python("if __name__ == '__main__':", 0), Some(Lens::Main));
        assert_eq!(
            python("def test_add():\n    pass", 0),
            test("test_add", None)
        );
        let classes = "class TestMath:\n    def test_mul(self):\n        pass\n\nclass Helper:\n    def test_no(self):\n        pass";
        assert_eq!(python(classes, 1), test("test_mul", Some("TestMath")));
        assert_eq!(python(classes, 5), None);
        assert_eq!(python("def helper():", 0), None);
    }

    #[test]
    fn go_and_c() {
        assert_eq!(go_lens("func main() {", false), Some(Lens::Main));
        assert_eq!(
            go_lens("func TestAdd(t *testing.T) {", true),
            test("TestAdd", None)
        );
        assert_eq!(go_lens("func Testify(t *testing.T) {", true), None);
        assert_eq!(go_lens("func TestAdd(t *testing.T) {", false), None);
        assert_eq!(c_lens("int main(void) {"), Some(Lens::Main));
        assert_eq!(c_lens("int main (int argc, char **argv)"), Some(Lens::Main));
        assert_eq!(c_lens("int maine() {"), None);
    }
}
