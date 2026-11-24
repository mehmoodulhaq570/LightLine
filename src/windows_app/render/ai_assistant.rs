use super::super::ai_chat::{
    AiAction, AiTask, ChatEntry, ContextOffer, ProposalButton, SUGGESTED_MODEL, model_menu_models,
};
use super::super::ai_edits::ProposalStatus;
use super::super::ai_project::{Permission, StepKind};
use super::super::markdown_view::{MarkdownSnippet, SNIPPET_CODE_HEADER};
use super::super::*;
use super::blend;
use lightline::ai::{self, Role};

// The AI Assistant panel. Until a model is chosen it explains the
// assistant and offers to connect to Ollama; then it shows the conversation
// (questions as bubbles, answers as Markdown) above the message box.
//
// Painting only lays out text the panel already holds, and each answer's
// layout is kept until its text or the panel's width changes, so an open
// panel costs nothing while it's idle.

// What the panel promises about AI, whatever the model.
const AI_PROMISES: [&str; 3] = [
    "Nothing runs in the background",
    "Nothing is sent until you ask",
    "Editing never depends on AI",
];

// Most lines the message box grows to before its text scrolls.
const COMPOSER_LINES: usize = 8;

// An attached file's chip: its icon (from the path), what it says, and
// whether it's amber.
struct ChipSpec {
    path: PathBuf,
    label: String,
    warn: bool,
    // The open file, offered: it goes unless taken off, and looks quieter
    // than a file attached on purpose.
    offered: bool,
}

// The permission card, laid out (`ai_permission_layout`).
struct PermissionLayout {
    title: String,
    lines: Vec<String>,
    buttons: Vec<(RECT, &'static str, Permission, bool)>,
    height: i32,
}

fn chip_specs(chips: &[super::super::ai_attach::Chip]) -> Vec<ChipSpec> {
    chips
        .iter()
        .map(|chip| ChipSpec {
            path: chip.path.clone(),
            label: chip.label.clone(),
            warn: chip.warn,
            offered: false,
        })
        .collect()
}

// A piece of an answer, in the order it arrived: text (a byte range of the
// visible answer) or a run of steps (a range of `ChatEntry::steps`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnswerPart {
    Text(usize, usize),
    Steps(usize, usize),
}

// Splits `answer` at its steps. `at` is where each step came in the whole
// text, which starts `skip` bytes before `answer` (a hidden <think> block).
// Text that's only blank between two steps joins them into one run.
fn answer_parts(answer: &str, skip: usize, at: &[usize]) -> Vec<AnswerPart> {
    let mut parts = Vec::new();
    let mut position = 0;
    for (index, &at) in at.iter().enumerate() {
        let at = at.saturating_sub(skip).clamp(position, answer.len());
        if at > position && answer.is_char_boundary(at) {
            if !answer[position..at].trim().is_empty() {
                parts.push(AnswerPart::Text(position, at));
            }
            position = at;
        }
        match parts.last_mut() {
            Some(AnswerPart::Steps(_, end)) => *end = index + 1,
            _ => parts.push(AnswerPart::Steps(index, index + 1)),
        }
    }
    if position < answer.len() && !answer[position..].trim().is_empty() {
        parts.push(AnswerPart::Text(position, answer.len()));
    }
    parts
}

// "3 steps · 14 s" under a finished answer.
fn answer_meta(steps: usize, took: Option<Duration>) -> String {
    let mut parts = Vec::new();
    match steps {
        0 => {}
        1 => parts.push("1 step".to_string()),
        count => parts.push(format!("{count} steps")),
    }
    if let Some(took) = took {
        let seconds = took.as_secs();
        parts.push(if seconds < 60 {
            format!("{} s", seconds.max(1))
        } else {
            format!("{} min {} s", seconds / 60, seconds % 60)
        });
    }
    parts.join(" \u{00b7} ")
}

// A step's icon.
fn step_glyph(kind: StepKind, error: bool) -> DebugGlyph {
    if error {
        return DebugGlyph::Warning;
    }
    match kind {
        StepKind::Search => DebugGlyph::Search,
        StepKind::Read => DebugGlyph::OpenFile,
        StepKind::List => DebugGlyph::Folder,
        StepKind::Problems => DebugGlyph::Warning,
        StepKind::Change => DebugGlyph::Notes,
        StepKind::Other => DebugGlyph::More,
    }
}

impl App {
    pub(in crate::windows_app) fn paint_ai_assistant(&self, hdc: HDC, rect: RECT) {
        let s = |value: i32| self.scale(value);
        self.panel_card(
            hdc,
            rect,
            s(CARD_RADIUS),
            self.theme.card_edge,
            self.theme.sidebar_bg,
        );
        *self.ai.hits.borrow_mut() = Default::default();
        let header_bottom = rect.top + s(AI_HEADER);
        self.paint_ai_header(hdc, rect, header_bottom);
        if self.ai_dragging {
            // The edge being dragged.
            Self::fill(
                hdc,
                RECT {
                    left: rect.left,
                    top: rect.top + s(CARD_RADIUS),
                    right: rect.left + s(2),
                    bottom: rect.bottom - s(CARD_RADIUS),
                },
                self.theme.violet,
            );
        }

        let content = RECT {
            left: rect.left + s(12),
            top: header_bottom + s(14),
            right: rect.right - s(12),
            bottom: rect.bottom - s(12),
        };
        unsafe { SelectObject(hdc, self.ui_font) };
        let line_height = self.text_height(hdc) + s(4);
        if !self.ai_ready() {
            let composer = RECT {
                top: content.bottom - self.ai_composer_height(0, 1, line_height),
                ..content
            };
            self.paint_ai_setup_state(
                hdc,
                RECT {
                    top: content.top + s(10),
                    bottom: composer.top - s(12),
                    left: content.left + s(4),
                    right: content.right - s(4),
                },
            );
            self.paint_ai_composer(hdc, composer, &[], &[], false);
            return;
        }

        // The message box: chips for what goes with the question, the
        // text, and a row with @, the model and Send.
        let inner_width = (content.right - content.left - s(24)).max(s(40));
        let chips = self.ai_composer_chips();
        let (spans, chips_height) = if chips.is_empty() {
            (Vec::new(), 0)
        } else {
            let (spans, height) = self.ai_chip_spans(hdc, &chips, inner_width, true);
            (spans, height + s(6))
        };
        unsafe { SelectObject(hdc, self.ui_font) };
        let lines = self.wrap_ranges(hdc, &self.ai.input, inner_width);
        let shown = lines.len().clamp(1, COMPOSER_LINES);
        let composer = RECT {
            top: content.bottom - self.ai_composer_height(chips_height, shown, line_height),
            ..content
        };
        let conversation_bottom = composer.top - s(10);
        self.paint_ai_conversation(
            hdc,
            RECT {
                bottom: conversation_bottom,
                left: content.left + s(4),
                right: content.right - s(4),
                ..content
            },
        );
        self.paint_ai_composer(hdc, composer, &chips, &spans, true);
        if self.ai_mention_query().is_some() {
            self.paint_ai_mentions(hdc, composer, conversation_bottom, header_bottom);
        }
        if self.ai.model_menu_open {
            self.paint_ai_model_picker(hdc, rect, header_bottom, composer.top);
        }
        if self.ai.history_open {
            self.paint_ai_history(hdc, rect, header_bottom);
        }
    }

    // The list of earlier chats under the header: a filter line, then the
    // chats newest first in groups (TODAY, YESTERDAY, EARLIER), each with
    // its time; the chosen one has a ✕ that deletes it.
    fn paint_ai_history(&self, hdc: HDC, panel: RECT, header_bottom: i32) {
        use super::super::ai_history::{HISTORY_ROWS, chat_when};
        let s = |value: i32| self.scale(value);
        let matches = self.ai_history_matches();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        let selected = self
            .ai
            .history_selected
            .min(matches.len().saturating_sub(1));
        // The rows shown keep the chosen one in view.
        let first = (selected + 1).saturating_sub(HISTORY_ROWS);
        let shown: Vec<(usize, usize)> = matches
            .iter()
            .copied()
            .enumerate()
            .skip(first)
            .take(HISTORY_ROWS)
            .collect();
        let rows: Vec<(usize, usize, &'static str, String)> = shown
            .iter()
            .map(|&(row, index)| {
                let (group, when) = chat_when(self.ai.history[index].updated, now);
                (row, index, group, when)
            })
            .collect();
        let row_height = s(28);
        let group_height = s(22);
        let mut groups = 0;
        let mut previous = None;
        for (_, _, group, _) in &rows {
            if previous != Some(*group) {
                groups += 1;
                previous = Some(*group);
            }
        }
        let body = if rows.is_empty() {
            s(40)
        } else {
            rows.len() as i32 * row_height + groups * group_height + s(6)
        };
        let card = RECT {
            left: panel.left + s(8),
            top: header_bottom + s(4),
            right: panel.right - s(8),
            bottom: (header_bottom + s(4) + s(38) + body).min(panel.bottom - s(12)),
        };
        Self::rounded_fill(
            hdc,
            RECT {
                left: card.left + s(3),
                top: card.top + s(4),
                right: card.right + s(3),
                bottom: card.bottom + s(4),
            },
            s(8),
            blend(self.theme.sidebar_bg, ui(0, 0, 0), 0.5),
        );
        self.panel_card(
            hdc,
            card,
            s(8),
            blend(self.theme.sidebar_bg, self.theme.blue, 0.4),
            self.theme.editor_bg,
        );
        let mut hits = self.ai.hits.borrow_mut();
        hits.history_card = Some(card);
        drop(hits);

        // The filter.
        unsafe { SelectObject(hdc, self.ui_font) };
        let search = RECT {
            left: card.left + s(10),
            top: card.top + s(4),
            right: card.right - s(10),
            bottom: card.top + s(34),
        };
        let mid = (search.top + search.bottom) / 2;
        self.glyph_in(
            hdc,
            DebugGlyph::Search,
            self.theme.muted,
            RECT {
                right: search.left + s(18),
                ..search
            },
            s(14),
        );
        let (text, color) = if self.ai.history_query.is_empty() {
            ("Search chats".to_string(), self.theme.muted)
        } else {
            (format!("{}|", self.ai.history_query), self.theme.text)
        };
        self.label_mid(hdc, &text, search.left + s(26), mid, color, search);
        Self::fill(
            hdc,
            RECT {
                left: card.left + s(1),
                top: search.bottom,
                right: card.right - s(1),
                bottom: search.bottom + s(1).max(1),
            },
            self.theme.edge,
        );

        let inner = RECT {
            left: card.left + s(1),
            top: search.bottom + s(4),
            right: card.right - s(1),
            bottom: card.bottom - s(2),
        };
        if rows.is_empty() {
            let note = if !self.settings.ai_keep_chats {
                "Keeping chats is off (Keep earlier chats in Settings)"
            } else if self.ai.history_loading {
                "Reading\u{2026}"
            } else if self.ai.history.is_empty() {
                "No earlier chats in this folder yet"
            } else {
                "No chat has that in its title"
            };
            unsafe { SelectObject(hdc, self.small_font) };
            self.label_ellipsis(
                hdc,
                note,
                inner.left + s(12),
                inner.top + s(8),
                self.theme.muted,
                inner,
            );
            unsafe { SelectObject(hdc, self.ui_font) };
            return;
        }
        let mut y = inner.top;
        let mut previous = None;
        let mut row_hits = Vec::new();
        let mut delete = None;
        for (row, index, group, when) in rows {
            if previous != Some(group) {
                if y + group_height > inner.bottom {
                    break;
                }
                unsafe { SelectObject(hdc, self.small_font) };
                self.label_mid(
                    hdc,
                    group,
                    inner.left + s(12),
                    y + group_height / 2,
                    self.theme.muted,
                    inner,
                );
                unsafe { SelectObject(hdc, self.ui_font) };
                y += group_height;
                previous = Some(group);
            }
            if y + row_height > inner.bottom {
                break;
            }
            let rect = RECT {
                left: inner.left,
                top: y,
                right: inner.right,
                bottom: y + row_height,
            };
            let chosen = row == selected;
            if chosen {
                Self::fill(
                    hdc,
                    rect,
                    blend(self.theme.editor_bg, self.theme.blue, 0.22),
                );
            }
            let chat = &self.ai.history[index];
            unsafe { SelectObject(hdc, self.small_font) };
            let when_width = self.text_width(hdc, &when);
            let mut right = rect.right - s(12) - when_width;
            self.label_mid(
                hdc,
                &when,
                right,
                (rect.top + rect.bottom) / 2,
                self.theme.muted,
                rect,
            );
            unsafe { SelectObject(hdc, self.ui_font) };
            if chosen {
                let close = RECT {
                    left: right - s(28),
                    top: rect.top + s(2),
                    right: right - s(4),
                    bottom: rect.bottom - s(2),
                };
                self.glyph_in(hdc, DebugGlyph::Close, self.theme.muted, close, s(12));
                delete = Some((close, row));
                right = close.left;
            }
            let current = self.ai.chat_id.as_deref() == Some(chat.id.as_str());
            self.label_ellipsis(
                hdc,
                &chat.title,
                rect.left + s(12),
                (rect.top + rect.bottom) / 2 - self.text_height(hdc) / 2,
                if current {
                    blend(self.theme.blue, self.theme.text, 0.4)
                } else {
                    self.theme.text
                },
                RECT {
                    right: right - s(6),
                    ..rect
                },
            );
            row_hits.push((rect, row));
            y += row_height;
        }
        let mut hits = self.ai.hits.borrow_mut();
        hits.history_rows = row_hits;
        hits.history_delete = delete;
    }

    // The message box's height: its chips, `shown` lines of text and the
    // row of buttons under them.
    fn ai_composer_height(&self, chips_height: i32, shown: usize, line_height: i32) -> i32 {
        let s = |value: i32| self.scale(value);
        s(10) + chips_height + shown as i32 * line_height + s(6) + s(28) + s(6)
    }

    // The chips above the text: attached files, then the selection or the
    // open file (the last one, when there is one).
    fn ai_composer_chips(&self) -> Vec<ChipSpec> {
        let mut chips: Vec<ChipSpec> = self
            .ai
            .attachments
            .iter()
            .map(|attached| ChipSpec {
                path: attached.path.clone(),
                label: if attached.secret {
                    format!("{} \u{00b7} looks like secrets", attached.name)
                } else {
                    attached.name.clone()
                },
                warn: attached.secret,
                offered: false,
            })
            .collect();
        match self.ai_context_offer() {
            Some(ContextOffer::Selection(label)) => chips.push(ChipSpec {
                path: self
                    .doc()
                    .path
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("untitled.txt")),
                label: label.replacen(", ", " \u{00b7} ", 1),
                warn: false,
                offered: false,
            }),
            Some(ContextOffer::OpenFile(path)) => chips.push(ChipSpec {
                label: self.ai_file_name(&path),
                path,
                warn: false,
                offered: true,
            }),
            None => {}
        }
        chips
    }

    // A 35 px header like the side panels': CHAT and the chat's first
    // question, New Chat and Close.
    fn paint_ai_header(&self, hdc: HDC, rect: RECT, header_bottom: i32) {
        let s = |value: i32| self.scale(value);
        unsafe { SelectObject(hdc, self.ui_font) };
        let mid = (rect.top + header_bottom) / 2;
        let title = "CHAT";
        self.label_mid(hdc, title, rect.left + s(16), mid, self.theme.muted, rect);
        let title_right = rect.left + s(16) + self.text_width(hdc, title);
        // Close; its hit target is in ai_click.
        let close = RECT {
            left: rect.right - s(32),
            top: mid - s(12),
            right: rect.right - s(8),
            bottom: mid + s(12),
        };
        self.glyph_in(hdc, DebugGlyph::Close, self.theme.muted, close, s(16));
        let mut right = close.left;
        if self.settings.ai_model.is_some() {
            let new_chat = RECT {
                left: close.left - s(26),
                right: close.left - s(2),
                ..close
            };
            self.glyph_in(hdc, DebugGlyph::Plus, self.theme.muted, new_chat, s(16));
            self.ai.hits.borrow_mut().new_chat = Some(new_chat);
            // Earlier chats.
            let history = RECT {
                left: new_chat.left - s(26),
                right: new_chat.left - s(2),
                ..close
            };
            if self.ai.history_open {
                Self::rounded_fill(
                    hdc,
                    history,
                    s(4),
                    blend(self.theme.sidebar_bg, self.theme.text, 0.1),
                );
            }
            self.glyph_in(
                hdc,
                DebugGlyph::History,
                if self.ai.history_open {
                    self.theme.text
                } else {
                    self.theme.muted
                },
                history,
                s(16),
            );
            self.ai.hits.borrow_mut().history_button = Some(history);
            right = history.left;
        }
        // The chat's first question names it.
        if let Some(first) = self
            .ai
            .entries
            .iter()
            .find(|entry| entry.role == Role::User)
        {
            let name = first.text.lines().next().unwrap_or_default();
            unsafe { SelectObject(hdc, self.small_font) };
            self.label_ellipsis(
                hdc,
                &format!("\u{00b7}  {name}"),
                title_right + s(8),
                mid - self.text_height(hdc) / 2,
                self.theme.muted,
                RECT {
                    right: right - s(8),
                    ..rect
                },
            );
            unsafe { SelectObject(hdc, self.ui_font) };
        }
        Self::fill(
            hdc,
            RECT {
                left: rect.left + s(1),
                top: header_bottom,
                right: rect.right - s(1),
                bottom: header_bottom + s(1).max(1),
            },
            self.theme.edge,
        );
    }

    /// The themed model picker. It deliberately stays within the assistant
    /// card and above the composer, unlike the old native popup menu.
    // It opens above the message box (`above`), where its chip is.
    fn paint_ai_model_picker(&self, hdc: HDC, panel: RECT, header_bottom: i32, above: i32) {
        let s = |value: i32| self.scale(value);
        let menu_bottom = above - s(8);
        let menu = RECT {
            left: panel.left + s(18),
            top: (menu_bottom - s(451)).max(header_bottom + s(8)),
            right: panel.right - s(18),
            bottom: menu_bottom,
        };
        if menu.right - menu.left < s(210) || menu.bottom - menu.top < s(240) {
            return;
        }

        // A restrained shadow gives the popup elevation without the bright,
        // disconnected appearance of a platform context menu.
        Self::rounded_fill(
            hdc,
            RECT {
                left: menu.left + s(4),
                top: menu.top + s(5),
                right: menu.right + s(4),
                bottom: menu.bottom + s(5),
            },
            s(10),
            ui(3, 9, 19),
        );
        self.panel_card(hdc, menu, s(10), ui(69, 75, 169), ui(10, 25, 47));

        let search = RECT {
            left: menu.left + s(12),
            top: menu.top + s(12),
            right: menu.right - s(12),
            bottom: menu.top + s(54),
        };
        self.panel_card(hdc, search, s(8), ui(40, 75, 126), ui(13, 31, 57));
        self.rail_icon(
            hdc,
            1,
            search.left + s(12),
            search.top + s(12),
            ui(145, 174, 218),
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        let query_empty = self.ai.model_query.is_empty();
        let query = if query_empty {
            "Search models..."
        } else {
            &self.ai.model_query
        };
        let query_x = search.left + s(42);
        self.label_ellipsis(
            hdc,
            query,
            query_x,
            search.top + s(11),
            if query_empty {
                self.theme.muted
            } else {
                self.theme.text
            },
            RECT {
                left: query_x,
                right: search.right - s(10),
                ..search
            },
        );
        if !query_empty {
            let caret_x = (query_x + self.text_width(hdc, query)).min(search.right - s(10));
            Self::fill(
                hdc,
                RECT {
                    left: caret_x,
                    top: search.top + s(10),
                    right: caret_x + s(1).max(1),
                    bottom: search.bottom - s(10),
                },
                ui(116, 190, 250),
            );
        }

        let actions_top = menu.bottom - s(113);
        Self::fill(
            hdc,
            RECT {
                left: menu.left + s(1),
                top: actions_top,
                right: menu.right - s(1),
                bottom: actions_top + s(1).max(1),
            },
            ui(31, 59, 99),
        );
        let refresh = RECT {
            left: menu.left + s(8),
            top: actions_top + s(7),
            right: menu.right - s(8),
            bottom: actions_top + s(38),
        };
        let provider = RECT {
            top: refresh.bottom,
            bottom: refresh.bottom + s(31),
            ..refresh
        };
        let turn_off = RECT {
            top: provider.bottom,
            bottom: menu.bottom - s(7),
            ..refresh
        };
        Self::label(
            hdc,
            "\u{2699}",
            provider.left + s(10),
            provider.top + s(5),
            ui(145, 174, 218),
            provider,
        );
        self.label_ellipsis(
            hdc,
            &format!(
                "Change provider\u{2026} ({})",
                ai::host(&self.settings.ai_endpoint)
            ),
            provider.left + s(38),
            provider.top + s(5),
            self.theme.text,
            provider,
        );
        Self::label(
            hdc,
            "\u{21bb}",
            refresh.left + s(10),
            refresh.top + s(5),
            ui(145, 174, 218),
            refresh,
        );
        Self::label(
            hdc,
            if self.ai.connecting {
                "Refreshing model list..."
            } else {
                "Refresh model list"
            },
            refresh.left + s(38),
            refresh.top + s(5),
            self.theme.text,
            refresh,
        );
        Self::label(
            hdc,
            "\u{23fb}",
            turn_off.left + s(10),
            turn_off.top + s(5),
            ui(145, 174, 218),
            turn_off,
        );
        Self::label(
            hdc,
            "Turn off AI Assistant",
            turn_off.left + s(38),
            turn_off.top + s(5),
            self.theme.text,
            turn_off,
        );

        let models = model_menu_models(
            &self.ai.models,
            self.settings.ai_model.as_deref(),
            &self.ai.model_query,
        );
        let first = self.ai.model_menu_first.min(models.len().saturating_sub(1));
        let list = RECT {
            left: menu.left + s(8),
            top: search.bottom + s(9),
            right: menu.right - s(8),
            bottom: actions_top - s(5),
        };
        let row_height = s(36);
        let section_height = s(22);
        let current = self.settings.ai_model.as_deref();
        let remote = !ai::is_local(&self.settings.ai_endpoint);
        let mut y = list.top;
        let mut previous_cloud = None;
        let mut visible_rows = 0usize;
        let mut row_hits = Vec::new();
        for model in models.iter().skip(first) {
            if visible_rows >= 6 {
                break;
            }
            let cloud = ai::is_cloud_model(model);
            if previous_cloud != Some(cloud) {
                if y + section_height + row_height > list.bottom {
                    break;
                }
                // From a cloud service, every model is that service's.
                let title = if remote {
                    if ai::is_anthropic(&self.settings.ai_endpoint) {
                        "ANTHROPIC"
                    } else {
                        "MODELS"
                    }
                } else if cloud {
                    "CLOUD"
                } else {
                    "LOCAL"
                };
                Self::label(
                    hdc,
                    title,
                    list.left + s(8),
                    y + s(2),
                    ui(145, 174, 218),
                    list,
                );
                let title_width = self.text_width(hdc, title);
                Self::fill(
                    hdc,
                    RECT {
                        left: list.left + s(18) + title_width,
                        top: y + s(10),
                        right: list.right - s(12),
                        bottom: y + s(11),
                    },
                    ui(34, 64, 105),
                );
                y += section_height;
                previous_cloud = Some(cloud);
            }
            if y + row_height > list.bottom {
                break;
            }

            let row = RECT {
                left: list.left,
                top: y,
                right: list.right - s(6),
                bottom: y + row_height,
            };
            let selected = current == Some(model.as_str());
            if selected {
                self.panel_card(hdc, row, s(6), ui(73, 83, 170), ui(29, 37, 91));
                Self::label(
                    hdc,
                    "\u{2713}",
                    row.left + s(10),
                    row.top + s(7),
                    ui(71, 210, 250),
                    row,
                );
            }

            let icon = RECT {
                left: row.left + s(31),
                top: row.top + s(6),
                right: row.left + s(55),
                bottom: row.top + s(30),
            };
            self.paint_ai_model_icon(hdc, icon, model, cloud);

            let meta = if selected {
                "Selected"
            } else if remote {
                ""
            } else if cloud {
                "Cloud"
            } else {
                "Local"
            };
            let meta_width = self.text_width(hdc, meta);
            if cloud && !selected {
                let badge = RECT {
                    left: row.right - meta_width - s(22),
                    top: row.top + s(7),
                    right: row.right - s(7),
                    bottom: row.bottom - s(7),
                };
                Self::rounded_fill(hdc, badge, s(6), ui(24, 53, 92));
                Self::label(
                    hdc,
                    meta,
                    badge.left + s(8),
                    badge.top + s(3),
                    ui(157, 190, 232),
                    badge,
                );
            } else {
                Self::label(
                    hdc,
                    meta,
                    row.right - meta_width - s(9),
                    row.top + s(8),
                    if selected {
                        ui(71, 210, 250)
                    } else {
                        self.theme.muted
                    },
                    row,
                );
            }
            self.label_ellipsis(
                hdc,
                &ai::model_label(model),
                row.left + s(64),
                row.top + s(8),
                self.theme.text,
                RECT {
                    left: row.left + s(64),
                    right: row.right - meta_width - s(32),
                    ..row
                },
            );
            row_hits.push((row, model.clone()));
            y += row_height;
            visible_rows += 1;
        }

        if models.is_empty() {
            Self::label(
                hdc,
                "No matching models",
                list.left + s(10),
                list.top + s(12),
                self.theme.muted,
                list,
            );
        } else if models.len() > visible_rows {
            let track = RECT {
                left: list.right - s(3),
                top: list.top + s(4),
                right: list.right,
                bottom: list.bottom - s(4),
            };
            Self::rounded_fill(hdc, track, s(2), ui(20, 45, 78));
            let track_height = track.bottom - track.top;
            let thumb_height =
                ((track_height as usize * visible_rows) / models.len()).max(s(24) as usize) as i32;
            let max_first = models.len().saturating_sub(visible_rows).max(1);
            let offset = ((track_height - thumb_height) as usize * first / max_first) as i32;
            Self::rounded_fill(
                hdc,
                RECT {
                    top: track.top + offset,
                    bottom: track.top + offset + thumb_height,
                    ..track
                },
                s(2),
                ui(70, 112, 170),
            );
        }

        let mut hits = self.ai.hits.borrow_mut();
        hits.model_menu = Some(menu);
        hits.model_search = Some(search);
        hits.model_rows = row_hits;
        hits.model_refresh = Some(refresh);
        hits.model_turn_off = Some(turn_off);
        hits.model_provider = Some(provider);
    }

    /// Standalone provider marks matching the model-picker mockup: the logo
    /// itself carries the identity, without a colored tile behind every icon.
    /// They are vector-drawn so they stay sharp at every Windows DPI.
    fn paint_ai_model_icon(&self, hdc: HDC, rect: RECT, model: &str, cloud: bool) {
        let s = |value: i32| self.scale(value);
        let name = model.to_ascii_lowercase();
        let cx = (rect.left + rect.right) / 2;
        let cy = (rect.top + rect.bottom) / 2;
        unsafe {
            if name.starts_with("qwen") {
                // Six faceted ribbons form Qwen's purple hexagonal knot.
                let outer = s(10) as f32;
                let inner = s(5) as f32;
                let colors = [
                    ui(143, 92, 246),
                    ui(126, 87, 238),
                    ui(108, 92, 231),
                    ui(126, 87, 238),
                    ui(154, 101, 255),
                    ui(139, 92, 246),
                ];
                let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
                let point = |angle: f32, radius: f32| POINT {
                    x: cx + (angle.cos() * radius).round() as i32,
                    y: cy + (angle.sin() * radius).round() as i32,
                };
                for (index, color) in colors.into_iter().enumerate() {
                    let first =
                        -std::f32::consts::FRAC_PI_2 + index as f32 * std::f32::consts::PI / 3.0;
                    let second = first + std::f32::consts::PI / 3.0;
                    let points = [
                        point(first, outer),
                        point(second, outer),
                        point(second, inner),
                        point(first, inner),
                    ];
                    let brush = CreateSolidBrush(color);
                    let old_brush = SelectObject(hdc, brush);
                    Polygon(hdc, points.as_ptr(), points.len() as i32);
                    SelectObject(hdc, old_brush);
                    DeleteObject(brush);
                }
                SelectObject(hdc, old_pen);
                return;
            }

            if name.starts_with("gemma") {
                // Compact multicolor Google G used by the Gemma family.
                let segments = [
                    (ui(66, 133, 244), (-7, -7, 7, -7)),
                    (ui(234, 67, 53), (-7, -7, -9, 1)),
                    (ui(251, 188, 5), (-9, 1, -4, 8)),
                    (ui(52, 168, 83), (-4, 8, 7, 5)),
                    (ui(66, 133, 244), (7, 5, 7, 0)),
                ];
                for (color, (x1, y1, x2, y2)) in segments {
                    let pen = CreatePen(PS_SOLID, s(4).max(2), color);
                    let old_pen = SelectObject(hdc, pen);
                    MoveToEx(hdc, cx + s(x1), cy + s(y1), null_mut());
                    LineTo(hdc, cx + s(x2), cy + s(y2));
                    SelectObject(hdc, old_pen);
                    DeleteObject(pen);
                }
                let blue = CreatePen(PS_SOLID, s(3).max(2), ui(66, 133, 244));
                let old_pen = SelectObject(hdc, blue);
                MoveToEx(hdc, cx, cy, null_mut());
                LineTo(hdc, cx + s(9), cy);
                SelectObject(hdc, old_pen);
                DeleteObject(blue);
                return;
            }

            let mark = if name.starts_with("nemotron") {
                ui(118, 196, 38)
            } else if name.starts_with("llama") {
                ui(66, 133, 244)
            } else if name.starts_with("gpt") {
                self.theme.text
            } else if cloud {
                self.theme.sky
            } else {
                self.theme.violet
            };
            let pen = CreatePen(PS_SOLID, s(2).max(2), mark);
            if pen.is_null() {
                return;
            }
            let old_pen = SelectObject(hdc, pen);
            let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
            if name.starts_with("gpt") {
                // White six-loop OpenAI knot, free of a surrounding tile.
                let r = s(3);
                for (dx, dy) in [
                    (0, -s(5)),
                    (s(5), -s(2)),
                    (s(5), s(3)),
                    (0, s(5)),
                    (-s(5), s(3)),
                    (-s(5), -s(2)),
                ] {
                    Ellipse(hdc, cx + dx - r, cy + dy - r, cx + dx + r, cy + dy + r);
                }
            } else if name.starts_with("nemotron") {
                // NVIDIA eye and iris, plus the squared terminal stroke seen
                // in the approved mockup.
                Arc(
                    hdc,
                    rect.left,
                    cy - s(8),
                    rect.right - s(2),
                    cy + s(8),
                    rect.right - s(2),
                    cy,
                    rect.left,
                    cy,
                );
                Ellipse(hdc, cx - s(5), cy - s(5), cx + s(5), cy + s(5));
                let pupil = CreateSolidBrush(mark);
                let prior = SelectObject(hdc, pupil);
                Ellipse(hdc, cx - s(2), cy - s(2), cx + s(2), cy + s(2));
                SelectObject(hdc, prior);
                DeleteObject(pupil);
                MoveToEx(hdc, rect.right - s(5), cy - s(6), null_mut());
                LineTo(hdc, rect.right - s(1), cy - s(6));
                LineTo(hdc, rect.right - s(1), cy + s(6));
                LineTo(hdc, rect.right - s(5), cy + s(6));
            } else if name.starts_with("llama") {
                // Meta/Llama infinity mark, also standalone.
                Ellipse(hdc, cx - s(10), cy - s(6), cx, cy + s(6));
                Ellipse(hdc, cx, cy - s(6), cx + s(10), cy + s(6));
                MoveToEx(hdc, cx - s(5), cy - s(4), null_mut());
                LineTo(hdc, cx + s(5), cy + s(4));
                MoveToEx(hdc, cx - s(5), cy + s(4), null_mut());
                LineTo(hdc, cx + s(5), cy - s(4));
            } else if cloud {
                // Generic cloud provider mark.
                Ellipse(hdc, cx - s(7), cy - s(2), cx + s(7), cy + s(6));
                Ellipse(hdc, cx - s(5), cy - s(7), cx + s(2), cy + s(3));
                Ellipse(hdc, cx, cy - s(5), cx + s(6), cy + s(3));
            } else {
                // Unknown local model: the same compact AI sparkle language
                // as the rest of the assistant, with no background tile.
                SelectObject(hdc, old_brush);
                SelectObject(hdc, old_pen);
                DeleteObject(pen);
                self.sparkle_glyph(hdc, rect.left + s(3), rect.top + s(3), s(18), mark);
                return;
            }
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old_pen);
            DeleteObject(pen);
        }
    }

    // The sparkle tile and title that open the setup and empty-chat views;
    // returns the y below the title.
    fn paint_ai_intro(&self, hdc: HDC, area: RECT, title: &str) -> i32 {
        let s = |value: i32| self.scale(value);
        let tile = RECT {
            left: area.left,
            top: area.top,
            right: area.left + s(44),
            bottom: area.top + s(44),
        };
        Self::rounded_fill(hdc, tile, s(10), ui(38, 28, 84));
        self.sparkle_glyph(
            hdc,
            tile.left + s(11),
            tile.top + s(11),
            s(22),
            ui(180, 150, 255),
        );
        unsafe { SelectObject(hdc, self.brand_font) };
        let y = tile.bottom + s(16);
        Self::label(hdc, title, area.left, y, self.theme.text, area);
        let below = y + self.text_height(hdc) + s(10);
        unsafe { SelectObject(hdc, self.ui_font) };
        below
    }

    // The promises, one per line with a check mark; returns the y below.
    fn paint_ai_promises(&self, hdc: HDC, area: RECT, mut y: i32, line_height: i32) -> i32 {
        let s = |value: i32| self.scale(value);
        for promise in AI_PROMISES {
            Self::label(hdc, "\u{2713}", area.left, y, self.theme.green, area);
            self.label_ellipsis(hdc, promise, area.left + s(22), y, self.theme.muted, area);
            y += line_height + s(2);
        }
        y
    }

    // No model chosen yet: what the assistant does, and a button that asks
    // the server (Ollama by default) which models it has.
    fn paint_ai_setup_state(&self, hdc: HDC, area: RECT) {
        let s = |value: i32| self.scale(value);
        let mut y = self.paint_ai_intro(hdc, area, "Connect a model");
        let line_height = self.text_height(hdc) + s(4);
        let endpoint = &self.settings.ai_endpoint;
        let remote = !ai::is_local(endpoint);
        let host = ai::host(endpoint);
        let about = if remote {
            format!(
                "The assistant explains, fixes and writes code with a model from {host}. \
                 Your questions and the code you include are sent there."
            )
        } else {
            "The assistant explains, fixes and writes code with a model that runs \
             on your PC through Ollama: free, private, and it works offline."
                .to_string()
        };
        y = self.paint_wrapped(hdc, &about, area, y, line_height, self.theme.text);
        y = self.paint_ai_promises(hdc, area, y + s(12), line_height);

        y += s(12);
        if let Some(problem) = &self.ai.problem {
            y = self.paint_wrapped(hdc, problem, area, y, line_height, self.theme.error) + s(8);
        }
        let default_endpoint = self.settings.ai_endpoint == ai::DEFAULT_ENDPOINT;
        let key = if remote { self.ai_key_source() } else { None };
        // A service without a key: the button sets one first.
        let needs_key = remote && key.is_none();
        let label = if self.ai.connecting {
            "Connecting\u{2026}".to_string()
        } else if needs_key {
            "Set API Key".to_string()
        } else if self.ai.problem.is_some() {
            "Try again".to_string()
        } else if default_endpoint {
            "Connect to Ollama".to_string()
        } else if remote {
            format!("Connect to {host}")
        } else {
            "Connect".to_string()
        };
        let label = label.as_str();
        let button = RECT {
            left: area.left,
            top: y,
            right: (area.left + self.text_width(hdc, label) + s(32)).min(area.right),
            bottom: y + s(34),
        };
        if button.bottom <= area.bottom {
            let fill = if self.ai.connecting {
                self.theme.active_bg
            } else {
                self.theme.violet
            };
            Self::rounded_fill(hdc, button, s(7), fill);
            let text = if self.ai.connecting {
                self.theme.muted
            } else {
                label_on(fill, 255, 255, 255)
            };
            self.label_mid(
                hdc,
                label,
                button.left + s(16),
                (button.top + button.bottom) / 2,
                text,
                button,
            );
            if needs_key {
                self.ai.hits.borrow_mut().set_key = Some(button);
            } else if !self.ai.connecting {
                self.ai.hits.borrow_mut().connect = Some(button);
            }
            // Another service, or a key: the provider card.
            let other = "Change provider\u{2026}";
            let provider = RECT {
                left: button.right + s(10),
                right: button.right + s(10) + self.text_width(hdc, other) + s(28),
                ..button
            };
            if provider.right <= area.right {
                self.panel_card(hdc, provider, s(7), self.theme.edge, self.theme.editor_bg);
                self.label_mid(
                    hdc,
                    other,
                    provider.left + s(14),
                    (provider.top + provider.bottom) / 2,
                    self.theme.text,
                    provider,
                );
                self.ai.hits.borrow_mut().provider = Some(provider);
            }
        }
        y = button.bottom + s(12);
        let hint = if remote {
            let key = match key {
                Some(ai::KeySource::Saved) => {
                    "Its API key is saved in Windows Credential Manager.".to_string()
                }
                Some(ai::KeySource::Environment) => {
                    format!(
                        "Its API key comes from {}.",
                        ai::key_variable(&self.settings.ai_endpoint)
                    )
                }
                None => "It needs an API key: Set API Key above.".to_string(),
            };
            format!("Uses {}. {key}", self.settings.ai_endpoint)
        } else if default_endpoint {
            format!(
                "No Ollama yet? Get it from ollama.com, then run in a terminal: \
                 ollama pull {SUGGESTED_MODEL}. Or use a cloud service such as OpenAI \
                 with Change provider."
            )
        } else {
            format!("Uses the server at {}.", self.settings.ai_endpoint)
        };
        self.paint_wrapped(hdc, &hint, area, y, line_height, self.theme.muted);
    }

    // A model is chosen but nothing has been asked yet.
    fn paint_ai_empty_chat(&self, hdc: HDC, area: RECT) {
        let s = |value: i32| self.scale(value);
        let mut y = self.paint_ai_intro(hdc, area, "Ask about your code");
        unsafe { SelectObject(hdc, self.small_font) };
        let small = self.text_height(hdc) + s(4);
        let model = self
            .settings
            .ai_model
            .as_deref()
            .map(ai::model_label)
            .unwrap_or_default();
        let raw = self.settings.ai_model.as_deref().unwrap_or_default();
        let local = ai::is_local(&self.settings.ai_endpoint);
        let source = if !local {
            format!(
                "Answers come from {model} via {}. Your question and the files shown as \
                 chips are sent there.",
                ai::host(&self.settings.ai_endpoint)
            )
        } else if ai::is_cloud_model(raw) {
            format!(
                "Answers come from {model}, a cloud model. Your question and the files \
                 shown as chips are sent to ollama.com."
            )
        } else {
            format!("Answers come from {model}, running on this PC.")
        };
        y = self.paint_wrapped(hdc, &source, area, y, small, self.theme.muted);
        unsafe { SelectObject(hdc, self.ui_font) };

        y += s(12);
        let row = s(32);
        let mut hits = Vec::new();
        for (glyph, label, task) in [
            (DebugGlyph::Book, "Explain this file", AiTask::ExplainFile),
            (
                DebugGlyph::Warning,
                "Fix problems in the selection",
                AiTask::Fix,
            ),
            (
                DebugGlyph::Check,
                "Write tests for the selection",
                AiTask::Tests,
            ),
        ] {
            let rect = RECT {
                left: area.left,
                top: y,
                right: area.right,
                bottom: y + row,
            };
            if rect.bottom > area.bottom {
                break;
            }
            self.panel_card(hdc, rect, s(6), self.theme.edge, self.theme.sidebar_bg);
            self.glyph_in(
                hdc,
                glyph,
                self.theme.muted,
                RECT {
                    left: rect.left + s(6),
                    right: rect.left + s(30),
                    ..rect
                },
                s(15),
            );
            self.label_mid(
                hdc,
                label,
                rect.left + s(34),
                (rect.top + rect.bottom) / 2,
                self.theme.text,
                rect,
            );
            hits.push((rect, task));
            y = rect.bottom + s(6);
        }
        self.ai.hits.borrow_mut().suggestions = hits;
        y += s(10);
        unsafe { SelectObject(hdc, self.small_font) };
        self.paint_wrapped(
            hdc,
            "Nothing is sent until you ask, and nothing runs in the background.",
            area,
            y,
            small,
            self.theme.muted,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
    }

    fn paint_ai_conversation(&self, hdc: HDC, area: RECT) {
        let s = |value: i32| self.scale(value);
        if area.bottom <= area.top {
            return;
        }
        let entries = &self.ai.entries;
        if entries.is_empty() {
            self.paint_ai_empty_chat(hdc, area);
            return;
        }
        let width = (area.right - area.left).max(s(60));
        let gap = s(16);
        let heights: Vec<i32> = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| self.ai_entry_height(hdc, index, entry, width))
            .collect();
        let total = heights.iter().sum::<i32>() + gap * (entries.len() as i32 - 1);
        let max_scroll = (total - (area.bottom - area.top)).max(0);
        let scroll = if self.ai.follow.get() {
            max_scroll
        } else {
            self.ai.scroll.get().min(max_scroll)
        };
        // Back at the bottom: follow new text again.
        self.ai.follow.set(scroll >= max_scroll);
        self.ai.scroll.set(scroll);
        unsafe {
            let saved = SaveDC(hdc);
            IntersectClipRect(hdc, area.left, area.top, area.right, area.bottom);
            let mut top = area.top - scroll;
            for (index, (entry, height)) in entries.iter().zip(&heights).enumerate() {
                let bottom = top + height;
                if bottom >= area.top && top <= area.bottom {
                    let bounds = RECT {
                        left: area.left,
                        top,
                        right: area.left + width,
                        bottom,
                    };
                    self.paint_ai_entry(hdc, index, entry, bounds, area);
                }
                top = bottom + gap;
            }
            RestoreDC(hdc, saved);
        }
    }

    // Whether `entry` is the answer still arriving.
    fn ai_receiving(&self, last: bool) -> bool {
        last && self.ai.busy()
    }

    fn ai_entry_height(&self, hdc: HDC, index: usize, entry: &ChatEntry, width: i32) -> i32 {
        let bounds = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: 0,
        };
        if entry.role == Role::User {
            self.ai_question(hdc, entry, bounds, bounds, false)
        } else {
            self.ai_answer(hdc, index, entry, bounds, bounds, false)
        }
    }

    // Paints entry `index` of the conversation within `bounds`.
    fn paint_ai_entry(&self, hdc: HDC, index: usize, entry: &ChatEntry, bounds: RECT, clip: RECT) {
        if entry.role == Role::User {
            self.ai_question(hdc, entry, bounds, clip, true);
        } else {
            self.ai_answer(hdc, index, entry, bounds, clip, true);
        }
    }

    // A question: a bubble on the right, at most 85% of the panel wide,
    // with the code and files sent along. Painted when `paint`; returns
    // its height either way.
    fn ai_question(
        &self,
        hdc: HDC,
        entry: &ChatEntry,
        bounds: RECT,
        clip: RECT,
        paint: bool,
    ) -> i32 {
        let s = |value: i32| self.scale(value);
        unsafe { SelectObject(hdc, self.ui_font) };
        let line_height = self.text_height(hdc) + s(4);
        let pad = s(11);
        let widest = (bounds.right - bounds.left) * 85 / 100;
        let lines = self.wrap_text(hdc, &entry.text, widest - 2 * pad);
        let text_width = lines
            .iter()
            .map(|line| self.text_width(hdc, line))
            .max()
            .unwrap_or(0);
        let wide = entry.context.is_some() || !entry.chips.is_empty();
        let bubble_width = if wide {
            widest
        } else {
            (text_width + 2 * pad).min(widest)
        };
        let inner_width = bubble_width - 2 * pad;
        let context = if entry.context.is_some() {
            line_height + s(2)
        } else {
            0
        };
        let specs = chip_specs(&entry.chips);
        let chips = if specs.is_empty() {
            None
        } else {
            Some(self.ai_chip_spans(hdc, &specs, inner_width, false))
        };
        unsafe { SelectObject(hdc, self.ui_font) };
        let chips_height = chips.as_ref().map_or(0, |(_, height)| s(6) + height);
        let height = s(16) + lines.len() as i32 * line_height + context + chips_height;
        if !paint || bounds.top + height <= clip.top || bounds.top >= clip.bottom {
            return height;
        }
        let bubble = RECT {
            left: bounds.right - bubble_width,
            top: bounds.top,
            right: bounds.right,
            bottom: bounds.top + height,
        };
        Self::rounded_fill(
            hdc,
            bubble,
            s(9),
            blend(self.theme.sidebar_bg, self.theme.blue, 0.16),
        );
        let inner = RECT {
            left: bubble.left + pad,
            right: bubble.right - pad,
            ..clip
        };
        let mut y = bubble.top + s(8);
        for line in &lines {
            Self::label(hdc, line, inner.left, y, self.theme.text, inner);
            y += line_height;
        }
        if let Some(context) = &entry.context {
            self.label_ellipsis(
                hdc,
                &format!("Includes {context}"),
                inner.left,
                y + s(2),
                self.theme.muted,
                inner,
            );
            y += line_height + s(2);
        }
        if let Some((spans, _)) = chips {
            let top = y + s(6);
            for (chip, span) in specs.iter().zip(&spans) {
                let rect = RECT {
                    left: inner.left + span.left,
                    top: top + span.top,
                    right: inner.left + span.right,
                    bottom: top + span.bottom,
                };
                self.paint_ai_chip(hdc, rect, chip, false);
            }
            unsafe { SelectObject(hdc, self.ui_font) };
        }
        height
    }

    // Answer `index`, in the order it came: who answered, then its text,
    // steps and permission card as they arrived, then any error, the
    // changes card, and a row of buttons once it's done. Measuring and
    // painting share this, so they can't disagree. Returns its height.
    fn ai_answer(
        &self,
        hdc: HDC,
        index: usize,
        entry: &ChatEntry,
        bounds: RECT,
        clip: RECT,
        paint: bool,
    ) -> i32 {
        let s = |value: i32| self.scale(value);
        let last = index + 1 == self.ai.entries.len();
        let receiving = self.ai_receiving(last);
        let width = bounds.right - bounds.left;
        let shown = |top: i32, bottom: i32| paint && bottom > clip.top && top < clip.bottom;
        let text_clip = RECT {
            left: bounds.left,
            right: bounds.right,
            ..clip
        };
        unsafe { SelectObject(hdc, self.ui_font) };
        let line_height = self.text_height(hdc) + s(4);
        let mut y = bounds.top;

        // Who answered, by name.
        if shown(y, y + s(22)) {
            self.sparkle_glyph(hdc, bounds.left, y + s(4), s(12), self.theme.violet);
            unsafe { SelectObject(hdc, self.small_font) };
            let name = if entry.model.is_empty() {
                "Assistant".to_string()
            } else {
                ai::model_label(&entry.model)
            };
            self.label_ellipsis(
                hdc,
                &name,
                bounds.left + s(19),
                y + s(2),
                self.theme.muted,
                text_clip,
            );
            unsafe { SelectObject(hdc, self.ui_font) };
        }
        y += s(24);

        let (answer, thinking) = ai::visible_answer(&entry.text);
        let skip = if answer.is_empty() {
            entry.text.len()
        } else {
            answer.as_ptr() as usize - entry.text.as_ptr() as usize
        };
        let at: Vec<usize> = entry.steps.iter().map(|step| step.at).collect();
        let parts = answer_parts(answer, skip, &at);
        let texts = parts
            .iter()
            .filter(|part| matches!(part, AnswerPart::Text(..)))
            .count();
        if entry.views.borrow().len() < texts {
            entry
                .views
                .borrow_mut()
                .resize_with(texts, Default::default);
        }
        let views = entry.views.borrow();
        let mut next_view = 0;
        for part in &parts {
            match *part {
                AnswerPart::Text(start, end) => {
                    // Text that's still growing is laid out again as it
                    // grows; text with steps after it never changes.
                    let revision = if end == answer.len() {
                        entry.revision
                    } else {
                        (1 << 63) | end as u64
                    };
                    let view = &views[next_view];
                    next_view += 1;
                    let height = self.snippet_height(
                        hdc,
                        view,
                        &self.ai.fonts,
                        (&answer[start..end], revision),
                        width,
                    );
                    if shown(y, y + height) {
                        self.paint_snippet(hdc, view, &self.ai.fonts, (bounds.left, y), clip);
                        if !receiving {
                            self.paint_ai_code_toolbars(
                                hdc,
                                index,
                                entry,
                                view,
                                (bounds.left, y),
                                clip,
                            );
                        }
                    }
                    y += height;
                }
                AnswerPart::Steps(first, end) => {
                    y = self.ai_steps(
                        hdc,
                        (index, entry),
                        first..end,
                        receiving,
                        bounds,
                        y,
                        clip,
                        paint,
                    );
                }
            }
        }
        unsafe { SelectObject(hdc, self.ui_font) };

        // What's happening now: a step running, or the model thinking
        // before it writes or after a step.
        if receiving && !entry.asking {
            let row = s(22);
            if let Some((kind, label)) = &entry.running {
                if shown(y, y + row) {
                    let rect = RECT {
                        left: bounds.left,
                        top: y,
                        right: bounds.right,
                        bottom: y + row,
                    };
                    self.paint_ai_step_row(
                        hdc,
                        rect,
                        step_glyph(*kind, false),
                        (&format!("{label}\u{2026}"), ""),
                        self.theme.text,
                        self.theme.edge,
                        clip,
                    );
                }
                y += row + s(6);
            } else if thinking || matches!(parts.last(), None | Some(AnswerPart::Steps(..))) {
                if shown(y, y + line_height) {
                    Self::label(
                        hdc,
                        "Thinking\u{2026}",
                        bounds.left,
                        y,
                        self.theme.muted,
                        text_clip,
                    );
                }
                y += line_height;
            }
        }
        if entry.asking {
            y = self.ai_permission_card(hdc, bounds, y, clip, paint);
        }
        unsafe { SelectObject(hdc, self.ui_font) };
        for (text, color) in [
            (
                entry.error.as_deref(),
                if entry.error.as_deref() == Some("Stopped") {
                    self.theme.muted
                } else {
                    self.theme.error
                },
            ),
            (entry.note.as_deref(), self.theme.muted),
        ] {
            let Some(text) = text else {
                continue;
            };
            y += s(6);
            for line in self.wrap_text(hdc, text, width) {
                if shown(y, y + line_height) {
                    Self::label(hdc, &line, bounds.left, y, color, text_clip);
                }
                y += line_height;
            }
        }
        if !entry.proposals.is_empty() {
            y += s(10);
            if paint {
                self.paint_ai_changes(hdc, index, entry, bounds, y, clip);
                unsafe { SelectObject(hdc, self.ui_font) };
            }
            y += self.ai_changes_height(entry);
        }

        // Once it's done: Copy, Retry (the last answer), the steps and time.
        if !receiving && (!answer.is_empty() || !entry.steps.is_empty()) {
            y += s(6);
            let row = s(24);
            if shown(y, y + row) {
                let mut x = bounds.left - s(4);
                let mut buttons = Vec::new();
                if !answer.is_empty() {
                    buttons.push((DebugGlyph::Copy, AiAction::Copy(answer.to_string())));
                }
                if last {
                    buttons.push((DebugGlyph::Refresh, AiAction::Retry));
                }
                for (glyph, action) in buttons {
                    let button = RECT {
                        left: x,
                        top: y,
                        right: x + s(24),
                        bottom: y + row,
                    };
                    let (glyph, color) = self.ai_copy_glyph(glyph, &action);
                    self.glyph_in(hdc, glyph, color, button, s(14));
                    self.ai.hits.borrow_mut().actions.push((button, action));
                    x = button.right + s(2);
                }
                let meta = answer_meta(entry.steps.len(), entry.took);
                if !meta.is_empty() {
                    unsafe { SelectObject(hdc, self.small_font) };
                    self.label_mid(
                        hdc,
                        &meta,
                        x + s(6),
                        y + row / 2,
                        self.theme.muted,
                        text_clip,
                    );
                    unsafe { SelectObject(hdc, self.ui_font) };
                }
            }
            y += row;
        }
        y - bounds.top
    }

    // A group of steps from `y`; returns where what follows starts. While
    // the answer arrives each step is a line; once it's done, two or more
    // fold into one line that opens them again.
    #[allow(clippy::too_many_arguments)]
    fn ai_steps(
        &self,
        hdc: HDC,
        (index, entry): (usize, &ChatEntry),
        steps: std::ops::Range<usize>,
        receiving: bool,
        bounds: RECT,
        mut y: i32,
        clip: RECT,
        paint: bool,
    ) -> i32 {
        let s = |value: i32| self.scale(value);
        let shown = |top: i32, bottom: i32| paint && bottom > clip.top && top < clip.bottom;
        let row = s(22);
        let first = steps.start;
        let group = &entry.steps[steps];
        let foldable = !receiving && group.len() >= 2;
        let open = !foldable || entry.open_groups.contains(&first);
        y += s(2);
        if foldable {
            let rect = RECT {
                left: bounds.left,
                top: y,
                right: bounds.right,
                bottom: y + row,
            };
            if shown(rect.top, rect.bottom) {
                let (title, detail) = super::super::ai_project::folded_steps_label(group);
                self.glyph_in(
                    hdc,
                    if open {
                        DebugGlyph::ChevronDown
                    } else {
                        DebugGlyph::ChevronRight
                    },
                    self.theme.muted,
                    RECT {
                        left: rect.left - s(2),
                        right: rect.left + s(16),
                        ..rect
                    },
                    s(14),
                );
                unsafe { SelectObject(hdc, self.small_font) };
                let text_clip = RECT {
                    left: rect.left,
                    right: rect.right,
                    ..clip
                };
                let mid = (rect.top + rect.bottom) / 2;
                let title_left = rect.left + s(20);
                self.label_mid(
                    hdc,
                    &title,
                    title_left,
                    mid,
                    blend(self.theme.muted, self.theme.text, 0.6),
                    text_clip,
                );
                if !detail.is_empty() {
                    let detail_left = title_left + self.text_width(hdc, &title);
                    self.label_mid(
                        hdc,
                        &format!(" \u{00b7} {detail}"),
                        detail_left,
                        mid,
                        self.theme.muted,
                        text_clip,
                    );
                }
                unsafe { SelectObject(hdc, self.ui_font) };
                self.ai
                    .hits
                    .borrow_mut()
                    .step_groups
                    .push((rect, index, first));
            }
            y += row;
        }
        if open {
            for step in group {
                let rect = RECT {
                    left: bounds.left,
                    top: y,
                    right: bounds.right,
                    bottom: y + row,
                };
                if shown(rect.top, rect.bottom) {
                    let (main, detail) = step
                        .summary
                        .split_once(" \u{00b7} ")
                        .unwrap_or((step.summary.as_str(), ""));
                    let (color, bar) = if step.error {
                        (
                            blend(self.theme.warning, self.theme.text, 0.3),
                            self.theme.warning,
                        )
                    } else if step.opened.is_some() {
                        (
                            blend(self.theme.blue, self.theme.text, 0.35),
                            self.theme.edge,
                        )
                    } else {
                        (
                            blend(self.theme.muted, self.theme.text, 0.45),
                            self.theme.edge,
                        )
                    };
                    self.paint_ai_step_row(
                        hdc,
                        rect,
                        step_glyph(step.kind, step.error),
                        (main, detail),
                        color,
                        bar,
                        clip,
                    );
                    if let Some((path, line)) = &step.opened {
                        self.ai
                            .hits
                            .borrow_mut()
                            .steps
                            .push((rect, path.clone(), *line));
                    }
                }
                y += row;
            }
        }
        y + s(8)
    }

    // One step's line: a bar on the left, the step's icon, what it did and,
    // on the right, what came of it.
    #[allow(clippy::too_many_arguments)]
    fn paint_ai_step_row(
        &self,
        hdc: HDC,
        rect: RECT,
        glyph: DebugGlyph,
        (main, detail): (&str, &str),
        color: u32,
        bar: u32,
        clip: RECT,
    ) {
        let s = |value: i32| self.scale(value);
        Self::fill(
            hdc,
            RECT {
                left: rect.left,
                top: rect.top + s(2),
                right: rect.left + s(2),
                bottom: rect.bottom - s(2),
            },
            bar,
        );
        self.glyph_in(
            hdc,
            glyph,
            self.theme.muted,
            RECT {
                left: rect.left + s(8),
                right: rect.left + s(24),
                ..rect
            },
            s(14),
        );
        unsafe { SelectObject(hdc, self.small_font) };
        let mid = (rect.top + rect.bottom) / 2;
        let mut right = rect.right;
        if !detail.is_empty() {
            let detail_width = self.text_width(hdc, detail);
            // The result only shows when the step's text has room too.
            if detail_width < (rect.right - rect.left) / 3 {
                right = rect.right - detail_width;
                self.label_mid(
                    hdc,
                    detail,
                    right,
                    mid,
                    self.theme.muted,
                    RECT {
                        left: rect.left,
                        right: rect.right,
                        ..clip
                    },
                );
                right -= s(10);
            }
        }
        let text_height = self.text_height(hdc);
        self.label_ellipsis(
            hdc,
            main,
            rect.left + s(30),
            mid - text_height / 2,
            color,
            RECT {
                left: rect.left,
                right,
                ..clip
            },
        );
        unsafe { SelectObject(hdc, self.ui_font) };
    }

    // The permission card for a panel `width` wide: its title, its text's
    // lines, its buttons (placed from the card's inner left and the top of
    // the button area, wrapping only when they don't fit) and its height.
    fn ai_permission_layout(&self, hdc: HDC, width: i32) -> PermissionLayout {
        let s = |value: i32| self.scale(value);
        let (title, text) = self.ai_permission_text();
        unsafe { SelectObject(hdc, self.small_font) };
        let lines = self.wrap_text(hdc, &text, width - s(24));
        let small = self.text_height(hdc) + s(3);
        unsafe { SelectObject(hdc, self.ui_font) };
        let title_height = self.text_height(hdc) + s(4);
        let room = width - s(24);
        let (mut x, mut y) = (0, 0);
        let mut buttons = Vec::new();
        for (label, answer, primary) in [
            ("Allow in This Chat", Permission::ForChat, true),
            ("Just Once", Permission::Once, false),
            ("Don't Allow", Permission::Deny, false),
        ] {
            let w = self.text_width(hdc, label) + s(20);
            if x > 0 && x + w > room {
                x = 0;
                y += s(32);
            }
            buttons.push((
                RECT {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + s(26),
                },
                label,
                answer,
                primary,
            ));
            x += w + s(6);
        }
        let buttons_height = y + s(26);
        let height =
            s(10) + title_height + lines.len() as i32 * small + s(10) + buttons_height + s(10);
        PermissionLayout {
            title,
            lines,
            buttons,
            height,
        }
    }

    // The permission card from `top`; returns where what follows starts.
    fn ai_permission_card(&self, hdc: HDC, bounds: RECT, top: i32, clip: RECT, paint: bool) -> i32 {
        let s = |value: i32| self.scale(value);
        let width = bounds.right - bounds.left;
        let PermissionLayout {
            title,
            lines,
            buttons,
            height,
        } = self.ai_permission_layout(hdc, width);
        let card = RECT {
            left: bounds.left,
            top: top + s(2),
            right: bounds.right,
            bottom: top + s(2) + height,
        };
        if !paint || card.bottom <= clip.top || card.top >= clip.bottom {
            return card.bottom + s(10);
        }
        self.panel_card(
            hdc,
            card,
            s(8),
            blend(self.theme.sidebar_bg, self.theme.blue, 0.4),
            blend(self.theme.sidebar_bg, self.theme.blue, 0.06),
        );
        let inner_left = card.left + s(12);
        let mut ty = card.top + s(10);
        let text_clip = RECT {
            left: card.left,
            right: card.right - s(12),
            ..clip
        };
        unsafe { SelectObject(hdc, self.ui_font) };
        Self::label(hdc, &title, inner_left, ty, self.theme.text, text_clip);
        ty += self.text_height(hdc) + s(4);
        unsafe { SelectObject(hdc, self.small_font) };
        let small = self.text_height(hdc) + s(3);
        for line in &lines {
            Self::label(hdc, line, inner_left, ty, self.theme.muted, text_clip);
            ty += small;
        }
        unsafe { SelectObject(hdc, self.ui_font) };
        let button_top = ty + s(10);
        let mut hits = Vec::new();
        for (place, label, answer, primary) in buttons {
            let button = RECT {
                left: inner_left + place.left,
                top: button_top + place.top,
                right: inner_left + place.right,
                bottom: button_top + place.bottom,
            };
            if primary {
                Self::rounded_fill(hdc, button, s(4), self.theme.violet);
            } else {
                self.panel_card(hdc, button, s(4), self.theme.edge, self.theme.editor_bg);
            }
            let color = if primary {
                ui(255, 255, 255)
            } else {
                self.theme.text
            };
            self.label_mid(
                hdc,
                label,
                button.left + s(10),
                (button.top + button.bottom) / 2,
                color,
                button,
            );
            hits.push((button, answer));
        }
        self.ai.hits.borrow_mut().permission = hits;
        card.bottom + s(10)
    }

    // The changes card: a 32 px header, a 26 px row per file, and a 42 px
    // row of buttons while anything waits.
    fn ai_changes_height(&self, entry: &ChatEntry) -> i32 {
        let s = |value: i32| self.scale(value);
        let pending = entry
            .proposals
            .iter()
            .any(|item| item.status == ProposalStatus::Pending);
        s(32) + entry.proposals.len() as i32 * s(26) + if pending { s(44) } else { s(8) }
    }

    // Paints answer `index`'s changes card from `top`; returns its bottom.
    fn paint_ai_changes(
        &self,
        hdc: HDC,
        index: usize,
        entry: &ChatEntry,
        bounds: RECT,
        top: i32,
        clip: RECT,
    ) -> i32 {
        let s = |value: i32| self.scale(value);
        let card = RECT {
            left: bounds.left,
            top,
            right: bounds.right,
            bottom: top + self.ai_changes_height(entry),
        };
        let visible = |rect: &RECT| rect.bottom > clip.top && rect.top < clip.bottom;
        let back = self.theme.sidebar_bg;
        self.panel_card(
            hdc,
            card,
            s(8),
            blend(back, self.theme.blue, 0.4),
            blend(back, self.theme.blue, 0.06),
        );
        let pending = entry
            .proposals
            .iter()
            .filter(|item| item.status == ProposalStatus::Pending)
            .count();
        let applied = entry
            .proposals
            .iter()
            .filter(|item| item.status == ProposalStatus::Applied)
            .count();
        let files = entry.proposals.len();
        let (title, note) = if pending > 0 {
            (
                "Proposed changes".to_string(),
                format!(
                    "{files} {} \u{00b7} {}",
                    if files == 1 { "file" } else { "files" },
                    if applied == 0 {
                        "nothing changed yet".to_string()
                    } else {
                        format!("{applied} applied")
                    }
                ),
            )
        } else if applied > 0 {
            (
                "Changes applied".to_string(),
                "not saved yet \u{00b7} Ctrl+Z undoes".to_string(),
            )
        } else {
            ("Changes not applied".to_string(), String::new())
        };
        unsafe { SelectObject(hdc, self.ui_font) };
        let header_middle = card.top + s(16);
        let inner = RECT {
            left: card.left + s(12),
            right: card.right - s(12),
            ..clip
        };
        self.label_mid(
            hdc,
            &title,
            inner.left,
            header_middle,
            self.theme.text,
            inner,
        );
        unsafe { SelectObject(hdc, self.small_font) };
        let note_width = self.text_width(hdc, &note);
        self.label_mid(
            hdc,
            &note,
            (inner.right - note_width).max(inner.left + self.text_width(hdc, &title) + s(70)),
            header_middle,
            self.theme.muted,
            inner,
        );
        Self::fill(
            hdc,
            RECT {
                left: card.left + s(1),
                top: card.top + s(32),
                right: card.right - s(1),
                bottom: card.top + s(32) + 1,
            },
            self.theme.edge,
        );
        let use_theme = self.has_extension("material-icon-theme");
        let mut rows = Vec::new();
        let mut y = card.top + s(32);
        for (file, item) in entry.proposals.iter().enumerate() {
            let row = RECT {
                left: card.left + s(1),
                top: y,
                right: card.right - s(1),
                bottom: y + s(26),
            };
            let middle = (row.top + row.bottom) / 2;
            let icon = RECT {
                left: inner.left,
                top: middle - s(8),
                right: inner.left + s(16),
                bottom: middle + s(8),
            };
            match &item.status {
                ProposalStatus::Applied => {
                    self.glyph_in(hdc, DebugGlyph::Check, self.theme.green, icon, s(14))
                }
                _ => {
                    self.icons.draw_for_path(
                        hdc,
                        &item.proposal.path,
                        false,
                        false,
                        use_theme,
                        icon.left,
                        icon.top + s(1),
                        s(14),
                    );
                }
            }
            let name = item
                .proposal
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| item.proposal.name.clone());
            let folder = match item.proposal.name.rsplit_once('/') {
                Some((folder, _)) => folder.to_string(),
                None => String::new(),
            };
            let detail = match &item.status {
                ProposalStatus::Skipped => "skipped".to_string(),
                ProposalStatus::Stale(_) => "no longer fits".to_string(),
                _ if item.proposal.new_file => {
                    if folder.is_empty() {
                        "new file".to_string()
                    } else {
                        format!("{folder} \u{00b7} new file")
                    }
                }
                _ => folder,
            };
            unsafe { SelectObject(hdc, self.ui_font) };
            let name_left = icon.right + s(8);
            let name_color = match item.status {
                ProposalStatus::Skipped | ProposalStatus::Stale(_) => self.theme.muted,
                _ => self.theme.text,
            };
            self.label_mid(hdc, &name, name_left, middle, name_color, inner);
            let name_right = name_left + self.text_width(hdc, &name);
            unsafe { SelectObject(hdc, self.small_font) };
            let plus = format!("+{}", item.proposal.added);
            let minus = format!("\u{2212}{}", item.proposal.removed);
            let minus_left = inner.right - self.text_width(hdc, &minus);
            let plus_left = minus_left - s(8) - self.text_width(hdc, &plus);
            self.label_mid(hdc, &minus, minus_left, middle, self.theme.error, inner);
            self.label_mid(hdc, &plus, plus_left, middle, self.theme.green, inner);
            let detail_color = match item.status {
                ProposalStatus::Stale(_) => self.theme.warning,
                _ => self.theme.muted,
            };
            self.label_ellipsis(
                hdc,
                &detail,
                name_right + s(8),
                middle - self.text_height(hdc) / 2,
                detail_color,
                RECT {
                    right: plus_left - s(8),
                    ..inner
                },
            );
            if item.status == ProposalStatus::Pending && visible(&row) {
                rows.push((row, index, file));
            }
            y += s(26);
        }
        let mut buttons = Vec::new();
        if pending > 0 {
            unsafe { SelectObject(hdc, self.ui_font) };
            let mut x = inner.left;
            let button_top = y + s(9);
            for (label, button, fill) in [
                ("Review", ProposalButton::Review, Some(self.theme.violet)),
                ("Apply All", ProposalButton::ApplyAll, Some(ui(18, 122, 82))),
                ("Discard", ProposalButton::Discard, None),
            ] {
                let rect = RECT {
                    left: x,
                    top: button_top,
                    right: x + self.text_width(hdc, label) + s(20),
                    bottom: button_top + s(26),
                };
                let color = match fill {
                    Some(fill) => {
                        Self::rounded_fill(hdc, rect, s(4), fill);
                        ui(255, 255, 255)
                    }
                    None => {
                        self.panel_card(hdc, rect, s(4), self.theme.edge, self.theme.editor_bg);
                        self.theme.text
                    }
                };
                self.label_mid(
                    hdc,
                    label,
                    rect.left + s(10),
                    (rect.top + rect.bottom) / 2,
                    color,
                    rect,
                );
                if visible(&rect) {
                    buttons.push((rect, index, button));
                }
                x = rect.right + s(6);
            }
        }
        unsafe { SelectObject(hdc, self.ui_font) };
        let mut hits = self.ai.hits.borrow_mut();
        hits.proposal_rows.extend(rows);
        hits.proposal_buttons.extend(buttons);
        card.bottom
    }

    // The strip at the top of each code block in one piece of answer
    // `index`: its language, icons that put the code at the cursor or copy
    // it, and Apply, which puts it in place of what the question was about
    // (or the selection). Apply only shows when the answer has no changes
    // card, so there's one way to apply a change.
    fn paint_ai_code_toolbars(
        &self,
        hdc: HDC,
        index: usize,
        entry: &ChatEntry,
        view: &MarkdownSnippet,
        (left, top): (i32, i32),
        clip: RECT,
    ) {
        let s = |value: i32| self.scale(value);
        unsafe { SelectObject(hdc, self.ui_font) };
        let can_insert = self.ai_can_insert();
        let can_apply =
            entry.proposals.is_empty() && (can_insert || self.ai_replace_target(index).is_some());
        for block in view.code_blocks() {
            let strip = RECT {
                left: left + block.rect.left,
                top: top + block.rect.top,
                right: left + block.rect.right,
                bottom: top + block.rect.top + s(SNIPPET_CODE_HEADER),
            };
            if strip.bottom <= clip.top || strip.top >= clip.bottom {
                continue;
            }
            Self::fill(
                hdc,
                RECT {
                    top: strip.bottom - s(1).max(1),
                    ..strip
                },
                self.theme.edge,
            );
            let mut right = strip.right - s(4);
            let mid = (strip.top + strip.bottom) / 2;
            let mut icons = vec![(DebugGlyph::Copy, AiAction::Copy(block.text.clone()))];
            if can_insert {
                icons.push((DebugGlyph::Insert, AiAction::Insert(block.text.clone())));
            }
            for (glyph, action) in icons {
                let button = RECT {
                    left: right - s(24),
                    top: mid - s(11),
                    right,
                    bottom: mid + s(11),
                };
                if button.left < strip.left + s(8) {
                    break;
                }
                let (glyph, color) = self.ai_copy_glyph(glyph, &action);
                self.glyph_in(hdc, glyph, color, button, s(14));
                self.ai.hits.borrow_mut().actions.push((button, action));
                right = button.left - s(2);
            }
            if can_apply {
                let label = "Apply";
                unsafe { SelectObject(hdc, self.small_font) };
                let button = RECT {
                    left: right - self.text_width(hdc, label) - s(16),
                    top: mid - s(11),
                    right,
                    bottom: mid + s(11),
                };
                if button.left >= strip.left + s(8) {
                    Self::rounded_fill(hdc, button, s(4), self.theme.sidebar_bg);
                    self.label_mid(hdc, label, button.left + s(8), mid, self.theme.text, button);
                    self.ai
                        .hits
                        .borrow_mut()
                        .actions
                        .push((button, AiAction::Apply(block.text.clone(), index)));
                    right = button.left - s(6);
                }
                unsafe { SelectObject(hdc, self.ui_font) };
            }
            if !block.language.is_empty() {
                unsafe { SelectObject(hdc, self.small_font) };
                self.label_mid(
                    hdc,
                    &block.language,
                    strip.left + s(12),
                    mid,
                    self.theme.muted,
                    RECT {
                        right: right - s(4),
                        ..strip
                    },
                );
                unsafe { SelectObject(hdc, self.ui_font) };
            }
        }
    }

    // The message box: `lines` of typed text (already wrapped), and the Send
    // button, which is Stop while an answer arrives. Disabled until a model
    // is chosen.
    // The message box in `input`: `chips` (placed at `spans`) for what goes
    // with the question, the typed text wrapped, as many lines as fit with
    // the caret's line in view, and a row with @, the model and Send.
    // Disabled until a model is chosen.
    fn paint_ai_composer(
        &self,
        hdc: HDC,
        input: RECT,
        chips: &[ChipSpec],
        spans: &[RECT],
        enabled: bool,
    ) {
        let s = |value: i32| self.scale(value);
        if input.bottom - input.top < s(40) {
            return;
        }
        let focused = enabled && self.ai_typing();
        let edge = if focused {
            self.theme.violet
        } else {
            self.theme.edge
        };
        self.panel_card(hdc, input, s(9), edge, self.theme.editor_bg);
        let inner_left = input.left + s(12);
        let mut top = input.top + s(10);

        // Chips: attached files (✕ takes one off), then the selection or
        // the open file.
        if !chips.is_empty() {
            let attached = self.ai.attachments.len();
            let mut closes = Vec::new();
            let mut bottom = top;
            for (index, (chip, span)) in chips.iter().zip(spans).enumerate() {
                let rect = RECT {
                    left: inner_left + span.left,
                    top: top + span.top,
                    right: inner_left + span.right,
                    bottom: top + span.bottom,
                };
                bottom = bottom.max(rect.bottom);
                if let Some(close) = self.paint_ai_chip(hdc, rect, chip, true) {
                    if index < attached {
                        closes.push((close, index));
                    } else {
                        self.ai.hits.borrow_mut().context_close = Some(close);
                    }
                }
            }
            self.ai.hits.borrow_mut().chip_close = closes;
            top = bottom + s(6);
        }

        let row_top = input.bottom - s(6) - s(28);
        let text = RECT {
            left: inner_left,
            top,
            right: input.right - s(12),
            bottom: row_top - s(4),
        };
        unsafe { SelectObject(hdc, self.ui_font) };
        let line_height = self.text_height(hdc) + s(4);
        let lines = if enabled {
            self.wrap_ranges(hdc, &self.ai.input, text.right - text.left)
        } else {
            Vec::new()
        };
        if !enabled || self.ai.input.is_empty() {
            let placeholder = if !enabled {
                "Connect a model to start chatting"
            } else if self.ai.entries.is_empty() {
                "Ask about your code\u{2026} (@ for files)"
            } else {
                "Ask a follow-up\u{2026} (@ for files)"
            };
            self.label_ellipsis(
                hdc,
                placeholder,
                text.left,
                text.top,
                self.theme.muted,
                text,
            );
        }
        // The lines shown: the caret's line stays in view.
        let caret = self.ai.caret.min(self.ai.input.len());
        let caret_line = super::super::ai_composer::caret_line(&lines, caret);
        let fit = (((text.bottom - text.top) / line_height.max(1)) as usize).max(1);
        let first = self
            .ai
            .composer_first
            .get()
            .min(caret_line)
            .max((caret_line + 1).saturating_sub(fit))
            .min(lines.len().saturating_sub(fit));
        self.ai.composer_first.set(first);
        let mut rows = Vec::new();
        for (index, &(start, end)) in lines.iter().enumerate().skip(first).take(fit) {
            let y = text.top + (index - first) as i32 * line_height;
            if !self.ai.input.is_empty() {
                Self::label(
                    hdc,
                    &self.ai.input[start..end],
                    text.left,
                    y,
                    self.theme.text,
                    text,
                );
            }
            rows.push(super::super::ai_composer::ComposerRow {
                rect: RECT {
                    left: text.left,
                    top: y,
                    right: text.right,
                    bottom: y + line_height,
                },
                start,
                end,
            });
        }
        *self.ai.composer_lines.borrow_mut() = lines.clone();
        *self.ai.composer_rows.borrow_mut() = rows;
        if focused {
            let (x, y) = match lines.get(caret_line) {
                Some(&(start, end)) if caret_line >= first && caret_line < first + fit => (
                    text.left
                        + self.text_width(hdc, &self.ai.input[start..caret.clamp(start, end)]),
                    text.top + (caret_line - first) as i32 * line_height,
                ),
                _ => (text.left, text.top),
            };
            Self::fill(
                hdc,
                RECT {
                    left: x.min(text.right),
                    top: y,
                    right: x.min(text.right) + s(1).max(1),
                    bottom: y + self.text_height(hdc),
                },
                self.theme.text,
            );
        }

        // The row under the text: @, the model, and Send (Stop while an
        // answer arrives).
        let row_mid = row_top + s(14);
        let mut x = input.left + s(6);
        if enabled {
            let at = RECT {
                left: x,
                top: row_mid - s(12),
                right: x + s(26),
                bottom: row_mid + s(12),
            };
            self.label_mid(
                hdc,
                "@",
                at.left + (s(26) - self.text_width(hdc, "@")) / 2,
                row_mid,
                self.theme.muted,
                at,
            );
            self.ai.hits.borrow_mut().at_button = Some(at);
            x = at.right + s(4);
            if let Some(model) = self.settings.ai_model.as_deref() {
                unsafe { SelectObject(hdc, self.small_font) };
                let label = ai::model_label(model);
                let chip = RECT {
                    left: x,
                    top: row_mid - s(12),
                    right: (x + self.text_width(hdc, &label) + s(30)).min(input.right - s(48)),
                    bottom: row_mid + s(12),
                };
                if chip.right - chip.left >= s(48) {
                    if self.ai.model_menu_open {
                        Self::rounded_fill(
                            hdc,
                            chip,
                            s(4),
                            blend(self.theme.editor_bg, self.theme.violet, 0.25),
                        );
                    }
                    self.label_ellipsis(
                        hdc,
                        &label,
                        chip.left + s(8),
                        row_mid - self.text_height(hdc) / 2,
                        blend(self.theme.muted, self.theme.text, 0.5),
                        RECT {
                            right: chip.right - s(20),
                            ..chip
                        },
                    );
                    self.glyph_in(
                        hdc,
                        if self.ai.model_menu_open {
                            DebugGlyph::ChevronUp
                        } else {
                            DebugGlyph::ChevronDown
                        },
                        self.theme.muted,
                        RECT {
                            left: chip.right - s(20),
                            right: chip.right - s(4),
                            ..chip
                        },
                        s(12),
                    );
                    self.ai.hits.borrow_mut().model = Some(chip);
                }
                unsafe { SelectObject(hdc, self.ui_font) };
            }
        }

        let send = RECT {
            left: input.right - s(6) - s(32),
            top: row_mid - s(13),
            right: input.right - s(6),
            bottom: row_mid + s(13),
        };
        let busy = enabled && self.ai.busy();
        let ready = enabled && !self.ai.input.trim().is_empty();
        let fill = if ready && !busy {
            self.theme.violet
        } else {
            self.theme.active_bg
        };
        Self::rounded_fill(hdc, send, s(6), fill);
        unsafe {
            if busy {
                // Stop: a square.
                let half = s(5);
                let (cx, cy) = ((send.left + send.right) / 2, (send.top + send.bottom) / 2);
                Self::fill(
                    hdc,
                    RECT {
                        left: cx - half,
                        top: cy - half,
                        right: cx + half,
                        bottom: cy + half,
                    },
                    self.theme.text,
                );
            } else {
                let color = if ready {
                    label_on(fill, 255, 255, 255)
                } else {
                    self.theme.muted
                };
                let brush = CreateSolidBrush(color);
                let previous_brush = SelectObject(hdc, brush);
                let previous_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
                let middle = (send.top + send.bottom) / 2;
                let cx = (send.left + send.right) / 2;
                let points = [
                    POINT {
                        x: cx - s(6),
                        y: middle - s(6),
                    },
                    POINT {
                        x: cx + s(7),
                        y: middle,
                    },
                    POINT {
                        x: cx - s(6),
                        y: middle + s(6),
                    },
                    POINT {
                        x: cx - s(3),
                        y: middle,
                    },
                ];
                Polygon(hdc, points.as_ptr(), points.len() as i32);
                SelectObject(hdc, previous_pen);
                SelectObject(hdc, previous_brush);
                DeleteObject(brush);
            }
        }
        if enabled {
            let mut hits = self.ai.hits.borrow_mut();
            hits.composer = Some(input);
            hits.send = Some(send);
        }
    }

    // Draws `text` word-wrapped to `area`'s width from `y`, stopping at its
    // bottom; returns the y below the last line.
    /// Where each chip goes, relative to the top left of a box `width`
    /// wide: left to right, wrapping to a new row, each at most the full
    /// width. Also the total height. `closable` chips leave room for a ✕.
    fn ai_chip_spans(
        &self,
        hdc: HDC,
        chips: &[ChipSpec],
        width: i32,
        closable: bool,
    ) -> (Vec<RECT>, i32) {
        let s = |value: i32| self.scale(value);
        let (height, gap) = (s(22), s(5));
        unsafe { SelectObject(hdc, self.small_font) };
        let mut spans = Vec::with_capacity(chips.len());
        let (mut x, mut y) = (0, 0);
        for chip in chips {
            let close = if closable { s(17) } else { 0 };
            let wanted = s(27) + self.text_width(hdc, &chip.label) + close + s(8);
            let w = wanted.min(width);
            if x > 0 && x + w > width {
                x = 0;
                y += height + gap;
            }
            spans.push(RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + height,
            });
            x += w + gap;
        }
        unsafe { SelectObject(hdc, self.ui_font) };
        let total = if chips.is_empty() { 0 } else { y + height };
        (spans, total)
    }

    /// One file chip; with `closable`, also its ✕, whose rectangle is
    /// returned for clicks.
    fn paint_ai_chip(&self, hdc: HDC, rect: RECT, chip: &ChipSpec, closable: bool) -> Option<RECT> {
        let s = |value: i32| self.scale(value);
        let back = self.theme.sidebar_bg;
        let accent = if chip.warn {
            self.theme.warning
        } else {
            self.theme.blue
        };
        if chip.offered {
            self.panel_card(hdc, rect, s(11), self.theme.edge, self.theme.editor_bg);
        } else {
            self.panel_card(
                hdc,
                rect,
                s(11),
                blend(back, accent, 0.45),
                blend(back, accent, 0.1),
            );
        }
        let icon = s(13);
        let middle = (rect.top + rect.bottom) / 2;
        let icon_left = rect.left + s(7);
        let use_theme = self.has_extension("material-icon-theme");
        if !self.icons.draw_for_path(
            hdc,
            &chip.path,
            false,
            false,
            use_theme,
            icon_left,
            middle - icon / 2,
            icon,
        ) {
            Self::rounded_fill(
                hdc,
                RECT {
                    left: icon_left,
                    top: middle - icon / 2,
                    right: icon_left + icon,
                    bottom: middle + icon / 2,
                },
                s(3),
                accent,
            );
        }
        let close = closable.then(|| RECT {
            left: rect.right - s(20),
            top: rect.top,
            right: rect.right - s(3),
            bottom: rect.bottom,
        });
        let text_right = close.map_or(rect.right - s(8), |close| close.left);
        unsafe { SelectObject(hdc, self.small_font) };
        let text = if chip.warn {
            blend(self.theme.warning, self.theme.text, 0.3)
        } else if chip.offered {
            self.theme.muted
        } else {
            blend(self.theme.muted, self.theme.text, 0.55)
        };
        self.label_ellipsis(
            hdc,
            &chip.label,
            icon_left + icon + s(6),
            middle - self.text_height(hdc) / 2,
            text,
            RECT {
                right: text_right,
                ..rect
            },
        );
        if let Some(close) = close {
            self.glyph_in(hdc, DebugGlyph::Close, self.theme.muted, close, s(10));
        }
        unsafe { SelectObject(hdc, self.ui_font) };
        close
    }

    /// The @ list: matching project files above the message box, over the
    /// conversation, never above `top_limit`.
    fn paint_ai_mentions(&self, hdc: HDC, composer: RECT, above: i32, top_limit: i32) {
        let s = |value: i32| self.scale(value);
        let matches = self.ai_mention_matches();
        let row = s(26);
        let footer = s(24);
        let rows = matches.len().max(1) as i32;
        let bottom = above.min(composer.top - s(6));
        let top = (bottom - rows * row - footer - s(8)).max(top_limit + s(4));
        let card = RECT {
            left: composer.left,
            top,
            right: composer.right,
            bottom,
        };
        self.panel_card(
            hdc,
            card,
            s(6),
            blend(self.theme.sidebar_bg, self.theme.blue, 0.4),
            self.theme.editor_bg,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        let use_theme = self.has_extension("material-icon-theme");
        let mut hits = Vec::new();
        let mut y = card.top + s(4);
        if matches.is_empty() {
            let note = if self.quick_loading {
                "Reading the project's files\u{2026}"
            } else if self.workspace_root.is_none() {
                "Open a folder to attach its files"
            } else {
                "No file matches"
            };
            self.label_mid(
                hdc,
                note,
                card.left + s(10),
                y + row / 2,
                self.theme.muted,
                card,
            );
            y += row;
        }
        for (index, path) in matches.iter().enumerate() {
            let rect = RECT {
                left: card.left + s(1),
                top: y,
                right: card.right - s(1),
                bottom: y + row,
            };
            if index == self.ai.mention_selected {
                Self::fill(hdc, rect, self.theme.select_bg);
            }
            let icon = s(15);
            let middle = (rect.top + rect.bottom) / 2;
            self.icons.draw_for_path(
                hdc,
                path,
                false,
                false,
                use_theme,
                rect.left + s(9),
                middle - icon / 2,
                icon,
            );
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let folder = self.ai_file_name(path);
            let folder = folder
                .rsplit_once('/')
                .map(|(folder, _)| folder.to_string())
                .unwrap_or_default();
            let name_left = rect.left + s(32);
            self.label_mid(hdc, &name, name_left, middle, self.theme.text, rect);
            if !folder.is_empty() {
                unsafe { SelectObject(hdc, self.small_font) };
                let name_right = name_left
                    + {
                        unsafe { SelectObject(hdc, self.ui_font) };
                        let width = self.text_width(hdc, &name);
                        unsafe { SelectObject(hdc, self.small_font) };
                        width
                    }
                    + s(12);
                let width = self.text_width(hdc, &folder);
                let x = (rect.right - s(10) - width).max(name_right);
                self.label_ellipsis(
                    hdc,
                    &folder,
                    x,
                    middle - self.text_height(hdc) / 2,
                    self.theme.muted,
                    RECT {
                        right: rect.right - s(10),
                        ..rect
                    },
                );
                unsafe { SelectObject(hdc, self.ui_font) };
            }
            hits.push((rect, path.clone()));
            y += row;
        }
        unsafe { SelectObject(hdc, self.small_font) };
        let foot = RECT {
            top: card.bottom - footer,
            ..card
        };
        Self::fill(
            hdc,
            RECT {
                left: card.left + s(1),
                top: foot.top,
                right: card.right - s(1),
                bottom: foot.top + 1,
            },
            self.theme.edge,
        );
        self.label_mid(
            hdc,
            "Enter attaches \u{00b7} files .gitignore hides are left out \u{00b7} Esc",
            card.left + s(10),
            (foot.top + foot.bottom) / 2,
            self.theme.muted,
            foot,
        );
        unsafe { SelectObject(hdc, self.ui_font) };
        self.ai.hits.borrow_mut().mentions = hits;
    }

    pub(in crate::windows_app) fn paint_wrapped(
        &self,
        hdc: HDC,
        text: &str,
        area: RECT,
        mut y: i32,
        line_height: i32,
        color: u32,
    ) -> i32 {
        for line in self.wrap_text(hdc, text, area.right - area.left) {
            if y + line_height > area.bottom {
                break;
            }
            Self::label(hdc, &line, area.left, y, color, area);
            y += line_height;
        }
        y
    }

    // An icon button's glyph and colour: a green tick for a Copy whose text
    // was just copied.
    fn ai_copy_glyph(&self, glyph: DebugGlyph, action: &AiAction) -> (DebugGlyph, u32) {
        match action {
            AiAction::Copy(text) if self.ai.copied.as_deref() == Some(text.as_str()) => {
                (DebugGlyph::Check, self.theme.green)
            }
            _ => (glyph, self.theme.muted),
        }
    }

    // Like `wrap_text`, but each line is a byte range of `text`: the space
    // at a wrap belongs to neither line, a line break ends one.
    fn wrap_ranges(&self, hdc: HDC, text: &str, width: i32) -> Vec<(usize, usize)> {
        let mut lines = Vec::new();
        let mut base = 0;
        for paragraph in text.split('\n') {
            let (mut start, mut end) = (base, base);
            let mut started = false;
            let mut word_start = base;
            for word in paragraph.split(' ') {
                let (from, to) = (word_start, word_start + word.len());
                word_start = to + 1;
                let candidate = if started { start } else { from };
                if self.text_width(hdc, &text[candidate..to]) <= width {
                    (start, end, started) = (candidate, to, true);
                    continue;
                }
                if started {
                    lines.push((start, end));
                }
                // A word longer than the line is split between characters.
                let mut piece = from;
                for (index, ch) in word.char_indices() {
                    let (at, next) = (from + index, from + index + ch.len_utf8());
                    if at > piece && self.text_width(hdc, &text[piece..next]) > width {
                        lines.push((piece, at));
                        piece = at;
                    }
                }
                (start, end, started) = (piece, to, true);
            }
            lines.push((start, end));
            base += paragraph.len() + 1;
        }
        lines
    }

    // `text` wrapped to `width` pixels in the font selected into `hdc`:
    // at spaces where possible, inside a word only when it can't fit on a
    // line of its own. Line breaks in the text are kept.
    fn wrap_text(&self, hdc: HDC, text: &str, width: i32) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let mut line = String::new();
            for word in paragraph.split(' ') {
                let candidate = if line.is_empty() {
                    word.to_string()
                } else {
                    format!("{line} {word}")
                };
                if self.text_width(hdc, &candidate) <= width {
                    line = candidate;
                    continue;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                // A word longer than the line is split between characters.
                for ch in word.chars() {
                    line.push(ch);
                    if line.chars().count() > 1 && self.text_width(hdc, &line) > width {
                        line.pop();
                        lines.push(std::mem::replace(&mut line, ch.to_string()));
                    }
                }
            }
            lines.push(line);
        }
        lines
    }

    pub(in crate::windows_app) fn sparkle_glyph(
        &self,
        hdc: HDC,
        x: i32,
        y: i32,
        size: i32,
        color: u32,
    ) {
        unsafe {
            let pen = CreatePen(PS_SOLID, self.scale(1).max(1), color);
            let brush = CreateSolidBrush(color);
            let prev_pen = SelectObject(hdc, pen);
            let prev_brush = SelectObject(hdc, brush);

            let s = size;
            let cx = x + s / 2;
            let cy = y + s / 2;
            let points = [
                POINT { x: cx, y },
                POINT {
                    x: cx + s / 6,
                    y: cy - s / 6,
                },
                POINT { x: x + s, y: cy },
                POINT {
                    x: cx + s / 6,
                    y: cy + s / 6,
                },
                POINT { x: cx, y: y + s },
                POINT {
                    x: cx - s / 6,
                    y: cy + s / 6,
                },
                POINT { x, y: cy },
                POINT {
                    x: cx - s / 6,
                    y: cy - s / 6,
                },
            ];
            Polygon(hdc, points.as_ptr(), points.len() as i32);

            SelectObject(hdc, prev_brush);
            SelectObject(hdc, prev_pen);
            DeleteObject(brush);
            DeleteObject(pen);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_shown_in_the_order_it_came() {
        // "Let me look." then two searches, then the answer.
        let answer = "Let me look.\n\nIt returns NaN.";
        let parts = answer_parts(answer, 0, &[12, 12]);
        assert_eq!(
            parts,
            vec![
                AnswerPart::Text(0, 12),
                AnswerPart::Steps(0, 2),
                AnswerPart::Text(12, answer.len()),
            ]
        );
        // Steps before any text come first; blank text between steps
        // doesn't split them.
        let answer = "\n\nDone.";
        assert_eq!(
            answer_parts(answer, 0, &[0, 2]),
            vec![AnswerPart::Steps(0, 2), AnswerPart::Text(2, answer.len())]
        );
        // A hidden <think> block before the answer moves every step back.
        assert_eq!(
            answer_parts("Done.", 20, &[20]),
            vec![AnswerPart::Steps(0, 1), AnswerPart::Text(0, 5)]
        );
        assert_eq!(answer_parts("", 0, &[]), vec![]);
    }

    #[test]
    fn a_finished_answer_says_its_steps_and_time() {
        assert_eq!(
            answer_meta(3, Some(Duration::from_millis(14_200))),
            "3 steps \u{00b7} 14 s"
        );
        assert_eq!(
            answer_meta(1, Some(Duration::from_millis(300))),
            "1 step \u{00b7} 1 s"
        );
        assert_eq!(answer_meta(0, Some(Duration::from_secs(75))), "1 min 15 s");
        assert_eq!(answer_meta(0, None), "");
    }
}
