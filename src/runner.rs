//! Direct language runners shared by the editor and live terminal tests.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    JavaScript,
    TypeScript,
    Go,
    Rust,
}

impl Language {
    pub fn for_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "js" | "mjs" | "cjs" => Some(Self::JavaScript),
            "ts" | "mts" | "cts" | "tsx" | "jsx" => Some(Self::TypeScript),
            "go" => Some(Self::Go),
            "rs" => Some(Self::Rust),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RunCommand {
    pub program: PathBuf,
    pub arguments: Vec<OsString>,
}

#[derive(Clone, Debug)]
pub struct RunPlan {
    pub cwd: PathBuf,
    pub commands: Vec<RunCommand>,
}

impl RunPlan {
    /// Paths and arguments are encoded as data, never interpolated as shell
    /// syntax. A failed compile exits before the old executable can run.
    pub fn powershell_command(&self) -> Result<String, String> {
        self.powershell_command_with_environment(&std::collections::BTreeMap::new())
    }

    pub fn powershell_command_with_environment(
        &self,
        environment: &std::collections::BTreeMap<String, String>,
    ) -> Result<String, String> {
        use crate::terminal::powershell_path_expression as quoted;
        let mut script = format!(
            "$ErrorActionPreference = 'Stop'; try {{ Set-Location -LiteralPath {} -ErrorAction Stop; ",
            quoted(&self.cwd)?
        );
        for (name, value) in environment {
            if name.is_empty() || name.contains(['=', '\0']) || value.contains('\0') {
                return Err("Invalid environment variable name or value".into());
            }
            script.push_str(&format!(
                "[Environment]::SetEnvironmentVariable({}, {}, 'Process'); ",
                quoted(Path::new(name))?,
                quoted(Path::new(value))?
            ));
        }
        for command in &self.commands {
            script.push_str(&format!("& {}", quoted(&command.program)?));
            for argument in &command.arguments {
                script.push(' ');
                script.push_str(&quoted(Path::new(argument))?);
            }
            script.push_str("; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; ");
        }
        script.push_str("exit 0 } catch { Write-Error $_ -ErrorAction Continue; exit 1 }");
        Ok(script)
    }
}

fn normal_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn nearest(file: &Path, markers: &[&str]) -> Option<PathBuf> {
    file.parent()?
        .ancestors()
        .find(|directory| {
            markers
                .iter()
                .any(|marker| directory.join(marker).is_file())
        })
        .map(Path::to_path_buf)
}

pub fn prepare(file: &Path) -> Result<RunPlan, String> {
    prepare_with(file, crate::workflow::resolve_command)
}

fn prepare_with(file: &Path, resolve: impl Fn(&str) -> Option<PathBuf>) -> Result<RunPlan, String> {
    let file = normal_path(&std::path::absolute(file).map_err(|error| error.to_string())?);
    if !file.is_file() {
        return Err("Save the file before running it".into());
    }
    let language = Language::for_path(&file).ok_or("This file type cannot be run")?;
    let cwd = file
        .parent()
        .ok_or("The file has no parent directory")?
        .to_path_buf();
    let require = |name: &str, hint: &str| {
        resolve(name).ok_or_else(|| format!("{name} was not found on PATH. {hint}"))
    };
    let single = |cwd, program, arguments| RunPlan {
        cwd,
        commands: vec![RunCommand { program, arguments }],
    };
    match language {
        Language::JavaScript | Language::TypeScript => {
            let project = nearest(&file, &["package.json"]);
            if let Some(root) = &project {
                let package = std::fs::read(root.join("package.json"))
                    .map_err(|error| format!("Could not read package.json: {error}"))?;
                let package: serde_json::Value = serde_json::from_slice(&package)
                    .map_err(|error| format!("Invalid package.json: {error}"))?;
                for task in ["start", "dev"] {
                    if package
                        .get("scripts")
                        .and_then(|scripts| scripts.get(task))
                        .and_then(|value| value.as_str())
                        .is_some_and(|value| !value.trim().is_empty())
                    {
                        return Ok(single(
                            root.clone(),
                            require(
                                "npm",
                                "Install Node.js from https://nodejs.org/ (includes node and npm), then restart LightLine.",
                            )?,
                            vec!["run".into(), task.into()],
                        ));
                    }
                }
            }
            if language == Language::JavaScript {
                Ok(single(
                    cwd,
                    require(
                        "node",
                        "Install Node.js from https://nodejs.org/ (includes node and npm), then restart LightLine.",
                    )?,
                    vec![file.into()],
                ))
            } else {
                require(
                    "node",
                    "Install Node.js from https://nodejs.org/ (includes node and npm), then restart LightLine.",
                )?;
                // Search every enclosing package: a monorepo often hoists tsx.
                let local =
                    file.parent()
                        .into_iter()
                        .flat_map(Path::ancestors)
                        .find_map(|root| {
                            let executable = root
                                .join("node_modules/.bin")
                                .join(if cfg!(windows) { "tsx.cmd" } else { "tsx" });
                            executable.is_file().then_some(executable)
                        });
                let tsx = local.or_else(|| resolve("tsx"))
                    .ok_or("tsx was not found. Install it in the project with `npm install --save-dev tsx`, or globally with `npm install -g tsx`.")?;
                Ok(single(project.unwrap_or(cwd), tsx, vec![file.into()]))
            }
        }
        Language::Go => {
            let go = require(
                "go",
                "Install Go from https://go.dev/dl/, then restart LightLine.",
            )?;
            let target = if nearest(&file, &["go.mod", "go.work"]).is_some() {
                OsString::from(".")
            } else {
                file.into_os_string()
            };
            Ok(single(cwd, go, vec!["run".into(), target]))
        }
        Language::Rust => {
            if let Some(root) = nearest(&file, &["Cargo.toml"]) {
                let manifest = root.join("Cargo.toml");
                let content =
                    std::fs::read_to_string(&manifest).map_err(|error| error.to_string())?;
                let config: toml::Value = toml::from_str(&content)
                    .map_err(|error| format!("Invalid Cargo.toml: {error}"))?;
                let relative = file.strip_prefix(&root).unwrap_or(&file);
                let mut target = Vec::new();
                let mut belongs = relative.starts_with("src") || relative.starts_with("examples");
                for (table, flag) in [("bin", "--bin"), ("example", "--example")] {
                    if let Some(entries) = config.get(table).and_then(toml::Value::as_array) {
                        for entry in entries {
                            if entry
                                .get("path")
                                .and_then(toml::Value::as_str)
                                .is_some_and(|path| root.join(path) == file)
                                && let Some(name) = entry.get("name").and_then(toml::Value::as_str)
                            {
                                target = vec![flag.into(), OsString::from(name)];
                                belongs = true;
                            }
                        }
                    }
                }
                if belongs {
                    if target.is_empty() {
                        for (directory, flag) in [
                            (root.join("src/bin"), "--bin"),
                            (root.join("examples"), "--example"),
                        ] {
                            if let Ok(relative) = file.strip_prefix(directory) {
                                let mut components = relative.components();
                                if let Some(first) = components.next() {
                                    let name = if components.next().is_some() {
                                        first.as_os_str()
                                    } else {
                                        file.file_stem().unwrap_or_default()
                                    };
                                    target = vec![flag.into(), name.into()];
                                }
                            }
                        }
                    }
                    let mut arguments =
                        vec!["run".into(), "--manifest-path".into(), manifest.into()];
                    arguments.extend(target);
                    return Ok(single(
                        root,
                        require(
                            "cargo",
                            "Install Rust through https://rustup.rs/ (includes cargo and rustc), then restart LightLine.",
                        )?,
                        arguments,
                    ));
                }
            }
            let rustc = require(
                "rustc",
                "Install Rust through https://rustup.rs/ (includes cargo and rustc), then restart LightLine.",
            )?;
            let directory = cwd.join(".lightline-run");
            std::fs::create_dir_all(&directory)
                .map_err(|error| format!("Could not create run output directory: {error}"))?;
            let mut output_name = file
                .file_stem()
                .ok_or("Invalid Rust filename")?
                .to_os_string();
            if cfg!(windows) {
                output_name.push(".exe");
            }
            let output = directory.join(output_name);
            Ok(RunPlan {
                cwd,
                commands: vec![
                    RunCommand {
                        program: rustc,
                        arguments: vec![
                            "--edition=2024".into(),
                            "--crate-name".into(),
                            "lightline_run".into(),
                            file.into(),
                            "-o".into(),
                            output.clone().into(),
                        ],
                    },
                    RunCommand {
                        program: output,
                        arguments: Vec::new(),
                    },
                ],
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn fixture() -> PathBuf {
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!(
                "runner-unit-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn write(root: &Path, name: &str, text: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    fn tool(name: &str) -> Option<PathBuf> {
        Some(PathBuf::from(format!("{name}.exe")))
    }

    #[test]
    fn package_scripts_take_precedence_and_use_project_root() {
        let root = fixture();
        write(
            &root,
            "package.json",
            r#"{"scripts":{"start":"node entry.js","dev":"tsx watch entry.ts"}}"#,
        );
        let file = write(&root, "src/helper.ts", "export const value = 42;");
        let plan = prepare_with(&file, tool).unwrap();
        assert_eq!(plan.cwd, root);
        assert_eq!(plan.commands[0].program, Path::new("npm.exe"));
        assert_eq!(plan.commands[0].arguments, ["run", "start"]);
    }

    #[test]
    fn typescript_prefers_project_local_tsx_and_javascript_uses_node() {
        let root = fixture();
        write(&root, "package.json", "{}");
        let executable = write(
            &root,
            if cfg!(windows) {
                "node_modules/.bin/tsx.cmd"
            } else {
                "node_modules/.bin/tsx"
            },
            "",
        );
        let ts = write(&root, "src/main.tsx", "const element = <div/>;");
        let plan = prepare_with(&ts, tool).unwrap();
        assert_eq!(plan.commands[0].program, executable);
        assert_eq!(plan.cwd, root);
        let js = write(&root, "main.mjs", "console.log(42)");
        assert_eq!(
            prepare_with(&js, tool).unwrap().commands[0].program,
            Path::new("node.exe")
        );
    }

    #[test]
    fn go_runs_whole_package_in_a_module_and_standalone_file_otherwise() {
        let root = fixture();
        let file = write(&root, "cmd/tool/main.go", "package main");
        let standalone = prepare_with(&file, tool).unwrap();
        assert_eq!(Path::new(&standalone.commands[0].arguments[1]), file);
        write(&root, "go.mod", "module example.com/tool\n");
        let package = prepare_with(&file, tool).unwrap();
        assert_eq!(package.cwd, file.parent().unwrap());
        assert_eq!(package.commands[0].arguments, ["run", "."]);
    }

    #[test]
    fn rust_uses_cargo_targets_but_compiles_loose_files_separately() {
        let root = fixture();
        write(
            &root,
            "Cargo.toml",
            "[package]\nname = 'runner-test'\nversion = '0.1.0'\n",
        );
        for (name, flag, target) in [
            ("src/bin/tool.rs", "--bin", "tool"),
            ("examples/demo/main.rs", "--example", "demo"),
        ] {
            let file = write(&root, name, "fn main() {}");
            let plan = prepare_with(&file, tool).unwrap();
            assert_eq!(plan.commands[0].program, Path::new("cargo.exe"));
            assert_eq!(
                &plan.commands[0].arguments[3..],
                [OsString::from(flag), OsString::from(target)]
            );
        }
        let loose = write(&root, "loose file.test.rs", "fn main() {}");
        let plan = prepare_with(&loose, tool).unwrap();
        assert_eq!(plan.commands.len(), 2);
        assert_eq!(plan.commands[0].program, Path::new("rustc.exe"));
        assert_eq!(
            plan.commands[1].program.file_name().unwrap(),
            if cfg!(windows) {
                "loose file.test.exe"
            } else {
                "loose file.test"
            }
        );
        assert_eq!(
            plan.commands[1].program.parent().unwrap(),
            root.join(".lightline-run")
        );
    }

    #[test]
    fn unsupported_files_and_missing_tools_report_errors() {
        let root = fixture();
        let json = write(&root, "data.json", "{}");
        assert!(
            prepare_with(&json, tool)
                .unwrap_err()
                .contains("cannot be run")
        );
        let js = write(&root, "main.js", "console.log(42)");
        assert!(
            prepare_with(&js, |_| None)
                .unwrap_err()
                .contains("node was not found")
        );
        assert!(
            prepare_with(&root.join("missing.ts"), tool)
                .unwrap_err()
                .contains("Save")
        );
    }
}
