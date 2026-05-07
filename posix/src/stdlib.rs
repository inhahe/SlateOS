//! C standard library conversion functions.
//!
//! Implements `atoi`, `atol`, `strtol`, `strtoul`, `abs`, `labs`.
//!
//! These are not strictly POSIX but are required by virtually every
//! C program and are part of the C standard library.


// ---------------------------------------------------------------------------
// Integer conversion
// ---------------------------------------------------------------------------

/// Convert a C string to an integer.
///
/// Skips leading whitespace, handles optional sign.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atoi(nptr: *const u8) -> i32 {
    unsafe { strtol(nptr, core::ptr::null_mut(), 10) as i32 }
}

/// Convert a C string to a long integer.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atol(nptr: *const u8) -> i64 {
    unsafe { strtol(nptr, core::ptr::null_mut(), 10) }
}

/// Convert a C string to a long integer with base and end pointer.
///
/// Skips leading whitespace, handles optional `+`/`-` sign, and
/// supports bases 2-36.  Base 0 auto-detects: `0x` = hex, `0` = octal,
/// else decimal.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
/// `endptr` may be null; if non-null, it receives a pointer to the
/// first character after the parsed number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtol(
    nptr: *const u8,
    endptr: *mut *const u8,
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
    while is_space(unsafe { *nptr.add(i) }) {
        i = i.wrapping_add(1);
    }

    // Handle sign.
    let negative = unsafe { *nptr.add(i) } == b'-';
    if negative || unsafe { *nptr.add(i) } == b'+' {
        i = i.wrapping_add(1);
    }

    // Auto-detect base.
    if base == 0 {
        if unsafe { *nptr.add(i) } == b'0' {
            if unsafe { *nptr.add(i.wrapping_add(1)) } == b'x'
                || unsafe { *nptr.add(i.wrapping_add(1)) } == b'X'
            {
                base = 16;
                i = i.wrapping_add(2);
            } else {
                base = 8;
                i = i.wrapping_add(1);
            }
        } else {
            base = 10;
        }
    } else if base == 16
        && unsafe { *nptr.add(i) } == b'0'
        && (unsafe { *nptr.add(i.wrapping_add(1)) } == b'x'
            || unsafe { *nptr.add(i.wrapping_add(1)) } == b'X')
    {
        // Skip optional 0x prefix for hex.
        i = i.wrapping_add(2);
    }

    // Parse digits.
    let mut result: i64 = 0;
    loop {
        let c = unsafe { *nptr.add(i) };
        let digit = char_to_digit(c, base);
        if digit < 0 {
            break;
        }
        // Saturating to avoid overflow UB.
        result = result.saturating_mul(i64::from(base)).saturating_add(i64::from(digit));
        i = i.wrapping_add(1);
    }

    if !endptr.is_null() {
        unsafe { *endptr = nptr.add(i); }
    }

    if negative { result.saturating_neg() } else { result }
}

/// Convert a C string to an unsigned long integer.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtoul(
    nptr: *const u8,
    endptr: *mut *const u8,
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
    while is_space(unsafe { *nptr.add(i) }) {
        i = i.wrapping_add(1);
    }

    // Skip optional '+'.
    if unsafe { *nptr.add(i) } == b'+' {
        i = i.wrapping_add(1);
    }

    // Auto-detect base.
    if base == 0 {
        if unsafe { *nptr.add(i) } == b'0' {
            if unsafe { *nptr.add(i.wrapping_add(1)) } == b'x'
                || unsafe { *nptr.add(i.wrapping_add(1)) } == b'X'
            {
                base = 16;
                i = i.wrapping_add(2);
            } else {
                base = 8;
                i = i.wrapping_add(1);
            }
        } else {
            base = 10;
        }
    } else if base == 16
        && unsafe { *nptr.add(i) } == b'0'
        && (unsafe { *nptr.add(i.wrapping_add(1)) } == b'x'
            || unsafe { *nptr.add(i.wrapping_add(1)) } == b'X')
    {
        i = i.wrapping_add(2);
    }

    // Parse digits.
    let mut result: u64 = 0;
    loop {
        let c = unsafe { *nptr.add(i) };
        let digit = char_to_digit(c, base);
        if digit < 0 {
            break;
        }
        result = result.saturating_mul(base as u64).saturating_add(digit as u64);
        i = i.wrapping_add(1);
    }

    if !endptr.is_null() {
        unsafe { *endptr = nptr.add(i); }
    }

    result
}

// ---------------------------------------------------------------------------
// Absolute value
// ---------------------------------------------------------------------------

/// Compute absolute value of an integer.
#[unsafe(no_mangle)]
pub extern "C" fn abs(j: i32) -> i32 {
    if j < 0 { j.saturating_neg() } else { j }
}

/// Compute absolute value of a long integer.
#[unsafe(no_mangle)]
pub extern "C" fn labs(j: i64) -> i64 {
    if j < 0 { j.saturating_neg() } else { j }
}

// ---------------------------------------------------------------------------
// Character classification (internal helpers)
// ---------------------------------------------------------------------------

/// Check if a byte is ASCII whitespace.
#[inline]
#[must_use]
const fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Convert an ASCII character to its digit value in a given base.
///
/// Returns -1 if the character is not a valid digit for the base.
#[inline]
#[must_use]
fn char_to_digit(c: u8, base: i32) -> i32 {
    let val = match c {
        b'0'..=b'9' => i32::from(c.wrapping_sub(b'0')),
        b'a'..=b'z' => i32::from(c.wrapping_sub(b'a')).wrapping_add(10),
        b'A'..=b'Z' => i32::from(c.wrapping_sub(b'A')).wrapping_add(10),
        _ => return -1,
    };
    if val < base { val } else { -1 }
}
