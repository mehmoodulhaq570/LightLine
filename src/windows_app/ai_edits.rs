//! Changes the AI Assistant's model proposed
//! (design/ai-project-access-design.html, part 3): a card under the answer
//! lists them, Review shows one in the diff view beside the file, and
//! Apply puts it into the editor as one undo step. Nothing is saved; a new
//! file opens as an unsaved tab with its path.
//!
//! The proposals were made against the text of the moment; Apply replays
//! each edit (`ai_tools::apply_edits`) on the editor's text now, and a
//! change that no longer fits is reported, not forced.

use super::*;
use lightline::ai_tools::{self, FileProposal};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum ProposalStatus {
    Pending,
    Applied,
    Skipped,
    /// It no longer fits the file, and why.
    Stale(String),
}

/// One file's proposed change, under an answer.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct AnswerProposal {
    pub(super) proposal: FileProposal,
    pub(super) status: ProposalStatus,
}

/// The proposal shown in the diff view: answer `entry`'s file `file`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ProposalReview {
    pub(super) entry: usize,
    pub(super) file: usize,
}

/// A button in the diff view's header, while comparing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum CompareAction {
    Restore,
    Close,
    Apply,
    Skip,
    Next,
}

/// The diff view's header while comparing: its two labels and buttons.
pub(super) struct CompareHeader {
    pub(super) before: String,
    pub(super) after: String,
    pub(super) buttons: Vec<(RECT, &'static str, bool, CompareAction)>,
}

impl App {
    /// A proposal for answer `entry`: a new one, or the latest state of a
    /// file already proposed.
    pub(super) fn ai_add_proposal(&mut self, entry: usize, proposal: FileProposal) {
        let Some(answer) = self.ai.entries.get_mut(entry) else {
            return;
        };
        let item = AnswerProposal {
            proposal,
            status: ProposalStatus::Pending,
        };
        match answer
            .proposals
            .iter_mut()
            .find(|known| known.proposal.path == item.proposal.path)
        {
            Some(known) => *known = item,
            None => answer.proposals.push(item),
        }
    }

    fn proposal(&self, entry: usize, file: usize) -> Option<&AnswerProposal> {
        self.ai.entries.get(entry)?.proposals.get(file)
    }

    fn set_proposal_status(&mut self, entry: usize, file: usize, status: ProposalStatus) {
        if let Some(item) = self
            .ai
            .entries
            .get_mut(entry)
            .and_then(|answer| answer.proposals.get_mut(file))
        {
            item.status = status;
        }
    }

    // The editor's text for `path` now, if it's open; else the file's.
    fn current_text(&self, path: &Path) -> Option<String> {
        if let Some(tab) = self.tabs.iter().find(|tab| {
            !tab.unloaded
                && tab
                    .document
                    .path
                    .as_deref()
                    .is_some_and(|open| Self::same_path(open, path))
        }) {
            return Some(tab.document.text());
        }
        std::fs::read_to_string(path)
            .ok()
            .map(|text| text.replace("\r\n", "\n"))
    }

    /// Review: the proposal beside the file as it is now, in the diff view.
    pub(super) fn ai_review(&mut self, hwnd: HWND, entry: usize, file: usize) {
        let Some(item) = self.proposal(entry, file).cloned() else {
            return;
        };
        let proposal = &item.proposal;
        let (before, after) = if proposal.new_file {
            (String::new(), proposal.proposed.clone())
        } else {
            let Some(now) = self.current_text(&proposal.path) else {
                self.set_proposal_status(entry, file, ProposalStatus::Stale("not found".into()));
                self.status = format!("{} can't be read now", proposal.name);
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            };
            match ai_tools::apply_edits(&now, &proposal.edits) {
                Ok(after) => (now, after),
                Err(why) => {
                    self.set_proposal_status(entry, file, ProposalStatus::Stale(why.clone()));
                    self.status = format!("{}: {why}", proposal.name);
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    return;
                }
            }
        };
        self.close_diff();
        self.backbuffer = None;
        self.diff_rows = workflow::text_diff_rows(&before, &after);
        self.diff_first = 0;
        self.review_file = None;
        self.panel_focus = false;
        self.terminal_focus = false;
        self.ai.focused = false;
        self.ai_review = Some(ProposalReview { entry, file });
        self.status = "Apply puts this change into the editor (Ctrl+Z undoes it)  \u{00b7}  Skip \
                       leaves the file as it is  \u{00b7}  Esc closes"
            .into();
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Applies one file's proposal in the editor. False when it couldn't be.
    pub(super) fn ai_apply(&mut self, hwnd: HWND, entry: usize, file: usize) -> bool {
        let Some(item) = self.proposal(entry, file).cloned() else {
            return false;
        };
        if item.status != ProposalStatus::Pending {
            return false;
        }
        let proposal = item.proposal;
        if proposal.new_file {
            if proposal.path.exists() {
                self.set_proposal_status(
                    entry,
                    file,
                    ProposalStatus::Stale("the file exists now".into()),
                );
                return false;
            }
            if let Some(folder) = proposal.path.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            let mut document = Document::new();
            document.path = Some(proposal.path.clone());
            self.tabs.push(Tab::new(document));
            self.set_active_index(self.tabs.len() - 1);
            self.show_active_tab(hwnd);
            self.doc_mut().begin_group();
            self.replace_range(Pos::default(), Pos::default(), &proposal.proposed);
            self.doc_mut().end_group();
            self.move_cursor(Pos::default(), false);
        } else {
            self.open(hwnd, Some(proposal.path.clone()));
            let open = self
                .doc()
                .path
                .as_deref()
                .is_some_and(|path| Self::same_path(path, &proposal.path));
            if !open {
                self.set_proposal_status(entry, file, ProposalStatus::Stale("not open".into()));
                return false;
            }
            if let Err(why) = ai_tools::apply_edits(&self.doc().text(), &proposal.edits) {
                self.set_proposal_status(entry, file, ProposalStatus::Stale(why));
                return false;
            }
            self.doc_mut().begin_group();
            let mut first_change = None;
            for (find, replace) in &proposal.edits {
                let text = self.doc().text();
                let Some(at) = text.find(find.as_str()) else {
                    continue;
                };
                let start = self.doc().pos_at(at);
                let end = self.doc().pos_at(at + find.len());
                self.replace_range(start, end, replace);
                first_change.get_or_insert(start);
            }
            self.doc_mut().end_group();
            if let Some(start) = first_change {
                self.move_cursor(start, false);
            }
        }
        self.keep_cursor_visible(hwnd);
        self.set_proposal_status(entry, file, ProposalStatus::Applied);
        // The keyboard goes to the change, so Ctrl+Z and Ctrl+S act on it.
        self.ai.focused = false;
        self.panel_focus = false;
        self.terminal_focus = false;
        true
    }

    /// The next file still waiting in answer `entry`, after `file`.
    fn next_pending(&self, entry: usize, after: Option<usize>) -> Option<usize> {
        let proposals = &self.ai.entries.get(entry)?.proposals;
        let start = after.map_or(0, |file| file + 1);
        (start..proposals.len())
            .chain(0..start)
            .find(|&file| proposals[file].status == ProposalStatus::Pending)
    }

    // After deciding a file in the diff view: the next one, or back to the
    // editor.
    fn review_next(&mut self, hwnd: HWND, entry: usize, file: usize) {
        match self.next_pending(entry, Some(file)) {
            Some(next) => self.ai_review(hwnd, entry, next),
            None => {
                self.close_diff();
                self.refresh(hwnd);
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
        }
    }

    /// A button in the diff view's header while a proposal is shown.
    fn ai_review_action(&mut self, hwnd: HWND, action: CompareAction) {
        let Some(ProposalReview { entry, file }) = self.ai_review else {
            return;
        };
        match action {
            CompareAction::Apply => {
                self.close_diff();
                let name = self
                    .proposal(entry, file)
                    .map(|item| item.proposal.name.clone())
                    .unwrap_or_default();
                self.status = if self.ai_apply(hwnd, entry, file) {
                    format!("Changed {name}: Ctrl+Z undoes it, Ctrl+S saves it")
                } else {
                    format!("{name} wasn't changed: the proposal no longer fits it")
                };
                if let Some(next) = self.next_pending(entry, Some(file)) {
                    self.ai_review(hwnd, entry, next);
                }
            }
            CompareAction::Skip => {
                self.set_proposal_status(entry, file, ProposalStatus::Skipped);
                self.review_next(hwnd, entry, file);
            }
            CompareAction::Next => {
                if let Some(next) = self.next_pending(entry, Some(file)) {
                    self.ai_review(hwnd, entry, next);
                }
            }
            CompareAction::Close | CompareAction::Restore => {
                self.close_diff();
            }
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// The changes card's buttons.
    pub(super) fn ai_proposals_review(&mut self, hwnd: HWND, entry: usize) {
        if let Some(file) = self.next_pending(entry, None) {
            self.ai_review(hwnd, entry, file);
        }
    }

    pub(super) fn ai_proposals_apply_all(&mut self, hwnd: HWND, entry: usize) {
        self.close_diff();
        let mut changed = 0;
        let mut failed = 0;
        while let Some(file) = self.next_pending(entry, None) {
            if self.ai_apply(hwnd, entry, file) {
                changed += 1;
            } else {
                failed += 1;
                if self.proposal(entry, file).map(|item| &item.status)
                    == Some(&ProposalStatus::Pending)
                {
                    self.set_proposal_status(entry, file, ProposalStatus::Skipped);
                }
            }
        }
        let files = |count: usize| if count == 1 { "file" } else { "files" };
        self.status = if failed == 0 {
            format!(
                "Changed {changed} {}: Ctrl+Z undoes each, Ctrl+S saves",
                files(changed)
            )
        } else {
            format!(
                "Changed {changed} {}; {failed} no longer fit and weren't changed",
                files(changed)
            )
        };
        self.refresh(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn ai_proposals_discard(&mut self, hwnd: HWND, entry: usize) {
        if let Some(answer) = self.ai.entries.get_mut(entry) {
            for item in &mut answer.proposals {
                if item.status == ProposalStatus::Pending {
                    item.status = ProposalStatus::Skipped;
                }
            }
        }
        if self.ai_review.is_some_and(|review| review.entry == entry) {
            self.close_diff();
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// The diff view's header while a proposal is shown.
    pub(super) fn ai_review_header(&self, right: i32) -> Option<CompareHeader> {
        let ProposalReview { entry, file } = self.ai_review?;
        let item = self.proposal(entry, file)?;
        let count = self.ai.entries.get(entry)?.proposals.len();
        let model = self.settings.ai_model.clone().unwrap_or_default();
        let name = &item.proposal.name;
        let before = if item.proposal.new_file {
            format!("NEW FILE  \u{00b7}  {name}")
        } else {
            format!("NOW  \u{00b7}  {name}")
        };
        let after = if count > 1 {
            format!("PROPOSED BY {model}  \u{00b7}  {} of {count}", file + 1)
        } else {
            format!("PROPOSED BY {model}")
        };
        let more = self
            .next_pending(entry, Some(file))
            .is_some_and(|next| next != file);
        let mut wanted: Vec<(&'static str, i32, bool, CompareAction)> = vec![
            ("Apply", 76, true, CompareAction::Apply),
            ("Skip", 64, false, CompareAction::Skip),
        ];
        if more {
            wanted.push(("Next \u{203a}", 70, false, CompareAction::Next));
        }
        Some(CompareHeader {
            before,
            after,
            buttons: self.header_buttons(right, &wanted),
        })
    }

    /// Buttons laid out from `right` leftwards in the diff header, the
    /// first of `wanted` leftmost.
    pub(super) fn header_buttons(
        &self,
        right: i32,
        wanted: &[(&'static str, i32, bool, CompareAction)],
    ) -> Vec<(RECT, &'static str, bool, CompareAction)> {
        let top = self.editor_top() + self.scale(6);
        let bottom = top + self.scale(26);
        let mut x = right - self.scale(16);
        let mut placed: Vec<_> = wanted
            .iter()
            .rev()
            .map(|&(label, width, primary, action)| {
                let rect = RECT {
                    left: x - self.scale(width),
                    top,
                    right: x,
                    bottom,
                };
                x = rect.left - self.scale(8);
                (rect, label, primary, action)
            })
            .collect();
        placed.reverse();
        placed
    }

    /// The diff header while comparing anything: a proposal, or a Local
    /// History version.
    pub(super) fn compare_header(&self, right: i32) -> Option<CompareHeader> {
        if let Some(header) = self.ai_review_header(right) {
            return Some(header);
        }
        let (before, after) = self.history_labels()?;
        Some(CompareHeader {
            before,
            after,
            buttons: self.header_buttons(
                right,
                &[
                    ("Restore", 84, true, CompareAction::Restore),
                    ("Close", 64, false, CompareAction::Close),
                ],
            ),
        })
    }

    /// A click in the diff header while comparing; true when it hit a button.
    pub(super) fn compare_header_click(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let Some(header) = self.compare_header(self.editor_right(hwnd)) else {
            return false;
        };
        let inside = |r: &RECT| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
        let Some(&(_, _, _, action)) = header.buttons.iter().find(|(rect, ..)| inside(rect)) else {
            return false;
        };
        if self.ai_review.is_some() {
            self.ai_review_action(hwnd, action);
        } else {
            match action {
                CompareAction::Restore => self.restore_history(hwnd),
                _ => {
                    self.close_diff();
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
            }
        }
        true
    }
}
