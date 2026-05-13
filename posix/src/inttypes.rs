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
    #[allow(clippy::arithmetic_side_effects)]
    ImaxdivT {
        quot: numer / denom,
        rem: numer % denom,
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

    // Parse digits.
    let start = i;
    let mut result: i64 = 0;
    loop {
        let wc = unsafe { *nptr.add(i) };
        if wc == 0 {
            break;
        }
        let d = wc_digit(wc, base);
        if d < 0 {
            break;
        }
        result = result.saturating_mul(i64::from(base)).saturating_add(i64::from(d));
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

    if negative { result.saturating_neg() } else { result }
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
    loop {
        let wc = unsafe { *nptr.add(i) };
        if wc == 0 {
            break;
        }
        let d = wc_digit(wc, base);
        if d < 0 {
            break;
        }
        result = result.saturating_mul(base as u64).saturating_add(d as u64);
        i = i.wrapping_add(1);
    }

    if !endptr.is_null() {
        if i == start {
            unsafe { *endptr = nptr; }
        } else {
            unsafe { *endptr = nptr.add(i); }
        }
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
}
