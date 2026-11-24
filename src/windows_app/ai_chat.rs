//! The AI Assistant's conversation: connecting to a model server, sending
//! questions (with the selected code), streaming answers into the panel, and
//! putting code from an answer back into the editor.
//! lightline::ai does the HTTP; render/ai_assistant.rs paints the panel.
//!
//! Which service answers, and its API key, are set in the AI provider card
//! (ai_provider.rs); this file runs its connection check.
//!
//! Nothing runs until the user connects or sends a message: the assistant
//! is only state until then. Each request runs on its own thread, and the
//! pieces of an answer are batched so a fast model causes at most one
//! repaint per ANSWER_FLUSH, however many words it sends.

use super::markdown_view::{MarkdownSnippet, SnippetFonts};
use super::*;
use lightline::ai::{self, Message, Role};
use std::cell::Cell;

pub(super) const AI_EVENT_MESSAGE: u32 = WM_APP + 10;

const ANSWER_FLUSH: Duration = Duration::from_millis(50);
// Code sent along with a question: at most this much.
pub(super) const MAX_CONTEXT_LINES: usize = 400;
const MAX_CONTEXT_BYTES: usize = 24 * 1024;
// Earlier messages sent along for context, newest first, up to this size.
const MAX_HISTORY_BYTES: usize = 48 * 1024;
pub(super) const MAX_INPUT_BYTES: usize = 16 * 1024;
// Lines around an error sent with "Explain" and "Fix" (the fix replaces
// exactly these lines, so it gets fewer).
const ERROR_CONTEXT_EXPLAIN: usize = 10;
const ERROR_CONTEXT_FIX: usize = 3;

/// The model a first-time user is told to download.
pub(super) const SUGGESTED_MODEL: &str = "qwen2.5-coder:7b";

const SYSTEM_PROMPT: &str = "You are the coding assistant in LightLine, a code \
editor. Answer concisely and accurately. Use Markdown, and put code in fenced \
code blocks tagged with the language.";

enum AiEvent {
    Models(Result<Vec<String>, String>),
    Text(u64, String),
    Done(u64, Result<(), String>),
    /// A tool call starting: what it's doing (ai_project.rs).
    Running(u64, super::ai_project::StepKind, String),
    /// A tool call the model made, done (ai_project.rs).
    Step(u64, super::ai_project::AiStep),
    /// The model wants to look through the project: the worker waits for
    /// the answer on this sender.
    Ask(u64, Sender<super::ai_project::Permission>),
    /// A line under the answer, e.g. that the model can't use tools.
    Note(u64, String),
    /// This model refused tools; it isn't offered them again.
    ToolsRefused(String),
    /// A change the model proposed (ai_edits.rs).
    Proposal(u64, lightline::ai_tools::FileProposal),
    /// The provider card's check: its generation and the models found.
    Provider(u64, Result<Vec<String>, String>),
}

/// The chip for what goes with the next question besides attached files.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum ContextOffer {
    /// The selection, e.g. "app.rs, lines 40–82".
    Selection(String),
    /// The open file, sent like an attached one.
    OpenFile(PathBuf),
}

/// Something the editor asks the assistant to do with code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AiTask {
    /// The whole open file (the empty panel's first action).
    ExplainFile,
    Explain,
    Fix,
    Tests,
    Comments,
    ExplainError,
    FixError,
}

impl AiTask {
    fn for_selection(self) -> bool {
        !matches!(
            self,
            AiTask::ExplainError | AiTask::FixError | AiTask::ExplainFile
        )
    }

    // What the chat shows as the question, and what the model is asked.
    fn wording(self) -> (&'static str, &'static str) {
        match self {
            AiTask::ExplainFile => (
                "Explain this file",
                "Explain what this file does: its purpose, its main parts and how they fit \
                 together. Be brief.",
            ),
            AiTask::Explain => (
                "Explain this code",
                "Explain what this code does, briefly and step by step.",
            ),
            AiTask::Fix => (
                "Fix this code",
                "Find and fix the bugs in this code. Reply with the corrected version of \
                 exactly this code in one code block, then briefly list what you changed.",
            ),
            AiTask::Tests => (
                "Write tests for this code",
                "Write unit tests for this code, using the usual test framework for its \
                 language. Reply with the tests in one code block.",
            ),
            AiTask::Comments => (
                "Add comments to this code",
                "Add clear doc comments and comments to this code without changing what \
                 it does. Reply with the complete commented code in one code block.",
            ),
            AiTask::ExplainError => ("Explain this", "Explain this problem and how to fix it."),
            AiTask::FixError => (
                "Fix this",
                "Fix this problem by rewriting the code at the end of this message. Reply \
                 with only those lines, corrected and with the same indentation, in one \
                 code block, then say in one sentence what you changed.",
            ),
        }
    }
}

/// The code a question was about, so Replace on its answer can put the new
/// code back in the same place.
#[derive(Clone)]
struct AiTarget {
    path: PathBuf,
    start: Pos,
    end: Pos,
    // The document's change count when the question was sent: once the
    // file changes, the range may no longer hold the same code.
    serial: u64,
}

// Code sent with a question.
struct AiContext {
    label: String,
    code: String,
    language: String,
    target: Option<AiTarget>,
}

pub(super) struct ChatEntry {
    pub(super) role: Role,
    /// The question as shown, or the answer so far.
    pub(super) text: String,
    /// What was sent to the model for a question: the text plus any code.
    prompt: String,
    /// The code sent with a question, e.g. "app.rs, lines 40–82".
    pub(super) context: Option<String>,
    /// The files sent with a question, as chips.
    pub(super) chips: Vec<super::ai_attach::Chip>,
    /// An answer's look through the project: one line per tool call, each
    /// placed where it came in the text.
    pub(super) steps: Vec<super::ai_project::AiStep>,
    /// The permission card shows in this answer (after its text so far:
    /// nothing more arrives while it waits).
    pub(super) asking: bool,
    /// The tool call running now, e.g. "Reading src/main.rs".
    pub(super) running: Option<(super::ai_project::StepKind, String)>,
    /// The model that answered.
    pub(super) model: String,
    /// Groups of steps (by their first step) opened again after the answer
    /// folded them.
    pub(super) open_groups: Vec<usize>,
    /// When the answer was asked for, and how long it took.
    started: Option<Instant>,
    pub(super) took: Option<Duration>,
    /// A muted line under the answer.
    pub(super) note: Option<String>,
    /// Changes the model proposed in this answer.
    pub(super) proposals: Vec<super::ai_edits::AnswerProposal>,
    target: Option<AiTarget>,
    /// Why an answer ended early.
    pub(super) error: Option<String>,
    /// Bumped whenever `text` changes, so the answer is laid out again.
    pub(super) revision: u64,
    /// The answer's text between its steps, each laid out on its own.
    pub(super) views: RefCell<Vec<MarkdownSnippet>>,
}

impl ChatEntry {
    fn new(role: Role, text: String, prompt: String) -> Self {
        Self {
            role,
            text,
            prompt,
            context: None,
            chips: Vec::new(),
            steps: Vec::new(),
            asking: false,
            running: None,
            model: String::new(),
            open_groups: Vec::new(),
            started: None,
            took: None,
            note: None,
            proposals: Vec::new(),
            target: None,
            error: None,
            revision: 0,
            views: RefCell::default(),
        }
    }
}

/// A button on the changes card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum ProposalButton {
    Review,
    ApplyAll,
    Discard,
}

/// A button on an answer.
#[derive(Clone)]
pub(super) enum AiAction {
    Copy(String),
    /// Put the code at the editor's cursor.
    Insert(String),
    /// Smart apply code into active editor with undo support.
    Apply(String, usize),
    /// Ask the last question again.
    Retry,
}

/// Where the panel's buttons were painted, for mouse clicks.
#[derive(Default, Clone)]
pub(super) struct AiHits {
    pub(super) connect: Option<RECT>,
    pub(super) set_key: Option<RECT>,
    pub(super) model: Option<RECT>,
    pub(super) model_menu: Option<RECT>,
    pub(super) model_search: Option<RECT>,
    pub(super) model_rows: Vec<(RECT, String)>,
    pub(super) model_refresh: Option<RECT>,
    pub(super) model_turn_off: Option<RECT>,
    pub(super) model_provider: Option<RECT>,
    pub(super) provider: Option<RECT>,
    pub(super) new_chat: Option<RECT>,
    pub(super) composer: Option<RECT>,
    pub(super) send: Option<RECT>,
    pub(super) actions: Vec<(RECT, AiAction)>,
    /// The @ list's rows.
    pub(super) mentions: Vec<(RECT, PathBuf)>,
    /// The ✕ on each attached file's chip, by its index.
    pub(super) chip_close: Vec<(RECT, usize)>,
    /// The permission card's buttons.
    pub(super) permission: Vec<(RECT, super::ai_project::Permission)>,
    /// The ✕ on the chip for the selection or the open file.
    pub(super) context_close: Option<RECT>,
    /// The @ button under the message box.
    pub(super) at_button: Option<RECT>,
    /// The empty panel's actions.
    pub(super) suggestions: Vec<(RECT, AiTask)>,
    /// "Read" lines, with the file and line each opens.
    pub(super) steps: Vec<(RECT, PathBuf, usize)>,
    /// A finished answer's folded steps: the line, its entry and the
    /// group's first step.
    pub(super) step_groups: Vec<(RECT, usize, usize)>,
    /// The changes card: a file's row (entry, file) and its buttons.
    pub(super) proposal_rows: Vec<(RECT, usize, usize)>,
    pub(super) proposal_buttons: Vec<(RECT, usize, ProposalButton)>,
}

pub(super) struct AiChat {
    pub(super) entries: Vec<ChatEntry>,
    /// The message being typed, and where in it the caret is (a byte
    /// offset; ai_composer.rs).
    pub(super) input: String,
    pub(super) caret: usize,
    /// The message box as last painted: its wrapped lines (byte ranges)
    /// and the rows drawn, for Up/Down, Home/End and clicks.
    pub(super) composer_lines: RefCell<Vec<(usize, usize)>>,
    pub(super) composer_rows: RefCell<Vec<super::ai_composer::ComposerRow>>,
    /// The first wrapped line shown when the text is taller than the box.
    pub(super) composer_first: Cell<usize>,
    /// Text just copied with a Copy icon, which shows a tick meanwhile.
    pub(super) copied: Option<String>,
    /// Files to send with the next question (ai_attach.rs).
    pub(super) attachments: Vec<super::ai_attach::Attachment>,
    /// The @ list's chosen row, and whether Esc closed it until the next
    /// change to the box.
    pub(super) mention_selected: usize,
    pub(super) mention_dismissed: bool,
    /// The selection or open file the user took off the next question: its
    /// document's path (None: an untitled one), and whether it was the
    /// selection.
    pub(super) context_skipped: Option<(Option<PathBuf>, bool)>,
    /// "Allow for This Chat" was chosen; a new chat asks again.
    pub(super) chat_allowed: bool,
    /// The question waiting on the permission card, and where its answer goes.
    pub(super) permission: Option<(u64, Sender<super::ai_project::Permission>)>,
    /// Models that refused tools this session.
    pub(super) tools_refused: std::collections::HashSet<String>,
    /// Whether the message box has keyboard focus.
    pub(super) focused: bool,
    /// How far the conversation is scrolled, in pixels from its top.
    pub(super) scroll: Cell<i32>,
    /// Keep the newest text in view while an answer arrives, until the
    /// user scrolls up.
    pub(super) follow: Cell<bool>,
    /// The chat models the server listed on the last connect.
    pub(super) models: Vec<String>,
    /// The custom model picker is part of the panel rather than a native
    /// Windows menu, so it can share the workbench theme and support search.
    pub(super) model_menu_open: bool,
    pub(super) model_query: String,
    pub(super) model_menu_first: usize,
    pub(super) connecting: bool,
    /// Why connecting failed.
    pub(super) problem: Option<String>,
    /// Where the endpoint's key comes from, for the panel, kept per
    /// endpoint: reading Credential Manager on every paint would be slow.
    key_source: RefCell<Option<(String, Option<ai::KeySource>)>>,
    // The answer being received: its id and its cancel flag.
    request: Option<(u64, Arc<AtomicBool>)>,
    next_request: u64,
    pub(super) fonts: SnippetFonts,
    pub(super) hits: RefCell<AiHits>,
    tx: Sender<AiEvent>,
    rx: Receiver<AiEvent>,
}

impl AiChat {
    pub(super) fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            entries: Vec::new(),
            input: String::new(),
            caret: 0,
            composer_lines: RefCell::default(),
            composer_rows: RefCell::default(),
            composer_first: Cell::new(0),
            copied: None,
            attachments: Vec::new(),
            mention_selected: 0,
            mention_dismissed: false,
            chat_allowed: false,
            context_skipped: None,
            permission: None,
            tools_refused: Default::default(),
            focused: false,
            scroll: Cell::new(0),
            follow: Cell::new(true),
            models: Vec::new(),
            model_menu_open: false,
            model_query: String::new(),
            model_menu_first: 0,
            connecting: false,
            problem: None,
            key_source: RefCell::new(None),
            request: None,
            next_request: 0,
            fonts: SnippetFonts::default(),
            hits: RefCell::default(),
            tx,
            rx,
        }
    }

    /// Whether an answer is being received.
    pub(super) fn busy(&self) -> bool {
        self.request.is_some()
    }

    /// Reads the key's source again on the next paint, after it changed.
    pub(super) fn forget_key_source(&self) {
        *self.key_source.borrow_mut() = None;
    }
}

/// Models shown by the custom picker: cloud first, local second, with the
/// active model retained even when it was not returned by the latest refresh.
pub(super) fn model_menu_models(
    models: &[String],
    current: Option<&str>,
    query: &str,
) -> Vec<String> {
    let needle = query.trim().to_ascii_lowercase();
    let mut unique = Vec::new();
    if let Some(current) = current
        && !current.is_empty()
    {
        unique.push(current.to_string());
    }
    for model in models {
        if !unique.contains(model) {
            unique.push(model.clone());
        }
    }
    unique.retain(|model| needle.is_empty() || model.to_ascii_lowercase().contains(&needle));
    let (cloud, local): (Vec<_>, Vec<_>) = unique
        .into_iter()
        .partition(|model| ai::is_cloud_model(model));
    cloud.into_iter().chain(local).collect()
}

// Printable virtual keys must continue through the normal Windows text-input
// path so TranslateMessage/WM_CHAR can supply the actual character (including
// keyboard-layout and Shift handling) to the model search field.
fn model_search_text_key(key: u32) -> bool {
    key == VK_SPACE as u32
        || (0x30..=0x39).contains(&key)
        || (0x41..=0x5a).contains(&key)
        || (VK_NUMPAD0 as u32..=VK_DIVIDE as u32).contains(&key)
        || (0xba..=0xdf).contains(&key)
        || key == 0xe2
}

// The language tag for a fenced code block, from the file's extension.
fn fence_language(path: Option<&Path>) -> String {
    path.and_then(Path::extension)
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

// `text` cut to MAX_CONTEXT_LINES lines and MAX_CONTEXT_BYTES bytes.
pub(super) fn clip_context(text: &str) -> (String, bool) {
    let mut clipped = String::new();
    for (index, line) in text.split('\n').enumerate() {
        if index >= MAX_CONTEXT_LINES || clipped.len() + line.len() + 1 > MAX_CONTEXT_BYTES {
            return (clipped, true);
        }
        if index > 0 {
            clipped.push('\n');
        }
        clipped.push_str(line);
    }
    (clipped, false)
}

// The last line a range covers: one ending at the start of a line doesn't
// include that line.
fn last_line(start: Pos, end: Pos) -> usize {
    if end.byte == 0 && end.line > start.line {
        end.line - 1
    } else {
        end.line
    }
}

fn leading_whitespace(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// The indentation shared by the non-blank `lines`: what an answer's code is
/// re-indented to when it replaces them.
fn common_indentation<'a>(lines: impl IntoIterator<Item = &'a str>) -> String {
    lines
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .map(leading_whitespace)
        .min_by_key(|indent| indent.len())
        .unwrap_or("")
        .to_string()
}

/// Lines `first..=last` (an error) widened by up to `margin` lines each way,
/// without leaving the error's block: a line indented less than the error's
/// first line ends it (blank lines don't). Blank lines at the edges are
/// dropped. `line` reads a line of the `count`-line document.
fn block_around<'a>(
    count: usize,
    line: impl Fn(usize) -> &'a str,
    first: usize,
    last: usize,
    margin: usize,
) -> (usize, usize) {
    let blank = |index: usize| line(index).trim().is_empty();
    let depth = leading_whitespace(line(first)).len();
    let inside = |index: usize| blank(index) || leading_whitespace(line(index)).len() >= depth;
    let mut top = first;
    while top > 0 && first - (top - 1) <= margin && inside(top - 1) {
        top -= 1;
    }
    let mut bottom = last;
    while bottom + 1 < count && bottom + 1 - last <= margin && inside(bottom + 1) {
        bottom += 1;
    }
    while top < first && blank(top) {
        top += 1;
    }
    while bottom > last && blank(bottom) {
        bottom -= 1;
    }
    (top, bottom)
}

/// `code` from an answer, re-indented to go where the editor text is
/// `before` (the start of its line, up to where the code goes): the code's
/// own common indentation becomes `base`, so a block the model wrote at
/// column 0 lands correctly inside an indented function, which matters in
/// Python. `whole_lines` keeps the line break a range of whole lines ended
/// with.
fn fit_indentation(code: &str, base: &str, before: &str, whole_lines: bool) -> String {
    let code = code.trim_end_matches(['\n', '\r']);
    let lines: Vec<&str> = code.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let common = common_indentation(lines.iter().copied()).len();
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        if line.trim().is_empty() {
            continue;
        }
        let body = line.get(common..).unwrap_or_else(|| line.trim_start());
        if index > 0 {
            out.push_str(base);
            out.push_str(body);
        } else if before.trim().is_empty() {
            // The indentation already before the code stays; only the rest
            // is added.
            let full = format!("{base}{body}");
            let skip = leading_whitespace(&full).len().min(before.len());
            out.push_str(full.get(skip..).unwrap_or(&full));
        } else {
            // After other code on the line: no indentation.
            out.push_str(body.trim_start());
        }
    }
    if whole_lines {
        out.push('\n');
    }
    out
}

impl App {
    /// Whether a model is chosen, i.e. the assistant is on.
    pub(super) fn ai_ready(&self) -> bool {
        self.settings.ai_model.is_some()
    }

    /// Whether typing goes to the assistant's message box.
    pub(super) fn ai_typing(&self) -> bool {
        self.ai_assistant_visible
            && !self.welcome
            && !self.quick_open
            && self.ai_ready()
            && (self.ai.focused || self.ai.model_menu_open)
    }

    /// Where the key for the endpoint comes from; None without one.
    pub(super) fn ai_key_source(&self) -> Option<ai::KeySource> {
        let endpoint = &self.settings.ai_endpoint;
        let mut cached = self.ai.key_source.borrow_mut();
        match cached.as_ref() {
            Some((known, source)) if known == endpoint => *source,
            _ => {
                let source = ai::find_key(endpoint).map(|(_, source)| source);
                *cached = Some((endpoint.clone(), source));
                source
            }
        }
    }

    /// AI: Remove API Key: forgets the key saved for the endpoint's host.
    pub(super) fn ai_remove_key(&mut self, hwnd: HWND) {
        let endpoint = self.settings.ai_endpoint.clone();
        let host = ai::host(&endpoint);
        *self.ai.key_source.borrow_mut() = None;
        self.status = match ai::remove_key(&endpoint) {
            Ok(true) => format!("Removed the API key for {host}."),
            Ok(false) => format!("No API key was saved for {host}."),
            Err(error) => format!("Couldn't remove the API key for {host}: {error}"),
        };
        let variable = ai::key_variable(&endpoint);
        if std::env::var_os(variable).is_some() && !ai::is_local(&endpoint) {
            self.status.push_str(&format!(
                " {variable} is still set, so that key is used instead."
            ));
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn focus_ai_input(&mut self) {
        self.ai.model_menu_open = false;
        self.ai.focused = true;
        self.terminal_focus = false;
        self.panel_focus = false;
    }

    /// Asks the server which models it has: the Connect button, and the
    /// model menu's refresh.
    pub(super) fn ai_connect(&mut self, hwnd: HWND) {
        if self.ai.connecting {
            return;
        }
        self.ai.connecting = true;
        self.ai.problem = None;
        let server = ai::server(&self.settings.ai_endpoint);
        let tx = self.ai.tx.clone();
        let hwnd_value = hwnd as isize;
        std::thread::spawn(move || {
            let _ = tx.send(AiEvent::Models(ai::list_models(&server)));
            unsafe { PostMessageW(hwnd_value as HWND, AI_EVENT_MESSAGE, 0, 0) };
        });
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// The provider card's check: asks `server` for its models on a worker
    /// thread; the answer comes back as AiEvent::Provider.
    pub(super) fn ai_check_provider(&mut self, hwnd: HWND, server: ai::Server, generation: u64) {
        let tx = self.ai.tx.clone();
        let hwnd_value = hwnd as isize;
        std::thread::spawn(move || {
            let _ = tx.send(AiEvent::Provider(generation, ai::list_models(&server)));
            unsafe { PostMessageW(hwnd_value as HWND, AI_EVENT_MESSAGE, 0, 0) };
        });
    }

    /// Takes in what the worker threads sent (AI_EVENT_MESSAGE).
    pub(super) fn poll_ai(&mut self, hwnd: HWND) {
        let mut changed = false;
        while let Ok(event) = self.ai.rx.try_recv() {
            changed = true;
            match event {
                AiEvent::Models(result) => self.ai_models_listed(result),
                AiEvent::Provider(generation, result) => {
                    self.provider_checked(hwnd, generation, result)
                }
                AiEvent::Text(id, text) => {
                    if self.ai_current(id)
                        && let Some(entry) = self.ai.entries.last_mut()
                    {
                        entry.text.push_str(&text);
                        entry.revision += 1;
                    }
                }
                AiEvent::Running(id, kind, label) => {
                    if self.ai_current(id)
                        && let Some(entry) = self.ai.entries.last_mut()
                    {
                        entry.running = Some((kind, label));
                    }
                }
                AiEvent::Step(id, mut step) => {
                    if self.ai_current(id)
                        && let Some(entry) = self.ai.entries.last_mut()
                    {
                        entry.asking = false;
                        entry.running = None;
                        step.at = entry.text.len();
                        entry.steps.push(step);
                    }
                }
                AiEvent::Ask(id, reply) => {
                    if self.ai_current(id)
                        && let Some(entry) = self.ai.entries.last_mut()
                    {
                        entry.asking = true;
                        self.ai.permission = Some((id, reply));
                        self.ai.follow.set(true);
                    }
                }
                AiEvent::Note(id, note) => {
                    if self.ai_current(id)
                        && let Some(entry) = self.ai.entries.last_mut()
                    {
                        entry.note = Some(note);
                    }
                }
                AiEvent::ToolsRefused(model) => {
                    self.ai.tools_refused.insert(model);
                }
                AiEvent::Proposal(id, proposal) => {
                    if self.ai_current(id) && !self.ai.entries.is_empty() {
                        let entry = self.ai.entries.len() - 1;
                        self.ai_add_proposal(entry, proposal);
                    }
                }
                AiEvent::Done(id, result) => {
                    if self.ai_current(id) {
                        self.ai.request = None;
                        self.ai.permission = None;
                        if let Some(entry) = self.ai.entries.last_mut() {
                            entry.asking = false;
                            entry.running = None;
                            entry.took = entry.started.map(|started| started.elapsed());
                        }
                        if let (Err(mut error), Some(entry)) = (result, self.ai.entries.last_mut())
                        {
                            if error.contains("API key") {
                                error.push_str(" Change it with AI: Choose Provider\u{2026}");
                            }
                            entry.error = Some(error);
                        }
                    }
                }
            }
        }
        if changed {
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    fn ai_current(&self, id: u64) -> bool {
        self.ai
            .request
            .as_ref()
            .is_some_and(|(current, _)| *current == id)
    }

    fn ai_models_listed(&mut self, result: Result<Vec<String>, String>) {
        self.ai.connecting = false;
        let models = match result {
            Ok(models) => ai::chat_models(&models),
            Err(error) => {
                // Nothing running at Ollama's address: say how to start it.
                let hint = if error.starts_with(ai::NOTHING_ANSWERED)
                    && self.settings.ai_endpoint == ai::DEFAULT_ENDPOINT
                {
                    " Is Ollama running? Open the Ollama app (or run: ollama serve), then try again."
                } else {
                    ""
                };
                self.ai.problem = Some(format!("{error}{hint}"));
                return;
            }
        };
        self.ai.models = models;
        let remote = !ai::is_local(&self.settings.ai_endpoint);
        if self.ai.models.is_empty() {
            self.ai.problem = Some(if remote {
                format!(
                    "{} listed no chat models.",
                    ai::host(&self.settings.ai_endpoint)
                )
            } else {
                format!(
                    "The server has no chat models yet. In a terminal, run: ollama pull \
                     {SUGGESTED_MODEL}"
                )
            });
            return;
        }
        self.ai.problem = None;
        let current = self.settings.ai_model.clone();
        if current.is_some_and(|model| self.ai.models.contains(&model)) {
            return;
        }
        match ai::pick_model(&self.ai.models, remote) {
            Some(model) => {
                self.set_ai_model(Some(model));
                if self.ai_assistant_visible {
                    self.focus_ai_input();
                }
            }
            // Only cloud models: none is picked for the user, since their
            // questions and code would leave the PC.
            None => {
                self.ai.problem = Some(format!(
                    "Only cloud models are installed. They run on ollama.com, not on your \
                     PC. For a local model, run in a terminal: ollama pull {SUGGESTED_MODEL}"
                ));
            }
        }
    }

    fn set_ai_model(&mut self, model: Option<String>) {
        self.settings.ai_model = model;
        let label = self.settings.ai_model.as_deref().map_or_else(
            || "AI Assistant turned off".to_string(),
            |model| format!("AI model: {model}"),
        );
        self.status = match self.settings.save() {
            Ok(()) => label,
            Err(error) => format!("{label} (not saved: {error})"),
        };
    }

    // Whether the active tab is a file that code can be put into.
    fn ai_editable(&self) -> bool {
        !self.welcome && !self.tab().read_only() && !self.tab().is_placeholder()
    }

    // "app.rs, lines 40–82" for a range of the active document.
    fn ai_range_label(&self, start: Pos, end: Pos) -> String {
        let name = self
            .doc()
            .path
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(
                || "Untitled".to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
        let last = last_line(start, end);
        if last == start.line {
            format!("{name}, line {}", start.line + 1)
        } else {
            format!("{name}, lines {}\u{2013}{}", start.line + 1, last + 1)
        }
    }

    /// What the next question will include, e.g. "app.rs, lines 40–82";
    /// None without a selection. Cheap enough for every repaint: the
    /// selected text itself is only read when a message is sent.
    pub(super) fn ai_selection_label(&self) -> Option<String> {
        if !self.ai_editable() {
            return None;
        }
        let (start, end) = self.selection_range()?;
        Some(self.ai_range_label(start, end))
    }

    // The code in a range of the active document, to send with a question.
    fn ai_context(&self, start: Pos, end: Pos) -> Option<AiContext> {
        let code = self.doc().text_range(start, end);
        if code.trim().is_empty() {
            return None;
        }
        let path = self.doc().path.clone();
        Some(AiContext {
            label: self.ai_range_label(start, end),
            code,
            language: fence_language(path.as_deref()),
            target: path.map(|path| AiTarget {
                path,
                start,
                end,
                serial: self.doc().change_serial(),
            }),
        })
    }

    fn ai_selection_context(&self) -> Option<AiContext> {
        if !self.ai_editable() {
            return None;
        }
        let (start, end) = self.selection_range()?;
        self.ai_context(start, end)
    }

    /// What goes with the next question besides attached files, shown as a
    /// chip: the selection, else (with aiIncludeOpenFile) the open file.
    /// None once the user took it off, until the question is sent.
    pub(super) fn ai_context_offer(&self) -> Option<ContextOffer> {
        if !self.ai_editable() {
            return None;
        }
        let path = self.doc().path.clone();
        let selection = self.ai_selection_label();
        if self.ai.context_skipped.as_ref() == Some(&(path.clone(), selection.is_some())) {
            return None;
        }
        if let Some(label) = selection {
            return Some(ContextOffer::Selection(label));
        }
        let path = path?;
        let attached = self
            .ai
            .attachments
            .iter()
            .any(|attached| Self::same_path(&attached.path, &path));
        (self.settings.ai_include_open_file && !attached && !super::ai_attach::looks_secret(&path))
            .then_some(ContextOffer::OpenFile(path))
    }

    /// Sends the typed message, with the selected code.
    pub(super) fn ai_send(&mut self, hwnd: HWND) {
        let question = self.ai.input.trim().to_string();
        if question.is_empty() || self.ai.busy() {
            return;
        }
        // What the chip above the text offers: the selection, or the open
        // file as an attachment.
        let offer = self.ai_context_offer();
        let context = match &offer {
            Some(ContextOffer::Selection(_)) => self.ai_selection_context(),
            _ => None,
        };
        let added = match offer {
            Some(ContextOffer::OpenFile(path)) => {
                let name = self.ai_file_name(&path);
                self.ai.attachments.push(super::ai_attach::Attachment {
                    path,
                    name,
                    secret: false,
                });
                true
            }
            _ => false,
        };
        // Asks first about a secrets file; "Don't Send" keeps the question.
        let Some(files) = self.ai_files_for_question(hwnd) else {
            if added {
                self.ai.attachments.pop();
            }
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        self.ai.context_skipped = None;
        self.ai_clear_input();
        self.ai_ask(hwnd, question.clone(), question, context, files);
    }

    /// The error or warning at the cursor, errors first.
    pub(super) fn ai_diagnostic_at_cursor(&self) -> Option<LspDiagnostic> {
        if !self.ai_editable() {
            return None;
        }
        let line = self.view().cursor.line as u32;
        self.tab()
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.range.start.line <= line && line <= diagnostic.range.end.line
            })
            .min_by_key(|diagnostic| diagnostic.severity)
            .cloned()
    }

    /// Runs `task` on the selection (or on the error at the cursor), showing
    /// the answer in the panel.
    pub(super) fn ai_run_task(&mut self, hwnd: HWND, task: AiTask) {
        if !self.ai_assistant_visible {
            self.ai_assistant_visible = true;
            self.keep_active_tab_visible(hwnd);
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
        if !self.ai_ready() {
            self.status = "Connect a model in the AI Assistant first".into();
            return;
        }
        if self.ai.busy() {
            self.status = "The assistant is still answering (Esc in its box stops it)".into();
            return;
        }
        let (shown, instruction) = task.wording();
        if task == AiTask::ExplainFile {
            if !self.ai_editable() {
                self.status = "Open a file first".into();
                return;
            }
            let last = self.doc().line_count().saturating_sub(1);
            let end = Pos {
                line: last,
                byte: self.doc().line(last).len(),
            };
            if let Some(context) = self.ai_context(Pos { line: 0, byte: 0 }, end) {
                self.ai_ask(
                    hwnd,
                    shown.into(),
                    instruction.into(),
                    Some(context),
                    Vec::new(),
                );
            }
            return;
        }
        if task.for_selection() {
            let Some(context) = self.ai_selection_context() else {
                self.status = "Select some code first".into();
                return;
            };
            self.ai_ask(
                hwnd,
                shown.into(),
                instruction.into(),
                Some(context),
                Vec::new(),
            );
            return;
        }

        let Some(diagnostic) = self.ai_diagnostic_at_cursor() else {
            self.status = "No error or warning at the cursor".into();
            return;
        };
        let lines = self.doc().line_count();
        let error_first = (diagnostic.range.start.line as usize).min(lines.saturating_sub(1));
        let error_last =
            (diagnostic.range.end.line as usize).clamp(error_first, lines.saturating_sub(1));
        let (first, last) = if task == AiTask::FixError {
            // The fix replaces these lines, so they stay inside the error's
            // block: then the answer's code can take their indentation.
            let doc = self.doc();
            block_around(
                lines,
                |line| doc.line(line),
                error_first,
                error_last,
                ERROR_CONTEXT_FIX,
            )
        } else {
            (
                error_first.saturating_sub(ERROR_CONTEXT_EXPLAIN),
                (error_last + ERROR_CONTEXT_EXPLAIN).min(lines.saturating_sub(1)),
            )
        };
        let start = Pos {
            line: first,
            byte: 0,
        };
        let end = if last + 1 < lines {
            Pos {
                line: last + 1,
                byte: 0,
            }
        } else {
            Pos {
                line: last,
                byte: self.doc().line(last).len(),
            }
        };
        if task == AiTask::FixError {
            // Select what the fix will replace, so it's visible before
            // Replace is clicked.
            self.view_mut().selection_anchor = Some(start);
            self.view_mut().cursor = end;
            self.refresh(hwnd);
        }
        let Some(context) = self.ai_context(start, end) else {
            return;
        };
        let kind = match diagnostic.severity {
            1 => "error",
            2 => "warning",
            _ => "problem",
        };
        let message = diagnostic.message.lines().next().unwrap_or("").trim();
        let line = diagnostic.range.start.line + 1;
        let mut instruction = format!("{instruction}\n\nThe {kind}, on line {line}:\n{message}");
        if task == AiTask::FixError {
            // Only the error's block is rewritten; the lines around it help
            // the model understand it.
            let around_first = error_first.saturating_sub(ERROR_CONTEXT_EXPLAIN);
            let around_last = (error_last + ERROR_CONTEXT_EXPLAIN).min(lines.saturating_sub(1));
            if (around_first, around_last) != (first, last) {
                let around: Vec<&str> = (around_first..=around_last)
                    .map(|index| self.doc().line(index))
                    .collect();
                let (around, _) = clip_context(&around.join("\n"));
                instruction.push_str(&format!(
                    "\n\nSurrounding code (lines {}\u{2013}{}), for context only:\n```{}\n{around}\n```",
                    around_first + 1,
                    around_last + 1,
                    context.language
                ));
            }
        }
        self.ai_ask(
            hwnd,
            format!("{shown} {kind}: {message}"),
            instruction,
            Some(context),
            Vec::new(),
        );
    }

    // Sends a question: `shown` in the chat, `instruction` (plus any code) to
    // the model, with the conversation so far.
    fn ai_ask(
        &mut self,
        hwnd: HWND,
        shown: String,
        instruction: String,
        context: Option<AiContext>,
        files: Vec<super::ai_attach::SentFile>,
    ) {
        let Some(model) = self.settings.ai_model.clone() else {
            return;
        };
        if self.ai.busy() {
            return;
        }
        let mut prompt = match &context {
            Some(context) => {
                let (code, clipped) = clip_context(&context.code);
                let note = if clipped { " (cut short)" } else { "" };
                format!(
                    "{instruction}\n\nCode ({}){note}:\n```{}\n{code}\n```",
                    context.label, context.language
                )
            }
            None => instruction,
        };
        for file in files.iter().filter(|file| !file.text.is_empty()) {
            let note = if file.cut { " (cut short)" } else { "" };
            prompt.push_str(&format!(
                "\n\nFile {}{note}:\n```{}\n{}\n```",
                file.name,
                file.language,
                file.text.trim_end_matches('\n')
            ));
        }

        let mut question = ChatEntry::new(Role::User, shown, prompt);
        if let Some(context) = context {
            question.context = Some(context.label);
            question.target = context.target;
        }
        question.chips = files.into_iter().map(|file| file.chip).collect();
        self.ai_start(hwnd, model, question);
    }

    /// Asks the last question again: its answer goes, and a new one comes.
    pub(super) fn ai_retry(&mut self, hwnd: HWND) {
        let Some(model) = self.settings.ai_model.clone() else {
            return;
        };
        let count = self.ai.entries.len();
        if self.ai.busy()
            || count < 2
            || self.ai.entries[count - 1].role != Role::Assistant
            || self.ai.entries[count - 2].role != Role::User
        {
            return;
        }
        if self.ai_review.is_some() {
            self.close_diff();
        }
        self.ai.entries.pop();
        let Some(question) = self.ai.entries.pop() else {
            return;
        };
        self.ai_start(hwnd, model, question);
    }

    // Sends `question` (its prompt already holds any code) with the
    // conversation so far, and starts receiving its answer.
    fn ai_start(&mut self, hwnd: HWND, model: String, question: ChatEntry) {
        let prompt = question.prompt.clone();
        // Earlier messages give the model the conversation so far.
        let mut history = Vec::new();
        let mut budget = MAX_HISTORY_BYTES;
        for entry in self.ai.entries.iter().rev() {
            let content = match entry.role {
                Role::Assistant => ai::visible_answer(&entry.text).0,
                _ => entry.prompt.as_str(),
            };
            if content.is_empty() {
                continue;
            }
            if content.len() > budget {
                break;
            }
            budget -= content.len();
            history.push(Message::new(entry.role, content));
        }
        history.reverse();
        let mut messages = vec![Message::new(Role::System, SYSTEM_PROMPT)];
        let access = self.ai_project_access(&model);
        if let Some(access) = &access {
            messages.push(Message::new(Role::System, access.instructions));
        }
        messages.extend(history);
        messages.push(Message::new(Role::User, prompt));

        self.ai.entries.push(question);
        let mut answer = ChatEntry::new(Role::Assistant, String::new(), String::new());
        answer.started = Some(Instant::now());
        answer.model = model.clone();
        self.ai.entries.push(answer);
        self.ai.follow.set(true);

        self.ai.next_request += 1;
        let id = self.ai.next_request;
        let cancel = Arc::new(AtomicBool::new(false));
        self.ai.request = Some((id, Arc::clone(&cancel)));
        let server = ai::server(&self.settings.ai_endpoint);
        let tx = self.ai.tx.clone();
        let hwnd_value = hwnd as isize;
        std::thread::spawn(move || {
            use super::ai_project::{AiStep, Permission};
            let wake = move || unsafe {
                PostMessageW(hwnd_value as HWND, AI_EVENT_MESSAGE, 0, 0);
            };
            let pending = RefCell::new(String::new());
            let flushed = Cell::new(Instant::now());
            // What's held goes out now: before a step's line, and at the end.
            let flush = || {
                let text = std::mem::take(&mut *pending.borrow_mut());
                if !text.is_empty() {
                    let _ = tx.send(AiEvent::Text(id, text));
                    wake();
                }
                flushed.set(Instant::now());
            };
            // An empty `text` is the stream saying the answer has paused:
            // whatever is held then goes out instead of waiting for more.
            let on_text = |text: &str| {
                pending.borrow_mut().push_str(text);
                if !pending.borrow().is_empty() && flushed.get().elapsed() >= ANSWER_FLUSH {
                    flush();
                }
            };
            let result = match access {
                None => ai::stream_chat(&server, &model, &messages, &cancel, on_text),
                Some(access) => {
                    let tools = lightline::ai_tools::definitions();
                    let mut allowed = access.allowed.then_some(true);
                    let run_tool = |call: &ai::ToolCall| {
                        flush();
                        if allowed.is_none() {
                            let (reply_tx, reply) = mpsc::channel();
                            let _ = tx.send(AiEvent::Ask(id, reply_tx));
                            wake();
                            let answer = loop {
                                if cancel.load(Ordering::Relaxed) {
                                    break Permission::Deny;
                                }
                                match reply.recv_timeout(Duration::from_millis(100)) {
                                    Ok(answer) => break answer,
                                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                                        break Permission::Deny;
                                    }
                                }
                            };
                            allowed = Some(answer != Permission::Deny);
                        }
                        let kind = super::ai_project::StepKind::of(&call.name);
                        let step = if allowed == Some(true) {
                            let input = call.input.clone().unwrap_or_default();
                            let label = lightline::ai_tools::running_label(&call.name, &input);
                            let _ = tx.send(AiEvent::Running(id, kind, label));
                            wake();
                            let outcome = access.project.run(&call.name, &input, &cancel);
                            if let Some(proposal) = outcome.proposal.clone() {
                                let _ = tx.send(AiEvent::Proposal(id, *proposal));
                            }
                            let step = AiStep {
                                summary: outcome.summary,
                                error: outcome.is_error,
                                kind,
                                opened: outcome.opened,
                                at: 0,
                            };
                            (
                                step,
                                ai::ToolReply {
                                    content: outcome.content,
                                    is_error: outcome.is_error,
                                },
                            )
                        } else {
                            (
                                AiStep {
                                    summary: "Not allowed to look through the project".into(),
                                    error: true,
                                    kind,
                                    opened: None,
                                    at: 0,
                                },
                                ai::ToolReply {
                                    content: "The user didn't allow looking through the project \
                                              for this question. Answer without it, and suggest \
                                              attaching files with @ if the answer needs them."
                                        .into(),
                                    is_error: true,
                                },
                            )
                        };
                        let _ = tx.send(AiEvent::Step(id, step.0));
                        wake();
                        step.1
                    };
                    ai::chat_with_tools(
                        &server, &model, &messages, &tools, &cancel, on_text, run_tool,
                    )
                    .map(|ended| {
                        if ended.tools_refused {
                            let _ = tx.send(AiEvent::ToolsRefused(model.clone()));
                            let _ = tx.send(AiEvent::Note(
                                id,
                                "This model can't look through the project; attach files with \
                                 @ instead."
                                    .into(),
                            ));
                        }
                    })
                }
            };
            flush();
            let _ = tx.send(AiEvent::Done(id, result));
            wake();
        });
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Stops the answer being received; what arrived so far stays.
    pub(super) fn ai_stop(&mut self, hwnd: HWND) {
        if let Some((_, cancel)) = self.ai.request.take() {
            cancel.store(true, Ordering::Relaxed);
            // A worker waiting on the permission card stops waiting.
            self.ai.permission = None;
            if let Some(entry) = self.ai.entries.last_mut() {
                entry.error = Some("Stopped".into());
                entry.asking = false;
                entry.running = None;
                entry.took = entry.started.map(|started| started.elapsed());
            }
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn ai_new_chat(&mut self, hwnd: HWND) {
        self.ai_stop(hwnd);
        if self.ai_review.is_some() {
            self.close_diff();
        }
        self.ai.entries.clear();
        self.ai.chat_allowed = false;
        self.ai.scroll.set(0);
        self.ai.follow.set(true);
        self.focus_ai_input();
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Whether Insert can put code at the editor's cursor.
    pub(super) fn ai_can_insert(&self) -> bool {
        self.ai_editable()
    }

    /// Where Replace on answer `index` puts its code: the range its question
    /// was about while that file is unchanged, else the current selection.
    pub(super) fn ai_replace_target(&self, index: usize) -> Option<(usize, Pos, Pos)> {
        let stored = index
            .checked_sub(1)
            .and_then(|question| self.ai.entries.get(question))
            .and_then(|question| question.target.as_ref());
        if let Some(target) = stored
            && let Some(tab) = self.tabs.iter().position(|tab| {
                tab.markdown.is_none() && tab.document.path.as_deref() == Some(&target.path)
            })
            && self.tabs[tab].document.change_serial() == target.serial
        {
            return Some((tab, target.start, target.end));
        }
        if !self.ai_editable() {
            return None;
        }
        let (start, end) = self.selection_range()?;
        Some((self.active, start, end))
    }

    // Puts `code` from an answer into tab `tab` in place of `start..end`
    // (the cursor, for Insert), indented to fit, as one undo step.
    fn ai_apply_code(&mut self, hwnd: HWND, code: &str, tab: usize, start: Pos, end: Pos) {
        if tab != self.active {
            self.set_active_index(tab);
            self.show_active_tab(hwnd);
        }
        if !self.ai_editable() {
            return;
        }
        let replacing = start != end;
        let line = self.doc().line(start.line).to_string();
        let before = line.get(..start.byte).unwrap_or("");
        // Replaced code keeps the indentation its lines had; inserted code
        // takes the cursor line's.
        let base = if replacing {
            let doc = self.doc();
            common_indentation((start.line..=last_line(start, end)).map(|index| doc.line(index)))
        } else {
            leading_whitespace(&line).to_string()
        };
        let whole_lines = end.byte == 0 && end.line > start.line;
        let text = fit_indentation(code, &base, before, whole_lines);
        let label = self.ai_range_label(start, end);
        self.replace_range(start, end, &text);
        self.view_mut().selection_anchor = None;
        self.ai.focused = false;
        self.status = if replacing {
            format!("Replaced {label} with the assistant's code (Ctrl+Z undoes it)")
        } else {
            "Inserted the assistant's code (Ctrl+Z undoes it)".into()
        };
        self.refresh(hwnd);
    }

    /// Opens the custom, searchable model picker painted inside the panel.
    pub(super) fn ai_model_menu(&mut self, hwnd: HWND, _x: i32, _y: i32) {
        self.ai.model_menu_open = !self.ai.model_menu_open;
        self.ai.focused = false;
        self.ai.model_query.clear();
        self.ai.model_menu_first = 0;
        if self.ai.model_menu_open {
            take_keyboard(hwnd);
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// A click inside the panel.
    pub(super) fn ai_click(&mut self, hwnd: HWND, x: i32, y: i32) {
        let inside =
            |rect: &RECT| x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let chrome_top = self.chrome_top();
        if y >= chrome_top
            && y <= chrome_top + self.scale(AI_HEADER)
            && x >= client.right - self.chrome_gap() - self.scale(34)
        {
            self.ai.focused = false;
            self.ai.model_menu_open = false;
            self.toggle_ai_assistant(hwnd);
            return;
        }
        let hits = self.ai.hits.borrow().clone();
        if self.ai.model_menu_open {
            if hits.model_search.as_ref().is_some_and(inside) {
                self.ai.focused = false;
                take_keyboard(hwnd);
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            }
            if let Some((_, model)) = hits.model_rows.iter().find(|(rect, _)| inside(rect)) {
                self.ai.model_menu_open = false;
                self.set_ai_model(Some(model.clone()));
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            }
            if hits.model_refresh.as_ref().is_some_and(inside) {
                self.ai_connect(hwnd);
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            }
            if hits.model_provider.as_ref().is_some_and(inside) {
                self.ai.model_menu_open = false;
                self.show_ai_provider(hwnd, false);
                return;
            }
            if hits.model_turn_off.as_ref().is_some_and(inside) {
                self.ai.model_menu_open = false;
                self.ai_stop(hwnd);
                self.ai.focused = false;
                self.set_ai_model(None);
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            }
            let inside_menu = hits.model_menu.as_ref().is_some_and(inside);
            let inside_chip = hits.model.as_ref().is_some_and(inside);
            if inside_menu {
                // The search box owns keyboard input whenever the picker is
                // open; clicks on its padding merely keep it open.
                return;
            }
            if !inside_chip {
                self.ai.model_menu_open = false;
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            }
        }
        if let Some((_, entry, button)) =
            hits.proposal_buttons.iter().find(|(rect, ..)| inside(rect))
        {
            match button {
                ProposalButton::Review => self.ai_proposals_review(hwnd, *entry),
                ProposalButton::ApplyAll => self.ai_proposals_apply_all(hwnd, *entry),
                ProposalButton::Discard => self.ai_proposals_discard(hwnd, *entry),
            }
        } else if let Some((_, entry, file)) =
            hits.proposal_rows.iter().find(|(rect, ..)| inside(rect))
        {
            self.ai_review(hwnd, *entry, *file);
        } else if let Some((_, answer)) = hits.permission.iter().find(|(rect, _)| inside(rect)) {
            self.ai_answer_permission(hwnd, *answer);
        } else if let Some((_, path, line)) = hits.steps.iter().find(|(rect, ..)| inside(rect)) {
            self.ai_open_step(hwnd, path.clone(), *line);
        } else if let Some((_, entry, first)) =
            hits.step_groups.iter().find(|(rect, ..)| inside(rect))
        {
            if let Some(entry) = self.ai.entries.get_mut(*entry) {
                if let Some(at) = entry.open_groups.iter().position(|open| open == first) {
                    entry.open_groups.remove(at);
                } else {
                    entry.open_groups.push(*first);
                }
            }
        } else if let Some((_, path)) = hits.mentions.iter().find(|(rect, _)| inside(rect)) {
            self.ai_accept_mention(hwnd, Some(path.clone()));
            self.focus_ai_input();
        } else if hits.context_close.as_ref().is_some_and(inside) {
            let selection = self.ai_selection_label().is_some();
            self.ai.context_skipped = Some((self.doc().path.clone(), selection));
            self.focus_ai_input();
        } else if hits.at_button.as_ref().is_some_and(inside) {
            self.focus_ai_input();
            self.ai_insert("@");
        } else if let Some((_, task)) = hits.suggestions.iter().find(|(rect, _)| inside(rect)) {
            self.ai_run_task(hwnd, *task);
            self.focus_ai_input();
        } else if let Some((_, index)) = hits.chip_close.iter().find(|(rect, _)| inside(rect)) {
            self.ai_remove_attachment(hwnd, *index);
            self.focus_ai_input();
        } else if hits.new_chat.as_ref().is_some_and(inside) {
            self.ai_new_chat(hwnd);
        } else if let Some(chip) = hits.model.filter(inside) {
            self.ai_model_menu(hwnd, chip.left, chip.bottom);
        } else if hits.connect.as_ref().is_some_and(inside) {
            self.ai_connect(hwnd);
        } else if hits.set_key.as_ref().is_some_and(inside) {
            self.show_ai_provider(hwnd, true);
            return;
        } else if hits.provider.as_ref().is_some_and(inside) {
            self.show_ai_provider(hwnd, false);
            return;
        } else if hits.send.as_ref().is_some_and(inside) {
            if self.ai.busy() {
                self.ai_stop(hwnd);
            } else {
                self.ai_send(hwnd);
            }
            self.focus_ai_input();
        } else if hits.composer.as_ref().is_some_and(inside) {
            self.focus_ai_input();
            self.ai_click_caret(hwnd, x, y);
        } else if let Some((_, action)) = hits.actions.iter().find(|(rect, _)| inside(rect)) {
            match action {
                AiAction::Copy(text) => self.ai_copy(hwnd, text),
                AiAction::Insert(code) => {
                    if self.ai_can_insert() {
                        let cursor = self.view().cursor;
                        self.ai_apply_code(hwnd, code, self.active, cursor, cursor);
                    }
                }
                AiAction::Apply(code, answer) => {
                    if let Some((tab, start, end)) = self.ai_replace_target(*answer) {
                        self.ai_apply_code(hwnd, code, tab, start, end);
                    } else if let Some((start, end)) = self.selection_range() {
                        self.ai_apply_code(hwnd, code, self.active, start, end);
                    } else if self.ai_can_insert() {
                        let cursor = self.view().cursor;
                        self.ai_apply_code(hwnd, code, self.active, cursor, cursor);
                    }
                }
                AiAction::Retry => self.ai_retry(hwnd),
            }
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// The mouse wheel over the conversation.
    pub(super) fn ai_scroll(&mut self, hwnd: HWND, delta: i32) {
        if self.ai.model_menu_open {
            let models = model_menu_models(
                &self.ai.models,
                self.settings.ai_model.as_deref(),
                &self.ai.model_query,
            );
            let max_first = models.len().saturating_sub(6);
            if delta > 0 {
                self.ai.model_menu_first = self.ai.model_menu_first.saturating_sub(1);
            } else if delta < 0 {
                self.ai.model_menu_first = (self.ai.model_menu_first + 1).min(max_first);
            }
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let step = self.scale(48) * delta / 120;
        let scroll = (self.ai.scroll.get() - step).max(0);
        self.ai.scroll.set(scroll);
        // Scrolling up stops following the answer; painting turns it back
        // on when the view reaches the bottom again.
        if delta > 0 {
            self.ai.follow.set(false);
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// A key while the message box has focus. Keys that would edit the
    /// document behind it are kept here; app-wide shortcuts (Ctrl+P, Ctrl+S,
    /// F5...) still work.
    pub(super) fn ai_key(&mut self, hwnd: HWND, key: u32, ctrl: bool, shift: bool) -> bool {
        if self.ai.model_menu_open {
            return match key {
                k if k == VK_ESCAPE as u32 => {
                    self.ai.model_menu_open = false;
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    true
                }
                k if k == VK_BACK as u32 => {
                    self.ai.model_query.pop();
                    self.ai.model_menu_first = 0;
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                    true
                }
                0x56 if ctrl => {
                    if let Ok(Some(text)) = clipboard::paste(hwnd) {
                        for ch in text.chars().filter(|ch| !ch.is_control()) {
                            self.ai.model_query.push(ch);
                        }
                        self.ai.model_menu_first = 0;
                        unsafe { InvalidateRect(hwnd, null(), 0) };
                    }
                    true
                }
                _ if (VK_F1 as u32..=VK_F24 as u32).contains(&key) => false,
                _ if ctrl => false,
                _ if model_search_text_key(key) => false,
                _ => true,
            };
        }
        if let Some(handled) = self.ai_mention_key(hwnd, key, ctrl) {
            return handled;
        }
        if let Some(handled) = self.ai_caret_key(hwnd, key, ctrl) {
            return handled;
        }
        match key {
            k if k == VK_RETURN as u32 && !ctrl => {
                if shift {
                    self.ai_type(hwnd, "\n");
                } else {
                    self.ai_send(hwnd);
                }
                true
            }
            k if k == VK_ESCAPE as u32 => {
                if self.ai.busy() {
                    self.ai_stop(hwnd);
                } else {
                    self.ai.focused = false;
                    unsafe { InvalidateRect(hwnd, null(), 0) };
                }
                true
            }
            0x56 if ctrl => {
                if let Ok(Some(text)) = clipboard::paste(hwnd) {
                    self.ai_type(hwnd, &text.replace("\r\n", "\n"));
                }
                true
            }
            _ if (VK_F1 as u32..=VK_F24 as u32).contains(&key) => false,
            // Ctrl+P, Ctrl+S, Ctrl+O, Ctrl+N, Ctrl+W, Ctrl+Tab, Ctrl+`, Ctrl+,.
            _ if ctrl => ![
                0x50,
                0x53,
                0x4f,
                0x4e,
                0x57,
                VK_TAB as u32,
                VK_OEM_3 as u32,
                VK_OEM_COMMA as u32,
            ]
            .contains(&key),
            _ => true,
        }
    }

    /// A typed character for the message box.
    pub(super) fn ai_char(&mut self, hwnd: HWND, unit: u16) {
        if unit < 32 || unit == 127 {
            return;
        }
        let ch = if (0xd800..=0xdbff).contains(&unit) {
            self.pending_high_surrogate = Some(unit);
            None
        } else if (0xdc00..=0xdfff).contains(&unit) {
            self.pending_high_surrogate.take().and_then(|high| {
                char::from_u32(0x10000 + ((high as u32 - 0xd800) << 10) + (unit as u32 - 0xdc00))
            })
        } else {
            self.pending_high_surrogate = None;
            char::from_u32(unit as u32)
        };
        if let Some(ch) = ch {
            if self.ai.model_menu_open {
                if self.ai.model_query.len() < 128 {
                    self.ai.model_query.push(ch);
                    self.ai.model_menu_first = 0;
                }
                unsafe { InvalidateRect(hwnd, null(), 0) };
            } else {
                self.ai_type(hwnd, ch.encode_utf8(&mut [0; 4]));
            }
        }
    }

    fn ai_type(&mut self, hwnd: HWND, text: &str) {
        self.ai_insert(text);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_picker_groups_filters_and_deduplicates_models() {
        let models = vec![
            "llama3.2:1b".to_string(),
            "gpt-oss:120b-cloud".to_string(),
            "qwen2.5-coder:7b".to_string(),
            "gpt-oss:120b-cloud".to_string(),
        ];

        assert_eq!(
            model_menu_models(&models, Some("gemma3:4b-cloud"), ""),
            [
                "gemma3:4b-cloud",
                "gpt-oss:120b-cloud",
                "llama3.2:1b",
                "qwen2.5-coder:7b",
            ]
        );
        assert_eq!(
            model_menu_models(&models, None, "CoDeR"),
            ["qwen2.5-coder:7b"]
        );
        assert!(model_search_text_key(0x51)); // Q
        assert!(model_search_text_key(0xbd)); // -
        assert!(!model_search_text_key(VK_LEFT as u32));
    }

    #[test]
    fn code_blocks_are_tagged_by_extension() {
        assert_eq!(fence_language(Some(Path::new("src/app.RS"))), "rs");
        assert_eq!(fence_language(Some(Path::new("Makefile"))), "");
        assert_eq!(fence_language(None), "");
    }

    #[test]
    fn long_selections_are_cut_short() {
        assert_eq!(clip_context("a\nb"), ("a\nb".to_string(), false));
        let many = "x\n".repeat(MAX_CONTEXT_LINES + 50);
        let (clipped, cut) = clip_context(&many);
        assert!(cut);
        assert_eq!(clipped.lines().count(), MAX_CONTEXT_LINES);
        let wide = "y".repeat(MAX_CONTEXT_BYTES + 10);
        assert_eq!(clip_context(&wide), (String::new(), true));
    }

    #[test]
    fn replaced_whole_lines_take_the_indentation_of_the_original() {
        // Lines of an indented Python body, selected from column 0 to the
        // start of the next line; the model answers at column 0.
        let fixed = "x = 1\nif x:\n    print(x)\n";
        assert_eq!(
            fit_indentation(fixed, "    ", "", true),
            "    x = 1\n    if x:\n        print(x)\n"
        );
    }

    #[test]
    fn code_already_at_the_right_indentation_is_left_alone() {
        // The model repeated the lines as they were: a function body and an
        // unindented call after it (common indentation 0 on both sides).
        let fixed = "    total = 0\n    return total / len(values)\n\n\nprint(average([1]))";
        assert_eq!(fit_indentation(fixed, "", "", true), format!("{fixed}\n"));
        assert_eq!(
            common_indentation(["    total = 0", "", "print(x)"]),
            "",
            "blank lines don't count"
        );
        assert_eq!(common_indentation(["        a", "    b"]), "    ");
    }

    #[test]
    fn code_starting_after_the_indent_keeps_its_first_line_bare() {
        // The selection (or cursor) starts after the line's 4 spaces.
        assert_eq!(
            fit_indentation("    a()\n    b()", "    ", "    ", false),
            "a()\n    b()"
        );
        // Blank lines stay blank, not indented.
        assert_eq!(
            fit_indentation("a()\n\nb()", "  ", "  ", false),
            "a()\n\n  b()"
        );
        // After other code on the line, the first line gets no indentation.
        assert_eq!(fit_indentation("    y", "    ", "    x = ", false), "y");
        // Nothing to fit at column 0 of an unindented line.
        assert_eq!(fit_indentation("fn f() {}\n", "", "", false), "fn f() {}");
    }

    #[test]
    fn a_fix_stays_inside_the_errors_block() {
        let file = [
            "def average(values):",
            "    total = 0",
            "    for v in values:",
            "        total += v",
            "    return total / len(value)",
            "",
            "",
            "print(average([1, 2, 3]))",
        ];
        let line = |index: usize| file[index];
        // The error is on line 5 (index 4): three lines up stay in the body,
        // and going down stops at the blank lines before `print`.
        assert_eq!(block_around(file.len(), line, 4, 4, 3), (1, 4));
        // A top-level error may take any line around it.
        assert_eq!(block_around(file.len(), line, 7, 7, 3), (4, 7));
        // The margin limits the range; the `def` line above is outside it.
        assert_eq!(block_around(file.len(), line, 1, 1, 1), (1, 2));
        // Inside a nested block only that block's lines qualify.
        assert_eq!(block_around(file.len(), line, 3, 3, 3), (3, 3));
    }

    #[test]
    fn a_range_ending_at_a_line_start_does_not_include_that_line() {
        let start = Pos { line: 3, byte: 0 };
        assert_eq!(last_line(start, Pos { line: 6, byte: 0 }), 5);
        assert_eq!(last_line(start, Pos { line: 6, byte: 2 }), 6);
        assert_eq!(last_line(start, Pos { line: 3, byte: 0 }), 3);
    }

    #[test]
    fn only_selection_tasks_need_a_selection() {
        assert!(AiTask::Fix.for_selection());
        assert!(!AiTask::FixError.for_selection());
        for task in [
            AiTask::Explain,
            AiTask::Fix,
            AiTask::Tests,
            AiTask::Comments,
            AiTask::ExplainError,
            AiTask::FixError,
        ] {
            let (shown, instruction) = task.wording();
            assert!(!shown.is_empty() && !instruction.is_empty());
        }
    }
}
