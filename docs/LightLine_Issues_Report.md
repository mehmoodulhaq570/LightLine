# LightLine IDE — GitHub Issues Report

Repo: [mehmoodulhaq570/LightLine](https://github.com/mehmoodulhaq570/LightLine)
Updated: 2026-10-12 | Open issues: 7 | Closed issues: 44 (listed in section 3)

The first version of this report (2026-09-21) listed 37 open issues and the second (2026-10-07) listed 16. This update re-checks the seven still open against `main` plus the local changes of 2026-10-12, which are not on GitHub yet: the language picker (#52) and the formatter install hints (#48).

**How the status was determined.** Each open issue's body and comments were read and compared with the source. Two items were also checked in a running build (#52's picker and Run, and #7 item 3); those are marked **verified live**.

Status key: **Resolved** · **Partly resolved** · **Open** · **Decision needed** · **Not a code fix**

---

## 1. Summary

#48 and #52 can be closed now that the 2026-10-12 changes are pushed, and #7 once the lone-file change below ships.

| # | Title | Status | Notes |
|---|-------|--------|-------|
| 59 | Own API key for OpenAI-compatible cloud services | **Open** | No `Authorization` header, credential storage or `OPENAI_API_KEY` support in `src/`. |
| 52 | More languages and a language picker for Run/Debug | **Resolved** (local) | Runners were already done; the picker is now in. Debugging other languages was out of scope. |
| 48 | Prettier CLI extension and architecture | **Resolved** (local) | Every requirement is met; the last gap (install instructions when Prettier is missing) is now in. |
| 43 | Contributor onboarding | **Not a code fix** | CONTRIBUTING.md (#44) and CI (`rust.yml`, `release.yml`) exist. Kept open on purpose for further onboarding. |
| 35 | Doesn't support Mac | **Not a code fix** | Win32 app; needs a platform port. |
| 13 | Lighline issue | **Partly resolved** | AI assistant fixed. The SmartScreen warning stays until the app is code-signed (SignPath application pending). |
| 7 | Extensions Tab & Switching | **Resolved** (local) | All three items resolved: item 2 by opening lone files without a folder, as VS Code does. |

---

## 2. Open Issues

### #59 — [Use your own API key with OpenAI-compatible cloud services](https://github.com/mehmoodulhaq570/LightLine/issues/59) `enhancement` `ai-assistant` — **Open**
- @RajDalvi08 offered to take it in September; there has been no PR since.
- Code: nothing under `src/` sends an `Authorization` header or stores a key. Pointing `aiEndpoint` at a cloud service still fails with 401.

### #52 — [More languages and a language picker for Run/Debug](https://github.com/mehmoodulhaq570/LightLine/issues/52) `enhancement` `run-debug` — **Resolved (local)**
- Runners, missing-tool messages, the Run dropdown and saved configurations were already done (`runner.rs`, `run_choice.rs`, `run_config*.rs`).
- New: clicking the language name in the status bar (or **Run: Choose the File's Language** in the Command Palette) opens a picker: Auto Detect, Python, C, C++, JavaScript, TypeScript, Go, Rust, plus the chosen language's own commands. Run then treats the file as that language, the status bar shows it, and the Run button appears even on a file without an extension (`run_language.rs`).
- C and C++ are compiled with `-x c` / `-x c++`, so any file name works. `go run` and tsx choose by file name, so Go and TypeScript explain that the file must be renamed.
- **Verified live:** a Python script named `hello` (no extension) showed "Plain Text"; choosing Python from the picker showed "Python" and a Run button, and Run printed its output in the Output tab.
- Not covered by a live test: the C/C++ path (no compiler on the test machine; the arguments follow gcc/clang's documented `-x`).
- The choice lasts until the tab is closed; it is not saved in the session. Debugging other languages was out of scope.

### #48 — [Dedicated Prettier CLI extension and architecture](https://github.com/mehmoodulhaq570/LightLine/issues/48) `enhancement` `extensions` `editor-qol` — **Resolved (local)**
Checked against each requirement:
- stdin/stdout with `--stdin-filepath` and a 10 s timeout that ends the whole process tree (`formatter.rs`, `jobs.rs`): **done**.
- Off the UI thread: **done**. Format Document runs on a worker; format-on-save saves first and formats afterwards.
- Project-local `node_modules/.bin/prettier`, then PATH, never `npx`: **done**.
- Extensions: JS/TS (incl. JSX/TSX, MJS/CJS), JSON, CSS/SCSS/Less, HTML, Markdown, YAML, GraphQL and Vue: **done**. They are listed in code rather than in a manifest file.
- `Shift+Alt+F` and the `formatOnSave` setting: **done**.
- Caret, selection and scroll kept after formatting (`apply_formatted`): **done**.
- Feedback: failures and syntax errors show in the status bar. New: a missing formatter now says how to install it ("Prettier was not found. Install it in the project with `npm install --save-dev prettier`, …"), and the same goes for rustfmt, ruff/black and clang-format.

### #43 — [Improve contributor onboarding](https://github.com/mehmoodulhaq570/LightLine/issues/43) `documentation` `enhancement` — **Not a code fix**
- CONTRIBUTING.md was merged in #44, and CI runs in `.github/workflows/rust.yml`. Left open by the owner for further onboarding work. @shubhayu-dev is assigned.

### #35 — [Doesn't support Mac](https://github.com/mehmoodulhaq570/LightLine/issues/35) `enhancement` `help wanted` `platform` — **Not a code fix**
- LightLine is a Win32 application. macOS support would be a port, not a fix.

### #13 — [Lighline issue](https://github.com/mehmoodulhaq570/LightLine/issues/13) `ai-assistant` `platform` — **Partly resolved**
1. Windows Defender/SmartScreen warning: **open** until the app is code-signed (SignPath application pending). The README documents the SHA256 check and "Run anyway".
2. AI Assistant: **resolved**.

### #7 — [Extensions Tab & Switching in IDE](https://github.com/mehmoodulhaq570/LightLine/issues/7) `extensions` `file-management` — **Resolved (local)**
1. "0 extensions installed" indicator: **resolved** (`Installed (0)` and "No installed extensions").
2. "Opening one file gives access to my whole Downloads folder": **resolved (2026-10-13, local)**. Opening or saving a file with no folder open used to make the file's folder the workspace (`set_workspace_from_file`), list it in the Explorer and save it as a recent workspace. Now the file opens on its own, as in VS Code: the Explorer shows "No folder opened" with an **Open Folder** button, and language servers use the file's folder as their root. Verified live.
3. "Files keep switching after clicking in Run & Debug": **resolved, verified live**. Every click in the Run & Debug panel is handled there and returns. With three tabs open, 72 clicks across the panel left the editor and tab strip unchanged.

---

## 3. Closed Issues

| Closed | Issues |
|--------|--------|
| 2026-09-22 | #8, #10, #17, #23, #25, #26, #30, #31, #33 |
| 2026-09-23 | #1, #2, #12, #19, #37 |
| 2026-09-24 | #34 |
| 2026-09-26 | #9, #24, #27, #32, #53 |
| 2026-09-27 | #28 |
| 2026-09-28 | #18, #36, #38, #42, #45, #50, #55, #56, #57 |
| 2026-10-03 | #4, #6, #11, #21, #58 |
| 2026-10-10 | #3, #5, #14, #15, #16, #20, #22, #29, #62 |

---

## 4. Problem Areas

| Area | Original problem | Current state | Still open |
|------|------------------|---------------|------------|
| A. AI Assistant | Panel didn't open or respond | Fixed. Chats with a local Ollama model, with Explain / Fix / Insert / Replace actions. | #59 (own API key / cloud services) |
| B. Run & Debug | Code didn't run; only C/C++; no tool checks | Run covers Python, C, C++, Rust, JS/TS and Go, with saved configurations, missing-tool guidance and a language picker. The debugger supports Rust and Python, and has a Debug Console. | None once #52 is closed |
| C. Terminal | No input / slow | Input fixed; tabs and shell picker; an idle shell causes 0 wakeups/s. | None |
| D. Extensions | Search/install broken, no empty state | Search, install and the "0 installed" state work. | None |
| E. File/Project management | Can't close/delete/add files; Open Project problems | Close, delete, New File, Add File to Project and the folder picker are in; a lone file no longer opens its folder. | None once #7 is closed |
| F. Navigation & UI stability | Scrollbar jumps, flicker, no hover feedback | Fixed: hover and tooltips (#62), scrollbar, and narrow repaints. | None |
| G. UI/UX & layout | Cramped layout, no tooltips, missing panel tabs | Problems and Debug Console tabs, Settings, shell picker and tooltips are in. | None |
| H. Platform / environment | No macOS, Defender warning, launch error | Runs without the VC++ runtime (#20 fixed in v0.4.0). | #35, #13 item 1 |
| I. Editor QoL | No Python auto-indent, unreliable Prettier | Fixed (#31); formatting pipeline complete. | None once #48 is closed |

---

## 5. Next Steps (suggested)

1. **Push the 2026-10-12 changes, then close #52 and #48** with a short note on what was delivered (section 2).
2. **Close #7** once the lone-file change is released.
3. **#59**: ask @RajDalvi08 whether they are still on it; otherwise it is open for anyone.
4. **Longer term:** #13 (code signing), #35 (macOS port), #43 (onboarding).
