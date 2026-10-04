//! SILK's fixed-point arithmetic (`silk/SigProc_FIX.h`, `silk/macros.h`,
//! `silk/Inlines.h`, `silk/log2lin.c`) the decoder uses, as functions with
//! the macros' C semantics on a 64-bit target (`OPUS_FAST_INT64`): the
//! 16-bit operands cast as the macros cast them, the products taken in 64
//! bits, and sums wrapping where C's signed overflow would -- which a valid
//! stream does not reach and a hostile one must not turn into a panic.
//!
//! Translated into Rust from libopus 1.5.2, copyright Skype Limited, Xiph.Org
//! and the contributors named in its `COPYING`, used under libopus's BSD
//! licence (`licenses/libopus-COPYING`).

/// `silk_SMULWB`: `(a * (i16)b) >> 16`.
#[inline]
pub(crate) const fn smulwb(a: i32, b: i32) -> i32 {
    ((a as i64 * b as i16 as i64) >> 16) as i32
}

/// `silk_SMLAWB`: `a + ((b * (i16)c) >> 16)`.
#[inline]
pub(crate) const fn smlawb(a: i32, b: i32, c: i32) -> i32 {
    (a as i64 + ((b as i64 * c as i16 as i64) >> 16)) as i32
}

/// `silk_SMULBB`: `(i16)a * (i16)b`.
#[inline]
pub(crate) const fn smulbb(a: i32, b: i32) -> i32 {
    (a as i16 as i32) * (b as i16 as i32)
}

/// `silk_SMLABB`: `a + (i16)b * (i16)c`.
#[inline]
pub(crate) const fn smlabb(a: i32, b: i32, c: i32) -> i32 {
    a.wrapping_add(smulbb(b, c))
}

/// `silk_SMULWW`: `(a * b) >> 16`.
#[inline]
pub(crate) const fn smulww(a: i32, b: i32) -> i32 {
    ((a as i64 * b as i64) >> 16) as i32
}

/// `silk_SMLAWW`: `a + ((b * c) >> 16)`.
#[inline]
pub(crate) const fn smlaww(a: i32, b: i32, c: i32) -> i32 {
    (a as i64 + ((b as i64 * c as i64) >> 16)) as i32
}

/// `silk_SMULL`: the 64-bit product.
#[inline]
pub(crate) const fn smull(a: i32, b: i32) -> i64 {
    a as i64 * b as i64
}

/// `silk_SMMUL`: the product's high 32 bits.
#[inline]
pub(crate) const fn smmul(a: i32, b: i32) -> i32 {
    (smull(a, b) >> 32) as i32
}

/// `silk_SMULTT`: `(a >> 16) * (b >> 16)`.
#[inline]
pub(crate) const fn smultt(a: i32, b: i32) -> i32 {
    (a >> 16).wrapping_mul(b >> 16)
}

/// `silk_MUL`.
#[inline]
pub(crate) const fn mul(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b)
}

/// `silk_MLA`: `a + b * c`.
#[inline]
pub(crate) const fn mla(a: i32, b: i32, c: i32) -> i32 {
    a.wrapping_add(b.wrapping_mul(c))
}

/// `silk_ADD_SAT32`.
#[inline]
pub(crate) const fn add_sat32(a: i32, b: i32) -> i32 {
    a.saturating_add(b)
}

/// `silk_SAT16`.
#[inline]
pub(crate) const fn sat16(a: i32) -> i32 {
    if a > i16::MAX as i32 {
        i16::MAX as i32
    } else if a < i16::MIN as i32 {
        i16::MIN as i32
    } else {
        a
    }
}

/// `silk_LSHIFT32`: through an unsigned shift.
#[inline]
pub(crate) const fn lshift(a: i32, shift: i32) -> i32 {
    (a as u32).wrapping_shl(shift as u32) as i32
}

/// `silk_RSHIFT_ROUND`: shifted right, rounding half up.
#[inline]
pub(crate) const fn rshift_round(a: i32, shift: i32) -> i32 {
    if shift == 1 {
        (a >> 1) + (a & 1)
    } else {
        ((a >> (shift - 1)) + 1) >> 1
    }
}

/// `silk_RSHIFT_ROUND64`.
#[inline]
pub(crate) const fn rshift_round64(a: i64, shift: i32) -> i64 {
    if shift == 1 {
        (a >> 1) + (a & 1)
    } else {
        ((a >> (shift - 1)) + 1) >> 1
    }
}

/// `silk_LIMIT`: `a` held between the two limits, in either order.
#[inline]
pub(crate) const fn limit(a: i32, limit1: i32, limit2: i32) -> i32 {
    if limit1 > limit2 {
        if a > limit1 {
            limit1
        } else if a < limit2 {
            limit2
        } else {
            a
        }
    } else if a > limit2 {
        limit2
    } else if a < limit1 {
        limit1
    } else {
        a
    }
}

/// `silk_LSHIFT_SAT32`: shifted left, saturating.
#[inline]
pub(crate) const fn lshift_sat32(a: i32, shift: i32) -> i32 {
    lshift(limit(a, i32::MIN >> shift, i32::MAX >> shift), shift)
}

/// `silk_abs`: `-a` for `a` not above 0 (and `i32::MIN` for itself).
#[inline]
pub(crate) const fn abs(a: i32) -> i32 {
    if a > 0 { a } else { a.wrapping_neg() }
}

/// `silk_CLZ32`: leading zeros (32 for 0).
#[inline]
pub(crate) const fn clz32(a: i32) -> i32 {
    (a as u32).leading_zeros() as i32
}

/// `silk_ROR32`: rotated right by `rot` (left for a negative one).
#[inline]
pub(crate) const fn ror32(a: i32, rot: i32) -> i32 {
    let x = a as u32;
    if rot >= 0 {
        x.rotate_right(rot as u32) as i32
    } else {
        x.rotate_left(rot.unsigned_abs()) as i32
    }
}

/// `silk_CLZ_FRAC`: leading zeros, and the 7 bits after the leading one.
#[inline]
pub(crate) const fn clz_frac(a: i32) -> (i32, i32) {
    let lz = clz32(a);
    (lz, ror32(a, 24 - lz) & 0x7f)
}

/// `silk_SQRT_APPROX`: a square root within 10% (2.5% above 120).
pub(crate) const fn sqrt_approx(x: i32) -> i32 {
    if x <= 0 {
        return 0;
    }
    let (lz, frac_q7) = clz_frac(x);
    // 46214 is sqrt(2) * 32768.
    let mut y = if lz & 1 != 0 { 32768 } else { 46214 };
    y >>= lz >> 1;
    smlawb(y, y, smulbb(213, frac_q7))
}

/// `silk_DIV32_16`, `silk_DIV32`: C's division (a zero divisor, which
/// libopus never passes, gives 0).
#[inline]
pub(crate) const fn div32(a: i32, b: i32) -> i32 {
    match a.checked_div(b) {
        Some(q) => q,
        None => 0,
    }
}

/// `silk_DIV32_varQ`: about `(a << q_res) / b`.
pub(crate) const fn div32_varq(a: i32, b: i32, q_res: i32) -> i32 {
    // Head room, and the inputs normalised.
    let a_headrm = clz32(abs(a)) - 1;
    let a_nrm = lshift(a, a_headrm);
    let b_headrm = clz32(abs(b)) - 1;
    let b_nrm = lshift(b, b_headrm);
    // b's inverse, to 14 bits.
    let b_inv = div32(i32::MAX >> 2, b_nrm >> 16);
    // A first approximation, its residual, and the refinement.
    let result = smulwb(a_nrm, b_inv);
    let a_nrm = a_nrm.wrapping_sub(lshift(smmul(b_nrm, result), 3));
    let result = smlawb(result, a_nrm, b_inv);
    let lshift_amount = 29 + a_headrm - b_headrm - q_res;
    if lshift_amount < 0 {
        lshift_sat32(result, -lshift_amount)
    } else if lshift_amount < 32 {
        result >> lshift_amount
    } else {
        0
    }
}

/// `silk_INVERSE32_varQ`: about `(1 << q_res) / b`.
pub(crate) const fn inverse32_varq(b: i32, q_res: i32) -> i32 {
    let b_headrm = clz32(abs(b)) - 1;
    let b_nrm = lshift(b, b_headrm);
    let b_inv = div32(i32::MAX >> 2, b_nrm >> 16);
    let result = lshift(b_inv, 16);
    // The residual from one, then the refinement.
    let err_q32 = lshift((1 << 29) - smulwb(b_nrm, b_inv), 3);
    let result = smlaww(result, err_q32, b_inv);
    let lshift_amount = 61 - b_headrm - q_res;
    if lshift_amount <= 0 {
        lshift_sat32(result, -lshift_amount)
    } else if lshift_amount < 32 {
        result >> lshift_amount
    } else {
        0
    }
}

/// `silk_log2lin`: `2^(x/128)`, about.
pub(crate) const fn log2lin(in_log_q7: i32) -> i32 {
    if in_log_q7 < 0 {
        return 0;
    } else if in_log_q7 >= 3967 {
        return i32::MAX;
    }
    let out = lshift(1, in_log_q7 >> 7);
    let frac_q7 = in_log_q7 & 0x7F;
    let poly = smlawb(frac_q7, smulbb(frac_q7, 128 - frac_q7), -174);
    if in_log_q7 < 2048 {
        // Piecewise parabolic.
        out.wrapping_add(mul(out, poly) >> 7)
    } else {
        mla(out, out >> 7, poly)
    }
}

/// `silk_RAND`: the linear congruential generator.
#[inline]
pub(crate) const fn rand(seed: i32) -> i32 {
    (907_633_515u32).wrapping_add((seed as u32).wrapping_mul(196_314_165)) as i32
}

/// `SILK_FIX_CONST` of a `double`: `c * 2^q + 0.5`, cast toward zero.
pub(crate) const fn fix_const(c: f64, q: u32) -> i32 {
    (c * (1u64 << q) as f64 + 0.5) as i32
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
    fn the_multiplies_cast_as_the_macros_do() {
        // SMULWB takes b's low 16 bits, signed.
        assert_eq!(smulwb(1 << 16, 0x1_FFFF), -1);
        assert_eq!(smulbb(0x1_0002, 3), 6);
        assert_eq!(smultt(3 << 16, 5 << 16), 15);
        assert_eq!(smmul(1 << 30, 1 << 4), 1 << 2);
        assert_eq!(rshift_round(5, 1), 3);
        assert_eq!(rshift_round(-5, 2), -1);
        // `silk_LSHIFT_SAT32` limits to `int32_MAX >> shift` before the
        // shift, so its ceiling is that shifted back: 0x7FFF_FFFC, not MAX.
        assert_eq!(lshift_sat32(1 << 30, 2), 0x1FFF_FFFF << 2);
        assert_eq!(lshift_sat32(-(1 << 30), 2), i32::MIN);
        assert_eq!(limit(5, 10, 0), 5);
        assert_eq!(limit(-5, 10, 0), 0);
        assert_eq!(limit(15, 10, 0), 10);
        assert_eq!(limit(15, 0, 10), 10);
        assert_eq!(clz32(0), 32);
    }

    #[test]
    fn log2lin_is_two_to_the_x_over_128() {
        for (x, near) in [(0, 1), (128, 2), (1280, 1024), (2560, 1 << 20)] {
            let y = log2lin(x);
            assert!((y - near).abs() <= near / 50, "{x}: {y}");
        }
        assert_eq!(log2lin(-1), 0);
        assert_eq!(log2lin(3967), i32::MAX);
    }
}
