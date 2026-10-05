//! Workspace-local, saved launch choices. Commands and arguments are data.
use crate::runner::{RunCommand, RunPlan};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Configuration {
    pub name: String,
    /// Empty uses the active file. Relative paths use the workspace root.
    pub entry_file: String,
    /// Empty detects the language; otherwise an executable, without shell syntax.
    pub command: String,
    pub arguments: Vec<String>,
    pub working_directory: String,
    pub environment: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Configurations {
    /// None means Automatic. Names survive reordering and editing.
    pub selected: Option<String>,
    pub configurations: Vec<Configuration>,
}

impl Configurations {
    pub fn path(root: &Path) -> PathBuf {
        root.join(".lightline/run.json")
    }

    pub fn load(root: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(Self::path(root)) {
            Ok(text) => {
                let result: Self = serde_json::from_str(&text)
                    .map_err(|e| format!("Invalid .lightline/run.json: {e}"))?;
                result.validate()?;
                Ok(result)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Could not read run configurations: {e}")),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut names = std::collections::HashSet::new();
        for config in &self.configurations {
            if config.name.trim().is_empty() || !names.insert(&config.name) {
                return Err("Each run configuration needs a unique, nonempty name".into());
            }
            if config
                .environment
                .iter()
                .any(|(k, v)| k.is_empty() || k.contains(['=', '\0']) || v.contains('\0'))
            {
                return Err(
                    "Environment names must be nonempty and cannot contain '=' or NUL".into(),
                );
            }
            if config.entry_file.contains('\0')
                || config.command.contains('\0')
                || config.working_directory.contains('\0')
                || config.arguments.iter().any(|a| a.contains('\0'))
            {
                return Err("Run configuration fields cannot contain NUL".into());
            }
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|name| !names.contains(name))
        {
            return Err("The selected run configuration no longer exists; choose Automatic or an existing name".into());
        }
        Ok(())
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        self.validate()?;
        let path = Self::path(root);
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        // Never silently replace a malformed hand-edited configuration file.
        Self::load(root)?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("Could not save run configurations: {e}"))
    }

    pub fn active(&self) -> Option<&Configuration> {
        self.selected
            .as_ref()
            .and_then(|name| self.configurations.iter().find(|c| &c.name == name))
    }
}

impl Configuration {
    pub fn entry(&self, root: &Path, active: Option<&Path>) -> Result<PathBuf, String> {
        if self.entry_file.trim().is_empty() {
            active
                .map(Path::to_path_buf)
                .ok_or_else(|| "Open a file or set an entry file".into())
        } else {
            Ok(root.join(&self.entry_file))
        }
    }

    pub fn command_plan(&self, root: &Path) -> Result<Option<RunPlan>, String> {
        if self.command.trim().is_empty() {
            return Ok(None);
        }
        let candidate = root.join(&self.command);
        let program = if candidate.is_file() {
            candidate
        } else {
            crate::workflow::resolve_command(&self.command).ok_or_else(|| {
                format!(
                    "{} was not found. Set an executable path or install it on PATH.",
                    self.command
                )
            })?
        };
        Ok(Some(RunPlan {
            cwd: root.to_path_buf(),
            commands: vec![RunCommand {
                program,
                arguments: Vec::new(),
            }],
        }))
    }

    pub fn apply(&self, root: &Path, plan: &mut RunPlan) -> Result<(), String> {
        if !self.working_directory.trim().is_empty() {
            plan.cwd = root.join(&self.working_directory);
        }
        if !plan.cwd.is_dir() {
            return Err(format!(
                "Working directory does not exist: {}",
                plan.cwd.display()
            ));
        }
        let last = plan
            .commands
            .last_mut()
            .ok_or("Run configuration has no command")?;
        if !self.arguments.is_empty() {
            let tool = last
                .program
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            // cargo/npm consume their own options unless application arguments follow --.
            if self.command.trim().is_empty() && (tool == "cargo" || tool == "npm") {
                last.arguments.push("--".into());
            }
            last.arguments.extend(self.arguments.iter().map(Into::into));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arguments_go_to_program_and_compile_step_is_untouched() {
        let root = std::env::current_dir().unwrap();
        let config = Configuration {
            arguments: vec!["hello world".into(), "$(bad)".into()],
            ..Default::default()
        };
        let mut plan = RunPlan {
            cwd: root.clone(),
            commands: vec![
                RunCommand {
                    program: "rustc".into(),
                    arguments: vec!["source.rs".into()],
                },
                RunCommand {
                    program: "program.exe".into(),
                    arguments: vec![],
                },
            ],
        };
        config.apply(&root, &mut plan).unwrap();
        assert_eq!(plan.commands[0].arguments.len(), 1);
        assert_eq!(plan.commands[1].arguments[0], "hello world");
        plan.commands = vec![RunCommand {
            program: "cargo.exe".into(),
            arguments: vec!["run".into()],
        }];
        config.apply(&root, &mut plan).unwrap();
        assert_eq!(plan.commands[0].arguments[1], "--");
    }
    #[test]
    fn persistence_selection_and_invalid_file_preservation() {
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("run-config-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(Configurations::path(&root), "{}")
            .or_else(|_| {
                std::fs::create_dir_all(root.join(".lightline"))?;
                std::fs::write(Configurations::path(&root), "{}")
            })
            .unwrap();
        let saved = Configurations {
            selected: Some("Demo".into()),
            configurations: vec![Configuration {
                name: "Demo".into(),
                entry_file: "app.js".into(),
                environment: BTreeMap::from([("DEMO".into(), "a b '$()".into())]),
                ..Default::default()
            }],
        };
        saved.save(&root).unwrap();
        let loaded = Configurations::load(&root).unwrap();
        assert_eq!(
            loaded.active().unwrap().entry(&root, None).unwrap(),
            root.join("app.js")
        );
        assert_eq!(loaded.active().unwrap().environment["DEMO"], "a b '$()");
        std::fs::write(Configurations::path(&root), "{broken").unwrap();
        assert!(saved.save(&root).is_err());
        assert_eq!(
            std::fs::read_to_string(Configurations::path(&root)).unwrap(),
            "{broken"
        );
    }
    #[test]
    fn rejects_invalid_selection_names_environment_and_directory() {
        let mut saved = Configurations {
            selected: Some("missing".into()),
            ..Default::default()
        };
        assert!(saved.validate().is_err());
        saved.selected = None;
        saved.configurations = vec![Configuration::default()];
        assert!(saved.validate().is_err());
        saved.configurations[0].name = "Test".into();
        saved.configurations[0]
            .environment
            .insert("A=B".into(), "x".into());
        assert!(saved.validate().is_err());
        let config = Configuration {
            working_directory: "nonexistent-run-directory".into(),
            ..Default::default()
        };
        let mut plan = RunPlan {
            cwd: ".".into(),
            commands: vec![],
        };
        assert!(config.apply(Path::new("."), &mut plan).is_err());
    }
}
