//! C standard library conversion functions.
//!
//! Implements integer and floating-point conversion, absolute value,
//! integer division structs, sorting, searching, random numbers, and
//! temporary file creation.
//!
//! ## Functions
//!
//! - `atoi`, `atol` — quick string→integer
//! - `strtol`, `strtoul`, `strtoll`, `strtoull` — full string→integer
//! - `strtod`, `strtof`, `strtold` — string→floating-point
//! - `abs`, `labs`, `llabs` — absolute value
//! - `div`, `ldiv`, `lldiv` — integer division with quotient/remainder
//! - `qsort`, `bsearch` — array sorting/searching
//! - `srand`, `rand`, `rand_r` — pseudo-random numbers
//! - `mkstemp`, `tmpfile` — temporary file creation
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn atoi(nptr: *const u8) -> i32 {
    unsafe { strtol(nptr, core::ptr::null_mut(), 10) as i32 }
}

/// Convert a C string to a long integer.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn atol(nptr: *const u8) -> i64 {
    unsafe { strtol(nptr, core::ptr::null_mut(), 10) }
}

/// Convert a C string to a long long integer.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn atoll(nptr: *const u8) -> i64 {
    unsafe { strtoll(nptr, core::ptr::null_mut(), 10) }
}

/// Convert a C string to a long integer with base and end pointer.
///
/// Skips leading whitespace, handles optional `+`/`-` sign, and
/// supports bases 2-36.  Base 0 auto-detects: `0x` = hex, `0` = octal,
/// else decimal.
///
/// On overflow, sets errno to ERANGE and returns `i64::MAX` (positive
/// overflow) or `i64::MIN` (negative overflow).  `endptr` still points
/// past the last valid digit.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
/// `endptr` may be null; if non-null, it receives a pointer to the
/// first character after the parsed number.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtol(
    nptr: *const u8,
    endptr: *mut *const u8,
    mut base: i32,
) -> i64 {
    // Check range and apply sign.
    // i64::MIN magnitude as u64 = 2^63 = (i64::MAX as u64) + 1.
    const POS_MAX: u64 = i64::MAX as u64;
    const NEG_MAX: u64 = POS_MAX.wrapping_add(1); // 2^63

    if nptr.is_null() {
        if !endptr.is_null() {
            unsafe { *endptr = nptr; }
        }
        return 0;
    }

    // POSIX: base must be 0 or in [2, 36].
    if base != 0 && !(2..=36).contains(&base) {
        crate::errno::set_errno(crate::errno::EINVAL);
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

    // Save position before prefix to restore if no digits follow "0x".
    let before_prefix = i;

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

    // Parse digits, accumulating as u64 to handle the full signed range
    // (i64::MIN's magnitude exceeds i64::MAX by 1).
    let base_u = base as u64;
    let mut result: u64 = 0;
    let mut overflow = false;
    let mut any_digits = false;

    loop {
        let c = unsafe { *nptr.add(i) };
        let digit = char_to_digit(c, base);
        if digit < 0 {
            break;
        }
        any_digits = true;
        // Detect overflow via checked arithmetic.
        if let Some(r) = result.checked_mul(base_u) {
            if let Some(r2) = r.checked_add(digit as u64) {
                result = r2;
            } else {
                overflow = true;
            }
        } else {
            overflow = true;
        }
        i = i.wrapping_add(1);
    }

    // If no digits were parsed after "0x" prefix, the "0" is still a
    // valid digit (it's an octal/hex zero).  Set endptr past the "0"
    // but don't consume the "x".
    if !any_digits && i != before_prefix {
        // before_prefix points at the '0'.  Advance 1 past it.
        i = before_prefix.wrapping_add(1);
        any_digits = true;
        // result stays 0.
    }

    if !endptr.is_null() {
        // POSIX: if no conversion performed, endptr = nptr.
        if any_digits {
            unsafe { *endptr = nptr.add(i); }
        } else {
            unsafe { *endptr = nptr; }
        }
    }

    if !any_digits {
        return 0;
    }

    if overflow {
        crate::errno::set_errno(crate::errno::ERANGE);
        return if negative { i64::MIN } else { i64::MAX };
    }

    if negative {
        #[allow(clippy::arithmetic_side_effects)]
        match result.cmp(&NEG_MAX) {
            core::cmp::Ordering::Greater => {
                crate::errno::set_errno(crate::errno::ERANGE);
                i64::MIN
            }
            core::cmp::Ordering::Equal => i64::MIN,
            // SAFETY: result <= i64::MAX, so cast is safe; then negate.
            core::cmp::Ordering::Less => -(result as i64),
        }
    } else if result > POS_MAX {
        crate::errno::set_errno(crate::errno::ERANGE);
        i64::MAX
    } else {
        result as i64
    }
}

/// Convert a C string to an unsigned long integer.
///
/// POSIX: if the subject sequence begins with a minus sign, the value
/// resulting from the conversion is negated (wrapping to the unsigned
/// range).  So `strtoul("-1", NULL, 10)` returns `ULONG_MAX`.
///
/// On overflow, sets errno to ERANGE and returns `u64::MAX`.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

    // POSIX: base must be 0 or in [2, 36].
    if base != 0 && !(2..=36).contains(&base) {
        crate::errno::set_errno(crate::errno::EINVAL);
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

    // Handle optional sign.  POSIX: a leading '-' negates the result
    // in the unsigned domain (wrapping).
    let negative = unsafe { *nptr.add(i) } == b'-';
    if negative || unsafe { *nptr.add(i) } == b'+' {
        i = i.wrapping_add(1);
    }

    let before_prefix = i;

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
    let base_u = base as u64;
    let mut result: u64 = 0;
    let mut overflow = false;
    let mut any_digits = false;

    loop {
        let c = unsafe { *nptr.add(i) };
        let digit = char_to_digit(c, base);
        if digit < 0 {
            break;
        }
        any_digits = true;
        if let Some(r) = result.checked_mul(base_u) {
            if let Some(r2) = r.checked_add(digit as u64) {
                result = r2;
            } else {
                overflow = true;
            }
        } else {
            overflow = true;
        }
        i = i.wrapping_add(1);
    }

    // Same "0x" rollback as strtol: the "0" is a valid digit.
    if !any_digits && i != before_prefix {
        i = before_prefix.wrapping_add(1);
        any_digits = true;
    }

    if !endptr.is_null() {
        if any_digits {
            unsafe { *endptr = nptr.add(i); }
        } else {
            unsafe { *endptr = nptr; }
        }
    }

    if !any_digits {
        return 0;
    }

    if overflow {
        crate::errno::set_errno(crate::errno::ERANGE);
        return u64::MAX;
    }

    // POSIX: negate in the unsigned domain for '-' prefix.
    if negative { result.wrapping_neg() } else { result }
}

/// Convert a C string to a long long integer (`strtoll`).
///
/// Identical to `strtol` — on our platform `long long` and `long`
/// are both 64-bit.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtoll(
    nptr: *const u8,
    endptr: *mut *const u8,
    base: i32,
) -> i64 {
    unsafe { strtol(nptr, endptr, base) }
}

/// Convert a C string to an unsigned long long integer (`strtoull`).
///
/// Identical to `strtoul` — on our platform `unsigned long long` and
/// `unsigned long` are both 64-bit.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtoull(
    nptr: *const u8,
    endptr: *mut *const u8,
    base: i32,
) -> u64 {
    unsafe { strtoul(nptr, endptr, base) }
}

// ---------------------------------------------------------------------------
// Floating-point conversion
// ---------------------------------------------------------------------------

/// Convert a C string to a double (`strtod`).
///
/// Parses decimal floating-point strings of the form:
///   `[whitespace][sign]digits[.digits][e[sign]digits]`
///
/// Also supports `INF`, `INFINITY`, and `NAN` (case-insensitive).
/// Hex floats (`0x` prefix) are not currently supported.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects, clippy::too_many_lines)]
pub unsafe extern "C" fn strtod(
    nptr: *const u8,
    endptr: *mut *const u8,
) -> f64 {
    if nptr.is_null() {
        if !endptr.is_null() {
            unsafe { *endptr = nptr; }
        }
        return 0.0;
    }

    let mut i: usize = 0;

    // Skip whitespace.
    while is_space(unsafe { *nptr.add(i) }) {
        i = i.wrapping_add(1);
    }

    // Sign.
    let negative = unsafe { *nptr.add(i) } == b'-';
    if negative || unsafe { *nptr.add(i) } == b'+' {
        i = i.wrapping_add(1);
    }

    // Check for "inf", "infinity", "nan" (case-insensitive).
    // Important: check bytes one at a time to avoid reading past null
    // terminator (which could be at the end of a mapped page).
    let c0 = unsafe { *nptr.add(i) };
    if c0 == 0 {
        // Empty subject string — fall through to digit parsing.
    } else if (c0 | 0x20) == b'i' {
        // Possible "inf" or "infinity".
        let c1 = unsafe { *nptr.add(i.wrapping_add(1)) };
        if c1 != 0 {
            let c2 = unsafe { *nptr.add(i.wrapping_add(2)) };
            if c2 != 0 && (c1 | 0x20) == b'n' && (c2 | 0x20) == b'f' {
                i = i.wrapping_add(3);
                // Check for full "infinity" — one byte at a time.
                let inity: [u8; 5] = [b'i', b'n', b'i', b't', b'y'];
                let mut j: usize = 0;
                let mut all_match = true;
                while j < 5 {
                    let ch = unsafe { *nptr.add(i.wrapping_add(j)) };
                    let expected = inity.get(j).copied().unwrap_or(0);
                    if ch == 0 || (ch | 0x20) != expected {
                        all_match = false;
                        break;
                    }
                    j = j.wrapping_add(1);
                }
                if all_match {
                    i = i.wrapping_add(5);
                }
                if !endptr.is_null() {
                    unsafe { *endptr = nptr.add(i); }
                }
                return if negative { f64::NEG_INFINITY } else { f64::INFINITY };
            }
        }
    } else if (c0 | 0x20) == b'n' {
        let c1 = unsafe { *nptr.add(i.wrapping_add(1)) };
        if c1 != 0 {
            let c2 = unsafe { *nptr.add(i.wrapping_add(2)) };
            if c2 != 0 && (c1 | 0x20) == b'a' && (c2 | 0x20) == b'n' {
                i = i.wrapping_add(3);
                // Skip optional (chars) payload per C99.
                if unsafe { *nptr.add(i) } == b'(' {
                    let mut j = i.wrapping_add(1);
                    while unsafe { *nptr.add(j) } != 0 && unsafe { *nptr.add(j) } != b')' {
                        j = j.wrapping_add(1);
                    }
                    if unsafe { *nptr.add(j) } == b')' {
                        i = j.wrapping_add(1);
                    }
                }
                if !endptr.is_null() {
                    unsafe { *endptr = nptr.add(i); }
                }
                return f64::NAN;
            }
        }
    }

    // Integer part.
    let mut int_part: f64 = 0.0;
    let mut has_digits = false;
    while (unsafe { *nptr.add(i) }).is_ascii_digit() {
        int_part = int_part * 10.0 + f64::from(unsafe { *nptr.add(i) }.wrapping_sub(b'0'));
        i = i.wrapping_add(1);
        has_digits = true;
    }

    // Fractional part.
    let mut frac_part: f64 = 0.0;
    if unsafe { *nptr.add(i) } == b'.' {
        i = i.wrapping_add(1);
        let mut divisor: f64 = 10.0;
        while (unsafe { *nptr.add(i) }).is_ascii_digit() {
            frac_part += f64::from(unsafe { *nptr.add(i) }.wrapping_sub(b'0')) / divisor;
            divisor *= 10.0;
            i = i.wrapping_add(1);
            has_digits = true;
        }
    }

    // If no digits were parsed, endptr points to nptr (no conversion).
    if !has_digits {
        if !endptr.is_null() {
            unsafe { *endptr = nptr; }
        }
        return 0.0;
    }

    let mut result = int_part + frac_part;

    // Exponent part.  Save position before 'e' so we can restore if no
    // exponent digits follow (POSIX: 'e' without digits is not consumed).
    let c = unsafe { *nptr.add(i) };
    if c == b'e' || c == b'E' {
        let before_exp = i;
        i = i.wrapping_add(1);
        let exp_neg = unsafe { *nptr.add(i) } == b'-';
        if exp_neg || unsafe { *nptr.add(i) } == b'+' {
            i = i.wrapping_add(1);
        }

        if (unsafe { *nptr.add(i) }).is_ascii_digit() {
            let mut exp_val: i32 = 0;
            while (unsafe { *nptr.add(i) }).is_ascii_digit() {
                exp_val = exp_val
                    .saturating_mul(10)
                    .saturating_add(i32::from(unsafe { *nptr.add(i) }.wrapping_sub(b'0')));
                i = i.wrapping_add(1);
            }

            if exp_neg {
                exp_val = exp_val.saturating_neg();
            }

            result *= pow10(exp_val);
        } else {
            // No exponent digits — roll back.
            i = before_exp;
        }
    }

    if negative {
        result = -result;
    }

    // POSIX: set ERANGE on overflow (result is infinite) or underflow
    // (result rounds to zero but input was nonzero).
    if result.is_infinite() || (result == 0.0 && (int_part != 0.0 || frac_part != 0.0)) {
        crate::errno::set_errno(crate::errno::ERANGE);
    }

    if !endptr.is_null() {
        unsafe { *endptr = nptr.add(i); }
    }

    result
}

/// Convert a C string to a float (`strtof`).
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtof(
    nptr: *const u8,
    endptr: *mut *const u8,
) -> f32 {
    let d = unsafe { strtod(nptr, endptr) };
    let f = d as f32;
    // POSIX: set ERANGE if the f32 result overflows or underflows.
    // strtod already sets ERANGE for f64 overflow; we additionally check
    // for values that fit in f64 but not f32.
    if (f.is_infinite() && !d.is_infinite()) || (f == 0.0 && d != 0.0) {
        crate::errno::set_errno(crate::errno::ERANGE);
    }
    f
}

/// Convert a C string to a long double (`strtold`).
///
/// On x86_64, `long double` is 80-bit extended precision in the C ABI.
/// Rust does not have native 80-bit float support, so we delegate to
/// `strtod` (64-bit `f64`).  This loses precision for values that
/// require more than 53 bits of mantissa, but covers the vast majority
/// of real-world uses.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtold(
    nptr: *const u8,
    endptr: *mut *const u8,
) -> f64 {
    // SAFETY: strtod safety requirements are identical.
    unsafe { strtod(nptr, endptr) }
}

/// Convert a C string to a double (`atof`).
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn atof(nptr: *const u8) -> f64 {
    unsafe { strtod(nptr, core::ptr::null_mut()) }
}

/// Compute 10^exp using repeated multiplication.
///
/// Handles both positive and negative exponents.
#[allow(clippy::arithmetic_side_effects)]
fn pow10(mut exp: i32) -> f64 {
    if exp == 0 {
        return 1.0;
    }
    let neg = exp < 0;
    if neg {
        exp = exp.saturating_neg();
    }

    let mut result: f64 = 1.0;
    let mut base: f64 = 10.0;
    let mut e = exp;
    // Repeated squaring for efficiency.
    while e > 0 {
        if e & 1 == 1 {
            result *= base;
        }
        base *= base;
        e >>= 1;
    }

    if neg { 1.0 / result } else { result }
}

// ---------------------------------------------------------------------------
// Absolute value
// ---------------------------------------------------------------------------

/// Compute absolute value of an integer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn abs(j: i32) -> i32 {
    if j < 0 { j.saturating_neg() } else { j }
}

/// Compute absolute value of a long integer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn labs(j: i64) -> i64 {
    if j < 0 { j.saturating_neg() } else { j }
}

/// Compute absolute value of a long long integer.
///
/// On our platform `long long` = `i64`, same as `labs`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn llabs(j: i64) -> i64 {
    if j < 0 { j.saturating_neg() } else { j }
}

// ---------------------------------------------------------------------------
// Integer division
// ---------------------------------------------------------------------------

/// Result of integer division (`div_t`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DivT {
    /// Quotient.
    pub quot: i32,
    /// Remainder.
    pub rem: i32,
}

/// Result of long integer division (`ldiv_t`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LdivT {
    /// Quotient.
    pub quot: i64,
    /// Remainder.
    pub rem: i64,
}

/// Result of long long integer division (`lldiv_t`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LldivT {
    /// Quotient.
    pub quot: i64,
    /// Remainder.
    pub rem: i64,
}

/// Compute quotient and remainder simultaneously.
///
/// Division by zero returns `{ 0, 0 }` (C UB — we choose a safe
/// fallback).  `MIN / -1` returns `{ MIN, 0 }` (wrapping) instead of
/// panicking, matching the behavior of C on two's-complement hardware.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn div(numer: i32, denom: i32) -> DivT {
    if denom == 0 {
        return DivT { quot: 0, rem: 0 };
    }
    if numer == i32::MIN && denom == -1 {
        // Overflow: wrapping_div gives MIN (two's complement wrap).
        return DivT { quot: i32::MIN, rem: 0 };
    }
    #[allow(clippy::arithmetic_side_effects)]
    DivT {
        quot: numer / denom,
        rem: numer % denom,
    }
}

/// Compute quotient and remainder for long integers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ldiv(numer: i64, denom: i64) -> LdivT {
    if denom == 0 {
        return LdivT { quot: 0, rem: 0 };
    }
    if numer == i64::MIN && denom == -1 {
        return LdivT { quot: i64::MIN, rem: 0 };
    }
    #[allow(clippy::arithmetic_side_effects)]
    LdivT {
        quot: numer / denom,
        rem: numer % denom,
    }
}

/// Compute quotient and remainder for long long integers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lldiv(numer: i64, denom: i64) -> LldivT {
    if denom == 0 {
        return LldivT { quot: 0, rem: 0 };
    }
    if numer == i64::MIN && denom == -1 {
        return LldivT { quot: i64::MIN, rem: 0 };
    }
    #[allow(clippy::arithmetic_side_effects)]
    LldivT {
        quot: numer / denom,
        rem: numer % denom,
    }
}

// ---------------------------------------------------------------------------
// Sorting and searching
// ---------------------------------------------------------------------------

/// Sort an array using the comparison function.
///
/// This is a simple insertion sort — O(n²) but correct and compact.
/// A real libc would use introsort or merge sort.
///
/// # Safety
///
/// `base` must point to an array of at least `nmemb` elements, each
/// of `size` bytes.  `compar` must be a valid comparison function.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn qsort(
    base: *mut u8,
    nmemb: usize,
    size: usize,
    compar: unsafe extern "C" fn(*const u8, *const u8) -> i32,
) {
    if nmemb <= 1 || size == 0 {
        return;
    }

    // Insertion sort.  Simple, in-place, stable.
    // A 256-byte stack buffer avoids mmap for small elements.
    let mut swap_buf = [0u8; 256];
    let use_stack = size <= swap_buf.len();

    let temp = if use_stack {
        swap_buf.as_mut_ptr()
    } else {
        // Allocate temp space via mmap for large elements.
        let ptr = crate::mman::mmap(
            core::ptr::null_mut(),
            size,
            crate::mman::PROT_READ | crate::mman::PROT_WRITE,
            crate::mman::MAP_PRIVATE | crate::mman::MAP_ANONYMOUS,
            -1,
            0,
        );
        if ptr == crate::mman::MAP_FAILED {
            return; // Cannot sort without temp space.
        }
        ptr.cast::<u8>()
    };

    let mut i: usize = 1;
    while i < nmemb {
        // Save element[i] into temp.
        let elem_i = unsafe { base.add(i.wrapping_mul(size)) };
        unsafe { core::ptr::copy_nonoverlapping(elem_i, temp, size); }

        // Shift elements right until we find the insertion point.
        let mut j = i;
        while j > 0 {
            let elem_j_minus_1 = unsafe { base.add(j.wrapping_sub(1).wrapping_mul(size)) };
            if unsafe { compar(elem_j_minus_1, temp) } <= 0 {
                break;
            }
            let elem_j = unsafe { base.add(j.wrapping_mul(size)) };
            unsafe { core::ptr::copy_nonoverlapping(elem_j_minus_1, elem_j, size); }
            j = j.wrapping_sub(1);
        }

        // Insert the saved element at position j.
        let dest = unsafe { base.add(j.wrapping_mul(size)) };
        unsafe { core::ptr::copy_nonoverlapping(temp, dest, size); }

        i = i.wrapping_add(1);
    }

    if !use_stack {
        let _ = crate::mman::munmap(temp.cast::<core::ffi::c_void>(), size);
    }
}

/// Binary search a sorted array.
///
/// Returns a pointer to the matching element, or NULL if not found.
///
/// # Safety
///
/// `base` must point to a sorted array of at least `nmemb` elements,
/// each of `size` bytes.  `compar` must be a valid comparison function.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn bsearch(
    key: *const u8,
    base: *const u8,
    nmemb: usize,
    size: usize,
    compar: unsafe extern "C" fn(*const u8, *const u8) -> i32,
) -> *const u8 {
    if nmemb == 0 || size == 0 {
        return core::ptr::null();
    }

    let mut lo: usize = 0;
    let mut hi: usize = nmemb;

    while lo < hi {
        let mid = lo.wrapping_add(hi.wrapping_sub(lo) / 2);
        let elem = unsafe { base.add(mid.wrapping_mul(size)) };
        let cmp = unsafe { compar(key, elem) };
        match cmp.cmp(&0) {
            core::cmp::Ordering::Less => hi = mid,
            core::cmp::Ordering::Greater => lo = mid.wrapping_add(1),
            core::cmp::Ordering::Equal => return elem,
        }
    }

    core::ptr::null()
}

// ---------------------------------------------------------------------------
// Random number generation
// ---------------------------------------------------------------------------

/// Linear congruential PRNG state.
///
/// Not thread-safe. Uses the glibc LCG parameters.
static mut RAND_STATE: u64 = 1;

/// Seed the random number generator.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn srand(seed: u32) {
    // SAFETY: Single-threaded userspace. Using addr_of_mut for Rust 2024.
    unsafe { core::ptr::addr_of_mut!(RAND_STATE).write(u64::from(seed)); }
}

/// Generate a pseudo-random integer in [0, RAND_MAX].
///
/// Uses the glibc LCG: state = state * 6364136223846793005 + 1.
/// Returns the upper 31 bits.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn rand() -> i32 {
    // SAFETY: Single-threaded access.
    let state = unsafe { core::ptr::addr_of_mut!(RAND_STATE).read() };
    let new_state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1);
    unsafe { core::ptr::addr_of_mut!(RAND_STATE).write(new_state); }
    // Return upper 31 bits as a non-negative i32.
    ((new_state >> 33) & 0x7FFF_FFFF) as i32
}

/// Thread-safe pseudo-random number generator.
///
/// Uses caller-provided state instead of the global `RAND_STATE`.
/// The algorithm matches glibc's LCG for compatibility.
///
/// # Safety
///
/// `seed` must point to a valid `u32`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn rand_r(seed: *mut u32) -> i32 {
    if seed.is_null() {
        return 0;
    }
    // Use a 32-bit LCG: state = state * 1103515245 + 12345 (POSIX spec).
    let state = unsafe { *seed };
    let new_state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    unsafe { *seed = new_state; }
    // Return upper bits as a non-negative i32.
    ((new_state >> 1) & 0x7FFF_FFFF) as i32
}

/// Maximum value returned by rand().
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static RAND_MAX: i32 = 0x7FFF_FFFF;

/// POSIX: Seed the better random number generator.
///
/// For our purposes, this is identical to `srand`.  POSIX specifies
/// `random()`/`srandom()` as a better-quality RNG than `rand()`/`srand()`,
/// but our implementation uses the same LCG for both.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn srandom(seed: u32) {
    srand(seed);
}

/// POSIX: Generate a pseudo-random integer in [0, 2^31).
///
/// Better-quality RNG than `rand()` per POSIX, but our implementation
/// delegates to the same LCG.  Returns a `i64` (`long`) per POSIX.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn random() -> i64 {
    i64::from(rand())
}

/// POSIX: Initialize random state for `random_r`.
///
/// Stub — stores the seed in the state buffer for compatibility.
///
/// # Safety
///
/// `statebuf` must be a valid pointer to at least 8 bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn initstate(seed: u32, statebuf: *mut u8, n: usize) -> *mut u8 {
    if statebuf.is_null() || n < 8 {
        return core::ptr::null_mut();
    }
    srand(seed);
    statebuf
}

/// POSIX: Set the random state buffer.
///
/// Stub — accepts the state pointer for API compatibility.
///
/// # Safety
///
/// `statebuf` must have been returned by a prior `initstate` call.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn setstate(statebuf: *mut u8) -> *mut u8 {
    // No-op: we use a global state regardless.
    statebuf
}

// ---------------------------------------------------------------------------
// Temporary files
// ---------------------------------------------------------------------------


/// Create a unique temporary file.
///
/// The `template` string must end with exactly six 'X' characters
/// (e.g., `"/tmp/fileXXXXXX"`).  These are replaced with unique
/// characters and the file is created atomically.
///
/// Returns an open file descriptor on success, or -1 on error.
///
/// # Safety
///
/// `template` must be a writable null-terminated string with at least
/// 6 trailing 'X' characters.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mkstemp(template: *mut u8) -> i32 {
    if template.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    let len = unsafe { crate::string::strlen(template) };
    if len < 6 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    // Verify the last 6 characters are 'X'.
    let suffix_start = len.wrapping_sub(6);
    let mut i: usize = 0;
    while i < 6 {
        if unsafe { *template.add(suffix_start.wrapping_add(i)) } != b'X' {
            crate::errno::set_errno(crate::errno::EINVAL);
            return -1;
        }
        i = i.wrapping_add(1);
    }

    // Try up to 100 unique names.
    let mut attempt: u32 = 0;
    while attempt < 100 {
        // Generate random bytes for the suffix.  Use getrandom (backed
        // by RDRAND) for unpredictability — predictable temp file names
        // are a security vulnerability (symlink attacks).
        let mut rand_bytes = [0u8; 6];
        crate::unistd::getrandom(rand_bytes.as_mut_ptr(), 6, 0);

        // Fill the 6 X's with alphanumeric characters from random bytes.
        let mut j: usize = 0;
        while j < 6 {
            let rb = rand_bytes.get(j).copied().unwrap_or(0);
            let idx = rb % 36;
            let ch = if idx < 10 {
                b'0'.wrapping_add(idx)
            } else {
                b'a'.wrapping_add(idx.wrapping_sub(10))
            };
            // SAFETY: suffix_start + j < len, template is writable.
            unsafe { *template.add(suffix_start.wrapping_add(j)) = ch; }
            j = j.wrapping_add(1);
        }

        // Try to create the file exclusively.
        let flags = crate::fcntl::O_RDWR | crate::fcntl::O_CREAT | crate::fcntl::O_EXCL;
        let fd = crate::file::open(template, flags, 0o600);
        if fd >= 0 {
            return fd;
        }

        // If EEXIST, try again.  Any other error, bail.
        if crate::errno::get_errno() != crate::errno::EEXIST {
            return -1;
        }

        attempt = attempt.wrapping_add(1);
    }

    crate::errno::set_errno(crate::errno::EEXIST);
    -1
}

/// Generate a unique temporary filename (DEPRECATED — use `mkstemp`).
///
/// Replaces the last 6 'X' characters in `template` with random
/// characters to create a unique filename.  Does NOT create the file,
/// which is inherently racy (TOCTOU vulnerability).
///
/// Returns `template` on success, or sets errno and returns NULL on
/// failure.
///
/// # Safety
///
/// `template` must be a writable null-terminated string with at least
/// 6 trailing 'X' characters.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mktemp(template: *mut u8) -> *mut u8 {
    if template.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return core::ptr::null_mut();
    }

    let len = unsafe { crate::string::strlen(template) };
    if len < 6 {
        crate::errno::set_errno(crate::errno::EINVAL);
        // POSIX: mktemp sets template[0] = '\0' on error.
        unsafe { *template = 0; }
        return core::ptr::null_mut();
    }

    // Verify the last 6 characters are 'X'.
    let suffix_start = len.wrapping_sub(6);
    let mut i: usize = 0;
    while i < 6 {
        if unsafe { *template.add(suffix_start.wrapping_add(i)) } != b'X' {
            crate::errno::set_errno(crate::errno::EINVAL);
            unsafe { *template = 0; }
            return core::ptr::null_mut();
        }
        i = i.wrapping_add(1);
    }

    // Generate random suffix.
    let mut rand_bytes = [0u8; 6];
    crate::unistd::getrandom(rand_bytes.as_mut_ptr(), 6, 0);

    let mut j: usize = 0;
    while j < 6 {
        let rb = rand_bytes.get(j).copied().unwrap_or(0);
        let idx = rb % 36;
        let ch = if idx < 10 {
            b'0'.wrapping_add(idx)
        } else {
            b'a'.wrapping_add(idx.wrapping_sub(10))
        };
        unsafe { *template.add(suffix_start.wrapping_add(j)) = ch; }
        j = j.wrapping_add(1);
    }

    template
}

/// Create a temporary file.
///
/// Returns a FILE* stream for a unique temporary file opened in "w+b"
/// mode, or null on error.  The file is automatically deleted when
/// closed.
///
/// Note: Automatic deletion is not implemented (no unlink-on-close
/// support yet).  The file persists until manually removed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tmpfile() -> *mut u8 {
    let mut template: [u8; 20] = *b"/tmp/tmpXXXXXX\0\0\0\0\0\0";
    let fd = unsafe { mkstemp(template.as_mut_ptr()) };
    if fd < 0 {
        return core::ptr::null_mut();
    }
    // Return a FILE* (not a raw fd) per POSIX.
    crate::stdio::fdopen(fd, c"w+".as_ptr().cast::<u8>())
}

// ---------------------------------------------------------------------------
// mkostemp — mkstemp with flags
// ---------------------------------------------------------------------------

/// Create a unique temporary file with additional open flags.
///
/// Like `mkstemp` but `flags` can include `O_CLOEXEC`, `O_APPEND`,
/// etc.  Currently, the flags are accepted but not enforced (our open
/// implementation doesn't support `O_CLOEXEC`).
///
/// # Safety
///
/// `template` must be a writable null-terminated string with at least
/// 6 trailing 'X' characters.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mkostemp(template: *mut u8, flags: i32) -> i32 {
    if template.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    let len = unsafe { crate::string::strlen(template) };
    if len < 6 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }

    // Verify the last 6 characters are 'X'.
    let suffix_start = len.wrapping_sub(6);
    let mut i: usize = 0;
    while i < 6 {
        if unsafe { *template.add(suffix_start.wrapping_add(i)) } != b'X' {
            crate::errno::set_errno(crate::errno::EINVAL);
            return -1;
        }
        i = i.wrapping_add(1);
    }

    // Try up to 100 unique names.
    let mut attempt: u32 = 0;
    while attempt < 100 {
        // Use getrandom for unpredictable suffix (same rationale as mkstemp).
        let mut rand_bytes = [0u8; 6];
        crate::unistd::getrandom(rand_bytes.as_mut_ptr(), 6, 0);

        let mut j: usize = 0;
        while j < 6 {
            let rb = rand_bytes.get(j).copied().unwrap_or(0);
            let idx = rb % 36;
            let ch = if idx < 10 {
                b'0'.wrapping_add(idx)
            } else {
                b'a'.wrapping_add(idx.wrapping_sub(10))
            };
            unsafe { *template.add(suffix_start.wrapping_add(j)) = ch; }
            j = j.wrapping_add(1);
        }

        // OR the caller's flags (e.g., O_CLOEXEC, O_APPEND) with the
        // mandatory O_RDWR | O_CREAT | O_EXCL flags.
        let open_flags = crate::fcntl::O_RDWR
            | crate::fcntl::O_CREAT
            | crate::fcntl::O_EXCL
            | flags;
        let fd = crate::file::open(template, open_flags, 0o600);
        if fd >= 0 {
            return fd;
        }

        if crate::errno::get_errno() != crate::errno::EEXIST {
            return -1;
        }

        attempt = attempt.wrapping_add(1);
    }

    crate::errno::set_errno(crate::errno::EEXIST);
    -1
}

// ---------------------------------------------------------------------------
// mkdtemp — create a unique temporary directory
// ---------------------------------------------------------------------------

/// Create a unique temporary directory.
///
/// Modifies `template` in-place (replacing the trailing 6 'X' chars
/// with a unique suffix) and creates the directory with mode 0700.
/// Returns `template` on success, or null on error.
///
/// # Safety
///
/// `template` must be a writable null-terminated string with at least
/// 6 trailing 'X' characters.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mkdtemp(template: *mut u8) -> *mut u8 {
    if template.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return core::ptr::null_mut();
    }

    let len = unsafe { crate::string::strlen(template) };
    if len < 6 {
        crate::errno::set_errno(crate::errno::EINVAL);
        return core::ptr::null_mut();
    }

    // Verify the last 6 characters are 'X'.
    let suffix_start = len.wrapping_sub(6);
    let mut i: usize = 0;
    while i < 6 {
        if unsafe { *template.add(suffix_start.wrapping_add(i)) } != b'X' {
            crate::errno::set_errno(crate::errno::EINVAL);
            return core::ptr::null_mut();
        }
        i = i.wrapping_add(1);
    }

    // Try up to 100 unique names.
    let mut attempt: u32 = 0;
    while attempt < 100 {
        // Generate a cryptographically random suffix via RDRAND-backed getrandom.
        let mut rand_bytes = [0u8; 6];
        crate::unistd::getrandom(rand_bytes.as_mut_ptr(), 6, 0);

        let mut j: usize = 0;
        while j < 6 {
            let rb = rand_bytes.get(j).copied().unwrap_or(0);
            let idx = rb % 36;
            let ch = if idx < 10 {
                b'0'.wrapping_add(idx)
            } else {
                b'a'.wrapping_add(idx.wrapping_sub(10))
            };
            // SAFETY: suffix_start + j < len, template is writable.
            unsafe { *template.add(suffix_start.wrapping_add(j)) = ch; }
            j = j.wrapping_add(1);
        }

        // Try to create the directory.
        let ret = crate::file::mkdir(template, 0o700);
        if ret == 0 {
            return template;
        }

        // If EEXIST, try again.
        if crate::errno::get_errno() != crate::errno::EEXIST {
            return core::ptr::null_mut();
        }

        attempt = attempt.wrapping_add(1);
    }

    crate::errno::set_errno(crate::errno::EEXIST);
    core::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// system — execute a shell command
// ---------------------------------------------------------------------------

/// Execute a command using the system shell.
///
/// If `command` is NULL, returns whether a shell is available (1 = yes,
/// 0 = no).  Otherwise, spawns `/bin/sh -c "command"` via `posix_spawnp`
/// and waits for completion, returning the child's wait status.
///
/// Returns -1 on spawn failure (errno set), 127 if the shell could not
/// be executed (matches POSIX convention), or the child's wait status
/// on success.
///
/// # Safety
///
/// `command` must be a valid null-terminated string (or NULL).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn system(command: *const u8) -> i32 {
    if command.is_null() {
        // POSIX: return non-zero if a command processor is available.
        // Try to stat /bin/sh to check.
        let mut st = crate::stat::Stat::zeroed();
        let sh = b"/bin/sh\0";
        let ret = crate::file::stat(sh.as_ptr(), &raw mut st);
        return i32::from(ret == 0);
    }

    // Build argv: ["sh", "-c", command, NULL].
    // POSIX: system() must use /bin/sh directly, NOT search PATH.
    // Searching PATH is a security vulnerability: a malicious PATH
    // entry could redirect "sh" to an attacker-controlled binary.
    let sh_path: *const u8 = c"/bin/sh".as_ptr().cast::<u8>();
    let sh_name: *const u8 = c"sh".as_ptr().cast::<u8>();
    let dash_c: *const u8 = c"-c".as_ptr().cast::<u8>();
    let argv: [*const u8; 4] = [sh_name, dash_c, command, core::ptr::null()];

    let mut pid: crate::types::PidT = 0;

    // Use posix_spawn (not posix_spawnp) to avoid PATH search.
    let ret = crate::spawn::posix_spawn(
        &raw mut pid,
        sh_path,
        core::ptr::null(),  // file_actions
        core::ptr::null(),  // attrp
        argv.as_ptr(),
        core::ptr::null(),  // envp (inherit)
    );

    if ret != 0 {
        // posix_spawnp failed (errno already set by spawnp).
        // POSIX says return as if the shell exited with status 127.
        return 127_i32.wrapping_shl(8); // Encode as wait status: exit 127.
    }

    // Wait for the child.
    let mut status: i32 = 0;
    let waited = crate::process::waitpid(pid, &raw mut status, 0);
    if waited < 0 {
        return -1;
    }

    status
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

// ---------------------------------------------------------------------------
// drand48 / lrand48 / mrand48 family — 48-bit LCG PRNG (POSIX)
// ---------------------------------------------------------------------------
//
// Uses the standard POSIX 48-bit linear congruential generator:
//   X_{n+1} = (a * X_n + c) mod 2^48
// where a = 0x5DEECE66D, c = 0xB.

/// 48-bit PRNG state.
static mut RAND48_STATE: u64 = 0x330E_ABCD_1234_u64;

/// LCG multiplier (POSIX standard value).
const RAND48_A: u64 = 0x0005_DEEC_E66D;
/// LCG addend (POSIX standard value).
const RAND48_C: u64 = 0xB;
/// 48-bit mask.
const RAND48_MASK: u64 = (1_u64 << 48) - 1;

/// Advance the 48-bit LCG state.
#[inline]
fn rand48_step() -> u64 {
    // SAFETY: Single-threaded access.
    let state = unsafe { core::ptr::addr_of_mut!(RAND48_STATE).read() };
    let next = (state.wrapping_mul(RAND48_A).wrapping_add(RAND48_C)) & RAND48_MASK;
    unsafe { core::ptr::addr_of_mut!(RAND48_STATE).write(next); }
    next
}

/// Return a non-negative `f64` in [0.0, 1.0).
///
/// Uses the full 48-bit state scaled to a double.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects, clippy::cast_precision_loss)]
pub extern "C" fn drand48() -> f64 {
    let state = rand48_step();
    // 2^48 = 281474976710656.0; 48-bit value fits in f64's 52-bit mantissa.
    state as f64 / 281_474_976_710_656.0
}

/// Return a non-negative `i64` in [0, 2^31).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lrand48() -> i64 {
    let state = rand48_step();
    (state >> 17) as i64 // Upper 31 bits.
}

/// Return a signed `i64` in [-2^31, 2^31).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mrand48() -> i64 {
    let state = rand48_step();
    // Interpret upper 32 bits as signed.
    i64::from((state >> 16) as i32)
}

/// Seed the 48-bit PRNG with a 32-bit value.
///
/// Sets the upper 32 bits of state; lower 16 bits are set to 0x330E
/// (POSIX default).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn srand48(seedval: i64) {
    let hi = (seedval as u64) << 16;
    let state = (hi | 0x330E) & RAND48_MASK;
    unsafe { core::ptr::addr_of_mut!(RAND48_STATE).write(state); }
}

/// Seed the 48-bit PRNG with a full 48-bit value.
///
/// `seed16v` points to an array of 3 `u16` values.
/// Returns a pointer to the previous seed (static storage).
///
/// # Safety
///
/// `seed16v` must point to at least 3 `u16` values.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn seed48(seed16v: *const u16) -> *const u16 {
    static mut OLD_SEED: [u16; 3] = [0; 3];

    // Use addr_of_mut to avoid creating shared references to mutable
    // statics (Rust 2024).  addr_of_mut! is safe; only the dereference
    // is unsafe.
    let old_seed_ptr = core::ptr::addr_of_mut!(OLD_SEED);

    if seed16v.is_null() {
        return old_seed_ptr.cast::<u16>();
    }

    // Save old state.
    let old = unsafe { core::ptr::addr_of_mut!(RAND48_STATE).read() };
    unsafe {
        (*old_seed_ptr)[0] = (old & 0xFFFF) as u16;
        (*old_seed_ptr)[1] = ((old >> 16) & 0xFFFF) as u16;
        (*old_seed_ptr)[2] = ((old >> 32) & 0xFFFF) as u16;
    }

    // Set new state from seed16v[0..3].
    // SAFETY: seed16v verified non-null, caller guarantees 3 elements.
    let s0 = u64::from(unsafe { *seed16v });
    let s1 = u64::from(unsafe { *seed16v.add(1) });
    let s2 = u64::from(unsafe { *seed16v.add(2) });
    let state = (s2 << 32) | (s1 << 16) | s0;
    unsafe { core::ptr::addr_of_mut!(RAND48_STATE).write(state & RAND48_MASK); }

    old_seed_ptr.cast::<u16>()
}

/// Same as `lrand48` but uses caller-provided state.
///
/// # Safety
///
/// `xsubi` must point to an array of 3 `u16` values that the
/// function will read and update.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn nrand48(xsubi: *mut u16) -> i64 {
    if xsubi.is_null() {
        return 0;
    }

    // Read state from xsubi.
    let s0 = u64::from(unsafe { *xsubi });
    let s1 = u64::from(unsafe { *xsubi.add(1) });
    let s2 = u64::from(unsafe { *xsubi.add(2) });
    let state = (s2 << 32) | (s1 << 16) | s0;

    // Step.
    let next = (state.wrapping_mul(RAND48_A).wrapping_add(RAND48_C)) & RAND48_MASK;

    // Write back.
    unsafe {
        *xsubi = (next & 0xFFFF) as u16;
        *xsubi.add(1) = ((next >> 16) & 0xFFFF) as u16;
        *xsubi.add(2) = ((next >> 32) & 0xFFFF) as u16;
    }

    (next >> 17) as i64
}

/// Same as `drand48` but uses caller-provided state.
///
/// # Safety
///
/// `xsubi` must point to an array of 3 `u16` values.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects, clippy::cast_precision_loss)]
pub extern "C" fn erand48(xsubi: *mut u16) -> f64 {
    if xsubi.is_null() {
        return 0.0;
    }

    let s0 = u64::from(unsafe { *xsubi });
    let s1 = u64::from(unsafe { *xsubi.add(1) });
    let s2 = u64::from(unsafe { *xsubi.add(2) });
    let state = (s2 << 32) | (s1 << 16) | s0;

    let next = (state.wrapping_mul(RAND48_A).wrapping_add(RAND48_C)) & RAND48_MASK;

    unsafe {
        *xsubi = (next & 0xFFFF) as u16;
        *xsubi.add(1) = ((next >> 16) & 0xFFFF) as u16;
        *xsubi.add(2) = ((next >> 32) & 0xFFFF) as u16;
    }

    // 48-bit value fits in f64's 52-bit mantissa — no precision loss.
    next as f64 / 281_474_976_710_656.0
}

/// Same as `mrand48` but uses caller-provided state.
///
/// # Safety
///
/// `xsubi` must point to an array of 3 `u16` values.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn jrand48(xsubi: *mut u16) -> i64 {
    if xsubi.is_null() {
        return 0;
    }

    let s0 = u64::from(unsafe { *xsubi });
    let s1 = u64::from(unsafe { *xsubi.add(1) });
    let s2 = u64::from(unsafe { *xsubi.add(2) });
    let state = (s2 << 32) | (s1 << 16) | s0;

    let next = (state.wrapping_mul(RAND48_A).wrapping_add(RAND48_C)) & RAND48_MASK;

    unsafe {
        *xsubi = (next & 0xFFFF) as u16;
        *xsubi.add(1) = ((next >> 16) & 0xFFFF) as u16;
        *xsubi.add(2) = ((next >> 32) & 0xFFFF) as u16;
    }

    i64::from((next >> 16) as i32)
}

// ---------------------------------------------------------------------------
// getsubopt — parse suboption strings
// ---------------------------------------------------------------------------

/// Parse comma-separated suboptions.
///
/// Scans `*optionp` for the next suboption from the null-terminated
/// `tokens` array.  On match, `*valuep` points to the value after `=`
/// (or null if no `=`), `*optionp` is advanced past the suboption,
/// and the matching token index is returned.  Returns -1 if no match.
///
/// # Safety
///
/// `optionp` must point to a valid `*mut u8` pointing into a
/// modifiable string.  `tokens` must be a null-terminated array of
/// null-terminated C strings.  `valuep` must be a valid pointer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getsubopt(
    optionp: *mut *mut u8,
    tokens: *const *const u8,
    valuep: *mut *mut u8,
) -> i32 {
    if optionp.is_null() || tokens.is_null() || valuep.is_null() {
        return -1;
    }

    let opt = unsafe { *optionp };
    if opt.is_null() || unsafe { *opt } == 0 {
        return -1;
    }

    // Find the end of this suboption (comma or null).
    let mut end: usize = 0;
    while unsafe { *opt.add(end) } != 0 && unsafe { *opt.add(end) } != b',' {
        end = end.wrapping_add(1);
    }

    // Find '=' within this suboption to separate key from value.
    let mut eq_pos: Option<usize> = None;
    let mut j: usize = 0;
    while j < end {
        if unsafe { *opt.add(j) } == b'=' {
            eq_pos = Some(j);
            break;
        }
        j = j.wrapping_add(1);
    }

    let key_len = eq_pos.unwrap_or(end);

    // Try to match against each token.
    let mut idx: i32 = 0;
    loop {
        let token = unsafe { *tokens.add(idx as usize) };
        if token.is_null() {
            break;
        }

        // Compare key_len bytes of opt against this token.
        let tok_len = unsafe { crate::string::strlen(token) };
        if tok_len == key_len {
            let mut matched = true;
            let mut k: usize = 0;
            while k < key_len {
                if unsafe { *opt.add(k) } != unsafe { *token.add(k) } {
                    matched = false;
                    break;
                }
                k = k.wrapping_add(1);
            }

            if matched {
                // Set valuep to the value after '=' (or null).
                if let Some(ep) = eq_pos {
                    unsafe { *valuep = opt.add(ep.wrapping_add(1)); }
                } else {
                    unsafe { *valuep = core::ptr::null_mut(); }
                }

                // Advance optionp past this suboption.
                if unsafe { *opt.add(end) } == b',' {
                    unsafe { *optionp = opt.add(end.wrapping_add(1)); }
                } else {
                    unsafe { *optionp = opt.add(end); }
                }

                // Null-terminate the key portion (write '\0' at '=' or end).
                unsafe { *opt.add(key_len) = 0; }

                return idx;
            }
        }

        idx = idx.wrapping_add(1);
    }

    // No match — still advance past this suboption.
    if let Some(ep) = eq_pos {
        unsafe { *valuep = opt.add(ep.wrapping_add(1)); }
    } else {
        unsafe { *valuep = core::ptr::null_mut(); }
    }
    if unsafe { *opt.add(end) } == b',' {
        unsafe { *optionp = opt.add(end.wrapping_add(1)); }
    } else {
        unsafe { *optionp = opt.add(end); }
    }
    unsafe { *opt.add(key_len) = 0; }

    -1
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- atoi / atol / atoll tests --

    #[test]
    fn test_atoi_basic() {
        assert_eq!(unsafe { atoi(b"42\0".as_ptr()) }, 42);
        assert_eq!(unsafe { atoi(b"-7\0".as_ptr()) }, -7);
        assert_eq!(unsafe { atoi(b"0\0".as_ptr()) }, 0);
        assert_eq!(unsafe { atoi(b"  123\0".as_ptr()) }, 123);
    }

    #[test]
    fn test_atoi_stops_at_nondigit() {
        assert_eq!(unsafe { atoi(b"123abc\0".as_ptr()) }, 123);
        assert_eq!(unsafe { atoi(b"abc\0".as_ptr()) }, 0);
    }

    #[test]
    fn test_atol_basic() {
        assert_eq!(unsafe { atol(b"1000000\0".as_ptr()) }, 1_000_000);
        assert_eq!(unsafe { atol(b"-999\0".as_ptr()) }, -999);
    }

    // -- strtol tests --

    #[test]
    fn test_strtol_decimal() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtol(b"  -42xyz\0".as_ptr(), &mut endptr, 10) };
        assert_eq!(val, -42);
        assert!(!endptr.is_null());
        assert_eq!(unsafe { *endptr }, b'x');
    }

    #[test]
    fn test_strtol_hex() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtol(b"0xff\0".as_ptr(), &mut endptr, 16) };
        assert_eq!(val, 255);
    }

    #[test]
    fn test_strtol_hex_auto() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtol(b"0x1A\0".as_ptr(), &mut endptr, 0) };
        assert_eq!(val, 26);
    }

    #[test]
    fn test_strtol_octal_auto() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtol(b"0755\0".as_ptr(), &mut endptr, 0) };
        assert_eq!(val, 493); // 0o755 = 493
    }



    #[test]
    fn test_strtol_empty_string() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtol(b"\0".as_ptr(), &mut endptr, 10) };
        assert_eq!(val, 0);
    }

    // -- strtoul tests --

    #[test]
    fn test_strtoul_basic() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtoul(b"12345\0".as_ptr(), &mut endptr, 10) };
        assert_eq!(val, 12345);
    }

    #[test]
    fn test_strtoul_hex() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtoul(b"0xDEAD\0".as_ptr(), &mut endptr, 0) };
        assert_eq!(val, 0xDEAD);
    }

    // -- strtod tests --

    #[test]
    fn test_strtod_basic() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtod(b"3.14\0".as_ptr(), &mut endptr) };
        assert!((val - 3.14).abs() < 1e-10);
    }

    #[test]
    fn test_strtod_negative() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtod(b"-2.5\0".as_ptr(), &mut endptr) };
        assert!((val - (-2.5)).abs() < 1e-10);
    }

    #[test]
    fn test_strtod_scientific() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtod(b"1.5e3\0".as_ptr(), &mut endptr) };
        assert!((val - 1500.0).abs() < 1e-10);
    }

    #[test]
    fn test_strtod_integer() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtod(b"42\0".as_ptr(), &mut endptr) };
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(val, 42.0);
        }
    }

    #[test]
    fn test_strtod_leading_whitespace() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtod(b"  3.0\0".as_ptr(), &mut endptr) };
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(val, 3.0);
        }
    }

    // -- abs / labs / llabs tests --

    #[test]
    fn test_abs_basic() {
        assert_eq!(abs(42), 42);
        assert_eq!(abs(-42), 42);
        assert_eq!(abs(0), 0);
    }

    #[test]
    fn test_labs_basic() {
        assert_eq!(labs(100_000), 100_000);
        assert_eq!(labs(-100_000), 100_000);
    }

    // -- div / ldiv tests --

    #[test]
    fn test_div_basic() {
        let r = div(17, 5);
        assert_eq!(r.quot, 3);
        assert_eq!(r.rem, 2);
    }

    #[test]
    fn test_div_negative() {
        let r = div(-17, 5);
        assert_eq!(r.quot, -3);
        assert_eq!(r.rem, -2);
    }

    #[test]
    fn test_ldiv_basic() {
        let r = ldiv(100, 7);
        assert_eq!(r.quot, 14);
        assert_eq!(r.rem, 2);
    }

    // -- qsort tests --

    extern "C" fn cmp_i32(a: *const u8, b: *const u8) -> i32 {
        let a_val = unsafe { *(a as *const i32) };
        let b_val = unsafe { *(b as *const i32) };
        a_val.wrapping_sub(b_val)
    }

    #[test]
    fn test_qsort_basic() {
        let mut arr: [i32; 5] = [5, 3, 1, 4, 2];
        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                5,
                core::mem::size_of::<i32>(),
                cmp_i32,
            );
        }
        assert_eq!(arr, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_qsort_already_sorted() {
        let mut arr: [i32; 4] = [1, 2, 3, 4];
        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                4,
                core::mem::size_of::<i32>(),
                cmp_i32,
            );
        }
        assert_eq!(arr, [1, 2, 3, 4]);
    }

    #[test]
    fn test_qsort_reverse() {
        let mut arr: [i32; 4] = [4, 3, 2, 1];
        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                4,
                core::mem::size_of::<i32>(),
                cmp_i32,
            );
        }
        assert_eq!(arr, [1, 2, 3, 4]);
    }

    #[test]
    fn test_qsort_single_element() {
        let mut arr: [i32; 1] = [42];
        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                1,
                core::mem::size_of::<i32>(),
                cmp_i32,
            );
        }
        assert_eq!(arr, [42]);
    }

    #[test]
    fn test_qsort_empty() {
        let mut arr: [i32; 0] = [];
        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                0,
                core::mem::size_of::<i32>(),
                cmp_i32,
            );
        }
        // Should not crash.
    }

    // -- bsearch tests --

    #[test]
    fn test_bsearch_found() {
        let arr: [i32; 5] = [1, 3, 5, 7, 9];
        let key: i32 = 5;
        let p = unsafe {
            bsearch(
                (&key as *const i32).cast(),
                arr.as_ptr().cast(),
                5,
                core::mem::size_of::<i32>(),
                cmp_i32,
            )
        };
        assert!(!p.is_null());
        assert_eq!(unsafe { *(p as *const i32) }, 5);
    }

    #[test]
    fn test_bsearch_not_found() {
        let arr: [i32; 5] = [1, 3, 5, 7, 9];
        let key: i32 = 4;
        let p = unsafe {
            bsearch(
                (&key as *const i32).cast(),
                arr.as_ptr().cast(),
                5,
                core::mem::size_of::<i32>(),
                cmp_i32,
            )
        };
        assert!(p.is_null());
    }

    // -- rand / srand tests --

    #[test]
    fn test_srand_rand_deterministic() {
        srand(12345);
        let a = rand();
        srand(12345);
        let b = rand();
        assert_eq!(a, b);
    }

    #[test]
    fn test_rand_nonnegative() {
        srand(42);
        for _ in 0..100 {
            assert!(rand() >= 0);
        }
    }

    // -- getsubopt tests --

    #[test]
    fn test_getsubopt_match() {
        // Tokens: "ro", "rw", "size"
        let tok0: *const u8 = b"ro\0".as_ptr();
        let tok1: *const u8 = b"rw\0".as_ptr();
        let tok2: *const u8 = b"size\0".as_ptr();
        let tokens: [*const u8; 4] = [
            tok0,
            tok1,
            tok2,
            core::ptr::null(),
        ];

        let mut input = *b"rw,size=100\0";
        let mut optionp: *mut u8 = input.as_mut_ptr();
        let mut valuep: *mut u8 = core::ptr::null_mut();

        // First suboption: "rw"
        let idx = unsafe {
            getsubopt(
                &mut optionp,
                tokens.as_ptr().cast::<*const u8>(),
                &mut valuep,
            )
        };
        assert_eq!(idx, 1); // matches "rw"
        assert!(valuep.is_null()); // no value

        // Second suboption: "size=100"
        let idx = unsafe {
            getsubopt(
                &mut optionp,
                tokens.as_ptr().cast::<*const u8>(),
                &mut valuep,
            )
        };
        assert_eq!(idx, 2); // matches "size"
        assert!(!valuep.is_null()); // has value "100"
    }
}
