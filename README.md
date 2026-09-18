# LightLine

A small Windows code editor written in Rust. It supports UTF-8 files, editing, undo/redo, saving, multiple tabs, vertical split panes, Rust syntax coloring, rust-analyzer diagnostics and hover, Pyright diagnostics and hover for Python, workspaces, project search, Quick Open, Rust test output, and read-only Git review. It paints only visible document lines.

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
| Split or unsplit the editor; focus the left or right pane | Ctrl+Backslash; Ctrl+1 / Ctrl+2 |
| Select all, copy, cut, paste | Ctrl+A, Ctrl+C, Ctrl+X, Ctrl+V |
| Undo, redo | Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z |
| Find in current file | Ctrl+F, type a query, then Enter; F3 next, Shift+F3 previous; Escape cancels input |
| Move by word or delete a word | Ctrl+Left/Right, Ctrl+Backspace/Delete |
| Move to file start or end | Ctrl+Home/End |
| Show or hide the file explorer | Ctrl+B |
| Zoom the whole interface in or out; reset zoom | Ctrl+Plus / Ctrl+Minus; Ctrl+0 |
| Open a folder or return to Start | Ctrl+Shift+O; Ctrl+Shift+H |
| Quick Open files and commands | Ctrl+P; type `>` for commands |
| Search across workspace files | Ctrl+Shift+F; type a query, press Enter |
| Run Rust tests; review Git changes | Ctrl+Shift+B; Ctrl+Shift+G |
| Run the active Python file | Ctrl+Shift+R, or click **Run Python** in the tab bar, or Ctrl+P, type `>python`, choose **Run Python File** |
| Show Rust or Python hover information at the cursor | F1, or pause the mouse over code |
| Select a Python interpreter or virtual environment | Ctrl+P, type `>python`, choose the interpreter or virtual-environment command |

The Start screen offers Open File, Open Folder, New File, and recent workspaces. Clicking the LightLine name returns to Start. Opening a file picks a nearby Cargo or Git root; Open Folder chooses one explicitly. Recent workspaces are stored in `%APPDATA%\LightLine\recent-workspaces.txt`. The explorer loads only opened folders, so project contents are not indexed at startup. The activity rail follows the LightLine reference design; the branch shown in its workspace footer comes from `.git/HEAD`.

Quick Open lists workspace files when requested; type `>` to run a command. The Search drawer searches file contents after Enter, shows a context preview for the selected result, and opens a hit at its line with Enter or a click. Scans skip generated directories, cap file count and size, run off the UI thread, and stop when you leave Search. **Run & Debug** streams `cargo test --offline` output in a Rust workspace; click **Stop** or focus the output panel and press Ctrl+C to interrupt it. **Source Control** reads Git status on demand and presents a side-by-side, read-only diff for tracked and new text files. Escape closes an overlay or returns focus to code. A debugger, AI, and extension marketplace are later systems. Extensions and AI Assistant appear muted in the rail until implemented.

Zoom changes text, icons, panels, and spacing together in 20% steps from 60% to 200%. Ctrl+0 resets it to 100%. The zoom level lasts for the current session. Switching files uses a short visual transition.

Click a tab to switch to it, or click its × to close it. Each tab retains its undo history and unsaved changes. Opening a file already open in a tab switches to that tab. Use the **Split** button at the top right or Ctrl+Backslash to create two panes. Click a pane or use Ctrl+1 / Ctrl+2 to focus it; drag the divider to resize. Opening or switching files affects the focused pane. When both panes show the same file, they share one document and keep separate cursor, selection, and scroll positions. Closing a dirty tab or the window prompts to save its changes. Arrow keys, Home, End, Page Up/Down, Enter, Backspace, Delete, Tab, mouse clicks, mouse drag selection, and the mouse wheel also work. Hold Shift while navigating to extend a selection. Ctrl+F and project search are currently case-sensitive.

To stop a `cargo run` session, close the editor window with its X button. The terminal command will then finish. You can also focus the terminal and press Ctrl+C; the editor will follow its normal close path and ask about unsaved changes. Ctrl+C while the editor has focus is Copy. If an older editor process remains open, save your work and stop that process from PowerShell with `Get-Process lightline | Stop-Process`.

Rust `.rs` files get syntax coloring for comments, strings, keywords, types, numbers, and macros. Files up to 128 KiB use Tree-sitter's Rust parser on a background worker; after edits, the worker updates the previous syntax tree. Colors appear when the worker finishes. Larger files use a lightweight lexer that caches line state and processes distant sections in small batches when you scroll. Other file types use plain text.

For Rust diagnostics and hover, install the toolchain components with `rustup component add rust-analyzer rust-src`. LightLine starts rust-analyzer when you open a Rust file up to 2 MiB, using the nearest Cargo workspace when available. For Python diagnostics and hover, install Pyright so `pyright-langserver` is on PATH, for example `npm.cmd install -g pyright` in PowerShell. LightLine starts Pyright lazily when you open a `.py` file up to 2 MiB, using the nearest Python project marker, Git root, or open workspace. Use Ctrl+P, type `>python`, then choose **Select Python interpreter** or **Select Python virtual environment**; picking a venv uses its `Scripts\python.exe` or `bin/python` so imports resolve against that environment. Errors and warnings appear in the gutter and under the code; move the cursor onto a marked line to read its message in the status bar. Press F1 or pause the mouse over Rust or Python code for hover information. Edits are sent incrementally, and saving sends a save notification so diagnostics can refresh. Split panes showing the same file share one document and one LSP update. Each server runs separately from the UI; if it is slow or exits, editing continues and the status bar shows the error. This LSP slice does not include completion, navigation, or formatting.

To check Python support, open a `.py` file containing `value: int = "wrong"`, wait for the red diagnostic, then move the cursor to that line to read its message. Place the cursor on `value` and press F1 to see its type. Replace `"wrong"` with `7` and save; the diagnostic should clear. For an automated server check, run `cargo test --test lsp_live pyright_publishes_diagnostics_and_hover -- --ignored --nocapture` after installing Pyright. The Pyright interpreter choice lasts for the current LightLine session.

**Run Python File** (Ctrl+Shift+R, the **Run Python** button in the tab bar, or Ctrl+P then `>python` and **Run Python File**) launches the selected Python interpreter or virtual environment against the active `.py` file. If the interpreter has not been selected yet, choose it first with **Select Python interpreter** or **Select Python virtual environment**. The run uses the file's project folder as its working directory (the nearest folder with `pyproject.toml`, `setup.py`, `setup.cfg`, `requirements.txt`, `.venv`, or `.git`, falling back to the open workspace or the file's own folder), so relative imports and relative file access behave the same as running the script from that folder in a shell; interpreter, file, and folder paths are passed as-is, so paths containing spaces work without extra quoting from you. If the file has unsaved changes, LightLine saves it first (or, if Save As is required, asks you to finish saving before running). The panel that opens is a real input-capable terminal, not a read-only log: stdout and stderr appear as they are produced (Python is started with `-u` so prompts are not buffered), and typing while the panel is focused sends characters to the process's standard input on Enter, so scripts using `input()` work normally. Click **Stop**, or focus the panel and press Ctrl+C, to interrupt a run; LightLine asks Windows to end the whole process tree and reports "stopped by user". When the process ends on its own, the panel shows its exit status. Starting another run reuses the same panel; the previous process (if still busy) keeps running independently. All process I/O runs on background threads, never the UI thread, and any process left running is terminated when you close LightLine.

## Default icons

The LightLine app icon comes from [LightLine-icon2.png](LightLine-icon2.png). Regenerate its multi-size Windows icon after updating the PNG with `python tools/update_app_icon.py` (Pillow required for this developer step). Every icon size uses the supplied image; the script crops its nearly transparent outer margin and applies mild sharpening after shrinking the smallest sizes. The generated `assets/lightline.ico` is used in the title bar, taskbar, Start screen, and activity rail. On Windows MSVC builds, `build.rs` also embeds it in the executable using the Windows SDK resource compiler so Explorer shows the same icon.

LightLine bundles a small snapshot of [Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme) as its default file and folder icons. The pinned version and selected icon names are in [VERSION.json](assets/material-icon-theme/VERSION.json), with the upstream [MIT license](assets/material-icon-theme/LICENSE.txt). The app embeds the icons in its executable; users do not need VS Code, its extension, or a marketplace.

When upstream icons change, refresh the snapshot from a newer installed extension or repository checkout:

```powershell
python tools/update_material_icons.py --source "C:\path\to\vscode-material-icon-theme"
cargo test --offline
```

The refresh script needs PyQt5 and Pillow on the developer's machine to convert selected SVGs into Windows icons. These packages are not runtime dependencies. Review the changed assets and ship them in a LightLine release. Additional file associations can be added to the icon map in `src/windows_app/icons.rs`. There is no automatic icon update in this version.

## Baseline measurement

```powershell
cargo run --release --example measure
```

The measurement creates a 100,000-line UTF-8 file, then times open, an insertion in the middle, and save. On the development machine, one release run measured 15.4 ms, 63.8 µs, and 641.8 ms respectively. These are document and disk timings; they do not measure window startup, input-to-paint latency, or idle memory. Repeat them on the target hardware before setting performance budgets.

To measure Rust syntax scheduling and completion separately, run `cargo run --release --example measure_syntax`. On the development machine, a 119 KB Rust sample took about 0.5 ms to schedule its initial parse and 0.08 ms to schedule an edit. Colors were ready after about 163 ms and 68 ms respectively. Those completion times are background work and vary by machine; the measurement does not include GDI painting.
