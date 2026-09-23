# Changelog

All notable changes to **LightLine** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### 2026-09-23

#### Added
- **Explorer File Operations & Context Menu**: Full support for file and folder management in the Explorer. Added dedicated vector toolbar action buttons (New File, New Folder, Refresh, Close Folder) and right-click context menu with `New File...`, `New Folder...`, `Reveal in File Explorer`, `Copy Path`, `Copy Relative Path`, `Rename...`, and `Delete`.
- **Inline Tree Creation & Rename**: Creating or renaming items renders an active input capsule with cursor directly inside the tree (press `Enter` to commit, `Esc` to cancel).
- **Safe Deletion with Dark Confirmation**: Deleting files or directories displays LightLine's custom dark confirmation dialog, automatically closes open tabs for deleted files, and updates the directory tree.
- **Close Workspace / Discard Project**: Added `>Close Workspace` to Command Palette and Explorer header, allowing clean reset back to the Welcome screen.
- **Workbench Header & Centered Command Center**: Dedicated top-level header bar with the brand mark, centered Quick Open command center, and native window controls.
- **Open-Source Documentation Suite**: Added `CONTRIBUTING.md` (Win32/Rust development guide, PR requirements), `ARCHITECTURE.md` (Mermaid architectural diagram and module breakdown), and `CODE_OF_CONDUCT.md` (Contributor Covenant v2.1).
- **CI/CD Pipeline Streamlining**: Automated GitHub Actions testing workflow for `cargo build`, `cargo test`, and `cargo clippy`.

#### Fixed
- **Explorer Disappearing Files On Watcher Event**: Fixed directory cache eviction where `WatchEvent::DirectoryChanged` removed directories from `directory_cache` without reloading them, causing all files in the Explorer tree to vanish. Added defensive reloading in paint cycle and automatic reloading of open workspace and expanded directories.
- **Corrupted Explorer Toolbar Buttons & Text Overlap**: Replaced multi-byte Unicode emoji strings (which caused Win32 GDI font fallback corruption where `📁+` rendered as `📁:`) with crisp, native GDI vector icons. Expanded folder name clipping bounds and separated collapse-sidebar from workspace action buttons.
- **Workspace Root Folder Collapse/Expand**: Clicking the workspace root folder or its chevron now properly toggles expansion (updating chevron between `v` and `>`, toggling child visibility, and changing folder icon between open and closed). Added "Collapse all folders" button that preserves the workspace root.
- **Single Tab Close Refresh Glitch**: Closing the only open tab when no workspace was active used to immediately respawn a blank untitled tab instead of returning to the Welcome screen, making the close button feel like an in-place page refresh.
- **Project Discard & Files Undeletable**: Resolved issues where projects could not be discarded and files could not be deleted from within the editor (issues #37, #38, #42).

### 2026-09-22

#### Added
- **Real Zed Extension Registry Integration**: LightLine now consumes extensions from the live [Zed extension registry](https://github.com/zed-industries/extensions) at runtime instead of a hardcoded local list — fetches the real registry index, resolves an extension's git repository, and installs/uninstalls it via `git clone`.
- **Material Icon Theme, for real**: File and folder icons are now sourced from the actual [Material Icon Theme Zed extension](https://github.com/zed-extensions/material-icon-theme) (1000+ real icons, installed automatically on first launch) instead of a bundled 22-icon subset. SVGs are rasterized to native icons on demand and cached.
- **Zed Color Theme Adapter**: Install a real Zed color-theme extension (e.g. Dracula) from the Extensions panel and LightLine's editor background, text, selection, cursor, line numbers, diagnostics, and syntax colors update live, with no restart. Colors a theme doesn't provide fall back to LightLine's own defaults.
- **Centralized Theme/Palette**: Introduced a single `Theme` struct holding every core editor/chrome/syntax color, replacing ~17 scattered constants used across a dozen render files — the foundation the color-theme adapter builds on, and a prerequisite for any future light theme or user-defined palette.
- **Generic Extensions Panel**: The Extensions view now supports searching any extension in the Zed registry by name or ID, not just the two built-in entries, with real Install/Uninstall wired to the registry + installer.
- **Generic Formatter System**: Added a small `Formatter` trait (`LightLine → Formatter → external process → formatted code → LightLine`) with Prettier as the first implementation — process spawning, stdin/stdout handling and a hard timeout are now shared, reusable infrastructure instead of Prettier-specific code, ready for rustfmt/Black/clang-format later.
- **Format on Save**: New `formatOnSave` setting runs the active formatter automatically before a file is written to disk.
- **Find References (`Shift+F12`)**: Added `textDocument/references` support to the LSP client, showing every usage of the symbol under the cursor in the Search panel.
- **Real C/C++ Run Support**: `Ctrl+Shift+R` / the Run button now compiles and runs C/C++ files directly (detects `g++`/`clang++` or `gcc`/`clang`), instead of only supporting Python and Rust.
- **C/C++ Inline Diagnostics**: A background `-fsyntax-only` compile now surfaces C/C++ syntax errors and warnings as the same squiggly-underline markers Rust and Python already get from their language servers.
- **Debugger Variables Panel**: Struct and collection values in the Run & Debug VARIABLES panel are now expandable/collapsible (fetches nested values from the debug adapter on click), and the row layout no longer misaligns.
- **Clickable Language Indicator**: The language name in the status bar is now a real control — clicking it opens the command palette pre-filtered to that language's run/setup actions (previously inert text).

#### Changed
- **Prettier No Longer Mutates Global npm State**: The Extensions panel's Prettier toggle used to run `npm install -g prettier`/`npm uninstall -g prettier` behind the scenes, silently changing the user's global npm environment, and could report "installed" even when that install had actually failed. It now only detects whether `prettier` (or `npx prettier`) is actually runnable and enables/disables LightLine's own formatting feature accordingly — nothing is installed or removed outside LightLine's own extensions folder.
- **Syntax and Editor Colors Now Theme-Driven**: Every core editor/chrome color (background, text, selection, cursor, line numbers, diagnostics, and all syntax highlighting colors) now reads from the centralized `Theme`, with no change to the default appearance. `settings.json`'s `colors` overrides are resolved once at startup instead of on every repaint.

#### Fixed
- **Indentation Settings Were Never Applied**: `tabSize`, `insertSpaces`, and `autoIndent` existed in `settings.json` and round-tripped correctly, but nothing in the editor actually read them — indentation was hardcoded to 4 spaces everywhere and the status bar always showed "Spaces: 4". They're now wired into auto-indent, tab rendering width, and the status bar.
- **Extension Installer Assumed Every Release Is Tagged**: Installing a Zed extension always tried `git clone --branch v<version>`, which fails for extensions whose repository never tags releases (confirmed against the real Dracula theme). It now falls back to the repository's default branch when the tagged clone fails.

### 2026-09-20

#### Added
- **Interactive DAP Debugger**: Integrated native Debug Adapter Protocol (DAP) client communicating with `lldb-dap`. Supports one-click build and debug launch (`F5`), execution stepping (`F10` Step Over, `F11` Step Into, `Shift+F11` Step Out), session stop (`Shift+F5`), and restart.
- **Variables & Call Stack Inspection**: Added real-time scopes inspector (Locals, Arguments, Registers) with expandable variable trees and values, along with multi-threaded call stack inspection that jumps directly to source lines on click.
- **Gutter Breakpoints**: Click any line number in the editor gutter to toggle visual red breakpoint indicators. Breakpoints automatically shift and persist when lines are inserted or deleted during editing.
- **Real Source Control**: The Git panel now stages (`git add`), unstages (`git restore --staged`), discards (`git restore`, `git clean -f` behind a native confirmation that names the file) and commits (`git commit`) instead of only listing changes. A commit message box sits above the change list; with nothing staged, committing offers to stage every change first.
- **Staged and Changes Sections**: Status now comes from `git status --branch --porcelain=v2`, so the list splits into what will be committed and what will not, keeps both paths of a rename, marks conflicts, and shows a file that is partly staged in both sections.
- **Status Words**: Rows read `Modified`, `Added`, `Deleted`, `Renamed`, `Conflicted` or `Untracked` instead of raw two-character plumbing codes, and the branch chip carries `↑n`/`↓n` ahead-and-behind counts from the `## branch...remote` record.
- **Sync Commands**: Push, Pull (`--ff-only`) and Fetch (`--all`) buttons run in the terminal panel, so Git's own output and any Git Credential Manager prompt stay visible and no credentials are ever handled in-process.
- **Diff Navigation**: Clicking a line in the side-by-side diff opens that file in the editor at the clicked line.
- **Modernized Extensions Panel**: Overhauled the extensions view (`Ctrl+Shift+X`) with authentic branding cards, detailed download metrics, ratings, installation states (`Install`, `Installing...`, `✓ Installed`), and a workflow capabilities guide.
- **Activity Bar & Debug Shortcuts**: Added `Ctrl+Shift+D` to toggle Debug, `Ctrl+Shift+X` to toggle Extensions, and standard function keys (`F5`, `Shift+F5`, `F10`, `F11`, `Shift+F11`) for execution control.

#### Fixed
- **Branch Detection Beyond `.git/HEAD`**: The checked-out commit is resolved through Git (`symbolic-ref`, falling back to `rev-parse --short HEAD`) and the repository top level through `git rev-parse --show-toplevel`, so linked worktrees, submodules, detached HEAD and workspaces opened on a subfolder of a repository report correctly.
- **Git Off The UI Thread**: Per-file gutter diffs no longer run inside `refresh()`, which had been spawning two Git processes after every repaint. Status, diff and history all arrive on the worker channel behind a generation counter, so a stale answer can never overwrite a newer one.
- **Refresh On Real Triggers**: The panel and branch chip update on workspace change, on save, when the window regains focus and after every completed write, rather than only when the panel is opened.
- **Serialised Writes**: One Git command may be in flight at a time; while it runs the row and commit buttons are drawn dimmed and further writes are refused instead of deadlocking on the index lock.
- **Dead Code Warnings**: Cleaned up unread struct fields in the extension model and integrated rating metadata into the rendered UI cards.
- **Terminal Input Lost While a Program Runs**: The read-only Output pane used to permanently refuse keyboard focus, so typing meant for a running program's `stdin` (e.g. Python's `input()`) silently landed in the code editor instead. The Output pane now takes focus for the duration of a live run (including `Ctrl+C` to a hung process) and reverts to read-only once it finishes.
- **Screen Flicker While Typing**: Every cursor move was calling `ShowScrollBar` even when its visibility wasn't changing, forcing a native frame recalculation on each keystroke. It's now only called when the scrollbar's shown/hidden state actually changes.
- **Auto-Inserted Bracket Rendered as a Blank Box**: The bracket-match highlight was painted after the character it highlights, erasing the glyph underneath — most visible as an auto-inserted closing `)` turning into an unlabeled solid rectangle. The highlight now redraws the character on top of itself.
- **Run Silently Doing Nothing for Python/C++**: `Run` always assumed a Cargo workspace, so clicking it on a Python or C++ file (or from the welcome screen) did nothing or tried to run `cargo test` regardless of the open file's language.

---

## [v0.1.0] - 2026-09-20 — Initial Release

This is the initial tagged and packaged public release of LightLine for Windows.

### Added
- **User Settings System**: Introduced a persistent JSON-backed settings system at `%APPDATA%\LightLine\settings.json` (`Ctrl+,` or `>Open Settings (JSON)`) supporting configurable font family, size, tab size, spaces vs tabs, word wrap, bracket matching, indent guides, syntax limits, and custom theme colors.
- **VS Code-Style Sidebar Views**: Added dedicated **Debug** and **Extensions** views to the activity rail, with smooth toggle/collapse behavior matching VS Code.
- **Bracket Matching**: Added visual bracket pair highlight (`()`, `[]`, `{}`) across lines when the caret is adjacent to a bracket.
- **Multi-Tab Terminal**: Integrated ConPTY terminal supporting multiple concurrent interactive PowerShell tabs (`Ctrl+Shift+\``), tab switching, and per-tab closing.
- **LSP Intelligence**: Full language server protocol integration for Rust (`rust-analyzer`) and Python (`Pyright`), including hover docs (`F1`), go-to-definition (`F12`), autocompletion (`Ctrl+Space`), and formatting (`Shift+Alt+F`).
- **Automatic Pyright Installation**: Background download and management of Pyright language server on first `.py` file open.
- **Binary Previews**: Added read-only hex-dump view for binary files and image preview for raster images.
- **Session Restore**: Reopens previous workspace, open tabs, active pane, scroll offsets, and cursor positions on launch (`%APPDATA%\LightLine\session.json`).

### Fixed
- **UTF-8 & Slice Safety**: Added safe character-boundary clamping (`safe_slice_prefix`, `safe_slice_range`) across selections, syntax tokens, diagnostics, bracket matching, and caret positioning to eliminate out-of-bounds panics.
- **ConPTY Process Spawning**: Removed conflicting `DETACHED_PROCESS` flag, fixing immediate shell termination on launch.
- **Terminal Prompt Formatting**: Stripped extended-length `\\?\` prefix from canonicalized paths in terminal and Git review.
- **Dialog Dark Theme**: Opted dialogs into native Windows dark mode via `uxtheme.dll` and added custom dark confirm modals.

---

## [0.0.3] - 2026-09-19

### Added
- **Run Python File**: Integrated execution of the active Python script (`Ctrl+Shift+R` or play button) streamed into the dedicated Output tab.
- **Python Project Root & Venv Detection**: Automatic resolution of nearby virtual environments (`.venv`) and project markers (`pyproject.toml`, `requirements.txt`).
- **Bracketed Paste & Shell Controls**: Added support for ConPTY bracketed paste sequences and PowerShell 7 detection with Windows PowerShell fallback.

---

## [0.0.2] - 2026-09-18

### Added
- **Python Syntax Highlighting**: Background Tree-sitter tokenization for Python with plain-text fallback for large files.
- **Vertical Split Editor**: Support for split panes (`Ctrl+\`), independent scroll/cursor state, and pane resizing.
- **Quiet Workbench UI**: Added sliding project drawer, Quick Open (`Ctrl+P`), workspace search (`Ctrl+Shift+F`), and read-only Git diff review (`Ctrl+Shift+G`).
- **Bundled Material Icons**: Embedded offline file and folder icons from Material Icon Theme.
- **Interface Zoom**: Full UI scaling from 60% to 200% via `Ctrl+Plus`, `Ctrl+Minus`, and `Ctrl+0`.

---

## [0.0.1] - 2026-09-17

### Added
- Initial native Windows code editor implementation using pure Rust and Win32 GDI.
- UTF-8 document buffer with undo/redo tree, search, and visible-line rendering.
- Incremental Tree-sitter syntax highlighting for Rust with lexical fallback.
- Multi-tab document management with unsaved change prompts.
