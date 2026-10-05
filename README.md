<div align="center">

<img src="LightLine-icon.png" alt="LightLine icon" width="96" height="96">

# LightLine

**A fast, lightweight code editor for Windows, written in Rust.**

[![Rust](https://github.com/mehmoodulhaq570/LightLine/actions/workflows/rust.yml/badge.svg)](https://github.com/mehmoodulhaq570/LightLine/actions/workflows/rust.yml)
[![Release](https://img.shields.io/github/v/release/mehmoodulhaq570/LightLine?color=blue)](https://github.com/mehmoodulhaq570/LightLine/releases/latest)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2B-0078D6?logo=windows&logoColor=white)](#get-started)
[![Language](https://img.shields.io/badge/language-Rust-DEA584?logo=rust&logoColor=white)](Cargo.toml)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

<br>

<img src="design/screens/welcome.png" alt="LightLine Welcome Screen" width="820">

</div>

---

LightLine is a code editor that feels like VS Code but starts fast and stays light. It's a single portable `.exe`: no installer, no Electron, no admin rights. It draws its own window straight through Windows, so it uses little memory and keeps typing and scrolling smooth.

## Features

- **Smart editing**: syntax colors, code folding, bracket matching, find and replace, word wrap, and a split editor.
- **Rust and Python support**: errors as you type, completions, go to definition, and hover docs, through rust-analyzer and Pyright.
- **Run and debug**: run Python, C/C++, JavaScript, TypeScript, Go or Rust with one key, and debug Rust and Python with breakpoints, stepping and variables.
- **Git built in**: see changed lines in the margin, then stage, commit, push and review diffs without leaving the editor.
- **Terminal**: PowerShell, Command Prompt, Git Bash or WSL, in tabs.
- **Markdown preview**: see your README rendered beside the file as you type.
- **Private AI Assistant (optional)**: ask about your code using a model on your own PC through [Ollama](https://ollama.com). Off until you turn it on.
- **Themes and icons**: install color and icon themes from the Zed extension registry.
- **Picks up where you left off**: your folder, tabs and cursor positions come back when you reopen it.

## Get started

1. Download **`lightline.exe`** from the [latest release](https://github.com/mehmoodulhaq570/LightLine/releases/latest). It needs Windows 10 or 11. On a Windows on ARM PC, such as a Snapdragon laptop, download **`lightline-arm64.exe`** instead.
2. Double-click it. That's it; there's nothing to install.

**Or install it with [Scoop](https://scoop.sh)**, which also keeps it up to date:

```powershell
scoop bucket add lightline https://github.com/mehmoodulhaq570/LightLine
scoop install lightline
```

Update with `scoop update lightline`. Scoop adds LightLine to the Start menu and lets you run `lightline` from any terminal.

LightLine isn't code-signed yet, so Windows may show **"Windows protected your PC"** the first time. Click **More info**, then **Run anyway**. To check your download is genuine first, compare it with the release's `SHA256SUMS.txt` ([how to](docs/USER_GUIDE.md#verifying-a-download)).

**Build it yourself** (needs [Rust](https://rustup.rs)):

```powershell
git clone https://github.com/mehmoodulhaq570/LightLine.git
cd LightLine
cargo run --release
```

## Essential shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl+P` | Open a file by name; type `>` for all commands |
| `Ctrl+Shift+O` | Open a folder |
| `Ctrl+F` / `Ctrl+H` | Find / replace |
| `Ctrl+Shift+F` | Search all files |
| `Ctrl+Shift+R` | Run the current file |
| `F5` | Start debugging |
| `` Ctrl+` `` | Show or hide the terminal |
| `Ctrl+Shift+G` | Source control |
| `Ctrl+B` | Show or hide the side panel |
| `Ctrl+,` | Settings |

See the **[User Guide](docs/USER_GUIDE.md)** for every shortcut and setting, and how to set up Rust, Python, debugging, formatting and the AI Assistant.

## Documentation

- [User Guide](docs/USER_GUIDE.md): shortcuts, settings, language setup, AI Assistant, extensions
- [Changelog](CHANGELOG.md): what's new in each release
- [Contributing](CONTRIBUTING.md) and [Code of Conduct](CODE_OF_CONDUCT.md)
- [Architecture](ARCHITECTURE.md): how LightLine is built

## Privacy

LightLine collects no usage data and sends nothing on its own. It uses the network only when you:

- search for or install extensions;
- push, pull or fetch with Git;
- run **Python: Install debugpy**;
- allow web images in a Markdown preview;
- open a Python file for the first time, when it downloads Pyright.

It writes only to `%APPDATA%\LightLine` and the files you open or edit; **Python: Install debugpy** also installs into your Python. To remove LightLine, delete `lightline.exe` and, if you like, `%APPDATA%\LightLine`.

## Code signing policy

Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org). Signing will start once the project is approved; until then, releases are unsigned.

Release builds of `lightline.exe` are built from this repository by the GitHub Actions [release workflow](.github/workflows/release.yml). Only binaries built from this repository's source are signed.

| Role | Member |
| --- | --- |
| Author (trusted to modify code) | [Mehmood-Ul-Haq](https://github.com/mehmoodulhaq570) |
| Reviewer (reviews external contributions) | [Mehmood-Ul-Haq](https://github.com/mehmoodulhaq570) |
| Approver (approves release signing) | [Mehmood-Ul-Haq](https://github.com/mehmoodulhaq570) |

## License

[MIT](LICENSE)
