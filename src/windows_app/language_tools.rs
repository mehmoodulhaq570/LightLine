// Tools from extensions, such as the spell checkers typos, codebook,
// harper and cspell (lightline::extensions::tools): language servers that
// run beside a file's own one.
//
// A shown file whose language a tool checks is opened in it (one server
// per tool and project, started from PATH) and sent each edit, as its own
// language server is. Their problems join the file's list tagged with the
// server they came from (`LspDiagnostic::origin`), so each server's list
// is replaced on its own, and Quick Fix on a tool's problem asks that tool
// (code_actions.rs). The installed tools are listed on a worker the first
// time a file is shown; a tool whose command isn't on PATH says so once
// and isn't tried again until an extension is installed or removed.

use super::*;
use lightline::extensions::language_servers;
use lightline::extensions::tool_download;
use lightline::extensions::tools::{self, Tool};
use lightline::lsp::{Command as LspCommand, Position as LspPosition, Range as LspRange};

/// What the status bar says after a tool is installed: each of its
/// servers' command, whether it's on PATH, and how to get it.
#[derive(Debug, Clone)]
pub(super) struct InstalledTool {
    command: Option<String>,
    found: bool,
    // LightLine can download its program when asked (tool_download.rs).
    downloadable: bool,
    hint: Option<&'static str>,
    languages: usize,
}

/// For the install message. Looks on PATH, so it's called on the install
/// worker.
pub(super) fn installed_tools(list: &[Tool]) -> Vec<InstalledTool> {
    list.iter()
        .map(|tool| {
            let command = language_servers::known_command(&tool.server).map(|c| c.command);
            InstalledTool {
                found: command
                    .as_deref()
                    .is_some_and(|command| language_servers::find_command(command).is_some()),
                command,
                downloadable: tool_download::release_for(&tool.server).is_some(),
                hint: tools::install_hint(&tool.server),
                languages: tool.languages.len(),
            }
        })
        .collect()
}

/// The status bar after a tool is installed.
pub(super) fn installed_note(name: &str, list: &[InstalledTool]) -> String {
    let Some(tool) = list.first() else {
        return format!("{name} installed");
    };
    let languages = format!("{} languages", tool.languages);
    match (&tool.command, tool.found) {
        (Some(command), true) => {
            format!("{name} installed: it checks files in {languages} with `{command}`")
        }
        (Some(command), false) if tool.downloadable => format!(
            "{name} installed. It needs `{command}`: open a file it checks and LightLine offers \
             to download it"
        ),
        (Some(command), false) => format!(
            "{name} installed. It needs `{command}`: {}",
            tool.hint.unwrap_or("see the extension's page")
        ),
        (None, _) => format!(
            "{name} installed, but LightLine doesn't know its server's command: set it under \
             \"languageServers\" in settings.json"
        ),
    }
}

/// The status bar when tool `server` couldn't start or stopped: when its
/// command is missing, the command and how to get it come first, so they
/// fit before the status bar cuts the message off.
pub(super) fn stopped_note(server: &str, message: &str) -> String {
    let command = language_servers::known_command(server).map(|command| command.command);
    match (command, tools::install_hint(server)) {
        (Some(command), Some(hint)) if message.contains("not found on PATH") => {
            format!("{server} needs `{command}` to check files: {hint}")
        }
        _ => message.to_string(),
    }
}

// Asks before downloading a checker's program, as for Pyright.
fn ask_to_download(hwnd: HWND, release: &tool_download::Release) -> bool {
    let message = format!(
        "{server} checks your files with {command}, which isn't installed.\n\nDownload \
         {command} {version} now? It's about {mb} MB from {site}. LightLine checks it's the \
         exact file it expects and puts it in its own folder; nothing else on your PC changes.",
        server = release.server,
        command = release.command,
        version = release.version,
        mb = release.megabytes,
        site = release.site,
    );
    dialog::show_dialog(
        hwnd,
        &format!("Set up {}", release.server),
        &message,
        dialog::DialogIcon::Question,
        &[
            dialog::DialogButton {
                label: "Download",
                id: dialog::DLG_YES,
                is_default: true,
                is_cancel: false,
            },
            dialog::DialogButton {
                label: "Not Now",
                id: dialog::DLG_NO,
                is_default: false,
                is_cancel: true,
            },
        ],
    ) == dialog::DLG_YES
}

// The LSP language a tool's server runs as: registered by the server's
// name, with each file's own language id.
fn tool_language(tool: &Tool) -> LspLanguage {
    lsp::extension_language(&tool.server, "", std::slice::from_ref(&tool.server))
}

impl App {
    /// The server running `language` for `root`.
    pub(super) fn client_for(&self, language: LspLanguage, root: &Path) -> Option<&LspClient> {
        self.lsp
            .iter()
            .find(|client| client.language() == language && client.root() == root)
    }

    /// The server tab `index` was opened with for `language`: its own
    /// language server or one of its tools.
    pub(super) fn tab_client(&self, index: usize, language: LspLanguage) -> Option<&LspClient> {
        let tab = &self.tabs[index];
        if tab.lsp_language == Some(language) {
            return self.tab_lsp(index);
        }
        let (_, root) = tab.tools.iter().find(|(tool, _)| *tool == language)?;
        self.client_for(language, root)
    }

    /// Whether `language` is a tool's server rather than a file's own.
    pub(super) fn is_tool_language(&self, language: LspLanguage) -> bool {
        self.tabs
            .iter()
            .any(|tab| tab.tools.iter().any(|(tool, _)| *tool == language))
            || self.tools_failed.iter().any(|(tool, _)| *tool == language)
            || tools::installed_if_listed()
                .is_some_and(|list| list.iter().any(|tool| tool_language(tool) == language))
    }

    // Lists the installed tools on a worker; `tools_listed` follows.
    fn list_tools(&mut self) {
        if self.tools_listing {
            return;
        }
        self.tools_listing = true;
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            tools::list_installed();
            tx.send(WorkerMessage::ToolsListed);
        });
    }

    /// The installed tools have been listed: the file shown gets its own.
    pub(super) fn tools_listed(&mut self, hwnd: HWND) {
        self.tools_listing = false;
        self.ensure_tools(hwnd);
    }

    /// Opens the active file in each installed tool that checks its
    /// language, starting the tool's server for its project if needed.
    pub(super) fn ensure_tools(&mut self, hwnd: HWND) {
        let Some(path) = self.doc().path.clone() else {
            return;
        };
        if self.tab().read_only()
            || self.tab().is_placeholder()
            || self.doc().byte_len() > super::language::LSP_MAX_FILE_BYTES
        {
            return;
        }
        let Some(list) = tools::installed_if_listed() else {
            self.list_tools();
            return;
        };
        if list.is_empty() {
            return;
        }
        let keys = self.snippet_keys().0;
        let wanted: Vec<LspLanguage> = list
            .iter()
            .filter(|tool| tool.checks(&keys))
            .map(tool_language)
            .collect();
        let active = self.active;
        // Tools that missed an edit get the whole text again.
        if !self.tabs[active].tools.is_empty()
            && self.tabs[active].tools_serial != self.doc().change_serial()
        {
            self.close_tools(active);
        }
        if wanted.is_empty() {
            return;
        }
        let Some(any) = wanted.first() else {
            return;
        };
        let root = PathBuf::from(display_path(&self.lsp_root(*any, &path)));
        let mut text: Option<String> = None;
        for language in wanted {
            if self.tools_failed.contains(&(language, root.clone()))
                || self.tabs[active]
                    .tools
                    .iter()
                    .any(|(tool, at)| *tool == language && *at == root)
            {
                continue;
            }
            if self.client_for(language, &root).is_none() {
                let hwnd_value = hwnd as isize;
                let wake = Arc::new(move || unsafe {
                    PostMessageW(hwnd_value as HWND, super::language::LSP_EVENT_MESSAGE, 0, 0);
                });
                self.lsp.push(LspClient::start(
                    language,
                    root.clone(),
                    None,
                    self.lsp_event_tx.clone(),
                    wake,
                ));
            }
            let text = text.get_or_insert_with(|| self.doc().text()).clone();
            let tab = &self.tabs[active];
            let version = tab.tools_version;
            let sent = self.client_for(language, &root).is_some_and(|client| {
                client.send(LspCommand::Open {
                    uri: lsp::file_uri(&path),
                    text,
                    version,
                })
            });
            if sent {
                let serial = self.doc().change_serial();
                let tab = &mut self.tabs[active];
                tab.tools.push((language, root.clone()));
                tab.tools_serial = serial;
            }
        }
    }

    /// Sends the active file's last edit to its tools.
    pub(super) fn sync_tool_edits(&mut self) {
        let active = self.active;
        let tab = &self.tabs[active];
        if tab.tools.is_empty() || tab.tools_serial == tab.document.change_serial() {
            return;
        }
        let (Some(path), Some(change)) = (
            tab.document.path.clone(),
            tab.document.last_change().cloned(),
        ) else {
            return;
        };
        let own_server = tab.lsp_opened;
        if change.serial != tab.tools_serial.wrapping_add(1) {
            // A missed edit: reopen with the whole text.
            self.close_tools(active);
            self.ensure_tools(self.hwnd);
            return;
        }
        let range = LspRange {
            start: LspPosition {
                line: change.start.line as u32,
                character: change.start_utf16 as u32,
            },
            end: LspPosition {
                line: change.end.line as u32,
                character: change.end_utf16 as u32,
            },
        };
        let version = tab.tools_version.saturating_add(1);
        let uri = lsp::file_uri(&path);
        let mut stopped = Vec::new();
        for (language, root) in &tab.tools {
            let sent = self.client_for(*language, root).is_some_and(|client| {
                client.send(LspCommand::Change {
                    uri: uri.clone(),
                    version,
                    range,
                    text: change.text.clone(),
                })
            });
            if !sent {
                stopped.push(*language);
            }
        }
        let lines_added = change.new_end().line as i64 - change.end.line as i64;
        let tab = &mut self.tabs[active];
        tab.tools
            .retain(|(language, _)| !stopped.contains(language));
        tab.tools_version = version;
        tab.tools_serial = change.serial;
        // The file's own server moves the problems when it has one
        // (sync_lsp_edit); otherwise they move here.
        if !own_server {
            super::language::shift_diagnostics(&mut tab.diagnostics, range.end.line, lines_added);
        }
    }

    /// Closes tab `index`'s file in its tools and drops their problems.
    pub(super) fn close_tools(&mut self, index: usize) {
        let open = std::mem::take(&mut self.tabs[index].tools);
        if open.is_empty() {
            return;
        }
        if let Some(path) = self.tabs[index].document.path.clone() {
            for (language, root) in &open {
                if let Some(client) = self.client_for(*language, root) {
                    client.send(LspCommand::Close {
                        uri: lsp::file_uri(&path),
                    });
                }
            }
        }
        let tab = &mut self.tabs[index];
        tab.diagnostics.retain(|problem| {
            !open
                .iter()
                .any(|(language, _)| problem.origin == Some(*language))
        });
    }

    /// Tells the active file's tools it was saved.
    pub(super) fn tools_saved(&self) {
        let tab = self.tab();
        let Some(path) = tab.document.path.as_deref() else {
            return;
        };
        for (language, root) in &tab.tools {
            if let Some(client) = self.client_for(*language, root) {
                client.send(LspCommand::Save {
                    uri: lsp::file_uri(path),
                });
            }
        }
    }

    /// A tool's server stopped or couldn't start: its files lose it and
    /// its problems, it isn't tried again this session, and the first time
    /// the status bar says why.
    pub(super) fn tool_stopped(
        &mut self,
        hwnd: HWND,
        language: LspLanguage,
        root: PathBuf,
        message: String,
    ) {
        self.lsp
            .retain(|client| !(client.language() == language && client.root() == root));
        for tab in &mut self.tabs {
            let before = tab.tools.len();
            tab.tools
                .retain(|(tool, at)| !(*tool == language && *at == root));
            if tab.tools.len() != before {
                tab.diagnostics
                    .retain(|problem| problem.origin != Some(language));
            }
        }
        self.tools_failed.insert((language, root));
        if !self.tools_reported.insert(language) {
            return;
        }
        let server = language.name();
        // Its program isn't installed: offer to download it (asked once a
        // session), as Pyright is.
        if message.contains("not found on PATH")
            && let Some(release) = tool_download::release_for(server)
        {
            if self.tools_downloading.contains(server) {
                return;
            }
            if ask_to_download(hwnd, &release) {
                self.download_tool(hwnd, release);
                return;
            }
        }
        self.status = stopped_note(server, &message);
    }

    // Downloads `release` on a worker; `tool_downloaded` follows.
    fn download_tool(&mut self, hwnd: HWND, release: tool_download::Release) {
        self.tools_downloading.insert(release.server.to_string());
        self.status = format!(
            "Downloading {} {} (about {} MB) from {}…",
            release.command, release.version, release.megabytes, release.site
        );
        unsafe { InvalidateRect(hwnd, null(), 0) };
        let tx = self.worker_tx.clone();
        std::thread::spawn(move || {
            let result = tool_download::download(&release);
            tx.send(WorkerMessage::ToolDownloaded(
                release.server.to_string(),
                result,
            ));
        });
    }

    /// A checker's program finished downloading: files it checks get it
    /// now, or the status bar says why not.
    pub(super) fn tool_downloaded(
        &mut self,
        hwnd: HWND,
        server: String,
        result: Result<PathBuf, String>,
    ) {
        self.tools_downloading.remove(&server);
        match result {
            Ok(_) => {
                self.tools_failed
                    .retain(|(language, _)| language.name() != server);
                self.tools_reported
                    .retain(|language| language.name() != server);
                let command = language_servers::known_command(&server)
                    .map(|command| command.command)
                    .unwrap_or_else(|| server.clone());
                self.status = format!("{command} downloaded: {server} is checking files");
                self.ensure_tools(hwnd);
            }
            Err(error) => {
                let hint = tools::install_hint(&server)
                    .map(|hint| format!(". You can also {hint}"))
                    .unwrap_or_default();
                self.status = format!("Couldn't get {server}'s program: {error}{hint}");
            }
        }
    }

    /// An extension was installed or removed: every tool's server stops,
    /// the list is read again, and the shown file gets the tools it has
    /// now.
    pub(super) fn restart_tools(&mut self) {
        let mut stopped: HashSet<LspLanguage> =
            self.tools_failed.iter().map(|(tool, _)| *tool).collect();
        for tab in &mut self.tabs {
            for (language, _) in tab.tools.drain(..) {
                stopped.insert(language);
            }
        }
        if let Some(list) = tools::installed_if_listed() {
            stopped.extend(list.iter().map(tool_language));
        }
        for tab in &mut self.tabs {
            tab.diagnostics.retain(|problem| {
                problem
                    .origin
                    .is_none_or(|origin| !stopped.contains(&origin))
            });
        }
        self.lsp
            .retain(|client| !stopped.contains(&client.language()));
        self.tools_failed.clear();
        self.tools_reported.clear();
        tools::reload();
        self.list_tools();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_note_says_whether_the_command_is_there() {
        let tool = |command: Option<&str>, found, downloadable| InstalledTool {
            command: command.map(str::to_string),
            found,
            downloadable,
            hint: Some("run cargo install typos-lsp"),
            languages: 62,
        };
        assert_eq!(
            installed_note("Typos", &[tool(Some("typos-lsp"), true, true)]),
            "Typos installed: it checks files in 62 languages with `typos-lsp`"
        );
        assert_eq!(
            installed_note("Typos", &[tool(Some("typos-lsp"), false, true)]),
            "Typos installed. It needs `typos-lsp`: open a file it checks and LightLine offers \
             to download it"
        );
        // Not one LightLine can download (cspell): how to get it.
        assert_eq!(
            installed_note("Typos", &[tool(Some("typos-lsp"), false, false)]),
            "Typos installed. It needs `typos-lsp`: run cargo install typos-lsp"
        );
        assert!(installed_note("X", &[tool(None, false, false)]).contains("languageServers"));
    }

    #[test]
    fn a_missing_command_says_how_to_get_it_first() {
        let lsp = "typos: `typos-lsp` not found on PATH, so its language server isn't running. \
                   Install it, or set its command under \"languageServers\" in settings.json.";
        assert_eq!(
            stopped_note("typos", lsp),
            "typos needs `typos-lsp` to check files: get typos-lsp.exe from \
             github.com/tekumara/typos-lsp/releases and put it on PATH, or run cargo install \
             typos-lsp"
        );
        // Anything else is said as the server said it.
        assert_eq!(stopped_note("typos", "it crashed"), "it crashed");
        assert_eq!(stopped_note("unknown-ls", lsp), lsp);
    }
}
