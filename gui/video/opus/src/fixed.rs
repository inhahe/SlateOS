//! libopus's fixed-point arithmetic (`celt/fixed_generic.h`, `celt/arch.h`
//! with `FIXED_POINT`), as functions with the macros' exact C semantics: each
//! argument cast to the width the macro casts it to (`opus_val16` is 16 bits,
//! and a macro taking one truncates whatever it is given), products taken in
//! 64 bits as libopus takes them where `OPUS_FAST_INT64` (every 64-bit
//! target), and sums wrapping where C's signed overflow would -- which a valid
//! stream never reaches and a hostile one must not turn into a panic.
//!
//! Translated into Rust from libopus 1.5.2, copyright Xiph.Org and the
//! contributors named in its `COPYING`, used under libopus's BSD licence
//! (`licenses/libopus-COPYING`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "each product is of operands cast to 16 bits first, within 32 bits, or taken in 64; the sums that can leave 32 bits wrap explicitly"
)]

/// `Q15ONE`: 1.0 in Q15, as near as 16 bits hold it.
pub(crate) const Q15ONE: i32 = 32767;
/// `SIG_SHIFT`: a CELT signal's fractional bits beyond 16-bit samples.
pub(crate) const SIG_SHIFT: u32 = 12;
/// `SIG_SAT`: where a 32-bit signal is clipped.
pub(crate) const SIG_SAT: i32 = 300_000_000;
/// `NORM_SCALING`: 1.0 for a normalised band coefficient.
pub(crate) const NORM_SCALING: i32 = 16384;
/// `DB_SHIFT`: an energy's fractional bits (log2 units).
pub(crate) const DB_SHIFT: u32 = 10;
/// `EPSILON`: the smallest step of a fixed-point value.
pub(crate) const EPSILON: i32 = 1;

/// `QCONST16`: `x` in Q`bits`, as C evaluates the macro on the `float`
/// literal libopus passes it: `x * 2^bits` in `float` (exact, a power of
/// two), `.5` added in `double`, the sum cast toward zero. (Two of
/// libopus's calls pass a `double` instead -- `QCONST16(1., SIG_SHIFT)` and
/// `QCONST32(0.999, 16)` -- and come to the same constant either way.)
pub(crate) const fn qconst16(x: f32, bits: u32) -> i32 {
    (0.5 + (x * (1u32 << bits) as f32) as f64) as i16 as i32
}

/// `QCONST32`, as [`qconst16`].
pub(crate) const fn qconst32(x: f32, bits: u32) -> i32 {
    (0.5 + (x * (1u32 << bits) as f32) as f64) as i32
}

/// `EXTRACT16`: the low 16 bits, as a signed value.
#[inline]
pub(crate) const fn extract16(x: i32) -> i32 {
    x as i16 as i32
}

/// `MULT16_16`: the product of two 16-bit values (each truncated to 16 bits
/// first, as the macro casts them).
#[inline]
pub(crate) const fn mult16_16(a: i32, b: i32) -> i32 {
    (a as i16 as i32) * (b as i16 as i32)
}

/// `MULT32_32_32`.
#[inline]
pub(crate) const fn mult32_32_32(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b)
}

/// `MULT16_32_Q15`: `(a * b) >> 15`, `a` taken as 16 bits.
#[inline]
pub(crate) const fn mult16_32_q15(a: i32, b: i32) -> i32 {
    (((a as i16 as i64) * (b as i64)) >> 15) as i32
}

/// `MULT16_32_P16`: rounded.
#[inline]
pub(crate) const fn mult16_32_p16(a: i32, b: i32) -> i32 {
    (((a as i16 as i64) * (b as i64) + 32768) >> 16) as i32
}

/// `MULT32_32_Q16`.
#[inline]
pub(crate) const fn mult32_32_q16(a: i32, b: i32) -> i32 {
    (((a as i64) * (b as i64)) >> 16) as i32
}

/// `MULT32_32_Q31`.
#[inline]
pub(crate) const fn mult32_32_q31(a: i32, b: i32) -> i32 {
    (((a as i64) * (b as i64)) >> 31) as i32
}

/// `MAC16_16`.
#[inline]
pub(crate) const fn mac16_16(c: i32, a: i32, b: i32) -> i32 {
    c.wrapping_add(mult16_16(a, b))
}

/// `MULT16_16_Q14`.
#[inline]
pub(crate) const fn mult16_16_q14(a: i32, b: i32) -> i32 {
    mult16_16(a, b) >> 14
}

/// `MULT16_16_Q15`.
#[inline]
pub(crate) const fn mult16_16_q15(a: i32, b: i32) -> i32 {
    mult16_16(a, b) >> 15
}

/// `MULT16_16_P15`.
#[inline]
pub(crate) const fn mult16_16_p15(a: i32, b: i32) -> i32 {
    (16384i32.wrapping_add(mult16_16(a, b))) >> 15
}

/// `SHL16`: in 16 bits, through an unsigned shift.
#[inline]
pub(crate) const fn shl16(a: i32, shift: u32) -> i32 {
    ((a as u16).wrapping_shl(shift)) as i16 as i32
}

/// `SHR16`, `SHR32`, `SHR`: arithmetic.
#[inline]
pub(crate) const fn shr32(a: i32, shift: u32) -> i32 {
    a >> shift
}

/// `SHL32`, `SHL`: through an unsigned shift, so a value shifted past 32
/// bits wraps rather than being undefined.
#[inline]
pub(crate) const fn shl32(a: i32, shift: u32) -> i32 {
    (a as u32).wrapping_shl(shift) as i32
}

/// `PSHR32`, `PSHR`: shifted right, rounding half up.
#[inline]
pub(crate) const fn pshr32(a: i32, shift: u32) -> i32 {
    a.wrapping_add((1i32 << shift) >> 1) >> shift
}

/// `VSHR32`: right for a positive shift, left for a negative one.
#[inline]
pub(crate) const fn vshr32(a: i32, shift: i32) -> i32 {
    if shift > 0 {
        a >> shift
    } else {
        shl32(a, shift.unsigned_abs())
    }
}

/// `SATURATE`: held to `[-a, a]`.
#[inline]
pub(crate) const fn saturate(x: i32, a: i32) -> i32 {
    if x > a {
        a
    } else if x < -a {
        -a
    } else {
        x
    }
}

/// `SATURATE16`, `SAT16`: held to 16 bits.
#[inline]
pub(crate) const fn saturate16(x: i32) -> i32 {
    if x > 32767 {
        32767
    } else if x < -32768 {
        -32768
    } else {
        x
    }
}

/// `ROUND16`: rounded down `a` bits, then truncated to 16 bits.
#[inline]
pub(crate) const fn round16(x: i32, a: u32) -> i32 {
    extract16(pshr32(x, a))
}

/// `SROUND16`: as `ROUND16`, saturating.
#[inline]
pub(crate) const fn sround16(x: i32, a: u32) -> i32 {
    extract16(saturate(pshr32(x, a), 32767))
}

/// `HALF16`, `HALF32`.
#[inline]
pub(crate) const fn half32(x: i32) -> i32 {
    x >> 1
}

/// `ADD16`: in 16 bits.
#[inline]
pub(crate) const fn add16(a: i32, b: i32) -> i32 {
    ((a as i16 as i32) + (b as i16 as i32)) as i16 as i32
}

/// `SUB16`: of two 16-bit values, the result in the 32 bits C promotes it
/// to (the macro casts its arguments, not its result).
#[inline]
pub(crate) const fn sub16(a: i32, b: i32) -> i32 {
    (a as i16 as i32) - (b as i16 as i32)
}

/// `ADD32`, `ADD32_ovflw`.
#[inline]
pub(crate) const fn add32(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

/// `SUB32`, `SUB32_ovflw`.
#[inline]
pub(crate) const fn sub32(a: i32, b: i32) -> i32 {
    a.wrapping_sub(b)
}

/// `DIV32`.
#[inline]
pub(crate) const fn div32(a: i32, b: i32) -> i32 {
    match a.checked_div(b) {
        Some(q) => q,
        None => 0,
    }
}

/// `SIG2WORD16`: a CELT signal to a 16-bit sample, rounded and saturated.
#[inline]
pub(crate) const fn sig2word16(x: i32) -> i32 {
    let x = pshr32(x, SIG_SHIFT);
    let x = if x < -32768 { -32768 } else { x };
    let x = if x > 32767 { 32767 } else { x };
    extract16(x)
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
    fn constants_round_as_c_casts_them() {
        assert_eq!(qconst16(0.5, 15), 16384);
        assert_eq!(qconst16(1.0, 14), 16384);
        // .5 - 16384 is -16383.5, which C's cast truncates toward zero.
        assert_eq!(qconst16(-0.5, 15), -16383);
        assert_eq!(qconst32(0.6, 15), 19661);
    }

    #[test]
    fn the_macros_truncate_their_arguments_as_c_does() {
        // MULT16_16 casts each argument to 16 bits first.
        assert_eq!(mult16_16(0x1_0002, 3), 6);
        assert_eq!(mult16_16(-32768, -32768), 1 << 30);
        assert_eq!(mult16_32_q15(16384, 1 << 20), 1 << 19);
        assert_eq!(mult32_32_q16(-1, 1), -1, "an arithmetic shift rounds down");
        assert_eq!(mult16_32_p16(1, 1 << 15), 1, "and P16 rounds half up");
        assert_eq!(add16(32767, 1), -32768);
        assert_eq!(sub16(-32768, 1), -32769, "SUB16's result is not cut");
        assert_eq!(pshr32(5, 1), 3);
        assert_eq!(vshr32(1, -3), 8);
        assert_eq!(sig2word16(1 << 30), 32767);
        assert_eq!(div32(7, 0), 0);
    }
}
