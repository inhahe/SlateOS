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
/// On overflow, sets errno to ERANGE and returns `i64::MAX` (positive
/// overflow) or `i64::MIN` (negative overflow).  `endptr` still points
/// past the last valid digit.
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

    // POSIX: base must be 0 or in [2, 36].
    if base != 0 && (base < 2 || base > 36) {
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
        if !any_digits {
            unsafe { *endptr = nptr; }
        } else {
            unsafe { *endptr = nptr.add(i); }
        }
    }

    if !any_digits {
        return 0;
    }

    // Check range and apply sign.
    // i64::MIN magnitude as u64 = 2^63 = (i64::MAX as u64) + 1.
    const POS_MAX: u64 = i64::MAX as u64;
    const NEG_MAX: u64 = POS_MAX.wrapping_add(1); // 2^63

    if overflow {
        crate::errno::set_errno(crate::errno::ERANGE);
        return if negative { i64::MIN } else { i64::MAX };
    }

    if negative {
        if result > NEG_MAX {
            crate::errno::set_errno(crate::errno::ERANGE);
            i64::MIN
        } else if result == NEG_MAX {
            i64::MIN
        } else {
            // SAFETY: result <= i64::MAX, so cast is safe; then negate.
            -(result as i64)
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

    // POSIX: base must be 0 or in [2, 36].
    if base != 0 && (base < 2 || base > 36) {
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
        if !any_digits {
            unsafe { *endptr = nptr; }
        } else {
            unsafe { *endptr = nptr.add(i); }
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
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
    let c0 = unsafe { *nptr.add(i) } | 0x20; // ASCII lowercase
    if c0 == b'i' {
        // Possible "inf" or "infinity".
        let c1 = unsafe { *nptr.add(i.wrapping_add(1)) } | 0x20;
        let c2 = unsafe { *nptr.add(i.wrapping_add(2)) } | 0x20;
        if c1 == b'n' && c2 == b'f' {
            i = i.wrapping_add(3);
            // Check for full "infinity".
            let rest = [
                unsafe { *nptr.add(i) } | 0x20,
                unsafe { *nptr.add(i.wrapping_add(1)) } | 0x20,
                unsafe { *nptr.add(i.wrapping_add(2)) } | 0x20,
                unsafe { *nptr.add(i.wrapping_add(3)) } | 0x20,
                unsafe { *nptr.add(i.wrapping_add(4)) } | 0x20,
            ];
            if rest == [b'i', b'n', b'i', b't', b'y'] {
                i = i.wrapping_add(5);
            }
            if !endptr.is_null() {
                unsafe { *endptr = nptr.add(i); }
            }
            return if negative { f64::NEG_INFINITY } else { f64::INFINITY };
        }
    } else if c0 == b'n' {
        let c1 = unsafe { *nptr.add(i.wrapping_add(1)) } | 0x20;
        let c2 = unsafe { *nptr.add(i.wrapping_add(2)) } | 0x20;
        if c1 == b'a' && c2 == b'n' {
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

        if !(unsafe { *nptr.add(i) }).is_ascii_digit() {
            // No exponent digits — roll back.
            i = before_exp;
        } else {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtof(
    nptr: *const u8,
    endptr: *mut *const u8,
) -> f32 {
    let d = unsafe { strtod(nptr, endptr) };
    let f = d as f32;
    // POSIX: set ERANGE if the f32 result overflows or underflows.
    // strtod already sets ERANGE for f64 overflow; we additionally check
    // for values that fit in f64 but not f32.
    if f.is_infinite() && !d.is_infinite() {
        crate::errno::set_errno(crate::errno::ERANGE);
    } else if f == 0.0 && d != 0.0 {
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn abs(j: i32) -> i32 {
    if j < 0 { j.saturating_neg() } else { j }
}

/// Compute absolute value of a long integer.
#[unsafe(no_mangle)]
pub extern "C" fn labs(j: i64) -> i64 {
    if j < 0 { j.saturating_neg() } else { j }
}

/// Compute absolute value of a long long integer.
///
/// On our platform `long long` = `i64`, same as `labs`.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn srand(seed: u32) {
    // SAFETY: Single-threaded userspace. Using addr_of_mut for Rust 2024.
    unsafe { core::ptr::addr_of_mut!(RAND_STATE).write(u64::from(seed)); }
}

/// Generate a pseudo-random integer in [0, RAND_MAX].
///
/// Uses the glibc LCG: state = state * 6364136223846793005 + 1.
/// Returns the upper 31 bits.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub static RAND_MAX: i32 = 0x7FFF_FFFF;

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
#[unsafe(no_mangle)]
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
            let idx = rand_bytes[j] % 36;
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

/// Create a temporary file.
///
/// Returns a FILE* stream for a unique temporary file opened in "w+b"
/// mode, or null on error.  The file is automatically deleted when
/// closed.
///
/// Note: Automatic deletion is not implemented (no unlink-on-close
/// support yet).  The file persists until manually removed.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
            let idx = rand_bytes[j] % 36;
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
#[unsafe(no_mangle)]
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
            let idx = rand_bytes[j] % 36;
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
#[unsafe(no_mangle)]
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
    let sh_path: *const u8 = c"sh".as_ptr().cast::<u8>();
    let dash_c: *const u8 = c"-c".as_ptr().cast::<u8>();
    let argv: [*const u8; 4] = [sh_path, dash_c, command, core::ptr::null()];

    let mut pid: crate::types::PidT = 0;

    let ret = crate::spawn::posix_spawnp(
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
