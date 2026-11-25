# LightLine User Guide

Everything the [README](../README.md) leaves out: every shortcut, every setting, and how to set up languages, the debugger, the AI Assistant and extensions.

- [The Welcome page](#the-welcome-page)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Settings](#settings)
- [Unsaved-work recovery](#unsaved-work-recovery)
- [Local History](#local-history)
- [Merge conflicts](#merge-conflicts)
- [Languages, formatting and debugging](#languages-formatting-and-debugging)
- [AI Assistant](#ai-assistant)
- [Extensions and themes](#extensions-and-themes)
- [Verifying a download](#verifying-a-download)

---

## The Welcome page

LightLine opens on the Welcome page when no files are restored; `Ctrl+Shift+H` brings it back at any time, and `Esc` leaves it when a folder is open.

- **Start**: New File (`Ctrl+N`), Open File (`Ctrl+O`) and Open Folder (`Ctrl+Shift+O`).
- **Recent**: the last folders you opened (LightLine remembers ten; eight show in a wide window, five in a narrow one), each with the folder it's in and how long ago it changed. Click one to open it; point at it and click **×** to take it off the list (the folder itself isn't touched). **File → Open Recent** lists the same folders.
- **Get started**: the Command Palette (`Ctrl+Shift+P`), Go to File (`Ctrl+P`), Search in Files (`Ctrl+Shift+F`), the terminal (`` Ctrl+` ``), Color Theme and Settings (`Ctrl+,`).
- **Learn**: the keyboard shortcuts, this guide, the release notes and a new issue, opened in your browser (the same links as the **Help** menu).
- **Started in … ms**: how long this start took, from launching LightLine to its first frame. It's measured once, when the window first draws, and costs nothing after that.

In a narrow window the two halves stack; in a narrow or short one, Get started drops its descriptions, and rows that don't fit are left out.

## Keyboard shortcuts

### Menu bar

The title bar has **File**, **Edit**, **Selection**, **View**, **Go**, **Run**, **Terminal** and **Help** menus, as in VS Code. Each item shows its shortcut, and does exactly what that shortcut (or its Command Palette command) does. Items that need a file open are greyed out on the Welcome screen.

- Click a title to open its menu; while one is open, moving onto another title opens that one. A click on the open title, outside the menu, or the mouse wheel closes it.
- `Alt+F`, `Alt+E`, `Alt+S`, `Alt+V`, `Alt+G`, `Alt+R`, `Alt+T` and `Alt+H` open a menu. Holding `Alt` underlines each menu's letter, and in an open menu the underlined letter of an item runs it.
- Tapping `Alt` on its own puts the keyboard on the menu titles: `Left` / `Right` move between them, `Down` or `Enter` opens one, `Esc` leaves.
- In a menu: `Up` / `Down` move, `Right` opens a submenu (or the next menu), `Left` closes it (or goes to the previous menu), `Home` / `End`, `Enter` runs, `Esc` closes.
- **File → Open Recent** lists recently opened folders; **Help** opens the User Guide, the keyboard shortcuts, the release notes or a new issue in your browser, and shows LightLine's version.
- In a narrow window the titles fold into one ☰ button next to the logo; its list opens each menu beside it.
- In the terminal, `Alt` keys still go to the shell, and in the find box `Alt+R` still switches **Regular Expression**.

### Unsaved-work recovery

LightLine checkpoints modified documents and untitled text approximately every five seconds when the session changes. Checkpoints are stored in `%APPDATA%\LightLine\session.json`; they do not save over your original files. The next launch offers **Restore** or **Discard**, including when you launch LightLine with a file argument.

If an original file changed or disappeared while LightLine was closed, its recovered text opens as an untitled buffer. Use **Save As** to choose where to keep it. An explicit **No** in a save-on-close prompt discards that document's unsaved text. Empty, untouched untitled tabs and generated previews have no recovery text.

Recovery is a periodic checkpoint: edits made after the latest completed checkpoint can be lost in a crash. Recovery write failures appear in the status bar; a failed final checkpoint keeps the window open. Recovery is stored locally as plain text.

Arrow movement, Backspace and Delete treat combining characters and joined emoji as complete characters. This does not add bidirectional layout or change the renderer's font shaping.

### Source Control

`Ctrl+Shift+G` opens Source Control for the open folder's Git repository.

- **Commit**: type a message in the box at the top and press `Enter`, or click **Commit**. If nothing is staged, LightLine offers to stage everything first. With no message and nothing to commit, the same button pushes your commits (**Push 2 commits**) or pulls new ones (**Pull 3 commits**). Its **▾** lists Commit, Push, Pull and Fetch.
- **Branch row**: the branch and how far it is ahead or behind, with **Pull**, **Push** and **Fetch** buttons at the right. They run Git in the terminal, where you see its output and can answer a password prompt. The panel updates when Git finishes.
- **··· in the header**: Commit, Refresh, Pull, Push, Fetch, Stage All Changes, Unstage All Changes and Discard All Changes, which asks first.
- **Files**: each changed file shows its name, its folder, and a letter for the change: **M** modified, **A** added, **U** untracked, **D** deleted, **R** renamed, **!** in conflict. Click a file to see its diff. Point at it (or select it with the keyboard) for **Open File**, **Discard Changes** and **Stage** or **Unstage**. Pointing at a section's title shows **Stage All**, **Unstage All** or **Discard All**.
- **History**: the latest commits, newest first, with their age; point at one to see its hash.

### Merge conflicts

When a merge, pull or rebase stops with conflicts, Git writes both versions into the file between markers:

```text
<<<<<<< HEAD
your version (current)
=======
the version being merged in (incoming)
>>>>>>> feature
```

Each `<<<<<<<` line shows three links. **Accept Current** keeps your version, **Accept Incoming** keeps the other, and **Accept Both** keeps yours followed by theirs; the markers are removed and `Ctrl+Z` undoes it. The status bar counts the conflicts left. The Command Palette has **Merge Conflict: Next Conflict** and **Merge Conflict: Accept All Current / Incoming / Both** for the whole file. When none are left, save the file and stage it in Source Control to finish the merge. A diff3 base section (`||||||| base`) is dropped whichever side you keep.

### Local History

Each time you save a file, LightLine keeps a copy of it, so you can go back to an earlier version even if you never committed it: after an accidental **Replace all**, a formatter that mangled the file, or an edit you regret. The first save of a file also keeps the file as it was before, so the version you opened is never lost.

1. Run **Local History: Show File History** from the Command Palette (`Ctrl+Shift+P`). The saved versions of the active file are listed, newest first, with when each was saved and its size.
2. Pick one and press `Enter`. It appears beside the file as it is now, with the changed lines colored. `F7` and `Shift+F7` jump between changes; click a line to go to it in the file.
3. Click **Restore** to put that version in the editor. It's an ordinary edit: `Ctrl+Z` undoes it, and nothing is written until you save. **Close** or `Esc` leaves without changing anything.

Versions are kept in `%APPDATA%\LightLine\history`, never next to your files: the last 50 versions of each file up to 1 MB. Saving a file without changing it adds nothing. Turn it off with `"localHistory": false`; versions already kept stay until you delete that folder.

### Files and workspace

| Shortcut | Action |
| --- | --- |
| `Ctrl+P` | Quick Open files; type `>` for the Command Palette |
| `Ctrl+N` / `Ctrl+W` | New tab / close the active tab |
| `Ctrl+O` / `Ctrl+S` | Open a file / save the active file |
| `Ctrl+Shift+O` | Open a folder as the workspace |
| `Ctrl+Shift+W` | Close the workspace and return to the Welcome screen (also `>Close Workspace`) |
| `F2` / `Delete` | In the Explorer: rename / delete the selected file or folder |

Find (`Ctrl+F`) and **Search** (`Ctrl+Shift+F`) have three switches at the right of the text: `Aa` **Match Case** (on to start with), `ab` **Whole Word** and `.*` **Regular Expression**. With `.*` on, the text is a [regular expression](https://docs.rs/regex/latest/regex/#syntax), such as `fn \w+_test` or `^\s*//`, and a replacement can use what it captured: find `(\w+)@(\w+)` and replace with `$2: $1`, or `${name}` for a named group. A pattern matches within a line, never across a line break. While the pattern isn't valid, the find box says **Invalid regex** and nothing is searched or replaced. The switches stay as set until LightLine closes.

Quick Open opens under the search box in the middle of the title bar, and you type in that box. The buttons along its bottom switch between files, **`>` Commands**, **`@` Symbols**, **`#` Workspace** symbols and **`:` Line**; click the lit one again to go back to files. Commands show their shortcut keys on the right.

Quick Open and **Search** (`Ctrl+Shift+F`) leave out what Git ignores: the patterns in each folder's `.gitignore`, the repository's `.git/info/exclude` and your own excludes file (`~/.config/git/ignore`, or `core.excludesFile`), as well as `.git`, `target`, `node_modules`, `.venv` and `__pycache__`. Build output, logs and downloaded packages don't crowd the results. The Explorer still shows every file.

### Editing and navigation

| Shortcut | Action |
| --- | --- |
| `Ctrl+F` | Find in the current file. `Enter` / `Shift+Enter` (or `F3` / `Shift+F3`) go to the next / previous match; `Esc` closes |
| `Ctrl+H` | Find and replace. `Tab` switches fields, `Enter` replaces the current match, `Alt+Enter` replaces all (one `Ctrl+Z` undoes it) |
| `Ctrl+Shift+F` | Search all files in the workspace; `Enter` moves to the results. `Tab` goes to **Replace with**; `Enter` there (or **Replace all**) replaces every occurrence in the workspace after asking. Open files undo with `Ctrl+Z`; files that aren't open are saved |
| `Alt+C` / `Alt+W` / `Alt+R` | In the find box or the Search panel's query: **Match Case**, **Whole Word**, **Regular Expression** (the `Aa`, `ab` and `.*` buttons at the right of the field) |
| `Ctrl+G` (or `Ctrl+P`, then `:`) | Go to a line: `42`, or `42:7` for its 7th character |
| `Ctrl+Space` | Show completions. A function goes in with its arguments as places to fill: type over the selected one, `Tab` / `Shift+Tab` move between them |
| `Tab` / `Shift+Tab` | Indent (spaces to the next tab stop) / outdent; with several lines selected, indent or outdent them all |
| `F1` | Show documentation for the symbol at the cursor |
| `F12` / `Shift+F12` | Go to definition / find all references |
| `F2` | Rename the symbol at the cursor everywhere it's used (see [Rename Symbol](#rename-symbol)) |
| `Ctrl+.` | Quick fixes and refactorings at the cursor or selection (see [Quick Fixes](#quick-fixes-and-refactorings)) |
| `Ctrl+P`, then `@` | Go to a function, type or other symbol in this file; type to filter, `Enter` jumps |
| `Ctrl+T` (or `Ctrl+P`, then `#`) | Search the whole project's symbols by name; `Enter` opens the file at the symbol |
| `Ctrl+Shift+M` | Show or hide the **Problems** panel: every error and warning in the open files; click one to jump to it (a click on the counts in the status bar opens it too) |
| `Ctrl+Shift+Space` | Show the parameters of the call at the cursor (they also show as you type `(` and `,`) |
| `Shift+Alt+O` | Organize imports (**Source Action...** in the Command Palette lists the other whole-file actions) |
| `Alt+Click` | Add a cursor there, or remove one (see [Multiple cursors](#multiple-cursors)) |
| `Ctrl+Alt+Up` / `Ctrl+Alt+Down` | Add a cursor on the line above / below |
| `Ctrl+D` | Select the word at the cursor; again, add the next match of the selection |
| `Shift+Alt+F` | Format the document |
| `Alt+Z` | Toggle word wrap for this file (on by default for Markdown) |
| `Ctrl+/` | Comment out the caret's line or the selected lines, or uncomment them (also **Toggle Line Comment** in the Command Palette), with the marker the file's language uses: `// `, `# `, `-- `, or `<!-- -->` around the lines |
| `Shift+Wheel` | Scroll sideways when a line is wider than the editor and word wrap is off; a tilt wheel or a sideways touchpad swipe does the same. The view also follows the cursor as you type or move it |
| `Ctrl+Shift+V` | Preview a Markdown file (`>Markdown: Open Preview to the Side` shows it beside the file) |
| Gutter chevron | Fold or unfold a block (`⌄` / `›`) |
| Minimap | Click to jump there; drag the shaded part (the lines on screen) to scroll |
| **▶ Run** / **Debug** / **▶ Run test** | Links after `main` and test functions: run the file, debug it (Rust, Python), or run just that test in the Output panel |

#### Multiple cursors

Add cursors with `Alt+Click`, `Ctrl+Alt+Up` / `Ctrl+Alt+Down` or `Ctrl+D`. Typing, `Enter`, `Tab`, `Backspace`, `Delete` (with `Ctrl` for whole words), the arrow keys, `Home` and `End` (with `Shift` to select) then act at every cursor, and one `Ctrl+Z` undoes an edit at all of them. `Ctrl+C` copies each cursor's selection on its own line; pasting as many lines as there are cursors puts one line at each, otherwise the whole text goes at every cursor. `Esc` or a click goes back to one cursor; so does any other edit, such as formatting or undo.

### Layout

| Shortcut | Action |
| --- | --- |
| `Ctrl+B` | Show or hide the side panel (also the ☰ at the top of the left rail) |
| `Ctrl+\` | Split the editor / close the split |
| `Ctrl+1` / `Ctrl+2` | Focus the left / right pane |
| `` Ctrl+` `` / `` Ctrl+Shift+` `` | Show or hide the terminal / open a new terminal |
| `Ctrl+Shift+G` | Source Control |
| `Ctrl+Shift+D` | Run & Debug |
| `Ctrl+Shift+Y` | Show or hide the **Debug Console**: the debugger's messages, and a prompt to evaluate expressions while paused |
| `Ctrl+Shift+X` | Extensions |
| `Ctrl+,` | Settings |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | Zoom in / out / reset |

The bottom panel has four tabs: **Problems** (with its count), **Output**, **Debug Console** and **Terminal**. With two shells or more, a list at the side of the terminal shows them all, each with its shell's icon: click one to open it, point at it for 🗑 to close it, right-click it to rename it. The toolbar at the right has **+** (a new terminal with the default profile), **▾** (pick PowerShell, Command Prompt, Git Bash or WSL, or the default), 🗑 (close the open shell, or **Stop** a running program in Output), **^** (make the panel as tall as it goes; again to go back) and ✕ (hide the panel; the shells keep running). When a shell exits, the terminal says so; press `Enter` to close it.

Prompts made with oh-my-posh, Starship or Powerlevel draw properly: LightLine draws their arrows itself, and their icons (git branch, folder, language logos) come from a [Nerd Font](https://www.nerdfonts.com/) if one is installed. Set `terminalSymbolFont` to choose which.

### Run and debug

| Shortcut | Action |
| --- | --- |
| `Ctrl+Shift+R` | Run the active Python, C/C++, JavaScript, TypeScript, Go or Rust file/project |
| `Ctrl+Shift+B` | Run Rust tests (`cargo test`) |
| `F5` / `Shift+F5` | Start or continue debugging / stop |
| `F10` | Step over |
| `F11` / `Shift+F11` | Step into / step out |
| `F9` or a click in the gutter | Toggle a breakpoint |

The **Run and Debug** panel (`Ctrl+Shift+D`) starts a session with its green **▶**; the picker beside it chooses **Automatic** (follows the open file), **Rust: Current Workspace** or **Python: Current File**. While you debug, the same bar holds Continue/Pause, Step Over, Step Into, Step Out, Restart and Stop, and the status bar turns amber. Below it:

- **Variables**: `name: value` for each scope; click an arrow to expand a struct or list.
- **Call Stack**: every frame, the paused one marked with the yellow arrow; click a frame to open its line.
- **Breakpoints**: those in open files; click one to go to it, point at it for **✕** to remove it, or point at the title to remove them all.

The **···** menu has Start Debugging, Select Configuration, Debug Console, Python: Install debugpy and Remove All Breakpoints.

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

**A file Run doesn't recognize**, such as a script without an extension: click the language name in the status bar (or use **Run: Choose the File's Language** in the Command Palette) and pick Python, C, C++, JavaScript, TypeScript, Go or Rust. Run then treats the file as that language, and the choice is remembered for that file, also after you close it or restart LightLine (in `%APPDATA%\LightLine\run-languages.json`). **Auto Detect** goes back to what the file's name says. The same list has the languages of installed [language extensions](#extensions): choosing one colors and edits the file as that language instead.

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

- **Search settings**: type in the field at the top to show only the settings whose name (or `settings.json` key) holds what you typed. `Esc` clears it.
- **Changed settings** have a violet bar at their left. Point at one and click the arrow beside its control to put it back to the default.
- **Point at any setting** to see what it does and its key in `settings.json`.

To edit the file directly, click the **{ }** button in the panel's header, **Open settings.json** at the bottom of the panel, or run **`>Open Settings (JSON)`**. Anything you leave out keeps its default:

```json
{
  "fontFamily": "Consolas",
  "fontSize": 14,
  "tabSize": 4,
  "insertSpaces": true,
  "wordWrap": false,
  "autoClosePairs": true,
  "autoIndent": true,
  "linkedTagRename": true,
  "pathCompletion": true,
  "formatOnSave": false,
  "inlayHints": true,
  "occurrencesHighlight": true,
  "stickyScroll": true,
  "bracketMatching": true,
  "indentGuides": true,
  "bracketPairColorization": true,
  "showMinimap": false,
  "gitBlame": true,
  "codeLens": true,
  "todoHighlight": true,
  "inlineDiagnostics": "errorsAndWarnings",
  "localHistory": true,
  "editorConfig": true,
  "animations": true,
  "smoothScrolling": false,
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
| `linkedTagRename` | With the caret in a tag's name in HTML, XML, JSX/TSX, Vue or Svelte, both the opening and closing names are outlined, and typing or deleting in one changes the other too; one Ctrl+Z undoes both (on by default). A space or any other character ends the link; self-closing tags and names that already differ are left alone |
| `pathCompletion` | Typing `./` or `../` in a string lists the files and folders there, with their icons, narrowing as you type (the letters typed are highlighted, and the list's last line says which folder it is and how many entries match), leaving out what `.gitignore` ignores and `node_modules`, `target`, `.venv` and `__pycache__`; Enter or Tab inserts one, and a folder lists what's inside next (on by default). Ctrl+Space in such a path lists them too, with or without a language server |
| `bracketMatching`, `indentGuides` | Highlight matching brackets; draw indentation guides (the current block's is brighter) |
| `bracketPairColorization` | Color nested brackets in turn with three colors (not those in strings or comments) |
| `showMinimap` | Show the file in miniature at the right of the editor (off by default; it costs some typing speed) |
| `gitBlame` | After the caret's line, show who last changed it, when, and the commit message |
| `codeLens` | Show **▶ Run** / **Debug** after `main` and **▶ Run test** after test functions (Rust, Python, Go, C/C++) |
| `inlineDiagnostics` | Write each line's worst problem after it, from its language server: `"errorsAndWarnings"` (the default), `"errors"` or `"off"`. **+1** means the line has more; the full list is in Problems. Nothing is written after a line that fills the window |
| `todoHighlight` | Colour `TODO`, `FIXME`, `HACK` and `NOTE` inside comments, each in its own colour on a faint tint, with a mark at their place on the scrollbar (on by default). **Find TODOs in Workspace** (Command Palette, or Edit menu) lists the ones in comments across the whole folder with Search in Files |
| `localHistory` | Keep a copy of a file each time it's saved, for [Local History](#local-history) |
| `animations` | Ease the side panel and the bottom panel open and closed, and fade in the button under the mouse. Short, and never while you type or while LightLine is idle. Windows' **Show animations** (Settings → Accessibility → Visual effects) turns it off too |
| `smoothScrolling` | A mouse-wheel notch or Page Up/Down glides into place instead of jumping (off by default: a glide draws three frames where a jump draws one). Needs `animations` |
| `formatOnSave` | Format the file before each save |
| `editorConfig` | Follow the project's `.editorconfig` files (on by default): `indent_style`, `indent_size` and `tab_width` set what Tab inserts and the indent guides; at each save, `trim_trailing_whitespace`, `insert_final_newline` (`true`) and `end_of_line` (`lf` or `crlf`) are applied, as one step Ctrl+Z undoes. The status bar then says **Spaces: 2 · .editorconfig**; click it to see the rules and open the file. Tab characters are drawn `tab_width` columns wide |
| `inlayHints` | Show the language server's type and parameter-name hints inside the code (`let total: i32`) |
| `occurrencesHighlight` | Highlight the other uses of the name at the caret |
| `stickyScroll` | Pin the first lines of the blocks you're scrolled into (function, loop...) at the top of the editor |
| `markdownLoadRemoteImages` | Load web images in Markdown previews without asking |
| `colorTheme` | An installed color theme, such as `"Catppuccin Mocha"`; leave it out for LightLine's own. **Preferences: Color Theme** sets it for you |
| `aiEndpoint`, `aiModel` | Where the AI Assistant connects, and which model it uses |
| `aiProjectAccess` | Whether the AI Assistant's model may search and read the open folder: `"ask"` (once per chat, the default), `"always"` or `"off"` |
| `aiKeepChats` | The AI Assistant keeps earlier chats on this PC, the last 50 per folder, in `%APPDATA%\LightLine\chats` (on by default; **Keep earlier chats** in Settings) |
| `aiIncludeOpenFile` | The AI Assistant offers the file you're in as a chip in the question box, sent with the question unless you take it off with its **✕** (on by default; **Include the open file** in Settings) |
| `languageServers` | Commands for the language servers of [language extensions](#extensions), by the server's name or the language's, such as `{"lua-language-server": {"command": "C:\\tools\\lua-ls\\bin\\lua-language-server.exe", "args": []}, "Gleam": "gleam lsp"}`. A string is split at spaces; use `command` and `args` when a path has spaces. Used the next time a file of that language is opened |
| `terminalSymbolFont` | The font for the symbols in terminal prompts (a Nerd Font, such as `"CaskaydiaCove Nerd Font Mono"`); leave it out to use the first Nerd Font installed. The terminal's text keeps the editor's font |
| `colors` | Override individual theme colors, on top of the color theme |

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
3. Type a question and press **Enter** (**Shift+Enter** for a new line). What goes with it shows as chips at the top of the question box: selected code, such as *stats.rs · lines 2–4*, or else the file you're in (a quieter chip, sent unless you take it off with its **✕**; the `aiIncludeOpenFile` setting turns this off). A chip you take off stays off until you send. The box grows to 8 lines. The question box edits like a text field: click to put the caret anywhere, **Left**/**Right** (with **Ctrl**, a word at a time), **Home**/**End** (with **Ctrl**, the whole text), **Up**/**Down** between lines, **Delete** and **Backspace** (with **Ctrl**, a word).

Answers appear as they're written. **Esc** stops one, and **+** at the top starts a new chat; the header shows the chat's first question. Under the question box, **@** attaches a file, and the model's name (Claude's models by their names, such as *Claude Opus 5.5*) opens the model menu, to switch models, change provider or turn the assistant off. A new chat offers **Explain this file**, **Fix problems in the selection** and **Write tests for the selection**. Drag the panel's left edge to make it wider or narrower (300 px up to half the window); LightLine remembers the width. Under a finished answer, the copy icon copies it (it turns into a green tick for a moment), the circular arrow (on the last answer) asks the same question again for a new answer, and the line beside them says how many steps it took and how long.

**Attaching files:** type `@` in the question box and a list of the project's files appears, matched as you type (names that start with what you typed come first). **Enter** or **Tab** attaches the chosen file, **Up**/**Down** choose, and **Esc** closes the list. **Add to AI Chat** on a file in the Explorer, or **Add file to AI chat** in the editor's right-click menu, attaches that file and opens the panel. Each attached file shows as a chip above the box, so you see what will be sent before you press Enter; **✕** on a chip, or **Backspace** in an empty box, takes it off. On Enter the files go with the question, each cut like selected code (400 lines or 24 KB) and 96 KB in all; the chips stay on the question in the chat and say how much was sent, such as *ai.rs · 1,320 lines, cut to 400*. An open file is sent as it is in the editor, unsaved edits included. Files the `.gitignore` hides aren't in the `@` list, but you can still attach one from the Explorer. A binary file can't be attached, and a file that looks like it holds secrets (`.env`, `*.pem`, `*.key`, `id_rsa`, `credentials`…) shows amber and asks before it's sent; **Don't Send**, the default, keeps your question.

**Letting the AI look through the project:** for a question like *where is this used?*, the model can ask LightLine to search the project, list its files, read a file (or some of its lines) and see the errors and warnings language servers report for open files. It only reads, and only inside the open folder: never files outside it, in `.git`, hidden by `.gitignore` (such as `node_modules` or `target`), or that look like secrets. The first time in a chat it asks, *Let Claude look through my-project?*, and says where what it reads goes (to the service, or that it stays on this PC with Ollama): **Allow in This Chat**, **Just Once** or **Don't Allow**. Each step then shows as a line in the answer, where it happened, such as *Read src/ai.rs 640–700* with *61 lines* on the right; the step running now reads like *Reading src/ai.rs…*. Click a Read line to open the file there. Once the answer is done, two or more steps in a row fold into one line, such as *Looked at 3 places · 2 searches, 1 file*; click it to see them again. A question takes at most 25 steps, a read at most 400 lines, and **Esc** stops it at any point. Open files are read as they are in the editor, unsaved edits included. This needs a model that can use tools: Claude, OpenAI's models and the other services' tool models do; with Ollama, models such as qwen2.5-coder and llama3.1 do. A model that can't answers without looking, and the panel says so once. **Project access** under AI Assistant in Settings (`aiProjectAccess`) chooses whether it asks per chat (the default), always may, or never does.

**Earlier chats:** each chat is kept on this PC when an answer finishes, the last 50 per folder (never uploaded or synced). The clock at the top of the panel, or **AI: Show Earlier Chats**, lists the folder's chats, newest first, under **TODAY**, **YESTERDAY** and **EARLIER**. Type to filter them by title, **Up**/**Down** and **Enter** (or a click) open one, and **Delete** (or the **✕** on the chosen row) deletes it. A reopened chat goes on where it ended; its proposed changes aren't kept, and the model asks again before looking through the project. **AI: Delete All Chats** deletes every kept chat, for every folder, after asking. **Keep earlier chats** in Settings (`aiKeepChats`) turns keeping them off.

**Proposed changes:** with project access allowed, the model can also propose changes: an edit to a file (it gives the exact text to replace) or a new file. Proposing changes nothing. The proposals collect in a **Proposed changes** card under the answer, one row per file with the lines added and removed. **Review** (or clicking a file's row) shows the change in the diff view, the file as it is now on the left and the proposal on the right, with **Apply**, **Skip** and **Next ›**; **Apply All** applies every file, and **Discard** drops them. Applied changes go into the editor and **aren't saved**: each file changes as one step that **Ctrl+Z** undoes, and **Ctrl+S** saves it as usual (Local History then keeps a copy). A new file opens in a tab of its own, unsaved, at the path the model gave (its folder is created if missing). If a file changed after the model proposed something and the proposal no longer fits, Apply says so and leaves the file alone. The model can't delete or rename files, run commands, or save anything.

Long lines in an answer's code wrap, each continued row starting a little past the line's own indent. Each code block has, at the top right:

- the arrow-onto-a-line icon, which inserts the code at the cursor;
- the copy icon, which copies it;
- **Apply**, which puts the code in place of the code you asked about (if that code hasn't changed since), else of the selection, else at the cursor. When the answer has a **Proposed changes** card, Apply isn't shown on its code blocks: the card is the one way to apply those changes.

Inserted code is indented to fit where it lands, and one **Ctrl+Z** undoes it.

**From the editor:** right-click selected code for **Explain**, **Fix**, **Write Tests** or **Add Comments**. Right-click a red or yellow underline for **Explain This Error** or **Fix This Error**. These are also in the Command Palette under `>AI:`.

- **Choosing a provider**: open **AI: Choose Provider...** from the Command Palette, **Change provider...** in the panel or at the bottom of the model menu, or click **Server** under AI Assistant in Settings. Pick **Ollama**, **OpenAI**, **Claude**, **OpenRouter**, **Groq**, or **Other** for any OpenAI-compatible server (type its address, such as LM Studio's `http://localhost:1234`). LightLine checks the connection, lists the models in **Model** (only ones that can chat) and picks a small, fast one (for Claude, Claude Opus 5.5); choose another if you like, then click **Save & Connect** (`Ctrl+Enter`).
- **Cloud services with your own API key**: after choosing OpenAI, Claude, OpenRouter or Groq, click **Get a key** to open the service's key page, then paste the key into **API key** (`Ctrl+V`). It shows only as dots and is checked at once. On **Save & Connect** it's saved in Windows Credential Manager under the service's name (never in `settings.json`), and it's only ever sent to that service. With a key already saved, the card says so and offers **Remove saved key**. If no key is saved, LightLine uses the `OPENAI_API_KEY` environment variable (`ANTHROPIC_API_KEY` for Claude). **AI: Set API Key...** opens the same card with the keyboard in the key field, and **AI: Remove API Key** forgets the key. The panel says which service answers, because your questions and the code you include are sent there. A rejected key or a used-up quota gets a plain message.
- **Claude**: LightLine talks to Claude through Anthropic's own API, not a compatibility layer. Get a key at platform.claude.com. Chat answers use Claude's medium effort setting, and inline completions (`Alt+\`) use low effort so they come back quickly; Claude thinks before it answers, so the first words can take a few seconds. If Claude's safety checks decline a question, another Claude model is asked where Anthropic offers one, and the panel says so if every one declines.
- **Cloud models**: Ollama models whose names end in `cloud` run on ollama.com, not your PC. LightLine never picks one for you, and warns you if you do.

---

## Extensions and themes

The Extensions panel (`Ctrl+Shift+X`) installs extensions from the [Zed extension registry](https://github.com/zed-industries/extensions) into `%APPDATA%\LightLine\extensions`. Five kinds work today.

**LightLine's list.** Extensions tested with LightLine are on its own list, [`registry/extensions.toml`](../registry/extensions.toml) in LightLine's repository, and carry a **LightLine** badge. Their name, kind and a description are shown before you install them, where an extension that isn't on the list says *Install to see whether LightLine supports it yet*. A copy of the list is built into LightLine, so it works offline. When the panel opens, LightLine also reads the latest list from GitHub, next to the Zed registry's, so newly tested extensions appear without updating LightLine. Anyone can propose an extension for the list with a pull request; [CONTRIBUTING.md](../CONTRIBUTING.md#proposing-an-extension-for-lightlines-list) says how.

**LightLine's own extensions** are made for LightLine and kept in one repository, [lightline-extensions](https://github.com/mehmoodulhaq570/lightline-extensions), each in its own folder:
- **LightLine Themes**: the **LightLine Daylight** (light) and **LightLine Contrast** (high contrast) color themes, made to match LightLine Midnight.
- **C and C++ Snippets**.

Installing one downloads only its folder, at the commit LightLine's list pins. The same goes for a Zed registry extension that is a folder of a larger repository, such as Zed's own `html`.

The panel lists **Installed** extensions, then **Recommended** ones from LightLine's list (LightLine Themes, C and C++ Snippets, Prettier, Material Icon Theme, Dracula, Catppuccin, One Dark Pro, typos, Python Snippets and React Snippets), then what's **In Use**: the formatter, the file icons and the color theme (click it to change the theme). The **All**, **Themes**, **Snippets** and **Checkers** chips filter the lists. **All** shows the recommended extensions; the others show every extension of that kind on LightLine's list. Typing in the search box searches LightLine's list and the whole Zed registry: matches on LightLine's list come first, under **LightLine list**, and the rest follow under **Zed registry**. Each extension has one button: **Install**; a green ✓ once installed, which becomes **Uninstall** when you point at it (**Turn Off** for Prettier); or **Update** when a newer version is out. Click a section's title to fold it. The **···** menu has Show Installed, Show Recommended, Check for Updates, Update All (when there are updates) and Open Extensions Folder.

**Updates.** When LightLine's list or the Zed registry has a newer version of an installed extension, its row shows the version you have and the new one (`0.9.0 → 1.0.0`) with an **Update** button. The **INSTALLED** header counts the updates, and with two or more its **Update all** takes them all, one after another. Only a new version number counts. After an update, the color theme in use is applied again in its new colors (if its variant is gone, LightLine Midnight is used and the status bar says so), icons and languages reload, snippets are read again and checkers restart. A language extension whose grammar is in use can't be replaced while LightLine runs: it keeps working as it was, and the status bar says to restart LightLine, then update it. LightLine looks for updates only when you open Extensions or choose **Check for Updates** (or the ⟳ in the panel's header), which downloads both lists again and says what it found; nothing is checked at startup or in the background. Installing or updating a Zed registry extension looks it up through GitHub's API, which allows 60 lookups an hour from one network; if they're used up, the status bar says how many minutes until you can try again. LightLine's own extensions and the lists don't count against it.

Zed extensions have no icons of their own, so LightLine makes one: an installed color theme shows a swatch of its colors, an icon theme its own folder icon, and a language its file icon. An extension that isn't installed, or a snippet pack, shows the first letters of its name on a color that's always the same for it.

Updates download and validate a staged copy before replacing an installed extension. A failed download, invalid manifest, malformed theme, missing icon asset or a grammar that doesn't build leaves the existing installation intact. If replacement fails, LightLine attempts to restore the previous copy; any backup it cannot restore is retained and its location is reported.

- **Color themes** recolor all of LightLine, dark or light, including the terminal. Installing one, such as **Dracula**, switches to it at once and it's remembered. Themes with several variants (Catppuccin Latte, Frappé, Macchiato, Mocha) offer each in **Preferences: Color Theme**. For a light theme made for LightLine, install **LightLine Themes** and pick **LightLine Daylight**; **LightLine Contrast** is its high-contrast sibling.
- **Icon themes** change file and folder icons; **Material Icon Theme** is the popular one. Without one, LightLine uses its built-in icons.
- **Snippet packs** add snippets for a language or a framework, such as **python-snippets**, **react-snippets** or **fastapi-snippets** (search the registry for "snippets"; it has about 50), or LightLine's own **C and C++ Snippets** (`main`, `for`, `struct`, `class`, include guards, and `malloc` and `fopen` with their error checks). Type the start of a snippet's prefix and press `Ctrl+Space`: the snippets whose prefix starts with what you typed are listed first, marked **~** with their description, ahead of the language server's suggestions (or alone in a file with no language server). `Enter` or `Tab` inserts one, and `Tab` then moves through its placeholders, as with a language server's snippets. A pack's file for a language, such as its `python.json`, is used in files of that language, including languages added by a language extension; a `snippets.json` file is used in every language. The status bar says how many snippets a pack added and for which languages. The snippets are read the first time you press `Ctrl+Space` in a file of that language, never at startup or while you type.
- **Languages** add languages LightLine doesn't know itself, such as Lua, Zig, TOML or Elixir. Installing one downloads the language's Tree-sitter grammar and builds it with the C compiler on your PC: the Visual Studio C++ Build Tools (found automatically, as Rust uses them), or `gcc` or `clang` on PATH. Without one, the install stops and says so. Building takes a few seconds; files of that language that are open change as soon as it finishes. The grammar is native code loaded into LightLine, like a grammar in Helix or Neovim, so install language extensions you trust. After removing one, restart LightLine to free its grammar file if a file of that language was open. From the extension, files of that language get:
  - **Colors** from its highlight rules.
  - **Comments**: `Ctrl+/` comments lines out with its comment marker (`-- ` for Lua, `# ` for TOML).
  - **Brackets and quotes**: the pairs it lists are closed as you type (before a space, the end of the line or one of the characters it lists, and not inside a string or comment when it says so), and are the ones matched and colored.
  - **Indentation**: Enter indents after its block openers (`{`, or `function` and `if ... then` in Lua), and typing `end` or `else` moves that line back out. Its own indent width, such as two spaces for Elixir, is used for what you type and for the indent guides.
  - **A place in the language picker**: click the language name in the status bar (or **Run: Choose the File's Language**) to color and edit a file with another name, such as `notes.txt` or a script without an extension, as one of these languages. The choice is remembered for that file; **Auto Detect** goes back to what its name says.
  - **Its language server, when you have it.** Zed extensions download their servers with code LightLine doesn't run, so LightLine starts a server only when its command is already on your PATH. It knows the commands of about 40 servers by the name the extension gives them, including `lua-language-server`, `zls`, `taplo`, `elixir-ls`, `bash-language-server` and `yaml-language-server`. If the command isn't found, the status bar says which one it looked for; nothing is downloaded. The `languageServers` setting names the command for any other server, or for a language whose extension names none. The server then works like the built-in ones: errors as you type, completions, hover, go to definition, rename and the rest, as far as it supports them.

- **Checkers** are extensions whose only part is a language server that checks files of many languages: the spell checkers **typos**, **codebook**, **harper** and **cspell**. A checker runs beside a file's own language server, or on its own in a file that has none, such as `notes.txt` or a Markdown file. Its problems are underlined and listed in Problems with the language server's; errors and warnings are also written after their line, while information (how typos reports misspellings) is only underlined. `Ctrl+.` on one of its problems offers its fixes, such as correcting `recieve` to `receive`. Like a language's server, LightLine starts a checker only when its command is on your PATH and downloads nothing:

  | Extension | Program | LightLine can download it | Or get it yourself |
  | --- | --- | --- | --- |
  | typos | `typos-lsp` | Yes, 0.1.58 (about 4 MB) | `typos-lsp.exe` from github.com/tekumara/typos-lsp/releases, or `cargo install typos-lsp` |
  | codebook | `codebook-lsp serve` | Yes, 0.4.0 (about 9 MB) | `codebook-lsp.exe` from github.com/blopker/codebook/releases, or `cargo install codebook-lsp` |
  | harper | `harper-ls --stdio` | Yes, 3.2.0 (about 14 MB; x64 only, no ARM64 build is published) | `harper-ls.exe` from github.com/Automattic/harper/releases, or `cargo install harper-ls --locked` |
  | cspell | `cspell-lsp --stdio` | No: it needs Node.js | `npm install -g @vlabo/cspell-lsp` |

  **Downloading it for you.** The first time you open a file a checker checks and its program isn't installed, LightLine asks: *Download typos-lsp 0.1.58 now?* Choose **Download** and it fetches that exact release from the program's official GitHub page, checks the file against the SHA-256 LightLine expects (a different file is refused before it's unpacked), and keeps just the `.exe` in `%APPDATA%\LightLine\tools\`. Nothing else on your PC changes, and nothing is downloaded unless you choose Download. The checker then starts in your open files. Uninstalling the extension deletes the program too. **Not Now** shows where to get it instead, and isn't asked again until LightLine restarts.

  **Getting it yourself**, without Cargo: download the zip for your PC (x86_64 or aarch64, `windows-msvc`) from the release page, unzip it, and either put the `.exe` in a folder on your PATH, or tell LightLine where it is in `settings.json`, for example `"languageServers": {"typos": "C:\\tools\\typos-lsp.exe"}`. Then reopen the file. A program on PATH or named there is used before one LightLine downloaded.

  A checker starts the first time a file it checks is shown, never at startup, and each keystroke only sends it the change.

Other kinds of extensions, such as debuggers or slash commands for AI, say they aren't supported yet rather than half-installing.

---

## Verifying a download

Each release includes `SHA256SUMS.txt`. In PowerShell, in the folder you downloaded to, run:

```powershell
Get-FileHash .\lightline.exe -Algorithm SHA256
```

The result must match the `lightline.exe` line in `SHA256SUMS.txt` exactly. For the ARM64 build or a ZIP, run it with that file's name instead. If they differ, delete the download.

LightLine isn't code-signed yet, so the first time you run it, Windows SmartScreen may say **"Windows protected your PC"**. If the checksum matches, click **More info**, check that the app is `lightline.exe`, and click **Run anyway**. Don't turn SmartScreen off, and don't continue if Windows reports malware rather than an unrecognized app.
