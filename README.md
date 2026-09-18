# My Editor

A small Windows text editor written in Rust. It supports UTF-8 files, editing, undo/redo, saving, multiple tabs, and Rust syntax coloring. It paints only the visible lines of the active document.

The project's direction is recorded in [the v0.1 architecture](docs/ARCHITECTURE.md). Implemented changes are tracked in the [changelog](CHANGELOG.md).

The native editor's current visual direction is documented in the [reference adaptation](design/REFERENCE_ADAPTATION.md). An earlier interactive concept remains in the [Quiet Workbench prototype](design/workbench-prototype.html).

## Run

```powershell
cargo run --release
```

You can pass a UTF-8 file path as the first argument.

| Action | Shortcut |
| --- | --- |
| New tab, open file, save, save as, close tab | Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Shift+S, Ctrl+W |
| Switch tabs | Ctrl+Tab / Ctrl+Shift+Tab, or Ctrl+PageDown / Ctrl+PageUp |
| Select all, copy, cut, paste | Ctrl+A, Ctrl+C, Ctrl+X, Ctrl+V |
| Undo, redo | Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z |
| Find in current file | Ctrl+F, type a query, then Enter; F3 next, Shift+F3 previous; Escape cancels input |
| Move by word or delete a word | Ctrl+Left/Right, Ctrl+Backspace/Delete |
| Move to file start or end | Ctrl+Home/End |
| Show or hide the file explorer | Ctrl+B |

The explorer opens around the first file you open, preferring a nearby Cargo or Git project root. Click a folder to expand it or a file to open it in a tab. The rail's Search action opens the existing current-file find. The explorer loads only opened folders, so project contents are not indexed at startup.

Click a tab to switch to it, or click its × to close it. Each tab retains its cursor, selection, scroll position, undo history, and unsaved changes. Opening a file already open in a tab switches to that tab. Closing a dirty tab or the window prompts to save its changes. Arrow keys, Home, End, Page Up/Down, Enter, Backspace, Delete, Tab, mouse clicks, mouse drag selection, and the mouse wheel also work. Hold Shift while navigating to extend a selection. Find is currently case-sensitive and searches the active file only.

To stop a `cargo run` session, close the editor window with its X button. The terminal command will then finish. You can also focus the terminal and press Ctrl+C; the editor will follow its normal close path and ask about unsaved changes. Ctrl+C while the editor has focus is Copy. If an older editor process remains open, save your work and stop that process from PowerShell with `Get-Process my-editor | Stop-Process`.

Rust `.rs` files get syntax coloring for comments, strings, keywords, types, numbers, and macros. Files up to 128 KiB use Tree-sitter's Rust parser on a background worker; after edits, the worker updates the previous syntax tree. Colors appear when the worker finishes. Larger files use a lightweight lexer that caches line state and processes distant sections in small batches when you scroll. Other file types use plain text. Split panes, project-wide search, and LSP language intelligence are not yet included.

## Baseline measurement

```powershell
cargo run --release --example measure
```

The measurement creates a 100,000-line UTF-8 file, then times open, an insertion in the middle, and save. On the development machine, one release run measured 15.4 ms, 63.8 µs, and 641.8 ms respectively. These are document and disk timings; they do not measure window startup, input-to-paint latency, or idle memory. Repeat them on the target hardware before setting performance budgets.

To measure Rust syntax scheduling and completion separately, run `cargo run --release --example measure_syntax`. On the development machine, a 119 KB Rust sample took about 0.5 ms to schedule its initial parse and 0.08 ms to schedule an edit. Colors were ready after about 163 ms and 68 ms respectively. Those completion times are background work and vary by machine; the measurement does not include GDI painting.
