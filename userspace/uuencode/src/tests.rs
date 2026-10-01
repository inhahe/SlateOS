//! Unit tests for the encoders. The program as a whole -- options, messages,
//! modes, failures -- is compared against sharutils by `scripts/uu-diff.sh`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::{enc, encode_block};

fn uu(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    encode_block(&mut out, data);
    out
}

fn b64(data: &[u8]) -> Vec<u8> {
    gnubase64::encode(data)
}

#[test]
fn a_full_group_is_four_characters() {
    assert_eq!(uu(b"Cat"), b"0V%T");
}

#[test]
fn a_short_group_is_padded_with_zero_bits_as_backquotes() {
    assert_eq!(uu(b"C"), b"0P``");
    assert_eq!(uu(b"Ca"), b"0V$`");
    assert_eq!(uu(b""), b"");
}

#[test]
fn zero_is_a_backquote_not_a_space() {
    assert_eq!(enc(0), b'`');
    assert_eq!(uu(&[0, 0, 0]), b"````");
    assert_eq!(enc(45), b'M');
}

#[test]
fn every_character_is_printable_and_never_a_space() {
    let data: Vec<u8> = (0..=255).collect();
    for c in uu(&data) {
        assert!((0x21..=0x60).contains(&c), "{c:#x}");
    }
}

#[test]
fn base64_pads_with_equals() {
    assert_eq!(b64(b""), b"");
    assert_eq!(b64(b"f"), b"Zg==");
    assert_eq!(b64(b"fo"), b"Zm8=");
    assert_eq!(b64(b"foo"), b"Zm9v");
    assert_eq!(b64(b"foobar"), b"Zm9vYmFy");
    assert_eq!(b64(&[0xff, 0xfe]), b"//4=");
}

#[test]
fn a_full_line_is_sixty_characters() {
    assert_eq!(uu(&[7u8; 45]).len(), 60);
    assert_eq!(b64(&[7u8; 45]).len(), 60);
}
