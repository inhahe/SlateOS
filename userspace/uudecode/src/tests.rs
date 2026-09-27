//! Unit tests for the pieces: the header scan and `fgets`. gnulib's base64
//! decoder is `gnubase64`'s, tested there; the program as a whole is compared
//! against sharutils by `scripts/uu-diff.sh`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::fs;

use scratchdir::ScratchDir;

use super::lines::{Lines, Source};
use super::{dec, scan_header};

#[test]
fn dec_is_six_bits_of_the_offset_from_space() {
    assert_eq!(dec(b' '), 0);
    assert_eq!(dec(b'`'), 0);
    assert_eq!(dec(b'M'), 45);
    assert_eq!(dec(b'\n'), 42);
    assert_eq!(dec(0xe0), 0);
}

#[test]
fn the_header_is_an_octal_mode_and_the_rest_of_the_line() {
    assert_eq!(scan_header(b" 644 name\n"), Some((0o644, b"name".to_vec())));
    assert_eq!(
        scan_header(b" 0755\ttwo words \n"),
        Some((0o755, b"two words ".to_vec()))
    );
    // A minus sign negates, as strtoul does. (Built rather than written out,
    // so that `-1` is not read as an option by check-help-vs-parser.)
    let negative = [&b" "[..], b"-", b"1 x\n"].concat();
    assert_eq!(scan_header(&negative), Some((u32::MAX, b"x".to_vec())));
    // No name: the second conversion fails even across the newline.
    assert_eq!(scan_header(b" 644 \n"), None);
    assert_eq!(scan_header(b" 644\n"), None);
    assert_eq!(scan_header(b" x name\n"), None);
    assert_eq!(scan_header(b" 8 name\n"), None);
    // Digits past 7 end the number; the rest is the name.
    assert_eq!(
        scan_header(b" 678 name\n"),
        Some((0o67, b"8 name".to_vec()))
    );
}

#[test]
fn fgets_splits_long_lines_and_leaves_the_rest_of_the_buffer() {
    let dir = ScratchDir::new("uudecode-lines");
    let path = dir.path("in");
    fs::write(&path, b"abcdef\nxy\nlast").unwrap();
    let mut lines = Lines::new(Source::File(fs::File::open(&path).unwrap()));
    let mut buf = [0u8; 5];
    assert_eq!(lines.fgets(&mut buf), Some(4));
    assert_eq!(&buf, b"abcd\0");
    assert_eq!(lines.fgets(&mut buf), Some(3));
    assert_eq!(&buf, b"ef\n\0\0");
    assert_eq!(lines.fgets(&mut buf), Some(3));
    // "xy\n" and its NUL; the byte after it is still the earlier line's.
    assert_eq!(&buf, b"xy\n\0\0");
    assert_eq!(lines.fgets(&mut buf), Some(4));
    assert_eq!(&buf, b"last\0");
    assert_eq!(lines.fgets(&mut buf), None);
    assert_eq!(lines.fgets(&mut buf), None);
}
