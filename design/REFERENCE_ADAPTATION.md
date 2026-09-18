# Reference design adaptation

The supplied LightLine image is a design board. Its large upper frame shows the main editing workspace. The smaller frames show separate Welcome, Run & Debug, Terminal, and Extensions views. The strip along the bottom presents a five-step workflow: open a project, write code, run/debug, ask AI, and build/ship.

## Visual system

- Base surfaces: `#0C1523` editor, `#0F1929` explorer, `#121F33` status, and `#111D31` active tab.
- Action and code accents: `#5E99FF` blue, `#956DFF` violet, `#67DCD7` teal, `#6FDCA3` green, and `#F8B482` warm numeric text.
- Near-black navy canvas and layered dark blue surfaces keep the code area dominant.
- Thin blue borders, violet active states, and restrained teal accents identify interaction and status.
- Code uses blue keywords, teal types, green strings, violet macros, warm numbers, and muted comments. Bright foreground text stays readable against the dark background.
- A narrow activity rail gives access to navigation. A collapsible file explorer sits beside the editor. Tabs and breadcrumbs tell users where they are. The bottom status strip carries transient messages and cursor details.
- The reference uses compact typography, subtle row highlights, and generous spacing between major panels. Decorative gradients and rounded cards belong mainly to its Welcome and tool views; the writing surface stays quiet.

## Applied to the current Windows editor

The native editor now uses this palette, dark Windows title bar, labeled activity rail, collapsible explorer, color-coded file rows, tab accent, breadcrumbs, active-line highlight, indentation guides, compact UI font, and segmented status bar. Explorer opens the folder around the first opened file, preferring a nearby Cargo or Git project root, and reveals the active file. Folder contents load only when expanded; the editor does not scan an entire project at startup. Generated `.git` and `target` folders are hidden from the tree. `Ctrl+B` toggles the explorer. Clicking a file opens it in a tab, and the mouse wheel scrolls the file list independently. The Search action activates the existing in-file find.

The rail displays working actions only. The reference's AI dock, debugger, terminal, extension marketplace, and build view are future product areas; they are not represented as working controls in this editor. When added, they should open on demand and preserve the central writing space. The existing Quiet Workbench prototype is an earlier concept; this reference is now the visual direction for the native editor.

## Screen workflow to build toward

1. **Start:** recent projects and Open Folder. No background services start until a workspace is opened.
2. **Edit:** explorer, tabs, breadcrumbs, and code occupy the primary view. Side and bottom panels can collapse.
3. **Find:** in-file search first; project search later with bounded, cancelable results.
4. **Run:** build output and terminal appear in a bottom panel only when invoked.
5. **Review:** source control and diagnostics are separate on-demand panels.
6. **Assist:** an optional AI side panel appears only when configured and opened by the user.

LightLine adopts the reference's name and visual direction. Its AI, debugger, terminal, and extension areas remain optional features to implement as those systems become real.
