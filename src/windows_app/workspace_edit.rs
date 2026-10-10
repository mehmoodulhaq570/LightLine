// Edits a language server asks for across files: a rename, a quick fix, or
// one it sends itself. Open files change like typed edits, one undo step per
// file; files that aren't open are changed on disk.

use super::app::remap_position;
use super::language::resolve_edits;
use super::*;
use lightline::lsp::{Command as LspCommand, FileEdit, TextEdit};

pub(super) struct Applied {
    pub(super) places: usize,
    pub(super) files: usize,
    // Files that couldn't be saved, with why.
    pub(super) failed: Vec<String>,
}

impl App {
    /// Makes `files`' edits. Every file that isn't open is read first, so
    /// one that can't be read stops it all with nothing changed (Err says
    /// which). Those are saved, and `language`'s servers told. Open files
    /// without unsaved work are saved too, the active one only with
    /// `save_active`: a check run from disk (cargo check) would otherwise see
    /// the files changed on disk next to these unchanged, and report errors
    /// that last until they're saved.
    pub(super) fn apply_workspace_edit(
        &mut self,
        files: Vec<FileEdit>,
        language: LspLanguage,
        save_active: bool,
    ) -> Result<Applied, String> {
        let mut in_tabs: Vec<(usize, Vec<TextEdit>)> = Vec::new();
        let mut on_disk: Vec<(PathBuf, String, Document)> = Vec::new();
        let mut places = 0;
        for file in files {
            places += file.edits.len();
            if let Some(index) = self.open_tab_for_uri(&file.uri) {
                in_tabs.push((index, file.edits));
                continue;
            }
            let Some(path) = lsp::uri_to_path(&file.uri) else {
                return Err(format!("it changes {}", file.uri));
            };
            match Document::open(path.clone()) {
                Ok(mut document) => {
                    for (start, end, text) in resolve_edits(&document, &file.edits) {
                        document.replace(start, end, &text);
                    }
                    on_disk.push((path, file.uri, document));
                }
                Err(error) => return Err(format!("{}: {error}", path.display())),
            }
        }
        let applied_files = in_tabs.len() + on_disk.len();
        let mut saved = Vec::new();
        let mut failed = Vec::new();
        for (path, uri, mut document) in on_disk {
            match document.save(&path) {
                Ok(()) => saved.push(uri),
                Err(error) => failed.push(format!("{}: {error}", display_name(&path))),
            }
        }
        if !saved.is_empty() {
            for client in self
                .lsp
                .iter()
                .filter(|client| client.language() == language)
            {
                client.send(LspCommand::FilesChanged {
                    uris: saved.clone(),
                });
            }
        }
        for (index, edits) in in_tabs {
            let save =
                !self.tabs[index].document.is_dirty() && (save_active || index != self.active);
            self.apply_edits_to_tab(index, &edits);
            if save && let Err(error) = self.save_tab_quietly(index) {
                let name = self.tabs[index].document.path.as_deref().map(display_name);
                failed.push(format!("{}: {error}", name.unwrap_or_default()));
            }
        }
        self.gutter_done = None;
        self.refresh_active_git_diff();
        self.refresh_git();
        self.keep_cursor_visible(self.hwnd);
        Ok(Applied {
            places,
            files: applied_files,
            failed,
        })
    }

    // Writes tab `index`'s file, without format-on-save or questions: one
    // that changed on disk since it was read stays unsaved, with the error.
    fn save_tab_quietly(&mut self, index: usize) -> io::Result<()> {
        let Some(path) = self.tabs[index].document.path.clone() else {
            return Ok(());
        };
        self.tabs[index].document.save(&path)?;
        let active = self.active;
        // lsp_after_save tells the active tab's server.
        self.active = index;
        self.lsp_after_save(self.hwnd, Some(&path));
        self.active = active;
        Ok(())
    }

    /// The tab with `uri`'s text in memory.
    pub(super) fn open_tab_for_uri(&self, uri: &str) -> Option<usize> {
        self.tabs.iter().position(|tab| {
            !tab.unloaded
                && !tab.is_placeholder()
                && tab.markdown.is_none()
                && tab
                    .document
                    .path
                    .as_deref()
                    .is_some_and(|path| lsp::same_file_uri(&lsp::file_uri(path), uri))
        })
    }

    // Applies a language server's edits to tab `index` as one undo step,
    // keeping each view's caret and selection on their code.
    fn apply_edits_to_tab(&mut self, index: usize, edits: &[TextEdit]) {
        let active = self.active;
        // replace_range edits the active tab.
        self.active = index;
        let resolved = resolve_edits(self.doc(), edits);
        let mut views = self.tab().views.clone();
        self.doc_mut().begin_group();
        for (start, end, text) in resolved {
            self.replace_range(start, end, &text);
            let inserted = self.view().cursor;
            for view in &mut views {
                view.cursor = remap_position(view.cursor, start, end, inserted);
                view.selection_anchor = view
                    .selection_anchor
                    .map(|anchor| remap_position(anchor, start, end, inserted));
            }
        }
        self.doc_mut().end_group();
        let tab = &mut self.tabs[index];
        for view in &mut views {
            view.cursor = tab.document.clamp(view.cursor);
            view.selection_anchor = view.selection_anchor.map(|pos| tab.document.clamp(pos));
        }
        tab.views = views;
        self.active = active;
    }
}

/// "1 place", "3 places".
pub(super) fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
