#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use super::*;

/// Bytes from a xorshift generator: the same every run.
fn noise(seed: u64, n: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

/// Every bin a mixed run of reads gives: context bins of several states,
/// bypass bins and signs.
fn bins(buf: &[u8], origin: usize) -> (Vec<i32>, Cabac) {
    let mut c = Cabac::default();
    let mut states = [0u8, 20, 62, 63, 80, 124, 125, 7];
    let mut out = Vec::new();
    if c.init(buf, origin, 0, buf.len() as isize).is_err() {
        return (out, c);
    }
    for k in 0..3000usize {
        let v = match k % 7 {
            0..=3 => c.get(buf, &mut states[k % states.len()]),
            4 | 5 => c.bypass(buf),
            _ => c.bypass_sign(buf, k as i32),
        };
        out.push(v);
    }
    (out, c)
}

/// `ff_init_cabac_decoder` reads a third byte only when the first two
/// leave its pointer at an odd address. Whichever it does, every bin after
/// must be the same -- FFmpeg relies on it -- and the two positions may be
/// no more than the byte apart.
#[test]
fn both_starts_read_the_same_bins() {
    let mut compared = 0;
    for seed in 0..200 {
        let buf = noise(seed, 4096);
        let (even, c_even) = bins(&buf, 0);
        let (odd, c_odd) = bins(&buf, 1);
        assert_eq!(even, odd, "seed {seed}");
        if !even.is_empty() {
            compared += 1;
            assert!((c_even.pos - c_odd.pos).abs() <= 1, "seed {seed}");
        }
    }
    assert!(compared > 100, "too few streams started: {compared}");
}

/// The third byte is read exactly when the address after two is odd.
#[test]
fn the_start_reads_a_third_byte_at_an_odd_address() {
    let buf = [0x12, 0x34, 0x56, 0x78];
    let mut c = Cabac::default();
    c.init(&buf, 0, 0, 4).unwrap();
    assert_eq!(c.pos, 2);
    assert_eq!(c.low, (0x12 << 18) + (0x34 << 10) + (1 << 9));
    c.init(&buf, 1, 0, 4).unwrap();
    assert_eq!(c.pos, 3);
    assert_eq!(c.low, (0x12 << 18) + (0x34 << 10) + (0x56 << 2) + 2);
    assert_eq!(c.range, 0x1FE);
    // A start whose first bits already lie past the range is refused.
    assert!(c.init(&[0xFF, 0xFF, 0xFF], 0, 0, 3).is_err());
}

/// The four tables are one array, and an index past it reads zero.
#[test]
fn the_tables_end_to_end() {
    assert_eq!(table(0), 9, "norm_shift[0]");
    assert_eq!(table(NORM_SHIFT + 511), 0);
    assert_eq!(table(LPS_RANGE), 128, "-128 as a byte");
    assert_eq!(table(LAST_COEFF_FLAG_OFFSET_8X8 + 62), 8);
    assert_eq!(table(1343), 0);
    assert_eq!(table(-1), 0);
}
