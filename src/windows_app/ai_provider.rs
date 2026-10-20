//! The AI provider card: where the assistant's answers come from. The user
//! picks a service instead of typing a URL, pastes a key (shown as dots),
//! sees the connection checked and the model list filled in, then saves.
//! Painted inside the workbench like the Run Configurations panel, with
//! LightLine's own controls.
//!
//! The key goes to Windows Credential Manager only on Save & Connect; the
//! check before that sends it to the chosen service alone, on a worker
//! thread (ai_chat.rs `ai_check_provider`).

use super::*;
use lightline::ai::{self, KeySource};

const MAX_KEY_BYTES: usize = 1024;
const MAX_URL_BYTES: usize = 2048;
// Model rows shown at once in the dropdown.
const MODEL_ROWS: usize = 6;

pub(super) struct Provider {
    pub(super) name: &'static str,
    // Empty for "Other", which takes its URL from the field.
    endpoint: &'static str,
    note: &'static str,
    // Where the service hands out API keys.
    key_page: Option<&'static str>,
}

pub(super) const PROVIDERS: [Provider; 5] = [
    Provider {
        name: "Ollama",
        endpoint: ai::DEFAULT_ENDPOINT,
        note: "On this PC \u{00b7} free, private, works offline",
        key_page: None,
    },
    Provider {
        name: "OpenAI",
        endpoint: "https://api.openai.com",
        note: "Cloud \u{00b7} needs an API key",
        key_page: Some("https://platform.openai.com/api-keys"),
    },
    Provider {
        name: "OpenRouter",
        endpoint: "https://openrouter.ai/api",
        note: "Cloud \u{00b7} many models, one key",
        key_page: Some("https://openrouter.ai/keys"),
    },
    Provider {
        name: "Groq",
        endpoint: "https://api.groq.com/openai",
        note: "Cloud \u{00b7} fast answers",
        key_page: Some("https://console.groq.com/keys"),
    },
    Provider {
        name: "Other",
        endpoint: "",
        note: "Any OpenAI-compatible server (LM Studio, llama.cpp\u{2026})",
        key_page: None,
    },
];

const OTHER: usize = PROVIDERS.len() - 1;
const OLLAMA_DOWNLOAD: &str = "https://ollama.com/download";

/// The provider `endpoint` belongs to: one of the listed services, else
/// Other.
fn provider_for(endpoint: &str) -> usize {
    let wanted = endpoint.trim().trim_end_matches('/');
    let wanted = wanted.strip_suffix("/v1").unwrap_or(wanted);
    PROVIDERS[..OTHER]
        .iter()
        .position(|p| p.endpoint.eq_ignore_ascii_case(wanted))
        .unwrap_or(OTHER)
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Check {
    /// Not asked yet.
    Idle,
    /// Asking the service for its models.
    Running,
    /// Its chat models.
    Passed(Vec<String>),
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Focus {
    List,
    Url,
    Key,
    Model,
    Check,
    Cancel,
    Save,
}

/// Something on the card the mouse can hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hit {
    Provider(usize),
    Url,
    Key,
    KeyPage,
    RemoveKey,
    Model,
    ModelRow(usize),
    Check,
    Cancel,
    Save,
    Close,
}

pub(super) struct ProviderCard {
    pub(super) choice: usize,
    url: String,
    /// A key typed or pasted; empty keeps the saved one.
    key: String,
    /// Where the chosen endpoint's key comes from now, if it has one.
    key_source: Option<KeySource>,
    pub(super) check: Check,
    /// Bumped by every check, so a slow answer for an earlier choice is
    /// ignored.
    pub(super) generation: u64,
    model: Option<String>,
    model_open: bool,
    model_first: usize,
    focus: Focus,
    hover: Option<Hit>,
    /// Save & Connect pressed before the check finished: save once it passes.
    save_when_passed: bool,
    message: Option<String>,
}

impl ProviderCard {
    pub(super) fn endpoint(&self) -> String {
        if self.choice == OTHER {
            self.url.trim().trim_end_matches('/').to_string()
        } else {
            PROVIDERS[self.choice].endpoint.to_string()
        }
    }

    fn remote(&self) -> bool {
        let endpoint = self.endpoint();
        !endpoint.is_empty() && !ai::is_local(&endpoint)
    }

    fn models(&self) -> &[String] {
        match &self.check {
            Check::Passed(models) => models,
            _ => &[],
        }
    }

    // Fields and buttons Tab visits, in order.
    fn tab_order(&self) -> Vec<Focus> {
        let mut order = vec![Focus::List];
        if self.choice == OTHER {
            order.push(Focus::Url);
        }
        if self.remote() {
            order.push(Focus::Key);
        }
        if !self.models().is_empty() {
            order.push(Focus::Model);
        }
        order.extend([Focus::Check, Focus::Cancel, Focus::Save]);
        order
    }
}

struct Layout {
    card: RECT,
    close: RECT,
    rows: [RECT; PROVIDERS.len()],
    url: Option<RECT>,
    key: Option<RECT>,
    key_page: Option<RECT>,
    key_note: Option<RECT>,
    remove_key: Option<RECT>,
    model: RECT,
    model_rows: Vec<RECT>,
    status: RECT,
    check: RECT,
    cancel: RECT,
    save: RECT,
}

fn contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn open_in_browser(hwnd: HWND, address: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let (operation, target) = (wide("open"), wide(address));
    unsafe {
        ShellExecuteW(
            hwnd,
            operation.as_ptr(),
            target.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        );
    }
}

impl App {
    /// Opens the card: AI: Choose Provider, AI: Set API Key, the model
    /// menu, the setup screen and Settings. `to_key` puts the keyboard in
    /// the key field when the current service takes one.
    pub(super) fn show_ai_provider(&mut self, hwnd: HWND, to_key: bool) {
        let endpoint = self.settings.ai_endpoint.clone();
        let choice = provider_for(&endpoint);
        let mut card = ProviderCard {
            choice,
            url: if choice == OTHER {
                endpoint.clone()
            } else {
                String::new()
            },
            key: String::new(),
            key_source: None,
            check: Check::Idle,
            generation: 0,
            model: self.settings.ai_model.clone(),
            model_open: false,
            model_first: 0,
            focus: Focus::List,
            hover: None,
            save_when_passed: false,
            message: None,
        };
        card.key_source = ai::find_key(&card.endpoint()).map(|(_, source)| source);
        if to_key && card.remote() {
            card.focus = Focus::Key;
        }
        self.ai_provider = Some(card);
        self.ai.model_menu_open = false;
        self.ai.focused = false;
        self.quick_open = false;
        self.editor_context = None;
        self.more_menu = None;
        self.run_choice = None;
        self.terminal_focus = false;
        self.panel_focus = false;
        self.pending_high_surrogate = None;
        self.provider_check_if_ready(hwnd);
        unsafe {
            SetFocus(hwnd);
            InvalidateRect(hwnd, null(), 0);
        }
    }

    fn close_ai_provider(&mut self, hwnd: HWND) {
        self.ai_provider = None;
        self.pending_high_surrogate = None;
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    // Checks the chosen service at once when nothing more is needed from
    // the user: Ollama or another server on this PC, or a service whose key
    // is already known.
    fn provider_check_if_ready(&mut self, hwnd: HWND) {
        let Some(card) = &self.ai_provider else {
            return;
        };
        let endpoint = card.endpoint();
        let ready = !endpoint.is_empty()
            && (!card.remote() || card.key_source.is_some() || !card.key.trim().is_empty());
        if ready {
            self.provider_check(hwnd);
        }
    }

    /// Asks the chosen service for its models, with the typed key (or the
    /// saved one).
    fn provider_check(&mut self, hwnd: HWND) {
        let Some(card) = &mut self.ai_provider else {
            return;
        };
        let endpoint = card.endpoint();
        card.model_open = false;
        card.message = None;
        if endpoint.is_empty() {
            card.check = Check::Failed("Type the server's address first.".into());
            card.focus = Focus::Url;
            card.save_when_passed = false;
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        if !endpoint.contains("://") {
            card.check = Check::Failed(
                "The address should start with http:// or https://, e.g. \
                 http://localhost:1234"
                    .into(),
            );
            card.focus = Focus::Url;
            card.save_when_passed = false;
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let remote = card.remote();
        let typed = card.key.trim().to_string();
        if remote && typed.is_empty() && card.key_source.is_none() {
            card.check = Check::Failed(format!(
                "{} needs an API key: paste it above.",
                ai::host(&endpoint)
            ));
            card.focus = Focus::Key;
            card.save_when_passed = false;
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let server = if typed.is_empty() {
            ai::server(&endpoint)
        } else {
            ai::Server::new(endpoint.clone(), Some(typed))
        };
        card.generation += 1;
        card.check = Check::Running;
        let generation = card.generation;
        self.ai_check_provider(hwnd, server, generation);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// The models a check found (AiEvent::Provider).
    pub(super) fn provider_checked(
        &mut self,
        hwnd: HWND,
        generation: u64,
        result: Result<Vec<String>, String>,
    ) {
        let Some(card) = &mut self.ai_provider else {
            return;
        };
        if card.generation != generation {
            return;
        }
        let remote = card.remote();
        let endpoint = card.endpoint();
        card.check = match result {
            Ok(models) => {
                let models = ai::chat_models(&models);
                if models.is_empty() {
                    Check::Failed(if remote {
                        format!("{} listed no chat models.", ai::host(&endpoint))
                    } else {
                        format!(
                            "Connected, but no models are installed. In a terminal, run: \
                             ollama pull {}",
                            super::ai_chat::SUGGESTED_MODEL
                        )
                    })
                } else {
                    Check::Passed(models)
                }
            }
            Err(error) => {
                let hint = if error.starts_with(ai::NOTHING_ANSWERED) && card.choice == 0 {
                    " Is Ollama running? Open the Ollama app (or run: ollama serve), then \
                     Check again. No Ollama yet? Get it from ollama.com."
                } else {
                    ""
                };
                Check::Failed(format!("{error}{hint}"))
            }
        };
        if let Check::Passed(models) = &card.check {
            let keep = card
                .model
                .as_ref()
                .is_some_and(|model| models.contains(model));
            if !keep {
                card.model = ai::pick_model(models, remote);
            }
            if card.model.is_none() {
                card.message = Some(
                    "Only cloud models are installed; pick one to send your questions to \
                     ollama.com, or run: ollama pull qwen2.5-coder:7b"
                        .into(),
                );
            }
        } else {
            card.save_when_passed = false;
        }
        let save = card.save_when_passed && card.model.is_some();
        card.save_when_passed = false;
        if save {
            self.provider_save(hwnd);
            return;
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Save & Connect: keeps the key, the service and the model, then
    /// closes the card with the assistant ready.
    fn provider_save(&mut self, hwnd: HWND) {
        let Some(card) = &mut self.ai_provider else {
            return;
        };
        if !matches!(card.check, Check::Passed(_)) {
            if card.check != Check::Running {
                card.save_when_passed = true;
                self.provider_check(hwnd);
            } else {
                card.save_when_passed = true;
            }
            return;
        }
        let Some(model) = card.model.clone() else {
            card.message = Some("Pick a model first.".into());
            card.focus = Focus::Model;
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        };
        let endpoint = card.endpoint();
        let host = ai::host(&endpoint);
        let key = card.key.trim().to_string();
        if card.remote() && !key.is_empty() {
            #[cfg(windows)]
            if let Err(error) = ai::save_key(&endpoint, &key) {
                card.message = Some(format!("Couldn't save the API key: {error}"));
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            }
        }
        let models = card.models().to_vec();
        let remote = card.remote();
        self.ai_provider = None;
        self.pending_high_surrogate = None;
        self.ai_stop(hwnd);
        self.ai.models = models;
        self.ai.problem = None;
        self.ai.forget_key_source();
        self.settings.ai_endpoint = endpoint;
        self.settings.ai_model = Some(model.clone());
        let label = if remote {
            format!("AI Assistant: {model} via {host}")
        } else {
            format!("AI Assistant: {model} on this PC")
        };
        self.status = match self.settings.save() {
            Ok(()) => label,
            Err(error) => format!("{label} (not saved: {error})"),
        };
        if self.welcome || !self.ai_assistant_visible {
            self.toggle_ai_assistant(hwnd);
        }
        self.focus_ai_input();
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    fn provider_choose(&mut self, hwnd: HWND, choice: usize) {
        let Some(card) = &mut self.ai_provider else {
            return;
        };
        if card.choice != choice {
            card.choice = choice;
            card.key.clear();
            card.check = Check::Idle;
            card.generation += 1;
            card.model_open = false;
            card.message = None;
            card.save_when_passed = false;
            card.key_source = ai::find_key(&card.endpoint()).map(|(_, source)| source);
            // The model stays only for the service it came from.
            card.model = (card.endpoint() == self.settings.ai_endpoint)
                .then(|| self.settings.ai_model.clone())
                .flatten();
        }
        self.provider_check_if_ready(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    fn provider_remove_key(&mut self, hwnd: HWND) {
        let Some(card) = &mut self.ai_provider else {
            return;
        };
        let endpoint = card.endpoint();
        #[cfg(windows)]
        {
            card.message = Some(match ai::remove_key(&endpoint) {
                Ok(_) => format!("Removed the saved key for {}.", ai::host(&endpoint)),
                Err(error) => format!("Couldn't remove the key: {error}"),
            });
        }
        card.key_source = ai::find_key(&endpoint).map(|(_, source)| source);
        card.check = Check::Idle;
        card.generation += 1;
        self.ai.forget_key_source();
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    fn provider_action(&mut self, hwnd: HWND, hit: Hit) {
        match hit {
            Hit::Provider(index) => {
                self.provider_choose(hwnd, index);
                if let Some(card) = &mut self.ai_provider {
                    card.focus = if index == OTHER {
                        Focus::Url
                    } else if card.remote() && card.key_source.is_none() {
                        Focus::Key
                    } else {
                        Focus::List
                    };
                }
            }
            Hit::Url => self.provider_focus(Focus::Url),
            Hit::Key => self.provider_focus(Focus::Key),
            Hit::KeyPage => {
                let page = self
                    .ai_provider
                    .as_ref()
                    .and_then(|card| PROVIDERS[card.choice].key_page);
                if let Some(page) = page {
                    open_in_browser(hwnd, page);
                    self.status = format!("Opened {page} in your browser");
                } else {
                    open_in_browser(hwnd, OLLAMA_DOWNLOAD);
                    self.status = format!("Opened {OLLAMA_DOWNLOAD} in your browser");
                }
            }
            Hit::RemoveKey => self.provider_remove_key(hwnd),
            Hit::Model => {
                if let Some(card) = &mut self.ai_provider
                    && !card.models().is_empty()
                {
                    card.model_open = !card.model_open;
                    card.focus = Focus::Model;
                    let selected = card
                        .model
                        .as_ref()
                        .and_then(|m| card.models().iter().position(|x| x == m))
                        .unwrap_or(0);
                    card.model_first = selected.saturating_sub(MODEL_ROWS - 1);
                }
            }
            Hit::ModelRow(index) => {
                if let Some(card) = &mut self.ai_provider
                    && let Some(model) = card.models().get(index).cloned()
                {
                    card.model = Some(model);
                    card.model_open = false;
                    card.message = None;
                }
            }
            Hit::Check => self.provider_check(hwnd),
            Hit::Cancel | Hit::Close => {
                self.close_ai_provider(hwnd);
                return;
            }
            Hit::Save => {
                self.provider_save(hwnd);
                return;
            }
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    fn provider_focus(&mut self, focus: Focus) {
        if let Some(card) = &mut self.ai_provider {
            card.focus = focus;
            card.model_open = false;
        }
    }

    fn provider_layout(&self, hwnd: HWND, card: &ProviderCard) -> Layout {
        let s = |v| self.scale(v);
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let remote = card.remote();
        let mut height = s(80) + PROVIDERS.len() as i32 * s(38) + s(14);
        if card.choice == OTHER {
            height += s(68);
        }
        if remote {
            height += s(90);
        }
        height += s(68) + s(60) + s(34) + s(22);
        let width = s(580).min((client.right - s(24)).max(s(320)));
        let left = (client.right - width) / 2;
        let top = ((client.bottom - height) / 2).max(self.chrome_top() + s(8));
        let card_rect = RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let inner_left = left + s(22);
        let inner_right = card_rect.right - s(22);
        let rows = std::array::from_fn(|i| {
            let y = top + s(80) + i as i32 * s(38);
            RECT {
                left: inner_left,
                top: y,
                right: inner_right,
                bottom: y + s(34),
            }
        });
        let field = |y: i32| RECT {
            left: inner_left,
            top: y + s(22),
            right: inner_right,
            bottom: y + s(56),
        };
        let mut y = top + s(80) + PROVIDERS.len() as i32 * s(38) + s(14);
        let mut url = None;
        if card.choice == OTHER {
            url = Some(field(y));
            y += s(68);
        }
        let (mut key, mut key_page, mut key_note, mut remove_key) = (None, None, None, None);
        if remote {
            let rect = field(y);
            key = Some(rect);
            if PROVIDERS[card.choice].key_page.is_some() {
                key_page = Some(RECT {
                    left: inner_right - s(110),
                    top: y,
                    right: inner_right,
                    bottom: y + s(20),
                });
            }
            let note = RECT {
                left: inner_left,
                top: rect.bottom + s(6),
                right: inner_right,
                bottom: rect.bottom + s(28),
            };
            if card.key_source == Some(KeySource::Saved) {
                remove_key = Some(RECT {
                    left: inner_right - s(130),
                    ..note
                });
            }
            key_note = Some(note);
            y += s(90);
        } else if card.choice == 0 {
            // Ollama: "Get Ollama" in place of "Get a key".
            key_page = Some(RECT {
                left: inner_right - s(110),
                top: y,
                right: inner_right,
                bottom: y + s(20),
            });
        }
        let model = field(y);
        y += s(68);
        let status = RECT {
            left: inner_left,
            top: y,
            right: inner_right,
            bottom: y + s(56),
        };
        let buttons_top = card_rect.bottom - s(22) - s(34);
        let button = |left: i32, right: i32| RECT {
            left,
            top: buttons_top,
            right,
            bottom: buttons_top + s(34),
        };
        let save = button(inner_right - s(150), inner_right);
        let cancel = button(save.left - s(100), save.left - s(10));
        let check = button(inner_left, inner_left + s(140));
        let models = card.models();
        let model_rows = if card.model_open {
            let first = card.model_first.min(models.len().saturating_sub(1));
            (first..models.len().min(first + MODEL_ROWS))
                .enumerate()
                .map(|(row, _)| RECT {
                    left: model.left,
                    top: model.bottom + s(4) + row as i32 * s(30),
                    right: model.right,
                    bottom: model.bottom + s(4) + (row as i32 + 1) * s(30),
                })
                .collect()
        } else {
            Vec::new()
        };
        Layout {
            card: card_rect,
            close: RECT {
                left: card_rect.right - s(48),
                top: top + s(14),
                right: card_rect.right - s(16),
                bottom: top + s(46),
            },
            rows,
            url,
            key,
            key_page,
            key_note,
            remove_key,
            model,
            model_rows,
            status,
            check,
            cancel,
            save,
        }
    }

    fn provider_hit(&self, hwnd: HWND, x: i32, y: i32) -> Option<Hit> {
        let card = self.ai_provider.as_ref()?;
        let layout = self.provider_layout(hwnd, card);
        // The open dropdown lies over everything below the model field.
        if let Some(row) = layout.model_rows.iter().position(|r| contains(*r, x, y)) {
            return Some(Hit::ModelRow(card.model_first + row));
        }
        let candidates = [
            (Some(layout.close), Hit::Close),
            (layout.url, Hit::Url),
            (layout.key_page, Hit::KeyPage),
            (layout.key, Hit::Key),
            (layout.remove_key, Hit::RemoveKey),
            (Some(layout.model), Hit::Model),
            (Some(layout.check), Hit::Check),
            (Some(layout.cancel), Hit::Cancel),
            (Some(layout.save), Hit::Save),
        ];
        if let Some((_, hit)) = candidates
            .iter()
            .find(|(rect, _)| rect.is_some_and(|r| contains(r, x, y)))
        {
            return Some(*hit);
        }
        layout
            .rows
            .iter()
            .position(|r| contains(*r, x, y))
            .map(Hit::Provider)
    }

    pub(super) fn ai_provider_click(&mut self, hwnd: HWND, x: i32, y: i32) {
        let hit = self.provider_hit(hwnd, x, y);
        if let Some(card) = &mut self.ai_provider
            && card.model_open
            && !matches!(hit, Some(Hit::ModelRow(_) | Hit::Model))
        {
            card.model_open = false;
        }
        match hit {
            Some(hit) => self.provider_action(hwnd, hit),
            None => unsafe {
                InvalidateRect(hwnd, null(), 0);
            },
        }
    }

    pub(super) fn ai_provider_hover(&mut self, hwnd: HWND, x: i32, y: i32) {
        let hit = self.provider_hit(hwnd, x, y);
        let text = matches!(hit, Some(Hit::Url | Hit::Key));
        let link = matches!(hit, Some(Hit::KeyPage | Hit::RemoveKey));
        unsafe {
            SetCursor(LoadCursorW(
                null_mut(),
                if text {
                    IDC_IBEAM
                } else if link {
                    IDC_HAND
                } else {
                    IDC_ARROW
                },
            ));
        }
        if let Some(card) = &mut self.ai_provider
            && card.hover != hit
        {
            card.hover = hit;
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    pub(super) fn ai_provider_scroll(&mut self, hwnd: HWND, delta: i32) {
        if let Some(card) = &mut self.ai_provider
            && card.model_open
        {
            let max_first = card.models().len().saturating_sub(MODEL_ROWS);
            card.model_first = if delta > 0 {
                card.model_first.saturating_sub(1)
            } else {
                (card.model_first + 1).min(max_first)
            };
            unsafe { InvalidateRect(hwnd, null(), 0) };
        }
    }

    pub(super) fn ai_provider_key(&mut self, hwnd: HWND, key: u32, ctrl: bool, shift: bool) {
        let Some(card) = &mut self.ai_provider else {
            return;
        };
        if key == VK_ESCAPE as u32 {
            if card.model_open {
                card.model_open = false;
                unsafe { InvalidateRect(hwnd, null(), 0) };
            } else {
                self.close_ai_provider(hwnd);
            }
            return;
        }
        if ctrl && (key == 0x53 || key == VK_RETURN as u32) {
            self.provider_save(hwnd);
            return;
        }
        if key == VK_TAB as u32 {
            let order = card.tab_order();
            let at = order.iter().position(|f| *f == card.focus).unwrap_or(0);
            card.focus = order[if shift {
                (at + order.len() - 1) % order.len()
            } else {
                (at + 1) % order.len()
            }];
            card.model_open = false;
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        let up = key == VK_UP as u32;
        let down = key == VK_DOWN as u32;
        let enter = key == VK_RETURN as u32;
        match card.focus {
            Focus::List if up || down => {
                let choice = if up {
                    card.choice.saturating_sub(1)
                } else {
                    (card.choice + 1).min(OTHER)
                };
                self.provider_choose(hwnd, choice);
            }
            Focus::List if enter => {
                let next = card.tab_order().get(1).copied().unwrap_or(Focus::Save);
                card.focus = next;
                if next == Focus::Check {
                    self.provider_save(hwnd);
                    return;
                }
            }
            Focus::Model if card.model_open && (up || down || enter) => {
                let models = card.models();
                let current = card
                    .model
                    .as_ref()
                    .and_then(|m| models.iter().position(|x| x == m))
                    .unwrap_or(0);
                if enter {
                    card.model_open = false;
                } else {
                    let next = if up {
                        current.saturating_sub(1)
                    } else {
                        (current + 1).min(models.len().saturating_sub(1))
                    };
                    card.model = models.get(next).cloned();
                    if next < card.model_first {
                        card.model_first = next;
                    } else if next >= card.model_first + MODEL_ROWS {
                        card.model_first = next + 1 - MODEL_ROWS;
                    }
                }
            }
            Focus::Model if enter || down || key == VK_SPACE as u32 => {
                self.provider_action(hwnd, Hit::Model);
                return;
            }
            Focus::Url | Focus::Key if enter => {
                self.provider_check(hwnd);
                return;
            }
            Focus::Url | Focus::Key => {
                let url = card.focus == Focus::Url;
                let text = if url { &mut card.url } else { &mut card.key };
                let limit = if url { MAX_URL_BYTES } else { MAX_KEY_BYTES };
                let mut edited = true;
                let mut pasted = false;
                if key == VK_BACK as u32 {
                    if ctrl {
                        text.clear();
                    } else {
                        text.pop();
                    }
                } else if ctrl && key == 0x56 {
                    if let Ok(Some(clip)) = clipboard::paste(hwnd) {
                        text.extend(clip.trim().chars().filter(|ch| !ch.is_control()));
                        while text.len() > limit {
                            text.pop();
                        }
                        pasted = true;
                    }
                } else {
                    edited = false;
                }
                if edited {
                    card.check = Check::Idle;
                    card.generation += 1;
                    card.message = None;
                    if url {
                        card.key_source = ai::find_key(&card.endpoint()).map(|(_, source)| source);
                    }
                }
                // A pasted key is checked at once: that's the usual way in.
                if pasted && !url {
                    self.provider_check(hwnd);
                    return;
                }
            }
            Focus::Check | Focus::Cancel | Focus::Save if enter || key == VK_SPACE as u32 => {
                let hit = match card.focus {
                    Focus::Check => Hit::Check,
                    Focus::Cancel => Hit::Cancel,
                    _ => Hit::Save,
                };
                self.provider_action(hwnd, hit);
                return;
            }
            _ => {}
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn ai_provider_character(&mut self, hwnd: HWND, unit: u16) {
        let Some(ch) = super::input::decode_utf16_input(&mut self.pending_high_surrogate, unit)
        else {
            return;
        };
        if ch.is_control() {
            return;
        }
        let Some(card) = &mut self.ai_provider else {
            return;
        };
        let (text, limit) = match card.focus {
            Focus::Url => (&mut card.url, MAX_URL_BYTES),
            Focus::Key => (&mut card.key, MAX_KEY_BYTES),
            _ => return,
        };
        if text.len() + ch.len_utf8() > limit {
            return;
        }
        text.push(ch);
        card.check = Check::Idle;
        card.generation += 1;
        card.message = None;
        if card.focus == Focus::Url {
            card.key_source = ai::find_key(&card.endpoint()).map(|(_, source)| source);
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    pub(super) fn paint_ai_provider(&self, hwnd: HWND, hdc: HDC) {
        let Some(card) = &self.ai_provider else {
            return;
        };
        let layout = self.provider_layout(hwnd, card);
        let s = |v| self.scale(v);
        let theme = &self.theme;
        let mut client = RECT::default();
        unsafe {
            GetClientRect(hwnd, &mut client);
            SelectObject(hdc, self.ui_font);
        }
        Self::fill(
            hdc,
            RECT {
                top: self.chrome_top(),
                ..client
            },
            theme.shell_bg,
        );
        self.panel_card(hdc, layout.card, s(12), theme.edge, theme.sidebar_bg);
        let left = layout.card.left + s(22);
        self.label_mid(
            hdc,
            "AI Provider",
            left,
            layout.card.top + s(30),
            theme.text,
            layout.card,
        );
        self.label_ellipsis(
            hdc,
            "Where the assistant's answers come from.",
            left,
            layout.card.top + s(46),
            theme.muted,
            RECT {
                right: layout.card.right - s(60),
                ..layout.card
            },
        );
        self.provider_button(
            hdc,
            layout.close,
            "\u{00d7}",
            false,
            card.hover == Some(Hit::Close),
        );

        // The services, as radio rows.
        for (index, row) in layout.rows.iter().copied().enumerate() {
            let chosen = card.choice == index;
            let hovered = card.hover == Some(Hit::Provider(index));
            if chosen || hovered {
                self.panel_card(
                    hdc,
                    row,
                    s(6),
                    if chosen && card.focus == Focus::List {
                        theme.blue
                    } else {
                        theme.edge
                    },
                    if chosen {
                        theme.select_bg
                    } else {
                        theme.active_bg
                    },
                );
            }
            let middle = (row.top + row.bottom) / 2;
            let dot = RECT {
                left: row.left + s(12),
                top: middle - s(7),
                right: row.left + s(26),
                bottom: middle + s(7),
            };
            self.panel_card(hdc, dot, s(14), theme.muted, theme.editor_bg);
            if chosen {
                Self::rounded_fill(
                    hdc,
                    RECT {
                        left: dot.left + s(4),
                        top: dot.top + s(4),
                        right: dot.right - s(4),
                        bottom: dot.bottom - s(4),
                    },
                    s(6),
                    theme.violet,
                );
            }
            let name = PROVIDERS[index].name;
            self.label_mid(hdc, name, row.left + s(38), middle, theme.text, row);
            let note_left = row.left + s(38) + s(110).max(self.text_width(hdc, name) + s(16));
            let note = PROVIDERS[index].note;
            self.label_ellipsis(
                hdc,
                note,
                note_left,
                middle - self.text_height(hdc) / 2,
                theme.muted,
                RECT {
                    left: note_left,
                    right: row.right - s(10),
                    ..row
                },
            );
        }

        let field_label = |text: &str, field: RECT| {
            Self::label(
                hdc,
                text,
                field.left,
                field.top - s(22),
                theme.muted,
                layout.card,
            );
        };
        if let Some(url) = layout.url {
            field_label("Server address", url);
            self.provider_field(
                hdc,
                url,
                &card.url,
                "http://localhost:1234",
                card.focus == Focus::Url,
            );
        }
        if let Some(page) = layout.key_page {
            let text = if card.remote() {
                "Get a key \u{2197}"
            } else {
                "Get Ollama \u{2197}"
            };
            let width = self.text_width(hdc, text);
            Self::label(
                hdc,
                text,
                page.right - width,
                page.top,
                if card.hover == Some(Hit::KeyPage) {
                    theme.text
                } else {
                    theme.sky
                },
                page,
            );
        }
        let host = ai::host(&card.endpoint());
        if let Some(key) = layout.key {
            field_label("API key", key);
            let dots = "\u{2022}".repeat(card.key.chars().count().min(64));
            let placeholder = match card.key_source {
                Some(KeySource::Saved) => "Saved key in use \u{00b7} paste a new one to replace it",
                Some(KeySource::Environment) => {
                    "Using OPENAI_API_KEY \u{00b7} paste a key to save one instead"
                }
                None => "Paste your API key (Ctrl+V)",
            };
            self.provider_field(hdc, key, &dots, placeholder, card.focus == Focus::Key);
        }
        if let Some(note) = layout.key_note {
            let text = format!("Kept in Windows Credential Manager and sent only to {host}.");
            let right = layout.remove_key.map_or(note.right, |r| r.left - s(8));
            self.label_ellipsis(
                hdc,
                &text,
                note.left,
                note.top,
                theme.muted,
                RECT { right, ..note },
            );
        }
        if let Some(remove) = layout.remove_key {
            let text = "Remove saved key";
            let width = self.text_width(hdc, text);
            Self::label(
                hdc,
                text,
                remove.right - width,
                remove.top,
                if card.hover == Some(Hit::RemoveKey) {
                    theme.text
                } else {
                    theme.sky
                },
                remove,
            );
        }

        // The model dropdown, filled by the check.
        field_label("Model", layout.model);
        let models = card.models();
        let model_text = match (&card.check, &card.model) {
            (Check::Passed(_), Some(model)) => model.clone(),
            (Check::Passed(_), None) => "Pick a model".to_string(),
            (Check::Running, _) => "Checking\u{2026}".to_string(),
            _ => "Check the connection to list the models".to_string(),
        };
        let enabled = !models.is_empty();
        self.panel_card(
            hdc,
            layout.model,
            s(6),
            if card.focus == Focus::Model {
                theme.blue
            } else {
                theme.edge
            },
            theme.editor_bg,
        );
        let middle = (layout.model.top + layout.model.bottom) / 2;
        self.label_ellipsis(
            hdc,
            &model_text,
            layout.model.left + s(10),
            middle - self.text_height(hdc) / 2,
            if enabled && card.model.is_some() {
                theme.text
            } else {
                theme.muted
            },
            RECT {
                right: layout.model.right - s(30),
                ..layout.model
            },
        );
        if enabled {
            self.label_mid(
                hdc,
                "\u{25be}",
                layout.model.right - s(22),
                middle,
                theme.muted,
                layout.model,
            );
        }

        // How the check went.
        let line_height = self.text_height(hdc) + s(4);
        let (status, color) = match (&card.message, &card.check) {
            (Some(message), _) => (message.clone(), theme.text),
            (None, Check::Running) => (format!("Checking {host}\u{2026}"), theme.muted),
            (None, Check::Passed(models)) => (
                if card.remote() {
                    format!(
                        "\u{2713} Connected to {host} \u{00b7} {} chat models",
                        models.len()
                    )
                } else if card.choice == 0 {
                    format!(
                        "\u{2713} Ollama is running \u{00b7} {} models installed",
                        models.len()
                    )
                } else {
                    format!(
                        "\u{2713} Connected to {host} \u{00b7} {} models",
                        models.len()
                    )
                },
                theme.green,
            ),
            (None, Check::Failed(error)) => (error.clone(), theme.error),
            (None, Check::Idle) => (
                if card.choice == OTHER && card.url.trim().is_empty() {
                    "Type the server's address, then press Enter to check it.".to_string()
                } else if card.remote() && card.key_source.is_none() && card.key.is_empty() {
                    "Paste a key and it's checked straight away.".to_string()
                } else {
                    "Press Enter or Check connection to check it.".to_string()
                },
                theme.muted,
            ),
        };
        self.paint_wrapped(
            hdc,
            &status,
            layout.status,
            layout.status.top,
            line_height,
            color,
        );

        self.provider_button(
            hdc,
            layout.check,
            if card.check == Check::Running {
                "Checking\u{2026}"
            } else {
                "Check connection"
            },
            card.focus == Focus::Check,
            card.hover == Some(Hit::Check),
        );
        self.provider_button(
            hdc,
            layout.cancel,
            "Cancel",
            card.focus == Focus::Cancel,
            card.hover == Some(Hit::Cancel),
        );
        // The primary action, in the assistant's color.
        let save = layout.save;
        Self::rounded_fill(
            hdc,
            save,
            s(7),
            if card.focus == Focus::Save {
                theme.blue
            } else {
                theme.violet
            },
        );
        let label = "Save & Connect";
        let width = self.text_width(hdc, label);
        self.label_mid(
            hdc,
            label,
            (save.left + save.right - width) / 2,
            (save.top + save.bottom) / 2,
            label_on(theme.violet, 255, 255, 255),
            save,
        );

        // The open model list goes over everything below the field.
        if !layout.model_rows.is_empty() {
            let list = RECT {
                top: layout.model_rows[0].top - s(2),
                bottom: layout.model_rows[layout.model_rows.len() - 1].bottom + s(2),
                ..layout.model
            };
            self.panel_card(hdc, list, s(6), theme.edge, theme.sidebar_bg);
            for (row, rect) in layout.model_rows.iter().copied().enumerate() {
                let index = card.model_first + row;
                let Some(model) = models.get(index) else {
                    break;
                };
                let selected = card.model.as_ref() == Some(model);
                if selected || card.hover == Some(Hit::ModelRow(index)) {
                    Self::fill(
                        hdc,
                        RECT {
                            left: rect.left + s(2),
                            right: rect.right - s(2),
                            ..rect
                        },
                        if selected {
                            theme.select_bg
                        } else {
                            theme.active_bg
                        },
                    );
                }
                self.label_ellipsis(
                    hdc,
                    model,
                    rect.left + s(10),
                    (rect.top + rect.bottom) / 2 - self.text_height(hdc) / 2,
                    theme.text,
                    RECT {
                        right: rect.right - s(10),
                        ..rect
                    },
                );
            }
            if models.len() > MODEL_ROWS {
                let track = RECT {
                    left: list.right - s(6),
                    top: list.top + s(4),
                    right: list.right - s(3),
                    bottom: list.bottom - s(4),
                };
                let height = track.bottom - track.top;
                let thumb = (height * MODEL_ROWS as i32 / models.len() as i32).max(s(16));
                let max_first = (models.len() - MODEL_ROWS).max(1) as i32;
                let top = track.top + (height - thumb) * card.model_first as i32 / max_first;
                Self::rounded_fill(
                    hdc,
                    RECT {
                        top,
                        bottom: top + thumb,
                        ..track
                    },
                    s(2),
                    theme.muted,
                );
            }
        }
    }

    // A one-line text field with the caret at the end while focused.
    fn provider_field(&self, hdc: HDC, rect: RECT, text: &str, placeholder: &str, focused: bool) {
        let s = |v| self.scale(v);
        self.panel_card(
            hdc,
            rect,
            s(6),
            if focused {
                self.theme.blue
            } else {
                self.theme.edge
            },
            self.theme.editor_bg,
        );
        let inner = RECT {
            left: rect.left + s(10),
            top: rect.top,
            right: rect.right - s(10),
            bottom: rect.bottom,
        };
        let middle = (rect.top + rect.bottom) / 2;
        let top = middle - self.text_height(hdc) / 2;
        if text.is_empty() {
            self.label_ellipsis(hdc, placeholder, inner.left, top, self.theme.muted, inner);
        } else {
            // A long entry shows its end, where typing goes.
            let width = self.text_width(hdc, text);
            let x = inner.left - (width - (inner.right - inner.left - s(4))).max(0);
            Self::label(hdc, text, x, top, self.theme.text, inner);
        }
        if focused {
            let caret = (inner.left + self.text_width(hdc, text)).min(inner.right - s(2));
            Self::fill(
                hdc,
                RECT {
                    left: caret,
                    top: middle - s(9),
                    right: caret + s(1).max(1),
                    bottom: middle + s(9),
                },
                self.theme.text,
            );
        }
    }

    fn provider_button(&self, hdc: HDC, rect: RECT, label: &str, focused: bool, hovered: bool) {
        let s = |v| self.scale(v);
        self.panel_card(
            hdc,
            rect,
            s(6),
            if focused {
                self.theme.blue
            } else {
                self.theme.edge
            },
            if hovered {
                self.theme.active_bg
            } else {
                self.theme.editor_bg
            },
        );
        let width = self.text_width(hdc, label);
        self.label_mid(
            hdc,
            label,
            (rect.left + rect.right - width) / 2,
            (rect.top + rect.bottom) / 2,
            self.theme.text,
            rect,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_map_to_their_provider() {
        assert_eq!(provider_for("http://localhost:11434"), 0);
        assert_eq!(provider_for("https://api.openai.com/"), 1);
        assert_eq!(provider_for("https://api.openai.com/v1"), 1);
        assert_eq!(provider_for("https://openrouter.ai/api"), 2);
        assert_eq!(provider_for("https://api.groq.com/openai/v1"), 3);
        assert_eq!(provider_for("http://localhost:1234/v1"), OTHER);
        assert_eq!(provider_for(""), OTHER);
    }

    #[test]
    fn every_service_but_ollama_and_other_has_a_key_page() {
        for (index, provider) in PROVIDERS.iter().enumerate() {
            let remote = !provider.endpoint.is_empty() && !ai::is_local(provider.endpoint);
            assert_eq!(provider.key_page.is_some(), remote, "{}", provider.name);
            assert_eq!(index == OTHER, provider.endpoint.is_empty());
        }
    }
}
