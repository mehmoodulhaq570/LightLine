# LightLine: the complete project context

Everything Claude needs to work on LightLine: what the product is for, how the code is built, the rules every change follows, and how we test, release and handle issues. The root `CLAUDE.md` holds a short version of the commands and conventions. This file is the full reference.

Longer documents, when you need detail:

| Document | What it holds |
| --- | --- |
| `docs/TECHNICAL_DIRECTION.md` | Vision, system boundaries, decision rules, roadmap, macOS plan |
| `ARCHITECTURE.md` | Subsystem diagram (Mermaid) and directory overview, for contributors |
| `docs/USER_GUIDE.md` | Every shortcut, every setting, language setup, AI, extensions |
| `docs/extension_implementation_plan.md` | Why extensions come from the Zed registry, and the adapter plan |
| `docs/RELIABILITY_VERIFICATION.md` | Recovery, staged extension installs, grapheme editing: behaviour and how it was verified |
| `CHANGELOG.md` | What each release changed, including speed measurements |
| `CONTRIBUTING.md` | Contributor setup, style, commit messages, changelog rules |

---

## 1. Vision

**LightLine is a code editor that feels like VS Code but starts fast and stays light.** It ships as one portable `.exe`: no installer, no Electron, no admin rights. It is written in Rust and draws every pixel itself through Win32 and GDI, with no GUI toolkit.

**Speed is the product.** It's what sets LightLine apart, so **performance ranks above features**. Startup time, idle wakeups and the cost of painting while typing or scrolling are constraints on every change, not something to clean up later. Today's numbers:

| Measure | Value |
| --- | --- |
| Cold start to a responding window | about 0.1 s |
| Keystroke to screen | about 2–3 ms (3,000-line Rust file, rust-analyzer running) |
| Idle, unfocused | 0 wakeups/s |

Releases are checked to make sure they don't regress; v0.7.0 is as fast as v0.6.0.

> Do the minimum amount of work necessary, at the latest possible time, without blocking the user's interaction.

**Complexity is opt-in.** Basic editing must stay useful and fast when language servers, Git, the terminal, extensions and AI are all off. A failure in any of them must leave documents editable and savable.

**Growth path:** text editor → code editor → project editor → IDE. Each stage must be solid before the next is built on it.

### Decision rules (from `docs/TECHNICAL_DIRECTION.md`)

1. Keep basic editing useful and responsive without optional services.
2. Start a subsystem only when a user action or open document needs it.
3. Update only the state a change affects.
4. Keep slow work off the UI thread, and discard stale results.
5. Bound memory, queues, caches and background work.
6. Keep text state separate from rendering and external integrations.
7. Isolate optional and failure-prone components.
8. Benchmark real workloads before choosing data structures, frameworks or optimizations.
9. Make Windows reliable first; add platforms deliberately.
10. Change library choices when the evidence changes, but keep these boundaries.

### Platform

- Windows 10 and 11, x64 and ARM64.
- macOS is not planned for now (issue #35, labelled `wontfix` but left open so people can find it). If a port happens, it keeps the self-drawn UI on a portable base: `winit`, `tiny-skia` with `softbuffer`, `cosmic-text`, `portable-pty`. The six steps are in `docs/TECHNICAL_DIRECTION.md`, starting with a `Canvas` trait in front of GDI.
- This is why the library crate must stay portable: never add Win32 calls to `src/` outside `windows_app/`, `terminal/platform.rs`, `clipboard.rs`, `credentials.rs` and `jobs.rs`.

### Privacy (a promise in the README)

LightLine collects no usage data and uses the network only when the user acts. That means:
- searching for or installing extensions;
- Git push, pull or fetch;
- **Python: Install debugpy**;
- web images in the Markdown preview, when the user allows them;
- downloading Pyright, only after asking;
- the AI Assistant.

AI API keys go in Windows Credential Manager (`credentials.rs`), never in LightLine's files. Don't add network calls or telemetry the user didn't ask for.

---

## 2. Commands

Windows only. You need the Rust toolchain and the Visual Studio C++ Build Tools. CI pins Rust **1.98.0**; Rust edition 2024.

```powershell
cargo run                                  # debug build; debug-only test hooks are compiled in
cargo build --release                      # what users get (thin LTO, 1 codegen unit; paints much faster than debug)
cargo fmt --check                          # CI gate
cargo clippy --locked --all-targets -- -D warnings   # CI gate: any warning fails
cargo test --locked                        # CI gate

cargo test --lib text_search               # one test or module in the library crate
cargo test --bin lightline merge_conflict  # tests inside src/windows_app (the binary crate)
cargo test --test lsp_live -- --ignored    # live tests: need real tools
cargo test --test runners_live -- --include-ignored   # real Python/Node/Go/Rust runs
cargo test --lib extensions -- --include-ignored      # hits the real Zed registry
cargo run --release --example measure_editing         # document benchmarks
```

- `tests/*_live.rs` (`lsp_live`, `runners_live`, `language_tools_live`, `expansion_live`) are mostly `#[ignore]`, because they start real language servers and toolchains.
- CI (`.github/workflows/rust.yml`) runs fmt, build, tests and Clippy on x64 and Windows ARM64 for every push and pull request to `main`.
- **Disk space on D: is tight** (it has filled up before):
  - Build with `CARGO_INCREMENTAL=0` in long sessions.
  - Reclaim space with `cargo clean --profile dev`; `target/prcheck` isn't ours.
  - To compare against a release, download the published exe instead of building it.

---

## 3. Crate layout

Two crates in one package.

### Library crate `lightline` (`src/lib.rs`): portable, no windows

| Area | Files | Notes |
| --- | --- | --- |
| Text model | `document.rs` | `Vec<String>` lines, kept after benchmarking up to 121 MiB (`docs/TEXT_MODEL_BENCHMARK.md`); undo groups; grapheme-aware movement and deletion; largest editable file `MAX_OPEN_BYTES` = 128 MB |
| Syntax | `syntax.rs`, `markdown_syntax.rs` | Tree-sitter for Rust, Python, C, JavaScript, TypeScript and JSON, on a worker with incremental edits. Recolors only edited lines plus the tree's changed ranges. Parse limit set by the `parseLimitKb` setting (default 4 MB); Rust files over 128 KB use a bounded lexical fallback |
| Language servers | `lsp.rs` | rust-analyzer, Pyright, clangd, typescript-language-server, gopls. One client per (language, workspace root), started lazily; handles crashes, versions, cancellation and UTF-16 positions |
| Debugger | `debug.rs` | DAP client: lldb-dap for native code, debugpy for Python |
| Terminal | `terminal/` (`mod`, `model`, `input`, `launch`, `platform`) | ConPTY plus a `vt100` model; PowerShell (`pwsh`), Command Prompt, Git Bash, WSL |
| Running code | `runner.rs`, `run_config.rs` | Direct runners for each language; saved configurations in `<workspace>/.lightline/run.json` |
| Formatting | `formatter.rs` | External formatters as processes (Prettier, rustfmt…); format-on-save runs after the save, never blocking it |
| Settings | `settings.rs` | `%APPDATA%\LightLine\settings.json` |
| Workspace services | `workflow.rs` | Git CLI, project search, session save/load, diff, Python/C toolchain detection, extensions folder |
| Search | `text_search.rs`, `gitignore.rs` | Plain/regex/case/whole-word matching; in-process `.gitignore` handling (spawning `git` costs ~50 ms) |
| History | `local_history.rs` | A copy on every save, in `%APPDATA%\LightLine\history`, 50 per file, up to 1 MB |
| File watching | `watcher.rs` | `ReadDirectoryChangesW`, one per watched folder |
| Extensions | `extensions/` (`zed_registry`, `installer`, `zed_manifest`), `color_theme.rs`, `icon_theme.rs` | See §6 |
| Markdown | `markdown.rs`, `image_cache.rs` | Blocks for the rendered preview; opt-in web image cache |
| AI | `ai.rs`, `credentials.rs` | OpenAI-compatible HTTP chat (Ollama by default; OpenAI, OpenRouter, Groq…) |
| Process lifetime | `jobs.rs` | Kill-on-close job object: every child process dies with LightLine |
| Clipboard | `clipboard.rs` | |

### Binary crate (`src/main.rs` + `src/windows_app/`): the whole UI, Windows only

`main.rs` is only the entry point. `window.rs` creates the window and routes Win32 messages.

---

## 4. How the UI works

### One `App` struct

- `windows_app/app.rs` holds **all** editor state. Features don't get their own UI-state types.
- Each feature file adds an `impl App { ... }` block and starts with `use super::*;`.
- `windows_app/mod.rs` re-exports the shared imports and types (`Document`, `Pos`, `Matcher`, Win32 items…) and the layout constants (`RAIL`, `STATUS`, `WORKBENCH_HEADER`, `TAB_HEIGHT`…).

Feature files, by area:

| Area | Files |
| --- | --- |
| Editing | `multi_cursor`, `indent`, `snippet`, `wrap`, `sticky`, `bracket_colors`, `occurrences`, `find_widget`, `search_options`, `goto_line`, `editor_context` (right-click menu) |
| Language features | `language` (diagnostics, hover, completion plumbing), `signature`, `inlay`, `rename`, `code_actions`, `symbols`, `problems`, `workspace_edit` |
| Run and debug | `codelens` (Run, Debug and Run test links), `run_language` (the file's language picker), `run_choice`, `run_config_ui`, `run_config_panel`, `debugger`, `debug_console` |
| Project | `workspace` (lazy Explorer), `session`, `git`, `blame`, `merge_conflict`, `local_history`, `replace_in_files` |
| Views | `markdown_view`, `image_view`, `binary_view`, `minimap` |
| Chrome | `panels` (side panels, Quick Open logic, workers), `settings_panel`, `more_menu`, `theme`, `theme_picker`, `color_theme_adapter`, `icons`, `builtin_icons`, `dialog`, `file_dialog`, `hot` (hover) |
| AI | `ai_chat`, `ai_provider` |
| Reliability | `crash` (panic → save unsaved work + `crash.log`) |
| Painting | `render/`: `editor`, `code_pane`, `panels`, `primitives`, `welcome`, `terminal`, `git`, `ai_assistant`, `completion`, `hover` |

### Input

- `window.rs::handle_message` sends keys and mouse events to `input.rs` (`App::key`, `mouse_click`…).
- Alt chords arrive as `WM_SYSKEYDOWN`. They reach `App::key` only if they're listed in `window.rs`'s `routed` condition.
- Modifiers are read with `GetKeyState`.
- Mouse `y` is in window coordinates. Sidebar code subtracts `chrome_top()`, and the sidebar is painted with the viewport shifted by the same amount, so painting and hit testing match.

### Painting

The goal is to redraw as little as possible.

- Painting goes into a persistent backbuffer (`render/editor.rs::paint`) and redraws only the invalid region.
- **`paint_code_area_only` is the fast path.** Typing, the caret blink and mouse-wheel scrolling must stay on it. The full-frame path redraws the chrome.
- When state changes, invalidate the **smallest rectangle** that changed.
- `scroll_blit.rs` moves rows inside the backbuffer on a wheel notch, and only the rows that come into view are drawn.
- Every drawn text row costs real time (GDI `ExtTextOut`, ~0.6 M cycles a row), so per-row work matters.
- Any new glyph that Segoe UI lacks must be added to `warm_font_fallback`, or drawn as vector graphics. Otherwise the first paint stalls.
- UI icons are SVG glyphs (`DebugGlyph` in `icons.rs`), drawn with `draw_glyph` / `glyph_in` at the exact size shown. Every icon is rasterized once into a shared atlas page and drawn with `AlphaBlend`; never make an HICON per icon (three GDI objects cost ~0.9 M cycles an icon, ~50 M on the first frame).
- The layout's sizes are constants in `windows_app/mod.rs` (VS Code's proportions: 35 px title bar, tabs and panel headers; 22 px rows and status bar). The side panel is painted from `side_origin()`, which hides the top `SIDE_SHIFT` pixels of each panel's header; clicks convert with the same origin.

### No polling timers

Background work runs on threads and wakes the window.

| Message | Sent by |
| --- | --- |
| `WM_APP+7` | LSP events |
| `WM_APP+8` | Terminal |
| `WM_APP+9` | Debugger |
| `WM_APP+10` | AI |
| `WM_APP+11` | Workers: a `WorkerMessage` through a `WorkerSender` |
| `WM_APP+12` | File watcher |

- Results carry ids or generations, so stale ones are dropped (for example the Explorer request ids and workspace generations).
- The only repeating timer is the caret blink (530 ms), and only while the window has focus. Everything else is one-shot: the session recovery snapshot armed from `paint()`, the status-bar timer, and short debounces (hover, inlay hints, occurrences, gutter diff). Don't add repeating timers.
- Animations (`motion.rs`: side panel and bottom panel easing, smooth scrolling, hover fade) run 16 ms frames only while something moves and stop at the end, so idle stays at 0 wakeups. They check `App::motion()`: the `animations` setting and Windows' Show animations. Never animate typing. The bottom panel's height on screen is `terminal_panel()`, not `terminal_height`.

### Child processes

- LSP servers, debug adapters, formatters, runs and git clones all go into the kill-on-close job (`jobs.rs`), so they die with LightLine.
- Never start a process at launch. Start it when a file or command needs it.

### Quick Open is several modes in one box

| Prefix | Mode |
| --- | --- |
| `>` | Commands |
| `@` | Symbols in the file |
| `#` | Workspace symbols |
| `:` | Go to line |

- Picker flags (`language_pick`, `history_pick`) reuse the same box.
- Adding a mode means updating `quick_count`, `activate_quick_item`, and the title, rows and hints in `render/panels.rs::paint_quick_open`.

### Other UI patterns to follow

- **Command Palette entries** are `(name, numeric id)` pairs in `panels.rs::QUICK_COMMANDS`, run by id in `run_command` (which `activate_quick_item` calls). Ids are global, so pick an unused one.
- **The menu bar** (`menu_bar.rs`) lists its items as data: each replays its shortcut through `key` (with `replayed_modifiers`, never the thread's key state) or runs a palette id. A new command that belongs in a menu goes in its list there.
- **"Links after a line"** (▶ Run, Debug, Run test, merge-conflict Accept…) all go through `codelens.rs`:
  1. `lens_on_line` decides whether a line gets links;
  2. `lens_links` lists them;
  3. paint records where each link was drawn;
  4. `lens_click` acts on a click.
- **The diff view** (Git review and Local History compare) is gated by `App::showing_diff()` and closed with `close_diff()`. Use these instead of checking `review_file` directly.
- **Edits must go through `App::replace_range`**, which also tells the language server and syntax worker. Wrap multi-step edits in `doc_mut().begin_group()` / `end_group()` so a single Ctrl+Z undoes them.
- **Tabs and split panes are views.** Two panes can show one document, each with its own cursor and scroll; text and undo history are shared. Never copy a document's text per view.
- **Session restore is lazy.** Only the visible tab is read at startup (`Tab::unloaded`).

---

## 5. User data and settings

Per-user state lives in `%APPDATA%\LightLine\`:

| Path | Contents |
| --- | --- |
| `settings.json` | User settings |
| `session.json` | Workspace root, tabs, cursors, and unsaved-text recovery checkpoints, written about every 5 s by one background writer, atomically |
| `history\` | Local History |
| `extensions\` | Installed extensions, with `.staging-*`, `.backup-*` and `.update-*.lock` during installs |
| `pyright\` | Downloaded Pyright |
| `cache\` | Caches |
| `recent-workspaces.txt` | Recently opened folders |
| `crash.log` | Written only after a crash |

Settings keys:
- **Editor:** `fontFamily`, `fontSize`, `tabSize`, `insertSpaces`, `wordWrap`, `autoClosePairs`, `autoIndent`, `smoothScrolling`, `parseLimitKb`.
- **Formatting:** `formatOnSave`, `prettierEnabled`.
- **Visual aids:** `animations`, `inlayHints`, `occurrencesHighlight`, `stickyScroll`, `bracketMatching`, `indentGuides`, `showMinimap`, `bracketPairColorization`, `gitBlame`, `codeLens`.
- **Other:** `localHistory`, `colorTheme`, `aiEndpoint`, `aiModel`, `markdownLoadRemoteImages`, `terminalDefaultProfile`.

**Adding a setting means editing `settings.rs` in five places:** the struct field, `Default`, parsing, serializing, and the `OWN_KEYS` list. Document it in the settings table in `docs/USER_GUIDE.md`.

If the library needs a setting globally, push it in at load and on reload through a setter, like `syntax::set_parse_limit_kb` and `local_history::set_enabled`. Do the same for any new one.

---

## 6. Extensions

- **The catalogue is the Zed registry** (`zed-industries/extensions`). LightLine doesn't run its own marketplace and doesn't support VS Code `.vsix` files. The reasons are in `docs/extension_implementation_plan.md`.
- **Lookup** (`zed_registry.rs`):
  1. Fetch `extensions.toml` (~1,580 entries), reused for 10 minutes.
  2. Find the submodule.
  3. GitHub's contents API gives the repository and the pinned commit in one request.
- **Install** (`installer.rs`):
  1. `git init`, then `fetch --depth 1` of exactly the pinned commit into a staging folder.
  2. Check the manifest and assets.
  3. Move it into place, keeping a backup and rolling back on failure.

  This needs `git` on PATH. Installs are serialized with an exclusive lock file.
- **What works today:** data-only extensions, meaning icon themes (e.g. Material Icon Theme) and color themes (e.g. Dracula, Catppuccin). Anything else is refused with "isn't supported yet". Extensions that need WASM would require an adapter first.
- **Entries with a `path`** live inside another repository (`html`, `glsl` and `proto` are inside Zed's own). They're refused before anything is downloaded.
- **Built-in entries:** Prettier is a curated "extension" that reports whether the Prettier CLI is available; it isn't downloaded. The Featured list is Prettier, Material Icons and Dracula. Searching shows the rest of the registry.

---

## 7. Language support, running and debugging

- **Run** (`Ctrl+Shift+R`, or the ▶ Run link beside `main`) uses the toolchain installed on the user's PC. LightLine bundles none.

  | Language | What runs it |
  | --- | --- |
  | Python | Selected interpreter → nearby `.venv` → `python` on PATH |
  | C/C++ | gcc/g++ or clang/clang++ |
  | JavaScript | `node`; the project's `start`/`dev` npm script if it has one |
  | TypeScript | `tsx`, local or global |
  | Go | `go run` |
  | Rust | `cargo run` inside a Cargo project; `rustc` into `%TEMP%\lightline-run` for a single file |

- Programs run in the **Output** pane, accept keyboard input and keep their exit status. If a tool is missing, LightLine shows an error with setup guidance.
- The status-bar language name (or **Run: Choose the File's Language**) picks which language to run the file as. The choice is remembered per file.
- **Debugging:** Rust and native code use lldb-dap; Python uses debugpy, which **Python: Install debugpy** installs. Debugging offers breakpoints, stepping, variables, call stack and a Debug Console.
- **Language servers** start only when a file of that language is opened. If one is slow or crashes, typing must not be affected.

---

## 8. Performance: how to measure

When reviewing or proposing work, check these first:
- startup time;
- idle wakeups and CPU;
- the paint cost of typing and scrolling.

Flag anything that starts a process at launch, polls on a timer, copies a whole document on each keystroke, or invalidates the whole window for a small change.

- **Wakeups:** watch `\Thread(lightline*)\Context Switches/sec` with typeperf or Get-Counter. Post `WM_KILLFOCUS` first, so the caret blink doesn't skew the count.
- **CPU cost:** compare `QueryThreadCycleTime` rather than wall-clock time, which swings 2× when the PC is busy.
- **Paint time:** use a temporary `WM_PAINT` logger, not timers around individual GDI calls (GDI batches its calls).
- **Repaint correctness:** debug builds answer `SCROLL_CHECK_MESSAGE` (`WM_APP+97`) with the number of backbuffer pixels that differ from a full redraw. Set `LIGHTLINE_SCROLL_CHECK_LOG` to get their positions.
- **Busy LSP threads aren't polling:** LSP reader threads can show around 90 wakeups/s while rust-analyzer is busy. That's message traffic, not polling, so check per-thread counters before reporting a regression.
- **Comparing releases:** use the official release exes, which saves a build and gives a fair comparison.

### Debug-build test hooks

| Message | Purpose |
| --- | --- |
| `WM_APP+96` | Geometry check (`minimap.rs`) |
| `WM_APP+97` | `SCROLL_CHECK_MESSAGE`, the repaint check (`scroll_blit.rs`) |
| `WM_APP+98` | `HOVER_TEST_MESSAGE`; `WM_MOUSEMOVE` can't drive hover because `TrackMouseEvent` sees the real cursor elsewhere |
| `WM_APP+99` | Crash test |

---

## 9. Driving the app live

The maintainer is usually using this PC while tests run.

Keep the window **invisible, never focused, and never opening dialogs** over the maintainer's work.

- **Make the driver DPI-aware.** Call `SetProcessDpiAwarenessContext(-4)` first. Without it, on this 125% display, window sizes, screenshots and click positions are scaled differently from LightLine's: clicks land about 48 px off and the status bar gets cut off. That looks like a LightLine bug but isn't.
- **Hide the window this way, not off-screen.** A window parked at 20000,20000 doesn't get reliable `WM_PAINT`, so captures come back black or out of date. Instead:
  1. Add `WS_EX_LAYERED | WS_EX_TRANSPARENT`.
  2. Set alpha 0 with `SetLayeredWindowAttributes`.
  3. Move it to 0,0 at the bottom of the z-order with `SWP_NOACTIVATE`.
  4. Capture with `GetDC` + `BitBlt`.
  5. **Before every capture**, check that the window is still layered, transparent and at alpha 0. If it isn't, the capture would copy the maintainer's own screen.
- **Give focus back** right after the window appears: `AttachThreadInput` to the foreground thread, then `SetForegroundWindow(previous)`.
- **Use a fresh, isolated `APPDATA` folder for every run.** Never reuse one after killing LightLine with unsaved edits, because the next launch shows a Restore prompt.
- **Open a workspace through `session.json`.** Write `{"root": "<dir>", "active": 0, "tabs": [{"path": "<file>"}]}` to `<profile>\LightLine\session.json`. **Never pass a folder on the command line**, because that opens a dialog.
- **Python files:** copy `%APPDATA%\LightLine\pyright` into the test profile, or opening a `.py` file asks to download Pyright.
- **Input:**
  - Send keys with `SendMessage`, not `PostMessage`. Sent messages jump ahead of posted ones, so posted characters would arrive after a sent Enter.
  - For Ctrl/Shift chords, attach to LightLine's UI thread, set the key state with `SetKeyboardState`, send `WM_KEYDOWN`, then clear the state. Alt chords are sent as `WM_SYSKEYDOWN`.
  - Clicks are `WM_LBUTTONDOWN`/`UP` in client pixels.
- **Keyboard routes, so you don't need click positions:**

  | Keys | Opens |
  | --- | --- |
  | `Ctrl+Shift+R` | Runs the file |
  | `Ctrl+Shift+X` | Extensions |
  | `Ctrl+Shift+P` | Command Palette |
  | `Ctrl+Shift+D` | Run & Debug |
  | `Ctrl+Shift+G` | Source control |

- **Dialogs:** a dialog is a `LightLineDialog` window. If one appears, close the test instance straight away.
- **Clean up:** kill every test instance. Delete the test profiles (each one copies about 30 MB of Pyright) and any downloaded exes.

---

## 10. Issues and GitHub

- **Repository:** `mehmoodulhaq570/LightLine`. Always pass `-R mehmoodulhaq570/LightLine` to `gh`; working out the repo from the remote can time out.
- **Reading issues:** use `gh issue view N --json author,body,createdAt` for the report itself; `--comments` shows only the replies.
- **Reports are often one line.** Before calling something a bug:
  1. Read the code path the report describes.
  2. Run the matching live tests.
  3. Drive the **published release** the user runs: `gh release download vX.Y.Z -R mehmoodulhaq570/LightLine -p lightline.exe -p SHA256SUMS.txt -D <dir>`, then check the hash.
  4. Report what reproduces and what doesn't, with evidence.
- **Common causes on the user's side:**
  - an old build;
  - a missing toolchain;
  - no network access to GitHub;
  - no `git` on PATH;
  - trying to install an extension that isn't a theme.
- **Replies:**
  - Never post comments or close issues without the maintainer's approval. Show the draft first.
  - Write in the maintainer's voice, plainly, and end with "Warm regards".
  - When a report doesn't reproduce, say what was tested and on which version. Then ask for the user's version, the exact status-bar or Output message (or a screenshot), and the relevant `--version` checks.
  - Mention fixes that aren't released yet as "in the next release".
- **Long-lived open issues:**
  - #13 stays open until releases are code-signed (SignPath; see §11).
  - #35 is macOS support (`wontfix`, open so people can find it).

---

## 11. Changes, docs and releases

- **Branch before committing on `main`.** Commit and push only when asked.
- **Changelog:** every user-facing change gets a bullet under `## [Unreleased]` in `CHANGELOG.md`, in `#### Added` / `Changed` / `Fixed`. Each bullet has a **bold title** and a plain-language description of what the user sees. Include measurements for speed changes.
- **Documentation:** shortcuts and settings go in `docs/USER_GUIDE.md`, and the README's feature list summarises them. Update the `ARCHITECTURE.md` diagram when a core system is added.
- **To release:**
  1. Rename the Unreleased section to `## [vX.Y.Z] - date` and add the "Released as…" line. Earlier sections show the format.
  2. Push.
  3. Run `gh workflow run release.yml -f version=X.Y.Z`, or comment `/release X.Y.Z` on an issue.

  The workflow tests and builds x64 and ARM64. It publishes the GitHub release with that version's changelog section as the description, and fails if the section is missing. It then commits the Scoop manifest update (`bucket/lightline.json`) to `main`, so pull after a release.
- **Release assets:** `lightline.exe`, `lightline-arm64.exe`, a zip for each architecture, and `SHA256SUMS.txt`. Users can also install with `scoop install lightline`.
- **Code signing** through SignPath is wired into `release.yml`. It stays off until the repository variable `SIGNPATH_ENABLED` is `true`. Until then, users see SmartScreen and are told to check the hash and choose **More info → Run anyway**.
