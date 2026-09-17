# Changelog

Changes to the project are recorded here. Planned architecture is documented separately from implemented behavior.

## Unreleased

### Added

- Recorded the [v0.1 technical direction](docs/ARCHITECTURE.md), including system boundaries, technology candidates, lazy services, optional AI and plugins, performance measures, and the staged roadmap.
- Added this changelog so architecture and implementation changes can be tracked over time.
- Added select all, keyboard and mouse selection, copy/cut/paste, word and page navigation, new/close shortcuts, and case-sensitive find in the open file.

### Current prototype

- Windows Rust editor with UTF-8 open/save, editing, undo/redo, selection, clipboard commands, in-file find, and visible-line painting.
- Native DPI handling and visible text caret; close-window path corrected.
- Document tests and a 100,000-line open/edit/save measurement example.

### Not yet implemented

- v0.1 target features still outstanding: tabs, syntax highlighting, and broader search.
- GPUI, Tree-sitter, LSP, Git, terminal, debugger, plugins, and AI remain future or conditional work.
