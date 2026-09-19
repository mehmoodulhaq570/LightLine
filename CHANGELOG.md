# Changelog

This is LightLine's development history. Dates are the GitHub commit dates in Asia/Karachi (UTC+05:00). The project has no tagged release yet, so these dates do not represent releases. Planned architecture is documented separately in [ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Unreleased

- Replaced the tab-bar Run Python text label with a VS Code-style green play button (hovering it shows the Ctrl+Shift+R hint in the status bar), and made LightLine auto-detect the Python interpreter — a `.venv` in the project or its parents first, otherwise the first `python.exe` on PATH — for both Run Python File and Pyright import resolution, so no manual selection is needed unless you want to override it. Covered by a detection test.
- Widened the tab-bar Run Python button so its Ctrl+Shift+R hint is never clipped, made the Run & Debug rail run the active Python file when a `.py` file is focused instead of reporting a Cargo.toml error, and replaced raw language-server spawn failures (for example Pyright missing from PATH) with an actionable install hint in the status bar, covered by a unit test.
- Added a Run Python File command (Ctrl+Shift+R, a tab-bar button, and a Ctrl+P command) that runs the active `.py` file with the selected interpreter or virtual environment in the file's detected project folder. Output streams to an input-capable terminal panel: stdout and stderr appear as produced, typed input is sent to the process's stdin on Enter, Stop and Ctrl+C end the whole process tree, and the panel shows the exit status. Saves dirty files before running, or prompts when a path is required first. All process spawning, streaming, and cancellation run on background threads and are cleaned up on stop or window close. Added workflow tests covering streamed output, a Python traceback and nonzero exit, interactive `input()`, cooperative cancellation, and Python project-root detection.
- Added first-class Python language support through the shared LSP client: lazy Pyright startup for `.py` files, diagnostics and hover, per-session interpreter or virtual-environment selection for import resolution, and independent server failure handling that keeps editing responsive. Verified the Windows npm launcher and Pyright file URIs with a live diagnostics, hover, and edit test.
- Added lazy rust-analyzer language support for Rust files: incremental document updates shared across split panes, diagnostics in the gutter and status bar, F1 and mouse hover, and a background server that cannot block editing. Added UTF-16 edit tracking and a live LSP test.
- Rebuilt every app icon size from `LightLine-icon2.png`, cropping its nearly transparent margin and sharpening small resizes; the activity rail now paints the supplied artwork at 32 px.
- Updated the bundled Windows application icon from `LightLine-icon2.png` for the title bar, taskbar, and executable.
- Added vertical split editor panes with a draggable divider, focused-pane tab switching, separate cursor/selection/scroll state, and shared document text and undo history. Added split controls, keyboard shortcuts, and tests for edit-position and tab-reference updates.
- Split the Windows UI out of the nearly 5,000-line `main.rs` into app state, workspace, tool panels, input, window setup, icons, and focused rendering modules. The entry point is now small; editor behavior is unchanged.
- Applied the new LightLine PNG as a multi-size Windows app icon in the title bar, taskbar, Start screen, activity rail, and executable resource. Moved Windows icon loading and file-icon mapping from `main.rs` into a dedicated module.
- Matched the supplied left workbench close-up with a lightning brand mark, `IDE` badge, six-row activity rail, compact explorer header and tree, workspace branch footer, and active-file chip. Updated navigation hit areas; unavailable Extensions and AI entries now explain their status.
- Added the native Quiet Workbench workflow: Start and recent workspaces, folder selection, a sliding project drawer, Quick Open for files and commands, project search with result previews, streamed and stoppable Rust test output, and read-only Git change review with side-by-side diffs.
- Added bounded, background workspace scans and on-demand Cargo/Git services; updated the README and reference design notes to distinguish implemented views from later tooling.
- Matched editor and explorer text sizes more closely, drew larger folder chevrons, and added 60%–200% interface zoom with Ctrl+Plus, Ctrl+Minus, and Ctrl+0.
- Reduced repaint flicker with a backbuffer and added a short crossfade when switching files or tabs.

## 2026-09-18

- Bundled 22 Material Icon Theme file and folder icons for the explorer and tabs, including the MIT notice, pinned version, and offline refresh script. Icon updates can ship with LightLine releases. ([0d986b0](https://github.com/mehmoodulhaq570/LightLine/commit/0d986b0138b20fe196e5e6d76dc0d5f950163cbe))
- Renamed the Rust package, executable, window, dialogs, and documentation to **LightLine**. Refined the rail and explorer with labeled navigation, compact rows, active-file reveal, and concise status messages. ([76d0d8e](https://github.com/mehmoodulhaq570/LightLine/commit/76d0d8eb354e4ee2921712e9fcd528c9c0e5c959))
- Adapted the dark navy reference design to the native editor with a lazy file explorer, breadcrumbs, active-line and indentation guides, dark title bar, and blue, violet, and teal accents. Added [reference adaptation notes](design/REFERENCE_ADAPTATION.md). ([5c0710e](https://github.com/mehmoodulhaq570/LightLine/commit/5c0710ea6e6511a3031593ee76004f2ce3a8cf2f))
- Added the clickable Quiet Workbench concept, screen previews, and workflow notes as an earlier UI proposal. ([1e25c2a](https://github.com/mehmoodulhaq570/LightLine/commit/1e25c2a5e1ba853d5c197a1c7e74653812648898))

## 2026-09-17

- Clipped line numbers to the gutter and kept editor painting out of the status bar. ([9591ab7](https://github.com/mehmoodulhaq570/LightLine/commit/9591ab72150d0914835418c06582b2c5eca325ae))
- Added Rust syntax coloring with Tree-sitter parsing for small files, bounded lexical coloring for larger files, background work, edit invalidation, and a syntax benchmark. ([2969a27](https://github.com/mehmoodulhaq570/LightLine/commit/2969a27d90f892a24c06d88c2af7cabc933361ee))
- Added multiple tabs with independent cursor, selection, scroll, and undo state, plus tab switching, duplicate-open detection, and save prompts for dirty tabs. ([fe74b0d](https://github.com/mehmoodulhaq570/LightLine/commit/fe74b0d71f4def1750c46652445eaa5928c8a702))
- Connected terminal Ctrl+C and Ctrl+Break to the editor's normal close path when launched from a console. ([cb4829e](https://github.com/mehmoodulhaq570/LightLine/commit/cb4829ee07c23242f7cc3d8f8aa5ebd6620aef28))
- Created the Windows Rust editor with UTF-8 open/save, editing, undo/redo, selection, clipboard and navigation shortcuts, in-file find, visible-line painting, DPI handling, document tests, and a 100,000-line measurement example. Recorded the v0.1 architecture and started this changelog. ([3e84be0](https://github.com/mehmoodulhaq570/LightLine/commit/3e84be0d3e2ecb82dd4593ff7dc1db20498f36cb))

## Status after the 2026-09-18 commits

LightLine has a native Windows editor, lazy project explorer, bundled Material icons, tabs, in-file search, and Rust syntax coloring. Project-wide search and split panes remain outstanding. GPUI, LSP, Git, an integrated terminal, debugger, plugins, and AI are future or conditional work. Large Rust files use a lexical fallback rather than Tree-sitter parsing.
