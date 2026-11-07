//! Zed language extensions, for syntax colors: a Tree-sitter grammar built
//! from source with the C compiler already on the PC, loaded as a native
//! library, and the extension's `highlights.scm` mapped onto LightLine's
//! colors. This is how Helix and Neovim use Tree-sitter grammars; Zed itself
//! compiles them to WebAssembly instead, which would mean embedding a WASM
//! runtime. Language servers declared by an extension need its compiled
//! WASM code and are not used.
//!
//! An extension's layout (verified against zed-industries' own language
//! extensions):
//!
//! ```text
//! extension.toml            [grammars.<name>] repository = "...", rev = "<sha>"
//! languages/<lang>/config.toml   name, grammar, path_suffixes, ...
//! languages/<lang>/highlights.scm
//! ```
//!
//! At install time each grammar's repository is fetched at its pinned commit
//! and compiled into `grammars/<name>.<dll>` inside the extension folder; the
//! sources are then deleted. Nothing here runs at startup: the installed
//! languages are listed the first time a file no built-in language claims
//! is opened, and a grammar is loaded on the parser thread of the first tab
//! that needs it.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use tree_sitter::{Language, Parser, Query};

/// One `[grammars.<name>]` entry of an extension's `extension.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarSpec {
    pub name: String,
    pub repository: String,
    pub rev: String,
    /// The grammar's folder inside the repository, when it isn't the root
    /// (tree-sitter-typescript keeps `typescript/` and `tsx/` side by side).
    pub path: Option<String>,
}

#[derive(Deserialize)]
struct GrammarEntry {
    repository: String,
    #[serde(alias = "commit")]
    rev: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize)]
struct ManifestGrammars {
    #[serde(default)]
    grammars: HashMap<String, GrammarEntry>,
    #[serde(default)]
    language_servers: HashMap<String, toml::Value>,
}

fn manifest(extension_dir: &Path) -> Option<ManifestGrammars> {
    let text = std::fs::read_to_string(extension_dir.join("extension.toml")).ok()?;
    toml::from_str(&text).ok()
}

/// The grammars an extension declares, sorted by name.
pub fn grammar_specs(extension_dir: &Path) -> Result<Vec<GrammarSpec>, String> {
    let manifest = manifest(extension_dir).ok_or("Invalid extension.toml")?;
    let mut specs: Vec<GrammarSpec> = manifest
        .grammars
        .into_iter()
        .map(|(name, entry)| GrammarSpec {
            name,
            repository: entry.repository,
            rev: entry.rev,
            path: entry.path,
        })
        .collect();
    specs.sort_by(|a, b| a.name.cmp(&b.name));
    for spec in &specs {
        if !valid_grammar_name(&spec.name) {
            return Err(format!("unusable grammar name \"{}\"", spec.name));
        }
        if let Some(path) = &spec.path
            && Path::new(path)
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(format!("grammar \"{}\" has an unusable path", spec.name));
        }
    }
    Ok(specs)
}

// Grammar names become file names and C symbol names.
fn valid_grammar_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// True for an extension that adds languages: it has a `languages/` folder
/// and at least one grammar to color them with.
pub fn is_language_extension(extension_dir: &Path) -> bool {
    extension_dir.join("languages").is_dir()
        && manifest(extension_dir).is_some_and(|manifest| !manifest.grammars.is_empty())
}

/// True when the extension also declares language servers, which LightLine
/// can't start: they come from the extension's compiled WASM code.
pub fn has_language_servers(extension_dir: &Path) -> bool {
    manifest(extension_dir).is_some_and(|manifest| !manifest.language_servers.is_empty())
}

/// The parts of `languages/<lang>/config.toml` LightLine uses.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LanguageConfig {
    pub name: String,
    #[serde(default)]
    pub grammar: Option<String>,
    #[serde(default)]
    pub path_suffixes: Vec<String>,
}

/// Each language folder of an extension with its parsed config, sorted by
/// folder. Folders whose config doesn't parse are skipped.
pub fn language_configs(extension_dir: &Path) -> Vec<(PathBuf, LanguageConfig)> {
    let Ok(entries) = std::fs::read_dir(extension_dir.join("languages")) else {
        return Vec::new();
    };
    let mut configs: Vec<(PathBuf, LanguageConfig)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| dir.is_dir())
        .filter_map(|dir| {
            let text = std::fs::read_to_string(dir.join("config.toml")).ok()?;
            let config: LanguageConfig = toml::from_str(&text).ok()?;
            Some((dir, config))
        })
        .collect();
    configs.sort_by(|a, b| a.0.cmp(&b.0));
    configs
}

fn library_path(extension_dir: &Path, grammar: &str) -> PathBuf {
    extension_dir
        .join("grammars")
        .join(format!("{grammar}.{}", std::env::consts::DLL_EXTENSION))
}

fn symbol_name(grammar: &str) -> String {
    format!("tree_sitter_{}", grammar.replace('-', "_"))
}

/// Fetches and compiles every grammar the extension declares into its
/// `grammars/` folder, removing the sources afterwards. `fetch(folder, url,
/// commit)` downloads exactly that commit into `folder`.
pub fn build_grammars(
    extension_dir: &Path,
    fetch: impl Fn(&Path, &str, &str) -> Result<(), String>,
) -> Result<(), String> {
    let specs = grammar_specs(extension_dir)?;
    let compiler = find_compiler().ok_or(
        "needs a C compiler to build its grammar: install the Visual Studio C++ Build Tools, \
         or put gcc or clang on PATH",
    )?;
    let grammars = extension_dir.join("grammars");
    std::fs::create_dir_all(&grammars).map_err(|error| error.to_string())?;
    for spec in specs {
        let source = extension_dir.join(format!(".grammar-src-{}", spec.name));
        let built = (|| {
            fetch(&source, &spec.repository, &spec.rev)?;
            let root = match &spec.path {
                Some(path) => source.join(path),
                None => source.clone(),
            };
            compile(
                &compiler,
                &spec.name,
                &root.join("src"),
                &library_path(extension_dir, &spec.name),
            )
        })();
        let _ = std::fs::remove_dir_all(&source);
        built.map_err(|error| format!("could not build the {} grammar: {error}", spec.name))?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum Compiler {
    // cl.exe, with the environment the Visual Studio installation needs.
    Msvc(find_msvc_tools::Tool),
    // gcc/clang on PATH, and the C++ driver for C++ scanners.
    Gnu { c: &'static str, cpp: &'static str },
}

fn find_compiler() -> Option<Compiler> {
    if let Some(tool) = find_msvc_tools::find_tool(std::env::consts::ARCH, "cl.exe") {
        return Some(Compiler::Msvc(tool));
    }
    if crate::workflow::command_available("gcc") {
        return Some(Compiler::Gnu {
            c: "gcc",
            cpp: "g++",
        });
    }
    crate::workflow::command_available("clang").then_some(Compiler::Gnu {
        c: "clang",
        cpp: "clang++",
    })
}

// Builds `<src>/parser.c` and the grammar's scanner, if it has one, into
// the shared library `out`. Intermediate files go to a folder next to `out`
// that is removed afterwards.
fn compile(compiler: &Compiler, name: &str, src: &Path, out: &Path) -> Result<(), String> {
    let parser = src.join("parser.c");
    if !parser.is_file() {
        return Err("the grammar has no src/parser.c".into());
    }
    let scanner = ["scanner.c", "scanner.cc", "scanner.cpp"]
        .iter()
        .map(|file| src.join(file))
        .find(|path| path.is_file());
    let cpp_scanner = scanner
        .as_ref()
        .is_some_and(|path| path.extension().is_some_and(|ext| ext != "c"));
    let work = out.with_extension("build");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|error| error.to_string())?;
    let library = work.join(out.file_name().ok_or("bad output path")?);
    let mut command = match compiler {
        Compiler::Msvc(cl) => {
            let mut command = cl.to_command();
            command.args(["/nologo", "/O2", "/LD", "/utf-8", "/EHsc"]);
            command.arg(format!("/I{}", src.display()));
            command.arg(&parser);
            command.args(scanner.iter());
            command.arg(format!("/Fe{}", library.display()));
            // Older generated parsers don't mark their entry point exported.
            command.args(["/link", &format!("/EXPORT:{}", symbol_name(name))]);
            command
        }
        Compiler::Gnu { c, cpp } => {
            let mut command = Command::new(if cpp_scanner { cpp } else { c });
            command.args(["-shared", "-O2", "-fPIC"]);
            command.arg("-I").arg(src);
            if cpp_scanner {
                command.args(["-x", "c"]).arg(&parser);
                command.args(["-x", "c++"]).args(scanner.iter());
                command.arg("-static-libstdc++");
            } else {
                command.arg(&parser).args(scanner.iter());
            }
            if *c == "gcc" {
                command.arg("-static-libgcc");
            }
            command.arg("-o").arg(&library);
            command
        }
    };
    command.current_dir(&work);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let result = crate::jobs::output(&mut command)
        .map_err(|error| format!("could not run the C compiler: {error}"))
        .and_then(|output| {
            if output.status.success() && library.is_file() {
                std::fs::rename(&library, out).map_err(|error| error.to_string())
            } else {
                // cl reports errors on stdout, gcc and clang on stderr.
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let last: Vec<&str> = text
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .rev()
                    .take(3)
                    .collect();
                let detail = last.into_iter().rev().collect::<Vec<_>>().join(" / ");
                Err(format!("the C compiler failed: {detail}"))
            }
        });
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Checks a downloaded language extension before it replaces anything:
/// every language names a grammar that was built, the grammar loads, and its
/// highlight query compiles against it. Returns the language names.
pub fn validate(extension_dir: &Path) -> Result<Vec<String>, String> {
    let configs = language_configs(extension_dir);
    if configs.is_empty() {
        return Err("the extension's languages could not be read".into());
    }
    let mut names = Vec::new();
    for (dir, config) in configs {
        let grammar = config
            .grammar
            .as_deref()
            .ok_or_else(|| format!("{} names no grammar", config.name))?;
        let library = library_path(extension_dir, grammar);
        let highlights = std::fs::read_to_string(dir.join("highlights.scm")).unwrap_or_default();
        probe(&library, grammar, &highlights)
            .map_err(|error| format!("{}: {error}", config.name))?;
        names.push(config.name);
    }
    Ok(names)
}

// Loads `library`, checks it is a grammar this Tree-sitter can use, compiles
// the query against it, and unloads it again (so the folder can still be
// moved into place). Locals drop in reverse order: everything that points
// into the library goes before the library itself.
fn probe(library: &Path, grammar: &str, highlights: &str) -> Result<(), String> {
    // SAFETY: the library was just built from the grammar's C sources;
    // loading it runs only the C runtime's initialisation.
    let loaded = unsafe { libloading::Library::new(library) }
        .map_err(|error| format!("its grammar could not be loaded: {error}"))?;
    let language = language_from(&loaded, grammar)?;
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|error| format!("its grammar can't be used: {error}"))?;
    Query::new(&language, highlights)
        .map_err(|error| format!("its highlight query doesn't fit its grammar: {error}"))?;
    drop(parser);
    drop(language);
    drop(loaded);
    Ok(())
}

fn language_from(library: &libloading::Library, grammar: &str) -> Result<Language, String> {
    let symbol = symbol_name(grammar);
    // SAFETY: Tree-sitter grammars export `const TSLanguage *tree_sitter_<name>(void)`.
    let entry: libloading::Symbol<unsafe extern "C" fn() -> *const ()> =
        unsafe { library.get(symbol.as_bytes()) }
            .map_err(|_| format!("its grammar has no {symbol}"))?;
    // SAFETY: as above; the pointer stays valid while the library is loaded.
    Ok(Language::new(unsafe {
        tree_sitter_language::LanguageFn::from_raw(*entry)
    }))
}

/// An installed language a file can be colored as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionLanguage {
    pub name: String,
    grammar: String,
    library: PathBuf,
    highlights: PathBuf,
    suffixes: Vec<String>,
}

// The installed languages, listed on first use and again after an install
// or removal.
static INSTALLED: Mutex<Option<Arc<Vec<Arc<ExtensionLanguage>>>>> = Mutex::new(None);

/// The installed language for `path`, matched by its extension or whole
/// file name against the languages' `path_suffixes`.
pub fn for_path(path: &Path) -> Option<Arc<ExtensionLanguage>> {
    let languages = {
        let mut installed = INSTALLED.lock().ok()?;
        installed
            .get_or_insert_with(|| {
                Arc::new(
                    crate::workflow::extensions_dir()
                        .map(|dir| installed_in(&dir))
                        .unwrap_or_default(),
                )
            })
            .clone()
    };
    match_path(&languages, path)
}

/// Forgets the listed languages, so the next lookup sees an install or
/// removal. Grammars already loaded stay loaded.
pub fn reload() {
    if let Ok(mut installed) = INSTALLED.lock() {
        *installed = None;
    }
}

fn match_path(languages: &[Arc<ExtensionLanguage>], path: &Path) -> Option<Arc<ExtensionLanguage>> {
    let file_name = path.file_name()?.to_str()?;
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    languages
        .iter()
        .find(|language| {
            language
                .suffixes
                .iter()
                .any(|suffix| suffix == file_name || Some(suffix.to_ascii_lowercase()) == extension)
        })
        .cloned()
}

// Every usable language in an extensions folder: its grammar was built and
// it has a highlight query. Staging and backup folders are skipped.
fn installed_in(extensions_dir: &Path) -> Vec<Arc<ExtensionLanguage>> {
    let Ok(entries) = std::fs::read_dir(extensions_dir) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .filter(|dir| dir.join("languages").is_dir() && dir.join("grammars").is_dir())
        .collect();
    folders.sort();
    let mut languages = Vec::new();
    for folder in folders {
        for (dir, config) in language_configs(&folder) {
            let Some(grammar) = config.grammar else {
                continue;
            };
            let library = library_path(&folder, &grammar);
            let highlights = dir.join("highlights.scm");
            if library.is_file() && highlights.is_file() && !config.path_suffixes.is_empty() {
                languages.push(Arc::new(ExtensionLanguage {
                    name: config.name,
                    grammar,
                    library,
                    highlights,
                    suffixes: config.path_suffixes,
                }));
            }
        }
    }
    languages
}

// Grammars loaded so far, by library path. A loaded grammar is never
// unloaded: parse trees and queries point into it.
static LOADED: Mutex<Vec<(PathBuf, Language)>> = Mutex::new(Vec::new());

/// The language's grammar and highlight query, loading the grammar the
/// first time. Called on a parser thread.
pub(crate) fn load(language: &ExtensionLanguage) -> Result<(Language, String), String> {
    let highlights =
        std::fs::read_to_string(&language.highlights).map_err(|error| error.to_string())?;
    let mut loaded = LOADED.lock().map_err(|error| error.to_string())?;
    if let Some((_, grammar)) = loaded.iter().find(|(path, _)| *path == language.library) {
        return Ok((grammar.clone(), highlights));
    }
    // SAFETY: a library this extension's install built and validated.
    let library = unsafe { libloading::Library::new(&language.library) }
        .map_err(|error| error.to_string())?;
    let grammar = language_from(&library, &language.grammar)?;
    std::mem::forget(library);
    loaded.push((language.library.clone(), grammar.clone()));
    Ok((grammar, highlights))
}

/// How a capture name from an extension's `highlights.scm` (Zed's names:
/// `@keyword`, `@string.escape`, `@function.method`...) maps onto
/// LightLine's colors. Names without a match (variables, punctuation,
/// properties) stay the default text color.
pub fn capture_color(name: &str) -> Option<crate::syntax::Color> {
    use crate::syntax::Color;
    let head = name.split('.').next().unwrap_or(name);
    Some(match head {
        "comment" => Color::Comment,
        "string" | "character" | "escape" | "regex" => Color::String,
        "keyword" | "boolean" | "conditional" | "repeat" | "include" | "exception" => {
            Color::Keyword
        }
        "type" | "constructor" | "enum" | "tag" | "namespace" | "module" => Color::Type,
        "number" | "float" | "constant" => Color::Number,
        "preproc" | "macro" => Color::Macro,
        "function" | "method" => {
            if name == "function.macro" {
                Color::Macro
            } else {
                Color::Function
            }
        }
        "operator" => Color::Operator,
        "attribute" | "decorator" | "label" => Color::Attribute,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Color;
    use std::fs;

    fn fixture(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lightline-language-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn grammars_and_language_servers_are_read_from_the_manifest() {
        let dir = fixture("manifest");
        write(
            &dir.join("extension.toml"),
            r#"
            id = "elixir"
            name = "Elixir"
            version = "0.1.0"
            [grammars.heex]
            repository = "https://github.com/phoenixframework/tree-sitter-heex"
            commit = "abc"
            [grammars.elixir]
            repository = "https://github.com/elixir-lang/tree-sitter-elixir"
            rev = "def"
            path = "sub/dir"
            [language_servers.elixir-ls]
            name = "ElixirLS"
            languages = ["Elixir"]
            "#,
        );
        fs::create_dir(dir.join("languages")).unwrap();
        let specs = grammar_specs(&dir).unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].name, "elixir");
        assert_eq!(specs[0].rev, "def");
        assert_eq!(specs[0].path.as_deref(), Some("sub/dir"));
        assert_eq!(specs[1].rev, "abc");
        assert!(is_language_extension(&dir));
        assert!(has_language_servers(&dir));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn grammar_names_and_paths_cannot_escape_the_extension() {
        let dir = fixture("escape");
        for grammar in [
            "[grammars.\"../x\"]\nrepository = \"r\"\nrev = \"c\"",
            "[grammars.x]\nrepository = \"r\"\nrev = \"c\"\npath = \"../up\"",
            "[grammars.x]\nrepository = \"r\"\nrev = \"c\"\npath = \"C:/abs\"",
        ] {
            write(
                &dir.join("extension.toml"),
                &format!("id='x'\nname='X'\nversion='1'\n{grammar}\n"),
            );
            assert!(grammar_specs(&dir).is_err(), "{grammar}");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn themes_are_not_language_extensions() {
        let dir = fixture("theme");
        write(
            &dir.join("extension.toml"),
            "id='dracula'\nname='Dracula'\nversion='1'\n",
        );
        fs::create_dir(dir.join("themes")).unwrap();
        assert!(!is_language_extension(&dir));
        assert!(!has_language_servers(&dir));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn installed_languages_match_files_by_suffix_or_name() {
        let dir = fixture("installed");
        let ext = dir.join("elixir");
        write(
            &ext.join("languages/elixir/config.toml"),
            "name = \"Elixir\"\ngrammar = \"elixir\"\npath_suffixes = [\"ex\", \"exs\", \"mix.lock\"]\nline_comments = [\"# \"]\n",
        );
        write(
            &ext.join("languages/elixir/highlights.scm"),
            "(comment) @comment",
        );
        // Built grammar present.
        write(&library_path(&ext, "elixir"), "");
        // A language whose grammar was never built is not offered.
        write(
            &ext.join("languages/heex/config.toml"),
            "name = \"HEEx\"\ngrammar = \"heex\"\npath_suffixes = [\"heex\"]\n",
        );
        write(&ext.join("languages/heex/highlights.scm"), "");
        // A staging folder from an unfinished install is ignored.
        write(
            &dir.join(".staging-other/languages/x/config.toml"),
            "name = \"X\"\ngrammar = \"x\"\npath_suffixes = [\"x\"]\n",
        );
        let languages = installed_in(&dir);
        assert_eq!(languages.len(), 1);
        let found = |path: &str| match_path(&languages, Path::new(path)).map(|l| l.name.clone());
        assert_eq!(found("lib/app.ex").as_deref(), Some("Elixir"));
        assert_eq!(found("TEST.EXS").as_deref(), Some("Elixir"));
        assert_eq!(found("project/mix.lock").as_deref(), Some("Elixir"));
        assert_eq!(found("page.heex"), None);
        assert_eq!(found("main.rs"), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn zed_capture_names_map_onto_lightline_colors() {
        assert_eq!(capture_color("comment.doc"), Some(Color::Comment));
        assert_eq!(capture_color("string.escape"), Some(Color::String));
        assert_eq!(capture_color("keyword.control"), Some(Color::Keyword));
        assert_eq!(capture_color("boolean"), Some(Color::Keyword));
        assert_eq!(capture_color("type.builtin"), Some(Color::Type));
        assert_eq!(capture_color("constant.builtin"), Some(Color::Number));
        assert_eq!(capture_color("function.method"), Some(Color::Function));
        assert_eq!(capture_color("function.macro"), Some(Color::Macro));
        assert_eq!(capture_color("attribute"), Some(Color::Attribute));
        assert_eq!(capture_color("variable"), None);
        assert_eq!(capture_color("punctuation.bracket"), None);
    }

    // A git repository at `dir` holding its current files, one commit.
    fn commit_all(dir: &Path) -> String {
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "git {args:?}: {output:?}");
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "fixture"]);
        git(&["rev-parse", "HEAD"])
    }

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap().flatten() {
            let target = to.join(entry.file_name());
            if entry.path().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    // The tree-sitter-json crate's sources, as Cargo unpacked them: a real
    // grammar repository's `src/` and `queries/`, without the network.
    fn cached_json_grammar() -> Option<PathBuf> {
        let cargo_home = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("USERPROFILE").map(|home| Path::new(&home).join(".cargo"))
            })?;
        fs::read_dir(cargo_home.join("registry").join("src"))
            .ok()?
            .flatten()
            .map(|index| index.path().join("tree-sitter-json-0.24.8"))
            .find(|dir| dir.join("src").join("parser.c").is_file())
    }

    #[test]
    #[ignore = "builds a grammar with the PC's C compiler; run explicitly with --ignored"]
    fn a_language_extension_installs_builds_its_grammar_and_colors_files() {
        let Some(json) = cached_json_grammar() else {
            eprintln!("skipped: tree-sitter-json sources not in the Cargo cache");
            return;
        };
        assert!(find_compiler().is_some(), "no C compiler found");
        let root = fixture("e2e");
        // The grammar's own repository.
        let grammar_repo = root.join("tree-sitter-json");
        copy_dir(&json.join("src"), &grammar_repo.join("src"));
        let grammar_commit = commit_all(&grammar_repo);
        // The extension's repository, pinning the grammar by commit.
        let extension_repo = root.join("extension");
        write(
            &extension_repo.join("extension.toml"),
            &format!(
                "id = \"jsonx\"\nname = \"JSONX\"\nversion = \"0.1.0\"\n\n\
                 [grammars.json]\nrepository = \"{}\"\nrev = \"{grammar_commit}\"\n\n\
                 [language_servers.jsonx-ls]\nname = \"JSONX LS\"\nlanguages = [\"JSONX\"]\n",
                grammar_repo.display().to_string().replace('\\', "/")
            ),
        );
        write(
            &extension_repo.join("languages/jsonx/config.toml"),
            "name = \"JSONX\"\ngrammar = \"json\"\npath_suffixes = [\"jsonx\"]\n",
        );
        fs::copy(
            json.join("queries").join("highlights.scm"),
            extension_repo.join("languages/jsonx/highlights.scm"),
        )
        .unwrap();
        let extension_commit = commit_all(&extension_repo);

        let extensions = root.join("extensions");
        let installed = crate::extensions::installer::install_in(
            &extensions,
            "jsonx",
            &extension_repo.display().to_string(),
            &extension_commit,
        )
        .unwrap();
        assert!(library_path(&installed, "json").is_file());
        assert!(!installed.join(".grammar-src-json").exists());
        assert!(!installed.join("grammars/json.build").exists());
        assert!(has_language_servers(&installed));

        let languages = installed_in(&extensions);
        let language = match_path(&languages, Path::new("data/settings.jsonx")).unwrap();
        assert_eq!(language.name, "JSONX");

        let mut document = crate::document::Document::new();
        document.replace(
            crate::document::Pos::default(),
            crate::document::Pos::default(),
            "{\"name\": \"LightLine\", \"fast\": true, \"version\": 8}",
        );
        let mut syntax = crate::syntax::Syntax::new_extension(language);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !syntax.advance_to(&document, 0, 100) {
            assert!(
                std::time::Instant::now() < deadline,
                "the parse never landed"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let colors: Vec<Color> = syntax
            .spans(&document, 0)
            .iter()
            .map(|span| span.color)
            .collect();
        assert!(colors.contains(&Color::String), "{colors:?}");
        // `8` and `true` (`@constant.builtin`).
        assert_eq!(
            colors
                .iter()
                .filter(|&&color| color == Color::Number)
                .count(),
            2,
            "{colors:?}"
        );
        // The loaded grammar stays mapped for the rest of the process.
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_parser_is_reported_not_compiled() {
        let dir = fixture("no-parser");
        let compiler = Compiler::Gnu {
            c: "gcc",
            cpp: "g++",
        };
        let error = compile(&compiler, "x", &dir, &dir.join("x.dll")).unwrap_err();
        assert!(error.contains("parser.c"), "{error}");
        fs::remove_dir_all(dir).unwrap();
    }
}
