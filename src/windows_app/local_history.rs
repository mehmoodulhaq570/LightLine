//! Local History in the editor: the saved versions of the active file in
//! the Quick Open box, and one of them compared with the file in the diff
//! view, with Restore to bring it back as an edit (Ctrl+Z undoes it).

use super::*;
use lightline::local_history::{self as history, Version};
use std::fs;
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::Storage::FileSystem::FileTimeToLocalFileTime;

/// A kept version shown beside the file.
pub(super) struct HistoryCompare {
    pub(super) version: Version,
    pub(super) text: String,
}

impl App {
    /// True while the diff view replaces the editor: a Git change or a
    /// Local History version.
    pub(super) fn showing_diff(&self) -> bool {
        (self.side_view == SideView::Review && self.review_file.is_some())
            || self.history_compare.is_some()
    }

    pub(super) fn close_diff(&mut self) {
        self.review_file = None;
        // Its hint (Restore, F7, Esc) no longer applies.
        if self.history_compare.take().is_some() {
            self.status.clear();
        }
    }

    /// Lists the active file's kept versions in the Quick Open box.
    pub(super) fn show_file_history(&mut self, hwnd: HWND) {
        let path = match self.doc().path.clone() {
            Some(path) if !self.tab().is_placeholder() && !self.tab().read_only() => path,
            _ => {
                self.status = "Open a saved file to see its history".into();
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            }
        };
        let versions = history::versions(&path);
        if versions.is_empty() {
            self.status = if self.settings.local_history {
                format!(
                    "No saved versions of {} yet: one is kept each time you save",
                    display_path(&path)
                )
            } else {
                "Local History is off: turn on \"localHistory\" in Settings".into()
            };
            unsafe { InvalidateRect(hwnd, null(), 0) };
            return;
        }
        self.show_quick_open(hwnd);
        self.history_pick = Some(versions);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// The picker's rows that match what's typed: label and version index.
    pub(super) fn history_picks(&self) -> Vec<(String, usize)> {
        let Some(versions) = &self.history_pick else {
            return Vec::new();
        };
        let query = self.quick_query.to_lowercase();
        let now = history_now_ms();
        versions
            .iter()
            .enumerate()
            .map(|(index, version)| {
                let mut label = format!(
                    "{}  ·  {}  ·  {}",
                    when_saved(version.saved_ms, now),
                    super::blame::ago(now.saturating_sub(version.saved_ms) as i64 / 1000),
                    size_label(version.len)
                );
                if index == 0 {
                    label.push_str("  ·  latest");
                }
                (label, index)
            })
            .filter(|(label, _)| label.to_lowercase().contains(&query))
            .collect()
    }

    /// Compares the picked version with the file as it is in the editor.
    pub(super) fn pick_history(&mut self, hwnd: HWND, index: usize) {
        let version = self
            .history_picks()
            .get(index)
            .and_then(|(_, at)| self.history_pick.as_ref()?.get(*at).cloned());
        self.quick_open = false;
        self.history_pick = None;
        self.backbuffer = None;
        let Some(version) = version else {
            return;
        };
        let text = match fs::read(&version.file) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) => {
                self.status = format!("Could not read that version: {error}");
                unsafe { InvalidateRect(hwnd, null(), 0) };
                return;
            }
        };
        self.diff_rows = workflow::text_diff_rows(&text, &self.doc().text());
        self.diff_first = 0;
        self.review_file = None;
        self.panel_focus = false;
        self.terminal_focus = false;
        self.status = if self.diff_rows.is_empty() {
            "This version is the same as the file now".into()
        } else {
            "Restore brings this version back (Ctrl+Z undoes it)  ·  F7 next change  ·  Esc closes"
                .into()
        };
        self.history_compare = Some(HistoryCompare { version, text });
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Replaces the file's text in the editor with the compared version, as
    /// one edit; it's saved like any other.
    pub(super) fn restore_history(&mut self, hwnd: HWND) {
        let Some(compare) = self.history_compare.take() else {
            return;
        };
        let end = {
            let doc = self.doc();
            let line = doc.line_count().saturating_sub(1);
            Pos {
                line,
                byte: doc.line(line).len(),
            }
        };
        self.doc_mut().begin_group();
        self.replace_range(Pos::default(), end, &compare.text);
        self.doc_mut().end_group();
        self.move_cursor(Pos::default(), false);
        self.keep_cursor_visible(hwnd);
        self.status = format!(
            "Restored the version from {}: Ctrl+Z undoes it, Ctrl+S saves it",
            when_saved(compare.version.saved_ms, history_now_ms())
        );
        self.refresh(hwnd);
    }

    /// A click on a line of the compared file goes to that line.
    pub(super) fn history_diff_click(&mut self, hwnd: HWND, y: i32) {
        let top = self.editor_top() + self.scale(43);
        let line = if y >= top && self.line_height > 0 {
            let index = self.diff_first + ((y - top) / self.line_height) as usize;
            self.diff_rows
                .get(index)
                .and_then(|row| row.after_number.or(row.deleted_at))
        } else {
            None
        };
        let Some(number) = line else {
            return;
        };
        self.close_diff();
        let line = number
            .saturating_sub(1)
            .min(self.doc().line_count().saturating_sub(1));
        self.move_cursor(Pos { line, byte: 0 }, false);
        self.keep_cursor_visible(hwnd);
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// The Restore and Close buttons at the right of the diff's header.
    pub(super) fn history_buttons(&self, right: i32) -> (RECT, RECT) {
        let top = self.editor_top() + self.scale(6);
        let bottom = top + self.scale(26);
        let close = RECT {
            left: right - self.scale(16 + 64),
            top,
            right: right - self.scale(16),
            bottom,
        };
        let restore = RECT {
            left: close.left - self.scale(8 + 84),
            top,
            right: close.left - self.scale(8),
            bottom,
        };
        (restore, close)
    }

    /// Handles a click in the diff's header; true when it hit a button.
    pub(super) fn history_header_click(&mut self, hwnd: HWND, x: i32, y: i32) -> bool {
        let (restore, close) = self.history_buttons(self.editor_right(hwnd));
        let inside = |r: RECT| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
        if inside(restore) {
            self.restore_history(hwnd);
        } else if inside(close) {
            self.close_diff();
            unsafe { InvalidateRect(hwnd, null(), 0) };
        } else {
            return false;
        }
        true
    }

    /// The diff header's two labels while a version is compared.
    pub(super) fn history_labels(&self) -> Option<(String, String)> {
        let compare = self.history_compare.as_ref()?;
        let name = self.history_file_name();
        Some((
            format!(
                "SAVED {}  ·  {name}",
                when_saved(compare.version.saved_ms, history_now_ms())
            ),
            format!("NOW  ·  {name}"),
        ))
    }

    /// The active file's name, for the picker's title and the diff header
    /// (the whole path didn't fit beside the date).
    pub(super) fn history_file_name(&self) -> String {
        self.doc()
            .path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

fn history_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// "Today 14:32", "Yesterday 09:05" or "3 Oct 2026 18:40", in local time.
fn when_saved(saved_ms: u64, now_ms: u64) -> String {
    let (saved, now) = (local_minutes(saved_ms), local_minutes(now_ms));
    let (saved_day, now_day) = (saved.div_euclid(1440), now.div_euclid(1440));
    let time = format!(
        "{:02}:{:02}",
        saved.rem_euclid(1440) / 60,
        saved.rem_euclid(60)
    );
    match now_day - saved_day {
        0 => format!("Today {time}"),
        1 => format!("Yesterday {time}"),
        _ => {
            let (year, month, day) = civil_from_days(saved_day);
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            format!("{day} {} {year} {time}", MONTHS[(month - 1) as usize])
        }
    }
}

// Minutes since 1970 in local time (Windows applies the time zone and
// daylight saving).
fn local_minutes(ms: u64) -> i64 {
    // FILETIME counts 100 ns steps from 1601.
    let ticks = ms * 10_000 + 116_444_736_000_000_000;
    let utc = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut local = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let ticks = if unsafe { FileTimeToLocalFileTime(&utc, &mut local) } != 0 {
        (u64::from(local.dwHighDateTime) << 32) | u64::from(local.dwLowDateTime)
    } else {
        ticks
    };
    (ticks.saturating_sub(116_444_736_000_000_000) / 600_000_000) as i64
}

// Days since 1970-01-01 to (year, month, day); Howard Hinnant's algorithm.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn size_label(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_become_calendar_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_384), (2025, 10, 23));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn sizes_read_in_bytes_or_kilobytes() {
        assert_eq!(size_label(900), "900 B");
        assert_eq!(size_label(4300), "4.2 KB");
    }

    #[test]
    fn recent_saves_read_as_today_or_yesterday() {
        let now = 1_761_220_800_000; // 2025-10-23 12:00 UTC
        assert!(when_saved(now - 60_000, now).starts_with("Today "));
        assert!(when_saved(now - 86_400_000, now).starts_with("Yesterday "));
        assert!(when_saved(now - 5 * 86_400_000, now).contains(" Oct 2025 "));
    }
}
