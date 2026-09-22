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

## 3. Current State (as of this document)

LightLine already has one real extension mechanism and the beginning of a second:

- **LSP client** (`src/lsp.rs`) — a genuine JSON-RPC client that talks to real language
  servers over stdio. Currently wired to **rust-analyzer** (Rust) and **pyright**
  (Python). Implemented requests: `didOpen`/`didChange`/`didSave`/`didClose`,
  `hover`, `definition`, `references`, `formatting`, `completion`. Not yet implemented:
  `rename`, `codeAction`. Not yet wired: **clangd** for C/C++. This LSP client is
  LightLine's own, independent of Zed — a future `LanguageAdapter` would use a Zed
  language extension only to *discover/configure* which server to run; LightLine's
  existing `src/lsp.rs` still does the actual protocol speaking.
- **CLI tool integration** — currently a single hardcoded case: Prettier. LightLine
  shells out to `prettier` (falling back to `npx --yes prettier`) to format supported
  file types. There is no generic "CLI extension" system yet — one special-cased Rust
  function. This is the execution primitive a future `Process/CLI Adapter` would sit
  in front of.
- **No WASM runtime, no JS runtime.** LightLine cannot execute arbitrary extension
  code today. The "Extensions" panel in the UI is a hardcoded 2-item list (Prettier,
  Material Icon Theme), not backed by any registry.

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
