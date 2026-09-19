#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InputModes {
    pub application_cursor: bool,
    pub application_keypad: bool,
    pub bracketed_paste: bool,
}

impl InputModes {
    pub(super) fn bits(self) -> u8 {
        u8::from(self.application_cursor)
            | (u8::from(self.application_keypad) << 1)
            | (u8::from(self.bracketed_paste) << 2)
    }

    pub(super) fn from_bits(bits: u8) -> Self {
        Self {
            application_cursor: bits & 1 != 0,
            application_keypad: bits & 2 != 0,
            bracketed_paste: bits & 4 != 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Insert,
    Delete,
    PageUp,
    PageDown,
    Function(u8),
}

pub fn encode_key(key: Key, modifiers: Modifiers, modes: InputModes) -> Vec<u8> {
    let modifier = 1
        + u8::from(modifiers.shift)
        + 2 * u8::from(modifiers.alt)
        + 4 * u8::from(modifiers.control);
    let final_byte = match key {
        Key::Up => Some('A'),
        Key::Down => Some('B'),
        Key::Right => Some('C'),
        Key::Left => Some('D'),
        Key::Home => Some('H'),
        Key::End => Some('F'),
        _ => None,
    };
    if let Some(final_byte) = final_byte {
        return if modifier != 1 {
            format!("\x1b[1;{modifier}{final_byte}").into_bytes()
        } else if modes.application_cursor {
            format!("\x1bO{final_byte}").into_bytes()
        } else {
            format!("\x1b[{final_byte}").into_bytes()
        };
    }
    if let Key::Function(number @ 1..=4) = key {
        let final_byte = char::from(b'P' + number - 1);
        return if modifier == 1 {
            format!("\x1bO{final_byte}").into_bytes()
        } else {
            format!("\x1b[1;{modifier}{final_byte}").into_bytes()
        };
    }
    let tilde = match key {
        Key::Insert => Some(2),
        Key::Delete => Some(3),
        Key::PageUp => Some(5),
        Key::PageDown => Some(6),
        Key::Function(5) => Some(15),
        Key::Function(6) => Some(17),
        Key::Function(7) => Some(18),
        Key::Function(8) => Some(19),
        Key::Function(9) => Some(20),
        Key::Function(10) => Some(21),
        Key::Function(11) => Some(23),
        Key::Function(12) => Some(24),
        _ => None,
    };
    if let Some(number) = tilde {
        return if modifier == 1 {
            format!("\x1b[{number}~").into_bytes()
        } else {
            format!("\x1b[{number};{modifier}~").into_bytes()
        };
    }
    let mut bytes = match key {
        Key::Char(c) if modifiers.control => {
            let control = match c.to_ascii_uppercase() {
                'A'..='Z' => Some(c.to_ascii_uppercase() as u8 - b'A' + 1),
                '@' | ' ' | '2' => Some(0),
                '[' | '3' => Some(27),
                '\\' | '4' => Some(28),
                ']' | '5' => Some(29),
                '^' | '6' => Some(30),
                '_' | '/' | '7' => Some(31),
                '?' | '8' => Some(127),
                _ => None,
            };
            control.map_or_else(|| c.to_string().into_bytes(), |value| vec![value])
        }
        Key::Char(c) => c.to_string().into_bytes(),
        Key::Enter => vec![b'\r'],
        Key::Tab if modifiers.shift => b"\x1b[Z".to_vec(),
        Key::Tab => vec![b'\t'],
        Key::Backspace if modifiers.control => vec![8],
        Key::Backspace => vec![127],
        Key::Escape => vec![27],
        _ => Vec::new(),
    };
    if modifiers.alt && !bytes.is_empty() {
        bytes.insert(0, 27);
    }
    bytes
}

// Call only for an explicit user paste, never in response to an OSC clipboard query.
pub fn encode_paste(text: &str, modes: InputModes) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len().saturating_add(12));
    if modes.bracketed_paste {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    let mut previous_cr = false;
    for c in text.chars() {
        match c {
            '\r' => bytes.push(b'\r'),
            '\n' if !previous_cr => bytes.push(b'\r'),
            '\n' => {}
            '\t' => bytes.push(b'\t'),
            // Strip embedded ESC/control sequences, including a forged paste terminator.
            c if c.is_control() => {}
            c => bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
        }
        previous_cr = c == '\r';
    }
    if modes.bracketed_paste {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_input_controls_and_utf8() {
        let ctrl = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        assert_eq!(encode_key(Key::Char('c'), ctrl, InputModes::default()), [3]);
        assert_eq!(encode_key(Key::Char('d'), ctrl, InputModes::default()), [4]);
        assert_eq!(
            encode_key(Key::Char('z'), ctrl, InputModes::default()),
            [26]
        );
        assert_eq!(encode_key(Key::Char(' '), ctrl, InputModes::default()), [0]);
        assert_eq!(
            encode_key(Key::Enter, Modifiers::default(), InputModes::default()),
            b"\r"
        );
        assert_eq!(
            encode_key(Key::Char('界'), Modifiers::default(), InputModes::default()),
            "界".as_bytes()
        );
        assert_eq!(
            encode_key(Key::Left, ctrl, InputModes::default()),
            b"\x1b[1;5D"
        );
    }

    #[test]
    fn terminal_input_modes_and_paste() {
        let modes = InputModes {
            application_cursor: true,
            bracketed_paste: true,
            ..InputModes::default()
        };
        assert_eq!(encode_key(Key::Up, Modifiers::default(), modes), b"\x1bOA");
        assert_eq!(
            encode_key(Key::Function(12), Modifiers::default(), modes),
            b"\x1b[24~"
        );
        assert_eq!(
            encode_paste("hé\r\n界\n", modes),
            "\x1b[200~hé\r界\r\x1b[201~".as_bytes()
        );
        assert_eq!(
            encode_paste("a\x1b[201~\0b", modes),
            b"\x1b[200~a[201~b\x1b[201~"
        );
    }
}
