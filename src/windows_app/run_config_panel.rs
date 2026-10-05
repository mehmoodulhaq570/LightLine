use super::*;
use lightline::run_config::Configurations;

impl App {
    pub(super) fn run_configuration_root(&self) -> Option<PathBuf> {
        self.workspace_root
            .clone()
            .or_else(|| {
                self.doc()
                    .path
                    .as_deref()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf)
            })
            .map(|root| PathBuf::from(display_path(&root)))
    }

    pub(super) fn prepare_configured_file(
        &mut self,
        file: &Path,
    ) -> Result<lightline::runner::RunPlan, String> {
        use lightline::runner::{RunCommand, RunPlan};
        let file = PathBuf::from(display_path(file));
        let extension = file
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if extension == "py" || extension == "pyw" {
            let root = workflow::python_project_root(&file, self.workspace_root.as_deref());
            let interpreter = self
                .resolve_python_interpreter(&root)
                .ok_or("Select a Python interpreter before running")?;
            return Ok(RunPlan {
                cwd: PathBuf::from(display_path(&root)),
                commands: vec![RunCommand {
                    program: PathBuf::from(display_path(&interpreter)),
                    arguments: vec!["-u".into(), file.into()],
                }],
            });
        }
        if matches!(extension.as_str(), "c" | "cc" | "cpp" | "cxx") {
            let cpp = extension != "c";
            let compiler = workflow::detect_c_compiler(cpp)
                .ok_or("Install GCC/MinGW or Clang before running C/C++")?;
            let folder = file.parent().ok_or("The file has no parent directory")?;
            let output_dir = folder.join(".lightline-run");
            std::fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;
            let mut name = file.file_stem().ok_or("Invalid filename")?.to_os_string();
            name.push(".exe");
            let output = output_dir.join(name);
            return Ok(RunPlan {
                cwd: folder.to_path_buf(),
                commands: vec![
                    RunCommand {
                        program: workflow::resolve_command(compiler).ok_or("Compiler not found")?,
                        arguments: vec![file.into(), "-o".into(), output.clone().into()],
                    },
                    RunCommand {
                        program: output,
                        arguments: Vec::new(),
                    },
                ],
            });
        }
        lightline::runner::prepare(&file)
    }

    pub(super) fn run_selected_configuration(&mut self, hwnd: HWND) -> bool {
        let Some(root) = self.run_configuration_root() else {
            return false;
        };
        let result = (|| {
            let saved = Configurations::load(&root)?;
            let Some(config) = saved.active() else {
                return Ok(None);
            };
            if self.doc().is_dirty() && !self.tab().read_only() && !self.save(hwnd, false) {
                return Err("Save the file before running this configuration".into());
            }
            // Keep unsaved editor changes out of the launch, including an entry tab.
            let entry = config.entry(&root, self.doc().path.as_deref()).ok();
            if let Some(entry) = &entry {
                for index in 0..self.tabs.len() {
                    if self.tabs[index]
                        .document
                        .path
                        .as_deref()
                        .is_some_and(|path| {
                            std::fs::canonicalize(path)
                                .ok()
                                .zip(std::fs::canonicalize(entry).ok())
                                .is_some_and(|(a, b)| a == b)
                        })
                        && self.tabs[index].document.is_dirty()
                    {
                        return Err("Save the entry file before running this configuration".into());
                    }
                }
            }
            let mut plan = match config.command_plan(&root)? {
                Some(plan) => plan,
                None => {
                    self.prepare_configured_file(&config.entry(&root, self.doc().path.as_deref())?)?
                }
            };
            config.apply(&root, &mut plan)?;
            let command = plan.powershell_command_with_environment(&config.environment)?;
            Ok(Some((plan.cwd, command, config.name.clone())))
        })();
        match result {
            Ok(None) => false,
            Ok(Some((cwd, command, name))) => {
                self.poll_terminal(hwnd);
                let was_running = self.run_session.is_some();
                self.start_language_run(hwnd, cwd, command);
                if !was_running && self.run_session.is_some() {
                    self.status = format!("Running {name}...");
                }
                self.refresh(hwnd);
                true
            }
            Err(error) => {
                self.status = error;
                self.refresh(hwnd);
                true
            }
        }
    }
}
