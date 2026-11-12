//! Live: a real language extension from the Zed registry, installed the way
//! the Extensions panel does it. Needs the network, `git` and a C compiler,
//! so it is ignored by default:
//!
//! ```text
//! set LIGHTLINE_LIVE_APPDATA=C:\some\scratch\folder
//! cargo test --test language_extension_live -- --ignored
//! ```
//!
//! The extension is installed under `%LIGHTLINE_LIVE_APPDATA%\LightLine\extensions`
//! and left there, so a LightLine started with that APPDATA can use it.

use lightline::extensions::{installer, language, language_servers, zed_registry};
use std::path::Path;

#[test]
#[ignore = "downloads the Lua extension and builds its grammar; set LIGHTLINE_LIVE_APPDATA"]
fn the_lua_extension_brings_its_comments_brackets_indentation_and_server() {
    let Some(appdata) = std::env::var_os("LIGHTLINE_LIVE_APPDATA") else {
        panic!("set LIGHTLINE_LIVE_APPDATA to a scratch folder to install into");
    };
    // SAFETY: this test binary runs this one test, before any other thread
    // reads the environment.
    unsafe { std::env::set_var("APPDATA", &appdata) };

    // `LIGHTLINE_LIVE_LUA=<git url>@<commit>` skips the registry lookup on a
    // slow connection (the registry file is 115 KB).
    let (git_url, commit) = match std::env::var("LIGHTLINE_LIVE_LUA") {
        Ok(pinned) => {
            let (url, commit) = pinned.rsplit_once('@').expect("<git url>@<commit>");
            (url.to_string(), commit.to_string())
        }
        Err(_) => {
            let resolved = zed_registry::resolve("lua").expect("lua is in the registry");
            (resolved.git_url, resolved.commit)
        }
    };
    let dir = installer::install("lua", &git_url, &commit).expect("installs");
    assert!(dir.join("grammars").is_dir());
    language::reload();

    let lua = language::for_path(Path::new("scripts/init.lua")).expect("Lua is installed");
    assert_eq!(lua.name, "Lua");
    assert_eq!(
        lua.editing.line_comments.first().map(String::as_str),
        Some("-- ")
    );
    let brackets = lua
        .editing
        .brackets
        .as_ref()
        .expect("Lua lists its brackets");
    assert!(brackets.matched.contains(&('{', '}')));
    assert_eq!(lua.editing.indents_after("local function f()"), Some(true));
    assert!(lua.editing.outdents("    end"));
    // Listed from config files: the grammar isn't loaded to show the name.
    assert!(
        language::installed()
            .iter()
            .any(|language| language.name == "Lua")
    );

    assert_eq!(lua.servers, ["lua-language-server"]);
    let lsp = lua.lsp_language().expect("its server can be tried");
    assert_eq!(lsp.name(), "Lua");
    assert_eq!(lsp.language_id(), "lua");
    // Whatever is on this PC's PATH, a missing server names its command.
    let missing = language_servers::resolve("Lua", &lua.servers, |_| None).unwrap_err();
    assert!(missing.contains("`lua-language-server`"), "{missing}");
}
