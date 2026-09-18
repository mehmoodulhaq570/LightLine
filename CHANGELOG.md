# Changelog

Changes to the project are recorded here. Planned architecture is documented separately from implemented behavior.

## Unreleased

### Added

- Bundled 22 Material Icon Theme file and folder icons as the default explorer and tab icons, with its MIT notice, pinned version, and an offline refresh script. Icon updates can ship with LightLine releases without an extension marketplace.
- Renamed the app, Rust package, executable, window title, dialogs, and documentation to **LightLine**. Refined the reference-inspired rail and explorer with compact typography, labeled actions, file colors, active-file reveal, and concise status messages.
- Adapted the supplied dark-navy IDE reference to the native editor: activity rail, lazy file explorer, breadcrumb row, active-line and indentation guides, dark title bar, compact UI text, and blue/violet/teal syntax colors. Documented the reference's separate screens and future workflow in [the adaptation notes](design/REFERENCE_ADAPTATION.md).
- Recorded the [v0.1 technical direction](docs/ARCHITECTURE.md), including system boundaries, technology candidates, lazy services, optional AI and plugins, performance measures, and the staged roadmap.
- Added this changelog so architecture and implementation changes can be tracked over time.
- Added select all, keyboard and mouse selection, copy/cut/paste, word and page navigation, new/close shortcuts, and case-sensitive find in the open file.
- Connected terminal Ctrl+C/Ctrl+Break to the editor's normal close path when launched from a console.
- Added multiple tabs with per-tab cursor, selection, scroll, and undo state; tab switching and closing by mouse or keyboard; duplicate-open detection; and save prompts for each dirty tab on window close.
- Added Rust syntax coloring for `.rs` files: background Tree-sitter parsing and incremental reparsing up to 128 KiB, with lazy line-state caching and bounded scanning for larger files. Edits and undo/redo invalidate the relevant syntax state.
- Added a syntax benchmark that reports UI scheduling time separately from background completion time.
- Added a clickable Quiet Workbench design concept and screen previews for the editor's future UI and workflow.

### Current prototype

- Windows Rust editor with UTF-8 open/save, tabs, editing, undo/redo, selection, clipboard commands, in-file find, Rust syntax coloring, and visible-line painting.
- Native DPI handling and visible text caret; close-window path corrected.
- Document tests and a 100,000-line open/edit/save measurement example.

### Not yet implemented

- v0.1 target feature still outstanding: broader search. Tree-sitter parsing is initially limited to small Rust files; larger files use the lexical fallback.
- GPUI, Tree-sitter, LSP, Git, terminal, debugger, plugins, and AI remain future or conditional work.
