//! CELT's fixed-point approximations of the functions a float build takes
//! from libm -- square roots, reciprocals, exponentials and cosines -- each
//! exact in integers, so that a decoder computes the
//! same samples on every machine.
//!
//! In C every assignment to an `opus_val16` truncates to 16 bits; here a
//! value bound for one goes through [`extract16`], where libopus's variable
//! would have truncated it.
//!
//! Translated into Rust from libopus 1.5.2's `celt/mathops.h` and
//! `celt/mathops.c` (`FIXED_POINT`), copyright Xiph.Org and the contributors
//! named in its `COPYING`, used under libopus's BSD licence
//! (`licenses/libopus-COPYING`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "libopus's own arithmetic on values its approximations keep within 16 and 32 bits (each input range is documented beside it); the few sums that could leave it wrap, as the casts below make explicit"
)]

use crate::entdec::ilog;
use crate::fixed::{
    add16, add32, extract16, mult16_16_p15, mult16_16_q15, mult16_32_q15, mult32_32_q31, pshr32,
    round16, shl16, shl32, sub16, sub32, vshr32,
};

/// `celt_ilog2`: the integer log2 of a positive value.
#[inline]
pub(crate) fn ilog2(x: i32) -> i32 {
    ilog(x as u32) - 1
}

/// `celt_zlog2`: as [`ilog2`], 0 for a value not above 0.
#[inline]
pub(crate) fn zlog2(x: i32) -> i32 {
    if x <= 0 { 0 } else { ilog2(x) }
}

/// `isqrt32`: `floor(sqrt(v))`, exactly, for a positive `v`.
pub(crate) fn isqrt32(mut v: u32) -> u32 {
    let mut g = 0u32;
    let mut bshift = (ilog(v) - 1) >> 1;
    let mut b = 1u32.checked_shl(bshift.max(0) as u32).unwrap_or(0);
    loop {
        let t = ((g << 1).wrapping_add(b))
            .checked_shl(bshift.max(0) as u32)
            .unwrap_or(0);
        if t <= v {
            g += b;
            v -= t;
        }
        b >>= 1;
        bshift -= 1;
        if bshift < 0 {
            break;
        }
    }
    g
}

/// `frac_div32`: `a / b` in Q31, for `b` positive.
pub(crate) fn frac_div32(a: i32, b: i32) -> i32 {
    let shift = ilog2(b) - 29;
    let a = vshr32(a, shift);
    let b = vshr32(b, shift);
    // A 16-bit reciprocal.
    let rcp = round16(rcp(round16(b, 16)), 3);
    let mut result = mult16_32_q15(rcp, a);
    let rem = sub32(pshr32(a, 2), mult32_32_q31(result, b));
    result = add32(result, shl32(mult16_32_q15(rcp, rem), 2));
    if result >= 536_870_912 {
        2_147_483_647
    } else if result <= -536_870_912 {
        -2_147_483_647
    } else {
        shl32(result, 2)
    }
}

/// `celt_rsqrt_norm`: `1/sqrt(x)` for `x` in [0.25, 1) (Q16 in, Q14 out).
pub(crate) fn rsqrt_norm(x: i32) -> i32 {
    let n = extract16(x - 32768);
    let r = add16(
        23557,
        mult16_16_q15(n, add16(-13490, mult16_16_q15(n, 6713))),
    );
    let r2 = extract16(mult16_16_q15(r, r));
    let y = shl16(sub16(add16(mult16_16_q15(r2, n), r2), 16384), 1);
    add16(
        r,
        mult16_16_q15(r, mult16_16_q15(y, sub16(mult16_16_q15(y, 12288), 16384))),
    )
}

/// `celt_sqrt`: `sqrt(x)` (Q`k` in, Q`k/2` out).
pub(crate) fn sqrt(x: i32) -> i32 {
    const C: [i32; 5] = [23175, 11561, -3011, 1699, -664];
    if x == 0 {
        return 0;
    } else if x >= 1_073_741_824 {
        return 32767;
    }
    let k = (ilog2(x) >> 1) - 7;
    let x = vshr32(x, 2 * k);
    let n = extract16(x - 32768);
    let rt = add16(
        C[0],
        mult16_16_q15(
            n,
            add16(
                C[1],
                mult16_16_q15(
                    n,
                    add16(C[2], mult16_16_q15(n, add16(C[3], mult16_16_q15(n, C[4])))),
                ),
            ),
        ),
    );
    vshr32(rt, 7 - k)
}

/// `_celt_cos_pi_2`: `cos(pi/2 * x)` for `x` in [0, 1) (Q15).
fn cos_pi_2(x: i32) -> i32 {
    const L1: i32 = 32767;
    const L2: i32 = -7651;
    const L3: i32 = 8277;
    const L4: i32 = -626;
    let x2 = extract16(mult16_16_p15(x, x));
    add16(
        1,
        32766.min(add32(
            sub16(L1, x2),
            mult16_16_p15(
                x2,
                add32(L2, mult16_16_p15(x2, add32(L3, mult16_16_p15(L4, x2)))),
            ),
        )),
    )
}

/// `celt_cos_norm`: `cos(pi/2 * x)` for `x` in Q15, taken modulo 4 (so
/// 32768 is a right angle and 2^17 a full turn).
pub(crate) fn cos_norm(x: i32) -> i32 {
    let mut x = x & 0x0001_ffff;
    if x > shl32(1, 16) {
        x = sub32(shl32(1, 17), x);
    }
    if x & 0x0000_7fff != 0 {
        if x < shl32(1, 15) {
            cos_pi_2(extract16(x))
        } else {
            extract16(-cos_pi_2(extract16(65536 - x)))
        }
    } else if x & 0x0000_ffff != 0 {
        0
    } else if x & 0x0001_ffff != 0 {
        -32767
    } else {
        32767
    }
}

/// `celt_rcp`: `1/x` (Q15 in, Q16 out), for `x` positive.
pub(crate) fn rcp(x: i32) -> i32 {
    let i = ilog2(x);
    // In Q15, in [0, 1).
    let n = extract16(vshr32(x, i - 15) - 32768);
    let mut r = add16(30840, mult16_16_q15(-15420, n));
    r = extract16(sub16(
        r,
        mult16_16_q15(r, add16(mult16_16_q15(r, n), add16(r, -32768))),
    ));
    r = extract16(sub16(
        r,
        add16(
            1,
            mult16_16_q15(r, add16(mult16_16_q15(r, n), add16(r, -32768))),
        ),
    ));
    vshr32(r, i - 16)
}

/// `celt_div`: `a / b` through [`rcp`].
#[inline]
pub(crate) fn div(a: i32, b: i32) -> i32 {
    mult32_32_q31(a, rcp(b))
}

/// `celt_exp2_frac`: `2^x` for `x` in [0, 1) (Q10 in, Q14 out).
pub(crate) fn exp2_frac(x: i32) -> i32 {
    const D0: i32 = 16383;
    const D1: i32 = 22804;
    const D2: i32 = 14819;
    const D3: i32 = 10204;
    let frac = shl16(x, 4);
    add16(
        D0,
        mult16_16_q15(
            frac,
            add16(D1, mult16_16_q15(frac, add16(D2, mult16_16_q15(D3, frac)))),
        ),
    )
}

/// `celt_exp2`: `2^x` (Q10 in, Q16 out).
pub(crate) fn exp2(x: i32) -> i32 {
    let x = extract16(x);
    let integer = x >> 10;
    if integer > 14 {
        return 0x7f00_0000;
    } else if integer < -15 {
        return 0;
    }
    let frac = extract16(exp2_frac(extract16(x - shl16(integer, 10))));
    vshr32(frac, -integer - 2)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    #[test]
    fn isqrt32_is_exact() {
        for v in [1u32, 2, 3, 4, 15, 16, 17, 99, 100, 65535, 65536, u32::MAX] {
            let g = isqrt32(v);
            assert!(u64::from(g) * u64::from(g) <= u64::from(v), "{v}");
            assert!(u64::from(g + 1) * u64::from(g + 1) > u64::from(v), "{v}");
        }
    }

    /// Values libopus 1.5.2's own functions return (a C program linked
    /// against its fixed-point build, the same that writes the table
    /// `celt::reference` compares a million and a half more against).
    #[test]
    fn the_approximations_are_libopuss() {
        // sqrt(x), integer in and out at these sizes.
        for (x, r) in [(2, 1), (4096, 64), (16384, 128), (65535, 255)] {
            assert_eq!(sqrt(x), r, "sqrt {x}");
        }
        // 1/x: Q15 in, Q16 out (0.5 -> 2.0, nearly).
        for (x, r) in [
            (1, 2_147_418_112),
            (2, 1_073_709_056),
            (100, 21_474_304),
            (16384, 131_068),
            (32767, 65536),
        ] {
            assert_eq!(rcp(x), r, "rcp {x}");
        }
        // 2^x: Q10 in, Q16 out (1.0 -> 2.0, nearly); out of range at both
        // ends.
        for (x, r) in [(1024, 131_064), (-16384, 0), (15360, 2_130_706_432)] {
            assert_eq!(exp2(x), r, "exp2 {x}");
        }
        assert_eq!(exp2_frac(512), 23169, "2^0.5 in Q14");
        // cos(pi/2 * x), x in Q15: 1.0 -> 0, 2.0 -> -1, 4.0 wraps to 0.
        for (x, r) in [(0, 32767), (32768, 0), (65536, -32767), (131_071, 32767)] {
            assert_eq!(cos_norm(x), r, "cos_norm {x}");
        }
        // 1/sqrt(x) for x in [0.25, 1): Q16 in, Q14 out.
        assert_eq!(rsqrt_norm(16384), 32766);
        assert_eq!(rsqrt_norm(65535), 16385);
        assert_eq!(frac_div32(113_907, 7_993_833), 30_600_268);
        assert_eq!(frac_div32(-55_051_902, 365_426_679), -323_520_600);
    }
}
