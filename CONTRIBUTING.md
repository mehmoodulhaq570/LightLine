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