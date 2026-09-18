# Reference design adaptation

The supplied LightLine image is a design board. Its large upper frame shows the main editing workspace. The smaller frames show separate Welcome, Run & Debug, Terminal, and Extensions views. The strip along the bottom presents a five-step workflow: open a project, write code, run/debug, ask AI, and build/ship.

## Visual system

- Base surfaces: `#0C1523` editor, `#0F1929` explorer, `#121F33` status, and `#111D31` active tab.
- Action and code accents: `#5E99FF` blue, `#956DFF` violet, `#67DCD7` teal, `#6FDCA3` green, and `#F8B482` warm numeric text.
- Near-black navy canvas and layered dark blue surfaces keep the code area dominant.
- Thin blue borders, violet active states, and restrained teal accents identify interaction and status.
- Code uses blue keywords, teal types, green strings, violet macros, warm numbers, and muted comments. Bright foreground text stays readable against the dark background.
- File and folder glyphs use a pinned Material Icon Theme snapshot that LightLine embeds and updates with its own releases.
- A narrow activity rail gives access to navigation. A collapsible file explorer sits beside the editor. Tabs and breadcrumbs tell users where they are. The bottom status strip carries transient messages and cursor details.
- The reference uses compact typography, subtle row highlights, and generous spacing between major panels. Decorative gradients and rounded cards belong mainly to its Welcome and tool views; the writing surface stays quiet.

## Applied to the current Windows editor

The native editor now uses this palette, dark Windows title bar, labeled activity rail, collapsible explorer, color-coded file rows, tab accent, breadcrumbs, active-line highlight, indentation guides, compact UI font, and segmented status bar. Explorer opens the folder around the first opened file, preferring a nearby Cargo or Git project root, and reveals the active file. Folder contents load only when expanded; the editor does not scan an entire project at startup. Generated `.git` and `target` folders are hidden from the tree. `Ctrl+B` toggles the explorer. Clicking a file opens it in a tab, and the mouse wheel scrolls the file list independently. The Search action activates the existing in-file find.

The rail now opens working Explorer, project Search, Rust test Output, and Git Review views. Start and recent workspaces, Quick Open for files and commands, search-hit previews, and side-by-side review follow the Quiet Workbench workflow while retaining this reference's navy palette. These services start on demand. The Run panel is task output; an interactive terminal, debugger, AI dock, and extension marketplace remain future product areas.

## Screen workflow to build toward

1. **Start:** recent workspaces, Open File, Open Folder, and New File are available.
2. **Edit:** explorer, tabs, breadcrumbs, and code occupy the primary view. The sidebar and output panel can collapse.
3. **Find:** Ctrl+F searches the current file; Ctrl+Shift+F searches bounded workspace files; Ctrl+P opens files and commands.
4. **Run:** Rust test output appears in a bottom panel only when invoked.
5. **Review:** Git status and a read-only side-by-side diff appear only when invoked.
6. **Assist:** an optional AI side panel is still a future area.

LightLine adopts the reference's name and visual direction. Its AI, debugger, terminal, and extension areas remain optional features to implement as those systems become real.
