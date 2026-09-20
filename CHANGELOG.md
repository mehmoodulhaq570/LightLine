# Changelog

All notable changes to **LightLine** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added
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

### Fixed
- **Branch Detection Beyond `.git/HEAD`**: The checked-out commit is resolved through Git (`symbolic-ref`, falling back to `rev-parse --short HEAD`) and the repository top level through `git rev-parse --show-toplevel`, so linked worktrees, submodules, detached HEAD and workspaces opened on a subfolder of a repository report correctly.
- **Git Off The UI Thread**: Per-file gutter diffs no longer run inside `refresh()`, which had been spawning two Git processes after every repaint. Status, diff and history all arrive on the worker channel behind a generation counter, so a stale answer can never overwrite a newer one.
- **Refresh On Real Triggers**: The panel and branch chip update on workspace change, on save, when the window regains focus and after every completed write, rather than only when the panel is opened.
- **Serialised Writes**: One Git command may be in flight at a time; while it runs the row and commit buttons are drawn dimmed and further writes are refused instead of deadlocking on the index lock.
- **Dead Code Warnings**: Cleaned up unread struct fields in the extension model and integrated rating metadata into the rendered UI cards.

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
