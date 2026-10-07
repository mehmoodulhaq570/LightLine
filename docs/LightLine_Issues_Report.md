# LightLine IDE — GitHub Issues Report

Repo: [mehmoodulhaq570/LightLine](https://github.com/mehmoodulhaq570/LightLine)
Updated: 2026-10-07 | Open issues: 16 | Closed issues: 35 (listed in section 3)

The first version of this report (2026-09-21) listed 37 open issues. This update re-checks every issue that is still open against the code on `main`.

**How the status was determined.** Each open issue's body and comment thread was read, then compared with the source. This was a code read only: the app was not run, so behavior that depends on live rendering (flicker, hover feel, logo alignment) is marked **unverified**. Eleven files had uncommitted local changes when the check was made (`workflow.rs`, `extensions/installer.rs`, `session.rs` and others), so verdicts that touch them may change once those are committed.

Status key: **Resolved** · **Partly resolved** · **Open** · **Unverified** (needs a live run or the reporter) · **Not a code fix**

---

## 1. Summary

No open issue is fully resolved. The closest to closeable are #48 and #52.

| # | Title | Status | Notes |
|---|-------|--------|-------|
| 62 | Hover highlight and tooltips for rail, title bar and Welcome buttons | **Open** | Hover handling covers only the scrollbar, the tab-strip Run button hint, the editor hover card and menus. |
| 59 | Own API key for OpenAI-compatible cloud services | **Open** | No `Authorization` header, credential storage or `OPENAI_API_KEY` fallback in `src/`. |
| 52 | More languages and a language picker for Run/Debug | **Partly resolved** | Runners, missing-tool messages and the Run dropdown are done. The status-bar language picker is not. |
| 48 | Prettier CLI extension and architecture | **Partly resolved** | Core pipeline is built; a few gaps remain (see below). |
| 43 | Contributor onboarding | **Not a code fix** | CONTRIBUTING.md merged (#44). Kept open on purpose for CI and further docs. |
| 35 | Doesn't support Mac | **Not a code fix** | Win32 app; needs a platform port. |
| 29 | Issue in functionality and UI | **Partly resolved** | Items 2 and 3 fixed, item 4 partly (see #52). Items 1 and 5 unverified. |
| 22 | light_ide_issues | **Partly resolved** | Syntax-error underlines and Run exist. Flicker and program-not-closing unverified. |
| 20 | System Error | **Unverified** | Only a launch-error screenshot; no way to tell from the code. |
| 16 | UI/UX improvements | **Open** | Umbrella issue. Tooltips and hover (#62) are still missing. |
| 15 | Missing Taskbar Options | **Open** | Problems tab is a placeholder; there is no Debug Console. Mayuri-004 is working on it. |
| 14 | ISSUES | **Partly resolved** | Compile/run fixed; language picker is the #52 gap; terminal slowness unverified. |
| 13 | Lighline issue | **Partly resolved** | AI assistant fixed. Defender warning remains until the app is code-signed (SignPath application pending). |
| 7 | Extensions Tab & Switching | **Partly resolved** | Item 1 fixed. Items 2 and 3 unverified. |
| 5 | User Inconvenience | **Partly resolved** | Item 2 fixed. Item 1 (hover) is the #62 gap. |
| 3 | UI Bug | **Open** | Hover and "what is clickable" are the #62 gap. IDE badge alignment unverified. |

---

## 2. Open Issues (detail and code findings)

### #62 — [Hover highlight and tooltips for rail, title bar and Welcome buttons](https://github.com/mehmoodulhaq570/LightLine/issues/62) `good first issue` — **Open**
- Wants hover highlight on the activity rail, title-bar buttons and Welcome actions, plus hints for icon-only buttons. Closes the remaining parts of #3, #5 and #16.
- Code: `mouse_hover_move` (`windows_app/language.rs`) handles the run-config panel, editor context menu, "..." menu, scrollbar, the tab-strip Run button hint (status bar) and the editor hover card. `WM_MOUSELEAVE` only clears the scrollbar hover. Nothing tracks hover for the rail, title bar or Welcome buttons.
- Constraint from the issue: repaint only the old and new button rects, no polling timers (speed-first).

### #59 — [Use your own API key with OpenAI-compatible cloud services](https://github.com/mehmoodulhaq570/LightLine/issues/59) `enhancement` — **Open**
- Taken by @RajDalvi08.
- Code: no `Authorization`/`Bearer`, Credential Manager or `OPENAI_API_KEY` reference anywhere under `src/`. Pointing `aiEndpoint` at a cloud service still fails with 401.

### #52 — [More languages and a language picker for Run/Debug](https://github.com/mehmoodulhaq570/LightLine/issues/52) `enhancement` — **Partly resolved**
- Assigned to @janhavibhoir.
- Done: JavaScript/TypeScript, Go and Rust runners alongside Python and C/C++ (`runner.rs`); clear missing-tool messages; the Run dropdown and saved configurations (`run_choice.rs`, `run_config*.rs`).
- Not done: the status-bar language picker. `open_language_actions` (`windows_app/language.rs`) still only opens the command palette, filtered for Python, C/C++ or Rust. For any other language, including JS and Go, it reports "No run action is available".
- Out of scope per the issue: debugger (DAP) support for new languages.

### #48 — [Dedicated Prettier CLI extension and architecture](https://github.com/mehmoodulhaq570/LightLine/issues/48) `enhancement` — **Partly resolved**
- Done: Prettier runs as a subprocess with the buffer on stdin and `--stdin-filepath`, under a timeout (`formatter.rs`). Format Document runs on a background thread. Format on save follows the `formatOnSave` setting. Extensions covered include JS/TS, JSON, CSS, HTML, Markdown and YAML. Rust, Python and C/C++ formatters were added alongside.
- Gaps against the issue:
  - Only PATH or `npx` is searched; workspace-local `node_modules/.bin/prettier` is not detected.
  - Format on save runs on the UI thread (bounded by the timeout) and fails silently.
  - Whether a "Prettier not found" message is clear, and whether cursor/scroll position survives formatting, were not confirmed.

### #43 — [Improve contributor onboarding](https://github.com/mehmoodulhaq570/LightLine/issues/43) `documentation` `enhancement` — **Not a code fix**
- CONTRIBUTING.md merged in #44. Left open for CI checks and further onboarding work. @shubhayu-dev is assigned.

### #35 — [Doesn't support Mac](https://github.com/mehmoodulhaq570/LightLine/issues/35) `enhancement` `help wanted` `platform` — **Not a code fix**
- LightLine is a Win32 application. macOS support would be a port, not a fix.

### #29 — [Issue in functionality and UI](https://github.com/mehmoodulhaq570/LightLine/issues/29) — **Partly resolved**
1. Scrollbar highlights on every click: **unverified** (owner asked the reporter to recheck on the latest release).
2. AI assistant: **resolved** (Ollama-backed panel on `main`).
3. No installed extensions: **resolved** (search/install work; the Installed tab shows its count and an empty-state message).
4. Run/Debug limited to C/C++: **partly resolved** (see #52).
5. Folder section UI: **unverified**; the reporter has not described a specific problem.

### #22 — [light_ide_issues](https://github.com/mehmoodulhaq570/LightLine/issues/22) — **Partly resolved**
1. Program does not close properly: **unverified**; not reproduced. Stop now ends the program and its child processes.
2. Flicker and two white lines: **unverified**; several flicker fixes have landed since the report.
3. No syntax error feedback: **resolved** (underlines in the editor, counts in the status bar).
4. AI Assistant not opening: **resolved**.
5. Run and Debug not responding: **resolved** (Run for Python, C/C++, Rust, JS/TS, Go; debugger for Rust and Python).

### #20 — [System Error](https://github.com/mehmoodulhaq570/LightLine/issues/20) `platform` — **Unverified**
- Launch error shown only in a screenshot. No reply on the issue. Needs the error text or Windows version from the reporter.

### #16 — [UI/UX improvements needed](https://github.com/mehmoodulhaq570/LightLine/issues/16) `navigation-ui` `ui-ux` — **Open**
- Umbrella issue. Tooltips and hover feedback are tracked by #62. The bottom panel is partly addressed under #15.

### #15 — [Missing Taskbar Options](https://github.com/mehmoodulhaq570/LightLine/issues/15) `ui-ux` — **Open**
- Mayuri-004 is working on the Problems tab first, then the Debug Console.
- Done: collapsible bottom panel, Terminal tabs and shell picker, Output tab.
- Not done: `TerminalTab` has only `Output` and `Terminal`. The Problems header is the fixed text `PROBLEMS  0`, and clicking it only sets a status-bar message. There is no Debug Console tab.

### #14 — [ISSUES](https://github.com/mehmoodulhaq570/LightLine/issues/14) — **Partly resolved**
1. Terminal glitchy/slow: **unverified**; speed fixes landed, reporter asked to retest.
2. Language switching: the #52 picker gap.
3. Code not compiling/running: **resolved**.
4. AI assistant: **resolved**.

### #13 — [Lighline issue](https://github.com/mehmoodulhaq570/LightLine/issues/13) `platform` — **Partly resolved**
1. Windows Defender/SmartScreen warning: **open**. Not code-signed yet; SignPath application pending. README documents the SHA256 check and "Run anyway".
2. AI Assistant: **resolved**.

### #7 — [Extensions Tab & Switching in IDE](https://github.com/mehmoodulhaq570/LightLine/issues/7) — **Partly resolved**
1. "0 extensions installed" indicator: **resolved** (`Installed (0)` and "No installed extensions").
2. Opening one file exposes the whole Downloads folder: **unverified**.
3. Files keep switching after clicking in Run & Debug: **unverified**.

### #5 — [User Inconvenience](https://github.com/mehmoodulhaq570/LightLine/issues/5) — **Partly resolved**
1. No hover highlight: **open** (#62).
2. Open Project doesn't say what to open: **resolved**. It uses the folder picker (`IFileOpenDialog` with `FOS_PICKFOLDERS`).

### #3 — [UI Bug](https://github.com/mehmoodulhaq570/LightLine/issues/3) — **Open**
1. "IDE" text outside the purple box: **unverified**. The badge and label are drawn separately in `render/welcome.rs`; alignment needs a visual check.
2. Rail buttons don't show what is clickable: **open** (#62).
3. Getting Started items give no sign they are inactive: **open** (#62 covers non-clickable items looking clickable).

---

## 3. Closed Since the First Report

Closed on GitHub between 2026-09-22 and 2026-10-03. Their themes are folded into section 4.

| Closed | Issues |
|--------|--------|
| 2026-09-22 | #8, #10, #17, #23, #25, #26, #30, #31, #33 |
| 2026-09-23 | #1, #2, #12, #19, #37 |
| 2026-09-24 | #34 |
| 2026-09-26 | #9, #24, #27, #32 |
| 2026-09-27 | #28 |
| 2026-09-28 | #18, #36, #38, #42, #45, #50, #55, #56, #57 |
| 2026-10-03 | #4, #6, #11, #21, #58 |

(#53, "Release v0.2.0", was also closed on 2026-09-26.)

---

## 4. Problem Areas (merged view)

The original areas A–I, with the current state of each.

| Area | Original problem | Current state | Still open |
|------|------------------|---------------|------------|
| A. AI Assistant | Panel didn't open or respond | Fixed. Chats with a local Ollama model; Explain / Fix This Error / Insert / Replace actions. | #59 (own API key / cloud services) |
| B. Run & Debug | Code didn't run; language limited to C/C++; no tool checks | Run works for Python, C/C++, Rust, JS/TS and Go with saved configurations and missing-tool guidance. Debugger supports Rust and Python. | #52 (language picker), #22 item 1 |
| C. Terminal | No input / slow | Input fixed; multiple tabs and shell picker; idle CPU reduced. | #14 item 1 (unverified) |
| D. Extensions | Search/install broken, no empty state | Search, install and the "0 installed" state work. | None |
| E. File/Project management | Can't close/delete/add files, Open Project problems | Close, delete, New File, Add File to Project and the folder picker are in. | #7 items 2–3 (unverified), #29 item 5 (unverified) |
| F. Navigation & UI stability | Scrollbar jumps, flicker, no hover feedback | Scrollbar fixes landed. | #62 (hover), #3, #5 item 1, #22 item 2, #29 item 1 |
| G. UI/UX & layout | Cramped layout, no tooltips, missing bottom panel tabs | Bottom panel, Settings and shell picker exist. | #16, #15, #62 |
| H. Platform / environment | No macOS, Defender warning, launch error | No change. | #35, #13 item 1, #20 |
| I. Editor QoL | No Python auto-indent | Fixed (#31 closed). | #48 gaps |

---

## 5. Priority Recommendation (suggested, not from GitHub)

LightLine's goal is a fast IDE first, so performance-sensitive items rank above new features.

1. **Close or update now:** #52 once the language picker exists, and #48 with a note on its gaps. Both are largely delivered by recent runner and formatter work.
2. **High, unclaimed and well scoped:** #62 (hover and tooltips; also closes the remaining parts of #3, #5 and #16) and #59 (own API key).
3. **Medium, already being worked on:** #15 (Problems tab, then Debug Console) by Mayuri-004.
4. **Needs the reporter or a live run before acting:** #20, #22, #29, #14, #7 items 2–3 and the #3 badge alignment. Ask for v0.3.1 repro steps, as the owner's comments already do.
5. **Longer term:** #13 (code signing), #35 (macOS port), #43 (CI and onboarding).
