# Changelog

All notable changes to **LightLine** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
