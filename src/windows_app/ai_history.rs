//! Earlier AI Assistant chats (design/ai-chat-design.html, part 3): each
//! chat is kept on this PC when an answer finishes (aiKeepChats), and the
//! clock in the panel's header lists the workspace's earlier ones, newest
//! first, to open or delete. lightline::chat_store reads and writes them;
//! every read and write runs on a thread, and the list is only read when
//! it's opened.

use super::ai_chat::ChatEntry;
use super::*;
use lightline::chat_store::{self, ChatSummary, StoredChat};

/// Rows the list shows at once.
pub(super) const HISTORY_ROWS: usize = 10;

/// How a chat's time reads in the list, and the group it's in: today's by
/// the clock ("14:02"), yesterday's and this week's by the day ("Mon"),
/// older ones by the date ("18 Nov").
pub(super) fn chat_when(updated: u64, now: u64) -> (&'static str, String) {
    let minutes = super::local_history::local_minutes(updated);
    let today = super::local_history::local_minutes(now).div_euclid(1440);
    let day = minutes.div_euclid(1440);
    let time = format!(
        "{:02}:{:02}",
        minutes.rem_euclid(1440) / 60,
        minutes.rem_euclid(60)
    );
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    match today - day {
        0 => ("TODAY", time),
        1 => ("YESTERDAY", time),
        2..=6 => ("EARLIER", DAYS[day.rem_euclid(7) as usize].to_string()),
        _ => {
            let (_, month, date) = super::local_history::civil_from_days(day);
            (
                "EARLIER",
                format!("{date} {}", MONTHS[(month - 1) as usize]),
            )
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

impl App {
    // The workspace's chat folder.
    fn ai_chats_dir(&self) -> Option<PathBuf> {
        Some(chat_store::workspace_dir(
            &chat_store::base_dir()?,
            self.workspace_root.as_deref(),
        ))
    }

    /// Keeps the chat as it is now, when aiKeepChats is on: an answer just
    /// finished. Written on a thread; the first save gives it its id.
    pub(super) fn ai_keep_chat(&mut self) {
        if !self.settings.ai_keep_chats || self.ai.entries.is_empty() {
            return;
        }
        let Some(dir) = self.ai_chats_dir() else {
            return;
        };
        let now = now_ms();
        let id = self
            .ai
            .chat_id
            .get_or_insert_with(|| now.to_string())
            .clone();
        let title = self
            .ai
            .entries
            .iter()
            .find(|entry| entry.role == lightline::ai::Role::User)
            .map_or_else(|| "Chat".into(), |entry| chat_store::title_for(&entry.text));
        let chat = StoredChat {
            id,
            title,
            updated: now,
            entries: self.ai.entries.iter().map(ChatEntry::to_stored).collect(),
        };
        std::thread::spawn(move || {
            let _ = chat_store::save(&dir, &chat);
        });
    }

    /// The clock in the header: opens the list of earlier chats (read on a
    /// thread) or closes it.
    pub(super) fn ai_toggle_history(&mut self, hwnd: HWND) {
        if self.ai.history_open {
            self.ai.history_open = false;
        } else {
            self.ai.model_menu_open = false;
            self.ai.history_open = true;
            self.ai.history_query.clear();
            self.ai.history_selected = 0;
            self.ai_read_history(hwnd);
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    fn ai_read_history(&mut self, hwnd: HWND) {
        let Some(dir) = self.ai_chats_dir() else {
            return;
        };
        self.ai.history_loading = true;
        let tx = self.ai.tx.clone();
        let hwnd_value = hwnd as isize;
        std::thread::spawn(move || {
            let _ = tx.send(super::ai_chat::AiEvent::Chats(chat_store::list(&dir)));
            unsafe { PostMessageW(hwnd_value as HWND, super::ai_chat::AI_EVENT_MESSAGE, 0, 0) };
        });
    }

    pub(super) fn ai_chats_listed(&mut self, chats: Vec<ChatSummary>) {
        self.ai.history_loading = false;
        self.ai.history = chats;
        let count = self.ai_history_matches().len();
        self.ai.history_selected = self.ai.history_selected.min(count.saturating_sub(1));
    }

    /// The earlier chats whose title has what's typed, newest first: their
    /// places in `history`.
    pub(super) fn ai_history_matches(&self) -> Vec<usize> {
        let query = self.ai.history_query.to_lowercase();
        self.ai
            .history
            .iter()
            .enumerate()
            .filter(|(_, chat)| query.is_empty() || chat.title.to_lowercase().contains(&query))
            .map(|(index, _)| index)
            .collect()
    }

    // Opens row `row` of the filtered list: the chat is read on a thread.
    fn ai_open_history_row(&mut self, hwnd: HWND, row: usize) {
        let Some(&index) = self.ai_history_matches().get(row) else {
            return;
        };
        let (Some(dir), Some(chat)) = (self.ai_chats_dir(), self.ai.history.get(index)) else {
            return;
        };
        let id = chat.id.clone();
        self.ai.history_open = false;
        let tx = self.ai.tx.clone();
        let hwnd_value = hwnd as isize;
        std::thread::spawn(move || {
            let _ = tx.send(super::ai_chat::AiEvent::ChatLoaded(chat_store::load(
                &dir, &id,
            )));
            unsafe { PostMessageW(hwnd_value as HWND, super::ai_chat::AI_EVENT_MESSAGE, 0, 0) };
        });
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// An earlier chat, read: it replaces the one showing, which stops
    /// first. It goes on from where it ended, asking again before the model
    /// looks through the project.
    pub(super) fn ai_chat_loaded(&mut self, hwnd: HWND, chat: Option<StoredChat>) {
        let Some(chat) = chat else {
            self.status = "That chat couldn't be read".into();
            return;
        };
        self.ai_stop(hwnd);
        if self.ai_review.is_some() {
            self.close_diff();
        }
        self.ai.entries = chat.entries.iter().map(ChatEntry::from_stored).collect();
        self.ai.chat_id = Some(chat.id);
        self.ai.chat_allowed = false;
        self.ai.scroll.set(0);
        self.ai.follow.set(true);
        self.focus_ai_input();
    }

    // Deletes row `row` of the filtered list, then reads the list again.
    fn ai_delete_history_row(&mut self, hwnd: HWND, row: usize) {
        let Some(&index) = self.ai_history_matches().get(row) else {
            return;
        };
        let (Some(dir), Some(chat)) = (self.ai_chats_dir(), self.ai.history.get(index)) else {
            return;
        };
        let id = chat.id.clone();
        if self.ai.chat_id.as_deref() == Some(id.as_str()) {
            // The chat showing is now a new one if it's kept again.
            self.ai.chat_id = None;
        }
        self.ai.history.remove(index);
        let tx = self.ai.tx.clone();
        let hwnd_value = hwnd as isize;
        std::thread::spawn(move || {
            let _ = chat_store::delete(&dir, &id);
            let _ = tx.send(super::ai_chat::AiEvent::Chats(chat_store::list(&dir)));
            unsafe { PostMessageW(hwnd_value as HWND, super::ai_chat::AI_EVENT_MESSAGE, 0, 0) };
        });
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// AI: Delete All Chats: every kept chat, of every folder, after asking.
    pub(super) fn ai_delete_all_chats(&mut self, hwnd: HWND) {
        let Some(base) = chat_store::base_dir() else {
            return;
        };
        let answer = dialog::show_dialog(
            hwnd,
            "Delete all AI chats?",
            "Every chat the AI Assistant kept on this PC, for every folder, is deleted. \
             This can't be undone.",
            dialog::DialogIcon::Question,
            &[
                dialog::DialogButton {
                    label: "Delete All",
                    id: dialog::DLG_YES,
                    is_default: false,
                    is_cancel: false,
                },
                dialog::DialogButton {
                    label: "Cancel",
                    id: dialog::DLG_NO,
                    is_default: true,
                    is_cancel: true,
                },
            ],
        );
        if answer != dialog::DLG_YES {
            return;
        }
        self.status = match chat_store::delete_all(&base) {
            Ok(()) => "Deleted all AI chats".into(),
            Err(error) => format!("Couldn't delete the AI chats: {error}"),
        };
        self.ai.chat_id = None;
        self.ai.history.clear();
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Keys while the list is open: Up/Down choose, Enter opens, Delete
    /// deletes, Esc closes, Backspace edits the filter.
    pub(super) fn ai_history_key(&mut self, hwnd: HWND, key: u32, ctrl: bool) -> bool {
        let count = self.ai_history_matches().len();
        match key {
            k if k == VK_ESCAPE as u32 => self.ai.history_open = false,
            k if k == VK_UP as u32 && count > 0 => {
                self.ai.history_selected = (self.ai.history_selected + count - 1) % count;
            }
            k if k == VK_DOWN as u32 && count > 0 => {
                self.ai.history_selected = (self.ai.history_selected + 1) % count;
            }
            k if k == VK_RETURN as u32 => {
                let row = self.ai.history_selected;
                self.ai_open_history_row(hwnd, row);
            }
            k if k == VK_DELETE as u32 => {
                let row = self.ai.history_selected;
                self.ai_delete_history_row(hwnd, row);
            }
            k if k == VK_BACK as u32 => {
                self.ai.history_query.pop();
                self.ai.history_selected = 0;
            }
            _ if (VK_F1 as u32..=VK_F24 as u32).contains(&key) => return false,
            _ if ctrl => return false,
            _ => {}
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
        true
    }

    /// A typed character filters the list.
    pub(super) fn ai_history_char(&mut self, hwnd: HWND, ch: char) {
        if self.ai.history_query.len() < 128 {
            self.ai.history_query.push(ch);
            self.ai.history_selected = 0;
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// A click while the list is open: a row opens its chat, its ✕ deletes
    /// it, the clock or anywhere outside closes the list.
    pub(super) fn ai_history_click(
        &mut self,
        hwnd: HWND,
        hits: &super::ai_chat::AiHits,
        x: i32,
        y: i32,
    ) {
        let inside =
            |rect: &RECT| x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
        if let Some((_, row)) = hits.history_delete.filter(|(rect, _)| inside(rect)) {
            self.ai_delete_history_row(hwnd, row);
        } else if let Some((_, row)) = hits.history_rows.iter().find(|(rect, _)| inside(rect)) {
            self.ai_open_history_row(hwnd, *row);
        } else if !hits.history_card.as_ref().is_some_and(inside) {
            self.ai.history_open = false;
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_times_read_by_how_long_ago() {
        // Local time differs per PC; the groups only depend on whole days.
        let now = now_ms();
        assert_eq!(chat_when(now, now).0, "TODAY");
        assert_eq!(chat_when(now - 86_400_000, now).0, "YESTERDAY");
        let (group, when) = chat_when(now - 3 * 86_400_000, now);
        assert_eq!(group, "EARLIER");
        assert_eq!(when.len(), 3, "a weekday: {when}");
        let (_, when) = chat_when(now - 40 * 86_400_000, now);
        assert!(when.contains(' '), "a date: {when}");
    }
}
