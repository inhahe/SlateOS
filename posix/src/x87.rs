//! x87 80-bit extended precision (`long double` on x86-64 SysV).
//!
//! Rust has no 80-bit float type, but `long double` is unavoidable at our C
//! ABI boundary: `printf("%Lf", …)`, `scanf("%Lf", …)` and `strtold` all speak
//! it, and a C caller's `long double` is 80 bits whether we like it or not.
//! This module is the single place that knows the format, so the rest of the
//! sysroot can keep working in `f64`.
//!
//! ## Why this exists at all
//!
//! Before this module the sysroot handled `long double` by *pretending it was
//! `double`* — `strtold` was an alias for `strtod` returning `f64`, and the
//! printf engine did not recognise the `L` length modifier. Both are silent
//! data corruption rather than a mere precision loss:
//!
//! - **Return value.** An 80-bit `long double` is returned in `%st(0)`, not
//!   `%xmm0`. A C caller of the old `strtold` read `%st(0)` and got whatever
//!   the x87 stack happened to hold. This is the same failure mode as
//!   BUG-SYSROOT-SOFT-FLOAT-ABI: two sides that link cleanly and disagree.
//! - **Varargs.** `long double` is class X87/X87UP, which the ABI resolves to
//!   MEMORY: it is *never* in a register, always 16 bytes on the stack,
//!   16-byte aligned. The old printf skipped the `L`, treated it as the
//!   conversion character, consumed no argument, and left every *subsequent*
//!   argument shifted by 16 bytes.
//!
//! ## What this module does and does not promise
//!
//! It gets the **format and the ABI** exactly right: values cross the boundary
//! in the encoding C expects, and the narrowing to `f64` is correctly rounded
//! (round-to-nearest, ties-to-even) with no double rounding, including into
//! the subnormal range.
//!
//! It does **not** give 64-bit-mantissa *arithmetic*. Anything we compute is
//! computed in `f64`, so a `long double` that round-trips through the sysroot
//! keeps 53 bits of significand, not 64. That is a documented limitation
//! (`TD-POSIX-LONG-DOUBLE-PRECISION` in `known-issues.md`), and it is a far
//! better failure than the previous one: results are accurate to `double`
//! rather than arbitrary.
//!
//! ## The format
//!
//! 80 bits, stored in 10 bytes little-endian (padded to 16 in memory):
//!
//! ```text
//!   bytes 0..8   significand : u64   -- bit 63 is the EXPLICIT integer bit
//!   bytes 8..10  sign_exp    : u16   -- bit 15 sign, bits 14..0 exponent
//! ```
//!
//! Unlike IEEE binary32/64 the leading 1 is *stored*, not implied. The
//! exponent bias is 16383. Value of a normal number is
//! `significand * 2^(exponent - 16383 - 63)`.

/// A `long double` as it appears in memory at the C ABI boundary.
///
/// 16 bytes with 16-byte alignment, matching the x86-64 SysV `long double`
/// parameter/object layout. Only the first 10 bytes are meaningful; the rest
/// is padding whose contents are unspecified (we zero it when we write one).
#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct LongDouble {
    /// Significand, with the integer bit explicitly stored at bit 63.
    pub significand: u64,
    /// Sign in bit 15, biased exponent in bits 14..0.
    pub sign_exp: u16,
    /// Padding to the ABI's 16-byte object size. Written as zero.
    pub pad: [u8; 6],
}

impl LongDouble {
    /// The all-zero value (`+0.0L`), for initialising before a store.
    pub const ZERO: Self = Self {
        significand: 0,
        sign_exp: 0,
        pad: [0; 6],
    };
}

/// Exponent bias of the x87 80-bit format.
const BIAS: i32 = 16383;
/// Reserved exponent field: infinities and NaNs.
const EXP_MAX: i32 = 0x7FFF;

/// Narrow an x87 80-bit value to `f64`, correctly rounded (nearest, ties even).
///
/// Rounding is done once, directly from the 64-bit significand to the target
/// precision — including when the result lands in the `f64` subnormal range,
/// where the obvious "convert then scale" shortcut would round twice and be
/// wrong by an ulp for some inputs.
///
/// Encodings that real x87 hardware treats as invalid (the "pseudo-" forms:
/// a maximal exponent with the integer bit clear, or a non-maximal, non-zero
/// exponent with the integer bit clear) are mapped to NaN, which is what the
/// hardware produces when it loads them.
#[must_use]
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
pub fn to_f64(value: LongDouble) -> f64 {
    let sign_bit = u64::from(value.sign_exp >> 15) << 63;
    let exp = i32::from(value.sign_exp & 0x7FFF);
    let sig = value.significand;

    // Infinity / NaN.
    if exp == EXP_MAX {
        let frac = sig & 0x7FFF_FFFF_FFFF_FFFF;
        if frac == 0 && (sig >> 63) == 1 {
            return f64::from_bits(sign_bit | 0x7FF0_0000_0000_0000);
        }
        // NaN. Shifting the fraction right by 11 lands the x87 quiet bit
        // (62) on the f64 quiet bit (51) and truncates the low payload, which
        // 52 bits cannot hold. The result is then forced quiet: a signalling
        // NaN whose payload happens to truncate to zero would otherwise come
        // out as an *infinity*, silently turning a NaN into a finite-looking
        // special value. Losing the signalling flag is the lesser evil.
        return f64::from_bits(sign_bit | 0x7FF0_0000_0000_0000 | (frac >> 11) | (1 << 51));
    }

    if sig == 0 {
        // ±0 (also covers the "pseudo-zero" encoding, exponent set but
        // significand zero, which hardware treats as invalid → we return zero
        // of the right sign rather than inventing a NaN for a value that has
        // no fraction bits at all).
        return f64::from_bits(sign_bit);
    }

    // A normal x87 number has the integer bit set. When the exponent field is
    // 0 the number is subnormal and the integer bit is legitimately clear; its
    // value is `sig * 2^(-16382 - 63)`. Any *other* exponent with the integer
    // bit clear is an unnormal, which is invalid.
    if exp != 0 && (sig >> 63) == 0 {
        return f64::NAN;
    }

    // Unified form: value = sig * 2^scale, with sig != 0. A zero exponent
    // field denotes the subnormal range, whose values are scaled as if the
    // field were 1 (there is no implicit leading bit to compensate for).
    // The `- 63` in both arms is the weight of the significand's bit 63.
    const SUBNORMAL_SCALE: i32 = -16382 - 63;
    let scale = if exp == 0 {
        SUBNORMAL_SCALE
    } else {
        exp.wrapping_sub(BIAS).wrapping_sub(63)
    };

    // Normalise so the most significant set bit is at bit 63; then the
    // unbiased binary exponent of the value is `msb_exp`.
    let shift = sig.leading_zeros();
    let norm = sig << shift;
    let msb_exp = scale.wrapping_add(63).wrapping_sub(shift as i32);

    if msb_exp > 1023 {
        return f64::from_bits(sign_bit | 0x7FF0_0000_0000_0000);
    }

    if msb_exp >= -1022 {
        // Normal f64. Keep 53 bits (implicit leading 1 + 52 stored), round the
        // discarded 11 to nearest, ties to even.
        let mut mant = norm >> 11; // 53 bits, bit 52 set
        let rem = norm & 0x7FF;
        if rem > 0x400 || (rem == 0x400 && (mant & 1) == 1) {
            mant = mant.wrapping_add(1);
        }
        let mut e = msb_exp;
        if mant >> 53 == 1 {
            // Rounding carried out of the significand: 1.111…1 → 10.000…0.
            mant >>= 1;
            e = e.wrapping_add(1);
            if e > 1023 {
                return f64::from_bits(sign_bit | 0x7FF0_0000_0000_0000);
            }
        }
        let biased = e.wrapping_add(1023) as u64;
        return f64::from_bits(sign_bit | (biased << 52) | (mant & 0x000F_FFFF_FFFF_FFFF));
    }

    // Subnormal f64: the result is `m * 2^-1074` for some m < 2^52. Compute m
    // straight from the original significand so there is exactly one rounding.
    // `rshift` is how far `sig` must move right to be measured in units of
    // 2^-1074; it can be negative when the value is subnormal only because the
    // significand is tiny, in which case the shift is exact.
    let rshift = scale.wrapping_add(1074).wrapping_neg();
    if rshift <= 0 {
        // Exact: no bits are discarded.
        let m = sig << rshift.wrapping_neg() as u32;
        return f64::from_bits(sign_bit | m);
    }
    let rshift = rshift as u32;
    if rshift >= 64 {
        // Everything is below the rounding position. The only case that does
        // not round to zero is rshift == 64 with the guard bit set and a
        // non-zero sticky (an exact tie rounds to even, i.e. to zero).
        if rshift == 64 && (sig >> 63) == 1 && (sig & 0x7FFF_FFFF_FFFF_FFFF) != 0 {
            return f64::from_bits(sign_bit | 1);
        }
        return f64::from_bits(sign_bit);
    }
    // `rshift` is in 1..=63 here (both bounds excluded above), so neither the
    // mask nor the guard-bit position can shift out of range.
    let mut m = sig >> rshift;
    let half = 1u64 << rshift.wrapping_sub(1);
    let rem = sig & (half | half.wrapping_sub(1));
    if rem > half || (rem == half && (m & 1) == 1) {
        m = m.wrapping_add(1);
    }
    // If rounding pushed m up to 2^52 the result is the smallest normal, and
    // `sign | m` already encodes it (exponent field 1, mantissa 0).
    f64::from_bits(sign_bit | m)
}

/// Widen an `f64` to x87 80-bit. Always exact — every `f64` is representable.
#[must_use]
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
pub fn from_f64(value: f64) -> LongDouble {
    let bits = value.to_bits();
    let sign = ((bits >> 63) as u16) << 15;
    let exp11 = ((bits >> 52) & 0x7FF) as i32;
    let frac = bits & 0x000F_FFFF_FFFF_FFFF;

    if exp11 == 0x7FF {
        // Infinity or NaN. For infinity the significand is just the integer
        // bit; for NaN, shifting the fraction left by 11 lands the f64 quiet
        // bit (51) on the x87 quiet bit (62).
        let significand = if frac == 0 {
            1u64 << 63
        } else {
            (1u64 << 63) | (frac << 11)
        };
        return LongDouble {
            significand,
            sign_exp: sign | 0x7FFF,
            pad: [0; 6],
        };
    }

    if exp11 == 0 {
        if frac == 0 {
            return LongDouble {
                significand: 0,
                sign_exp: sign,
                pad: [0; 6],
            };
        }
        // Subnormal f64 — value is `frac * 2^-1074`. x87 has a far wider
        // exponent range, so it becomes an ordinary normal number there.
        let shift = frac.leading_zeros();
        let significand = frac << shift;
        // The msb sits at bit 63 after the shift; its weight is
        // 2^(-1074 + 63 - shift).
        let msb_exp = (-1074_i32).wrapping_add(63).wrapping_sub(shift as i32);
        return LongDouble {
            significand,
            sign_exp: sign | (msb_exp.wrapping_add(BIAS) as u16 & 0x7FFF),
            pad: [0; 6],
        };
    }

    LongDouble {
        significand: (1u64 << 63) | (frac << 11),
        sign_exp: sign | (exp11.wrapping_sub(1023).wrapping_add(BIAS) as u16 & 0x7FFF),
        pad: [0; 6],
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    fn ld(significand: u64, sign_exp: u16) -> LongDouble {
        LongDouble {
            significand,
            sign_exp,
            pad: [0; 6],
        }
    }

    #[test]
    fn round_trips_every_f64_exactly() {
        let values = [
            0.0f64,
            -0.0,
            1.0,
            -1.0,
            0.5,
            2.0,
            3.5,
            -7.25,
            1e300,
            -1e300,
            1e-300,
            f64::MAX,
            f64::MIN,
            f64::MIN_POSITIVE,
            f64::from_bits(1),                 // smallest subnormal
            f64::from_bits(0x000F_FFFF_FFFF_FFFF), // largest subnormal
            core::f64::consts::PI,
            core::f64::consts::E,
        ];
        for v in values {
            let back = to_f64(from_f64(v));
            assert_eq!(
                back.to_bits(),
                v.to_bits(),
                "round trip changed {v:e} (bits {:#x} -> {:#x})",
                v.to_bits(),
                back.to_bits()
            );
        }
    }

    #[test]
    fn round_trips_infinities_and_nan() {
        assert!(to_f64(from_f64(f64::INFINITY)).is_infinite());
        assert!(to_f64(from_f64(f64::INFINITY)).is_sign_positive());
        assert!(to_f64(from_f64(f64::NEG_INFINITY)).is_infinite());
        assert!(to_f64(from_f64(f64::NEG_INFINITY)).is_sign_negative());
        assert!(to_f64(from_f64(f64::NAN)).is_nan());
    }

    #[test]
    fn decodes_known_encodings() {
        // 1.0L = significand 0x8000000000000000, exponent 16383.
        assert_eq!(to_f64(ld(0x8000_0000_0000_0000, 16383)), 1.0);
        // 2.0L: exponent one higher.
        assert_eq!(to_f64(ld(0x8000_0000_0000_0000, 16384)), 2.0);
        // -1.0L: sign bit set.
        assert_eq!(to_f64(ld(0x8000_0000_0000_0000, 0x8000 | 16383)), -1.0);
        // 0.5L
        assert_eq!(to_f64(ld(0x8000_0000_0000_0000, 16382)), 0.5);
        // 3.5L = 1.75 * 2^1 -> significand 0xE000...
        assert_eq!(to_f64(ld(0xE000_0000_0000_0000, 16384)), 3.5);
        // ±0
        assert_eq!(to_f64(ld(0, 0)).to_bits(), 0.0f64.to_bits());
        assert_eq!(to_f64(ld(0, 0x8000)).to_bits(), (-0.0f64).to_bits());
        // +inf / -inf
        assert!(to_f64(ld(0x8000_0000_0000_0000, 0x7FFF)).is_infinite());
        assert!(to_f64(ld(0x8000_0000_0000_0000, 0xFFFF)).is_sign_negative());
        // NaN
        assert!(to_f64(ld(0xC000_0000_0000_0000, 0x7FFF)).is_nan());
    }

    #[test]
    fn narrowing_rounds_to_nearest_even() {
        // Build 1.0 + 2^-53 exactly (representable in 80-bit, not in f64).
        // significand = 1000...0 with bit (63-53)=10 set.
        let halfway = ld((1u64 << 63) | (1u64 << 10), 16383);
        // Exactly halfway between 1.0 and the next f64; ties to even -> 1.0.
        assert_eq!(to_f64(halfway), 1.0);

        // Just above halfway must round up to nextafter(1.0).
        let above = ld((1u64 << 63) | (1u64 << 10) | 1, 16383);
        assert_eq!(to_f64(above).to_bits(), 1.0f64.to_bits() + 1);

        // Just below halfway rounds down to 1.0.
        let below = ld((1u64 << 63) | ((1u64 << 10) - 1), 16383);
        assert_eq!(to_f64(below), 1.0);

        // A tie whose lower neighbour is odd must round *up* (to even).
        let odd_base = (1u64 << 63) | (1u64 << 11); // mantissa lsb = 1
        let tie_up = ld(odd_base | (1u64 << 10), 16383);
        assert_eq!(to_f64(tie_up).to_bits(), to_f64(ld(odd_base, 16383)).to_bits() + 1);
    }

    #[test]
    fn narrowing_carries_out_of_the_significand() {
        // All 53 kept bits are 1 and the rest round up: 1.111…1 -> 2.0.
        let v = ld(u64::MAX, 16383);
        assert_eq!(to_f64(v), 2.0);
    }

    #[test]
    fn overflow_becomes_infinity() {
        // Exponent far beyond f64's range.
        assert!(to_f64(ld(0x8000_0000_0000_0000, 16383 + 2000)).is_infinite());
        assert!(to_f64(ld(0x8000_0000_0000_0000, 0x8000 | (16383 + 2000))).is_sign_negative());
        // Rounding that carries past f64::MAX must also become infinity, not
        // wrap to a small exponent.
        let just_over = ld(u64::MAX, (16383 + 1023) as u16);
        assert!(to_f64(just_over).is_infinite());
    }

    #[test]
    fn underflow_reaches_subnormals_then_zero() {
        // 2^-1030: subnormal in f64, normal in x87.
        let v = ld(0x8000_0000_0000_0000, (16383 - 1030) as u16);
        let got = to_f64(v);
        assert_ne!(got, 0.0, "2^-1030 must survive as a subnormal");
        assert_eq!(got.to_bits(), (1u64 << (52 - 8)));

        // 2^-1074: the smallest subnormal.
        let v = ld(0x8000_0000_0000_0000, (16383 - 1074) as u16);
        assert_eq!(to_f64(v).to_bits(), 1);

        // 2^-1075: exactly half of the smallest subnormal -> ties to even -> 0.
        let v = ld(0x8000_0000_0000_0000, (16383 - 1075) as u16);
        assert_eq!(to_f64(v), 0.0);

        // Just above 2^-1075 must round up to the smallest subnormal.
        let v = ld(0x8000_0000_0000_0000 | 1, (16383 - 1075) as u16);
        assert_eq!(to_f64(v).to_bits(), 1);

        // Far below: flushes to zero, keeping the sign.
        let v = ld(0x8000_0000_0000_0000, 0x8000 | (16383 - 2000) as u16);
        assert_eq!(to_f64(v).to_bits(), (-0.0f64).to_bits());
    }

    #[test]
    fn subnormal_x87_input_is_negligible_but_signed() {
        // Exponent field 0 with a non-zero significand: an x87 subnormal, whose
        // magnitude is below 2^-16382 and so far below any f64 subnormal.
        assert_eq!(to_f64(ld(1, 0)).to_bits(), 0.0f64.to_bits());
        assert_eq!(to_f64(ld(1, 0x8000)).to_bits(), (-0.0f64).to_bits());
    }

    #[test]
    fn invalid_unnormal_encodings_are_nan() {
        // Non-zero exponent with the integer bit clear is an "unnormal", which
        // no x87 since the 80387 produces and which the FPU faults on.
        assert!(to_f64(ld(0x4000_0000_0000_0000, 16383)).is_nan());
    }

    #[test]
    fn widening_a_subnormal_f64_normalises_it() {
        let smallest = f64::from_bits(1); // 2^-1074
        let wide = from_f64(smallest);
        // Must be a *normal* x87 number: integer bit set, exponent non-zero.
        assert_eq!(wide.significand >> 63, 1);
        assert_ne!(wide.sign_exp & 0x7FFF, 0);
        assert_eq!(to_f64(wide).to_bits(), 1);
    }

    #[test]
    fn long_double_is_the_abi_size_and_alignment() {
        assert_eq!(core::mem::size_of::<LongDouble>(), 16);
        assert_eq!(core::mem::align_of::<LongDouble>(), 16);
    }
}
