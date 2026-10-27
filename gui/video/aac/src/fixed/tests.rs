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

/// `tools/fixed_reference.c`'s generator: the same inputs, in the same
/// order.
struct Draws(u64);

impl Draws {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 32) as u32
    }

    /// A word shifted down by a random count, so that every magnitude turns
    /// up.
    fn sample(&mut self) -> i32 {
        let v = self.next() as i32;
        let shift = self.next() % 32;
        v >> shift
    }

    /// A power of two of either sign, now and then one off it: products of
    /// these fall on Q31's rounding boundaries.
    fn power(&mut self) -> i32 {
        let exp = self.next() % 31;
        let mut v = (1u32 << exp) as i32;
        let r = self.next();
        if r & 1 != 0 {
            v = -v;
        }
        if r & 2 != 0 {
            v += if r & 4 != 0 { 1 } else { -1 };
        }
        v
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

const N: usize = 64;

#[test]
fn every_function_gives_ffmpegs_results() {
    let mut d = Draws(0x2545f4914f6cdd1d);
    let mut h = [FNV_BASIS; 8];
    for a in 0..70000 {
        h[0] = fnv(h[0], ff_sqrt(a) as i32);
    }
    for _ in 0..200_000 {
        h[0] = fnv(h[0], ff_sqrt(d.next()) as i32);
    }
    for _ in 0..200_000 {
        let r = d.next();
        let shift = d.next() % 32;
        let x = ((r >> shift) as i32) & 0x7fff_ffff;
        let bits = 16 + (d.next() % 16) as i32;
        h[1] = fnv(h[1], fixed_sqrt(x, bits));
    }
    for _ in 0..2000 {
        let (mut a, mut b, mut c, mut w) = ([0; 2 * N], [0; 2 * N], [0; 2 * N], [0; 2 * N]);
        let mut out = [0; 2 * N];
        let len = 1 + (d.next() as usize % N);
        for i in 0..2 * N {
            a[i] = d.sample();
            b[i] = d.sample();
            c[i] = d.sample();
            w[i] = d.sample();
        }
        vector_fmul(&mut out, &a, &b, len);
        h[2] = out[..len].iter().fold(h[2], |h, &v| fnv(h, v));
        vector_fmul_reverse(&mut out, &a, &b, len);
        h[3] = out[..len].iter().fold(h[3], |h, &v| fnv(h, v));
        vector_fmul_add(&mut out, &a, &b, &c, len);
        h[4] = out[..len].iter().fold(h[4], |h, &v| fnv(h, v));
        vector_fmul_window(&mut out, &a, &b, &w, len);
        h[5] = out[..2 * len].iter().fold(h[5], |h, &v| fnv(h, v));
        for i in 0..len {
            a[i] >>= 4;
            b[i] >>= 4;
        }
        h[6] = fnv(h[6], scalarproduct_fixed(&a, &b, len));
        butterflies_fixed(&mut a, &mut b, len);
        for i in 0..len {
            h[7] = fnv(fnv(h[7], a[i]), b[i]);
        }
    }
    // Random words seldom land on a rounding boundary; powers of two, and
    // squares, land on nothing else.
    let mut e = [FNV_BASIS; 3];
    for _ in 0..200_000 {
        let k = d.next() % 46341;
        let shift = d.next() % 4;
        let x = ((k * k) >> shift) as i32;
        let bits = 16 + (d.next() % 16) as i32;
        e[0] = fnv(e[0], fixed_sqrt(x, bits));
    }
    for _ in 0..2000 {
        let (mut a, mut b, mut w) = ([0; 2 * N], [0; 2 * N], [0; 2 * N]);
        let mut out = [0; 2 * N];
        let len = 1 + (d.next() as usize % N);
        for i in 0..2 * N {
            a[i] = d.power();
            b[i] = d.power();
            w[i] = d.power();
        }
        vector_fmul(&mut out, &a, &b, len);
        e[1] = out[..len].iter().fold(e[1], |h, &v| fnv(h, v));
        vector_fmul_window(&mut out, &a, &b, &w, len);
        e[2] = out[..2 * len].iter().fold(e[2], |h, &v| fnv(h, v));
    }
    assert_eq!(e[0], 0xa324f4b314fe7bb2, "fixed_sqrt of squares");
    assert_eq!(e[1], 0x8f0f6929caba6f9e, "vector_fmul of powers of two");
    assert_eq!(
        e[2], 0x7664a7b27cb3488d,
        "vector_fmul_window of powers of two"
    );
    // What the reference prints, compiled against FFmpeg 8.1.3.
    let expected: [u64; 8] = [
        0x7483cf28339124d6,
        0x0995589d0cb060fe,
        0x0ae4839aba16e337,
        0xcc95a7c627b098a3,
        0x0919caff73bedf6e,
        0x012cf0165d425ab7,
        0x0774eb0c9236c9e0,
        0xc031053b013b59f2,
    ];
    let names = [
        "ff_sqrt",
        "fixed_sqrt",
        "vector_fmul",
        "vector_fmul_reverse",
        "vector_fmul_add",
        "vector_fmul_window",
        "scalarproduct_fixed",
        "butterflies_fixed",
    ];
    for i in 0..8 {
        assert_eq!(h[i], expected[i], "{}", names[i]);
    }
}

#[test]
fn the_in_place_forms_are_the_same() {
    let mut d = Draws(7);
    let a: Vec<i32> = (0..64).map(|_| d.sample()).collect();
    let b: Vec<i32> = (0..64).map(|_| d.sample()).collect();
    let mut out = vec![0; 64];
    let mut inplace = a.clone();
    vector_fmul(&mut out, &a, &b, 64);
    vector_fmul_in_place(&mut inplace, &b, 64);
    assert_eq!(out, inplace);
    let mut inplace = a.clone();
    vector_fmul_reverse(&mut out, &a, &b, 64);
    vector_fmul_reverse_in_place(&mut inplace, &b, 64);
    assert_eq!(out, inplace);
}

#[test]
fn square_roots_are_rounded_down() {
    for a in [
        0u32,
        1,
        2,
        3,
        4,
        254,
        255,
        4095,
        4096,
        65535,
        65536,
        u32::MAX - 1 - (1 << 17),
    ] {
        let r = ff_sqrt(a);
        assert!(u64::from(r) * u64::from(r) <= u64::from(a), "{a}");
        assert!(u64::from(r + 1) * u64::from(r + 1) > u64::from(a), "{a}");
    }
    // Next to 2^32 FFmpeg's guess is 65536, whose square wraps to 0 in its
    // unsigned check, and stands: one too many.
    assert_eq!(ff_sqrt(u32::MAX), 65536);
    // 31 fraction bits: 0.25's root is 0.5.
    assert_eq!(fixed_sqrt(1 << 29, 31), 1 << 30);
}
