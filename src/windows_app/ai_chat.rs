//! The AI Assistant's conversation: connecting to a model server, sending
//! questions (with the selected code), streaming answers into the panel, and
//! putting code from an answer back into the editor.
//! lightline::ai does the HTTP; render/ai_assistant.rs paints the panel.
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
const MAX_CONTEXT_LINES: usize = 400;
const MAX_CONTEXT_BYTES: usize = 24 * 1024;
// Earlier messages sent along for context, newest first, up to this size.
const MAX_HISTORY_BYTES: usize = 48 * 1024;
const MAX_INPUT_BYTES: usize = 16 * 1024;
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
}

/// Something the editor asks the assistant to do with code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AiTask {
    Explain,
    Fix,
    Tests,
    Comments,
    ExplainError,
    FixError,
}

impl AiTask {
    fn for_selection(self) -> bool {
        !matches!(self, AiTask::ExplainError | AiTask::FixError)
    }

    // What the chat shows as the question, and what the model is asked.
    fn wording(self) -> (&'static str, &'static str) {
        match self {
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
    target: Option<AiTarget>,
    /// Why an answer ended early.
    pub(super) error: Option<String>,
    /// Bumped whenever `text` changes, so the answer is laid out again.
    pub(super) revision: u64,
    pub(super) view: MarkdownSnippet,
}

impl ChatEntry {
    fn new(role: Role, text: String, prompt: String) -> Self {
        Self {
            role,
            text,
            prompt,
            context: None,
            target: None,
            error: None,
            revision: 0,
            view: MarkdownSnippet::default(),
        }
    }
}

/// A button on an answer.
#[derive(Clone)]
pub(super) enum AiAction {
    Copy(String),
    /// Put the code at the editor's cursor.
    Insert(String),
    /// Put the code in place of what answer `usize`'s question was about.
    Replace(String, usize),
    /// Smart apply code into active editor with undo support.
    Apply(String, usize),
}

/// Where the panel's buttons were painted, for mouse clicks.
#[derive(Default, Clone)]
pub(super) struct AiHits {
    pub(super) connect: Option<RECT>,
    pub(super) model: Option<RECT>,
    pub(super) model_menu: Option<RECT>,
    pub(super) model_search: Option<RECT>,
    pub(super) model_rows: Vec<(RECT, String)>,
    pub(super) model_refresh: Option<RECT>,
    pub(super) model_turn_off: Option<RECT>,
    pub(super) new_chat: Option<RECT>,
    pub(super) composer: Option<RECT>,
    pub(super) send: Option<RECT>,
    pub(super) actions: Vec<(RECT, AiAction)>,
}

pub(super) struct AiChat {
    pub(super) entries: Vec<ChatEntry>,
    /// The message being typed.
    pub(super) input: String,
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
            focused: false,
            scroll: Cell::new(0),
            follow: Cell::new(true),
            models: Vec::new(),
            model_menu_open: false,
            model_query: String::new(),
            model_menu_first: 0,
            connecting: false,
            problem: None,
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
fn clip_context(text: &str) -> (String, bool) {
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
            && self.ai_ready()
            && !self.welcome
            && !self.quick_open
            && (self.ai.focused || self.ai.model_menu_open)
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
        let endpoint = self.settings.ai_endpoint.clone();
        let tx = self.ai.tx.clone();
        let hwnd_value = hwnd as isize;
        std::thread::spawn(move || {
            let _ = tx.send(AiEvent::Models(ai::list_models(&endpoint)));
            unsafe { PostMessageW(hwnd_value as HWND, AI_EVENT_MESSAGE, 0, 0) };
        });
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Takes in what the worker threads sent (AI_EVENT_MESSAGE).
    pub(super) fn poll_ai(&mut self, hwnd: HWND) {
        let mut changed = false;
        while let Ok(event) = self.ai.rx.try_recv() {
            changed = true;
            match event {
                AiEvent::Models(result) => self.ai_models_listed(result),
                AiEvent::Text(id, text) => {
                    if self.ai_current(id)
                        && let Some(entry) = self.ai.entries.last_mut()
                    {
                        entry.text.push_str(&text);
                        entry.revision += 1;
                    }
                }
                AiEvent::Done(id, result) => {
                    if self.ai_current(id) {
                        self.ai.request = None;
                        if let (Err(error), Some(entry)) = (result, self.ai.entries.last_mut()) {
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
        if self.ai.models.is_empty() {
            self.ai.problem = Some(format!(
                "The server has no chat models yet. In a terminal, run: ollama pull {SUGGESTED_MODEL}"
            ));
            return;
        }
        self.ai.problem = None;
        let current = self.settings.ai_model.clone();
        if current.is_some_and(|model| self.ai.models.contains(&model)) {
            return;
        }
        match ai::pick_model(&self.ai.models) {
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

    /// Sends the typed message, with the selected code.
    pub(super) fn ai_send(&mut self, hwnd: HWND) {
        let question = self.ai.input.trim().to_string();
        if question.is_empty() || self.ai.busy() {
            return;
        }
        let context = self.ai_selection_context();
        self.ai.input.clear();
        self.ai_ask(hwnd, question.clone(), question, context);
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
        if task.for_selection() {
            let Some(context) = self.ai_selection_context() else {
                self.status = "Select some code first".into();
                return;
            };
            self.ai_ask(hwnd, shown.into(), instruction.into(), Some(context));
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
    ) {
        let Some(model) = self.settings.ai_model.clone() else {
            return;
        };
        if self.ai.busy() {
            return;
        }
        let prompt = match &context {
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
        messages.extend(history);
        messages.push(Message::new(Role::User, prompt.clone()));

        let mut question = ChatEntry::new(Role::User, shown, prompt);
        if let Some(context) = context {
            question.context = Some(context.label);
            question.target = context.target;
        }
        self.ai.entries.push(question);
        self.ai.entries.push(ChatEntry::new(
            Role::Assistant,
            String::new(),
            String::new(),
        ));
        self.ai.follow.set(true);

        self.ai.next_request += 1;
        let id = self.ai.next_request;
        let cancel = Arc::new(AtomicBool::new(false));
        self.ai.request = Some((id, Arc::clone(&cancel)));
        let endpoint = self.settings.ai_endpoint.clone();
        let tx = self.ai.tx.clone();
        let hwnd_value = hwnd as isize;
        std::thread::spawn(move || {
            let wake = move || unsafe {
                PostMessageW(hwnd_value as HWND, AI_EVENT_MESSAGE, 0, 0);
            };
            let mut pending = String::new();
            let mut flushed = Instant::now();
            // An empty `text` is stream_chat saying the answer has paused:
            // whatever is held then goes out instead of waiting for more.
            let result = ai::stream_chat(&endpoint, &model, &messages, &cancel, |text| {
                pending.push_str(text);
                if !pending.is_empty() && flushed.elapsed() >= ANSWER_FLUSH {
                    let _ = tx.send(AiEvent::Text(id, std::mem::take(&mut pending)));
                    wake();
                    flushed = Instant::now();
                }
            });
            if !pending.is_empty() {
                let _ = tx.send(AiEvent::Text(id, pending));
            }
            let _ = tx.send(AiEvent::Done(id, result));
            wake();
        });
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Stops the answer being received; what arrived so far stays.
    pub(super) fn ai_stop(&mut self, hwnd: HWND) {
        if let Some((_, cancel)) = self.ai.request.take() {
            cancel.store(true, Ordering::Relaxed);
            if let Some(entry) = self.ai.entries.last_mut() {
                entry.error = Some("Stopped".into());
            }
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn ai_new_chat(&mut self, hwnd: HWND) {
        self.ai_stop(hwnd);
        self.ai.entries.clear();
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
            unsafe { SetFocus(hwnd) };
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
                unsafe { SetFocus(hwnd) };
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
        if hits.new_chat.as_ref().is_some_and(inside) {
            self.ai_new_chat(hwnd);
        } else if let Some(chip) = hits.model.filter(inside) {
            self.ai_model_menu(hwnd, chip.left, chip.bottom);
        } else if hits.connect.as_ref().is_some_and(inside) {
            self.ai_connect(hwnd);
        } else if hits.send.as_ref().is_some_and(inside) {
            if self.ai.busy() {
                self.ai_stop(hwnd);
            } else {
                self.ai_send(hwnd);
            }
            self.focus_ai_input();
        } else if hits.composer.as_ref().is_some_and(inside) {
            self.focus_ai_input();
        } else if let Some((_, action)) = hits.actions.iter().find(|(rect, _)| inside(rect)) {
            match action {
                AiAction::Copy(text) => {
                    self.status = match clipboard::copy(hwnd, text) {
                        Ok(()) => "Copied".into(),
                        Err(error) => format!("Couldn't copy: {error}"),
                    };
                }
                AiAction::Insert(code) => {
                    if self.ai_can_insert() {
                        let cursor = self.view().cursor;
                        self.ai_apply_code(hwnd, code, self.active, cursor, cursor);
                    }
                }
                AiAction::Replace(code, answer) => {
                    if let Some((tab, start, end)) = self.ai_replace_target(*answer) {
                        self.ai_apply_code(hwnd, code, tab, start, end);
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
            k if k == VK_BACK as u32 => {
                if ctrl {
                    let kept = self.ai.input.trim_end().rfind(char::is_whitespace);
                    self.ai.input.truncate(kept.map_or(0, |index| index + 1));
                } else {
                    self.ai.input.pop();
                }
                unsafe { InvalidateRect(hwnd, null(), 0) };
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
        if self.ai.input.len() + text.len() <= MAX_INPUT_BYTES {
            self.ai.input.push_str(text);
        }
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
