//! Resolves an extension id to its real Zed registry entry (git URL +
//! pinned version) by fetching zed-industries/extensions' own
//! `extensions.toml` and `.gitmodules` at runtime -- the same two files
//! this project's manual `gh api` lookups read this session to find
//! `material-icon-theme`'s real repository, now done in Rust.
//!
//! Deliberately not cloning the whole registry repo: `extensions.toml` maps
//! an id to a submodule path (`[material-icon-theme] submodule =
//! "extensions/material-icon-theme"`), and `.gitmodules` maps that path to
//! the actual git URL -- two small text fetches instead of a multi-hundred
//! megabyte clone.

use serde::Deserialize;
use std::collections::HashMap;

const EXTENSIONS_TOML_URL: &str =
    "https://raw.githubusercontent.com/zed-industries/extensions/main/extensions.toml";
const GITMODULES_URL: &str =
    "https://raw.githubusercontent.com/zed-industries/extensions/main/.gitmodules";

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
}

/// Looks up `id` in the live registry and resolves it to a clonable git URL.
pub fn resolve(id: &str) -> Result<ResolvedExtension, String> {
    let extensions_toml = fetch(EXTENSIONS_TOML_URL)?;
    let entries: HashMap<String, RegistryEntry> = toml::from_str(&extensions_toml)
        .map_err(|error| format!("could not parse the Zed extensions registry: {error}"))?;
    let entry = entries
        .get(id)
        .ok_or_else(|| format!("\"{id}\" is not in the Zed extensions registry"))?;
    let gitmodules = fetch(GITMODULES_URL)?;
    let git_url = git_url_for_submodule(&gitmodules, &entry.submodule).ok_or_else(|| {
        format!("the registry lists \"{id}\" but its git URL could not be resolved")
    })?;
    Ok(ResolvedExtension {
        id: id.to_string(),
        version: entry.version.clone(),
        git_url,
    })
}

/// Lists every id (and its pinned version) currently in the registry, for
/// populating an "Install" search list. Git URLs aren't resolved here (that
/// needs a second round trip against `.gitmodules`, via `resolve()`) --
/// listing hundreds of ids is one fetch; resolving all of them up front
/// would be hundreds.
pub fn list_ids() -> Result<Vec<(String, String)>, String> {
    let extensions_toml = fetch(EXTENSIONS_TOML_URL)?;
    let entries: HashMap<String, RegistryEntry> = toml::from_str(&extensions_toml)
        .map_err(|error| format!("could not parse the Zed extensions registry: {error}"))?;
    let mut ids: Vec<(String, String)> = entries
        .into_iter()
        .map(|(id, entry)| (id, entry.version))
        .collect();
    ids.sort();
    Ok(ids)
}

fn fetch(url: &str) -> Result<String, String> {
    let mut response = ureq::get(url)
        .call()
        .map_err(|error| format!("could not reach {url}: {error}"))?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("could not read the response from {url}: {error}"))
}

// .gitmodules is git's own config format, not TOML/JSON:
//   [submodule "extensions/material-icon-theme"]
//       path = extensions/material-icon-theme
//       url = https://github.com/zed-extensions/material-icon-theme.git
fn git_url_for_submodule(gitmodules: &str, submodule_path: &str) -> Option<String> {
    let header = format!("[submodule \"{submodule_path}\"]");
    let start = gitmodules.find(&header)?;
    let after = &gitmodules[start + header.len()..];
    let end = after.find("[submodule").unwrap_or(after.len());
    after[..end].lines().find_map(|line| {
        line.trim()
            .strip_prefix("url = ")
            .map(|url| url.trim().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_gitmodules_snippet() {
        let text = r#"
[submodule "extensions/gruvbox-material-icons"]
	path = extensions/gruvbox-material-icons
	url = https://github.com/RiverMatsumoto/zed-gruvbox-material-icons

[submodule "extensions/material-icon-theme"]
	path = extensions/material-icon-theme
	url = https://github.com/zed-extensions/material-icon-theme.git

[submodule "extensions/material-theme"]
	path = extensions/material-theme
	url = https://github.com/zed-extensions/material-theme.git
"#;
        assert_eq!(
            git_url_for_submodule(text, "extensions/material-icon-theme").as_deref(),
            Some("https://github.com/zed-extensions/material-icon-theme.git")
        );
        assert_eq!(
            git_url_for_submodule(text, "extensions/material-theme").as_deref(),
            Some("https://github.com/zed-extensions/material-theme.git")
        );
        assert!(git_url_for_submodule(text, "extensions/does-not-exist").is_none());
    }

    // Live network test, same spirit as the LSP live tests: skipped by
    // default, run explicitly to prove this resolves against the real
    // registry, not a fixture.
    #[test]
    #[ignore = "hits the real network; run explicitly with --ignored"]
    fn resolves_the_real_material_icon_theme_entry() {
        let resolved = resolve("material-icon-theme").expect("should resolve from the live registry");
        assert_eq!(resolved.git_url, "https://github.com/zed-extensions/material-icon-theme.git");
    }

    #[test]
    #[ignore = "hits the real network; run explicitly with --ignored"]
    fn lists_hundreds_of_real_ids_including_material_icon_theme() {
        let ids = list_ids().expect("should list from the live registry");
        assert!(ids.len() > 100, "expected hundreds of real registry entries, got {}", ids.len());
        let material = ids
            .iter()
            .find(|(id, _)| id == "material-icon-theme")
            .expect("material-icon-theme should be in the live registry");
        assert_eq!(material.1, "1.3.1");
    }
}
