use super::*;
use lightline::run_config::{Configuration, Configurations};

struct Form {
    root: PathBuf,
    saved: Configurations,
    choice: HWND,
    fields: Vec<HWND>,
    error: HWND,
    editing: Option<usize>,
    new: bool,
    done: bool,
    changed: bool,
    theme: Theme,
    background: HBRUSH,
    field_background: HBRUSH,
}

impl Drop for Form {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.background);
            DeleteObject(self.field_background);
        }
    }
}

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

    pub(super) fn show_run_configurations(&mut self, hwnd: HWND) {
        let Some(root) = self.run_configuration_root() else {
            self.status = "Open a folder or save a file before creating Run configurations".into();
            self.refresh(hwnd);
            return;
        };
        let saved = match Configurations::load(&root) {
            Ok(saved) => saved,
            Err(error) => {
                self.status = error;
                self.refresh(hwnd);
                return;
            }
        };
        if show_form(hwnd, self.ui_font, self.dpi, &self.theme, root, saved) {
            self.status = "Run configuration saved. Use Run or Ctrl+Shift+R.".into();
        }
        self.refresh(hwnd);
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

fn text(hwnd: HWND) -> String {
    unsafe {
        let mut buffer = vec![0u16; GetWindowTextLengthW(hwnd) as usize + 1];
        let count = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..count as usize])
    }
}

impl Form {
    unsafe fn populate(&mut self) {
        unsafe {
            SendMessageW(self.choice, CB_RESETCONTENT, 0, 0);
            SendMessageW(
                self.choice,
                CB_ADDSTRING,
                0,
                wide("Automatic (active file)").as_ptr() as isize,
            );
            for config in &self.saved.configurations {
                SendMessageW(
                    self.choice,
                    CB_ADDSTRING,
                    0,
                    wide(&config.name).as_ptr() as isize,
                );
            }
            let index = self.saved.selected.as_ref().and_then(|name| {
                self.saved
                    .configurations
                    .iter()
                    .position(|c| &c.name == name)
            });
            SendMessageW(self.choice, CB_SETCURSEL, index.map_or(0, |i| i + 1), 0);
            self.load(index, false);
        }
    }

    unsafe fn load(&mut self, index: Option<usize>, new: bool) {
        self.editing = index;
        self.new = new;
        let config = index
            .map(|i| self.saved.configurations[i].clone())
            .unwrap_or_default();
        let values = [
            config.name,
            config.entry_file,
            config.command,
            config.working_directory,
            config.arguments.join("\r\n"),
            config
                .environment
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("\r\n"),
        ];
        unsafe {
            for (field, value) in self.fields.iter().zip(values) {
                SetWindowTextW(*field, wide(&value).as_ptr());
                EnableWindow(*field, i32::from(index.is_some() || new));
            }
            SetWindowTextW(self.error, wide("").as_ptr());
        }
    }

    fn save(&mut self) -> Result<(), String> {
        let mut saved = self.saved.clone();
        if self.editing.is_none() && !self.new {
            saved.selected = None;
        } else {
            let config = Configuration {
                name: text(self.fields[0]).trim().to_string(),
                entry_file: text(self.fields[1]).trim().to_string(),
                command: text(self.fields[2]).trim().to_string(),
                working_directory: text(self.fields[3]).trim().to_string(),
                arguments: text(self.fields[4])
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(str::to_string)
                    .collect(),
                environment: {
                    let mut environment = std::collections::BTreeMap::new();
                    for line in text(self.fields[5])
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                    {
                        let (name, value) = line
                            .split_once('=')
                            .ok_or("Each environment line must be NAME=value")?;
                        let name = name.trim();
                        if environment
                            .insert(name.to_string(), value.to_string())
                            .is_some()
                        {
                            return Err(format!("Duplicate environment variable: {name}"));
                        }
                    }
                    environment
                },
            };
            saved.selected = Some(config.name.clone());
            if let Some(index) = self.editing {
                saved.configurations[index] = config;
            } else {
                saved.configurations.push(config);
            }
        }
        saved.save(&self.root)?;
        self.saved = saved;
        self.changed = true;
        Ok(())
    }
}

unsafe extern "system" fn form_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Form;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        let form = &mut *ptr;
        match message {
            WM_ERASEBKGND => {
                let mut rect = RECT::default();
                GetClientRect(hwnd, &mut rect);
                FillRect(wparam as HDC, &rect, form.background);
                1
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
                let hdc = wparam as HDC;
                let field = message != WM_CTLCOLORSTATIC;
                SetTextColor(hdc, form.theme.text);
                SetBkColor(
                    hdc,
                    if field {
                        form.theme.editor_bg
                    } else {
                        form.theme.sidebar_bg
                    },
                );
                if field {
                    form.field_background as isize
                } else {
                    form.background as isize
                }
            }
            WM_COMMAND => {
                let id = wparam & 0xffff;
                let notification = (wparam >> 16) & 0xffff;
                match id {
                    100 if notification == CBN_SELCHANGE as usize => {
                        let index = SendMessageW(form.choice, CB_GETCURSEL, 0, 0);
                        form.load(
                            if index > 0 {
                                Some(index as usize - 1)
                            } else {
                                None
                            },
                            false,
                        );
                    }
                    1 => match form.save() {
                        Ok(()) => {
                            form.done = true;
                            DestroyWindow(hwnd);
                        }
                        Err(error) => {
                            SetWindowTextW(form.error, wide(&error).as_ptr());
                        }
                    },
                    2 => {
                        form.done = true;
                        DestroyWindow(hwnd);
                    }
                    3 => {
                        SendMessageW(form.choice, CB_SETCURSEL, 0, 0);
                        form.load(None, true);
                        SetFocus(form.fields[0]);
                    }
                    4 => {
                        if let Some(index) = form.editing {
                            let mut saved = form.saved.clone();
                            let removed = saved.configurations.remove(index);
                            if saved.selected.as_ref() == Some(&removed.name) {
                                saved.selected = None;
                            }
                            match saved.save(&form.root) {
                                Ok(()) => {
                                    form.saved = saved;
                                    form.changed = true;
                                    form.populate();
                                }
                                Err(error) => {
                                    SetWindowTextW(form.error, wide(&error).as_ptr());
                                }
                            }
                        }
                    }
                    _ => {}
                }
                0
            }
            WM_CLOSE => {
                form.done = true;
                DestroyWindow(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

fn show_form(
    owner: HWND,
    font: HFONT,
    dpi: u32,
    theme: &Theme,
    root: PathBuf,
    saved: Configurations,
) -> bool {
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = wide("LightLineRunConfigurations");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(form_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            ..zeroed()
        };
        RegisterClassW(&wc);
        let s = |v| scaled(v, dpi, 100);
        let mut owner_rect = RECT::default();
        GetWindowRect(owner, &mut owner_rect);
        let hwnd = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            class.as_ptr(),
            wide("Run Configurations").as_ptr(),
            WS_CAPTION | WS_SYSMENU | WS_POPUP,
            owner_rect.left + s(60),
            owner_rect.top + s(50),
            s(730),
            s(700),
            owner,
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return false;
        }
        let color = theme.sidebar_bg;
        let dark: i32 =
            i32::from((color & 255) + ((color >> 8) & 255) + ((color >> 16) & 255) < 384);
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &dark as *const i32 as *const _,
            size_of::<i32>() as u32,
        );
        let control = |class: &str, value: &str, style: u32, id: usize, x, y, width, height| {
            let child = CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(value).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                s(x),
                s(y),
                s(width),
                s(height),
                hwnd,
                id as HMENU,
                instance,
                null(),
            );
            SendMessageW(child, WM_SETFONT, font as usize, 1);
            child
        };
        control(
            "STATIC",
            "Select a saved configuration, or use Automatic for the active file.",
            0,
            0,
            20,
            15,
            680,
            25,
        );
        let choice = control(
            "COMBOBOX",
            "",
            WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
            100,
            20,
            45,
            500,
            260,
        );
        control("BUTTON", "New", WS_TABSTOP, 3, 535, 45, 75, 30);
        control("BUTTON", "Delete", WS_TABSTOP, 4, 620, 45, 75, 30);
        let labels = [
            "Name",
            "Entry file (blank = active file)",
            "Command (blank = detect language)",
            "Working directory (blank = detected)",
            "Arguments (one argument per line; no quoting needed)",
            "Environment (one NAME=value per line)",
        ];
        let mut fields = Vec::new();
        for (index, label) in labels.iter().enumerate() {
            let y = if index == 5 {
                420
            } else {
                88 + index as i32 * 62
            };
            control("STATIC", label, 0, 0, 20, y, 675, 22);
            let style = if index >= 4 {
                ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_WANTRETURN as u32 | WS_VSCROLL
            } else {
                ES_AUTOHSCROLL as u32
            };
            fields.push(control(
                "EDIT",
                "",
                WS_TABSTOP | WS_BORDER | style,
                110 + index,
                20,
                y + 23,
                675,
                if index >= 4 { 50 } else { 28 },
            ));
        }
        control(
            "STATIC",
            "Paths are relative to the workspace. Command is an executable; put its options in Arguments.",
            0,
            0,
            20,
            510,
            675,
            40,
        );
        let error = control("STATIC", "", 0, 0, 20, 550, 675, 50);
        control(
            "BUTTON",
            "Save && Select",
            WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            1,
            445,
            610,
            155,
            32,
        );
        control("BUTTON", "Cancel", WS_TABSTOP, 2, 610, 610, 85, 32);
        let mut form = Box::new(Form {
            root,
            saved,
            choice,
            fields,
            error,
            editing: None,
            new: false,
            done: false,
            changed: false,
            theme: theme.clone(),
            background: CreateSolidBrush(theme.sidebar_bg),
            field_background: CreateSolidBrush(theme.editor_bg),
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut *form as *mut Form as isize);
        form.populate();
        EnableWindow(owner, 0);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(choice);
        let mut message = MSG::default();
        while !form.done {
            let result = GetMessageW(&mut message, null_mut(), 0, 0);
            if result <= 0 {
                if result == 0 {
                    PostQuitMessage(message.wParam as i32);
                }
                DestroyWindow(hwnd);
                break;
            }
            if IsDialogMessageW(hwnd, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        EnableWindow(owner, 1);
        SetForegroundWindow(owner);
        SetFocus(owner);
        form.changed
    }
}
