#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "test code, on values the tests choose"
)]

use super::*;

/// `tools/softfloat_reference.c`'s generator: the same inputs, in the
/// same order.
struct Draws(u64);

impl Draws {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 32) as u32
    }

    fn draw(&mut self) -> SoftFloat {
        let v = self.next() as i32;
        let frac = (self.next() % 61) as i32;
        int2sf(v, frac)
    }
}

const FNV_BASIS: u64 = 0xcbf29ce484222325;

fn fnv(mut h: u64, v: i32) -> u64 {
    for b in v.to_le_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn fnv_sf(h: u64, v: SoftFloat) -> u64 {
    fnv(fnv(h, v.mant), v.exp)
}

#[test]
fn every_function_gives_libavutils_results() {
    let mut d = Draws(0x2545f4914f6cdd1d);
    let mut h = [FNV_BASIS; 11];
    let edges = [
        0,
        1,
        -1,
        2,
        -2,
        i32::MAX,
        i32::MIN,
        i32::MIN + 1,
        0x20000000,
        -0x20000000,
        0x3fffffff,
        -0x40000000,
    ];
    for v in edges {
        for f in (0..64).step_by(7) {
            h[0] = fnv_sf(h[0], int2sf(v, f));
        }
    }
    for _ in 0..200_000 {
        let a = d.draw();
        let b = d.draw();
        h[0] = fnv_sf(h[0], a);
        h[1] = fnv_sf(h[1], mul_sf(a, b));
        h[2] = fnv_sf(h[2], add_sf(a, b));
        h[3] = fnv_sf(h[3], sub_sf(a, b));
        if b.mant != 0 {
            h[4] = fnv_sf(h[4], div_sf(a, b));
        }
        h[5] = fnv(h[5], i32::from(gt_sf(a, b)));
        h[6] = fnv(h[6], cmp_sf(a, b));
        if a.mant >= 0 {
            h[7] = fnv_sf(h[7], sqrt_sf(a));
        }
        let (s, c) = sincos_sf(d.next() as i32);
        h[8] = fnv(fnv(h[8], s), c);
        let m = d.next() as i32;
        let shift = d.next() % 31;
        let e = (d.next() % 300) as i32 - 150;
        let raw = sf(m >> shift, e);
        h[9] = fnv_sf(h[9], normalize_sf(normalize1_sf(raw)));
        // A divisor smaller than normalized: a quotient too big for 32 bits,
        // halved till it fits.
        if raw.mant != 0 {
            h[10] = fnv_sf(h[10], div_sf(a, raw));
        }
    }
    // What the reference prints, compiled against FFmpeg 8.1.3's
    // softfloat.h.
    let expected: [u64; 11] = [
        0xdddb793fef886bf2,
        0xce4cd930acbe5ba9,
        0x4bc1727e15f008e9,
        0x4529614365ab8ab6,
        0xefc10fe910f68baf,
        0x0d7fa4328e63b995,
        0x8902ea267470da15,
        0xe0e674d1236a9de3,
        0x2d9947fa00a48c18,
        0x1542785516c73273,
        0x86f5a94b2a948ae4,
    ];
    let names = [
        "int2sf", "mul", "add", "sub", "div", "gt", "cmp", "sqrt", "sincos", "norm", "divraw",
    ];
    for i in 0..11 {
        assert_eq!(h[i], expected[i], "{}", names[i]);
    }
}

#[test]
fn values_are_what_they_say() {
    assert_eq!(int2sf(1, 0), FLOAT_1);
    assert_eq!(int2sf(1, 1), FLOAT_05);
    assert_eq!(mul_sf(FLOAT_1, FLOAT_05), FLOAT_05);
    assert_eq!(div_sf(FLOAT_1, FLOAT_05), int2sf(2, 0));
    assert_eq!(sqrt_sf(int2sf(4, 0)), int2sf(2, 0));
    assert_eq!(add_sf(FLOAT_05, FLOAT_05), FLOAT_1);
    assert_eq!(sub_sf(FLOAT_1, FLOAT_1), FLOAT_0);
    assert!(gt_sf(FLOAT_1, FLOAT_05));
    assert!(!gt_sf(FLOAT_05, FLOAT_1));
    assert_eq!(sincos_sf(0), (0, 1 << 30));
    // A quarter turn is 2^29: sine 1, cosine 0, to within rounding.
    let (s, c) = sincos_sf(1 << 29);
    assert!((s - (1 << 30)).abs() <= 1 && c.abs() <= 1, "{s} {c}");
}

#[test]
fn what_ffmpeg_would_crash_on_answers_zero() {
    assert_eq!(div_sf(FLOAT_1, SoftFloat::default()), FLOAT_0);
    assert_eq!(sqrt_sf(sf(-0x2000_0000, 1)), FLOAT_0);
    // A zero mantissa is zero with the smallest exponent.
    assert_eq!(sqrt_sf(sf(0, 7)), FLOAT_0);
}
