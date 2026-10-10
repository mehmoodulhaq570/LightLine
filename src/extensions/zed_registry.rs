//! Resolves an extension id to its Zed registry entry: the repository and
//! the exact commit the registry pins. zed-industries/extensions lists ids in
//! `extensions.toml` and pins each extension as a git submodule; GitHub's
//! contents API reports a submodule's repository and commit in one request,
//! so nothing has to be cloned to find them.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const EXTENSIONS_TOML_URL: &str =
    "https://raw.githubusercontent.com/zed-industries/extensions/main/extensions.toml";
const CONTENTS_API: &str = "https://api.github.com/repos/zed-industries/extensions/contents";
// A request that takes longer is treated as failed rather than left hanging.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
// The registry list is reused this long: searching and then installing
// would otherwise download it twice.
const LIST_REUSE: Duration = Duration::from_secs(10 * 60);

#[derive(Deserialize)]
struct RegistryEntry {
    submodule: String,
    version: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedExtension {
    pub id: String,
    pub version: String,
    pub git_url: String,
    /// The commit the registry pins: what gets installed.
    pub commit: String,
}

/// Looks up `id` in the live registry and resolves it to a repository and
/// the commit to install.
pub fn resolve(id: &str) -> Result<ResolvedExtension, String> {
    let entries = registry()?;
    let entry = entries
        .get(id)
        .ok_or_else(|| format!("\"{id}\" is not in the Zed extensions registry"))?;
    let contents = fetch(&format!("{CONTENTS_API}/{}?ref=main", entry.submodule))?;
    let (git_url, commit) = parse_submodule(&contents).ok_or_else(|| {
        format!("the registry lists \"{id}\" but its repository could not be resolved")
    })?;
    Ok(ResolvedExtension {
        id: id.to_string(),
        version: entry.version.clone(),
        git_url,
        commit,
    })
}

/// Lists every id (and its version) in the registry, for the Extensions
/// panel's search.
pub fn list_ids() -> Result<Vec<(String, String)>, String> {
    let mut ids: Vec<(String, String)> = registry()?
        .into_iter()
        .map(|(id, entry)| (id, entry.version))
        .collect();
    ids.sort();
    Ok(ids)
}

fn registry() -> Result<HashMap<String, RegistryEntry>, String> {
    static LIST: Mutex<Option<(Instant, String)>> = Mutex::new(None);
    let cached = LIST
        .lock()
        .ok()
        .and_then(|list| list.clone())
        .filter(|(fetched, _)| fetched.elapsed() < LIST_REUSE)
        .map(|(_, text)| text);
    let text = match cached {
        Some(text) => text,
        None => {
            let text = fetch(EXTENSIONS_TOML_URL)?;
            if let Ok(mut list) = LIST.lock() {
                *list = Some((Instant::now(), text.clone()));
            }
            text
        }
    };
    toml::from_str(&text)
        .map_err(|error| format!("could not parse the Zed extensions registry: {error}"))
}

fn fetch(url: &str) -> Result<String, String> {
    let mut response = ureq::get(url)
        .config()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .call()
        .map_err(|error| format!("could not reach {url}: {error}"))?;
    response
        .body_mut()
        .with_config()
        .limit(16 * 1024 * 1024)
        .read_to_string()
        .map_err(|error| format!("could not read the response from {url}: {error}"))
}

// The repository and commit in GitHub's contents-API answer for a
// submodule. Only an https repository and a full commit id are accepted:
// both are handed to git.
fn parse_submodule(json: &str) -> Option<(String, String)> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    if value.get("type")?.as_str()? != "submodule" {
        return None;
    }
    let url = value.get("submodule_git_url")?.as_str()?;
    let commit = value.get("sha")?.as_str()?;
    let full_commit = commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit());
    (url.starts_with("https://") && full_commit).then(|| (url.to_owned(), commit.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submodule_answers_give_the_repository_and_pinned_commit() {
        let answer = r#"{"type":"submodule","sha":"92f46057151693648bd3acac1e0c138696f34433",
            "submodule_git_url":"https://github.com/dracula/zed.git","path":"extensions/dracula"}"#;
        assert_eq!(
            parse_submodule(answer),
            Some((
                "https://github.com/dracula/zed.git".into(),
                "92f46057151693648bd3acac1e0c138696f34433".into()
            ))
        );
        // Anything git could misread is refused.
        for bad in [
            answer.replace("https://", "ext::"),
            answer.replace("92f46057", "--upload"),
            answer.replace("\"submodule\"", "\"dir\""),
        ] {
            assert_eq!(parse_submodule(&bad), None, "{bad}");
        }
    }

    // Live network tests, same spirit as the LSP live tests: skipped by
    // default, run explicitly to prove this resolves against the real
    // registry, not a fixture.
    #[test]
    #[ignore = "hits the real network; run explicitly with --ignored"]
    fn resolves_the_real_material_icon_theme_entry() {
        let resolved =
            resolve("material-icon-theme").expect("should resolve from the live registry");
        assert_eq!(
            resolved.git_url,
            "https://github.com/zed-extensions/material-icon-theme.git"
        );
        assert_eq!(resolved.commit.len(), 40);
    }

    #[test]
    #[ignore = "hits the real network; run explicitly with --ignored"]
    fn lists_hundreds_of_real_ids_including_material_icon_theme() {
        let ids = list_ids().expect("should list from the live registry");
        assert!(
            ids.len() > 100,
            "expected hundreds of real registry entries, got {}",
            ids.len()
        );
        assert!(ids.iter().any(|(id, _)| id == "material-icon-theme"));
    }
}
