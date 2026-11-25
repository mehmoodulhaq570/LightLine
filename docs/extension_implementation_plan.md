# LightLine Extension Implementation Plan

## 1. Goal

Give LightLine a real extension ecosystem — icon/color themes, formatters, linters,
language intelligence, custom commands — **without** turning it into an
Electron/VS Code clone, **without** requiring `.vsix` compatibility, and **without**
LightLine having to build and maintain its own extension marketplace/catalog from
scratch.

## 2. Decision: use the Zed extension ecosystem as the catalog, via an adapter layer

**This supersedes the earlier plan of LightLine defining and hosting its own
marketplace/manifest format from scratch.** After investigating both the VS Code
Marketplace (rejected — see §6) and building our own bespoke registry (rejected as
unnecessary), we settled on: **LightLine consumes extensions from the
[zed-industries/extensions](https://github.com/zed-industries/extensions) registry,
through an adapter layer that translates Zed's formats into LightLine's own internal
types.** LightLine never becomes Zed, and never promises to run every Zed extension —
only the types of extension an adapter has been written for.

### Why this, concretely (verified, not assumed)

- `zed-industries/extensions` is a real, actively maintained index repo. Its
  `extensions.toml` lists ~hundreds of entries, each a git submodule pointing at a
  separate extension repo.
- Two very different kinds of content live behind those entries, confirmed directly
  from Zed's own docs and the `zed_extension_api` crate source:
  - **Data-only extensions** (icon themes, color themes): plain `extension.toml` +
    JSON (icon themes validate against `icon_themes/v0.3.0.json`) + SVG assets. **No
    compiled code, no runtime needed to execute anything.** LightLine can load these
    with nothing more than a JSON parser.
  - **Procedural extensions** (languages, some formatters/debuggers/MCP servers): real
    Rust code compiled to `wasm32-wasip2`, executed inside Zed's own embedded
    `wasmtime` runtime, calling a host API defined by the `zed_extension_api` crate.
    This crate is **Apache-2.0 licensed** — confirmed via its `Cargo.toml` — so unlike
    Microsoft's Marketplace (whose ToS restricts usage to Microsoft's own products),
    there's no licensing trap in reading/implementing against this API surface.
- `wasmtime` is a pure-Rust crate. Running WASM-based Zed extensions, when we get to
  that, is a native-Rust-shaped problem, not a "embed a JS engine" problem — a much
  better structural fit for LightLine than VS Code's Node/JS extension model would
  have been.

### The new architecture

```
                    LightLine
                       │
                       ▼
              Zed Extension Registry
                       │
                       ▼
              Download extension
                       │
                       ▼
             LightLine Adapter
                       │
             ┌─────────┴─────────┐
             │                   │
       Data-based extension   Executable
       (icons/themes/etc.)    functionality
```

The key architectural rule (unchanged from the original plan, just re-aimed): **don't
make `Zed extension == LightLine extension`.** Every Zed extension goes through an
adapter into LightLine's own stable internal representation:

```
Zed Icon Theme
       ↓
IconThemeAdapter
       ↓
LightLine IconTheme

Zed Language Extension           (later phase)
       ↓
LanguageAdapter
       ↓
LightLine LSP

Zed extension (process:exec)     (later phase)
       ↓
Process/CLI Adapter
       ↓
LightLine CLI Runner
```

This keeps the promise scoped and honest at every stage: **"LightLine can consume
selected Zed extension types"** — starting with icon themes, then themes/snippets,
then language/LSP-related extensions, and only much later (if ever) procedural/WASM
extensions. Never a blanket "LightLine supports Zed extensions."

## 3. Current State (v0.8.0, updated 2026-11)

- **LSP client** (`src/lsp.rs`): LightLine's own JSON-RPC client over stdio, one
  client per (language, workspace root), started when a file of that language opens.
  Wired to **rust-analyzer**, **Pyright**, **clangd**, **typescript-language-server**
  and **gopls**, with hover, definition, references, completion, signature help,
  rename, code actions, formatting, inlay hints and symbols. A future
  `LanguageAdapter` would use a Zed language extension only to *discover/configure*
  which server to run; `src/lsp.rs` still speaks the protocol.
- **Formatters** (`src/formatter.rs`): external tools run as processes in the
  kill-on-close job: Prettier (local, global or `npx`), rustfmt, and Ruff with a Black
  fallback. Still special-cased per tool rather than a generic CLI adapter.
- **Extensions panel**: backed by the Zed registry (Phase 3 below). Featured shows
  Prettier, Material Icon Theme and Dracula; search covers the whole registry.
- **What installs**: color themes, icon themes, and (§7) language extensions whose
  Tree-sitter grammars build with the user's C compiler. Entries that live inside
  another repository (`html`, `glsl`, `proto`) are refused before downloading.
- **No WASM runtime, no JS runtime.** LightLine still cannot run extension code, so
  language servers declared by extensions and other code extensions are refused.

## 4. Phased Plan

### Phase 1 — Zed registry integration

New modules:
```
src/extensions/
├── mod.rs
├── zed_registry.rs   — fetch/parse extensions.toml, find an extension by id
├── zed_manifest.rs   — parse a Zed extension's own extension.toml
└── installer.rs      — download + install a Zed extension's files locally
```

Flow LightLine's Rust code needs to support:
```
Find Zed extension
        ↓
Get metadata
        ↓
Download it
        ↓
Install locally
```

Not a full clone of `zed-industries/extensions` — just resolving the registry
metadata for the specific extension(s) LightLine is installing, then fetching that
one extension's own repo/files.

### Phase 2 — Material Icons adapter (first proof, in progress)

Take one real Zed extension and make it work in LightLine, end to end:
```
Zed Material Icons
        ↓
extension.toml
        ↓
LightLine adapter
        ↓
IconTheme
        ↓
Explorer
```

Internal representation stays LightLine's own and doesn't leak Zed's shape into the
rest of the app:
```rust
struct IconTheme {
    file_associations: ...,
    folder_associations: ...,
    icons: ...,
}
```

This is the proof point: once this works, LightLine has demonstrated it can consume
an extension from an existing ecosystem, not just a self-authored one.

**Known open gap:** LightLine's current icon rendering (`src/windows_app/icons.rs`)
loads `.ico` via GDI (`HICON`). Real Zed icon themes ship SVG. To load icon themes
beyond our own vendored/pre-converted one, LightLine needs either a small SVG
rasterizer or a conversion step — to be decided before Phase 2 is "done," not after.

### Phase 3 — No new marketplace; the Extensions panel browses Zed's registry

The existing Extensions panel UI (currently backed by a hardcoded 2-item list) points
at the Zed registry instead of a LightLine-maintained database:

```
Extensions

🔍 Search Zed Extensions

Material Icon Theme       Installed
Python                    Install
Rust                      Install
Dracula Theme             Install
...
```

Install flow:
```
LightLine
   ↓
Zed registry
   ↓
download
   ↓
adapt/validate
   ↓
LightLine extension directory
```

LightLine never maintains its own catalog database — Zed's registry *is* the catalog.

### Phase 3b — LightLine's own list of tested extensions (2026-11)

Zed's registry says nothing about which of its ~1,600 extensions work in
LightLine, so the panel used to describe every result as "install to see
whether LightLine supports it yet". `registry/extensions.toml` lists the ones
tested with LightLine, still without a server or a database of our own:

- Each entry is either `from = "zed"` (installed at the commit Zed's registry
  pins, so it follows Zed's updates) or `repository` + `commit` (an extension
  in its own GitHub repository, in Zed's format, pinned in the list), plus
  Prettier, which is built in.
- A copy is compiled in (`include_str!`) and read the first time the panel
  opens. When the panel opens, the latest copy is read from `main` on
  GitHub, next to Zed's `extensions.toml`, reused for 10 minutes. A list
  that can't be read or is in a newer `format` leaves the built-in one.
- Entries this build can't use (an unknown `kind`, a newer `requires`) are
  skipped, so older releases keep working with newer lists.
- `cargo test --lib lightline_list` checks the file (CI); the ignored test
  installs every entry and checks its kind. Contributors propose entries by
  pull request (`CONTRIBUTING.md`).
- The panel shows listed entries with a **LightLine** badge, recommends the
  `recommended` ones, and puts listed search results first. Code:
  `src/extensions/lightline_list.rs`, `windows_app/extensions_panel.rs`
  (`merge_extension_list`). Mock-up: `design/extension-list-design.html`.

### Phase 3c — Our own extensions, in one repository (planned 2026-11-24, built 2026-11-25)

LightLine's own extensions live in **one repository**, `lightline-extensions`
(under `mehmoodulhaq570` for now; moving it to an organization later is fine,
since GitHub redirects the old address). Decided over one repository per
extension: one place to maintain, one CI, shared tooling and templates, and
one PR can update several extensions. It stays out of the LightLine
repository, so extension changes don't ride along with editor releases.

**The repository:**

- Folders by kind, each extension in Zed's format (`extension.toml`, then
  `themes/`, `icon_themes/`, `snippets/`, `languages/` as Zed expects):

  ```
  lightline-extensions/
    themes/<id>/            color themes
    icon-themes/<id>/       icon themes
    snippets/<id>/          snippet packs
    languages/<id>/         language extensions (grammar repo + queries)
    template/               a starting point for a new extension
    README.md, CONTRIBUTING.md, LICENSE
    .github/workflows/      checks every manifest on each PR
  ```

- Each extension keeps its own `version` in its `extension.toml`; the folder
  name is its id (as everywhere in LightLine).
- CI: every `extension.toml` parses and its parts exist; ids are unique and
  match folders; themes and snippet files are valid JSON. A small checker
  script in the repository, or `lightline_list` checks reused.
- Zed's registry accepts extensions in a subfolder of a repository
  (`path = "..."`), so ours can be submitted there too.

**LightLine changes needed (one PR):**

1. `registry/extensions.toml`: an optional `path` on `repository` + `commit`
   entries, e.g. `path = "themes/lightline-dark"`. `lightline_list::parse`
   checks it's relative, uses `/`, and has no `..` or drive (strict in
   `problems`, the CI test).
2. `installer.rs` (`fetch_folder`): with a `path`, download only that folder
   into `.download-<id>`: `git init`, the partial-clone settings and
   `core.sparseCheckout` written with `git config`, `/<path>/` written to
   `.git/info/sparse-checkout` (not `git sparse-checkout`, whose options
   changed between git versions), `git fetch --depth 1 --filter=blob:none
   origin <commit>`, `git checkout FETCH_HEAD`. The folder then becomes
   `.staging-<id>` and is validated and promoted exactly as a whole
   repository (backup, rollback, lock). Without a `path`, nothing changes.
3. The same change lets Zed registry entries with a `path` install
   (`zed_registry::check_installable` refuses them today; Zed's own `html`,
   `glsl`, `proto` are inside its huge main repository, which is why the
   download must be partial). Their kinds still have to be supported.
4. Tests: a local fixture repository with two extensions in subfolders
   (install one, only its files arrive); `path` validation; an ignored live
   test installing from the real repository.
5. Docs: `USER_GUIDE.md` (Extensions), `CONTRIBUTING.md` (how to add an
   extension to the new repository), this section, `CHANGELOG.md`.

**Decided 2026-11-25** (design `design/own-extensions-design.html`, all
recommendations approved):

- First extensions: `lightline-themes` (LightLine Daylight + LightLine
  Contrast; Midnight stays built in) and `lightline-c-snippets` (C and C++,
  which Zed's registry lacks). The theme pack is first in the list.
- Ids start with `lightline-`; MIT license; `scripts/check.py` (Python
  standard library) in CI; submitting to Zed's registry waits until they've
  shipped in a LightLine release.
- Neither needs WASM.
- Next: an **Update** button when the list's version is newer than the
  installed one (helps every pinned entry), as its own PR.

Verified: the ignored `every_listed_extension_installs_and_is_its_kind`
installs both from GitHub (folder only), and
`a_folder_of_zeds_own_repository_downloads_alone` fetches Zed's `html`
folder alone (about 35 s, mostly Zed's trees).

### Phase 3d — Updating installed extensions (built 2026-11-25)

Design `design/extension-updates-design.html`, all recommendations approved:
look for updates only when the user opens Extensions or chooses Check for
Updates (which refetches both lists, skipping the 10-minute reuse); show them
on installed rows (`installed → new` and **Update**) and in the INSTALLED
header, with **Update all** there (two or more) and **Update All (n)** in the
··· menu; only a newer version number is an update (`lightline_list::newer`),
never a new commit alone; a language whose grammar is loaded is tried, and
if Windows holds its folder the old copy stays with *Restart LightLine, then
update X*.

- `Extension::installed_version` (from the scan) is kept apart from
  `version` (what an install gets: the list's pin, else the Zed registry's).
  This also fixed rows that showed the registry's version as installed.
- An update is an install over the installed copy (`start_extension_install`):
  staged download, checks, backup, swap, rollback. `extension_updated` then
  re-applies the theme in use by its variant name, reloads icons and
  languages, forgets snippets and restarts checkers. Update All starts every
  update; the installer's lock takes them one at a time, with one status line.

Verified: `an_update_replaces_the_installed_copy_or_leaves_it_alone` (a local
fixture repository at 1.0.0, 1.1.0 and a broken commit); live, with LightLine
Themes, the C snippets and Dracula installed at lowered versions: the rows,
header, menu and status lines match the mock-up, the Contrast theme stays in
use through its update, and the repaint check finds 0 stale pixels.

### Phase 4 — Generalize the adapter pattern

Each new supported Zed extension *type* gets its own adapter, not a special case
bolted onto the loader:

- `IconThemeAdapter` → `LightLine::IconTheme` (Phase 2)
- `ColorThemeAdapter` → LightLine's theme/color system (data-only, same shape of work
  as icon themes — natural next target once Phase 2 is solid)
- `LanguageAdapter` → configuration for LightLine's existing `src/lsp.rs` client
  (which LSP binary to run, how to find/install it) — **not** a reimplementation of
  Zed's WASM execution, just reading whatever static config a language extension
  exposes that doesn't require running its WASM
- `Process/CLI Adapter` → LightLine's CLI runner (generalizing the current
  hardcoded Prettier integration, per §5 below) for extensions whose Zed
  `process:exec` capability usage maps onto "run this tool, read its output"

Explicitly **not** in Phase 4: executing a Zed extension's compiled WASM directly.
That's a distinct, later, larger effort (embedding `wasmtime` + implementing enough of
the `zed_extension_api` host surface to run real procedural extensions) — see §6.

## 5. CLI tool execution model (still needed, regardless of extension source)

Whether a "run this tool" capability comes from a Zed extension's `process:exec`
declaration or (still, for now) a hardcoded LightLine integration like Prettier, the
underlying execution primitive is the same and still needs building generically:

```
LightLine
   │
   │ Rust launches process
   ↓
prettier.exe / ruff / clang-format / ...
   │
   │ result
   ↓
LightLine
```

```rust
Command::new("prettier")
    .arg("--stdin-filepath")
    .arg("app.js")
    .stdin(...)
    .stdout(...);
```

Requirements for a generic CLI runner (unchanged from earlier planning, still valid
independent of where the extension description comes from):

- **Timeout enforcement** — a broken tool must not freeze the editor; terminate and
  report an error, keep LightLine running.
- **stdout/stderr captured separately** — don't treat every stderr line as fatal;
  many legitimate tools log warnings there.
- **Structured diagnostics where available** — prefer JSON output so LightLine's
  internal `Diagnostic` type doesn't need a bespoke parser per tool (reuses the same
  `lsp::Diagnostic` shape already populated by LSP servers and by the C/C++
  `-fsyntax-only` check added this session).
- **Process isolation is inherent** — a crashing subprocess cannot crash LightLine's
  own process; no extra work needed beyond not panicking on a bad exit code.
- **Cross-platform command resolution** — reuse `workflow::command_available()`
  (already added) rather than hardcoding `.exe` assumptions.
- **Working directory / environment** — a linter often needs the project root and its
  own config file discovery to behave correctly; the runner must accept
  `working_dir`/`env`, not just a bare command.

## 6. Non-Goals / explicitly deferred

- **Executing Zed extensions' compiled WASM.** Requires embedding `wasmtime` and
  implementing a real slice of the `zed_extension_api` host interface (Worktree
  access, settings, download helpers, etc.). Smaller and more architecturally aligned
  than a VS Code-compatible JS runtime would have been, but still a genuine
  multi-phase effort of its own — not started, not scheduled until the data-only
  adapters (icon themes, color themes) are solid.
- **VS Code `.vsix` compatibility.** Rejected as a near-term goal: it would mean
  embedding a JS engine *and* reimplementing a large slice of VS Code's own `vscode`
  API and UI contribution model — a multi-year undertaking at the scale of Eclipse
  Theia or VSCodium. Also has a real licensing snag Zed's approach doesn't: the actual
  VS Code Marketplace's terms of use are scoped to Microsoft's own products, which is
  why non-Microsoft editors that do this (VSCodium, Theia) default to the Open VSX
  Registry instead.
- **Promising "LightLine supports Zed extensions" as a blanket claim.** Only ever
  true for the specific extension *types* that have an adapter. An extension
  requiring capabilities LightLine hasn't built an adapter for (yet) should fail
  clearly at install time, not silently partially work.
- **A LightLine-hosted extension registry/marketplace database.** Deliberately not
  building this — Zed's registry is the catalog; LightLine only needs to *read* it,
  never maintain a competing one.
- **Sandboxing beyond the process-isolation CLI model gives for free.** Becomes a real
  question again once WASM execution (§6, first bullet) is in scope — a WASM module
  can be given host-function access that reaches further than an OS process boundary
  would, and that access needs a deliberate answer before it ships, not an
  afterthought.

## 7. Language extensions: Tree-sitter grammars (decided 2026-11)

A Zed language extension is mostly data around one piece of code, the
grammar:

```text
extension.toml                    [grammars.<name>] repository = "...", rev = "<sha>" (or commit), path = "<subdir>"?
                                  [language_servers.<id>] languages = [...]   (started from PATH, see Step 2)
languages/<lang>/config.toml      name, grammar, path_suffixes, line_comments, brackets, ...
languages/<lang>/highlights.scm   Tree-sitter query with Zed's capture names
languages/<lang>/{injections,indents,outline,brackets}.scm
```

The grammar is a separate repository pinned by commit. Its `src/parser.c`
(generated, committed by convention) plus an optional `src/scanner.c` or
`scanner.cc` is the code. Zed compiles it to WebAssembly when the extension
is published; the extension repository itself carries no built grammar.

### Options considered

| Option | What it needs | Verdict |
| --- | --- | --- |
| **Build the grammar's C sources at install time into a native library** and load it (`libloading`) | A C compiler on the user's PC; `libloading` (tiny) and `find-msvc-tools` (tiny, already in the lockfile through `cc`) | **Chosen.** Helix and Neovim do exactly this. No runtime cost beyond the grammar itself; parsing runs on the existing parser thread with the existing incremental-edit code. |
| Tree-sitter's `wasm` feature (wasmtime) running grammars as WASM | `tree-sitter` with `wasm`, so wasmtime, plus prebuilt `.wasm` grammars, which only Zed's own download service provides (or a WASI SDK to build them) | Rejected for now. wasmtime is a very large dependency (not measured here: building it needed network and disk this machine didn't have that day), every parse pays the WASM boundary, and grammars would come from Zed's service rather than the registry. Revisit when WASM is needed anyway for extension code (§6). |
| Ship nothing; use only `config.toml` and queries against grammars LightLine bundles | Nothing | Rejected as the main path: language extensions exist precisely for languages LightLine doesn't bundle. |

The trade-off, taken knowingly: a grammar is native code loaded into
LightLine's process, so a crash in it takes LightLine down (Zed's WASM sandbox
avoids that). Grammars are generated tables plus small hand-written scanners
from the extension's pinned commit, the same code Helix and Neovim users run
natively, and installing is an explicit user action. The user guide says to
install language extensions you trust.

### How it works

- **Install** (`installer.rs`, `language.rs`): after the extension's pinned
  commit is fetched into the staging folder, each `[grammars.<name>]` is
  fetched at its `rev` (the same `git fetch --depth 1` of exactly that
  commit) into `.grammar-src-<name>`, compiled to `grammars/<name>.dll`, and
  the sources are deleted. The staging copy is then validated and promoted
  exactly as themes are: a failure anywhere leaves the previous install
  intact.
- **Compiler**: MSVC `cl.exe`, located through the Visual Studio installation
  (`find-msvc-tools`, the lookup `cc` uses, so no Developer Prompt is
  needed); else `gcc`/`g++`; else `clang`/`clang++` on PATH. `cl /O2 /LD`,
  plus `/EXPORT:tree_sitter_<name>` because older generated parsers don't
  mark their entry point exported. gcc gets `-static-libgcc` (and
  `-static-libstdc++` for a C++ scanner) so the DLL doesn't depend on MinGW's
  runtime DLLs. The build runs in the kill-on-close job like every other
  child process. With no compiler the install stops with: "needs a C compiler
  to build its grammar: install the Visual Studio C++ Build Tools, or put gcc
  or clang on PATH".
- **Validation**, before promotion: every language names a grammar that was
  built; the library loads and exports `tree_sitter_<name>`; Tree-sitter
  accepts its ABI (`Parser::set_language`); the language's `highlights.scm`
  compiles against it (`Query::new`). The probe unloads the library again so
  the folder can still be moved into place. Grammar names must match
  `[A-Za-z0-9_-]+` and grammar `path`s must be relative, so nothing in a
  manifest can name a file outside the extension folder.
- **Use**: nothing happens at startup. The first time a file opens that no
  built-in language claims, `extensions::language::for_path` lists the
  installed languages (one `read_dir` plus a few small TOML files) and
  matches `path_suffixes` against the file's extension or whole name. The
  tab gets `Syntax::Extension`, an ordinary `TreeSitterSyntax`. Its parser
  thread loads the grammar (once per process; it is never unloaded, since
  trees and queries point into it) and the query, and from there it is the
  same incremental parse and recolor as the built-in languages, under the
  same `parseLimitKb` limit.
- **Colors**: Zed's capture names map by their first segment:
  `comment` → Comment; `string`, `character`, `escape`, `regex` → String;
  `keyword`, `boolean` (and the Neovim-style `conditional`, `repeat`,
  `include`, `exception`) → Keyword; `type`, `constructor`, `enum`, `tag`,
  `namespace`, `module` → Type; `number`, `float`, `constant` → Number;
  `preproc`, `macro`, `function.macro` → Macro; `function`, `method` →
  Function; `operator` → Operator; `attribute`, `decorator`, `label` →
  Attribute. Variables, properties and punctuation keep the text color, as in
  the built-in languages. The active color theme decides what those nine
  colors look like.
- **Installing or removing while running**: the language list is re-read
  after an install or removal, and open tabs pick their language again. A
  loaded grammar stays mapped until LightLine exits, so removing or updating
  an extension whose grammar is in use can't delete the DLL until a restart;
  the removal reports that files remain.

### Step 2: editing rules, the picker and language servers (2026-11)

- **Editing rules** (`extensions/editing.rs`): the language's `config.toml`
  is parsed once, when the installed languages are listed, into
  `EditingRules`: `line_comments` (the first is what `Ctrl+/` types),
  `block_comment` (both the old `["/* ", " */"]` form and the newer table),
  `brackets` (one-character pairs with `close = true` are auto-closed, honouring
  `autoclose_before` and `not_in = ["string", "comment"]` through the syntax
  spans of the caret's line; which of `()`, `[]`, `{}` it lists are matched
  and colored; those with `newline = true` indent the next line),
  `increase_indent_pattern` / `decrease_indent_pattern` (regexes: Enter
  indents after a match, typing a matching `end`/`else` outdents the line
  once, and typing past it, as in `endpoint`, puts the indent back), and
  `tab_size` / `hard_tabs` (what Tab, Enter and snippets insert, the status
  bar's "Spaces: N" and the indent guides). The rules hang off the
  `ExtensionLanguage` the tab's `Syntax` already holds, so typing reads them
  through a pointer, with no lookup. A language without `brackets` keeps
  LightLine's own pairs and Enter rule. Built-in languages are unchanged;
  `Ctrl+/` is new for them too, with their usual markers.
- **The language picker** (`run_language.rs`) lists the installed languages
  by name after the Run languages. Listing reads only the config files the
  installed-language list already reads; a grammar is loaded only when a tab
  is colored with it. Choosing one sets the tab's `extension_choice`, which
  wins over what the file's name says, and is remembered in
  `run-languages.json` as `{"path", "extension": "<name>"}` (a key older
  versions skip). It is re-applied when the file is read again.
- **Language servers** (`extensions/language_servers.rs`, `lsp.rs`):
  `[language_servers.<name>]` entries are read with their `languages` (or
  the older single `language`), `opt_in_languages` (tried last) and
  `language_ids`, in the file's order. An extension language with a server
  gets an `lsp::Language::Extension(id)`, interned by name, so the existing
  per-(language, root) clients, events and `lsp_failed_at` work unchanged.
  The server thread picks the command: the `languageServers` setting for the
  language, then for each declared server its setting or its entry in a
  table of about 40 well-known servers (`lua-language-server`, `zls`,
  `taplo lsp stdio`, `elixir-ls`, `nextls --stdio`, ...), the first found on
  PATH. Nothing is downloaded. When none is found, `Event::Stopped` puts
  "Lua: `lua-language-server` not found on PATH, so its language server
  isn't running..." in the status bar, command first so a narrow status bar
  still shows it. The root is the nearest folder with `.git`,
  else the workspace. Servers run in the kill-on-close job, like the others.
- **Checked live** against the real `lua` extension from the registry
  (`tests/language_extension_live.rs`, built with MSVC), and in the app: see
  the CHANGELOG entry.

### Not supported yet, in rough order of value

1. `indents.scm` (Tree-sitter indentation): the regex patterns and
   `newline` brackets cover the common cases; `indents.scm` would need the
   tree on the UI thread.
2. `injections.scm` (languages embedded in others) and `outline.scm` (which
   could feed `@` file symbols without a language server).
3. Servers that only the extension's WASM can find or download (most Zed
   extensions download theirs), and server settings or initialization
   options the extension computes. Those need the WASM runtime of §6.
4. File icons for the new languages.
5. Registry entries with a `path` (inside another repository) are still
   refused before download (`zed_registry::check_installable`). Planned as
   part of Phase 3c (partial download of one folder).
6. gcc/clang builds and Windows ARM64 still need a live check.
