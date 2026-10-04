//! The one C library function libvpx's realtime encoder calls whose last
//! bit a decision can turn on: `logf`, as the reference encode's C library
//! computed it.
//!
//! libvpx's learned partitioning (`ml_predict_var_partitioning`, the speed-8
//! partitioning of pictures of 352x288 and smaller) feeds the logarithms of
//! block variances to a small network and splits a block when the network's
//! score is above zero. A score can sit a rounding away from zero, so the
//! logarithms must be the reference's to the bit -- and `logf` is not
//! correctly rounded (glibc documents 0.82 ulp), so "a good logarithm" is
//! not enough.
//!
//! The reference encodes were made with glibc 2.39 on x86-64, whose `logf`
//! is an ifunc: on a processor with FMA and AVX2 -- every x86-64 processor
//! of the last decade -- it runs a build of `sysdeps/ieee754/flt-32/e_logf.c`
//! compiled with FMA, in which the compiler fused four of the source's five
//! multiply-adds. [`logf`] is that build, operation for operation, read from
//! the library's own machine code (`tools/logf_reference.c` says how), with
//! glibc's table and polynomial. [`fma`] is an exact fused multiply-add in
//! integer arithmetic, so the result does not depend on the platform's own
//! `fma`.
//!
//! Translated into Rust from glibc 2.39's `sysdeps/ieee754/flt-32/e_logf.c`
//! and `e_logf_data.c` (copyright Arm Limited, from Arm's optimized-routines,
//! used under its MIT licence as glibc distributes it).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "IEEE 754 encodings taken apart and put together: every narrowing is of a field to its width"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "exponents of normal doubles (within +-1100) and significands below 2^106 shifted within 128 bits: the fused multiply-add's arithmetic cannot overflow"
)]

/// glibc's `__logf_data.tab`: for each of 16 subintervals of [0x1.66p-1,
/// 0x1.66p+0], `1/c` and `log(c)` for a `c` near its centre, as bit patterns.
const LOGF_TABLE: [(u64, u64); 16] = [
    (0x3ff6_61ec_79f8_f3be, 0xbfd5_7bf7_808c_aade),
    (0x3ff5_71ed_4aaf_883d, 0xbfd2_bef0_a7c0_6ddb),
    (0x3ff4_9539_f0f0_10b0, 0xbfd0_1eae_7f51_3a67),
    (0x3ff3_c995_b0b8_0385, 0xbfcb_31d8_a682_24e9),
    (0x3ff3_0d19_0c88_64a5, 0xbfc6_574f_0ac0_7758),
    (0x3ff2_5e22_7b0b_8ea0, 0xbfc1_aa2b_c79c_8100),
    (0x3ff1_bb4a_4a1a_343f, 0xbfba_4e76_ce8c_0e5e),
    (0x3ff1_2358_f08a_e5ba, 0xbfb1_973c_5a61_1ccc),
    (0x3ff0_953f_4199_00a7, 0xbfa2_52f4_38e1_0c1e),
    (0x3ff0_0000_0000_0000, 0x0000_0000_0000_0000),
    (0x3fee_608c_fd9a_47ac, 0x3faa_a5aa_5df2_5984),
    (0x3fec_a4b3_1f02_6aa0, 0x3fbc_5e53_aa36_2eb4),
    (0x3feb_2036_576a_fce6, 0x3fc5_26e5_7720_db08),
    (0x3fe9_c2d1_63a1_aa2d, 0x3fcb_c286_0d22_4770),
    (0x3fe8_86e6_0378_41ed, 0x3fd1_058b_c8a0_7ee1),
    (0x3fe7_67dc_f553_4862, 0x3fd4_0430_57b6_ee09),
];
/// glibc's `__logf_data.ln2`.
const LN2: u64 = 0x3fe6_2e42_fefa_39ef;
/// glibc's `__logf_data.poly`: `log1p(r)` is `r + A0 r^4.. ` -- the
/// coefficients of `r^4`, `r^3` and `r^2`.
const POLY: [u64; 3] = [
    0xbfd0_0ea3_48b8_8334,
    0x3fd5_575b_0be0_0b6a,
    0xbfdf_fffe_f20a_4123,
];

/// `a * b + c` rounded once, to nearest with ties to even: IEEE 754's
/// fused multiply-add, in integer arithmetic.
///
/// For the finite arguments `logf` gives it, whose result is a normal
/// double; anything else (a zero, an infinity, a NaN, a result past the
/// normal range) is computed unfused, which `logf` never asks for.
pub(crate) fn fma(a: f64, b: f64, c: f64) -> f64 {
    /// A normal double's sign, integer significand and exponent:
    /// `x = (-1)^s * m * 2^e`.
    fn parts(x: f64) -> Option<(bool, u128, i32)> {
        let bits = x.to_bits();
        let biased = ((bits >> 52) & 0x7ff) as i32;
        if biased == 0 || biased == 0x7ff {
            return None;
        }
        let m = u128::from((bits & ((1 << 52) - 1)) | (1 << 52));
        Some((bits >> 63 != 0, m, biased - 1075))
    }
    /// `x >> d`, any bits shifted out kept as a set lowest bit (the sticky
    /// bit), which is all rounding needs to know of them.
    fn shr_sticky(x: u128, d: u32) -> u128 {
        if d == 0 {
            x
        } else if d >= 128 {
            u128::from(x != 0)
        } else {
            (x >> d) | u128::from(x & ((1u128 << d) - 1) != 0)
        }
    }
    let (Some((sa, ma, ea)), Some((sb, mb, eb)), Some((sc, mc, ec))) =
        (parts(a), parts(b), parts(c))
    else {
        return a * b + c;
    };
    // The exact product, below 2^106, moved up to just under 2^126; the
    // addend, below 2^53, to just under 2^125: room below both for the
    // rounding to see every bit that matters.
    let (sp, mut xp, mut ep) = (sa != sb, (ma * mb) << 20, ea + eb - 20);
    let (mut xc, mut ecs) = (mc << 72, ec - 72);
    if ep >= ecs {
        xc = shr_sticky(xc, (ep - ecs).unsigned_abs());
        ecs = ep;
    } else {
        xp = shr_sticky(xp, (ecs - ep).unsigned_abs());
        ep = ecs;
    }
    debug_assert_eq!(ep, ecs);
    let (mag, negative) = if sp == sc {
        (xp + xc, sp)
    } else if xp >= xc {
        (xp - xc, sp)
    } else {
        (xc - xp, sc)
    };
    if mag == 0 {
        // An exact cancellation is +0 when rounding to nearest.
        return 0.0;
    }
    // Round the significand to 53 bits.
    let len = 128 - mag.leading_zeros() as i32;
    let (mut m, mut e) = if len <= 53 {
        (mag << (53 - len), ep - (53 - len))
    } else {
        let shift = (len - 53) as u32;
        let kept = mag >> shift;
        let rest = mag & ((1u128 << shift) - 1);
        let half = 1u128 << (shift - 1);
        let up = rest > half || (rest == half && kept & 1 == 1);
        (kept + u128::from(up), ep + shift as i32)
    };
    if m >> 53 != 0 {
        // Rounding carried into a 54th bit.
        m >>= 1;
        e += 1;
    }
    let biased = e + 1075;
    if !(1..0x7ff).contains(&biased) {
        return a * b + c;
    }
    let bits = (u64::from(negative) << 63) | ((biased as u64) << 52) | (m as u64 & ((1 << 52) - 1));
    f64::from_bits(bits)
}

/// glibc 2.39's `logf` as its x86-64 FMA build computes it: `log(x)` to
/// within 0.82 ulp, the same float glibc returns for every input.
pub(crate) fn logf(x: f32) -> f32 {
    /// `OFF`: the subintervals start at 0x1.66p-1.
    const OFF: u32 = 0x3f33_0000;
    let mut ix = x.to_bits();
    if ix == 0x3f80_0000 {
        return 0.0;
    }
    if ix.wrapping_sub(0x0080_0000) >= 0x7f80_0000 - 0x0080_0000 {
        // Below 0x1p-126, infinite or not a number.
        if ix.wrapping_mul(2) == 0 {
            return f32::NEG_INFINITY;
        }
        if ix == 0x7f80_0000 {
            return x;
        }
        if ix & 0x8000_0000 != 0 || ix.wrapping_mul(2) >= 0xff00_0000 {
            return f32::NAN;
        }
        // Subnormal: normalise it.
        ix = (x * f32::from_bits(0x4b00_0000)).to_bits();
        ix = ix.wrapping_sub(23 << 23);
    }
    // x = 2^k z, z in [OFF, 2 OFF), the subinterval i holding z.
    let tmp = ix.wrapping_sub(OFF);
    let i = ((tmp >> 19) % 16) as usize;
    let k = (tmp as i32) >> 23;
    let iz = ix.wrapping_sub(tmp & (0x1ff << 23));
    // `i` is below 16; the fallback, 1/c = 1 and log c = 0, is never taken.
    let (invc_bits, logc_bits) = LOGF_TABLE
        .get(i)
        .copied()
        .unwrap_or((0x3ff0_0000_0000_0000, 0));
    let (invc, logc) = (f64::from_bits(invc_bits), f64::from_bits(logc_bits));
    let z = f64::from(f32::from_bits(iz));
    let [a0, a1, a2] = POLY.map(f64::from_bits);
    // log(x) = log1p(z/c - 1) + log(c) + k ln2. The FMA build's order: the
    // multiply-adds fused, `r * r` and `y0 + r` not.
    let y0 = fma(f64::from(k), f64::from_bits(LN2), logc);
    let r = fma(z, invc, -1.0);
    let r2 = r * r;
    let mut y = fma(r, a1, a2);
    y = fma(r2, a0, y);
    y = fma(r2, y, r + y0);
    y as f32
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::float_cmp,
        reason = "a test: a failure should be loud"
    )]

    use super::*;

    /// FNV-1a over little-endian bytes.
    fn fnv(h: &mut u64, bytes: &[u8]) {
        for &b in bytes {
            *h ^= u64::from(b);
            *h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }

    /// The fused multiply-add is exact where its parts are: a product that
    /// fits rounds nowhere, and a sum whose exact value is a tie rounds to
    /// even.
    #[test]
    fn the_fused_multiply_add_rounds_once() {
        assert_eq!(fma(3.0, 5.0, 7.0), 22.0);
        assert_eq!(fma(-3.0, 5.0, 7.0), -8.0);
        assert_eq!(fma(0.5, 0.25, -0.125), 0.0);
        // (1 + 2^-30)^2 - 1 is 2^-29 + 2^-60: unfused, 2^-60 is lost to
        // the product's rounding.
        let x = 1.0 + f64::from_bits(0x3e10_0000_0000_0000);
        assert_eq!(fma(x, x, -1.0), 2f64.powi(-29) + 2f64.powi(-60));
        assert_ne!(x * x - 1.0, fma(x, x, -1.0));
        // A tie: 1 + 2^-53 is halfway between 1 and its successor, and
        // rounds to the even one, 1.
        assert_eq!(fma(1.0, 1.0, 2f64.powi(-53)), 1.0);
        // Just past the tie rounds up.
        let past = 2f64.powi(-53) + 2f64.powi(-105);
        assert_eq!(fma(1.0, 1.0, past), 1.0 + f64::EPSILON);
    }

    /// The fused multiply-add matches the C library's (glibc's, which is
    /// exact) on a million seeded triples: the hash is the one
    /// `tools/logf_reference.c` prints for the same triples.
    #[test]
    fn the_fused_multiply_add_is_the_c_librarys() {
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = || {
            // xorshift64*: the same generator as the C program.
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            state.wrapping_mul(0x2545_f491_4f6c_dd1d)
        };
        // Normal doubles of moderate exponent, either sign.
        let mut draw = || {
            let r = next();
            let exp = 1023 - 40 + (r >> 52) % 80;
            f64::from_bits((r & (0x800f_ffff_ffff_ffff)) | (exp << 52))
        };
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for _ in 0..1_000_000 {
            let (a, b, c) = (draw(), draw(), draw());
            fnv(&mut h, &fma(a, b, c).to_bits().to_le_bytes());
        }
        assert_eq!(h, FMA_HASH);
    }

    /// `tools/logf_reference.c`'s hash of glibc's `fma` on the triples above.
    const FMA_HASH: u64 = 10_927_660_646_676_674_524;

    /// `logf` is glibc's on every 4099th float from 1 to 2^29 -- the range
    /// the learned partitioning's features take (one more than a variance,
    /// or than a quantiser's square over 256) -- and at its edges.
    #[test]
    fn the_logarithm_is_glibcs_across_the_features_range() {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let (start, end) = (1.0f32.to_bits(), 2f32.powi(29).to_bits());
        let mut bits = start;
        while bits <= end {
            fnv(&mut h, &logf(f32::from_bits(bits)).to_bits().to_le_bytes());
            bits += 4099;
        }
        assert_eq!(h, LOGF_STRIDED_HASH);
        assert_eq!(logf(1.0).to_bits(), 0);
        // log(2), far from any rounding boundary: the nearest float.
        assert_eq!(logf(2.0).to_bits(), 0x3f31_7218);
    }

    /// `tools/logf_reference.c`'s hash of glibc's `logf` on those floats.
    const LOGF_STRIDED_HASH: u64 = 14_317_617_639_039_218_002;

    /// Every float from 1 to 2^29: some 243 million, about a minute in a
    /// release build.
    #[test]
    #[ignore = "exhaustive: every float of the features' range"]
    fn the_logarithm_is_glibcs_on_every_float_of_the_features_range() {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for bits in 1.0f32.to_bits()..=2f32.powi(29).to_bits() {
            fnv(&mut h, &logf(f32::from_bits(bits)).to_bits().to_le_bytes());
        }
        assert_eq!(h, LOGF_ALL_HASH);
    }

    /// `tools/logf_reference.c`'s hash of glibc's `logf` on all of them.
    const LOGF_ALL_HASH: u64 = 2_753_577_037_530_377_380;
}
