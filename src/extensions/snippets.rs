//! Snippet packs: Zed extensions that bring snippets, such as
//! `python-snippets` or `react-snippets`.
//!
//! An extension names its snippet files in `extension.toml`, one path or a
//! list (checked against fastapi-snippets, angular-snippets and
//! comment-block-snippets in zed-industries/extensions):
//!
//! ```toml
//! snippets = "./snippets/python.json"
//! snippets = ["./snippets/c++.json", "./snippets/shell script.json"]
//! ```
//!
//! A file's name is the language it's for, as Zed names languages but in
//! lower case (`python`, `tsx`, `c++`, `shell script`); `snippets.json`
//! is for every language. Each file is VS Code's snippet format:
//!
//! ```json
//! { "FastAPI app": { "prefix": "fastapp", "body": ["app = FastAPI()", "$0"],
//!                    "description": "A FastAPI application" } }
//! ```
//!
//! `prefix` and `body` may be a string or a list; the body's tab stops are
//! expanded by `lsp::expand_snippet`. Zed reads these files leniently, so
//! `//` and `/* */` comments and trailing commas are accepted too.
//! Nothing here runs at startup: a language's snippets are read when a
//! completion list is first asked for in a file of that language.

use serde::Deserialize;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

/// One snippet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    /// Its key in the file, shown when it has no description.
    pub name: String,
    /// What's typed to find it; at least one.
    pub prefixes: Vec<String>,
    /// Snippet syntax, lines joined with `\n`.
    pub body: String,
    pub description: Option<String>,
}

/// A snippet file bigger than this is refused; real ones are a few KB to a
/// few hundred.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// At most this many snippets are kept for one language.
const MAX_SNIPPETS: usize = 5_000;
/// The file name, without `.json`, of a file for every language.
const ANY_LANGUAGE: &str = "snippets";

#[derive(Deserialize)]
#[serde(untagged)]
enum Paths {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct SnippetManifest {
    #[serde(default)]
    snippets: Option<Paths>,
}

/// The snippet files `extension.toml` names, in its order. A path that
/// isn't a `.json` file inside the extension's folder is left out.
pub fn snippet_files(extension_dir: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(extension_dir.join("extension.toml")) else {
        return Vec::new();
    };
    let Ok(manifest) = toml::from_str::<SnippetManifest>(&text) else {
        return Vec::new();
    };
    let paths = match manifest.snippets {
        Some(Paths::One(path)) => vec![path],
        Some(Paths::Many(paths)) => paths,
        None => Vec::new(),
    };
    paths
        .iter()
        .filter_map(|path| inside(extension_dir, path))
        .collect()
}

// `path` from the manifest under `dir`, if it stays inside it and names a
// .json file.
fn inside(dir: &Path, path: &str) -> Option<PathBuf> {
    let relative = Path::new(path);
    let plain = relative
        .components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir));
    let json = relative
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
    (plain && json && relative.file_stem().is_some()).then(|| dir.join(relative))
}

/// Whether the extension in `extension_dir` names any snippet file.
pub fn is_snippet_pack(extension_dir: &Path) -> bool {
    !snippet_files(extension_dir).is_empty()
}

/// The language a snippet file is for, in lower case, or `None` for a file
/// that applies to every language.
pub fn file_language(file: &Path) -> Option<String> {
    let stem = file.file_stem()?.to_string_lossy().to_lowercase();
    (stem != ANY_LANGUAGE).then_some(stem)
}

/// Checks a downloaded pack before it's installed: every file it names
/// exists, isn't too big and parses. Returns how many snippets it has.
pub fn validate(extension_dir: &Path) -> Result<usize, String> {
    let files = snippet_files(extension_dir);
    if files.is_empty() {
        return Err("Its extension.toml names no snippet file".into());
    }
    let mut count = 0;
    for file in &files {
        let name = file
            .strip_prefix(extension_dir)
            .unwrap_or(file)
            .display()
            .to_string();
        count += read_file(file)
            .map_err(|error| format!("{name}: {error}"))?
            .len();
    }
    if count == 0 {
        return Err("Its snippet files hold no snippets".into());
    }
    Ok(count)
}

/// The languages a pack's files are for, as their files name them, for the
/// message after an install. `None` stands for every language.
pub fn pack_languages(extension_dir: &Path) -> Vec<Option<String>> {
    let mut languages = Vec::new();
    for language in snippet_files(extension_dir)
        .iter()
        .map(|file| file_language(file))
    {
        if !languages.contains(&language) {
            languages.push(language);
        }
    }
    languages
}

fn read_file(file: &Path) -> Result<Vec<Snippet>, String> {
    let size = std::fs::metadata(file)
        .map_err(|error| error.to_string())?
        .len();
    if size > MAX_FILE_BYTES {
        return Err(format!("too big ({} KB)", size / 1024));
    }
    let text = std::fs::read_to_string(file).map_err(|error| error.to_string())?;
    parse(&text)
}

/// The snippets in one file's text, in file order. Entries without a prefix
/// or a body are skipped, as Zed does.
pub fn parse(text: &str) -> Result<Vec<Snippet>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let value: Value = serde_json::from_str(text)
        .or_else(|_| serde_json::from_str(&relaxed_json(text)))
        .map_err(|error| format!("not valid JSON ({error})"))?;
    let Value::Object(entries) = value else {
        return Err("not a JSON object of snippets".into());
    };
    let lines = |value: Option<&Value>| -> Option<Vec<String>> {
        match value? {
            Value::String(text) => Some(vec![text.clone()]),
            Value::Array(items) => items
                .iter()
                .map(|item| item.as_str().map(str::to_string))
                .collect(),
            _ => None,
        }
    };
    let mut snippets = Vec::new();
    for (name, entry) in entries {
        let Some(prefixes) = lines(entry.get("prefix")) else {
            continue;
        };
        let prefixes: Vec<String> = prefixes
            .into_iter()
            .map(|prefix| prefix.trim().to_string())
            .filter(|prefix| !prefix.is_empty())
            .collect();
        let Some(body) = lines(entry.get("body")) else {
            continue;
        };
        if prefixes.is_empty() {
            continue;
        }
        let description = lines(entry.get("description"))
            .map(|lines| lines.join(" "))
            .filter(|text| !text.trim().is_empty());
        snippets.push(Snippet {
            name,
            prefixes,
            body: body.join("\n"),
            description,
        });
    }
    Ok(snippets)
}

// `text` with `//` and `/* */` comments and trailing commas removed, outside
// strings, for files written the way Zed accepts them.
fn relaxed_json(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match (c, chars.get(i + 1)) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            ('/', Some('*')) => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 2;
                continue;
            }
            (',', _) => {
                // A comma whose next significant character closes the
                // object or list is dropped.
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if !matches!(chars.get(j), Some('}' | ']')) {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// The names a file's snippet files may have, most specific first: the
/// extension language coloring it (by its name), else the language Zed
/// gives its extension, with the spellings packs use.
pub fn language_keys(path: Option<&Path>, extension_language: Option<&str>) -> Vec<String> {
    if let Some(name) = extension_language {
        return vec![name.to_lowercase()];
    }
    let extension = path
        .and_then(Path::extension)
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase());
    let names: &[&str] = match extension.as_deref() {
        Some("rs") => &["rust"],
        Some("py" | "pyi" | "pyw") => &["python"],
        Some("js" | "mjs" | "cjs" | "jsx") => &["javascript"],
        Some("ts" | "mts" | "cts") => &["typescript"],
        Some("tsx") => &["tsx"],
        Some("json") => &["json"],
        Some("jsonc") => &["jsonc"],
        Some("c" | "h") => &["c"],
        Some("cc" | "cpp" | "cxx" | "c++" | "hpp" | "hh" | "hxx") => &["c++", "cpp"],
        Some("go") => &["go"],
        Some("html" | "htm") => &["html"],
        Some("css") => &["css"],
        Some("md" | "markdown") => &["markdown"],
        Some("yaml" | "yml") => &["yaml"],
        Some("toml") => &["toml"],
        Some("sh" | "bash" | "zsh") => &["shell script", "shellscript", "bash"],
        Some("java") => &["java"],
        Some("rb") => &["ruby"],
        Some("php") => &["php"],
        Some("lua") => &["lua"],
        Some("cs") => &["csharp", "c#"],
        Some("kt" | "kts") => &["kotlin"],
        Some("swift") => &["swift"],
        Some("dart") => &["dart"],
        Some("vue") => &["vue"],
        Some("svelte") => &["svelte"],
        Some("astro") => &["astro"],
        Some("sql") => &["sql"],
        _ => &[],
    };
    names.iter().map(|name| name.to_string()).collect()
}

/// Every installed snippet for a file whose language goes by `keys`
/// (`language_keys`): its own language's first, then the ones for every
/// language. Reads the installed extensions' manifests and files, so it's
/// called on a worker.
pub fn installed_for(keys: &[String]) -> Vec<Snippet> {
    crate::workflow::extensions_dir()
        .map(|dir| installed_in(&dir, keys))
        .unwrap_or_default()
}

pub(crate) fn installed_in(extensions_dir: &Path, keys: &[String]) -> Vec<Snippet> {
    let Ok(entries) = std::fs::read_dir(extensions_dir) else {
        return Vec::new();
    };
    // Staging and backup folders from an install are skipped.
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .filter(|dir| dir.is_dir())
        .collect();
    folders.sort();
    let (mut own, mut any) = (Vec::new(), Vec::new());
    for folder in folders {
        for file in snippet_files(&folder) {
            let target = match file_language(&file) {
                Some(language) if keys.contains(&language) => &mut own,
                None if !keys.is_empty() => &mut any,
                _ => continue,
            };
            // A pack that changed into something unreadable since it was
            // installed is skipped.
            if let Ok(snippets) = read_file(&file) {
                target.extend(snippets);
            }
        }
    }
    own.extend(any);
    own.truncate(MAX_SNIPPETS);
    own
}

/// The snippets to offer with `before` being the line's text before the
/// caret, each with the byte in `before` its typed prefix starts at.
/// A snippet matches when the end of `before` (from the start of a word, or
/// of the text since the last space) begins one of its prefixes, ignoring
/// case: `fa` and `fastap` both find `fastapp`, and `com-sep` finds
/// `com-sep-sm`. With nothing typed, every snippet is offered.
pub fn matching<'a>(snippets: &'a [Snippet], before: &str) -> Vec<(usize, &'a Snippet)> {
    let token_start = before
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map(|(at, c)| at + c.len_utf8())
        .unwrap_or(0);
    // Where a typed prefix may start: the token's start, and every word
    // start inside it, longest first.
    let mut starts = vec![token_start];
    let token: Vec<(usize, char)> = before[token_start..].char_indices().collect();
    for pair in token.windows(2) {
        let (prev, (at, c)) = (pair[0].1, pair[1]);
        let word = |c: char| c.is_alphanumeric() || c == '_';
        if word(c) && !word(prev) {
            starts.push(token_start + at);
        }
    }
    let end = before.len();
    if token_start == end {
        return snippets.iter().map(|snippet| (end, snippet)).collect();
    }
    snippets
        .iter()
        .filter_map(|snippet| {
            starts
                .iter()
                .find(|&&start| {
                    let typed = before[start..].to_lowercase();
                    snippet
                        .prefixes
                        .iter()
                        .any(|prefix| prefix.to_lowercase().starts_with(&typed))
                })
                .map(|&start| (start, snippet))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lightline-snippets-{label}-{}-{}",
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

    const MANIFEST: &str = "id = \"x\"\nname = \"X\"\nversion = \"1.0.0\"\nschema_version = 1\n";

    #[test]
    fn reads_the_snippet_formats_packs_use() {
        let snippets = parse(
            r#"{
              "FastAPI App": {
                "prefix": "fastapp",
                "body": ["app = FastAPI(", "    title=\"${1:API}\",", ")"],
                "description": "Creates an app."
              },
              "Two prefixes": { "prefix": ["log", "clg"], "body": "console.log($1)" },
              "No prefix": { "body": "x" },
              "No body": { "prefix": "nb" }
            }"#,
        )
        .unwrap();
        assert_eq!(snippets.len(), 2);
        assert_eq!(snippets[0].name, "FastAPI App");
        assert_eq!(snippets[0].prefixes, ["fastapp"]);
        assert_eq!(
            snippets[0].body,
            "app = FastAPI(\n    title=\"${1:API}\",\n)"
        );
        assert_eq!(snippets[0].description.as_deref(), Some("Creates an app."));
        assert_eq!(snippets[1].prefixes, ["log", "clg"]);
        assert_eq!(snippets[1].body, "console.log($1)");
        assert_eq!(snippets[1].description, None);
    }

    #[test]
    fn accepts_comments_and_trailing_commas_outside_strings() {
        let snippets = parse(
            "// Snippets\n{\n  /* block */ \"A\": { \"prefix\": \"a//b\", \"body\": [\"x /* y */\", \"z,\"], },\n}\n",
        )
        .unwrap();
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].prefixes, ["a//b"]);
        assert_eq!(snippets[0].body, "x /* y */\nz,");
        assert!(parse("[1, 2]").is_err());
        assert!(parse("{ nope").is_err());
    }

    #[test]
    fn manifest_paths_stay_inside_the_extension() {
        let dir = fixture("paths");
        write(
            &dir.join("extension.toml"),
            &format!(
                "{MANIFEST}snippets = [\"./snippets/python.json\", \"../evil.json\", \
                 \"C:/abs.json\", \"snippets/readme.md\", \"snippets/shell script.json\"]\n"
            ),
        );
        let files = snippet_files(&dir);
        assert_eq!(
            files,
            [
                dir.join("./snippets/python.json"),
                dir.join("snippets/shell script.json")
            ]
        );
        assert_eq!(file_language(&files[1]).as_deref(), Some("shell script"));
        assert_eq!(file_language(Path::new("x/Snippets.json")), None);

        write(
            &dir.join("extension.toml"),
            &format!("{MANIFEST}snippets = \"snippets/go.json\"\n"),
        );
        assert_eq!(snippet_files(&dir), [dir.join("snippets/go.json")]);
        write(&dir.join("extension.toml"), MANIFEST);
        assert!(!is_snippet_pack(&dir));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn validation_needs_every_file_to_parse() {
        let dir = fixture("validate");
        write(
            &dir.join("extension.toml"),
            &format!("{MANIFEST}snippets = [\"snippets/python.json\", \"snippets/rust.json\"]\n"),
        );
        write(
            &dir.join("snippets/python.json"),
            r#"{ "main": { "prefix": "ifmain", "body": "if __name__ == \"__main__\":\n\t$0" } }"#,
        );
        assert!(validate(&dir).unwrap_err().contains("rust.json"));
        write(&dir.join("snippets/rust.json"), "{ broken");
        assert!(validate(&dir).unwrap_err().contains("not valid JSON"));
        write(
            &dir.join("snippets/rust.json"),
            r##"{ "test": { "prefix": "tst", "body": ["#[test]", "fn $1() {}"] } }"##,
        );
        assert_eq!(validate(&dir), Ok(2));
        assert_eq!(
            pack_languages(&dir),
            [Some("python".to_string()), Some("rust".to_string())]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn installed_snippets_follow_the_files_language() {
        let dir = fixture("installed");
        let pack = |id: &str, files: &[(&str, &str)]| {
            let list: Vec<String> = files
                .iter()
                .map(|(name, _)| format!("\"snippets/{name}\""))
                .collect();
            write(
                &dir.join(id).join("extension.toml"),
                &format!("{MANIFEST}snippets = [{}]\n", list.join(", ")),
            );
            for (name, prefix) in files {
                write(
                    &dir.join(id).join("snippets").join(name),
                    &format!(
                        "{{ \"{prefix}\": {{ \"prefix\": \"{prefix}\", \"body\": \"{prefix}\" }} }}"
                    ),
                );
            }
        };
        pack("py", &[("python.json", "fastapp"), ("tsx.json", "rfc")]);
        pack("all", &[("snippets.json", "lorem")]);
        pack(".staging-other", &[("python.json", "staged")]);
        let prefixes = |keys: &[&str]| -> Vec<String> {
            let keys: Vec<String> = keys.iter().map(|key| key.to_string()).collect();
            installed_in(&dir, &keys)
                .into_iter()
                .map(|snippet| snippet.prefixes[0].clone())
                .collect()
        };
        assert_eq!(prefixes(&["python"]), ["fastapp", "lorem"]);
        assert_eq!(prefixes(&["tsx"]), ["rfc", "lorem"]);
        assert_eq!(prefixes(&["go"]), ["lorem"]);
        // A file with no known language (plain text) gets none.
        assert!(prefixes(&[]).is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn files_go_by_zeds_language_names() {
        let keys = |path: &str| language_keys(Some(Path::new(path)), None);
        assert_eq!(keys("a/main.py"), ["python"]);
        assert_eq!(keys("App.TSX"), ["tsx"]);
        assert_eq!(keys("x.jsx"), ["javascript"]);
        assert_eq!(keys("x.hpp"), ["c++", "cpp"]);
        assert_eq!(keys("run.sh"), ["shell script", "shellscript", "bash"]);
        assert!(keys("notes.txt").is_empty());
        assert!(language_keys(None, None).is_empty());
        assert_eq!(
            language_keys(Some(Path::new("a.ex")), Some("Elixir")),
            ["elixir"]
        );
    }

    #[test]
    fn typed_text_finds_snippets_by_prefix() {
        let snippet = |prefix: &str| Snippet {
            name: prefix.into(),
            prefixes: vec![prefix.into()],
            body: String::new(),
            description: None,
        };
        let all = [snippet("fastapp"), snippet("com-sep-sm"), snippet("!html")];
        let found = |before: &str| -> Vec<(usize, String)> {
            matching(&all, before)
                .into_iter()
                .map(|(at, snippet)| (at, snippet.prefixes[0].clone()))
                .collect()
        };
        assert_eq!(found("    fa"), [(4, "fastapp".to_string())]);
        assert_eq!(found("x = FASTA"), [(4, "fastapp".to_string())]);
        assert_eq!(found("  com-sep"), [(2, "com-sep-sm".to_string())]);
        assert_eq!(found("!ht"), [(0, "!html".to_string())]);
        // A word inside the text since the last space can start one.
        assert_eq!(found("(fast"), [(1, "fastapp".to_string())]);
        assert!(found("zzz").is_empty());
        assert_eq!(found("    ").len(), 3);
    }
}
