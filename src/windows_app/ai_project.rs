//! Letting the AI Assistant's model look through the project
//! (design/ai-project-access-design.html, part 2). The tools themselves are
//! in lightline::ai_tools and the calling loop in lightline::ai; this file
//! takes the snapshot a question works from, asks the user once per chat,
//! and turns each tool call into a line in the answer.
//!
//! The question's worker thread waits on the user's answer to the
//! permission card; the card's buttons send it (`ai_answer_permission`).

use super::*;
use lightline::ai;
use lightline::ai_tools::{Problem, Project};
use lightline::settings::AiProjectAccess;

/// An open file with unsaved edits bigger than this is read from disk.
const MAX_EDITED_BYTES: usize = 4 * 1024 * 1024;

/// The user's answer to "Let … look through the project?".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Permission {
    ForChat,
    Once,
    Deny,
}

/// One tool call, as a line above the answer.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct AiStep {
    pub(super) summary: String,
    pub(super) error: bool,
    /// For a read: the file and line clicking the line opens.
    pub(super) opened: Option<(PathBuf, usize)>,
}

/// What a question with project access starts with.
pub(super) struct ProjectAccess {
    pub(super) project: Project,
    /// Allowed already: for this chat, or by the setting.
    pub(super) allowed: bool,
    /// Said to the model alongside the system prompt.
    pub(super) instructions: &'static str,
}

const TOOL_INSTRUCTIONS: &str = "You can look through the user's project with the tools \
search, list_files, read_file and problems. Use them when the question is about the project \
and you haven't been shown the code it needs; don't guess what a file contains. Paths are \
relative to the project's folder. Say which files your answer is based on.";

impl App {
    /// The project as a question sees it, when the model may look through
    /// it: the setting allows it, a folder is open, and the model hasn't
    /// refused tools before. Unsaved edits are copied here, on the UI
    /// thread, once per question.
    pub(super) fn ai_project_access(&self, model: &str) -> Option<ProjectAccess> {
        let access = self.settings.ai_project_access;
        if access == AiProjectAccess::Off || self.ai.tools_refused.contains(model) {
            return None;
        }
        let root = self.workspace_root.clone()?;
        let mut edited = Vec::new();
        let mut problems = Vec::new();
        for tab in &self.tabs {
            let Some(path) = tab.document.path.clone() else {
                continue;
            };
            if !tab.unloaded
                && tab.document.is_dirty()
                && tab.document.byte_len() <= MAX_EDITED_BYTES
            {
                edited.push((path.clone(), tab.document.text()));
            }
            for diagnostic in &tab.diagnostics {
                let severity = match diagnostic.severity {
                    1 => "error",
                    2 => "warning",
                    _ => continue,
                };
                problems.push(Problem {
                    path: path.clone(),
                    line: diagnostic.range.start.line as usize + 1,
                    severity,
                    message: diagnostic.message.clone(),
                });
            }
        }
        Some(ProjectAccess {
            project: Project::new(root, edited, problems),
            allowed: access == AiProjectAccess::Always || self.ai.chat_allowed,
            instructions: TOOL_INSTRUCTIONS,
        })
    }

    /// The card's title and text, naming who receives what's read.
    pub(super) fn ai_permission_text(&self) -> (String, String) {
        let folder = self
            .workspace_root
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "the project".into());
        let endpoint = &self.settings.ai_endpoint;
        let (who, goes) = if ai::is_anthropic(endpoint) {
            ("Claude".to_string(), "is sent to Anthropic".to_string())
        } else if ai::is_local(endpoint) {
            let model = self.settings.ai_model.clone().unwrap_or_default();
            if ai::is_cloud_model(&model) {
                (model, "is sent to ollama.com".to_string())
            } else {
                (model, "stays on this PC".to_string())
            }
        } else {
            let model = self.settings.ai_model.clone().unwrap_or_default();
            (model, format!("is sent to {}", ai::host(endpoint)))
        };
        (
            format!("Let {who} look through {folder}?"),
            format!(
                "It wants to search and read files to answer. What it reads {goes}. It can't \
                 change anything, and each step is listed here."
            ),
        )
    }

    /// A button on the permission card.
    pub(super) fn ai_answer_permission(&mut self, hwnd: HWND, answer: Permission) {
        if let Some((_, reply)) = self.ai.permission.take() {
            let _ = reply.send(answer);
        }
        if answer == Permission::ForChat {
            self.ai.chat_allowed = true;
        }
        for entry in &mut self.ai.entries {
            entry.asking = false;
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Clicking a "Read" line: the file, at the line it was read from.
    pub(super) fn ai_open_step(&mut self, hwnd: HWND, path: PathBuf, line: usize) {
        self.open(hwnd, Some(path));
        let doc = self.doc();
        let line = line
            .saturating_sub(1)
            .min(doc.line_count().saturating_sub(1));
        self.doc_mut().unfold_to_reveal(line);
        self.move_cursor(Pos { line, byte: 0 }, false);
        self.keep_cursor_visible(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}
