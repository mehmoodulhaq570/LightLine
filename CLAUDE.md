# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

LightLine is a native Windows code editor in Rust (no Electron, no GUI toolkit). It draws every pixel itself through Win32 + GDI. The product's differentiator is speed, so **performance ranks above features**: startup time, idle wakeups, and typing/scroll paint cost are treated as constraints on every change (see "Decision rules" in `docs/TECHNICAL_DIRECTION.md`).

## Commands

Windows only (Win32). Needs the Rust toolchain and the Visual Studio C++ Build Tools. CI pins Rust 1.98.0 and runs on x64 and Windows ARM64.

```powershell
cargo run                                  # debug build; debug-only test hooks are compiled in
cargo build --release                      # what users get (much faster paint than debug)
cargo fmt --check                          # CI gate
cargo clippy --locked --all-targets -- -D warnings   # CI gate: any warning fails
cargo test --locked                        # CI gate

cargo test --lib text_search               # single test/module in the library crate
cargo test --bin lightline merge_conflict  # tests inside src/windows_app (the binary crate)
cargo test --test lsp_live -- --ignored    # live tests: need real tools (rust-analyzer, pyright, clangd, Go...)
```

The tests in `tests/*_live.rs` are mostly `#[ignore]` because they start real language servers and toolchains.

Disk space on the dev machine is tight. Prefer `CARGO_INCREMENTAL=0` for long sessions; `cargo clean --profile dev` is the safe way to reclaim space.

## Crate layout

- **`src/lib.rs` → crate `lightline`**: everything that doesn't touch Win32 windows. That includes:
  - the text model (`document.rs`) and Tree-sitter syntax (`syntax.rs`, run on a worker with incremental edits);
  - the LSP client (`lsp.rs`) and DAP client (`debug.rs`);
  - ConPTY terminals (`terminal/`) and the language runners (`runner.rs`, `run_config.rs`);
  - settings (`settings.rs`);
  - Git, search, session and diff helpers (`workflow.rs`);
  - matching for Find/Search (`text_search.rs`) and save history (`local_history.rs`);
  - extensions and themes (`extensions/`, `color_theme.rs`, `icon_theme.rs`);
  - child-process lifetime (`jobs.rs`).

  This crate is meant to stay portable: `docs/TECHNICAL_DIRECTION.md` has the macOS plan.
- **`src/main.rs` + `src/windows_app/` → binary**: the whole UI, Windows-only.

## How the UI is structured

- **One `App` struct** (`windows_app/app.rs`) holds all editor state. Features don't get their own types for UI state. Each feature file adds an `impl App { ... }` block and starts with `use super::*;`. `windows_app/mod.rs` re-exports the shared imports and types (`Document`, `Pos`, `Matcher`, Win32 items...).
- **Message routing**: `window.rs::handle_message` routes Win32 messages to `App` methods.
  - Keys and mouse go through `input.rs`.
  - Alt chords arrive as `WM_SYSKEYDOWN`. They reach `App::key` only if `window.rs` lists them in its `routed` condition.
  - Modifiers are read with `GetKeyState`.
- **Painting** goes into a persistent backbuffer (`render/editor.rs::paint`) and redraws only the invalid region:
  - `paint_code_area_only` is the fast path. Typing, the caret blink and wheel scrolls must stay on it.
  - The full-frame path redraws the chrome.
  - Code that changes state should invalidate the smallest rect that changed. `scroll_blit.rs` moves rows inside the backbuffer on a wheel notch instead of redrawing them.
  - Each drawn text row costs real time (GDI `ExtTextOut`), so per-row work matters.
- **No polling timers.** Background work runs on threads and wakes the window:
  - Workers send a `WorkerMessage` through a `WorkerSender`, which posts `WORKER_EVENT_MESSAGE`.
  - The file watcher (`watcher.rs`) uses `ReadDirectoryChangesW`.
  - LSP, DAP and terminal events each have their own `WM_APP+n` message.
  - Results carry ids or generations, so stale ones can be dropped.
- **Child processes** (LSP, DAP, formatters, runs, git clone) go into a kill-on-close job (`jobs.rs`). They die with LightLine. LSP clients are kept per (language, workspace root).
- **Quick Open is several modes in one box**: `>` commands, `@` file symbols, `#` workspace symbols, `:` go to line. Picker flags such as `language_pick` and `history_pick` reuse the same box. Adding a mode means touching each of these:
  - `quick_count`
  - `activate_quick_item`
  - the title, rows and hints in `render/panels.rs::paint_quick_open`
- **Command Palette entries** are `(name, numeric id)` pairs in `panels.rs::quick_commands`, dispatched by id in `activate_quick_item`. Ids are global; pick an unused one.
- **"Links after a line"** (▶ Run, Debug, Run test, merge-conflict Accept…) all go through `codelens.rs`. `lens_on_line` decides, `lens_links` lists, paint records hit spans, and `lens_click` acts.
- **The diff view** (Git review and Local History compare) is gated by `App::showing_diff()` and closed by `close_diff()`. Use these instead of checking `review_file` directly.
- **Edits must go through `App::replace_range`**, which also notifies LSP and syntax. Wrap multi-step edits in `doc_mut().begin_group()`/`end_group()` so Ctrl+Z undoes them in one step.

## User data and settings

- Per-user state lives in `%APPDATA%\LightLine\`: `settings.json`, `session.json` (tabs plus unsaved-text recovery), `history\` (Local History), `extensions\`, `pyright\`.
- **Adding a setting means editing `settings.rs` in five places**: the struct field, `Default`, parsing, serializing, and the `OWN_KEYS` list. Document it in the settings table in `docs/USER_GUIDE.md`.
- Settings the library needs globally are pushed in at load and on reload via setters, such as `syntax::set_parse_limit_kb` and `local_history::set_enabled`. Do the same for any new one.

## Debug-build test hooks

Debug builds answer these `WM_APP` messages, used by live UI checks:

- `+96` geometry (`minimap.rs`)
- `+97` `SCROLL_CHECK_MESSAGE`: pixels in the backbuffer that differ from a full redraw (`scroll_blit.rs`). Set `LIGHTLINE_SCROLL_CHECK_LOG` to get where they are.
- `+98` `HOVER_TEST_MESSAGE`
- `+99` crash test

When driving the app live:

- Use an isolated `APPDATA` folder.
- Keep the window off-screen. The maintainer is often using the PC.
- Avoid anything that opens a LightLine dialog. A folder passed as the command-line argument does. So does a `.py` file in a fresh profile without `pyright\`. To open a workspace without a dialog, write `session.json` with a `root` instead.

## Docs, changelog and releases

- Every user-facing change gets a bullet under `## [Unreleased]` in `CHANGELOG.md`: `#### Added` / `Changed` / `Fixed`, with a **bold title** and a plain-language description of what the user sees.
- Shortcuts and settings are documented in `docs/USER_GUIDE.md`. The README's feature list summarises them.
- **To release**:
  1. Rename the Unreleased section to `## [vX.Y.Z] - date` and add the "Released as…" line. Earlier sections show the format.
  2. Push.
  3. Run the Release workflow (`gh workflow run release.yml -f version=X.Y.Z`), or comment `/release X.Y.Z` on an issue.

  The workflow:
  - tests and builds x64 and ARM64;
  - publishes the GitHub release, using the version's `CHANGELOG.md` section as the description (it fails if that section is missing);
  - commits the Scoop manifest update (`bucket/lightline.json`) to `main`. Pull after a release.
- Code signing (SignPath) is wired into `release.yml` but stays disabled until the repository variable `SIGNPATH_ENABLED` is `true`.
