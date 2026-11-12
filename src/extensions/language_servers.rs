//! Language servers that language extensions name, started through
//! LightLine's own LSP client (`lsp.rs`).
//!
//! A Zed extension declares its servers in `extension.toml`:
//!
//! ```toml
//! [language_servers.lua-language-server]
//! name = "Lua Language Server"
//! languages = ["Lua"]          # or the older `language = "Lua"`
//! ```
//!
//! Zed then runs the extension's WebAssembly code to find or download the
//! server. LightLine doesn't run that code, so it starts a server only when
//! its command is already on PATH: a declared name is looked up in a small
//! table of well-known servers (`KNOWN`), and the `languageServers` setting
//! can name the command for any server or language. Nothing is downloaded.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A command line that starts a language server on stdio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCommand {
    pub command: String,
    pub args: Vec<String>,
}

impl ServerCommand {
    fn new(command: &str, args: &[&str]) -> Self {
        Self {
            command: command.into(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
        }
    }

    /// `"taplo lsp stdio"`: the command and its arguments, split on spaces.
    /// A path with spaces needs the `{"command", "args"}` form instead.
    pub fn parse_line(line: &str) -> Option<Self> {
        let mut words = line.split_whitespace();
        let command = words.next()?.to_string();
        Some(Self {
            command,
            args: words.map(str::to_string).collect(),
        })
    }
}

/// Servers Zed's language extensions declare, by their declared name, and
/// the command each one's own install puts on PATH. Checked against the
/// extensions' `extension.toml` files in zed-industries/extensions.
const KNOWN: &[(&str, &str, &[&str])] = &[
    ("astro-language-server", "astro-ls", &["--stdio"]),
    ("bash-language-server", "bash-language-server", &["start"]),
    ("clojure-lsp", "clojure-lsp", &[]),
    ("csharp-ls", "csharp-ls", &[]),
    ("dart", "dart", &["language-server", "--protocol=lsp"]),
    (
        "docker-language-server",
        "docker-language-server",
        &["start", "--stdio"],
    ),
    (
        "dockerfile-language-server",
        "docker-langserver",
        &["--stdio"],
    ),
    ("elixir-ls", "elixir-ls", &[]),
    ("elm-language-server", "elm-language-server", &["--stdio"]),
    ("elp", "elp", &["server"]),
    ("erlang-ls", "erlang_ls", &[]),
    ("gleam", "gleam", &["lsp"]),
    ("hls", "haskell-language-server-wrapper", &["--lsp"]),
    ("intelephense", "intelephense", &["--stdio"]),
    ("jdtls", "jdtls", &[]),
    ("kotlin-language-server", "kotlin-language-server", &[]),
    ("lua-language-server", "lua-language-server", &[]),
    ("marksman", "marksman", &["server"]),
    ("next-ls", "nextls", &["--stdio"]),
    ("nil", "nil", &[]),
    ("nixd", "nixd", &[]),
    ("nu", "nu", &["--lsp"]),
    ("ocamllsp", "ocamllsp", &[]),
    ("ols", "ols", &[]),
    ("omnisharp", "OmniSharp", &["-lsp"]),
    ("phpactor", "phpactor", &["language-server"]),
    (
        "prisma-language-server",
        "prisma-language-server",
        &["--stdio"],
    ),
    (
        "purescript-language-server",
        "purescript-language-server",
        &["--stdio"],
    ),
    ("racket-lsp", "racket", &["-l", "racket-langserver"]),
    ("rubocop", "rubocop", &["--lsp"]),
    ("ruby-lsp", "ruby-lsp", &[]),
    ("solargraph", "solargraph", &["stdio"]),
    ("sourcekit-lsp", "sourcekit-lsp", &[]),
    ("steep", "steep", &["langserver"]),
    ("svelte-language-server", "svelteserver", &["--stdio"]),
    ("taplo", "taplo", &["lsp", "stdio"]),
    ("terraform-ls", "terraform-ls", &["serve"]),
    ("texlab", "texlab", &[]),
    ("tinymist", "tinymist", &["lsp"]),
    ("vue-language-server", "vue-language-server", &["--stdio"]),
    ("yaml-language-server", "yaml-language-server", &["--stdio"]),
    ("zls", "zls", &[]),
];

/// The command a well-known server starts with.
pub fn known_command(server: &str) -> Option<ServerCommand> {
    KNOWN
        .iter()
        .find(|(name, _, _)| *name == server)
        .map(|(_, command, args)| ServerCommand::new(command, args))
}

// The `languageServers` setting: commands by server or language name.
static OVERRIDES: Mutex<Vec<(String, ServerCommand)>> = Mutex::new(Vec::new());

/// Sets the `languageServers` setting, at load and on reload. Servers
/// already running keep their command until they restart.
pub fn set_overrides(overrides: Vec<(String, ServerCommand)>) {
    if let Ok(mut current) = OVERRIDES.lock() {
        *current = overrides;
    }
}

fn override_for(overrides: &[(String, ServerCommand)], name: &str) -> Option<ServerCommand> {
    overrides
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, command)| command.clone())
}

/// True when a file of `language` has a server to try: the extension
/// declares one, or the setting names one for the language.
pub fn has_server(language: &str, servers: &[String]) -> bool {
    !servers.is_empty()
        || OVERRIDES
            .lock()
            .is_ok_and(|overrides| override_for(&overrides, language).is_some())
}

/// A server to try, in order, and the name the status bar gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub server: String,
    pub command: ServerCommand,
}

// What to try for `language`, whose extension declares `servers` (in the
// order it prefers them): the setting for the language, then each server's
// setting or well-known command. Servers with neither come back separately.
fn candidates_with(
    overrides: &[(String, ServerCommand)],
    language: &str,
    servers: &[String],
) -> (Vec<Candidate>, Vec<String>) {
    let mut found = Vec::new();
    let mut unknown = Vec::new();
    if let Some(command) = override_for(overrides, language) {
        found.push(Candidate {
            server: command.command.clone(),
            command,
        });
    }
    for server in servers {
        match override_for(overrides, server).or_else(|| known_command(server)) {
            Some(command) => found.push(Candidate {
                server: server.clone(),
                command,
            }),
            None => unknown.push(server.clone()),
        }
    }
    (found, unknown)
}

/// How to start `language`'s server: the first candidate whose command
/// `find` locates. When none is found, the message for the status bar says
/// which commands were looked for.
pub fn resolve(
    language: &str,
    servers: &[String],
    find: impl Fn(&str) -> Option<PathBuf>,
) -> Result<(Candidate, PathBuf), String> {
    let overrides = OVERRIDES
        .lock()
        .map(|overrides| overrides.clone())
        .unwrap_or_default();
    resolve_with(&overrides, language, servers, find)
}

fn resolve_with(
    overrides: &[(String, ServerCommand)],
    language: &str,
    servers: &[String],
    find: impl Fn(&str) -> Option<PathBuf>,
) -> Result<(Candidate, PathBuf), String> {
    let (candidates, unknown) = candidates_with(overrides, language, servers);
    for candidate in &candidates {
        if let Some(program) = find(&candidate.command.command) {
            return Ok((candidate.clone(), program));
        }
    }
    let quoted = |names: Vec<&str>| {
        names
            .iter()
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    // What matters comes first: the status bar shows the start of it.
    if candidates.is_empty() {
        return Err(format!(
            "{language}: no command known for its language server ({}): set it under \
             \"languageServers\" in settings.json. Editing works without it.",
            quoted(unknown.iter().map(String::as_str).collect())
        ));
    }
    Err(format!(
        "{language}: {} not found on PATH, so its language server isn't running. Install it, \
         or set its command under \"languageServers\" in settings.json. Editing works without it.",
        quoted(
            candidates
                .iter()
                .map(|candidate| candidate.command.command.as_str())
                .collect()
        )
    ))
}

/// Finds `command` the way the language servers are started: a path as
/// given, a name on PATH (with `.exe`, `.cmd`, `.bat`).
pub fn find_command(command: &str) -> Option<PathBuf> {
    let path = Path::new(command);
    if path.is_absolute() || command.contains(['/', '\\']) {
        return path.is_file().then(|| path.to_path_buf()).or_else(|| {
            ["exe", "cmd", "bat"]
                .iter()
                .map(|extension| path.with_extension(extension))
                .find(|candidate| candidate.is_file())
        });
    }
    crate::workflow::resolve_command(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on_path<'a>(names: &'a [&'a str]) -> impl Fn(&str) -> Option<PathBuf> + 'a {
        move |command| {
            names
                .contains(&command)
                .then(|| PathBuf::from(format!(r"C:\bin\{command}.exe")))
        }
    }

    #[test]
    fn declared_servers_map_to_their_well_known_commands() {
        assert_eq!(
            known_command("taplo"),
            Some(ServerCommand::new("taplo", &["lsp", "stdio"]))
        );
        assert_eq!(
            known_command("lua-language-server").unwrap().command,
            "lua-language-server"
        );
        assert_eq!(known_command("zls").unwrap().args, Vec::<String>::new());
        assert_eq!(known_command("some-new-server"), None);
        // Each name once.
        for (index, (name, _, _)) in KNOWN.iter().enumerate() {
            assert!(
                KNOWN[index + 1..].iter().all(|(other, _, _)| other != name),
                "{name}"
            );
        }
    }

    #[test]
    fn the_first_server_found_on_path_is_used() {
        let servers = ["elixir-ls".to_string(), "next-ls".to_string()];
        let (candidate, program) =
            resolve_with(&[], "Elixir", &servers, on_path(&["nextls"])).unwrap();
        assert_eq!(candidate.server, "next-ls");
        assert_eq!(candidate.command.args, ["--stdio"]);
        assert_eq!(program, PathBuf::from(r"C:\bin\nextls.exe"));
        let (candidate, _) =
            resolve_with(&[], "Elixir", &servers, on_path(&["nextls", "elixir-ls"])).unwrap();
        assert_eq!(candidate.server, "elixir-ls");
    }

    #[test]
    fn a_missing_server_says_which_commands_were_looked_for() {
        let servers = ["lua-language-server".to_string()];
        let error = resolve_with(&[], "Lua", &servers, on_path(&[])).unwrap_err();
        assert!(
            error.starts_with("Lua: `lua-language-server` not found on PATH"),
            "{error}"
        );
        assert!(error.contains("languageServers"), "{error}");
        let unknown = ["brand-new-ls".to_string()];
        let error = resolve_with(&[], "Brand", &unknown, on_path(&[])).unwrap_err();
        assert!(error.starts_with("Brand: no command known"), "{error}");
        assert!(error.contains("`brand-new-ls`"), "{error}");
    }

    #[test]
    fn the_setting_names_a_command_by_server_or_by_language() {
        let overrides = vec![
            (
                "taplo".to_string(),
                ServerCommand::parse_line("taplo-full lsp stdio").unwrap(),
            ),
            (
                "gleam".to_string(),
                ServerCommand::new(r"D:\tools\gleam.exe", &["lsp"]),
            ),
        ];
        let (candidate, _) = resolve_with(
            &overrides,
            "TOML",
            &["taplo".to_string()],
            on_path(&["taplo-full", "taplo"]),
        )
        .unwrap();
        assert_eq!(candidate.command.command, "taplo-full");
        // A language whose extension declares no server, by its name.
        let (candidate, program) = resolve_with(&overrides, "Gleam", &[], |command: &str| {
            Some(PathBuf::from(command))
        })
        .unwrap();
        assert_eq!(candidate.command.args, ["lsp"]);
        assert_eq!(program, PathBuf::from(r"D:\tools\gleam.exe"));
        // A setting for the server is tried before the well-known command.
        let unknown = ["brand-new-ls".to_string()];
        let overrides = vec![(
            "brand-new-ls".to_string(),
            ServerCommand::new("brand", &["--stdio"]),
        )];
        let (candidate, _) =
            resolve_with(&overrides, "Brand", &unknown, on_path(&["brand"])).unwrap();
        assert_eq!(candidate.server, "brand-new-ls");
    }

    #[test]
    fn command_lines_split_on_spaces() {
        assert_eq!(
            ServerCommand::parse_line("  taplo lsp stdio "),
            Some(ServerCommand::new("taplo", &["lsp", "stdio"]))
        );
        assert_eq!(ServerCommand::parse_line("   "), None);
    }
}
