# Contributing to LightLine

First off, thanks for taking the time to contribute! ❤️

All types of contributions are encouraged and valued. See the [Table of Contents](#table-of-contents) for different ways to help and details about how this project handles them. Please make sure to read the relevant section before making your contribution. It will make it a lot easier for us maintainers and smooth out the experience for all involved. The community looks forward to your contributions. 🎉

> And if you like the project, but just don't have time to contribute, that's fine. There are other easy ways to support the project and show your appreciation, which we would also be very happy about:
>
> * Star the project
> * Tweet about it
> * Refer this project in your project's README
> * Mention the project at local meetups and tell your friends/colleagues

## Table of Contents

- [Contributing to LightLine](#contributing-to-lightline)
  - [Table of Contents](#table-of-contents)
  - [Code of Conduct](#code-of-conduct)
  - [I Have a Question](#i-have-a-question)
  - [I Want To Contribute](#i-want-to-contribute)
    - [Reporting Bugs](#reporting-bugs)
      - [Before Submitting a Bug Report](#before-submitting-a-bug-report)
      - [How Do I Submit a Good Bug Report?](#how-do-i-submit-a-good-bug-report)
    - [Suggesting Enhancements](#suggesting-enhancements)
      - [Before Submitting an Enhancement](#before-submitting-an-enhancement)
      - [How Do I Submit a Good Enhancement Suggestion?](#how-do-i-submit-a-good-enhancement-suggestion)
    - [Your First Code Contribution](#your-first-code-contribution)
      - [Local Setup](#local-setup)
    - [Improving The Documentation](#improving-the-documentation)
    - [Proposing an Extension for LightLine's List](#proposing-an-extension-for-lightlines-list)
  - [Styleguides](#styleguides)
    - [Code Formatting](#code-formatting)
    - [Commit Messages](#commit-messages)
    - [Updating the Changelog](#updating-the-changelog)
  - [Attribution](#attribution)

## Code of Conduct

This project and everyone participating in it is governed by the [LightLine Code of Conduct](CODE_OF_CONDUCT.md).

By participating, you are expected to uphold this code. Please report unacceptable behavior by opening an issue or contacting the maintainers directly.

## I Have a Question

Before you ask a question, it is best to search for existing [Issues](https://github.com/mehmoodulhaq570/LightLine/issues) that might help you.

In case you have found a suitable issue and still need clarification, you can write your question in this issue. It is also advisable to search the internet for answers first.

If you then still feel the need to ask a question and need clarification, we recommend the following:

* Open an [Issue](https://github.com/mehmoodulhaq570/LightLine/issues/new).
* Provide as much context as you can about what you're running into.
* Provide project and platform versions (Windows OS version, Rust toolchain version), depending on what seems relevant.

We will then take care of the issue as soon as possible.

## I Want To Contribute

> ### Legal Notice
>
> When contributing to this project, you must agree that you have authored 100% of the content, that you have the necessary rights to the content, and that the content you contribute may be provided under the project license.

### Reporting Bugs

#### Before Submitting a Bug Report

A good bug report shouldn't leave others needing to chase you up for more information. Therefore, we ask you to investigate carefully, collect information, and describe the issue in detail in your report.

Please complete the following steps in advance to help us fix any potential bug as fast as possible:

* Make sure that you are using the latest version of the `main` branch.
* Determine if your bug is really a bug and not an error on your side, e.g., missing the Visual Studio C++ Build Tools required for Win32 applications.
* Check whether there is already an existing bug report for your issue in the [bug tracker](https://github.com/mehmoodulhaq570/LightLine/issues?q=label%3Abug).
* Collect information about the bug:

  * Stack trace (traceback) if the Rust app panics.
  * OS, platform, and version (Windows 10 or Windows 11).
  * Version of the Rust compiler (`rustc --version`).
  * Whether you can reliably reproduce the issue.
  * Whether the issue can also be reproduced with older versions.

#### How Do I Submit a Good Bug Report?

We use GitHub Issues to track bugs and errors. If you run into an issue with the project:

* Open an [Issue](https://github.com/mehmoodulhaq570/LightLine/issues/new).
* Explain the behavior you expected and the actual behavior.
* Provide as much context as possible.
* Describe the *reproduction steps* that someone else can follow to recreate the issue.
* Provide the information you collected in the previous section.

### Suggesting Enhancements

This section guides you through submitting an enhancement suggestion for LightLine, **including completely new features and minor improvements to existing functionality**.

#### Before Submitting an Enhancement

* Make sure that you are using the latest version.
* Perform a [search](https://github.com/mehmoodulhaq570/LightLine/issues) to see if the enhancement has already been suggested.
* If it has, add a comment to the existing issue instead of opening a new one.
* Find out whether your idea fits with the scope and aims of the project.
* Keep in mind that we want features that will be useful to the majority of our users and not just a small subset.

#### How Do I Submit a Good Enhancement Suggestion?

Enhancement suggestions are tracked as [GitHub Issues](https://github.com/mehmoodulhaq570/LightLine/issues).

* Use a **clear and descriptive title** for the issue.
* Provide a **step-by-step description** of the suggested enhancement with as much detail as possible.
* **Describe the current behavior** and **explain which behavior you expected to see instead** and why.
* You may want to **include screenshots and animated GIFs** to demonstrate the steps, especially for UI changes.
* **Explain why this enhancement would be useful** to most LightLine users.

### Your First Code Contribution

Because LightLine relies heavily on the Windows API (Win32), you will need a Windows environment to build and run the project natively.

#### Local Setup

1. **Install the Rust Toolchain**

   Install Rust via [rustup](https://rustup.rs/).

2. **Install Windows Build Tools**

   Ensure you have the Visual Studio C++ Build Tools installed.

3. **Fork and Clone the Repository**

   Clone your fork:

   ```bash
   git clone https://github.com/YOUR_USERNAME/LightLine.git
   cd LightLine
   ```

4. **Build and Run the Project**

   ```bash
   cargo run
   ```

5. **Run the Test Suite**

   ```bash
   cargo test
   ```

### Improving The Documentation

If you are updating documentation, please ensure that you check the formatting locally.

We welcome updates to:

* `README.md`
* `ARCHITECTURE.md`
* Inline Rustdoc comments across the codebase
* **Architecture Updates:** If your Pull Request introduces new core modules, major folders, or external dependencies, please ensure you update the Mermaid diagram and directory tree in `ARCHITECTURE.md` to reflect these changes.


### Proposing an Extension for LightLine's List

[`registry/extensions.toml`](registry/extensions.toml) lists the extensions tested with LightLine. The Extensions panel shows them first, with a **LightLine** badge and the description written there, and recommends some of them. The panel reads the file from `main` when it opens, so a merged entry reaches users without a release.

To propose one, add an `[[extension]]` to the file in a pull request:

```toml
[[extension]]
id = "tokyo-night"        # the Zed registry's id, or a new one for your own
from = "zed"              # a Zed registry extension, at the commit Zed pins
kind = "color-theme"      # color-theme, icon-theme, language, snippets or checker
name = "Tokyo Night"
description = "Tokyo Night, Storm, Moon and Light"   # 60 characters at most
publisher = "ssaunderss"
recommended = false       # true shows it under Recommended
```

An extension that isn't in the Zed registry lives in its own public GitHub repository in [Zed's extension format](https://zed.dev/docs/extensions/developing-extensions). It replaces `from = "zed"` with:

```toml
repository = "https://github.com/you/your-extension.git"
commit = "<the full 40-character commit to install>"
version = "1.0.0"
```

Updating it means another pull request that changes `commit` and `version`. `requires = "0.13.0"` hides an entry from LightLine releases older than the one it needs.

An extension that is one folder of a larger repository adds `path`, and only that folder is downloaded:

```toml
path = "themes/lightline-themes"   # relative, with /, no . or ..
```

LightLine's own extensions are all in one repository this way, [lightline-extensions](https://github.com/mehmoodulhaq570/lightline-extensions). To add or change one, open a pull request there first; its CONTRIBUTING.md and `scripts/check.py` say what's checked. Then open a pull request here that adds the entry or moves its `commit`. Older LightLine releases don't know `path`, so such an entry needs `requires = "0.13.0"` or later.

Before merging a change to the list, run `cargo test --lib lightline_list -- --ignored`, which installs every entry.

Before opening the pull request, run:

```powershell
cargo test --lib lightline_list                 # checks the file (CI runs this)
cargo test --lib lightline_list -- --ignored    # installs every entry and checks its kind
```

What the review checks:

* **It works in the latest LightLine release.** You installed it from the panel and used it: the theme looks right, the snippets complete, the checker reports problems.
* **The kind is right.** The second test above installs it and checks.
* **It does only what its kind needs.** An extension never downloads or starts programs itself. A checker's program comes from the user's PATH, or from a release pinned in `src/extensions/tool_download.rs`.
* **Its license allows it to be redistributed.**
* **The description is written for LightLine**, plainly, and not copied from a VS Code listing.

## Styleguides

### Code Formatting

We use standard Rust formatting tools to maintain consistency across the codebase:

* Always run `cargo fmt` before committing to format your code.
* Run `cargo clippy` to catch common anti-patterns and performance issues.
* Ensure your code compiles without warnings using `cargo check`.

### Commit Messages

Please follow these guidelines when writing commit messages:

* Use the present tense ("Add feature" not "Added feature").
* Use the imperative mood ("Move cursor to..." not "Moves cursor to...").
* Limit the first line to **72 characters or less**.
* Reference issues and pull requests liberally after the first line (e.g., `Closes #1`).

### Updating the Changelog

If your Pull Request introduces a new feature, fixes a bug, or makes a breaking change, please add a brief note to the `CHANGELOG.md` file under the **[Unreleased]** section (or the current working version). 

* Keep the description concise.
* Include a link to your Pull Request or Issue number.
* Example: `- Added a feature to discard the active project (#1).`

## Attribution

This contributing guide is based on common open-source contribution guidelines and has been adapted for the LightLine project.