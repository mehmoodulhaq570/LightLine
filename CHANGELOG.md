# Changelog

All notable changes to **LightLine** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

#### Changed
- **Debugger Uses No CPU While Paused**: During a Rust or Python debug session, the thread talking to the debug adapter woke about 30 times a second to check for work, even while stopped at a breakpoint. It now sleeps until the editor or the adapter has something for it: paused at a breakpoint, 95 wakeups in 3 seconds went to none.

## [v0.3.1] - 2026-09-29

Released as `lightline.exe` (x64), `lightline-arm64.exe`, `lightline-v0.3.1-windows-x86_64.zip`, `lightline-v0.3.1-windows-arm64.zip` and `SHA256SUMS.txt`. Still unsigned, like v0.3.0: check the file against `SHA256SUMS.txt`, then choose **More info** → **Run anyway** if SmartScreen warns.

#### Added
- **Windows on ARM**: Each release now includes a native ARM64 build (`lightline-arm64.exe` and `lightline-vX.Y.Z-windows-arm64.zip`) for Snapdragon and other Windows on ARM PCs, which previously ran the x64 build under emulation. It's built and tested on GitHub's Windows 11 ARM machines, and the everyday Rust check now builds and tests ARM64 on every push too.
- **Install with Scoop**: `scoop bucket add lightline https://github.com/mehmoodulhaq570/LightLine`, then `scoop install lightline`. Scoop adds LightLine to the Start menu and to the command line, and `scoop update lightline` gets new versions: each release updates the bucket's manifest automatically, for both x64 and ARM64.

#### Changed
- **Release checksums cover everything**: `SHA256SUMS.txt` now lists both architectures' executables and zips. The release tag is now placed on the exact commit that was built and tested.

## [v0.3.0] - 2026-09-29

Released as `lightline.exe`, `lightline-v0.3.0-windows-x86_64.zip` and `SHA256SUMS.txt`. Like earlier releases it is not code-signed (SignPath signing stays off until the project is approved), so Windows SmartScreen may warn on first run: check the file against `SHA256SUMS.txt`, then choose **More info** → **Run anyway**.

#### Added
- **Markdown Preview** ([#45](https://github.com/mehmoodulhaq570/LightLine/issues/45)): `.md` files and READMEs render inside LightLine. Run **Markdown: Open Preview** (`Ctrl+Shift+V`) for a preview tab, or **Markdown: Open Preview to the Side** to see it next to the file as you type. A Markdown file also gets an **Open Preview to the Side** button at the right end of the tab bar, where code files show their Run button; it is highlighted while the preview is open, and clicking it again closes the preview.
  - Headings, paragraphs, bold/italic/strikethrough, inline code, links, numbered and nested lists, task lists, block quotes, code blocks, tables (with column alignment), rules and images are shown.
  - Web links open in your browser, links to other files in the workspace open in LightLine, and `#heading` links scroll to the heading. Links that point outside the workspace or use other schemes (`javascript:`, `file:`) are refused.
  - Local images are shown, including SVG. Images in a line of text, such as README badges, sit side by side in that line and keep their links.
  - Web images load only when you allow it: a bar above the preview offers **Load images**, and the choice is remembered for that workspace (or set `"markdownLoadRemoteImages": true` to always allow). Downloads run in the background with a time and size limit and are cached in `%APPDATA%\LightLine\cache\images`, so each image is fetched once.
  - HTML in the file is never run: `<img>`, headings, text and line breaks are shown, and scripts, styles and embedded content are dropped.
  - The preview only reads the file and updates shortly after you stop typing; unsaved changes are never touched.
- **Markdown Syntax Colors**: Markdown files are colored in the editor, as in VS Code: headings, **bold**, *italic*, `code` and fenced code blocks, link text and URLs, list, task and quote markers, tables and rules. The colors come from the active theme, so color themes such as Dracula restyle Markdown too.
- **Word Wrap**: Long lines continue on the next screen row instead of running off the edge. It is on by default for Markdown, `"wordWrap": true` in `settings.json` turns it on for every file (the setting was previously ignored), and **Alt+Z** or **View: Toggle Word Wrap** switches it for the current file.
  - Rows after the first keep the line's indent, so wrapped list items stay aligned; line numbers, breakpoints and folds stay on each line's first row.
  - Up/Down and Page Up/Down move by screen row and keep the caret's horizontal position, and the mouse wheel scrolls by rows. Clicking, selecting, search results, error underlines and bracket matching all follow the wrapped rows.
  - Only lines on screen are wrapped, so large files stay fast.
- **Color Themes Recolor All of LightLine**: An installed color theme used to change only the editor text and a few surfaces, while the sidebar, tabs, buttons, menus, dialogs, the Extensions panel and the terminal kept LightLine's midnight blue. Now every part of the window follows the theme, dark or light (e.g. Dracula Light, Catppuccin Latte), including the terminal's 16 colors and the window frame. Colors a theme doesn't define are worked out from its own palette, so they still match.
  - **Preferences: Color Theme** (command palette, or the Color Theme row under Active Capabilities in the Extensions panel) lists LightLine's own theme and every installed theme variant, so themes that ship several (Catppuccin Latte/Frappé/Macchiato/Mocha, Dracula/Dracula Light) can each be picked. Previously only the first one in the extension's first file was used.
  - The chosen theme is remembered as `"colorTheme"` in `settings.json` and comes back after a restart; before, LightLine started in its default theme every time. `colors` overrides still apply on top.
- **Add File to Project** ([#54](https://github.com/mehmoodulhaq570/LightLine/pull/54)): Right-click a folder in the Explorer and choose **Add File...** to copy any file into it. A file of the same name is never overwritten, a failed copy leaves nothing behind, and large files copy in the background without freezing the window.
- **AI Assistant with Local Models**: The AI Assistant panel, which only showed a made-up sample conversation, now works. Click **Connect to Ollama** and it chats with a model running on your own PC through [Ollama](https://ollama.com) (free, private, works offline), or with any OpenAI-compatible server set as `aiEndpoint` (e.g. LM Studio).
  - Select code in the editor and it's sent with your question; the panel shows which lines before you send. Earlier messages are sent too, so follow-up questions work.
  - Answers appear as they're written and are shown as Markdown. Code blocks have a **Copy** button, and **Copy answer** copies a whole answer. **Esc** or the Stop button ends an answer early; **+** starts a new chat.
  - The model menu in the header switches models, refreshes the list or turns the assistant off. The chosen model is saved as `aiModel`. LightLine picks a chat model made for code, and never one of Ollama's cloud models, which run on ollama.com; if you choose one yourself, the panel says so.
  - Nothing runs until you connect: no AI process, download or background work. Answers arrive on a worker thread, batched to about 20 screen updates a second, so a fast model can't slow typing, and editing never depends on the assistant.
- **AI Actions in the Editor**: Code from the AI Assistant goes straight into your file, and the assistant can be asked about code without typing.
  - Code blocks in answers have **Insert** (at the cursor), **Replace** (the code the question was about, or the selection) and **Copy** buttons. Inserted code takes the indentation of where it lands, so Python stays valid, and one **Ctrl+Z** undoes it. Replace is only offered while the original code is unchanged, so it can't overwrite edits made since.
  - Right-clicking in the editor opens a menu (it did nothing before): **AI: Explain / Fix / Write Tests for / Add Comments to Selection**, **AI: Explain / Fix This Error** on an underlined problem, then Cut, Copy, Paste and Select All. The same AI actions are in the command palette as `AI: ...`, plus **AI: New Chat**.
  - **Fix This Error** sends the lines around the error as context and selects just the error's block for the answer to replace.
- **Colored Code Blocks**: Rust and Python code blocks in AI answers and in the Markdown preview use the editor's syntax colors.
- **Editor Scrollbar**: The editor had no visible scrollbar. The window's own scrollbar was hidden by LightLine's custom title bar, yet it was still updated on every scroll. Each code pane now has a scrollbar down its right edge, as in VS Code. The slider shows how much of the file is on screen and where. Drag it to scroll, or click the track to jump there and keep dragging. It lights up under the mouse and while held, clicking it never moves the caret, and wrapped lines stop short of it. Its colors come from the theme.

#### Changed
- **Idle Terminals Use No CPU**: An open terminal woke its background threads about 100 times a second to check for work, even with nothing happening. They now sleep until there is output, input, a resize or a stop. An idle shell went from about 100 wakeups and 47 ms of CPU every 10 seconds to none.
- **Faster Startup With Many Tabs**: LightLine reopened every tab from the last session, reading each file and starting its highlighting, language server and git check, before the window would respond. Now all the tabs come back at once, but only the one that was showing is read; the others are read the first time you switch to them. With 20 tabs open, the window responds in about 300–400 ms instead of 600 ms, the same as with one.
- **Syntax Colors Update Incrementally**: For Rust and Python files, every keystroke sent a copy of the whole file to the background highlighter, which compared it with its previous copy and then recolored every line. It now receives just the edit and recolors only the lines that could have changed. Typing in a 92 KB file went from about 23 ms of background work per key to 2–5 ms.
- **Faster Typing**: Each keystroke redrew the whole window about three times (for the key itself, the new syntax colors and the changed-line marks), about 17–19 ms of drawing per key. Now the key redraws only the editor and the status bar, the colors and marks only the code, and marks that didn't change not at all: about 8–9 ms per key.
- **Cheaper Caret Blink**: Each blink of the editor caret redrew the whole window, twice a second while you were just reading. It now redraws only the code under the caret, about 1.4 ms instead of 8.5–9 ms per blink.
- **Faster Clicking and Selecting**: Clicking in the code or dragging to select redrew the whole window, about 10 ms on every mouse move of a drag, and the arrow, Home/End and Page keys redrew the whole editor. Now only the lines whose caret, selection or bracket highlight changed are redrawn, plus the status bar: about 1.7 ms per mouse move of a drag instead of 10 ms. If anything else changes as well (the view scrolls, the text is edited, a fold opens, the find box closes), the whole editor is still redrawn. The editor also skips drawing lines outside the area being redrawn, which brings each caret blink down to about 0.4 ms.
- **Changed-Line Marks No Longer Copy the File**: In a file tracked by Git, each pause in typing copied every line of the file and started a background thread to work out the green, blue and red marks beside changed lines. Now the unchanged lines at the start and end of the file are skipped without copying, and only the lines between them are compared. A small change is compared at once, without a thread; only large ones still go to the background.
- **Idle Language Servers Use No CPU**: With a Rust or Python file open, the thread talking to rust-analyzer or Pyright woke about 30 times a second to check for work. It now sleeps until the editor or the server has something for it. With a Python file open and nothing happening, LightLine went from about 40 wakeups a second to 2–5.
- **No Terminal at Startup**: LightLine no longer starts a PowerShell session on every launch. Startup is faster, the Welcome screen shows again, and the editor keeps keyboard focus. `` Ctrl+` `` starts the first terminal when you need it.
- **No Stray "Untitled" Tab**: Opening a folder (or closing the last file) no longer shows an empty "Untitled" tab. The editor says no file is open and how to open one; `Ctrl+N` still creates a real Untitled file.
- **Welcome Screen Redesign**: Reworked the native Welcome screen with clearer hero typography, Open Folder and New File actions, numbered Getting Started steps, compact Recent Projects and Quick Actions panels, and a streamlined community bar. The code-window illustration now uses a sharper violet-to-cyan LightLine bolt without the decorative orbit line. The layout adapts to smaller windows by reducing the visible rows, hiding secondary content and removing the illustration before it can overlap the heading or actions; all existing commands and click targets keep their original behavior.

#### Fixed
- **Rust CI Passes Again**: Every GitHub Actions "Rust" check since 28 September failed on the live Prettier test. GitHub's machines have `npx` but not Prettier, so `npx --yes prettier` downloads it first and ran past the formatter's 10-second limit. That test now counts as skipped there, as it already did when Prettier is missing; the timeout itself is still tested. The SVG preview's `chunks_exact` pixel loop had also come back, which Rust 1.98's Clippy rejects (see v0.2.0), so it uses `as_chunks` again. Build, tests and Clippy now pass on GitHub and with the release's pinned Rust 1.98.0.
- **Preview Button Stays Put**: With a Markdown preview open beside its file, scrolling the preview with the mouse wheel moved the focus to it, so the **Open Preview to the Side** button at the right end of the tab bar disappeared (and an open find box closed). The wheel now scrolls the pane under the mouse without moving the focus, as in VS Code. While the preview itself has the focus, for example after clicking in it, the button stays too, highlighted; clicking it closes the preview.
- **Side Panel Toggle and Less Flicker** ([#56](https://github.com/mehmoodulhaq570/LightLine/issues/56)): A ☰ button at the top of the left rail shows or hides the side panel, as **Ctrl+B** does; both now keep the view it shows (Ctrl+B used to switch it to the Explorer and stop a running search). The rail icons moved down one row, to where they sit on the Welcome page. Switching tabs no longer swaps the side panel to the Explorer: opening a search result or a changed file from Source Control that was already open used to replace the whole panel and discard the search results. The window title is now only set when it changes; setting it on every click and keystroke made Windows redraw the window frame and notify the taskbar each time.
- **Settings Panel** ([#18](https://github.com/mehmoodulhaq570/LightLine/issues/18)): The gear at the bottom of the left rail only said "Settings are not available yet". It now opens **Settings** in the side panel (also **Ctrl+,**), with font size and tab size, switches for indent with spaces, word wrap, auto indent, closing brackets and quotes, bracket highlighting, indent guides, format on save and web images in Markdown previews, plus the color theme and default shell. Changes apply at once and are saved to `settings.json`. **Open settings.json** at the bottom (or `>Open Settings (JSON)`) still edits the file directly.
- **Font Size, Font Family, Bracket Matching and Indent Guides Settings Work**: `fontSize`, `fontFamily`, `bracketMatching` and `indentGuides` in `settings.json` were read but ignored: the code font was always 15 px and the highlights always on. They now take effect, as soon as the file is saved.
- **Saving Settings No Longer Erases Your Colors**: Choosing a color theme, an AI model or a default shell rewrote `settings.json` from scratch, dropping your `colors` overrides and any other keys it didn't know. Saving now only updates LightLine's own settings and keeps the rest, and a `settings.json` with a syntax error is left alone instead of being overwritten.
- **"..." Menu in the Editor** ([#57](https://github.com/mehmoodulhaq570/LightLine/issues/57)): The "..." button at the right end of each editor's path row did nothing when clicked. It now opens a menu with Split Editor (or Close Split), Find, Replace, Toggle Word Wrap, Format Document, Close Editor and Command Palette, each with its shortcut. It can be used with the mouse or the arrow keys, Enter and Esc.
- **Find Box** ([#55](https://github.com/mehmoodulhaq570/LightLine/issues/55)): **Ctrl+F** looked like it did nothing: its only sign was a "Find:" prompt in the status bar that faded after 5 seconds, while typing still went into it. It now opens a find box at the top right of the editor that shows what you type and the match count ("2 of 5"), and jumps to the first match as you type. **Enter** / **Shift+Enter** go to the next / previous match and keep the box open, **Esc** or clicking in the code closes it, and selected text is used as the search. **Ctrl+H** adds a Replace row; **Alt+Enter** now does Replace All (it never reached LightLine before, so only Ctrl+Enter worked).
- **Error and Warning Counts**: The status bar always showed `⊗ 0  ⚠ 0`, even with errors underlined in the editor. It now counts the errors and warnings in every open file, and clicking **Problems** in the panel header gives the same totals instead of always saying there are none.
- **Run Button Hint**: Hovering the tab bar's Run button now shows "Run File (Ctrl+Shift+R)" in the status bar; it checked the wrong spot, so the hint never appeared.
- **Replace All No Longer Hangs**: Replacing text with something that contains it (e.g. `foo` → `self.foo`) looped forever. Replace All now finds every match first and applies them as a single edit, which is also a single undo step.
- **Fast Saves**: Saving wrote each line with its own system call; a 100,000-line file took about 2 seconds and now takes about 20 ms.
- **No Color Flicker While Typing**: Rust and Python syntax colors no longer vanish on every keystroke until the parser catches up. The previous colors move with the text until the new ones arrive.
- **Prettier Is Found on Windows**: Prettier installed through npm (`prettier.cmd`, `npx.cmd`) was always reported as missing, so formatting never ran. It is now found on `PATH`.
- **Language Server Stays in Sync After Reloads**: When an open file changed on disk (e.g. `git checkout`), the language server kept the old text, so diagnostics and completions pointed at the wrong places. The file is now reopened with its new contents.
- **Explorer, Quick Open and Search Show Every File**: The Explorer stopped at 250 rows, 9 folder levels and 400 entries per folder, and Quick Open and project search only saw the first 3,000 files. The Explorer limits are gone, and Quick Open and search now go up to 200,000 files (Quick Open says so if a workspace is larger).
- **New Files in Subfolders Appear**: Only the workspace's top folder was watched, so files created in expanded subfolders (by a terminal or Git) didn't appear until a manual refresh.
- **Large and Binary Files Open Safely**: Binary files are recognized from their first bytes and only the 64 KB the hex preview shows are read, instead of the whole file, twice. Text files over 128 MiB show an error instead of freezing LightLine.
- **AI Assistant Panel Toggle** ([#28](https://github.com/mehmoodulhaq570/LightLine/issues/28)): The rail button and Welcome action now open and close the Assistant panel, and the editor resizes to fit. Opening it keeps Find/Replace open, clicks, hovers and scrolling over the panel no longer reach the editor behind it, and in a narrow window the panel shrinks to fit instead of running off the edge.

## [v0.2.0] - 2026-09-26

### 2026-09-26

#### Added
- **Python Debugging (debugpy)**: The Run & Debug panel now debugs the active Python file as **Python: Current File**, through `debugpy` over DAP, alongside **Rust: Current Workspace** (LLDB).
  - Uses the same interpreter as Run Python File (the selected one, a nearby `.venv`, or `python` on `PATH`). If it lacks `debugpy`, the panel says so and the command palette's **Python: Install debugpy** installs it in the Output tab.
  - The program runs in the Output tab (debugpy's `runInTerminal`), so `input()` and prints work as in a normal run; `F5`, `F10` and `F11` still drive the debugger while it has focus.
  - Breakpoints, Locals/Globals with expandable values, call stack, Step Over/In/Out, Pause/Continue, Restart and Stop all work, and an uncaught exception pauses where it was raised instead of ending the program.
  - Restart relaunches the file the session started with, even if another tab is active by then.
  - Programs that start other Python processes run normally; those child processes are not debugged.
- **Launch Configuration Dropdown**: The configuration selector is a real dropdown: **Automatic** follows the active file (`.py` → Python, `.rs` or a Cargo workspace → Rust), or Rust/Python can be pinned. Files neither debugger handles show "No configuration" / "Nothing to debug". Also available as **Debug: Select Configuration** in the command palette.
- **F9 Toggles a Breakpoint** on the cursor line, as in VS Code.

#### Changed
- **Sharper Debug Toolbar Icons**: Start, Continue, Pause, Step Over, Step Into, Step Out, Restart and Stop are now anti-aliased SVG glyphs, cached per colour and size, instead of jagged GDI lines. Step Over/Into/Out use the familiar arrow-and-statement-dot shapes and Restart is a proper circular arrow, so each control reads at a glance.
- **Clearer Gutter Breakpoints and Paused Line**: Breakpoints are now smooth red dots in their own column at the left of the gutter, and line numbers are right-aligned (as in VS Code), so a dot no longer sits on top of the line number. While a debug session is paused, its line gets a yellow arrow in that column (with a small red dot when the line also has a breakpoint) and a faint yellow highlight.
- **Run & Debug Panel Redesign**: Reworked the native debugger sidebar around a real launch configuration selector (Rust or Python), a prominent Start action, a session-status card, count badges, and collapsible Variables/Call Stack/Breakpoints sections. Pause/Continue, Step Over/In/Out, Restart and Stop controls now appear only while a debug session exists, removing the confusing duplicate disabled toolbar from the idle state. Restart waits for the previous adapter to shut down and drains its queued events so stale termination messages cannot cancel the new session.
- **Debugger Scope Is Now Documented Accurately**: The DAP debugger targets Rust workspaces through `lldb-dap` and Python files through `debugpy`. C/C++ files can be run with `Ctrl+Shift+R` but have no debugging adapter yet; the README and shortcut descriptions say so.
- **Extensions Marketplace Redesign**: Rebuilt the native Extensions sidebar with a larger search field, Marketplace/Installed tabs, Featured/Themes/Formatters filters for the curated extensions, a recommended-count heading, richer cards for Prettier, Material Icon Theme and Dracula, and a compact Active Capabilities summary. The panel shows as many cards as fit, so registry search results past the first three are visible, install/installed buttons keep their real behavior, registry versions refresh in place, and capability rows report the formatter and icon theme that are actually active.

#### Fixed
- **Settings Needed a Restart**: `settings.json` was read only at startup, so saving a change such as `"formatOnSave": true` from inside LightLine did nothing until the next launch. Saving the file now re-reads and applies it immediately ("Settings saved and applied").
- **Broken `settings.json` Was Silently Ignored**: A syntax error (e.g. a trailing comma) made LightLine quietly fall back to every default. The error is now shown in red in the status bar, on startup and on save, and the settings already in effect are kept.
- **Command Palette Showed Only 7 Commands**: The Quick Open list was cut off at seven rows, so commands further down, like `Open Settings (JSON)`, could not be reached without typing a filter. The list now scrolls with `Up`/`Down`, `Page Up`/`Page Down` and the mouse wheel, shows a position hint such as `8–14 of 36`, and file results go up to 50 instead of 8. Clicking the hint line no longer opens a hidden eighth result.
- **Ctrl+P and Ctrl+, Went to the Shell**: With the terminal focused, as it is right after launch, `Ctrl+P` and `Ctrl+,` were sent to the shell instead of opening Quick Open or the settings. Both are now kept for LightLine, like `Ctrl+Shift+P`.
- **Every Save Reloaded the File**: The file watcher treated LightLine's own save as an outside change and reloaded the document, which wiped its undo history (`Ctrl+Z` could not go back past a save, so format-on-save could not be undone) and replaced the save's status message with "Reloaded: ...". Changes whose modification time and size match LightLine's own last save are now ignored; edits from other programs still reload.
- **Stepping Targeted the Wrong Thread**: After a stop had been reported, Step Over/In/Out, Continue and Pause were always sent for thread 1 rather than the thread that stopped, so stepping failed whenever that thread's ID was not 1 (lldb-dap uses OS thread IDs). They now follow the thread of the last stop.
- **Stopping at a Breakpoint Left Run & Debug**: Opening the stopped file switched the sidebar to the Explorer, hiding the variables and call stack. The panel now stays on Run & Debug, and a session that is still launching shows "Starting…" instead of "Paused".
- **Extension Cards Showed Invented Numbers**: Cards listed download counts and star ratings that were typed into the code (VS Code Marketplace-style figures, not data from the Zed registry, which has none), and every card carried a "verified" check mark. They now show only real data (publisher, registry version) and what kind of extension it is. Registry search results no longer claim only icon themes are supported; color themes are too.
- **Crash When an Open File Shrank on Disk**: Reloading a file that another program had made shorter left the cursor past the new end, and the next repaint panicked (index out of bounds). Cursors, selections and scroll positions are now kept inside the reloaded text.

### 2026-09-25

#### Added
- **Terminal Shell Picker & Multi-Profile Support**: Built-in support for launching multiple interactive shells: **PowerShell** (`pwsh`/`powershell`), **Command Prompt** (`cmd.exe`), **Git Bash** (`bash.exe`), and **WSL** (`wsl.exe`).
  - Added a dropdown button (`⌄`) right next to the terminal `+` button, and right-click support on the `+` button, opening a native popup menu to select and launch any installed shell.
  - Added "Select Default Profile" menu and `terminalDefaultProfile` configuration in `%APPDATA%\LightLine\settings.json`.
  - Terminal tab headers show the shell profile (e.g. `PWSH`, `CMD`, `BASH`, `WSL`), numbered (`PWSH 1`, `CMD 2`) when more than one terminal is open.
  - WSL is offered only when a Linux distribution is installed, not merely when `wsl.exe` exists; Docker Desktop's internal `docker-desktop` distributions don't count.
  - Choosing an unavailable shell (e.g. from the command palette) shows why in the status bar instead of opening a dead tab, and a default profile that has become unavailable falls back to PowerShell.
- **Built-in JSON & TOML Formatters**: `.json` and `.toml` files are formatted without Prettier or any other tool installed, via `Shift+Alt+F` (Format Document) and `formatOnSave`.
  - Both formatters only change layout. JSON keeps key order, number spelling and `//`/`/* */` comments (JSONC such as `tsconfig.json`); TOML keeps comments, key order and multi-line strings.
  - The TOML result is re-parsed and compared with the original, and formatting is refused if the document would change.
- **Inline Git Gutter Indicators**: Change markers in the editor gutter comparing the unsaved buffer against Git `HEAD`.
  - Line diff uses Myers' algorithm after trimming the common prefix and suffix. It runs on a background thread once typing pauses, with a work budget per recompute (live marks are skipped above 50,000 lines), so typing never waits on it.
  - Each changed region is classified on its own: 🟢 green bar for added lines, 🔵 blue bar for modified lines, 🔴 red marker below a deletion. Unchanged lines between edits stay unmarked.
  - Updates on keystrokes, undo (`Ctrl+Z`) and redo (`Ctrl+Y`); existing marks move with inserted or deleted lines immediately, before the recompute lands.
  - Refreshes when `HEAD` moves: a commit or checkout from LightLine, from an outside tool, or from the built-in terminal (LightLine watches `.git/index` and `.git/HEAD`; its own Git reads run with `GIT_OPTIONAL_LOCKS=0` so they never trigger that watch).
  - Only files inside a Git repository get markers; new files not yet in `HEAD` are shown as added.
- **Code Folding**: Fold blocks by brackets (`{ }`, `[ ]`, `( )`) or indentation (e.g. Python `def`/`class` blocks). Indentation folding applies only outside bracket languages, so a wrapped line in JSON or Rust isn't offered as a fold.
  - Bracket matching ignores brackets inside strings and comments, and requires matching bracket types.
  - Gutter column displays fold chevrons: `⌄` for foldable blocks and `›` for collapsed blocks; a collapsed block shows a `...` pill at the line end.
  - Gutter click partition distinguishes between breakpoint toggling (left 24px) and code folding (chevron column).
  - Arrow keys, Page Up/Down, mouse wheel and scrollbar move by visible lines, and the caret is drawn on its visual row. The scrollbar's range and thumb count visible rows, so folded lines don't distort it.
  - Folds move with edits above them; an edit inside a folded block, or a cursor landing in one (search, go to definition, undo), unfolds it. Folding the block the caret is in moves the caret to the fold's first line.
  - Chevrons are drawn as vector strokes, so they render even when the editor font lacks the `⌄`/`›` glyphs.

#### Changed
- **Source Control Panel Redesign**: Reworked the native Git sidebar around a clearer commit composer, larger Commit and sync controls, a dedicated branch/ahead-behind status strip, count badges, collapsible Staged/Changes/History sections, a polished clean-worktree state, timeline-style history cards, and an internal history scrollbar. Rendering and hit-testing continue to share the same geometry, so all existing stage, unstage, discard, commit, Push, Pull, Fetch, keyboard navigation, and diff actions remain aligned with the new layout.

#### Fixed
- **Workspace Search Keyboard Navigation**: Submitting a project-wide search now transfers focus from the query field to the results list, where `Up`/`Down` change the selection and `Enter` opens it. Mouse and keyboard result activation both return input to the editor cleanly.
- **Stale Sidebar and Search Focus**: Opening or closing a workspace now clears obsolete search/list focus. `Esc` also exits focused sidebar lists, closes Search when appropriate, and prevents an invisible focus state from trapping later input.
- **Hidden Editor Mutations**: Navigation, Tab, Backspace, and Delete are consumed by the focused sidebar instead of changing the editor behind it. Editor control chords are likewise guarded while list focus is active.
- **Global Shortcuts While Panels Are Focused**: `Ctrl+Shift+X` continues to open Extensions, debugger commands (`F5`, `Shift+F5`, `F10`, `F11`, `Shift+F11`) still reach their handlers, and `Ctrl+Shift+W` closes the workspace even when the terminal has focus.
- **Output Pane Program Input**: `Enter` now sends the pipe-appropriate CRLF sequence to a running program, while Backspace uses modifier-aware terminal encoding (`DEL` normally and `BS` with Control).
- **Shift+Alt+F, F10 and Alt Keys Never Reached LightLine**: Windows delivers Alt chords and F10 as `WM_SYSKEYDOWN`, which the window ignored, so Format Document (`Shift+Alt+F`), Step Over (`F10`) and Alt keys in the terminal did nothing. They are now routed to the key handler; `Alt+F4` and other system keys keep their default behavior.
- **Minimize Scrolled the Editor**: Minimizing and restoring the window no longer leaves the editor scrolled to the caret line.
- **Status Messages Were Never Shown**: The editor's status bar always displayed a fixed "Ready", so every message LightLine reported (format results and errors, "Committed", unavailable shells, ...) was invisible. The latest message now appears in the status bar for five seconds, in red when something failed.
- **Enter in the Command Palette Also Typed a Newline**: When Enter ran a palette command, accepted a completion or confirmed an Explorer rename, the character Windows generates for the same key press still reached the editor and inserted a blank line (a Tab accepting a completion likewise inserted a tab). Those characters are now dropped.
- **Editor Caret Rendering Consistency**: Normalized the caret visibility condition so it remains suppressed whenever terminal, search, sidebar, or another split pane owns focus.

### 2026-09-24

#### Added
- **Bundled Curated Vector SVG Suite (VS Code Style)**: LightLine now bundles an authentic, anti-aliased SVG icon suite directly into the binary (`BuiltinIcon`), rasterized on-the-fly with `resvg`/`tiny-skia`. Out of the box, it provides clean, color-accurate vector icons for Rust (`.rs`), Python (`.py`), Markdown (`.md`), TOML, JSON (`{ }`), YAML, Git (`.git*`), HTML, CSS, JS, TS, shell scripts (`>_`), media, lock files, and documents, alongside sleek two-tone slate-blue folders with distinct open/closed states. Zero network requests, 100% offline, and zero user configuration needed.
- **Tree Indentation Guidelines**: Subtle 1px vertical hierarchy guidelines rendered down each indentation level in the Explorer tree, visually connecting parent directories to nested child items.
- **Automated Windows Releases**: Added a GitHub Actions release workflow that accepts a version from manual workflow dispatch or `/release <version>` issue comments, runs Windows formatting, tests, and Clippy checks, builds the portable executable, creates the versioned ZIP archive, generates SHA-256 checksums, and publishes the GitHub Release assets without code signing.
- **SmartScreen Guidance**: Documented the expected unsigned-app warning and the safe `More info` → `Run anyway` path, explicitly limited to downloads from the official release whose checksum matches.
- **Release SHA-256 Checksums**: The release workflow now generates and uploads `SHA256SUMS.txt` containing checksums for both `lightline.exe` and the versioned Windows ZIP.

#### Added (release & signing preparation)
- **MIT `LICENSE` File**: Added the license text and `license = "MIT"` in `Cargo.toml`; the README license badge now links to it.
- **Executable Version Metadata**: `lightline.exe` now embeds product name, file description, copyright, and file/product version (visible in Properties > Details), generated in `build.rs`. Release builds take the version from the release request through `LIGHTLINE_VERSION`.
- **Code Signing Policy**: README section listing team roles, the SignPath Foundation attribution, and a privacy statement, as required to apply for free open-source code signing.
- **Disabled SignPath Signing Steps**: `release.yml` contains opt-in signing steps (gated on the `SIGNPATH_ENABLED` repository variable) plus `.signpath/artifact-configuration.xml`. Nothing changes for releases until signing is enabled.

#### Changed
- **Instant First-Launch (Removed Synchronous Git Clone)**: Removed the silent, blocking first-launch `git clone` of Material Icons from `App::new`. LightLine now launches instantaneously in milliseconds, relying on the built-in SVG suite by default, while keeping full support for installing the 1,000+ icon Material Icon Theme on demand via the Extensions marketplace (`Ctrl+Shift+X`).
- **Clean Workspace Header Layout**: Removed the redundant folder icon from the workspace root header row so the project name aligns cleanly right next to the expand/collapse chevron (matching modern VS Code conventions).
- **Professional Welcome Dashboard**: Rebuilt the Welcome screen to match the main workbench's midnight-blue visual system, with the compact LightLine title bar, centered command search, icon-only activity rail, polished quick-start cards, recent projects, Quick Actions, Getting Started, and Community panels.
- **Responsive DPI-Aware Layout**: Rebalanced the Welcome dashboard for maximized Windows displays and 125% scaling so all launch cards, recent projects, and six Quick Actions remain visible without forcing fullscreen behavior or clipping the right column.
- **Consistent Workbench Interaction**: The global command center and minimize, maximize/restore, and close controls now use the same geometry and behavior on both the Welcome screen and editor.
- **Documentation Screenshot**: Updated the README Welcome image to show the implemented native interface rather than an earlier layout.
- **Explorer Sort Order**: Removed the special case that pinned a folder named `src` above all others. Folders now sort purely alphabetically (dot-folders first), followed by files.
- **Explorer Row Spacing**: The first item under the workspace root is now one normal row (27px) below it instead of about 38px, so the root sits closer to its children. Rendering, click handling and scrolling share the same `EXPLORER_TOP` constant, so all three moved together.

#### Fixed
- **Explorer Tree & Activity Rail Icon Loss on Theme Removal**: Fixed issues where uninstalling an icon theme left the Activity Rail Explorer icon as a blank blue square and reduced file and folder tree items to 6×6 gray squares. The editor now seamlessly falls back to the built-in SVG catalog across the explorer tree, tab strip, welcome page, and rail.
- **Welcome Header Controls Ignored**: Page-specific Welcome hit testing previously intercepted clicks before the custom title bar could handle window controls or open Quick Open.
- **GitHub Actions Clippy Failure**: Updated SVG pixel conversion for Rust 1.98's `chunks_exact_to_as_chunks` lint, which was promoted to an error by the workflow's `-D warnings` setting. The locked test suite and strict all-target Clippy check now pass locally.
- **Extension Marketplace Preservation**: Confirmed the Welcome/workbench redesign continues to route the Extensions activity to the existing dynamic Zed registry; no marketplace, search, install, uninstall, icon-theme, or color-theme logic was removed.

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
