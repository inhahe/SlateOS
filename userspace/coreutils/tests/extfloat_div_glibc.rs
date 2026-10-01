//! [`coreutils::extfloat::ExtF80`] division against glibc's x87 `long double`,
//! case for case.
//!
//! The expectations were not computed by this crate or written by hand: they
//! were *measured*. `scripts/extfloat-div-probe.c` divides 757 pairs of x87
//! values on real hardware -- a cross product of values chosen for their
//! edges (exact quotients, repeating ones, the largest and smallest normals,
//! the smallest subnormal, `INT64_MAX` and `UINT64_MAX` as they round, signed
//! zeros, division by zero) and 400 pseudo-random pairs whose exponents reach
//! the subnormal floor and the overflow ceiling -- and records the ten bytes of
//! each operand and quotient. This test replays the recording.
//!
//! It is a recording rather than a live comparison for the reason
//! `fnmatch_glibc.rs` gives: the development host is Windows, where the tests
//! run, and its x87 control word is set to 53 bits, so the hardware beside the
//! test would not give the answer the target gives.

use coreutils::extfloat::ExtF80;

fn bytes(hex: &str) -> [u8; 10] {
    let mut out = [0u8; 10];
    for (i, slot) in out.iter_mut().enumerate() {
        let pair = hex.get(i * 2..i * 2 + 2).expect("twenty hex digits");
        *slot = u8::from_str_radix(pair, 16).expect("hex");
    }
    out
}

#[test]
fn division_matches_the_x87_bit_for_bit() {
    let recording = include_str!("extfloat-div-glibc.txt");
    let mut cases = 0;
    let mut wrong = Vec::new();
    for line in recording.lines() {
        let mut fields = line.split(' ');
        let (Some(a), Some(b), Some(q)) = (fields.next(), fields.next(), fields.next()) else {
            panic!("malformed line: {line}");
        };
        let a = ExtF80::from_x87_bytes(bytes(a));
        let b = ExtF80::from_x87_bytes(bytes(b));
        let expected = ExtF80::from_x87_bytes(bytes(q));
        let got = a / b;
        let agrees = if expected.is_nan() {
            got.is_nan()
        } else {
            got.to_x87_bytes() == expected.to_x87_bytes()
        };
        if !agrees {
            wrong.push(format!("{line}  got {:02x?}", got.to_x87_bytes()));
        }
        cases += 1;
    }
    assert_eq!(cases, 757, "the recording has lost or gained lines");
    assert!(
        wrong.is_empty(),
        "{} of {cases} differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn the_byte_form_round_trips() {
    for line in include_str!("extfloat-div-glibc.txt").lines().take(50) {
        for hex in line.split(' ') {
            let v = ExtF80::from_x87_bytes(bytes(hex));
            if !v.is_nan() {
                assert_eq!(v.to_x87_bytes(), bytes(hex), "{hex}");
            }
        }
    }
}

#[test]
fn integers_convert_exactly_and_truncate_as_c_casts_do() {
    for v in [
        0i64,
        1,
        -1,
        1000,
        -1024,
        i64::MAX,
        i64::MIN,
        1 << 62,
        -(1 << 40) - 7,
    ] {
        assert_eq!(ExtF80::from_i64(v).to_i64_trunc(), v, "{v}");
    }
    assert_eq!(
        ExtF80::from_u64(u64::MAX).to_i64_trunc(),
        i64::MIN,
        "out of range"
    );
    let seven = ExtF80::from_i64(7);
    let two = ExtF80::from_i64(2);
    assert_eq!((seven / two).to_i64_trunc(), 3);
    assert_eq!((-seven / two).to_i64_trunc(), -3);
    assert_eq!((ExtF80::ONE / ExtF80::from_i64(3)).to_i64_trunc(), 0);
    assert_eq!(ExtF80::NAN.to_i64_trunc(), i64::MIN);
    assert_eq!(ExtF80::INFINITY.to_i64_trunc(), i64::MIN);
    assert_eq!((seven - two).to_i64_trunc(), 5);
    assert_eq!((two - seven).to_i64_trunc(), -5);
}
