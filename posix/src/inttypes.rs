//! `<inttypes.h>` — integer type conversion functions.
//!
//! On x86_64, `intmax_t` is `int64_t` (= `i64`) and `uintmax_t` is
//! `uint64_t` (= `u64`).  `strtoimax` and `strtoumax` therefore
//! delegate directly to `strtoll`/`strtoull`.
//!
//! Also provides `imaxabs`, `imaxdiv`, `wcstoimax`, and `wcstoumax`.

/// Absolute value of an `intmax_t`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn imaxabs(j: i64) -> i64 {
    if j < 0 { j.wrapping_neg() } else { j }
}

/// Result of `imaxdiv`.
#[repr(C)]
pub struct ImaxdivT {
    /// Quotient.
    pub quot: i64,
    /// Remainder.
    pub rem: i64,
}

/// Compute quotient and remainder of `numer / denom`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn imaxdiv(numer: i64, denom: i64) -> ImaxdivT {
    if denom == 0 {
        return ImaxdivT { quot: 0, rem: 0 };
    }
    // Guard against overflow: i64::MIN / -1 would panic in debug builds.
    // POSIX leaves this undefined, but a C library must not crash.
    // Use wrapping division (same as release-mode behavior).
    ImaxdivT {
        quot: numer.wrapping_div(denom),
        rem: numer.wrapping_rem(denom),
    }
}

/// Convert a string to `intmax_t` (= `int64_t` on x86_64).
///
/// Delegates to `strtoll`.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtoimax(
    nptr: *const u8,
    endptr: *mut *const u8,
    base: i32,
) -> i64 {
    unsafe { crate::stdlib::strtoll(nptr, endptr, base) }
}

/// Convert a string to `uintmax_t` (= `uint64_t` on x86_64).
///
/// Delegates to `strtoull`.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtoumax(
    nptr: *const u8,
    endptr: *mut *const u8,
    base: i32,
) -> u64 {
    unsafe { crate::stdlib::strtoull(nptr, endptr, base) }
}

// ---------------------------------------------------------------------------
// Wide character → integer conversion helpers
// ---------------------------------------------------------------------------

/// Check if a wide character is ASCII whitespace.
#[inline]
const fn wc_is_space(wc: i32) -> bool {
    matches!(wc, 0x20 | 0x09 | 0x0a | 0x0d | 0x0b | 0x0c)
}

/// Convert a wide character to its digit value in the given base.
/// Returns -1 if not a valid digit.
#[inline]
fn wc_digit(wc: i32, base: i32) -> i32 {
    let val = match wc {
        0x30..=0x39 => wc.wrapping_sub(0x30), // '0'..'9'
        0x61..=0x7a => wc.wrapping_sub(0x61).wrapping_add(10), // 'a'..'z'
        0x41..=0x5a => wc.wrapping_sub(0x41).wrapping_add(10), // 'A'..'Z'
        _ => return -1,
    };
    if val < base { val } else { -1 }
}

/// Convert a `wchar_t` string to `intmax_t`.
///
/// Skips leading whitespace, handles optional sign, auto-detects base
/// if `base` is 0, and skips `0x`/`0X` prefix for hex.  Stores end
/// pointer through `endptr` if non-null.
///
/// # Safety
///
/// `nptr` must point to a valid null-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn wcstoimax(
    nptr: *const i32,
    endptr: *mut *const i32,
    mut base: i32,
) -> i64 {
    if nptr.is_null() {
        if !endptr.is_null() {
            unsafe { *endptr = nptr; }
        }
        return 0;
    }

    let mut i: usize = 0;

    // Skip whitespace.
    while unsafe { *nptr.add(i) } != 0 && wc_is_space(unsafe { *nptr.add(i) }) {
        i = i.wrapping_add(1);
    }

    // Handle sign.
    let negative = unsafe { *nptr.add(i) } == 0x2d; // '-'
    if negative || unsafe { *nptr.add(i) } == 0x2b { // '+'
        i = i.wrapping_add(1);
    }

    // Auto-detect or skip prefix.
    if base == 0 {
        if unsafe { *nptr.add(i) } == 0x30 { // '0'
            let next = unsafe { *nptr.add(i.wrapping_add(1)) };
            if next == 0x78 || next == 0x58 { // 'x' or 'X'
                // Only commit to hex if a valid hex digit follows "0x".
                let after_x = unsafe { *nptr.add(i.wrapping_add(2)) };
                if wc_digit(after_x, 16) >= 0 {
                    base = 16;
                    i = i.wrapping_add(2);
                } else {
                    // "0x" with no hex digit: parse "0" as octal.
                    base = 8;
                    // Don't advance i — let the digit loop consume '0'.
                }
            } else {
                base = 8;
                // Don't advance i — let the digit loop consume '0'
                // so it counts as a parsed digit (strtol("09", &e, 0)
                // should return 0 with e pointing to "9").
            }
        } else {
            base = 10;
        }
    } else if base == 16 && unsafe { *nptr.add(i) } == 0x30 {
        let next = unsafe { *nptr.add(i.wrapping_add(1)) };
        if next == 0x78 || next == 0x58 {
            // Only skip "0x" if a hex digit follows (glibc compat).
            let after_x = unsafe { *nptr.add(i.wrapping_add(2)) };
            if wc_digit(after_x, 16) >= 0 {
                i = i.wrapping_add(2);
            }
        }
    }

    // Parse digits into u64 for full range coverage.
    // Accumulating as i64 cannot represent i64::MIN's absolute value
    // (9223372036854775808 > i64::MAX), so we parse unsigned and convert.
    let start = i;
    let mut acc: u64 = 0;
    let mut overflowed = false;
    loop {
        let wc = unsafe { *nptr.add(i) };
        if wc == 0 {
            break;
        }
        let d = wc_digit(wc, base);
        if d < 0 {
            break;
        }
        match acc.checked_mul(base as u64).and_then(|a| a.checked_add(d as u64)) {
            Some(v) => acc = v,
            None => overflowed = true,
        }
        i = i.wrapping_add(1);
    }

    if !endptr.is_null() {
        // If no digits were consumed, endptr points to nptr (original start).
        if i == start {
            unsafe { *endptr = nptr; }
        } else {
            unsafe { *endptr = nptr.add(i); }
        }
    }

    // Convert u64 accumulator to signed result with overflow detection.
    // i64::MIN magnitude (9223372036854775808) is (i64::MAX as u64) + 1.
    if overflowed {
        crate::errno::set_errno(crate::errno::ERANGE);
        if negative { i64::MIN } else { i64::MAX }
    } else if negative {
        let min_mag = (i64::MAX as u64).wrapping_add(1); // 9223372036854775808
        if acc > min_mag {
            crate::errno::set_errno(crate::errno::ERANGE);
            i64::MIN
        } else if acc == min_mag {
            i64::MIN // Exactly representable.
        } else {
            // acc <= i64::MAX, safe to cast and negate.
            -(acc as i64)
        }
    } else {
        if acc > i64::MAX as u64 {
            crate::errno::set_errno(crate::errno::ERANGE);
            i64::MAX
        } else {
            acc as i64
        }
    }
}

/// Convert a `wchar_t` string to `uintmax_t`.
///
/// Like `wcstoimax` but parses an unsigned value.
///
/// # Safety
///
/// `nptr` must point to a valid null-terminated wide string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn wcstoumax(
    nptr: *const i32,
    endptr: *mut *const i32,
    mut base: i32,
) -> u64 {
    if nptr.is_null() {
        if !endptr.is_null() {
            unsafe { *endptr = nptr; }
        }
        return 0;
    }

    let mut i: usize = 0;

    // Skip whitespace.
    while unsafe { *nptr.add(i) } != 0 && wc_is_space(unsafe { *nptr.add(i) }) {
        i = i.wrapping_add(1);
    }

    // Handle sign.  POSIX: if the subject sequence begins with a
    // minus sign, the resulting value is negated (wrapping).
    let negative = unsafe { *nptr.add(i) } == 0x2d; // '-'
    if negative || unsafe { *nptr.add(i) } == 0x2b { // '+'
        i = i.wrapping_add(1);
    }

    // Auto-detect or skip prefix.
    if base == 0 {
        if unsafe { *nptr.add(i) } == 0x30 {
            let next = unsafe { *nptr.add(i.wrapping_add(1)) };
            if next == 0x78 || next == 0x58 {
                // Only commit to hex if a valid hex digit follows "0x".
                let after_x = unsafe { *nptr.add(i.wrapping_add(2)) };
                if wc_digit(after_x, 16) >= 0 {
                    base = 16;
                    i = i.wrapping_add(2);
                } else {
                    base = 8;
                }
            } else {
                base = 8;
                // Don't advance i — let the digit loop consume '0'.
            }
        } else {
            base = 10;
        }
    } else if base == 16 && unsafe { *nptr.add(i) } == 0x30 {
        let next = unsafe { *nptr.add(i.wrapping_add(1)) };
        if next == 0x78 || next == 0x58 {
            let after_x = unsafe { *nptr.add(i.wrapping_add(2)) };
            if wc_digit(after_x, 16) >= 0 {
                i = i.wrapping_add(2);
            }
        }
    }

    // Parse digits.
    let start = i;
    let mut result: u64 = 0;
    let mut overflowed = false;
    loop {
        let wc = unsafe { *nptr.add(i) };
        if wc == 0 {
            break;
        }
        let d = wc_digit(wc, base);
        if d < 0 {
            break;
        }
        match result.checked_mul(base as u64).and_then(|a| a.checked_add(d as u64)) {
            Some(v) => result = v,
            None => overflowed = true,
        }
        i = i.wrapping_add(1);
    }

    if !endptr.is_null() {
        if i == start {
            unsafe { *endptr = nptr; }
        } else {
            unsafe { *endptr = nptr.add(i); }
        }
    }

    if overflowed {
        crate::errno::set_errno(crate::errno::ERANGE);
        return u64::MAX;
    }

    // POSIX: negative sign means wrapping negation of the unsigned result.
    if negative { result.wrapping_neg() } else { result }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_imaxabs() {
        assert_eq!(imaxabs(42), 42);
        assert_eq!(imaxabs(-42), 42);
        assert_eq!(imaxabs(0), 0);
    }

    #[test]
    fn test_imaxdiv() {
        let r = imaxdiv(17, 5);
        assert_eq!(r.quot, 3);
        assert_eq!(r.rem, 2);
    }

    #[test]
    fn test_imaxdiv_zero() {
        let r = imaxdiv(42, 0);
        assert_eq!(r.quot, 0);
        assert_eq!(r.rem, 0);
    }

    #[test]
    fn test_wcstoimax_decimal() {
        let s: [i32; 5] = [0x20, 0x34, 0x32, 0x00, 0x00]; // " 42\0"
        let mut end: *const i32 = core::ptr::null();
        let v = unsafe { wcstoimax(s.as_ptr(), &mut end, 10) };
        assert_eq!(v, 42);
        assert_eq!(end, unsafe { s.as_ptr().add(3) });
    }

    #[test]
    fn test_wcstoimax_negative() {
        let s: [i32; 4] = [0x2d, 0x31, 0x30, 0x00]; // "-10\0"
        let v = unsafe { wcstoimax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, -10);
    }

    #[test]
    fn test_wcstoimax_hex_auto() {
        // "0xff\0"
        let s: [i32; 5] = [0x30, 0x78, 0x66, 0x66, 0x00];
        let v = unsafe { wcstoimax(s.as_ptr(), core::ptr::null_mut(), 0) };
        assert_eq!(v, 255);
    }

    #[test]
    fn test_wcstoumax_hex() {
        // "0x1A\0"
        let s: [i32; 5] = [0x30, 0x78, 0x31, 0x41, 0x00];
        let v = unsafe { wcstoumax(s.as_ptr(), core::ptr::null_mut(), 16) };
        assert_eq!(v, 26);
    }

    #[test]
    fn test_wcstoimax_octal() {
        // "077\0"
        let s: [i32; 4] = [0x30, 0x37, 0x37, 0x00];
        let v = unsafe { wcstoimax(s.as_ptr(), core::ptr::null_mut(), 0) };
        assert_eq!(v, 63);
    }

    #[test]
    fn test_wcstoumax_no_digits() {
        // "abc\0" (no valid digits for base 10)
        let s: [i32; 4] = [0x61, 0x62, 0x63, 0x00];
        let mut end: *const i32 = core::ptr::null();
        let v = unsafe { wcstoumax(s.as_ptr(), &mut end, 10) };
        assert_eq!(v, 0);
        // endptr should point to start (no conversion).
        assert_eq!(end, s.as_ptr());
    }

    // -----------------------------------------------------------------------
    // wcstoimax edge cases: i64::MIN, overflow, ERANGE
    // -----------------------------------------------------------------------

    #[test]
    fn test_wcstoimax_i64_min() {
        // "-9223372036854775808\0" — exactly i64::MIN
        let s: [i32; 21] = [
            0x2d, // '-'
            0x39, 0x32, 0x32, 0x33, 0x33, 0x37, 0x32, 0x30, // 92233720
            0x33, 0x36, 0x38, 0x35, 0x34, 0x37, 0x37, 0x35, // 36854775
            0x38, 0x30, 0x38, // 808
            0x00,
        ];
        crate::errno::set_errno(0);
        let v = unsafe { wcstoimax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, i64::MIN);
        // Not an overflow — should NOT set ERANGE.
        assert_eq!(crate::errno::get_errno(), 0);
    }

    #[test]
    fn test_wcstoimax_i64_max() {
        // "9223372036854775807\0" — exactly i64::MAX
        let s: [i32; 20] = [
            0x39, 0x32, 0x32, 0x33, 0x33, 0x37, 0x32, 0x30, // 92233720
            0x33, 0x36, 0x38, 0x35, 0x34, 0x37, 0x37, 0x35, // 36854775
            0x38, 0x30, 0x37, // 807
            0x00,
        ];
        crate::errno::set_errno(0);
        let v = unsafe { wcstoimax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, i64::MAX);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    #[test]
    fn test_wcstoimax_positive_overflow_sets_erange() {
        // "9223372036854775808\0" — one past i64::MAX
        let s: [i32; 20] = [
            0x39, 0x32, 0x32, 0x33, 0x33, 0x37, 0x32, 0x30, // 92233720
            0x33, 0x36, 0x38, 0x35, 0x34, 0x37, 0x37, 0x35, // 36854775
            0x38, 0x30, 0x38, // 808
            0x00,
        ];
        crate::errno::set_errno(0);
        let v = unsafe { wcstoimax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, i64::MAX);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);
    }

    #[test]
    fn test_wcstoimax_negative_overflow_sets_erange() {
        // "-9223372036854775809\0" — one past i64::MIN magnitude
        let s: [i32; 21] = [
            0x2d, // '-'
            0x39, 0x32, 0x32, 0x33, 0x33, 0x37, 0x32, 0x30, // 92233720
            0x33, 0x36, 0x38, 0x35, 0x34, 0x37, 0x37, 0x35, // 36854775
            0x38, 0x30, 0x39, // 809
            0x00,
        ];
        crate::errno::set_errno(0);
        let v = unsafe { wcstoimax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, i64::MIN);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);
    }

    #[test]
    fn test_wcstoimax_massive_overflow() {
        // "99999999999999999999\0" — way past u64::MAX, digit overflow
        let s: [i32; 21] = [
            0x39, 0x39, 0x39, 0x39, 0x39, 0x39, 0x39, 0x39, 0x39, 0x39,
            0x39, 0x39, 0x39, 0x39, 0x39, 0x39, 0x39, 0x39, 0x39, 0x39,
            0x00,
        ];
        crate::errno::set_errno(0);
        let v = unsafe { wcstoimax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, i64::MAX);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);
    }

    // -----------------------------------------------------------------------
    // wcstoumax edge cases: overflow, ERANGE, wrapping negation
    // -----------------------------------------------------------------------

    #[test]
    fn test_wcstoumax_u64_max() {
        // "18446744073709551615\0" — exactly u64::MAX
        let s: [i32; 21] = [
            0x31, 0x38, 0x34, 0x34, 0x36, 0x37, 0x34, 0x34, 0x30, 0x37, // 1844674407
            0x33, 0x37, 0x30, 0x39, 0x35, 0x35, 0x31, 0x36, 0x31, 0x35, // 3709551615
            0x00,
        ];
        crate::errno::set_errno(0);
        let v = unsafe { wcstoumax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, u64::MAX);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    #[test]
    fn test_wcstoumax_overflow_sets_erange() {
        // "18446744073709551616\0" — one past u64::MAX
        let s: [i32; 21] = [
            0x31, 0x38, 0x34, 0x34, 0x36, 0x37, 0x34, 0x34, 0x30, 0x37, // 1844674407
            0x33, 0x37, 0x30, 0x39, 0x35, 0x35, 0x31, 0x36, 0x31, 0x36, // 3709551616
            0x00,
        ];
        crate::errno::set_errno(0);
        let v = unsafe { wcstoumax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, u64::MAX);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);
    }

    #[test]
    fn test_wcstoumax_negative_wraps() {
        // "-1\0" — POSIX says wrapping negation: 0u64.wrapping_sub(1) = u64::MAX
        let s: [i32; 3] = [0x2d, 0x31, 0x00];
        crate::errno::set_errno(0);
        let v = unsafe { wcstoumax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, u64::MAX);
        // Wrapping negation is NOT an error.
        assert_eq!(crate::errno::get_errno(), 0);
    }

    #[test]
    fn test_wcstoumax_negative_two_wraps() {
        // "-2\0" — wraps to u64::MAX - 1
        let s: [i32; 3] = [0x2d, 0x32, 0x00];
        let v = unsafe { wcstoumax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, u64::MAX - 1);
    }

    // -----------------------------------------------------------------------
    // imaxabs / imaxdiv edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_imaxabs_i64_min() {
        // i64::MIN.wrapping_neg() = i64::MIN (two's complement).
        // This is defined behavior for imaxabs per POSIX (undefined in C
        // standard, but we use wrapping_neg so it's deterministic).
        let r = imaxabs(i64::MIN);
        assert_eq!(r, i64::MIN); // wrapping semantics
    }

    #[test]
    fn test_imaxdiv_negative() {
        let r = imaxdiv(-17, 5);
        assert_eq!(r.quot, -3);
        assert_eq!(r.rem, -2);
    }

    #[test]
    fn test_imaxdiv_i64_min_by_minus_one() {
        // Division overflow: i64::MIN / -1 can't be represented in i64.
        // POSIX leaves this undefined, but we must not crash.
        // Our implementation uses wrapping division → quot = i64::MIN, rem = 0.
        let r = imaxdiv(i64::MIN, -1);
        assert_eq!(r.quot, i64::MIN);
        assert_eq!(r.rem, 0);
    }

    // -----------------------------------------------------------------------
    // strtoimax / strtoumax — delegation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_strtoimax_decimal() {
        let s = b"12345\0";
        let mut end: *const u8 = core::ptr::null();
        let v = unsafe { strtoimax(s.as_ptr(), &mut end, 10) };
        assert_eq!(v, 12345);
        // endptr should point past the last digit.
        assert_eq!(unsafe { *end }, 0);
    }

    #[test]
    fn test_strtoimax_negative() {
        let s = b"-9876\0";
        let v = unsafe { strtoimax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, -9876);
    }

    #[test]
    fn test_strtoimax_hex() {
        let s = b"0xFF\0";
        let v = unsafe { strtoimax(s.as_ptr(), core::ptr::null_mut(), 0) };
        assert_eq!(v, 255);
    }

    #[test]
    fn test_strtoimax_max() {
        let s = b"9223372036854775807\0";
        let v = unsafe { strtoimax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, i64::MAX);
    }

    #[test]
    fn test_strtoumax_decimal() {
        let s = b"42\0";
        let mut end: *const u8 = core::ptr::null();
        let v = unsafe { strtoumax(s.as_ptr(), &mut end, 10) };
        assert_eq!(v, 42);
        assert_eq!(unsafe { *end }, 0);
    }

    #[test]
    fn test_strtoumax_hex() {
        let s = b"0xDEAD\0";
        let v = unsafe { strtoumax(s.as_ptr(), core::ptr::null_mut(), 0) };
        assert_eq!(v, 0xDEAD);
    }

    #[test]
    fn test_strtoumax_max() {
        let s = b"18446744073709551615\0";
        let v = unsafe { strtoumax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, u64::MAX);
    }

    #[test]
    fn test_strtoumax_octal() {
        let s = b"0777\0";
        let v = unsafe { strtoumax(s.as_ptr(), core::ptr::null_mut(), 0) };
        assert_eq!(v, 0o777);
    }

    // -----------------------------------------------------------------------
    // wcstoimax / wcstoumax — additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_wcstoimax_null_nptr() {
        let mut end: *const i32 = 0x1234 as *const i32; // garbage initial
        let v = unsafe { wcstoimax(core::ptr::null(), &mut end, 10) };
        assert_eq!(v, 0);
        assert!(end.is_null(), "endptr should be set to nptr (null)");
    }

    #[test]
    fn test_wcstoumax_null_nptr() {
        let mut end: *const i32 = 0x1234 as *const i32;
        let v = unsafe { wcstoumax(core::ptr::null(), &mut end, 10) };
        assert_eq!(v, 0);
        assert!(end.is_null(), "endptr should be set to nptr (null)");
    }

    #[test]
    fn test_wcstoimax_base_16_explicit() {
        // "1A\0" with explicit base 16.
        let s: [i32; 3] = [0x31, 0x41, 0x00]; // '1', 'A', NUL
        let v = unsafe { wcstoimax(s.as_ptr(), core::ptr::null_mut(), 16) };
        assert_eq!(v, 26);
    }

    #[test]
    fn test_wcstoumax_base_2() {
        // "1010\0" in base 2 = 10.
        let s: [i32; 5] = [0x31, 0x30, 0x31, 0x30, 0x00];
        let v = unsafe { wcstoumax(s.as_ptr(), core::ptr::null_mut(), 2) };
        assert_eq!(v, 10);
    }

    #[test]
    fn test_wcstoimax_leading_whitespace() {
        // "  42\0" — whitespace then digits.
        let s: [i32; 5] = [0x20, 0x20, 0x34, 0x32, 0x00]; // ' ', ' ', '4', '2', NUL
        let v = unsafe { wcstoimax(s.as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, 42);
    }
}
