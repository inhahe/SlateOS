//! C's conversions between `double` and the integer types, as gcc compiles
//! them for x86-64.
//!
//! procps computes elapsed times, percentages and clock-tick counts in
//! `double` and converts the results back with a cast. Inside the range of
//! the target type that is truncation toward zero, which Rust's `as` also
//! does. Outside it the two part company: C leaves the result undefined, and
//! what x86-64 actually produces is `cvttsd2si`'s "integer indefinite" --
//! the type's most negative value -- where Rust's `as` saturates. A process
//! that started "after" boot, which a wrapping tick subtraction turns into an
//! elapsed time near 2^64, shows up in exactly that difference: `ps -o etimes`
//! prints `0`, `pgrep -O 5` excludes it. These functions are those casts.

/// `double` to `long`: `cvttsd2si`, whose answer outside the range (and for
/// NaN) is `LONG_MIN`.
#[must_use]
pub fn cvt_i64(x: f64) -> i64 {
    // Truncation toward zero, in range by the test above it.
    #[allow(clippy::cast_possible_truncation)]
    if !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&x) {
        // NaN too: no range contains it.
        i64::MIN
    } else {
        x as i64
    }
}

/// `double` to `int`: the 32-bit `cvttsd2si`, whose answer outside the range
/// (and for NaN) is `INT_MIN`.
#[must_use]
pub fn cvt_i32(x: f64) -> i32 {
    // Truncation toward zero, in range by the test above it.
    #[allow(clippy::cast_possible_truncation)]
    // -2^31 - 1 itself truncates out of range, to the same `INT_MIN` that
    // `as` saturates it to.
    if !(-2_147_483_649.0..2_147_483_648.0).contains(&x) {
        // NaN too: no range contains it.
        i32::MIN
    } else {
        x as i32
    }
}

/// `double` to `unsigned long`: `cvttsd2si` below 2^63, and above it the same
/// on `x - 2^63` with the top bit put back.
#[must_use]
pub fn cvt_u64(x: f64) -> u64 {
    const TWO63: f64 = 9_223_372_036_854_775_808.0;
    let bits = |v: i64| u64::from_le_bytes(v.to_le_bytes());
    if x < TWO63 {
        bits(cvt_i64(x))
    } else {
        bits(cvt_i64(x - TWO63)) ^ 0x8000_0000_0000_0000
    }
}

/// `double` to `unsigned int`: the low 32 bits of the 64-bit conversion.
#[must_use]
pub fn cvt_u32(x: f64) -> u32 {
    let b = cvt_i64(x).to_le_bytes();
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// `unsigned long` to `double`: to nearest, as C converts it.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn dbl(v: u64) -> f64 {
    v as f64
}

#[cfg(test)]
mod tests {
    use super::{cvt_i32, cvt_i64, cvt_u32, cvt_u64, dbl};

    #[test]
    fn conversions_as_gcc_emits_them() {
        assert_eq!(cvt_u64(1.9), 1);
        assert_eq!(cvt_u64(-1.5), u64::MAX);
        assert_eq!(cvt_u64(1.0e19), 10_000_000_000_000_000_000);
        assert_eq!(cvt_u64(f64::NAN), 0);
        assert_eq!(cvt_u32(4_294_967_297.0), 1);
        assert_eq!(cvt_i64(f64::NAN), i64::MIN);
    }

    #[test]
    fn int_is_the_32_bit_instruction() {
        assert_eq!(cvt_i32(5.9), 5);
        assert_eq!(cvt_i32(-5.9), -5);
        assert_eq!(cvt_i32(2_147_483_647.9), i32::MAX);
        assert_eq!(cvt_i32(2_147_483_648.0), i32::MIN);
        assert_eq!(cvt_i32(-2_147_483_648.9), i32::MIN);
        assert_eq!(cvt_i32(-2_147_483_649.0), i32::MIN);
        assert_eq!(cvt_i32(1.8e17), i32::MIN);
        assert_eq!(cvt_i32(f64::NAN), i32::MIN);
    }

    #[test]
    fn unsigned_to_double_rounds_to_nearest() {
        assert!((dbl(u64::MAX) - 18_446_744_073_709_551_616.0).abs() < 1.0);
        assert!((dbl(100) - 100.0).abs() < f64::EPSILON);
    }
}
