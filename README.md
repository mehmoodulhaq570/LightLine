# My Editor

A small Windows text editor written in Rust. This first milestone supports opening UTF-8 files, editing, undo/redo, and saving. It paints only the visible lines of a document.

The project's direction is recorded in [the v0.1 architecture](docs/ARCHITECTURE.md). Implemented changes are tracked in the [changelog](CHANGELOG.md).

## Run

```powershell
cargo run --release
```

You can pass a UTF-8 file path as the first argument.

| Action | Shortcut |
| --- | --- |
| New, open, save, save as, close | Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Shift+S, Ctrl+W |
| Select all, copy, cut, paste | Ctrl+A, Ctrl+C, Ctrl+X, Ctrl+V |
| Undo, redo | Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z |
| Find in current file | Ctrl+F, type a query, then Enter; F3 next, Shift+F3 previous; Escape cancels input |
| Move by word or delete a word | Ctrl+Left/Right, Ctrl+Backspace/Delete |
| Move to file start or end | Ctrl+Home/End |

Arrow keys, Home, End, Page Up/Down, Enter, Backspace, Delete, Tab, mouse clicks, mouse drag selection, and the mouse wheel also work. Hold Shift while navigating to extend a selection. Find is currently case-sensitive and searches the open file only.

To stop a `cargo run` session, close the editor window with its X button. The terminal command will then finish. You can also focus the terminal and press Ctrl+C; the editor will follow its normal close path and ask about unsaved changes. Ctrl+C while the editor has focus is Copy. If an older editor process remains open, save your work and stop that process from PowerShell with `Get-Process my-editor | Stop-Process`.

This is an initial editor core and viewport prototype. It does not yet include syntax highlighting, tabs, split panes, or project-wide search.

## Baseline measurement

```powershell
cargo run --release --example measure
```

The measurement creates a 100,000-line UTF-8 file, then times open, an insertion in the middle, and save. On the development machine, one release run measured 15.4 ms, 63.8 µs, and 641.8 ms respectively. These are document and disk timings; they do not measure window startup, input-to-paint latency, or idle memory. Repeat them on the target hardware before setting performance budgets.
