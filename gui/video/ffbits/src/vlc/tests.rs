#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::bits::Bits;

/// The bits of `s` (`'0'` and `'1'`), padded to whole bytes with zeros.
fn bytes(s: &str) -> Vec<u8> {
    let mut out = vec![0u8; s.len().div_ceil(8) + 8];
    for (i, c) in s.chars().enumerate() {
        if c == '1' {
            out[i / 8] |= 0x80 >> (i % 8);
        }
    }
    out
}

#[test]
fn codes_short_and_long_read_back_with_their_lengths() {
    // 1, 01, 001, 0001 0, 0001 1, 0000 001: the last two past a 3-bit lookup.
    let lens = [1, 2, 3, 5, 5, 7];
    let codes = [1, 1, 1, 2, 3, 1];
    let vlc = Vlc::new(3, &lens, &codes, None).unwrap();
    let stream = "1".to_owned() + "01" + "001" + "00010" + "00011" + "0000001";
    let buf = bytes(&stream);
    let mut gb = Bits::whole(&buf).unwrap();
    let got: Vec<i32> = (0..6).map(|_| vlc.read(&mut gb, 3)).collect();
    assert_eq!(got, [0, 1, 2, 3, 4, 5]);
    assert_eq!(gb.count() as usize, stream.len());
}

#[test]
fn a_pattern_that_is_no_code_answers_minus_one_and_takes_the_lookup_bits_before_it() {
    // Codes 1 and 01 only: 00 is no code.
    let vlc = Vlc::new(2, &[1, 2], &[1, 1], None).unwrap();
    let buf = bytes("00");
    let mut gb = Bits::whole(&buf).unwrap();
    assert_eq!(vlc.read(&mut gb, 1), -1);
    assert_eq!(gb.count(), 0, "a first-level miss takes no bits");

    // 1, then 0000 1 past a 2-bit lookup: 01... is no code in the
    // subtable's range, after the two lookup bits are taken.
    let vlc = Vlc::new(2, &[1, 5], &[1, 1], None).unwrap();
    let buf = bytes("00010");
    let mut gb = Bits::whole(&buf).unwrap();
    assert_eq!(vlc.read(&mut gb, 2), -1);
    assert_eq!(gb.count(), 2);
}

#[test]
fn symbols_can_be_given_and_codes_that_collide_are_refused() {
    let vlc = Vlc::new(2, &[1, 2, 2], &[1, 1, 0], Some(&[7, 8, 9])).unwrap();
    let buf = bytes("10100");
    let mut gb = Bits::whole(&buf).unwrap();
    let got: Vec<i32> = (0..3).map(|_| vlc.read(&mut gb, 1)).collect();
    assert_eq!(got, [7, 8, 9]);
    assert_eq!(Vlc::new(2, &[1, 1], &[1, 1], None), Err(VlcError));
    assert_eq!(
        Vlc::new(2, &[2], &[4], None),
        Err(VlcError),
        "a code past its length"
    );
}

#[test]
fn codes_from_lengths_are_each_the_next_after_the_one_before() {
    // Lengths 1, 2, 3, 3 make 0, 10, 110, 111; symbols offset by -2.
    let vlc = Vlc::from_lengths(2, &[1, 2, 3, 3], None, -2).unwrap();
    let stream = "0".to_owned() + "10" + "110" + "111";
    let buf = bytes(&stream);
    let mut gb = Bits::whole(&buf).unwrap();
    let got: Vec<i32> = (0..4).map(|_| vlc.read(&mut gb, 2)).collect();
    assert_eq!(got, [-2, -1, 0, 1]);
    assert_eq!(gb.count() as usize, stream.len());

    // A negative length holds its code's place for no symbol: 0 is no
    // code, 10 is symbol 1's, 11 symbol 2's.
    let vlc = Vlc::from_lengths(2, &[-1, 2, 2], Some(&[5, 6, 7]), 0).unwrap();
    let buf = bytes("1011");
    let mut gb = Bits::whole(&buf).unwrap();
    assert_eq!([vlc.read(&mut gb, 1), vlc.read(&mut gb, 1)], [6, 7]);
    let buf = bytes("0");
    assert_eq!(vlc.read(&mut Bits::whole(&buf).unwrap(), 1), -1);
}

#[test]
fn lengths_that_are_no_prefix_code_are_refused() {
    assert_eq!(
        Vlc::from_lengths(2, &[1, 1, 1], None, 0),
        Err(VlcError),
        "more codes than room"
    );
    assert_eq!(
        Vlc::from_lengths(2, &[2, 1], None, 0),
        Err(VlcError),
        "a shorter code after a longer one starts inside it"
    );
    assert_eq!(
        Vlc::from_lengths(2, &[7], None, 0),
        Err(VlcError),
        "longer than three lookups"
    );
}
