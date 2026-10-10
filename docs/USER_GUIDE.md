# LightLine User Guide

Everything the [README](../README.md) leaves out: every shortcut, every setting, and how to set up languages, the debugger, the AI Assistant and extensions.

- [Keyboard shortcuts](#keyboard-shortcuts)
- [Settings](#settings)
- [Unsaved-work recovery](#unsaved-work-recovery)
- [Languages, formatting and debugging](#languages-formatting-and-debugging)
- [AI Assistant](#ai-assistant)
- [Extensions and themes](#extensions-and-themes)
- [Verifying a download](#verifying-a-download)

---

## Keyboard shortcuts

### Unsaved-work recovery

LightLine checkpoints modified documents and untitled text approximately every five seconds when the session changes. Checkpoints are stored in `%APPDATA%\LightLine\session.json`; they do not save over your original files. The next launch offers **Restore** or **Discard**, including when you launch LightLine with a file argument.

If an original file changed or disappeared while LightLine was closed, its recovered text opens as an untitled buffer. Use **Save As** to choose where to keep it. An explicit **No** in a save-on-close prompt discards that document's unsaved text. Empty, untouched untitled tabs and generated previews have no recovery text.

Recovery is a periodic checkpoint: edits made after the latest completed checkpoint can be lost in a crash. Recovery write failures appear in the status bar; a failed final checkpoint keeps the window open. Recovery is stored locally as plain text.

Arrow movement, Backspace and Delete treat combining characters and joined emoji as complete characters. This does not add bidirectional layout or change the renderer's font shaping.

### Files and workspace

| Shortcut | Action |
| --- | --- |
| `Ctrl+P` | Quick Open files; type `>` for the Command Palette |
| `Ctrl+N` / `Ctrl+W` | New tab / close the active tab |
| `Ctrl+O` / `Ctrl+S` | Open a file / save the active file |
| `Ctrl+Shift+O` | Open a folder as the workspace |
| `Ctrl+Shift+W` | Close the workspace and return to the Welcome screen (also `>Close Workspace`) |
| `F2` / `Delete` | In the Explorer: rename / delete the selected file or folder |

### Editing and navigation

| Shortcut | Action |
| --- | --- |
| `Ctrl+F` | Find in the current file. `Enter` / `Shift+Enter` (or `F3` / `Shift+F3`) go to the next / previous match; `Esc` closes |
| `Ctrl+H` | Find and replace. `Tab` switches fields, `Enter` replaces the current match, `Alt+Enter` replaces all (one `Ctrl+Z` undoes it) |
| `Ctrl+Shift+F` | Search all files in the workspace; `Enter` moves to the results |
| `Ctrl+Space` | Show completions |
| `F1` | Show documentation for the symbol at the cursor |
| `F12` / `Shift+F12` | Go to definition / find all references |
| `F2` | Rename the symbol at the cursor everywhere it's used (see [Rename Symbol](#rename-symbol)) |
| `Ctrl+.` | Quick fixes and refactorings at the cursor or selection (see [Quick Fixes](#quick-fixes-and-refactorings)) |
| `Shift+Alt+F` | Format the document |
| `Alt+Z` | Toggle word wrap for this file (on by default for Markdown) |
| `Ctrl+Shift+V` | Preview a Markdown file (`>Markdown: Open Preview to the Side` shows it beside the file) |
| Gutter chevron | Fold or unfold a block (`⌄` / `›`) |

### Layout

| Shortcut | Action |
| --- | --- |
| `Ctrl+B` | Show or hide the side panel (also the ☰ at the top of the left rail) |
| `Ctrl+\` | Split the editor / close the split |
| `Ctrl+1` / `Ctrl+2` | Focus the left / right pane |
| `` Ctrl+` `` / `` Ctrl+Shift+` `` | Show or hide the terminal / open a new terminal tab |
| `Ctrl+Shift+G` | Source Control |
| `Ctrl+Shift+D` | Run & Debug |
| `Ctrl+Shift+X` | Extensions |
| `Ctrl+,` | Settings |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | Zoom in / out / reset |

### Run and debug

| Shortcut | Action |
| --- | --- |
| `Ctrl+Shift+R` | Run the active Python, C/C++, JavaScript, TypeScript, Go or Rust file/project |
| `Ctrl+Shift+B` | Run Rust tests (`cargo test`) |
| `F5` / `Shift+F5` | Start or continue debugging / stop |
| `F10` | Step over |
| `F11` / `Shift+F11` | Step into / step out |
| `F9` or a click in the gutter | Toggle a breakpoint |

In the search results and the Source Control list, `↑` / `↓` move, `Enter` opens, and `Space` stages or unstages. While your program runs in the Output pane, it receives what you type, including `Enter`.

### Direct language runners

Click the **Run** triangle or press **Ctrl+Shift+R**. LightLine saves changes first and runs the program in **Output**, with keyboard input and an exit status. Output stays visible after the program exits. Use **Stop Running Program** in the Command Palette, or the kill button while Output is selected, to stop it and its child processes. A second run is blocked until the first finishes or stops.

No runner extension is required. Install the language's runtime or compiler:

| Language | What Run executes | Required tool |
| --- | --- | --- |
| Python | The saved file with the selected interpreter | Python |
| C/C++ | Compile the file, then run it only if compilation succeeds | GCC/G++ or Clang/Clang++ on PATH |
| JavaScript (`.js`, `.mjs`, `.cjs`) | The file with Node.js | Node.js |
| TypeScript, JSX, TSX | The file with project-local `tsx`, or `tsx` on PATH | Node.js and `npm install --save-dev tsx` in the project, or `npm install -g tsx` |
| Go | `go run .` in the current package when a `go.mod`/`go.work` is found; otherwise `go run <file>` | Go |
| Rust | `cargo run` for Cargo source files, including named binaries/examples; loose files compile with `rustc` and then run | Rust/Cargo |

For JavaScript/TypeScript projects, a `start` script in the nearest `package.json` takes precedence; otherwise a `dev` script is used when present. These run from the package directory with `npm run`. Without a script, files run directly. Runtime configuration and project dependencies still apply; `tsx` runs TypeScript without type-checking it.

Rust **Run** executes the program; **Ctrl+Shift+B** and **Run Rust tests** run tests separately. Standalone Rust executables are built into a `lightline-run` folder in your temp directory, not beside the source file. Go packages must be executable `main` packages. JSON and other data files have no Run action.

### Saved Run configurations

Use the dropdown beside **Run** to switch between **Automatic** and saved configurations. Choose **Configure...** to edit them. The list supports arrow keys, Home/End, Enter, Escape and the mouse wheel. When the tab strip is crowded, use the editor's **...** menu or Command Palette to select a configuration.

Open the editor's **...** menu and choose **Run Configurations...**, or use **Run: Configure / Select Saved Configuration** in the Command Palette. **Automatic** keeps the language detection described above.

The panel is part of LightLine's workbench and follows the current editor theme. Select a configuration in the left-hand list, then edit its fields on the right. Use **Tab / Shift+Tab** to move between controls, **Ctrl+S** or **Ctrl+Enter** to save and select, and **Esc** to cancel. Inputs support selection, copy/paste, and undo/redo. In smaller windows, use the mouse wheel or drag the scrollbar; keyboard focus scrolls fields into view automatically.

Click **New**, enter a name, and customize any of these fields:

- **Entry file:** leave blank to follow the active file, or choose a fixed file.
- **Command:** leave blank to detect the language from the entry file, or enter an executable such as `npm`, `cargo`, or a full path. Put command options in Arguments; shell operators are not supported here.
- **Working directory:** leave blank for the detected directory, or enter another folder.
- **Arguments:** enter one argument per line. A line containing `hello world` is one argument; do not add shell quotes. For detected Cargo and npm runs, these are forwarded to the application or script.
- **Environment:** enter one `NAME=value` per line. These values apply only to the launched program and its children.

Paths are relative to the open workspace (or the saved file's folder when no workspace is open). **Save & Select** saves the configuration and makes it the default for **Run** / **Ctrl+Shift+R**. Select **Automatic** and click **Save & Select** to return to detection. **Delete** removes the selected saved configuration immediately. Use **Run** in the **...** menu to launch a saved project command while viewing a data file.

Configurations and the selected name persist in `.lightline/run.json`. Keep secrets out of this file if you share it or commit it to source control. Other open entry files with unsaved changes must be saved before launching.

---

## Settings

Click the **gear** at the bottom of the left rail, or press **`Ctrl+,`**, to open **Settings**. Changes apply at once and are saved to `%APPDATA%\LightLine\settings.json`.

To edit the file directly, click **Open settings.json** at the bottom of the panel or run **`>Open Settings (JSON)`**. Anything you leave out keeps its default:

```json
{
  "fontFamily": "Consolas",
  "fontSize": 14,
  "tabSize": 4,
  "insertSpaces": true,
  "wordWrap": false,
  "autoClosePairs": true,
  "autoIndent": true,
  "formatOnSave": false,
  "bracketMatching": true,
  "indentGuides": true,
  "markdownLoadRemoteImages": false,
  "colorTheme": "Dracula",
  "aiEndpoint": "http://localhost:11434",
  "aiModel": "qwen2.5-coder:7b",
  "colors": {
    "editorBg": "#141820",
    "text": "#d8dee9",
    "selectBg": "#264f78",
    "keyword": "#4a90e2",
    "string": "#a3be8c",
    "comment": "#6b7280"
  }
}
```

| Setting | What it does |
| --- | --- |
| `fontFamily`, `fontSize` | The code font (if installed) and its size, 8–48 |
| `tabSize`, `insertSpaces`, `autoIndent` | Indentation: tab width, spaces or tabs, and indenting new lines |
| `wordWrap` | Wrap long lines in every file (Markdown wraps by default) |
| `autoClosePairs` | Type closing brackets and quotes for you |
| `bracketMatching`, `indentGuides` | Highlight matching brackets; draw indentation guides |
| `formatOnSave` | Format the file before each save |
| `markdownLoadRemoteImages` | Load web images in Markdown previews without asking |
| `colorTheme` | An installed color theme, such as `"Catppuccin Mocha"`; leave it out for LightLine's own. **Preferences: Color Theme** sets it for you |
| `aiEndpoint`, `aiModel` | Where the AI Assistant connects, and which model it uses |
| `colors` | Override individual theme colors, on top of the color theme |

`minimap`, `smoothScrolling` and `parseLimitKb` are accepted but don't do anything yet.

---

## Languages, formatting and debugging

Colors, folding and search are built in. Running uses the language tools listed above. These add more:

- **Rust**: for errors, completions and go-to-definition, run `rustup component add rust-analyzer rust-src`.
- **Python**: install Node.js once (for example `winget install OpenJS.NodeJS.LTS`). The first time you open a Python file without Pyright, LightLine asks before downloading Pyright 1.1.414 (about 19 MB) into `%APPDATA%\LightLine\pyright`.
- **Debugging Rust**: needs `lldb-dap` on your `PATH`; it comes with LLVM or the Visual Studio C++ Build Tools.
- **Debugging Python**: needs `debugpy` in the Python that `Ctrl+Shift+R` uses (the selected one, a nearby `.venv`, or `python` on `PATH`). Run **Python: Install debugpy** from the Command Palette, or `python -m pip install debugpy`. Your program runs in the Output pane, so `input()` works while debugging.
- **C and C++**: files can be run, not yet debugged.
- **Formatting**: JSON and TOML format with nothing installed, keeping comments and key order. For JavaScript, TypeScript, CSS, HTML, Markdown and YAML, install Prettier in the project (`npm install --save-dev prettier`) or globally (`npm install -g prettier`); the project's copy is used first. LightLine only looks for it; it never installs it.

### Rename Symbol

Put the cursor on a variable, function, type or other name and press `F2` (or right-click → **Rename symbol**, or **Rename Symbol** in the Command Palette). Type the new name in the box below it and press `Enter`; `Esc` cancels. The language server finds every use, in all files of the project:

- Open files change in the editor, and are saved straight away unless they already had unsaved changes. One `Ctrl+Z` undoes the rename in that file.
- Files that aren't open are changed and saved on disk. Undo doesn't reach them, so use Git to review or revert a large rename.
- When the server can't rename something (a name from a library, say), the status bar says why and nothing changes. A rename that would also rename or move files isn't supported yet.

It needs the language's server (see above): rust-analyzer, Pyright, clangd, typescript-language-server or gopls.

### Quick Fixes and refactorings

Press `Ctrl+.` (or right-click → **Quick fix...**) to see what the language server can do at the cursor: fixes for the problem on that line, such as adding a missing import, and refactorings such as extracting a selected expression into a variable or function. Pick one with the arrow keys and `Enter`, or click it; `Esc` closes the list.

The change lands like an edit you made: `Ctrl+Z` undoes it in each open file, and the file you're editing stays unsaved. Other files it changes are saved, as with Rename Symbol. What's offered depends on the server: rust-analyzer and typescript-language-server offer many actions, Pyright only a few.

---

## AI Assistant

The AI Assistant (the sparkle in the left rail) chats about your code with a model running **on your own PC** through [Ollama](https://ollama.com): free, private, and it works offline. It stays off until you connect it, and until then nothing runs or is sent anywhere.

1. Install Ollama and download a model, for example `ollama pull qwen2.5-coder:7b` (on smaller PCs, `qwen2.5-coder:1.5b`).
2. Open the AI Assistant and click **Connect to Ollama**. LightLine picks a model made for code.
3. Type a question and press **Enter** (**Shift+Enter** for a new line). Selected code is sent along, and the panel shows which lines before you send.

Answers appear as they're written. **Esc** stops one, **+** starts a new chat, and the model name at the top switches models or turns the assistant off.

Each code block in an answer has three buttons:

- **Insert** adds the code at the cursor.
- **Replace** swaps it in for the code you asked about, if that code hasn't changed since.
- **Copy** copies it.

Inserted code is indented to fit where it lands, and one **Ctrl+Z** undoes it.

**From the editor:** right-click selected code for **Explain**, **Fix**, **Write Tests** or **Add Comments**. Right-click a red or yellow underline for **Explain This Error** or **Fix This Error**. These are also in the Command Palette under `>AI:`.

- **Other servers**: set `aiEndpoint` to any OpenAI-compatible server, such as LM Studio (`http://localhost:1234`).
- **Cloud models**: Ollama models whose names end in `cloud` run on ollama.com, not your PC. LightLine never picks one for you, and warns you if you do.

---

## Extensions and themes

The Extensions panel (`Ctrl+Shift+X`) installs extensions from the [Zed extension registry](https://github.com/zed-industries/extensions) into `%APPDATA%\LightLine\extensions`. Two kinds work today. Neither runs code inside LightLine.

Updates download and validate a staged copy before replacing an installed theme. A failed download, invalid manifest, malformed theme or missing icon asset leaves the existing installation intact. If replacement fails, LightLine attempts to restore the previous copy; any backup it cannot restore is retained and its location is reported.

- **Color themes** recolor all of LightLine, dark or light, including the terminal. Installing one, such as **Dracula**, switches to it at once and it's remembered. Themes with several variants (Catppuccin Latte, Frappé, Macchiato, Mocha) offer each in **Preferences: Color Theme**.
- **Icon themes** change file and folder icons; **Material Icon Theme** is the popular one. Without one, LightLine uses its built-in icons.

Other kinds of extensions, such as language servers, say they aren't supported yet rather than half-installing.

---

## Verifying a download

Each release includes `SHA256SUMS.txt`. In PowerShell, in the folder you downloaded to, run:

```powershell
Get-FileHash .\lightline.exe -Algorithm SHA256
```

The result must match the `lightline.exe` line in `SHA256SUMS.txt` exactly. For the ARM64 build or a ZIP, run it with that file's name instead. If they differ, delete the download.

LightLine isn't code-signed yet, so the first time you run it, Windows SmartScreen may say **"Windows protected your PC"**. If the checksum matches, click **More info**, check that the app is `lightline.exe`, and click **Run anyway**. Don't turn SmartScreen off, and don't continue if Windows reports malware rather than an unrecognized app.
