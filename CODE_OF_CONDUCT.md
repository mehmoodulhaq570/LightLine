# Contributing to LightLine

First off, thanks for taking the time to contribute! ❤️

All types of contributions are encouraged and valued. See the [Table of Contents](#table-of-contents) for different ways to help and details about how this project handles them.

Please make sure to read the relevant section before making your contribution. It will make it a lot easier for us maintainers and smooth out the experience for all involved.

The community looks forward to your contributions. 🎉

> And if you like the project, but just don't have time to contribute, that's fine. There are other easy ways to support the project and show your appreciation, which we would also be very happy about:
>
> * ⭐ Star the project
> * 🐦 Tweet about it
> * 📖 Refer to this project in your project's README
> * 🗣️ Mention the project at local meetups and tell your friends/colleagues

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
    - [Local Development Setup](#local-development-setup)
      - [1. Install Rust](#1-install-rust)
      - [2. Install Windows Build Tools](#2-install-windows-build-tools)
      - [3. Fork and Clone](#3-fork-and-clone)
      - [4. Build and Run](#4-build-and-run)
      - [5. Run Tests](#5-run-tests)
  - [Styleguides](#styleguides)
    - [Code Formatting](#code-formatting)
    - [Commit Messages](#commit-messages)

## Code of Conduct

This project and everyone participating in it is governed by the [LightLine Code of Conduct](CODE_OF_CONDUCT.md).

By participating, you are expected to uphold this code.

Please report unacceptable behavior by opening an issue or contacting the maintainers directly.

## I Have a Question

Before you ask a question, it is best to search for existing [Issues](https://github.com/mehmoodulhaq570/LightLine/issues) that might help you.

If you find a suitable issue and still need clarification, you can write your question in that issue.

If you still feel the need to ask a question, we recommend the following:

* Open an [Issue](https://github.com/mehmoodulhaq570/LightLine/issues/new).
* Provide as much context as possible about what you're running into.
* Provide your OS and Rust toolchain versions.

## I Want To Contribute

> ### Legal Notice
>
> When contributing to this project, you must agree that you have authored 100% of the content, that you have the necessary rights to the content, and that the content you contribute may be provided under the project license.

### Reporting Bugs

#### Before Submitting a Bug Report

Before submitting a bug report:

* Make sure that you are using the latest version of the `main` branch.
* Check whether there is already an existing bug report for your issue in the [bug tracker](https://github.com/mehmoodulhaq570/LightLine/issues?q=label%3Abug).
* Collect information about the bug:

  * OS, platform, and version (Windows 10/11)
  * Version of the Rust compiler (`rustc --version`)
  * Stack trace (traceback) if the application panics
  * Whether the issue can be reliably reproduced

#### How Do I Submit a Good Bug Report?

When submitting a bug report:

* Open an [Issue](https://github.com/mehmoodulhaq570/LightLine/issues/new).
* Explain the behavior you expected and the actual behavior.
* Provide as much context as possible.
* Describe the **reproduction steps** that someone else can follow to recreate the issue.
* Include screenshots or GIFs if the bug involves the UI.

### Suggesting Enhancements

#### Before Submitting an Enhancement

Before submitting an enhancement:

* Perform a [search](https://github.com/mehmoodulhaq570/LightLine/issues) to see if the enhancement has already been suggested.
* If it has, add a comment to the existing issue instead of opening a new one.
* Keep in mind that we want features that will be useful to the majority of our users, not just a small subset.

#### How Do I Submit a Good Enhancement Suggestion?

When submitting an enhancement:

* Use a **clear and descriptive title** for the issue.
* Provide a **step-by-step description** of the suggested enhancement with as much detail as possible.
* **Explain why this enhancement would be useful** to most LightLine users.

### Your First Code Contribution

Unsure where to begin contributing to LightLine?

You can start by looking through these issue categories:

* [Good First Issues](https://github.com/mehmoodulhaq570/LightLine/labels/good%20first%20issue) — issues that should only require a few lines of code and a test or two.
* [Help Wanted Issues](https://github.com/mehmoodulhaq570/LightLine/labels/help%20wanted) — issues that are a bit more involved than `good first issue`s.

### Local Development Setup

Because LightLine relies heavily on the Windows API (Win32) and native GUI rendering, you will need a Windows environment to build and run the project natively.

#### 1. Install Rust

Install the Rust toolchain via [rustup](https://rustup.rs/).

#### 2. Install Windows Build Tools

Ensure you have the Visual Studio C++ Build Tools installed.

You are usually prompted to install these during the Rust installation on Windows.

#### 3. Fork and Clone

Fork the repository and clone your fork:

```bash
git clone https://github.com/YOUR_USERNAME/LightLine.git
cd LightLine
```

#### 4. Build and Run

Build and run the project with:

```bash
cargo run
```

#### 5. Run Tests

Run the test suite with:

```bash
cargo test
```

## Styleguides

### Code Formatting

This project relies on standard Rust formatting and linting tools.

Before submitting a pull request, please ensure your code passes the standard checks:

* Run `cargo fmt` to format your code.
* Run `cargo clippy` to check for common Rust anti-patterns.
* Ensure `cargo check` passes locally.
* Ensure `cargo test` passes locally.

Our CI pipeline will automatically run these checks on your pull request.

### Commit Messages

Please follow these guidelines when writing commit messages:

* Use the present tense:

  * ✅ `Add feature`
  * ❌ `Added feature`
* Use the imperative mood:

  * ✅ `Move cursor to...`
  * ❌ `Moves cursor to...`
* Keep the first line concise and descriptive.
* Reference issues and pull requests when appropriate after the first line.

For example:

```text
Add support for custom cursor colors

Closes #1
```
