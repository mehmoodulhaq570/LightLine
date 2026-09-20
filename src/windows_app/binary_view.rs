//! Read-only hex-dump preview for files that are neither valid UTF-8 text nor
//! a recognized image — instead of the hard "Only UTF-8 files are supported"
//! error, show the bytes the way a hex editor would.

use std::fmt::Write as _;

// Large binary files are common (executables, archives); dumping the whole
// thing would make the tab sluggish to open and scroll for little benefit,
// since a hex preview is for inspection, not full analysis.
const PREVIEW_LIMIT: usize = 64 * 1024;

pub(super) fn hex_dump(bytes: &[u8]) -> String {
    let limited = &bytes[..bytes.len().min(PREVIEW_LIMIT)];
    let mut out = String::with_capacity(limited.len() * 4);
    for (row, chunk) in limited.chunks(16).enumerate() {
        let _ = write!(out, "{:08x}  ", row * 16);
        for (index, byte) in chunk.iter().enumerate() {
            if index == 8 {
                out.push(' ');
            }
            let _ = write!(out, "{byte:02x} ");
        }
        for index in chunk.len()..16 {
            if index == 8 {
                out.push(' ');
            }
            out.push_str("   ");
        }
        out.push_str(" |");
        for &byte in chunk {
            out.push(if (0x20..0x7f).contains(&byte) {
                byte as char
            } else {
                '.'
            });
        }
        out.push('|');
        out.push('\n');
    }
    if bytes.len() > PREVIEW_LIMIT {
        let _ = write!(
            out,
            "\n... {} more byte(s) not shown ...\n",
            bytes.len() - PREVIEW_LIMIT
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_offset_hex_columns_and_ascii_gutter() {
        let dump = hex_dump(b"Hi!\x00\x01\xff");
        let line = dump.lines().next().unwrap();
        assert!(line.starts_with("00000000  "));
        assert!(line.contains("48 69 21 00 01 ff"));
        assert!(line.ends_with("|Hi!...|"));
    }

    #[test]
    fn pads_a_short_final_row_to_align_the_ascii_gutter() {
        let short = hex_dump(b"A");
        let full = hex_dump(&[b'A'; 16]);
        let short_gutter = short.find('|').unwrap();
        let full_gutter = full.find('|').unwrap();
        assert_eq!(short_gutter, full_gutter);
    }

    #[test]
    fn truncates_past_the_preview_limit_and_says_so() {
        let dump = hex_dump(&vec![0u8; PREVIEW_LIMIT + 10]);
        assert!(dump.contains("10 more byte(s) not shown"));
    }
}
