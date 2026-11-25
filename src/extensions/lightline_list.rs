//! LightLine's own extension list (`registry/extensions.toml` in the
//! repository): extensions tested with LightLine, shown first in the
//! Extensions panel. It's a file, not a marketplace: a copy is built in, and
//! the panel also reads the latest one from GitHub when it opens, so a new
//! entry reaches users without a release. Entries are Zed registry
//! extensions (installed at Zed's pinned commit), extensions in their own
//! repository (installed at the commit the list pins), or Prettier.

use serde::Deserialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The copy built into this LightLine, used until (and unless) a fetched one
/// arrives.
pub const BUILT_IN: &str = include_str!("../../registry/extensions.toml");

const LIST_URL: &str =
    "https://raw.githubusercontent.com/mehmoodulhaq570/LightLine/main/registry/extensions.toml";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
// Matches the Zed registry list: opening the panel twice in a row downloads
// it once.
const LIST_REUSE: Duration = Duration::from_secs(10 * 60);
/// The list's `format` this LightLine reads. A list in a newer format is
/// ignored and the built-in copy kept.
const FORMAT: u32 = 1;
/// A description longer than this is cut off in the panel's row.
pub const DESCRIPTION_LIMIT: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    ColorTheme,
    IconTheme,
    Language,
    Snippets,
    Checker,
    Formatter,
}

impl Kind {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "color-theme" => Self::ColorTheme,
            "icon-theme" => Self::IconTheme,
            "language" => Self::Language,
            "snippets" => Self::Snippets,
            "checker" => Self::Checker,
            "formatter" => Self::Formatter,
            _ => return None,
        })
    }
}

/// Where an entry is installed from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The Zed registry's entry with the same id, at the commit it pins.
    Zed,
    /// Its own repository, at exactly this commit; with a `path`, that
    /// folder of it (lightline-extensions holds several extensions).
    Repository {
        url: String,
        commit: String,
        version: String,
        path: Option<String>,
    },
    /// Part of LightLine (Prettier): nothing to download.
    BuiltIn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub publisher: String,
    pub kind: Kind,
    pub source: Source,
    /// Shown under RECOMMENDED; otherwise found by search and the chips.
    pub recommended: bool,
}

#[derive(Deserialize)]
struct RawList {
    format: u32,
    #[serde(default)]
    extension: Vec<RawEntry>,
}

// Unknown fields are ignored, so a newer list still reads in an older
// LightLine.
#[derive(Deserialize)]
struct RawEntry {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    publisher: String,
    kind: String,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    repository: Option<String>,
    #[serde(default)]
    commit: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    builtin: bool,
    #[serde(default)]
    recommended: bool,
    #[serde(default)]
    requires: Option<String>,
}

/// The entries of `text` this LightLine can use, in the file's order. An
/// entry this build can't use (an unknown kind, a newer `requires`, a
/// malformed source) is left out; the rest still load.
pub fn parse(text: &str) -> Result<Vec<Entry>, String> {
    parse_for(text, this_version())
}

/// Every problem in `text`, for the test that checks the file in the
/// repository: a pull request with a bad entry fails CI.
pub fn problems(text: &str) -> Vec<String> {
    let list: RawList = match toml::from_str(text) {
        Ok(list) => list,
        Err(error) => return vec![format!("the list doesn't parse: {error}")],
    };
    let mut problems = Vec::new();
    if list.format != FORMAT {
        problems.push(format!("format is {}, expected {FORMAT}", list.format));
    }
    let mut seen = std::collections::HashSet::new();
    for raw in &list.extension {
        if !seen.insert(raw.id.as_str()) {
            problems.push(format!("{}: listed twice", raw.id));
        }
        if let Err(problem) = entry(raw, None) {
            problems.push(format!("{}: {problem}", raw.id));
        }
        if raw.description.trim().is_empty() || raw.publisher.trim().is_empty() {
            problems.push(format!("{}: needs a description and a publisher", raw.id));
        }
        if raw.description.chars().count() > DESCRIPTION_LIMIT {
            problems.push(format!(
                "{}: description is longer than {DESCRIPTION_LIMIT} characters",
                raw.id
            ));
        }
    }
    problems
}

/// Downloads the latest list from the repository. Called on a worker when
/// the Extensions panel opens, alongside the Zed registry list. A list
/// downloaded in the last 10 minutes is reused; `fresh` downloads it again
/// (Check for Updates).
pub fn fetch(fresh: bool) -> Result<Vec<Entry>, String> {
    static LIST: Mutex<Option<(Instant, String)>> = Mutex::new(None);
    let cached = LIST
        .lock()
        .ok()
        .and_then(|list| list.clone())
        .filter(|(fetched, _)| !fresh && fetched.elapsed() < LIST_REUSE)
        .map(|(_, text)| text);
    let text = match cached {
        Some(text) => text,
        None => {
            let text = download()?;
            if let Ok(mut list) = LIST.lock() {
                *list = Some((Instant::now(), text.clone()));
            }
            text
        }
    };
    parse(&text)
}

fn download() -> Result<String, String> {
    let mut response = ureq::get(LIST_URL)
        .config()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .call()
        .map_err(|error| format!("could not reach {LIST_URL}: {error}"))?;
    response
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_to_string()
        .map_err(|error| format!("could not read {LIST_URL}: {error}"))
}

// The release version, set by the release workflow; a local build has none
// and shows every entry.
fn this_version() -> Option<&'static str> {
    option_env!("LIGHTLINE_VERSION")
}

fn parse_for(text: &str, version: Option<&str>) -> Result<Vec<Entry>, String> {
    let list: RawList = toml::from_str(text)
        .map_err(|error| format!("could not read LightLine's extension list: {error}"))?;
    if list.format != FORMAT {
        return Err(format!(
            "LightLine's extension list is in format {}; this LightLine reads format {FORMAT}",
            list.format
        ));
    }
    let mut seen = std::collections::HashSet::new();
    Ok(list
        .extension
        .iter()
        .filter_map(|raw| entry(raw, version).ok())
        .filter(|entry| seen.insert(entry.id.clone()))
        .collect())
}

fn entry(raw: &RawEntry, version: Option<&str>) -> Result<Entry, String> {
    if !valid_id(&raw.id) {
        return Err("ids use lowercase letters, digits, - and _".into());
    }
    if raw.name.trim().is_empty() {
        return Err("needs a name".into());
    }
    let kind = Kind::parse(&raw.kind).ok_or_else(|| format!("unknown kind \"{}\"", raw.kind))?;
    if let (Some(requires), Some(version)) = (&raw.requires, version)
        && newer(requires, version)
    {
        return Err(format!("needs LightLine {requires}"));
    }
    if raw.path.is_some() && raw.repository.is_none() {
        return Err("path goes only with repository + commit".into());
    }
    let source = match (raw.from.as_deref(), raw.repository.as_deref(), raw.builtin) {
        (Some("zed"), None, false) => Source::Zed,
        (None, Some(url), false) => {
            let commit = raw.commit.as_deref().unwrap_or_default();
            let full_commit =
                commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit());
            if !url.starts_with("https://github.com/") || url.contains("..") {
                return Err("repository must be an https://github.com/ address".into());
            }
            if !full_commit {
                return Err("commit must be a full 40-character commit id".into());
            }
            if let Some(path) = &raw.path
                && !super::installer::valid_folder_path(path)
            {
                return Err(
                    "path must be a folder like themes/lightline-themes: relative, with /, \
                     no . or .."
                        .into(),
                );
            }
            Source::Repository {
                url: url.to_owned(),
                commit: commit.to_ascii_lowercase(),
                version: raw.version.clone().unwrap_or_default(),
                path: raw.path.clone(),
            }
        }
        (None, None, true) if raw.id == "prettier" && kind == Kind::Formatter => Source::BuiltIn,
        (None, None, true) => return Err("only Prettier is built in".into()),
        _ => {
            return Err(
                "needs exactly one of from = \"zed\", repository + commit, or builtin = true"
                    .into(),
            );
        }
    };
    Ok(Entry {
        id: raw.id.clone(),
        name: raw.name.trim().to_owned(),
        description: raw.description.trim().to_owned(),
        publisher: raw.publisher.trim().to_owned(),
        kind,
        source,
        recommended: raw.recommended,
    })
}

// The same rule the installer applies to folder names.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
}

/// Whether version `wanted` is newer than `have` ("0.13.0" > "0.12.2",
/// "1.10.0" > "1.9.2"). Anything after a `-` (a pre-release tag) is ignored.
pub fn newer(wanted: &str, have: &str) -> bool {
    let numbers = |text: &str| -> Vec<u64> {
        text.split('-')
            .next()
            .unwrap_or_default()
            .split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    let (wanted, have) = (numbers(wanted), numbers(have));
    let length = wanted.len().max(have.len());
    let at = |list: &[u64], index: usize| list.get(index).copied().unwrap_or(0);
    (0..length)
        .map(|index| at(&wanted, index).cmp(&at(&have, index)))
        .find(|order| order.is_ne())
        .is_some_and(|order| order.is_gt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_in_the_repository_is_valid() {
        assert_eq!(problems(BUILT_IN), Vec::<String>::new());
        let entries = parse_for(BUILT_IN, None).unwrap();
        assert!(entries.len() >= 10, "{} entries", entries.len());
        assert!(entries.iter().any(|entry| entry.recommended));
        // What the Extensions panel used to have written into its code.
        for id in ["prettier", "material-icon-theme", "dracula"] {
            assert!(entries.iter().any(|entry| entry.id == id), "{id}");
        }
    }

    #[test]
    fn entries_keep_the_files_order_and_their_sources() {
        let text = r#"
            format = 1
            [[extension]]
            id = "zeta"
            from = "zed"
            kind = "color-theme"
            name = "Zeta"
            [[extension]]
            id = "alpha"
            repository = "https://github.com/someone/alpha.git"
            commit = "92F46057151693648BD3ACAC1E0C138696F34433"
            version = "1.2.0"
            kind = "snippets"
            name = "Alpha"
            recommended = true
            [[extension]]
            id = "prettier"
            builtin = true
            kind = "formatter"
            name = "Prettier"
        "#;
        let entries = parse_for(text, None).unwrap();
        let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, ["zeta", "alpha", "prettier"]);
        assert_eq!(entries[0].source, Source::Zed);
        assert_eq!(
            entries[1].source,
            Source::Repository {
                url: "https://github.com/someone/alpha.git".into(),
                commit: "92f46057151693648bd3acac1e0c138696f34433".into(),
                version: "1.2.0".into(),
                path: None,
            }
        );
        assert!(entries[1].recommended && !entries[0].recommended);
        assert_eq!(entries[2].source, Source::BuiltIn);
    }

    #[test]
    fn entries_this_build_cant_use_are_left_out_and_the_rest_load() {
        let text = r#"
            format = 1
            [[extension]]
            id = "future"
            from = "zed"
            kind = "debugger"
            name = "A kind from a later LightLine"
            [[extension]]
            id = "newer"
            from = "zed"
            kind = "snippets"
            name = "Needs a later release"
            requires = "0.20.0"
            some_later_field = "ignored"
            [[extension]]
            id = "fine"
            from = "zed"
            kind = "snippets"
            name = "Fine"
            some_later_field = "ignored"
        "#;
        let ids = |version| -> Vec<String> {
            parse_for(text, version)
                .unwrap()
                .into_iter()
                .map(|entry| entry.id)
                .collect()
        };
        assert_eq!(ids(Some("0.13.0")), ["fine"]);
        assert_eq!(ids(Some("0.20.0")), ["newer", "fine"]);
        // A local build (no release version) shows them all.
        assert_eq!(ids(None), ["newer", "fine"]);
    }

    #[test]
    fn a_list_in_a_newer_format_is_refused_whole() {
        let error = parse_for("format = 2\n", None).unwrap_err();
        assert!(error.contains("format 2"), "{error}");
        assert!(problems("format = 2\n")[0].contains("format is 2"));
    }

    #[test]
    fn sources_git_could_misread_are_refused() {
        let with = |source: &str| {
            format!(
                "format = 1\n[[extension]]\nid = \"x\"\nkind = \"snippets\"\nname = \"X\"\n\
                 description = \"d\"\npublisher = \"p\"\n{source}\n"
            )
        };
        let good = "a".repeat(40);
        for bad in [
            format!("repository = \"ext::sh -c touch\"\ncommit = \"{good}\""),
            format!("repository = \"http://github.com/a/b\"\ncommit = \"{good}\""),
            format!("repository = \"https://github.com/../b\"\ncommit = \"{good}\""),
            "repository = \"https://github.com/a/b\"\ncommit = \"--upload-pack=x\"".to_owned(),
            "repository = \"https://github.com/a/b\"\ncommit = \"main\"".to_owned(),
            format!("from = \"zed\"\nrepository = \"https://github.com/a/b\"\ncommit = \"{good}\""),
            "from = \"zed\"\npath = \"themes/x\"".to_owned(),
            format!(
                "repository = \"https://github.com/a/b\"\ncommit = \"{good}\"\npath = \"../x\""
            ),
            format!(
                "repository = \"https://github.com/a/b\"\ncommit = \"{good}\"\npath = \"/themes/x\""
            ),
            format!(
                "repository = \"https://github.com/a/b\"\ncommit = \"{good}\"\npath = \"themes\\\\x\""
            ),
            "from = \"npm\"".to_owned(),
            "builtin = true".to_owned(),
            String::new(),
        ] {
            let text = with(&bad);
            assert_eq!(parse_for(&text, None).unwrap(), Vec::new(), "{bad}");
            assert_eq!(problems(&text).len(), 1, "{bad}");
        }
        let fine = with(&format!(
            "repository = \"https://github.com/a/b\"\ncommit = \"{good}\""
        ));
        assert_eq!(problems(&fine), Vec::<String>::new());
        let folder = with(&format!(
            "repository = \"https://github.com/a/b\"\ncommit = \"{good}\"\n\
             path = \"themes/lightline-themes\""
        ));
        assert_eq!(problems(&folder), Vec::<String>::new());
        assert!(matches!(
            &parse_for(&folder, None).unwrap()[0].source,
            Source::Repository { path: Some(path), .. } if path == "themes/lightline-themes"
        ));
    }

    #[test]
    fn problems_name_each_bad_entry() {
        let text = r#"
            format = 1
            [[extension]]
            id = "Bad Id"
            from = "zed"
            kind = "snippets"
            name = "X"
            description = "d"
            publisher = "p"
            [[extension]]
            id = "twice"
            from = "zed"
            kind = "snippets"
            name = "X"
            description = "d"
            publisher = "p"
            [[extension]]
            id = "twice"
            from = "zed"
            kind = "snippets"
            name = "X"
            description = "This description goes on far too long to fit in the panel's row"
            publisher = "p"
        "#;
        let problems = problems(text);
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].starts_with("Bad Id: ids use"));
        assert!(problems[1].contains("listed twice"));
        assert!(problems[2].contains("longer than"));
        // The first of a repeated id wins when loading.
        let loaded = parse_for(text, None).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].description, "d");
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.13.0", "0.12.9"));
        assert!(newer("0.10.0", "0.9.0"));
        assert!(newer("1.0", "0.99.99"));
        assert!(!newer("0.12.0", "0.12.0"));
        assert!(!newer("0.12", "0.12.0"));
        assert!(!newer("0.12.0-beta", "0.12.0"));
        assert!(!newer("0.11.5", "0.12.0"));
    }

    // Installs every entry of the list in the repository into a temporary
    // folder and checks that it is the kind the list says. Run before merging
    // a change to the list, and before a release.
    #[test]
    #[ignore = "downloads every listed extension; run with --ignored"]
    fn every_listed_extension_installs_and_is_its_kind() {
        use crate::extensions::{installer, language, snippets, tools, zed_manifest, zed_registry};
        let dir = std::env::temp_dir().join(format!("lightline-list-{}", std::process::id()));
        let mut failures = Vec::new();
        for entry in parse_for(BUILT_IN, None).unwrap() {
            let (url, commit, path) = match &entry.source {
                Source::BuiltIn => continue,
                Source::Zed => match zed_registry::resolve(&entry.id) {
                    Ok(resolved) => (resolved.git_url, resolved.commit, resolved.path),
                    Err(error) => {
                        failures.push(format!("{}: {error}", entry.id));
                        continue;
                    }
                },
                Source::Repository {
                    url, commit, path, ..
                } => (url.clone(), commit.clone(), path.clone()),
            };
            let installed = installer::install_in(&dir, &entry.id, &url, &commit, path.as_deref());
            let folder = match installed {
                Ok(folder) => folder,
                Err(error) => {
                    failures.push(format!("{}: {error}", entry.id));
                    continue;
                }
            };
            let is = match entry.kind {
                Kind::ColorTheme => {
                    let themes = crate::color_theme::ZedColorTheme::load_all(&folder);
                    let names: Vec<&str> = themes.iter().map(|theme| theme.name.as_str()).collect();
                    eprintln!("{}: {}", entry.id, names.join(", "));
                    zed_manifest::is_color_theme(&folder) && !themes.is_empty()
                }
                Kind::IconTheme => zed_manifest::is_icon_theme(&folder),
                Kind::Language => language::is_language_extension(&folder),
                Kind::Snippets => {
                    let count = snippets::validate(&folder);
                    eprintln!(
                        "{}: {count:?} snippets for {:?}",
                        entry.id,
                        snippets::pack_languages(&folder)
                    );
                    snippets::is_snippet_pack(&folder) && count.is_ok_and(|count| count > 0)
                }
                Kind::Checker => !tools::tools_of(&folder).is_empty(),
                Kind::Formatter => false,
            };
            if !is {
                failures.push(format!("{}: isn't a {:?}", entry.id, entry.kind));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
