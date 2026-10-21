#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use super::*;

/// Bytes from a fixed seed: every pattern of bits, but the same each run.
fn noise(len: usize) -> Vec<u8> {
    let mut x: u32 = 0x9E37_79B9;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 24) as u8
        })
        .collect()
}

/// Bit `at` of `buf`, most significant first, zero past the end: the
/// reader's answer worked out one bit at a time, for comparison.
fn bit_at(buf: &[u8], at: usize) -> u32 {
    buf.get(at / 8)
        .map_or(0, |b| u32::from(b >> (7 - at % 8)) & 1)
}

fn value_at(buf: &[u8], at: usize, n: u32) -> u32 {
    (0..n as usize).fold(0, |v, k| (v << 1) | bit_at(buf, at + k))
}

/// A writer of bits, most significant first, for codes to read back.
#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    len: usize,
}

impl Writer {
    fn put(&mut self, v: u64, n: u32) {
        for k in (0..n).rev() {
            if self.len.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (v >> k) & 1 != 0 {
                *self.bytes.last_mut().unwrap() |= 0x80 >> (self.len % 8);
            }
            self.len += 1;
        }
    }

    /// An unsigned Exp-Golomb code: as many zeros as the value plus one has
    /// bits after its first, then the value plus one.
    fn ue(&mut self, v: u64) {
        let x = v + 1;
        let bits = 64 - x.leading_zeros();
        self.put(0, bits - 1);
        self.put(x, bits);
    }

    fn se(&mut self, v: i64) {
        self.ue(if v > 0 {
            2 * v as u64 - 1
        } else {
            (-2 * v) as u64
        });
    }
}

#[test]
fn reads_agree_with_a_reader_taking_one_bit_at_a_time() {
    let buf = noise(24);
    for start in 0..96 {
        for n in 1..=32 {
            let mut b = Bits::whole(&buf).unwrap();
            b.skip_long(start);
            let want = value_at(&buf, start as usize, n);
            assert_eq!(b.show_long(n), want, "show_long({n}) at {start}");
            if n <= 25 {
                assert_eq!(b.show(n), want, "show({n}) at {start}");
                let mut c = b.clone();
                assert_eq!(c.bits(n), want, "bits({n}) at {start}");
                assert_eq!(c.count(), start as u32 + n);
                // The same bits, the top one the sign.
                let signed = ((want << (32 - n)) as i32) >> (32 - n);
                let mut c = b.clone();
                assert_eq!(c.sbits(n), signed, "sbits({n}) at {start}");
                assert_eq!(c.count(), start as u32 + n);
            }
            assert_eq!(b.bits_long(n), want, "bits_long({n}) at {start}");
            assert_eq!(b.count(), start as u32 + n);
        }
        let mut b = Bits::whole(&buf).unwrap();
        b.skip_long(start);
        assert_eq!(b.clone().bits_long(0), 0);
        assert_eq!(b.bit(), bit_at(&buf, start as usize));
        assert_eq!(b.count(), start as u32 + 1);
    }
}

#[test]
fn exp_golomb_codes_read_back_at_every_alignment() {
    for offset in 0..8u32 {
        let mut w = Writer::default();
        w.put(0, offset);
        for v in (0..300).chain([1000, 4095, 4096, 8189, 8190]) {
            w.ue(v);
        }
        for v in (-300..=300).chain([-4095, 4095]) {
            w.se(v);
        }
        for v in [0u64, 1, 8190, 8191, 65535, 1 << 20, u64::from(u32::MAX) - 1] {
            w.ue(v);
        }
        for v in [0i64, 1, -1, 4096, -4096, 1 << 20, -(1 << 20)] {
            w.se(v);
        }
        let mut b = Bits::whole(&w.bytes).unwrap();
        b.skip(offset);
        for v in (0..300).chain([1000, 4095, 4096, 8189, 8190]) {
            assert_eq!(b.ue(), v, "ue at offset {offset}");
        }
        for v in (-300..=300).chain([-4095, 4095]) {
            assert_eq!(b.se(), v, "se at offset {offset}");
        }
        for v in [0u32, 1, 8190, 8191, 65535, 1 << 20, u32::MAX - 1] {
            assert_eq!(b.ue_long(), v, "ue_long at offset {offset}");
        }
        for v in [0i32, 1, -1, 4096, -4096, 1 << 20, -(1 << 20)] {
            assert_eq!(b.se_long(), v, "se_long at offset {offset}");
        }
        assert_eq!(b.count() as usize, w.len);
    }
}

/// Past twelve leading zeros `get_ue_golomb` gives up: invalid data, and
/// the position moved on by `32 - log` -- 63 bits for nothing but zeros. A
/// caller reads on from there, so both must be FFmpeg's.
#[test]
fn a_code_too_long_for_ue_is_invalid_and_skipped_as_ffmpeg_skips_it() {
    let mut w = Writer::default();
    w.ue(8191); // thirteen leading zeros
    let mut b = Bits::whole(&w.bytes).unwrap();
    assert_eq!(b.ue(), AVERROR_INVALIDDATA);
    // av_log2 of the cache is 31 - 13 = 18, so log is 5: 27 bits.
    assert_eq!(b.count(), 27);

    let zeros = [0u8; 16];
    let mut b = Bits::whole(&zeros).unwrap();
    assert_eq!(b.ue(), AVERROR_INVALIDDATA);
    assert_eq!(b.count(), 63);
    // The signed code reads 32 zeros as a value, as get_se_golomb does.
    let mut b = Bits::whole(&zeros).unwrap();
    b.se();
    assert_eq!(b.count(), 63);
}

/// `get_ue_golomb_31` reads nine bits through the table whatever comes
/// after: a value under 31 as itself, a longer code as 31 or more and the
/// length its leading zeros say.
#[test]
fn ue_31_takes_a_longer_code_as_out_of_range() {
    for v in 0..31 {
        let mut w = Writer::default();
        w.ue(v);
        let mut b = Bits::whole(&w.bytes).unwrap();
        assert_eq!(u64::from(b.ue_31()), v);
        assert_eq!(b.count() as usize, w.len);
    }
    for (v, len) in [(31u64, 11u32), (40, 11), (62, 11), (63, 13), (200, 15)] {
        let mut w = Writer::default();
        w.ue(v);
        let mut b = Bits::whole(&w.bytes).unwrap();
        assert!(b.ue_31() >= 31, "{v}");
        assert_eq!(b.count(), len, "{v}");
    }
}

/// A read past the end goes on into the bytes after the syntax, then into
/// zeros. The checked reader stops counting eight bits past the end; the
/// unchecked one does not.
#[test]
fn reading_past_the_end() {
    // Twelve bits of syntax, then an RBSP stop bit and zeros.
    let buf = [0xFF, 0xF8];
    let mut checked = Bits::new(&buf, 12).unwrap();
    checked.skip(12);
    assert_eq!(checked.left(), 0);
    assert_eq!(checked.bits(1), 1, "the stop bit, past the syntax");
    assert_eq!(checked.bits(20), 0, "then zeros");
    assert_eq!(
        checked.count(),
        12 + 8,
        "no further than eight past the end"
    );
    assert_eq!(checked.left(), -8);
    assert_eq!(checked.bit(), 0);
    assert_eq!(checked.count(), 20);

    let mut unchecked = Bits::new(&buf, 12).unwrap();
    unchecked.set_unchecked(true);
    unchecked.skip(12);
    assert_eq!(unchecked.bits(21), 1 << 20);
    assert_eq!(unchecked.count(), 33);
    assert_eq!(unchecked.left(), -21);
    unchecked.bits_long(32);
    assert_eq!(unchecked.left(), -53, "it counts on past the end");
    assert_eq!(unchecked.bit(), 0);
    assert_eq!(unchecked.left(), -54);
}

#[test]
fn skipping_and_aligning() {
    let buf = noise(4);
    let mut b = Bits::whole(&buf).unwrap();
    b.skip_long(-5);
    assert_eq!(b.count(), 0, "not before the start");
    b.skip_long(1000);
    assert_eq!(b.count(), 32 + 8, "not past eight beyond the end");
    b.skip_long(-37);
    assert_eq!(b.count(), 3);
    assert_eq!(b.align(), 1);
    assert_eq!(b.count(), 8);
    assert_eq!(b.align(), 1, "an aligned position stays");
    assert_eq!(b.byte_size(false), 4);
    assert_eq!(Bits::new(&buf, 13).unwrap().byte_size(true), 2);
    assert_eq!(Bits::new(&buf, 13).unwrap().byte_size(false), 1);
    assert!(
        Bits::new(&buf, MAX_BITS).is_none(),
        "too many bits for an int"
    );
}

#[test]
fn log2() {
    assert_eq!(av_log2(0), 0);
    assert_eq!(av_log2(1), 0);
    assert_eq!(av_log2(0x8000_0000), 31);
    assert_eq!(av_log2(0x0001_0001), 16);
}
