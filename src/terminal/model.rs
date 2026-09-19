use super::input::InputModes;
use super::{SessionId, SessionKind, SessionStatus, TerminalSize};
use std::collections::VecDeque;

pub use vt100::Color;

pub const SCROLLBACK_LINES: usize = 5_000;
const MAX_REPLIES: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cell {
    pub text: String,
    pub foreground: Color,
    pub background: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub wide: bool,
    pub wide_continuation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    pub cells: Vec<Cell>,
    pub wrapped: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub row: u16,
    pub column: u16,
    pub visible: bool,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub session_id: SessionId,
    pub kind: SessionKind,
    pub generation: u64,
    pub status: SessionStatus,
    pub size: TerminalSize,
    pub rows: Vec<Row>,
    pub cursor: Cursor,
    pub modes: InputModes,
    pub alternate_screen: bool,
    pub scrollback_offset: usize,
    pub scrollback_available: usize,
    pub title: String,
    pub bell_count: u64,
}

impl Snapshot {
    // Coordinates are viewport cells; the end is exclusive. Hold an Arc<Snapshot>
    // while selecting to keep the selection stable even as the terminal updates.
    pub fn selected_text(&self, start: (u16, u16), end: (u16, u16)) -> String {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let mut text = String::new();
        for index in usize::from(start.0)..=usize::from(end.0) {
            let Some(row) = self.rows.get(index) else {
                break;
            };
            let first = if index == usize::from(start.0) {
                usize::from(start.1)
            } else {
                0
            };
            let last = if index == usize::from(end.0) {
                usize::from(end.1)
            } else {
                row.cells.len()
            };
            let mut line = String::new();
            for cell in row.cells.iter().take(last).skip(first) {
                if !cell.wide_continuation {
                    if cell.text.is_empty() {
                        line.push(' ');
                    } else {
                        line.push_str(&cell.text);
                    }
                }
            }
            text.push_str(line.trim_end_matches(' '));
            if index != usize::from(end.0) && !row.wrapped {
                text.push('\n');
            }
        }
        text
    }

    pub fn text(&self) -> String {
        self.selected_text((0, 0), (self.size.rows - 1, self.size.columns))
    }
}

#[derive(Default)]
struct Callbacks {
    replies: VecDeque<Vec<u8>>,
    overflow: bool,
    title: String,
    bells: u64,
}

impl Callbacks {
    fn reply(&mut self, bytes: Vec<u8>) {
        if self.replies.len() < MAX_REPLIES {
            self.replies.push_back(bytes);
        } else {
            self.overflow = true;
        }
    }
}

impl vt100::Callbacks for Callbacks {
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.bells = self.bells.wrapping_add(1);
    }

    fn visual_bell(&mut self, screen: &mut vt100::Screen) {
        self.audible_bell(screen);
    }

    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = String::from_utf8_lossy(title)
            .chars()
            .filter(|c| !c.is_control())
            .take(256)
            .collect();
    }

    fn unhandled_escape(
        &mut self,
        _: &mut vt100::Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        byte: u8,
    ) {
        if i1.is_none() && i2.is_none() && byte == b'Z' {
            self.reply(b"\x1b[?1;2c".to_vec());
        }
    }

    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        params: &[&[u16]],
        command: char,
    ) {
        if i2.is_some() {
            return;
        }
        let first = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        match (i1, command, first) {
            (None, 'c', 0) => self.reply(b"\x1b[?1;2c".to_vec()),
            (Some(b'>'), 'c', 0) => self.reply(b"\x1b[>0;1;0c".to_vec()),
            (None, 'n', 5) => self.reply(b"\x1b[0n".to_vec()),
            (prefix @ (None | Some(b'?')), 'n', 6) => {
                let (row, column) = screen.cursor_position();
                let column = column.min(screen.size().1 - 1);
                let private = if prefix.is_some() { "?" } else { "" };
                self.reply(format!("\x1b[{private}{};{}R", row + 1, column + 1).into_bytes());
            }
            (None, 't', 18) => {
                let (rows, columns) = screen.size();
                self.reply(format!("\x1b[8;{rows};{columns}t").into_bytes());
            }
            _ => {}
        }
    }
    // OSC 52 is intentionally left as the trait's no-op: no clipboard access.
}

pub struct TerminalModel {
    parser: vt100::Parser<Callbacks>,
    osc_length: Option<usize>,
    escape_pending: bool,
}

impl TerminalModel {
    pub fn new(size: TerminalSize) -> Self {
        let size = size.normalized();
        Self {
            parser: vt100::Parser::new_with_callbacks(
                size.rows,
                size.columns,
                SCROLLBACK_LINES,
                Callbacks::default(),
            ),
            osc_length: None,
            escape_pending: false,
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        // vte's std feature grows its OSC payload without a bound. Retain at most 8 KiB
        // per OSC while still forwarding the terminator and all ordinary output.
        // Never decode chunks independently: vte retains incomplete UTF-8 and VT sequences.
        let mut start = 0;
        for (index, &byte) in bytes.iter().enumerate() {
            let keep = if let Some(length) = self.osc_length {
                if matches!(byte, 7 | 24 | 26 | 27) {
                    self.osc_length = None;
                    self.escape_pending = byte == 27;
                    true
                } else {
                    self.osc_length = Some((length + 1).min(8_193));
                    length < 8_192
                }
            } else {
                if self.escape_pending && byte == b']' {
                    self.osc_length = Some(0);
                }
                if byte == 27 {
                    self.escape_pending = true;
                } else if (32..=126).contains(&byte) || matches!(byte, 24 | 26) {
                    self.escape_pending = false;
                }
                true
            };
            if !keep {
                if start < index {
                    self.parser.process(&bytes[start..index]);
                }
                start = index + 1;
            }
        }
        if start < bytes.len() {
            self.parser.process(&bytes[start..]);
        }
    }

    pub fn resize(&mut self, size: TerminalSize) {
        let size = size.normalized();
        self.parser.screen_mut().set_size(size.rows, size.columns);
    }

    pub fn modes(&self) -> InputModes {
        let screen = self.parser.screen();
        InputModes {
            application_cursor: screen.application_cursor(),
            application_keypad: screen.application_keypad(),
            bracketed_paste: screen.bracketed_paste(),
        }
    }

    pub fn take_replies(&mut self) -> Result<Vec<Vec<u8>>, &'static str> {
        let callbacks = self.parser.callbacks_mut();
        if std::mem::take(&mut callbacks.overflow) {
            callbacks.replies.clear();
            return Err("Terminal reply queue overflow");
        }
        Ok(callbacks.replies.drain(..).collect())
    }

    pub fn snapshot(
        &mut self,
        session_id: SessionId,
        kind: SessionKind,
        generation: u64,
        status: SessionStatus,
        scrollback_offset: usize,
    ) -> Snapshot {
        let modes = self.modes();
        let callbacks = self.parser.callbacks();
        let title = callbacks.title.clone();
        let bell_count = callbacks.bells;
        let screen = self.parser.screen_mut();
        let (rows, columns) = screen.size();
        let (cursor_row, cursor_column) = screen.cursor_position();
        screen.set_scrollback(SCROLLBACK_LINES);
        let scrollback_available = screen.scrollback();
        screen.set_scrollback(scrollback_offset);
        let scrollback_offset = screen.scrollback();
        let snapshot = Snapshot {
            session_id,
            kind,
            generation,
            status,
            size: TerminalSize { rows, columns },
            rows: (0..rows)
                .map(|row| Row {
                    wrapped: screen.row_wrapped(row),
                    cells: (0..columns)
                        .map(|column| {
                            // Historical rows retain their old width after a resize in vt100.
                            // Pad missing cells so every published viewport remains rectangular.
                            screen.cell(row, column).map_or_else(
                                || Cell {
                                    text: String::new(),
                                    foreground: Color::Default,
                                    background: Color::Default,
                                    bold: false,
                                    dim: false,
                                    italic: false,
                                    underline: false,
                                    inverse: false,
                                    wide: false,
                                    wide_continuation: false,
                                },
                                |cell| Cell {
                                    text: cell.contents().to_owned(),
                                    foreground: cell.fgcolor(),
                                    background: cell.bgcolor(),
                                    bold: cell.bold(),
                                    dim: cell.dim(),
                                    italic: cell.italic(),
                                    underline: cell.underline(),
                                    inverse: cell.inverse(),
                                    wide: cell.is_wide(),
                                    wide_continuation: cell.is_wide_continuation(),
                                },
                            )
                        })
                        .collect(),
                })
                .collect(),
            cursor: Cursor {
                row: cursor_row,
                column: cursor_column.min(columns - 1),
                visible: !screen.hide_cursor() && scrollback_offset == 0,
            },
            modes,
            alternate_screen: screen.alternate_screen(),
            scrollback_offset,
            scrollback_available,
            title,
            bell_count,
        };
        // Query replies and subsequent output always operate on the live screen.
        screen.set_scrollback(0);
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(model: &mut TerminalModel, offset: usize) -> Snapshot {
        model.snapshot(
            SessionId(1),
            SessionKind::Shell,
            1,
            SessionStatus::Running,
            offset,
        )
    }

    #[test]
    fn terminal_split_utf8_vt_attributes_cursor_and_wide_cells() {
        let bytes = "\x1b[31;1m界e\u{301}\x1b[0m\x1b[2;3H!".as_bytes();
        for split in 0..=bytes.len() {
            let mut model = TerminalModel::new(TerminalSize::new(3, 10).unwrap());
            model.process(&bytes[..split]);
            model.process(&bytes[split..]);
            let snap = snapshot(&mut model, 0);
            assert_eq!(snap.rows[0].cells[0].text, "界");
            assert!(snap.rows[0].cells[0].wide);
            assert!(snap.rows[0].cells[1].wide_continuation);
            assert_eq!(snap.rows[0].cells[2].text, "e\u{301}");
            assert_eq!(snap.rows[0].cells[0].foreground, Color::Idx(1));
            assert!(snap.rows[0].cells[0].bold);
            assert_eq!((snap.cursor.row, snap.cursor.column), (1, 3));
            assert_eq!(snap.selected_text((0, 0), (0, 3)), "界e\u{301}");
        }
    }

    #[test]
    fn terminal_scrollback_bound_and_alternate_screen() {
        let mut model = TerminalModel::new(TerminalSize::new(3, 12).unwrap());
        for _ in 0..6_100 {
            model.process(b"row\r\n");
        }
        let snap = snapshot(&mut model, usize::MAX);
        assert_eq!(snap.scrollback_available, SCROLLBACK_LINES);
        assert_eq!(snap.scrollback_offset, SCROLLBACK_LINES);
        assert!(!snap.cursor.visible);
        model.process(b"\x1b[?1049h\x1b[?1h\x1b[?2004hALT");
        let snap = snapshot(&mut model, 0);
        assert!(snap.alternate_screen);
        assert!(snap.modes.application_cursor && snap.modes.bracketed_paste);
        assert_eq!(snap.scrollback_available, 0);
        model.process(b"\x1b[?1049l");
        assert_eq!(
            snapshot(&mut model, 0).scrollback_available,
            SCROLLBACK_LINES
        );
    }

    #[test]
    fn terminal_osc_payload_is_bounded_and_history_stays_rectangular() {
        let mut model = TerminalModel::new(TerminalSize::new(2, 5).unwrap());
        model.process(b"\x1b\x7f]2;");
        for _ in 0..100 {
            model.process(&[b'x'; 1024]);
        }
        assert_eq!(model.osc_length, Some(8_193));
        model.process(b"\x07row\r\nrow\r\nrow\r\n");
        assert_eq!(model.osc_length, None);
        model.resize(TerminalSize::new(3, 20).unwrap());
        let snap = snapshot(&mut model, 10);
        assert_eq!(snap.title.len(), 256);
        assert!(snap.rows.iter().all(|row| row.cells.len() == 20));
        assert!(snap.text().contains("row"));
    }

    #[test]
    fn terminal_replies_use_live_cursor_and_ignore_clipboard() {
        let mut model = TerminalModel::new(TerminalSize::new(4, 20).unwrap());
        model.process(b"\x1b[2;3H\x1b[6");
        model.process(b"n\x1b[5n\x1b[c\x1b[>c\x1b[?6n\x1b[18t\x1b]52;c;?\x07");
        let replies = model.take_replies().unwrap();
        assert_eq!(
            replies,
            [
                b"\x1b[2;3R".to_vec(),
                b"\x1b[0n".to_vec(),
                b"\x1b[?1;2c".to_vec(),
                b"\x1b[>0;1;0c".to_vec(),
                b"\x1b[?2;3R".to_vec(),
                b"\x1b[8;4;20t".to_vec()
            ]
        );
        model.process(b"\x1b[2J\x1b[H");
        assert_eq!(snapshot(&mut model, 0).text().trim(), "");
    }
}
