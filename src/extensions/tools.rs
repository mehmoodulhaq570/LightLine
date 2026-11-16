//! Tools: extensions whose only part is a language server that checks
//! files of many languages, such as the spell checkers typos, codebook,
//! harper and cspell. They run beside a file's own language server, and
//! their problems are shown with its own.
//!
//! Such an extension declares its server for the languages it checks, by
//! the names Zed gives languages, and has no `languages/` folder of its own
//! (checked against zed-typos, codebook-zed, harper and zed-cspell):
//!
//! ```toml
//! [language_servers.typos]
//! name = "typos"
//! languages = ["Rust", "Python", "Markdown", "Plain Text", ...]
//! ```
//!
//! Zed runs the extension's WebAssembly to download the server. LightLine
//! doesn't run that code: it starts the server only when its command is on
//! PATH (`language_servers::KNOWN`, or the `languageServers` setting), and
//! downloads nothing. The installed tools are listed on a worker, never at
//! startup.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// One tool's server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    /// The extension's folder name (its registry id).
    pub extension: String,
    /// The extension's name, for messages.
    pub title: String,
    /// The server's declared name, such as `typos` or `harper-ls`.
    pub server: String,
    /// The languages it checks, in lower case (`rust`, `plain text`).
    pub languages: Vec<String>,
}

impl Tool {
    /// Whether it checks a file whose language goes by one of `keys`
    /// (`snippets::language_keys`, lower case).
    pub fn checks(&self, keys: &[String]) -> bool {
        keys.iter().any(|key| self.languages.contains(key))
    }
}

#[derive(Deserialize, Default)]
struct ServerEntry {
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    languages: Vec<String>,
}

#[derive(Deserialize)]
struct ToolManifest {
    #[serde(default)]
    name: String,
    #[serde(default)]
    language_servers: HashMap<String, ServerEntry>,
}

/// The tools an extension brings: its servers, when it adds no language of
/// its own (a language extension's servers serve its languages instead).
pub fn tools_of(extension_dir: &Path) -> Vec<Tool> {
    if extension_dir.join("languages").is_dir() {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(extension_dir.join("extension.toml")) else {
        return Vec::new();
    };
    let Ok(manifest) = toml::from_str::<ToolManifest>(&text) else {
        return Vec::new();
    };
    let extension = extension_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut tools: Vec<Tool> = manifest
        .language_servers
        .into_iter()
        .filter_map(|(server, entry)| {
            let mut languages: Vec<String> = entry
                .languages
                .iter()
                .chain(entry.language.iter())
                .map(|language| language.to_lowercase())
                .collect();
            languages.sort();
            languages.dedup();
            (!languages.is_empty()).then(|| Tool {
                extension: extension.clone(),
                title: manifest.name.clone(),
                server,
                languages,
            })
        })
        .collect();
    tools.sort_by(|a, b| a.server.cmp(&b.server));
    tools
}

/// Whether the extension in `extension_dir` is a tool.
pub fn is_tool_extension(extension_dir: &Path) -> bool {
    !tools_of(extension_dir).is_empty()
}

/// How to get a tool's command when it isn't on PATH, for the status bar.
pub fn install_hint(server: &str) -> Option<&'static str> {
    Some(match server {
        "typos" => {
            "get typos-lsp.exe from github.com/tekumara/typos-lsp/releases and put it on PATH, \
             or run cargo install typos-lsp"
        }
        "codebook" => {
            "get codebook-lsp.exe from github.com/blopker/codebook/releases and put it on PATH, \
             or run cargo install codebook-lsp"
        }
        "harper-ls" => {
            "get harper-ls.exe from github.com/Automattic/harper/releases and put it on PATH, \
             or run cargo install harper-ls --locked"
        }
        "cspell" => "install Node.js, then run npm install -g @vlabo/cspell-lsp",
        _ => return None,
    })
}

// Listed by `list_installed` (on a worker), dropped by `reload`.
static INSTALLED: Mutex<Option<Arc<Vec<Tool>>>> = Mutex::new(None);

/// The installed tools if they've been listed; None until `list_installed`
/// has run once (and again after `reload`).
pub fn installed_if_listed() -> Option<Arc<Vec<Tool>>> {
    INSTALLED.lock().ok()?.clone()
}

/// Lists the installed tools (reads each extension's `extension.toml`).
/// Called on a worker.
pub fn list_installed() -> Arc<Vec<Tool>> {
    let tools = Arc::new(
        crate::workflow::extensions_dir()
            .map(|dir| installed_in(&dir))
            .unwrap_or_default(),
    );
    if let Ok(mut installed) = INSTALLED.lock() {
        *installed = Some(tools.clone());
    }
    tools
}

/// Forgets the list, so the next one sees an install or removal.
pub fn reload() {
    if let Ok(mut installed) = INSTALLED.lock() {
        *installed = None;
    }
}

fn installed_in(extensions_dir: &Path) -> Vec<Tool> {
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
    folders.iter().flat_map(|dir| tools_of(dir)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lightline-tools-{label}-{}-{}",
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

    const TYPOS: &str = r#"
id = "typos"
name = "Typos spell checker"
version = "0.0.5"
schema_version = 1

[language_servers.typos]
name = "typos"
languages = ["Rust", "Python", "Plain Text", "Shell Script", "C++"]
"#;

    #[test]
    fn a_server_without_languages_of_its_own_is_a_tool() {
        let dir = fixture("typos");
        let ext = dir.join("typos");
        write(&ext.join("extension.toml"), TYPOS);
        let tools = tools_of(&ext);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].extension, "typos");
        assert_eq!(tools[0].title, "Typos spell checker");
        assert_eq!(tools[0].server, "typos");
        assert_eq!(
            tools[0].languages,
            ["c++", "plain text", "python", "rust", "shell script"]
        );
        assert!(tools[0].checks(&["python".to_string()]));
        assert!(tools[0].checks(&["shell script".to_string(), "bash".to_string()]));
        assert!(!tools[0].checks(&["go".to_string()]));
        assert!(!tools[0].checks(&[]));

        // The older single `language`.
        write(
            &ext.join("extension.toml"),
            "id = \"x\"\nname = \"X\"\nversion = \"1\"\n[language_servers.x-ls]\nlanguage = \"Go\"\n",
        );
        assert_eq!(tools_of(&ext)[0].languages, ["go"]);

        // A language extension's servers serve its own languages instead.
        fs::create_dir_all(ext.join("languages/lua")).unwrap();
        assert!(!is_tool_extension(&ext));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn themes_and_snippet_packs_are_not_tools() {
        let dir = fixture("others");
        write(
            &dir.join("extension.toml"),
            "id = \"t\"\nname = \"T\"\nversion = \"1\"\nsnippets = \"snippets/rust.json\"\n",
        );
        assert!(!is_tool_extension(&dir));
        // A declared server with no languages checks nothing.
        write(
            &dir.join("extension.toml"),
            "id = \"t\"\nname = \"T\"\nversion = \"1\"\n[language_servers.t]\nname = \"t\"\n",
        );
        assert!(!is_tool_extension(&dir));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn installed_tools_skip_staging_folders() {
        let dir = fixture("installed");
        write(&dir.join("typos/extension.toml"), TYPOS);
        write(&dir.join(".staging-typos2/extension.toml"), TYPOS);
        write(&dir.join("dracula/themes/dracula.json"), "{}");
        let tools = installed_in(&dir);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].extension, "typos");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn each_known_tool_has_a_command_and_a_hint() {
        for server in ["typos", "codebook", "harper-ls", "cspell"] {
            assert!(
                crate::extensions::language_servers::known_command(server).is_some(),
                "{server}"
            );
            assert!(install_hint(server).is_some(), "{server}");
        }
    }
}
