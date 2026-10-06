//! C standard library conversion functions.
//!
//! Implements integer and floating-point conversion, absolute value,
//! integer division structs, sorting, searching, and temporary file
//! creation. The pseudo-random number generators are [`crate::prng`].
//!
//! ## Functions
//!
//! - `atoi`, `atol` — quick string→integer
//! - `strtol`, `strtoul`, `strtoll`, `strtoull` — full string→integer
//! - `strtod`, `strtof`, `strtold` — string→floating-point
//! - `abs`, `labs`, `llabs` — absolute value
//! - `div`, `ldiv`, `lldiv` — integer division with quotient/remainder
//! - `qsort`, `bsearch` — array sorting/searching
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
pub unsafe extern "C" fn strtol(nptr: *const u8, endptr: *mut *const u8, mut base: i32) -> i64 {
    // Check range and apply sign.
    // i64::MIN magnitude as u64 = 2^63 = (i64::MAX as u64) + 1.
    const POS_MAX: u64 = i64::MAX as u64;
    const NEG_MAX: u64 = POS_MAX.wrapping_add(1); // 2^63

    if nptr.is_null() {
        if !endptr.is_null() {
            unsafe {
                *endptr = nptr;
            }
        }
        return 0;
    }

    // POSIX: base must be 0 or in [2, 36].
    if base != 0 && !(2..=36).contains(&base) {
        crate::errno::set_errno(crate::errno::EINVAL);
        if !endptr.is_null() {
            unsafe {
                *endptr = nptr;
            }
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
            unsafe {
                *endptr = nptr.add(i);
            }
        } else {
            unsafe {
                *endptr = nptr;
            }
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
pub unsafe extern "C" fn strtoul(nptr: *const u8, endptr: *mut *const u8, mut base: i32) -> u64 {
    if nptr.is_null() {
        if !endptr.is_null() {
            unsafe {
                *endptr = nptr;
            }
        }
        return 0;
    }

    // POSIX: base must be 0 or in [2, 36].
    if base != 0 && !(2..=36).contains(&base) {
        crate::errno::set_errno(crate::errno::EINVAL);
        if !endptr.is_null() {
            unsafe {
                *endptr = nptr;
            }
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
            unsafe {
                *endptr = nptr.add(i);
            }
        } else {
            unsafe {
                *endptr = nptr;
            }
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
    if negative {
        result.wrapping_neg()
    } else {
        result
    }
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
pub unsafe extern "C" fn strtoll(nptr: *const u8, endptr: *mut *const u8, base: i32) -> i64 {
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
pub unsafe extern "C" fn strtoull(nptr: *const u8, endptr: *mut *const u8, base: i32) -> u64 {
    unsafe { strtoul(nptr, endptr, base) }
}

// ---------------------------------------------------------------------------
// Floating-point conversion
// ---------------------------------------------------------------------------

/// A C string as a source of bytes for the shared float scanner.
///
/// Reads one byte at a time and never past the terminator, which could sit at
/// the end of a mapped page.
struct CStrSource(*const u8);

impl crate::decfloat::ByteSource for CStrSource {
    fn byte_at(&self, i: usize) -> u8 {
        // SAFETY: the constructor's contract is that `self.0` is a valid
        // null-terminated string, and the scanner stops at the first 0 it
        // sees, so `i` never runs past the terminator.
        unsafe { *self.0.add(i) }
    }
}

/// Scan a `strtod` subject sequence from a C string and set `*endptr`.
///
/// The grammar lives in [`crate::decfloat::scan_float_token`], shared with
/// `wcstod`; this only supplies the bytes and reports where the scan stopped.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string, and `endptr` either null or
/// writable.
unsafe fn scan_float_cstr(
    nptr: *const u8,
    endptr: *mut *const u8,
    acc: &mut crate::decfloat::DigitCollector,
) -> (crate::decfloat::FloatToken, bool) {
    if nptr.is_null() {
        if !endptr.is_null() {
            // SAFETY: the caller promises `endptr` is writable.
            unsafe {
                *endptr = nptr;
            }
        }
        return (crate::decfloat::FloatToken::None, false);
    }

    let (token, negative, consumed) = crate::decfloat::scan_float_token(&CStrSource(nptr), acc);

    if !endptr.is_null() {
        // SAFETY: the caller promises `endptr` is writable, and `consumed`
        // never passes the terminator because the scanner stops at it.
        unsafe {
            *endptr = nptr.add(consumed);
        }
    }

    (token, negative)
}

/// Convert a C string to a double (`strtod`).
///
/// Parses decimal floating-point strings of the form:
///   `[whitespace][sign]digits[.digits][e[sign]digits]`
///
/// Also supports `INF`, `INFINITY`, `NAN` and `NAN(chars)` (case-insensitive),
/// and C99's hexadecimal form, `0x1.8p+3`.
///
/// The digits are collected exactly and rounded once, so the result is the
/// nearest `double` to the input, ties to even.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtod(nptr: *const u8, endptr: *mut *const u8) -> f64 {
    let mut acc = crate::decfloat::DigitCollector::new();
    // SAFETY: forwarding this function's own contract.
    let (token, negative) = unsafe { scan_float_cstr(nptr, endptr, &mut acc) };
    let value = match token {
        crate::decfloat::FloatToken::None => return 0.0,
        crate::decfloat::FloatToken::Nan(p) => return crate::decfloat::nan_f64(p, negative),
        crate::decfloat::FloatToken::Infinity => f64::INFINITY,
        crate::decfloat::FloatToken::Number => {
            let (v, out_of_range) = acc.to_f64(negative);
            // ERANGE on overflow, and on underflow as glibc judges it: a
            // result tiny after rounding and inexact -- an exact subnormal
            // is no error. Rounded in the current direction, as glibc's.
            if out_of_range {
                crate::errno::set_errno(crate::errno::ERANGE);
            }
            v
        }
    };
    if negative { -value } else { value }
}

/// `strtof` in an explicit locale.
///
/// We have exactly one locale, so this is `strtof` and the handle is ignored —
/// the same wrapper musl writes, for the same reason. The decimal point is `.`
/// in the C locale and there is no other locale here in which it could be `,`.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtof_l(
    nptr: *const u8,
    endptr: *mut *const u8,
    _loc: crate::locale::LocaleT,
) -> f32 {
    // SAFETY: forwarding this function's own contract.
    unsafe { strtof(nptr, endptr) }
}

/// Convert a C string to a float (`strtof`).
///
/// Rounds to `f32` directly from the decimal digits rather than by way of
/// `strtod`: two roundings are not one, and a value a hair above an `f32`
/// midpoint can land exactly on that midpoint in `f64` and then be sent the
/// wrong way by ties-to-even.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtof(nptr: *const u8, endptr: *mut *const u8) -> f32 {
    let mut acc = crate::decfloat::DigitCollector::new();
    // SAFETY: forwarding this function's own contract.
    let (token, negative) = unsafe { scan_float_cstr(nptr, endptr, &mut acc) };
    let value = match token {
        crate::decfloat::FloatToken::None => return 0.0,
        crate::decfloat::FloatToken::Nan(p) => return crate::decfloat::nan_f32(p, negative),
        crate::decfloat::FloatToken::Infinity => f32::INFINITY,
        crate::decfloat::FloatToken::Number => {
            let (v, out_of_range) = acc.to_f32(negative);
            if out_of_range {
                crate::errno::set_errno(crate::errno::ERANGE);
            }
            v
        }
    };
    if negative { -value } else { value }
}

/// `strtold`'s conversion, for this library's own callers: the `long double`
/// the text names, to all 64 bits of x87's significand, rounded in the
/// current direction -- glibc's answer for every input, `ERANGE` included.
///
/// `None`, with `*endptr` set to `nptr` as if nothing were converted, when
/// the digits need more memory than there is: a literal of more than 768
/// significant digits, or a value outside about `1e-2550` to `1e1800`, takes a
/// block sized to it ([`crate::decfloat::DigitCollector::to_ld80`]).
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string, and `endptr` either NULL or
/// a valid `char **`.
pub(crate) unsafe fn strtold_ld(
    nptr: *const u8,
    endptr: *mut *const u8,
) -> Option<crate::x87::LongDouble> {
    let mut acc = crate::decfloat::DigitCollector::for_long_double();
    // SAFETY: forwarding this function's own contract.
    let (token, negative) = unsafe { scan_float_cstr(nptr, endptr, &mut acc) };
    let Some((value, out_of_range)) = crate::decfloat::ld80_of(token, negative, &acc) else {
        if !endptr.is_null() {
            // SAFETY: the caller promises `endptr` is writable.
            unsafe { *endptr = nptr };
        }
        return None;
    };
    if out_of_range {
        crate::errno::set_errno(crate::errno::ERANGE);
    }
    Some(value)
}

/// Convert a C string to a `long double` (`strtold`), at the format's full
/// precision ([`strtold_ld`]).
///
/// When the digits need more memory than there is, nothing is converted:
/// the result is 0, `*endptr` is `nptr`, and `errno` is `ENOMEM` -- rather
/// than a value the text does not name. glibc never allocates here, so it
/// has no such case; it can arise only for a literal of more than 768
/// significant digits or a value outside about `1e-2550` to `1e1800`.
///
/// A `long double` comes back in `%st(0)`, which Rust cannot express, so the
/// C symbol is a thunk ([`crate::ld_c`]) into `__slate_ld_strtold`. Until
/// 2026-09-28 it was a `double` conversion widened, 53 bits of the 64
/// (`TD-POSIX-LONG-DOUBLE-PRECISION`).
///
/// # Safety
///
/// As [`strtold_ld`].
pub unsafe fn strtold(nptr: *const u8, endptr: *mut *const u8) -> crate::x87::LongDouble {
    // SAFETY: forwarding this function's own contract.
    unsafe { strtold_ld(nptr, endptr) }.unwrap_or_else(|| {
        crate::errno::set_errno(crate::errno::ENOMEM);
        crate::x87::LongDouble::POS_ZERO
    })
}

/// `strtold` for C, through the thunk: the result into `out`.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_strtold(
    nptr: *const u8,
    endptr: *mut *const u8,
    out: *mut crate::x87::LongDouble,
) {
    // SAFETY: `strtold`'s contract is the C caller's; `out` is the thunk's
    // result slot.
    unsafe { out.write(strtold(nptr, endptr)) }
}
crate::ld_c!(l_pp "strtold" => __slate_ld_strtold);

// The `_l` variants: same conversion, explicit locale object.
//
// They exist because a program that has to parse a number in a *known* format
// — a config file, a `/proc` field, a `--size=` argument — cannot use `strtod`
// safely when the process locale might make the decimal separator a comma.
// gnulib and glibc's own `strtod` internals reach for `strtod_l` for exactly
// that reason, which is how they turned up among the nineteen symbols missing
// from `libc.a` in `scripts/coreutils-spike/run.sh`.
//
// We support one locale, `C`, so honouring the argument is a no-op today — but
// it is a no-op *in the direction that stays correct*: the `_l` caller is
// asking for the C-locale format and that is what it gets, whereas the plain
// `strtod` caller is the one who would be surprised if we ever grew a second
// locale.  The parameter is therefore ignored rather than validated; see
// `locale.rs`, where `newlocale` likewise returns a single opaque tag.

/// `strtod_l(nptr, endptr, loc)` — `strtod` in an explicit locale.
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string, and `endptr` either NULL or
/// a valid `char **`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtod_l(
    nptr: *const u8,
    endptr: *mut *const u8,
    _loc: crate::locale::LocaleT,
) -> f64 {
    // SAFETY: identical requirements, forwarded.
    unsafe { strtod(nptr, endptr) }
}

/// `strtol_l` -- [`strtol`] in a locale, C's.
///
/// # Safety
///
/// As [`strtol`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtol_l(
    nptr: *const u8,
    endptr: *mut *const u8,
    base: i32,
    _loc: crate::locale::LocaleT,
) -> i64 {
    // SAFETY: forwarded.
    unsafe { strtol(nptr, endptr, base) }
}

/// `strtoul_l` -- [`strtoul`] in a locale, C's.
///
/// # Safety
///
/// As [`strtoul`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtoul_l(
    nptr: *const u8,
    endptr: *mut *const u8,
    base: i32,
    _loc: crate::locale::LocaleT,
) -> u64 {
    // SAFETY: forwarded.
    unsafe { strtoul(nptr, endptr, base) }
}

/// `strtoll_l` -- [`strtoll`] in a locale, C's.
///
/// # Safety
///
/// As [`strtol`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtoll_l(
    nptr: *const u8,
    endptr: *mut *const u8,
    base: i32,
    _loc: crate::locale::LocaleT,
) -> i64 {
    // SAFETY: forwarded.
    unsafe { strtoll(nptr, endptr, base) }
}

/// `strtoull_l` -- [`strtoull`] in a locale, C's.
///
/// # Safety
///
/// As [`strtoul`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtoull_l(
    nptr: *const u8,
    endptr: *mut *const u8,
    base: i32,
    _loc: crate::locale::LocaleT,
) -> u64 {
    // SAFETY: forwarded.
    unsafe { strtoull(nptr, endptr, base) }
}

/// `strtoq` -- 4.4BSD's name for [`strtoll`].
///
/// # Safety
///
/// As [`strtol`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtoq(nptr: *const u8, endptr: *mut *const u8, base: i32) -> i64 {
    // SAFETY: forwarded.
    unsafe { strtoll(nptr, endptr, base) }
}

/// `strtouq` -- 4.4BSD's name for [`strtoull`].
///
/// # Safety
///
/// As [`strtoul`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strtouq(nptr: *const u8, endptr: *mut *const u8, base: i32) -> u64 {
    // SAFETY: forwarded.
    unsafe { strtoull(nptr, endptr, base) }
}

/// `strtold_l(nptr, endptr, loc)` — `strtold` in an explicit locale, which
/// is always C's here, as for [`strtod_l`].
///
/// # Safety
///
/// As [`strtold`].
pub unsafe fn strtold_l(
    nptr: *const u8,
    endptr: *mut *const u8,
    _loc: crate::locale::LocaleT,
) -> crate::x87::LongDouble {
    // SAFETY: identical requirements, forwarded.
    unsafe { strtold(nptr, endptr) }
}

/// `strtold_l` for C, through the thunk: the result into `out`.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_strtold_l(
    nptr: *const u8,
    endptr: *mut *const u8,
    loc: crate::locale::LocaleT,
    out: *mut crate::x87::LongDouble,
) {
    // SAFETY: as in `__slate_ld_strtold`.
    unsafe { out.write(strtold_l(nptr, endptr, loc)) }
}
crate::ld_c!(l_ppp "strtold_l" => __slate_ld_strtold_l);

/// Convert a C string to a double (`atof`).
///
/// # Safety
///
/// `nptr` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn atof(nptr: *const u8) -> f64 {
    unsafe { strtod(nptr, core::ptr::null_mut()) }
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
        return DivT {
            quot: i32::MIN,
            rem: 0,
        };
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
        return LdivT {
            quot: i64::MIN,
            rem: 0,
        };
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
        return LldivT {
            quot: i64::MIN,
            rem: 0,
        };
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

// `qsort` and `qsort_r` share one engine, `qsort_core` below.
//
// That engine is an **introsort**: median-of-three quicksort, dropping to
// insertion sort on short ranges and to heapsort once the partition depth
// exceeds 2·log₂n.  It is O(n log n) in the worst case, entirely in place, and
// allocates nothing.
//
// It replaces a plain insertion sort (O(n²)) that borrowed a 256-byte stack
// buffer for the element hold and `mmap`'d one for elements larger than that.
// Both properties were defects rather than merely slow:
//
//   * O(n²) is not a theoretical concern for a libc whose first real consumer
//     is coreutils — `ls` sorts a directory, `sort` sorts a file.  A 100 000
//     entry directory is 10^10 comparisons on the old code and about 1.7×10^6
//     on this one.
//   * If the `mmap` failed the old code **returned silently, leaving the array
//     unsorted** — which a caller cannot detect, because `qsort` has no return
//     value.  Allocating nothing removes the failure mode rather than
//     reporting it.
//
// The new engine is not stable.  Neither was required: POSIX explicitly leaves
// the relative order of equal elements unspecified, and glibc's quicksort path
// is unstable too.  Callers needing stability must sort on a tie-breaking key.

/// Ranges at or below this length are insertion-sorted rather than partitioned.
///
/// Insertion sort wins on short ranges because its inner loop is branch-light
/// and touches only adjacent elements; the usual crossover for a byte-wise
/// element swap is around a dozen.
const QSORT_INSERTION_MAX: usize = 12;

/// Exchange the `size`-byte elements at `a` and `b`.
///
/// # Safety
/// `a` and `b` must each point to `size` readable and writable bytes, and must
/// either be equal or not overlap at all — which holds for two slots of one
/// array.
#[inline]
unsafe fn qsort_swap(a: *mut u8, b: *mut u8, size: usize) {
    if core::ptr::eq(a, b) {
        return;
    }
    // SAFETY: distinct elements of one array never *partially* overlap, so the
    // equality test above is enough to establish `swap_nonoverlapping`'s
    // precondition.
    unsafe { core::ptr::swap_nonoverlapping(a, b, size) };
}

/// Sort the inclusive index range `lo..=hi` by insertion, exchanging adjacent
/// elements rather than holding one aside.
///
/// Swapping costs roughly three times the memory traffic of the usual
/// shift-and-reinsert, but needs no element-sized temporary — which is what
/// lets the whole engine work without allocating, for any element size.  Over
/// ranges of at most [`QSORT_INSERTION_MAX`] the difference is noise.
///
/// # Safety
/// `base` must address at least `hi + 1` elements of `size` bytes, and `cmp`
/// must be a valid comparator over them.
unsafe fn qsort_insertion<C: Fn(*const u8, *const u8) -> i32>(
    base: *mut u8,
    size: usize,
    lo: usize,
    hi: usize,
    cmp: &C,
) {
    let mut i = lo.saturating_add(1);
    while i <= hi {
        let mut j = i;
        while j > lo {
            // SAFETY: `j <= hi` and `j - 1 >= lo`, both within the array.
            let cur = unsafe { base.add(j.wrapping_mul(size)) };
            // SAFETY: as above.
            let prev = unsafe { base.add(j.wrapping_sub(1).wrapping_mul(size)) };
            if cmp(cur.cast_const(), prev.cast_const()) >= 0 {
                break;
            }
            // SAFETY: two distinct in-bounds elements of the same array.
            unsafe { qsort_swap(cur, prev, size) };
            j = j.wrapping_sub(1);
        }
        i = i.wrapping_add(1);
    }
}

/// Restore the max-heap property at `root` within the `count`-element heap
/// based at element index `lo`.
///
/// # Safety
/// `base` must address at least `lo + count` elements of `size` bytes, `root`
/// must be `< count`, and `cmp` must be a valid comparator over them.
unsafe fn qsort_sift_down<C: Fn(*const u8, *const u8) -> i32>(
    base: *mut u8,
    size: usize,
    lo: usize,
    mut root: usize,
    count: usize,
    cmp: &C,
) {
    loop {
        let child = match root.checked_mul(2).and_then(|c| c.checked_add(1)) {
            Some(c) if c < count => c,
            _ => return,
        };
        // Pick the larger of the two children.
        let mut swap_with = child;
        let right = child.wrapping_add(1);
        if right < count {
            // SAFETY: `child` and `right` are both `< count`, hence in bounds.
            let lc = unsafe { base.add(lo.wrapping_add(child).wrapping_mul(size)) };
            // SAFETY: as above.
            let rc = unsafe { base.add(lo.wrapping_add(right).wrapping_mul(size)) };
            if cmp(rc.cast_const(), lc.cast_const()) > 0 {
                swap_with = right;
            }
        }
        // SAFETY: `root < count` and `swap_with < count`, both in bounds.
        let rp = unsafe { base.add(lo.wrapping_add(root).wrapping_mul(size)) };
        // SAFETY: as above.
        let cp = unsafe { base.add(lo.wrapping_add(swap_with).wrapping_mul(size)) };
        if cmp(rp.cast_const(), cp.cast_const()) >= 0 {
            return;
        }
        // SAFETY: two distinct in-bounds elements of the same array.
        unsafe { qsort_swap(rp, cp, size) };
        root = swap_with;
    }
}

/// Heapsort the inclusive index range `lo..=hi`.
///
/// This is introsort's escape hatch: slower than quicksort on typical input
/// but with no bad case, so reaching it bounds the whole sort at O(n log n)
/// however adversarial the data or the pivot choices.
///
/// # Safety
/// As [`qsort_insertion`].
unsafe fn qsort_heap<C: Fn(*const u8, *const u8) -> i32>(
    base: *mut u8,
    size: usize,
    lo: usize,
    hi: usize,
    cmp: &C,
) {
    let count = hi.wrapping_sub(lo).wrapping_add(1);
    if count <= 1 {
        return;
    }
    // Build the heap bottom-up, from the last parent down to the root.  That
    // is O(n), unlike inserting one element at a time.
    let mut k = count / 2;
    while k > 0 {
        k = k.wrapping_sub(1);
        // SAFETY: `k < count`; the range is in bounds per this fn's contract.
        unsafe { qsort_sift_down(base, size, lo, k, count, cmp) };
    }
    // Repeatedly move the maximum to the end of the shrinking heap.
    let mut end = count;
    while end > 1 {
        end = end.wrapping_sub(1);
        // SAFETY: `lo` and `lo + end` are both within `lo..=hi`.
        let root = unsafe { base.add(lo.wrapping_mul(size)) };
        // SAFETY: as above.
        let last = unsafe { base.add(lo.wrapping_add(end).wrapping_mul(size)) };
        // SAFETY: two distinct in-bounds elements of the same array.
        unsafe { qsort_swap(root, last, size) };
        // SAFETY: the remaining `end` elements form a heap but for the root.
        unsafe { qsort_sift_down(base, size, lo, 0, end, cmp) };
    }
}

/// Partition the inclusive range `lo..=hi` and return the pivot's final index.
///
/// Median-of-three chooses the pivot and parks it at `lo`; the scan is
/// Sedgewick's two-pointer form, which **stops on keys equal to the pivot**
/// rather than sweeping past them.  That detail is what keeps an array of
/// identical elements — the classic degenerate input, and a realistic one when
/// sorting records by a low-cardinality field — splitting evenly instead of
/// peeling off one element per pass.
///
/// # Safety
/// As [`qsort_insertion`], with `lo < hi`.
unsafe fn qsort_partition<C: Fn(*const u8, *const u8) -> i32>(
    base: *mut u8,
    size: usize,
    lo: usize,
    hi: usize,
    cmp: &C,
) -> usize {
    // SAFETY (all uses below): every index passed to `at` lies within
    // `lo..=hi`, which this function's contract places inside the array.
    let at = |i: usize| unsafe { base.add(i.wrapping_mul(size)) };

    // Median of three: order (lo, mid, hi), then park the median at `lo`.
    let mid = lo.wrapping_add(hi.wrapping_sub(lo) / 2);
    if cmp(at(mid).cast_const(), at(lo).cast_const()) < 0 {
        // SAFETY: distinct in-bounds elements.
        unsafe { qsort_swap(at(mid), at(lo), size) };
    }
    if cmp(at(hi).cast_const(), at(mid).cast_const()) < 0 {
        // SAFETY: distinct in-bounds elements.
        unsafe { qsort_swap(at(hi), at(mid), size) };
        if cmp(at(mid).cast_const(), at(lo).cast_const()) < 0 {
            // SAFETY: distinct in-bounds elements.
            unsafe { qsort_swap(at(mid), at(lo), size) };
        }
    }
    // SAFETY: distinct in-bounds elements.  The scan below never moves `lo`,
    // so the pivot stays readable throughout.
    unsafe { qsort_swap(at(mid), at(lo), size) };

    let mut i = lo;
    let mut j = hi.wrapping_add(1);
    loop {
        // Scan right for an element >= pivot.  Bounded explicitly by `hi`: the
        // pivot sits *behind* `i`, so it cannot act as a sentinel here.
        loop {
            i = i.wrapping_add(1);
            if i > hi || cmp(at(i).cast_const(), at(lo).cast_const()) >= 0 {
                break;
            }
        }
        // Scan left for an element <= pivot.  Self-limiting: the pivot at `lo`
        // compares equal to itself and stops the scan.
        loop {
            j = j.wrapping_sub(1);
            if j <= lo || cmp(at(j).cast_const(), at(lo).cast_const()) <= 0 {
                break;
            }
        }
        if i >= j {
            break;
        }
        // SAFETY: `lo < i < j <= hi`; two distinct in-bounds elements.
        unsafe { qsort_swap(at(i), at(j), size) };
    }
    // SAFETY: `j` lies within `lo..=hi`; this seats the pivot at its final index.
    unsafe { qsort_swap(at(lo), at(j), size) };
    j
}

/// What `qsort` and `qsort_r` check before they sort, as glibc 2.39's
/// `__qsort_r` meets them: fewer than two elements return at once, calling
/// nothing, so they need no `compar`.  Otherwise glibc calls `compar` and moves
/// elements through `base`, so a NULL either (with elements of some size to
/// move) is where its process faults -- and `qsort` has no way to fail, so here
/// it ends too, with a message (design-decisions.md §1115).  Until 2026-09-26
/// the parameter could not be NULL, and a NULL `base` sorted nothing.
///
/// `None` means there is nothing to do.
fn qsort_comparator<F>(base: *mut u8, nmemb: usize, size: usize, compar: Option<F>) -> Option<F> {
    if nmemb <= 1 {
        return None;
    }
    let Some(f) = compar else {
        crate::unistd::libc_fatal(b"Fatal libc error: qsort: the comparison function is NULL\n");
    };
    if size != 0 && base.is_null() {
        crate::unistd::libc_fatal(b"Fatal libc error: qsort: the array is NULL\n");
    }
    Some(f)
}

/// The shared introsort engine behind `qsort` and `qsort_r`.
///
/// Recursion is replaced by an explicit stack that always defers the *smaller*
/// partition and iterates on the larger.  Each deferred range is therefore at
/// most half the one it came from, so the stack can never exceed `usize::BITS`
/// entries however the pivots fall — which is why the fixed-size array below
/// needs no overflow path.
///
/// # Safety
/// `base` must point to at least `nmemb` elements of `size` bytes, and `cmp`
/// must be a valid comparator over them.
unsafe fn qsort_core<C: Fn(*const u8, *const u8) -> i32>(
    base: *mut u8,
    nmemb: usize,
    size: usize,
    cmp: &C,
) {
    if nmemb <= 1 || size == 0 || base.is_null() {
        return;
    }

    // Introsort's depth budget: 2·floor(log2 n).  Exceeding it means the
    // pivots have been pathologically bad (or the input was crafted to make
    // them so), and the range is finished by heapsort instead.
    let log2 = usize::BITS
        .saturating_sub(nmemb.leading_zeros())
        .saturating_sub(1) as usize;
    let depth0 = log2.saturating_mul(2).max(1);

    // (lo, hi, remaining depth); `hi` is inclusive.
    let mut stack = [(0usize, 0usize, 0usize); usize::BITS as usize];
    let mut sp = 0usize;
    let mut cur = (0usize, nmemb.wrapping_sub(1), depth0);

    loop {
        let (mut lo, mut hi, mut depth) = cur;
        while hi.wrapping_sub(lo) > QSORT_INSERTION_MAX {
            if depth == 0 {
                // SAFETY: `lo..=hi` is in bounds; see the loop invariant.
                unsafe { qsort_heap(base, size, lo, hi, cmp) };
                lo = hi;
                break;
            }
            depth = depth.wrapping_sub(1);
            // SAFETY: `lo < hi` here, since `hi - lo > QSORT_INSERTION_MAX`.
            let p = unsafe { qsort_partition(base, size, lo, hi, cmp) };

            // Either side is empty when the pivot lands at an end; carrying
            // that as `None` is what keeps `p - 1` from underflowing at
            // `p == lo`.
            let left = if p == lo {
                None
            } else {
                Some((lo, p.wrapping_sub(1)))
            };
            let right = if p == hi {
                None
            } else {
                Some((p.wrapping_add(1), hi))
            };

            // Defer the smaller side, iterate on the larger.
            let (defer, next) = if p.wrapping_sub(lo) < hi.wrapping_sub(p) {
                (left, right)
            } else {
                (right, left)
            };

            if let Some((dlo, dhi)) = defer {
                match stack.get_mut(sp) {
                    Some(slot) => {
                        *slot = (dlo, dhi, depth);
                        sp = sp.wrapping_add(1);
                    }
                    // Unreachable given the halving bound documented above.
                    // Handled anyway rather than dropped, because an unsorted
                    // tail is invisible to the caller — `qsort` has no return
                    // value with which to report that it gave up.
                    // SAFETY: `dlo..=dhi` is in bounds; see the loop invariant.
                    None => unsafe { qsort_heap(base, size, dlo, dhi, cmp) },
                }
            }
            match next {
                Some((nlo, nhi)) => {
                    lo = nlo;
                    hi = nhi;
                }
                None => {
                    lo = hi;
                    break;
                }
            }
        }
        if hi > lo {
            // SAFETY: `lo..=hi` is in bounds; see the loop invariant.
            unsafe { qsort_insertion(base, size, lo, hi, cmp) };
        }
        if sp == 0 {
            break;
        }
        sp = sp.wrapping_sub(1);
        match stack.get(sp) {
            Some(&range) => cur = range,
            // Unreachable: `sp` was non-zero and never exceeds `stack.len()`.
            None => break,
        }
    }
}

/// Sort an array using the comparison function.
///
/// Introsort — see the commentary above [`QSORT_INSERTION_MAX`].  O(n log n)
/// worst case, in place, allocation-free.  Not stable; POSIX does not require
/// it to be.
///
/// # Safety
///
/// `base` must point to an array of at least `nmemb` elements, each
/// of `size` bytes.  `compar` must be a valid comparison function, or NULL
/// for fewer than two elements (see [`qsort_comparator`]).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn qsort(
    base: *mut u8,
    nmemb: usize,
    size: usize,
    compar: Option<unsafe extern "C" fn(*const u8, *const u8) -> i32>,
) {
    let Some(compar) = qsort_comparator(base, nmemb, size, compar) else {
        return;
    };
    // SAFETY: forwarded from this function's contract; the closure only calls
    // the caller's comparator on the pointers the engine hands it.
    unsafe { qsort_core(base, nmemb, size, &|a, b| compar(a, b)) };
}

/// `qsort_r(base, nmemb, size, compar, arg)` — `qsort` with a context pointer.
///
/// The **GNU** form, in which `arg` is the comparator's *last* parameter.  (The
/// BSD/macOS `qsort_r` puts it first and reorders the comparator's arguments
/// too; the two are not interchangeable, and a program compiled for one and
/// linked against the other passes a `void *` where a `const void *` is
/// expected and crashes.  glibc and musl both use the GNU order, and so does
/// everything targeting Linux, which is what we are ABI-compatible with.)
///
/// Own archive member: gnulib ships a `qsort_r` replacement for platforms that
/// lack it, so a GNU program that vendors that module defines the symbol
/// itself and must be able to decline ours.  See `string.rs`'s module header
/// and `design-decisions.md` §339–§340.
#[cfg(target_os = "none")]
mod gnu_qsort_r {
    use super::qsort_core;

    /// See the module-level documentation.
    ///
    /// # Safety
    /// As [`super::qsort`], and `arg` must be valid for whatever `compar` does
    /// with it.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn qsort_r(
        base: *mut u8,
        nmemb: usize,
        size: usize,
        compar: Option<unsafe extern "C" fn(*const u8, *const u8, *mut core::ffi::c_void) -> i32>,
        arg: *mut core::ffi::c_void,
    ) {
        let Some(compar) = super::qsort_comparator(base, nmemb, size, compar) else {
            return;
        };
        // SAFETY: forwarded from this function's contract.
        unsafe { qsort_core(base, nmemb, size, &|a, b| compar(a, b, arg)) };
    }
}

#[cfg(target_os = "none")]
pub use gnu_qsort_r::qsort_r;

/// `qsort_r` — host build.
///
/// Written out twice rather than `cfg`-switched inside one definition: the
/// target build needs the body inside `mod gnu_qsort_r` so it lands in its own
/// archive member, and the host build should not carry an extra public module
/// that exists only to serve a linker concern.
///
/// # Safety
///
/// As [`qsort`], and `arg` must be valid for whatever `compar` does with it.
#[cfg(not(target_os = "none"))]
pub unsafe extern "C" fn qsort_r(
    base: *mut u8,
    nmemb: usize,
    size: usize,
    compar: Option<unsafe extern "C" fn(*const u8, *const u8, *mut core::ffi::c_void) -> i32>,
    arg: *mut core::ffi::c_void,
) {
    let Some(compar) = qsort_comparator(base, nmemb, size, compar) else {
        return;
    };
    // SAFETY: forwarded from this function's contract.
    unsafe { qsort_core(base, nmemb, size, &|a, b| compar(a, b, arg)) };
}

/// Binary search a sorted array.
///
/// Returns a pointer to the matching element, or NULL if not found.
///
/// glibc's (bits/stdlib-bsearch.h), which checks nothing: the elements are
/// computed from `base` and handed to `compar`, and a `size` of 0 makes every
/// element `base` -- which was "not found" without a comparison until
/// 2026-09-26.  An empty array needs no `compar`.  A NULL one with elements
/// to compare ends the process: glibc faults calling it, and "not found" is
/// the only failure `bsearch` could report, which would be a wrong answer
/// rather than a failure (design-decisions.md §1115).
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
    compar: Option<unsafe extern "C" fn(*const u8, *const u8) -> i32>,
) -> *mut u8 {
    if nmemb == 0 {
        return core::ptr::null_mut();
    }
    let Some(compar) = compar else {
        crate::unistd::libc_fatal(b"Fatal libc error: bsearch: the comparison function is NULL\n");
    };

    let mut lo: usize = 0;
    let mut hi: usize = nmemb;

    while lo < hi {
        let mid = lo.wrapping_add(hi.wrapping_sub(lo) / 2);
        // Computed, not dereferenced: the element is `compar`'s to read.
        let elem = base.wrapping_add(mid.wrapping_mul(size));
        // SAFETY: the caller's comparator on the caller's key and element.
        let cmp = unsafe { compar(key, elem) };
        match cmp.cmp(&0) {
            core::cmp::Ordering::Less => hi = mid,
            core::cmp::Ordering::Greater => lo = mid.wrapping_add(1),
            // POSIX: bsearch returns void* (mutable).  We cast here because
            // the array was received as *const but POSIX semantics permit
            // the caller to write through the returned pointer.
            core::cmp::Ordering::Equal => return elem.cast_mut(),
        }
    }

    core::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// Random number generation
// ---------------------------------------------------------------------------
//
// rand, random, the rand48 family, their reentrant forms and RAND_MAX are
// crate::prng's.

// ---------------------------------------------------------------------------
// Temporary files
// ---------------------------------------------------------------------------
//
// Each over crate::tempname -- glibc's __gen_tempname and __path_search --
// where what a template must be, and what becomes of it, is written down.
// mkstemp, mkostemp, mkstemps, mkostemps and mkdtemp are archive members of
// their own (gnulib replaces them: check-libc-shape.py's REPLACEABLE), and
// their large-file names one more, written over crate::tempname rather than
// over them: a program that brings its own mkstemp is neither given this
// library's by mkstemp64 nor has its own run by it.

/// Own archive member — gnulib replaces `mkstemp`. See string.rs's module header.
mod gnu_mkstemp {
    /// `mkstemp(template)`: a new file, named by the template with its last
    /// six `X`s made letters and digits, opened `O_RDWR`, mode 0600 before
    /// the umask.  Its descriptor, or -1 with `errno`: `EINVAL` for a
    /// template that does not end in six `X`s (or is NULL, where glibc's
    /// faults), `EEXIST` when every name tried was taken, else the `open`'s
    /// (the template then holding the name that failed).
    ///
    /// # Safety
    ///
    /// `template` must be NULL or a writable C string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn mkstemp(template: *mut u8) -> i32 {
        // SAFETY: the caller's contract.
        unsafe { crate::tempname::make_file(template, 0, 0) }
    }
}
pub use gnu_mkstemp::mkstemp;

/// `mktemp(template)`: the template's last six `X`s made a name nothing has
/// yet, as `mkstemp` makes one, but nothing created -- which is why it is
/// obsolete: the name is free when it is returned and anyone's after.
///
/// Returns `template`, always, as SUSv2 and glibc have it: empty (its first
/// byte NUL) if no name could be made, whatever the reason, with `errno`
/// saying which -- `EINVAL` for a template that does not end in six `X`s,
/// `ENOTDIR` for one inside a file, `EEXIST` when every name was taken.  (It
/// returned NULL for some of those until 2026-09-30.)  A NULL template,
/// where glibc's faults, is NULL with `EINVAL`.
///
/// # Safety
///
/// `template` must be NULL or a writable C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mktemp(template: *mut u8) -> *mut u8 {
    if template.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return core::ptr::null_mut();
    }
    // SAFETY: a writable C string, the caller's.
    if unsafe { crate::tempname::gen_tempname(template, 0, crate::tempname::Kind::NoCreate) } < 0 {
        // SAFETY: as above; the string has at least its NUL.
        unsafe { *template = 0 };
    }
    template
}

/// `tmpfile()`: a new file, read and written through the stream returned
/// (`w+b`), and removed when the stream is closed or the program exits, as
/// ISO C has it.  NULL, with `errno`, if none could be made.
///
/// glibc makes the file nameless -- `O_TMPFILE` in `/tmp` -- and failing
/// that makes `/tmp/tmpfXXXXXX` as `mkstemp` makes a name and unlinks it at
/// once, the file living on through its descriptor.  The first is tried
/// here too.  The second cannot be done as glibc does it: this kernel's
/// descriptors reach a file through its name, so unlinking an open file
/// cuts every descriptor off from it.  The name stays, then, until the
/// stream lets go of the file -- `fclose`, `freopen` onto another, `exit` --
/// and is removed then, by the process that made it (a child that inherits
/// the stream leaves it to the parent).  A program that ends otherwise
/// (`_exit`, a fault) leaves the file, which ISO C allows
/// (design-decisions.md §1151).  It never did anything else: until
/// 2026-09-30 the file was `/tmp/tmpXXXXXX` and was never removed at all.
///
/// A success leaves `errno` as it was, as glibc's does where the nameless
/// file can be made.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tmpfile() -> *mut u8 {
    use crate::fcntl::{O_EXCL, O_RDWR, O_TMPFILE};
    use crate::tempname::{FILE_MODE, Kind, gen_tempname, path_search, remove_name};
    let saved = crate::errno::get_errno();
    // glibc's __gen_tempfd: a nameless file, where there are any.
    let fd = crate::file::open(
        c"/tmp".as_ptr().cast(),
        O_RDWR | O_TMPFILE | O_EXCL,
        FILE_MODE,
    );
    if fd >= 0 {
        crate::errno::set_errno(saved);
        // SAFETY: a C string for the mode.
        let f = unsafe { crate::stdio::fdopen(fd, c"w+b".as_ptr().cast()) };
        if f.is_null() {
            let e = crate::errno::get_errno();
            crate::file::close(fd);
            crate::errno::set_errno(e);
        }
        return f;
    }
    crate::errno::set_errno(saved);
    let mut name = [0u8; crate::unistd::PATH_MAX];
    // SAFETY: NULL for the directory, a C string for the prefix.
    if unsafe { path_search(&mut name, core::ptr::null(), c"tmpf".as_ptr().cast(), false) }
        .is_none()
    {
        return core::ptr::null_mut();
    }
    // SAFETY: `path_search` wrote a C string into `name`.
    let fd = unsafe { gen_tempname(name.as_mut_ptr(), 0, Kind::File(0)) };
    if fd < 0 {
        return core::ptr::null_mut();
    }
    // The name, for the stream to remove; without room for it, the file is
    // removed now, as there would be nothing to remove it by later.
    // SAFETY: a C string.
    let held = unsafe { crate::string::strdup(name.as_ptr()) };
    let f = if held.is_null() {
        core::ptr::null_mut()
    } else {
        // SAFETY: a C string for the mode.
        unsafe { crate::stdio::fdopen(fd, c"w+b".as_ptr().cast()) }
    };
    if f.is_null() {
        let e = crate::errno::get_errno();
        crate::file::close(fd);
        remove_name(name.as_ptr());
        // SAFETY: strdup's allocation, or NULL.
        unsafe { crate::malloc::free(held) };
        crate::errno::set_errno(e);
        return core::ptr::null_mut();
    }
    // SAFETY: a stream just made, and a name `malloc`ed for it.
    unsafe { crate::stdio::hold_temporary(f, held) };
    f
}

/// `tmpfile64`: [`tmpfile`] by glibc's large-file name.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tmpfile64() -> *mut u8 {
    tmpfile()
}

// ---------------------------------------------------------------------------
// mkostemp — mkstemp with flags
// ---------------------------------------------------------------------------

/// Own archive member — gnulib replaces `mkostemp`. See string.rs's module header.
mod gnu_mkostemp {
    /// `mkostemp(template, flags)`: [`mkstemp`](super::mkstemp) with
    /// `flags` for the `open` -- `O_APPEND`, `O_CLOEXEC`, `O_SYNC` and the
    /// rest -- their access mode replaced by `O_RDWR`, as glibc's is; the
    /// rest are the `open`'s to judge.  (Until 2026-09-30 an access mode was
    /// or'd into `O_RDWR`, and `O_WRONLY` made an `open` of mode 3.)
    ///
    /// # Safety
    ///
    /// `template` must be NULL or a writable C string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn mkostemp(template: *mut u8, flags: i32) -> i32 {
        // SAFETY: the caller's contract.
        unsafe { crate::tempname::make_file(template, 0, flags) }
    }
}
pub use gnu_mkostemp::mkostemp;

// ---------------------------------------------------------------------------
// mkstemps — create a temporary file with a suffix
// ---------------------------------------------------------------------------

/// Own archive member — gnulib replaces `mkstemps`. See string.rs's module header.
mod gnu_mkstemps {
    /// `mkstemps(template, suffixlen)`: [`mkstemp`](super::mkstemp) with the
    /// six `X`s before the template's last `suffixlen` bytes, which stay
    /// (`"/tmp/fileXXXXXX.txt"`, 4).  A negative `suffixlen`, or one that
    /// leaves no room for six `X`s, is `EINVAL`.
    ///
    /// # Safety
    ///
    /// `template` must be NULL or a writable C string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn mkstemps(template: *mut u8, suffixlen: i32) -> i32 {
        // SAFETY: the caller's contract.
        unsafe { crate::tempname::make_file(template, suffixlen, 0) }
    }
}
pub use gnu_mkstemps::mkstemps;

// ---------------------------------------------------------------------------
// mkostemps — create a temporary file with suffix + flags
// ---------------------------------------------------------------------------

/// Own archive member — gnulib replaces `mkostemps`. See string.rs's module header.
mod gnu_mkostemps {
    /// `mkostemps(template, suffixlen, flags)`: [`mkstemps`](super::mkstemps)
    /// with [`mkostemp`](super::mkostemp)'s flags.
    ///
    /// # Safety
    ///
    /// `template` must be NULL or a writable C string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn mkostemps(template: *mut u8, suffixlen: i32, flags: i32) -> i32 {
        // SAFETY: the caller's contract.
        unsafe { crate::tempname::make_file(template, suffixlen, flags) }
    }
}
pub use gnu_mkostemps::mkostemps;

// ---------------------------------------------------------------------------
// The large-file names of the four
// ---------------------------------------------------------------------------

/// glibc's `mkstemp64`, `mkostemp64`, `mkstemps64` and `mkostemps64` (its
/// `O_LARGEFILE`, all they add, is 0 on x86_64): an archive member of their
/// own, over crate::tempname -- see this section's head.
mod lfs_mkstemp {
    /// `mkstemp64`: [`mkstemp`](super::mkstemp) by glibc's large-file name.
    ///
    /// # Safety
    ///
    /// `template` must be NULL or a writable C string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn mkstemp64(template: *mut u8) -> i32 {
        // SAFETY: the caller's contract.
        unsafe { crate::tempname::make_file(template, 0, 0) }
    }

    /// `mkostemp64`: [`mkostemp`](super::mkostemp) by glibc's large-file
    /// name.
    ///
    /// # Safety
    ///
    /// `template` must be NULL or a writable C string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn mkostemp64(template: *mut u8, flags: i32) -> i32 {
        // SAFETY: the caller's contract.
        unsafe { crate::tempname::make_file(template, 0, flags) }
    }

    /// `mkstemps64`: [`mkstemps`](super::mkstemps) by glibc's large-file
    /// name.
    ///
    /// # Safety
    ///
    /// `template` must be NULL or a writable C string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn mkstemps64(template: *mut u8, suffixlen: i32) -> i32 {
        // SAFETY: the caller's contract.
        unsafe { crate::tempname::make_file(template, suffixlen, 0) }
    }

    /// `mkostemps64`: [`mkostemps`](super::mkostemps) by glibc's large-file
    /// name.
    ///
    /// # Safety
    ///
    /// `template` must be NULL or a writable C string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn mkostemps64(template: *mut u8, suffixlen: i32, flags: i32) -> i32 {
        // SAFETY: the caller's contract.
        unsafe { crate::tempname::make_file(template, suffixlen, flags) }
    }
}
pub use lfs_mkstemp::{mkostemp64, mkostemps64, mkstemp64, mkstemps64};

// ---------------------------------------------------------------------------
// mkdtemp — create a unique temporary directory
// ---------------------------------------------------------------------------

/// Own archive member — gnulib replaces `mkdtemp`. See string.rs's module header.
mod gnu_mkdtemp {
    /// `mkdtemp(template)`: a new directory, named as
    /// [`mkstemp`](super::mkstemp) names a file, mode 0700 before the
    /// umask.  `template`, or NULL with `errno` as `mkstemp`'s.
    ///
    /// # Safety
    ///
    /// `template` must be NULL or a writable C string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn mkdtemp(template: *mut u8) -> *mut u8 {
        if template.is_null() {
            crate::errno::set_errno(crate::errno::EINVAL);
            return core::ptr::null_mut();
        }
        // SAFETY: a writable C string, the caller's.
        if unsafe { crate::tempname::gen_tempname(template, 0, crate::tempname::Kind::Dir) } < 0 {
            return core::ptr::null_mut();
        }
        template
    }
}
pub use gnu_mkdtemp::mkdtemp;

// ---------------------------------------------------------------------------
// system — execute a shell command
// ---------------------------------------------------------------------------

crate::perprocess::process_global! {
    /// What `system` saved of `SIGINT`'s and `SIGQUIT`'s dispositions, and
    /// how many calls are running, under [`system_lock_ptr`].
    fn system_save_ptr() -> SystemSave = SystemSave {
        users: 0,
        intr: crate::signal::DEFAULT_SIGACTION,
        quit: crate::signal::DEFAULT_SIGACTION,
    };

    /// The low-level lock over [`system_save_ptr`].
    fn system_lock_ptr() -> core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);
}

/// `SIGINT`'s and `SIGQUIT`'s dispositions as the first of the `system`
/// calls running found them, which the last puts back.
struct SystemSave {
    /// `system` calls running.
    users: u32,
    /// `SIGINT`'s action before the first.
    intr: crate::signal::Sigaction,
    /// `SIGQUIT`'s action before the first.
    quit: crate::signal::Sigaction,
}

/// Run `f` on the saved dispositions, under their lock.
fn with_system_save<R>(f: impl FnOnce(&mut SystemSave) -> R) -> R {
    // SAFETY: this process's lock, an atomic.
    let lock = unsafe { &*system_lock_ptr() };
    crate::lowlevellock::lll_lock(lock);
    // SAFETY: this process's record, which only this lock's holder touches.
    let r = f(unsafe { &mut *system_save_ptr() });
    crate::lowlevellock::lll_unlock(lock);
    r
}

/// Execute a command using the system shell.
///
/// If `command` is NULL, returns whether a shell is available (1 = yes,
/// 0 = no).  Otherwise runs `/bin/sh -c -- command` and waits for it: glibc's
/// `do_system` (`stdlib/system.c`).
///
/// - The shell gets the caller's environment.  Until 2026-10-06 it got none:
///   the call passed a NULL `envp`, which the kernel stores as "no
///   environment", so a command ran without `PATH`, `HOME` or anything its
///   caller had set -- the bug `execl` and its kin had until 2026-09-24.
/// - `--` before the command, so one that begins with `-` is not an option
///   to the shell.
/// - While it waits, the caller ignores `SIGINT` and `SIGQUIT` and blocks
///   `SIGCHLD`, as POSIX requires: a `^C` meant for the command does not end
///   the program waiting for it, and a `SIGCHLD` handler cannot reap the
///   shell first.  The shell gets `SIGINT` and `SIGQUIT` at their defaults --
///   unless the caller had ignored them itself -- and the mask the caller had
///   before `SIGCHLD` was blocked (`POSIX_SPAWN_SETSIGDEF`,
///   `POSIX_SPAWN_SETSIGMASK`).  Threads calling `system` at once share one
///   save: the first ignores the two signals and the last restores them.
///   Until 2026-10-06 none of this was done.
///
/// Returns the shell's wait status; if the shell could not be started, the
/// status of one that exited with 127, as POSIX has it, with `errno` the
/// reason; -1 if it could not be waited for.
///
/// # Safety
///
/// `command` must be a valid null-terminated string (or NULL).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn system(command: *const u8) -> i32 {
    use crate::signal::{SIG_BLOCK, SIG_IGN, SIG_SETMASK, SIGCHLD, SIGINT, SIGQUIT, Sigaction};
    use crate::signal::{SigsetT, sigaction, sigaddset, sigprocmask};

    if command.is_null() {
        // POSIX: return non-zero if a command processor is available.
        // Try to stat /bin/sh to check.
        let mut st = crate::stat::Stat::zeroed();
        let sh = b"/bin/sh\0";
        let ret = crate::file::stat(sh.as_ptr(), &raw mut st);
        return i32::from(ret == 0);
    }

    // The first call saves SIGINT's and SIGQUIT's actions and ignores both.
    let ignore = Sigaction {
        sa_handler: SIG_IGN,
        ..crate::signal::DEFAULT_SIGACTION
    };
    let (intr, quit) = with_system_save(|save| {
        if save.users == 0 {
            // Neither can fail: a valid signal, a valid action, a local.
            // SAFETY: valid actions and out-pointers.
            unsafe {
                let _ = sigaction(SIGINT, &raw const ignore, &raw mut save.intr);
                let _ = sigaction(SIGQUIT, &raw const ignore, &raw mut save.quit);
            }
        }
        save.users = save.users.saturating_add(1);
        (save.intr.sa_handler, save.quit.sa_handler)
    });

    // Block SIGCHLD; the old mask is the shell's.
    let mut chld = SigsetT::EMPTY;
    let mut old_mask = SigsetT::EMPTY;
    // SAFETY: locals.  `sigaddset` with a valid signal and `sigprocmask` with
    // SIG_BLOCK and valid sets cannot fail.
    unsafe {
        let _ = sigaddset(&raw mut chld, SIGCHLD);
        let _ = sigprocmask(SIG_BLOCK, &raw const chld, &raw mut old_mask);
    }

    // SIGINT and SIGQUIT at their defaults in the shell, unless the caller
    // ignored them itself.
    let mut reset = SigsetT::EMPTY;
    // SAFETY: a local; valid signals.
    unsafe {
        if intr != SIG_IGN {
            let _ = sigaddset(&raw mut reset, SIGINT);
        }
        if quit != SIG_IGN {
            let _ = sigaddset(&raw mut reset, SIGQUIT);
        }
    }
    // SAFETY: an all-zero attribute object is what `init` makes.
    let mut attr = unsafe { core::mem::zeroed::<crate::spawn::PosixSpawnattrT>() };
    // None of these can fail on a valid, local object with valid values.
    let _ = crate::spawn::posix_spawnattr_init(&raw mut attr);
    let _ = crate::spawn::posix_spawnattr_setsigmask(&raw mut attr, &raw const old_mask);
    let _ = crate::spawn::posix_spawnattr_setsigdefault(&raw mut attr, &raw const reset);
    let _ = crate::spawn::posix_spawnattr_setflags(
        &raw mut attr,
        crate::spawn::POSIX_SPAWN_SETSIGDEF | crate::spawn::POSIX_SPAWN_SETSIGMASK,
    );

    // POSIX: /bin/sh itself, never a search of PATH -- a hostile PATH entry
    // could hand the command to a shell of its choosing.
    let argv: [*const u8; 5] = [
        c"sh".as_ptr().cast(),
        c"-c".as_ptr().cast(),
        c"--".as_ptr().cast(),
        command,
        core::ptr::null(),
    ];
    let mut pid: crate::types::PidT = 0;
    let spawned = crate::spawn::posix_spawn(
        &raw mut pid,
        c"/bin/sh".as_ptr().cast(),
        core::ptr::null(),
        &raw const attr,
        argv.as_ptr(),
        crate::environ::current_environ(),
    );
    let _ = crate::spawn::posix_spawnattr_destroy(&raw mut attr);

    let status = if spawned == 0 {
        let mut status: i32 = 0;
        loop {
            let waited = crate::process::waitpid(pid, &raw mut status, 0);
            if waited == pid {
                break status;
            }
            if waited < 0 && crate::errno::get_errno() == crate::errno::EINTR {
                continue;
            }
            break -1;
        }
    } else {
        // An exit status of 127, as a wait status.
        127_i32.wrapping_shl(8)
    };

    // The last call puts SIGINT and SIGQUIT back.
    with_system_save(|save| {
        save.users = save.users.saturating_sub(1);
        if save.users == 0 {
            // SAFETY: the actions saved above.
            unsafe {
                let _ = sigaction(SIGINT, &raw const save.intr, core::ptr::null_mut());
                let _ = sigaction(SIGQUIT, &raw const save.quit, core::ptr::null_mut());
            }
        }
    });
    // Cannot fail: SIG_SETMASK and the mask saved above.
    let _ = sigprocmask(SIG_SETMASK, &raw const old_mask, core::ptr::null_mut());
    if spawned != 0 {
        crate::errno::set_errno(spawned);
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
// ecvt, fcvt, gcvt
// ---------------------------------------------------------------------------
//
// The legacy digit-string conversions (removed from POSIX in 2008; musl's
// <stdlib.h> declares the three, glibc also has the `_r` forms), with
// glibc's conventions -- how many digits, where `decpt` points, what zero,
// the infinities and a rounding carry look like -- and exact digits: the
// value's own, correctly rounded, which is what `printf` gives. glibc's
// `ecvt` scales the value into [1, 10) by repeated multiplication by ten, in
// floating point, and gets the last digits wrong about one call in six;
// that is not copied (design-decisions §1135).

/// The most digits `ecvt` produces and the most fraction digits `fcvt` does,
/// glibc's `NDIGIT_MAX` for a double: 17, enough to tell every double apart.
const NDIGIT_MAX: i32 = 17;
/// glibc's static buffer sizes: `NDIGIT_MAX + 3`, and for `fcvt` room for
/// `DBL_MAX`'s 309 integer digits as well.
const ECVT_BUF: usize = 20;
/// See [`ECVT_BUF`].
const FCVT_BUF: usize = 308 + 20;

/// Append `b` to `out` at `*len`; `None` when it does not fit.
fn put_digit(out: &mut [u8], len: &mut usize, b: u8) -> Option<()> {
    *out.get_mut(*len)? = b;
    *len = len.checked_add(1)?;
    Some(())
}

/// A value as the digit routines take it: a `double`'s or a `long double`'s
/// class and sign, and a finite one's magnitude, exactly expanded.
struct Cvt<D> {
    nan: bool,
    infinite: bool,
    negative: bool,
    zero: bool,
    dec: crate::decfloat::Decimal<D>,
}

impl Cvt<[u8; crate::decfloat::MAX_DIGITS]> {
    fn of_f64(value: f64) -> Self {
        Self {
            nan: value.is_nan(),
            infinite: value.is_infinite(),
            negative: value.is_sign_negative(),
            zero: value == 0.0,
            dec: crate::decfloat::Decimal::new(if value.is_finite() { value.abs() } else { 0.0 }),
        }
    }
}

impl Cvt<crate::decfloat::DigitBuf> {
    /// A `long double`: `None` when a value far outside a `double`'s range
    /// cannot get the memory its expansion needs.
    fn of_ld(l: crate::x87::LongDouble) -> Option<Self> {
        let finite = l.is_finite();
        let dec = if finite {
            crate::printf::long_expansion(l)?
        } else {
            crate::decfloat::Decimal::of_parts(0, 0)?
        };
        Some(Self {
            nan: l.is_nan(),
            infinite: l.is_infinite(),
            negative: l.is_sign_negative(),
            zero: finite && l.is_zero(),
            dec,
        })
    }
}

/// What `printf("%.*f")` writes for a non-finite value: `inf`, `-inf`,
/// `nan` or `-nan` -- which `fcvt` hands back as its "digits", with
/// `decpt` 0 and `sign` 0, as glibc does.
fn non_finite_text(nan: bool, negative: bool) -> &'static [u8] {
    match (nan, negative) {
        (true, false) => b"nan",
        (true, true) => b"-nan",
        (false, false) => b"inf",
        (false, true) => b"-inf",
    }
}

/// `fcvt_r`'s digits for `value` into `out` (unterminated): `(len, decpt,
/// sign)`, or `None` when `out` is too small.
fn fcvt_digits(value: f64, ndigit: i32, out: &mut [u8]) -> Option<(usize, i32, bool)> {
    fcvt_parts(Cvt::of_f64(value), ndigit, NDIGIT_MAX, out)
}

/// [`fcvt_digits`], for either precision: at most `max` fraction digits.
///
/// glibc's recipe, computed exactly: `printf("%.*f", min(ndigit, max))` of
/// `|value|`; the integer digits, then the fraction's, with the point
/// dropped; `decpt` the number of integer digits -- and a value below 1 that
/// is not zero has its `0.` and the zeros after it stripped, each lowering
/// `decpt`, so 0.00123 is "123" with `decpt` -2. A negative `ndigit` rounds
/// to the left of the point, to `10^-ndigit` -- but, glibc's loop, never so
/// far that the value would drop below 1: 5 with `ndigit` -2 stays "5".
fn fcvt_parts<D: AsRef<[u8]> + AsMut<[u8]>>(
    v: Cvt<D>,
    ndigit: i32,
    max: i32,
    out: &mut [u8],
) -> Option<(usize, i32, bool)> {
    let mut len = 0usize;
    if v.nan || v.infinite {
        for &b in non_finite_text(v.nan, v.negative) {
            put_digit(out, &mut len, b)?;
        }
        return Some((len, 0, false));
    }
    let sign = v.negative;
    let mut dec = v.dec;
    // The place to round at, and the fraction digits written.
    let precision = if ndigit < 0 {
        // Integer digits, 0 below 1; the scaling stops at one.
        let int_digits = if dec.is_zero() { 0 } else { dec.decpt().max(0) };
        let k = if int_digits >= 2 {
            ndigit.saturating_neg().min(int_digits.saturating_sub(1))
        } else {
            0
        };
        dec.round_to_place_in(
            k.saturating_neg(),
            crate::decfloat::Rounding::current(),
            sign,
        );
        0
    } else {
        let p = ndigit.min(max);
        dec.round_to_place_in(p, crate::decfloat::Rounding::current(), sign);
        p
    };
    let decpt = dec.decpt();
    // The integer digits `%f` writes: at least one.
    let int_len = if dec.is_zero() || decpt <= 0 {
        1
    } else {
        decpt
    };
    let strip = precision > 0 && !v.zero && (dec.is_zero() || decpt <= 0);
    if !strip {
        for i in 0..int_len {
            let d = if dec.is_zero() || decpt <= 0 {
                b'0'
            } else {
                dec.digit(i)
            };
            put_digit(out, &mut len, d)?;
        }
        for j in 0..precision {
            put_digit(out, &mut len, dec.digit(decpt.saturating_add(j)))?;
        }
        return Some((len, int_len, sign));
    }
    // A nonzero value below 1: the `0.` goes, and each zero after it.
    let mut dp = 0i32;
    let mut leading = true;
    for j in 0..precision {
        let d = if dec.is_zero() {
            b'0'
        } else {
            dec.digit(decpt.saturating_add(j))
        };
        if leading && d == b'0' {
            dp = dp.saturating_sub(1);
            continue;
        }
        leading = false;
        put_digit(out, &mut len, d)?;
    }
    Some((len, dp, sign))
}

/// `ecvt_r`'s digits for `value` into `out` (unterminated): `(len, decpt,
/// sign)`, or `None` when `out` is too small.
fn ecvt_digits(value: f64, ndigit: i32, out: &mut [u8]) -> Option<(usize, i32, bool)> {
    ecvt_parts(Cvt::of_f64(value), ndigit, NDIGIT_MAX, out)
}

/// [`ecvt_digits`], for either precision: at most `max` digits.
///
/// `min(ndigit, max)` significant digits, correctly rounded, `decpt` where
/// the point goes. glibc's conventions: an `ndigit` of 0 or less is no
/// digits, `decpt` still the value's; zero is that many zeros with `decpt`
/// 1; the infinities and NaNs are `fcvt`'s text; and a rounding that carries
/// into a new leading digit is written with one digit more -- 9.9999 to one
/// digit is "10", `decpt` 2 -- as glibc's scaled `fcvt` writes it.
fn ecvt_parts<D: AsRef<[u8]> + AsMut<[u8]>>(
    v: Cvt<D>,
    ndigit: i32,
    max: i32,
    out: &mut [u8],
) -> Option<(usize, i32, bool)> {
    let finite = !v.nan && !v.infinite;
    if ndigit <= 0 {
        // No digits -- but `decpt` still says where the point is, as glibc's
        // adds the value's exponent after its early branch: 1 for zero and
        // the non-finite, else the value's own.
        let decpt = if finite && !v.zero { v.dec.decpt() } else { 1 };
        return Some((0, decpt, finite && v.negative));
    }
    if !finite {
        return fcvt_parts(v, 0, max, out);
    }
    let n = ndigit.min(max);
    let mut len = 0usize;
    if v.zero {
        for _ in 0..n {
            put_digit(out, &mut len, b'0')?;
        }
        return Some((len, 1, v.negative));
    }
    let negative = v.negative;
    let mut dec = v.dec;
    let before = dec.decpt();
    dec.round_to_significant_in(n, crate::decfloat::Rounding::current(), negative);
    let carried = dec.decpt() > before;
    let digits = if carried { n.saturating_add(1) } else { n };
    for i in 0..digits {
        put_digit(out, &mut len, dec.digit(i))?;
    }
    Some((len, dec.decpt(), negative))
}

/// Write `(digits, decpt, sign)` out through the C pointers; `buf` gets the
/// terminator after `len`. `-1` for a result that did not fit.
///
/// # Safety
///
/// `decpt` and `sign` are NULL or valid `int *`s; `buf` holds `buflen` bytes.
unsafe fn cvt_finish(
    r: Option<(usize, i32, bool)>,
    buf: *mut u8,
    buflen: usize,
    decpt: *mut i32,
    sign: *mut i32,
) -> i32 {
    let Some((len, dp, sg)) = r else {
        return -1;
    };
    if len >= buflen {
        return -1;
    }
    // SAFETY: this function's contract; `len < buflen`.
    unsafe {
        buf.add(len).write(0);
        if let Some(d) = decpt.as_mut() {
            *d = dp;
        }
        if let Some(s) = sign.as_mut() {
            *s = i32::from(sg);
        }
    }
    0
}

/// [`fcvt`] into the caller's buffer (GNU): 0, or -1 when it does not fit,
/// or `EINVAL` for a NULL `buf`.
///
/// # Safety
///
/// `buf` is NULL or holds `len` bytes; `decpt` and `sign` valid `int *`s.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fcvt_r(
    value: f64,
    ndigit: i32,
    decpt: *mut i32,
    sign: *mut i32,
    buf: *mut u8,
    len: usize,
) -> i32 {
    if buf.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    // SAFETY: `len` bytes at `buf`, the caller's.
    let out = unsafe { core::slice::from_raw_parts_mut(buf, len) };
    let r = fcvt_digits(value, ndigit, out);
    // SAFETY: this function's contract.
    unsafe { cvt_finish(r, buf, len, decpt, sign) }
}

/// [`ecvt`] into the caller's buffer (GNU); as [`fcvt_r`].
///
/// # Safety
///
/// As for [`fcvt_r`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ecvt_r(
    value: f64,
    ndigit: i32,
    decpt: *mut i32,
    sign: *mut i32,
    buf: *mut u8,
    len: usize,
) -> i32 {
    if buf.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    // SAFETY: `len` bytes at `buf`, the caller's.
    let out = unsafe { core::slice::from_raw_parts_mut(buf, len) };
    let r = ecvt_digits(value, ndigit, out);
    // SAFETY: this function's contract.
    unsafe { cvt_finish(r, buf, len, decpt, sign) }
}

/// `value`'s first `ndigit` significant digits (at most 17), as a string in
/// storage the next call reuses, with the point's position in `*decpt` and
/// the sign in `*sign`. See [`ecvt_digits`] for the conventions.
///
/// # Safety
///
/// `decpt` and `sign` are valid `int *`s. Not thread-safe (one buffer), as
/// in every C library.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ecvt(value: f64, ndigit: i32, decpt: *mut i32, sign: *mut i32) -> *mut u8 {
    static mut BUF: [u8; ECVT_BUF] = [0; ECVT_BUF];
    let buf = core::ptr::addr_of_mut!(BUF).cast::<u8>();
    // SAFETY: the static buffer's own size; the caller's pointers. A result
    // always fits: 18 digits, a terminator.
    unsafe {
        let _ = ecvt_r(value, ndigit, decpt, sign, buf, ECVT_BUF);
    }
    buf
}

/// `value` with `ndigit` fraction digits (at most 17; a negative `ndigit`
/// rounds left of the point), as a string of digits without the point, in
/// storage the next call reuses. See [`fcvt_digits`] for the conventions.
///
/// # Safety
///
/// As for [`ecvt`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fcvt(value: f64, ndigit: i32, decpt: *mut i32, sign: *mut i32) -> *mut u8 {
    static mut BUF: [u8; FCVT_BUF] = [0; FCVT_BUF];
    let buf = core::ptr::addr_of_mut!(BUF).cast::<u8>();
    // SAFETY: as in `ecvt`; `DBL_MAX` with 17 fraction digits fits.
    unsafe {
        let _ = fcvt_r(value, ndigit, decpt, sign, buf, FCVT_BUF);
    }
    buf
}

/// `sprintf(buf, "%.*g", min(ndigit, 17), value)`: glibc's `gcvt`.
///
/// # Safety
///
/// `buf` has room for the result: 25 bytes always do.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn gcvt(value: f64, ndigit: i32, buf: *mut u8) -> *mut u8 {
    let p = usize::try_from(ndigit.clamp(0, NDIGIT_MAX)).unwrap_or(0);
    // SAFETY: this function's contract.
    unsafe { crate::printf::format_g_into(buf, value, p) };
    buf
}

// ---------------------------------------------------------------------------
// qecvt, qfcvt, qgcvt: the same, for a long double
// ---------------------------------------------------------------------------
//
// glibc's `q` forms (<stdlib.h>, `__USE_MISC`): the conventions above with
// glibc's limit for a `long double`, 21 digits, and exact digits here too --
// glibc's scale the value by repeated multiplication by ten in `long double`
// arithmetic and get the last digit wrong some of the time
// (`posix/tools/oracle/qcvt_harness.py` counts 156 of 2,160 calls;
// design-decisions §1135). A `long double` reaches each through
// `ld_abi.rs`'s thunk, by pointer.

/// glibc's `NDIGIT_MAX` for a `long double`.
const QNDIGIT_MAX: i32 = 21;
/// glibc's `qecvt` buffer: `NDIGIT_MAX + 12`.
const QECVT_BUF: usize = 33;
/// glibc's `qfcvt` buffer: `LDBL_MAX_10_EXP` more, for `LDBL_MAX`'s 4,933
/// integer digits.
const QFCVT_BUF: usize = 4932 + 33;

mod q_forms {
    use super::{Cvt, QECVT_BUF, QFCVT_BUF, QNDIGIT_MAX, cvt_finish, ecvt_parts, fcvt_parts};
    use crate::x87::LongDouble;

    /// The `_r` forms' shared body.
    ///
    /// # Safety
    ///
    /// `value` is a readable `long double`; `buf` NULL or `len` bytes;
    /// `decpt` and `sign` NULL or valid.
    unsafe fn q_r(
        ecvt: bool,
        value: *const LongDouble,
        ndigit: i32,
        decpt: *mut i32,
        sign: *mut i32,
        buf: *mut u8,
        len: usize,
    ) -> i32 {
        if buf.is_null() {
            crate::errno::set_errno(crate::errno::EINVAL);
            return -1;
        }
        // SAFETY: the thunk's pointer to the caller's argument.
        let Some(v) = Cvt::of_ld(unsafe { value.read() }) else {
            crate::errno::set_errno(crate::errno::ENOMEM);
            return -1;
        };
        // SAFETY: `len` bytes at `buf`, the caller's.
        let out = unsafe { core::slice::from_raw_parts_mut(buf, len) };
        let r = if ecvt {
            ecvt_parts(v, ndigit, QNDIGIT_MAX, out)
        } else {
            fcvt_parts(v, ndigit, QNDIGIT_MAX, out)
        };
        // SAFETY: this function's contract.
        unsafe { cvt_finish(r, buf, len, decpt, sign) }
    }

    /// `qecvt_r` (glibc): [`ecvt_r`](super::ecvt_r) of a `long double`, at
    /// most 21 digits.
    ///
    /// # Safety
    ///
    /// As `ecvt_r`, and `value` a readable `long double`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn __slate_ld_qecvt_r(
        value: *const LongDouble,
        ndigit: i32,
        decpt: *mut i32,
        sign: *mut i32,
        buf: *mut u8,
        len: usize,
    ) -> i32 {
        // SAFETY: this function's contract.
        unsafe { q_r(true, value, ndigit, decpt, sign, buf, len) }
    }
    crate::ld_c!(i_lipppn "qecvt_r" => __slate_ld_qecvt_r);

    /// `qfcvt_r` (glibc): [`fcvt_r`](super::fcvt_r) of a `long double`, at
    /// most 21 fraction digits.
    ///
    /// # Safety
    ///
    /// As `fcvt_r`, and `value` a readable `long double`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn __slate_ld_qfcvt_r(
        value: *const LongDouble,
        ndigit: i32,
        decpt: *mut i32,
        sign: *mut i32,
        buf: *mut u8,
        len: usize,
    ) -> i32 {
        // SAFETY: this function's contract.
        unsafe { q_r(false, value, ndigit, decpt, sign, buf, len) }
    }
    crate::ld_c!(i_lipppn "qfcvt_r" => __slate_ld_qfcvt_r);

    /// `qecvt` (glibc): [`qecvt_r`](__slate_ld_qecvt_r) into storage the
    /// next call reuses.
    ///
    /// # Safety
    ///
    /// `value` is a readable `long double`; `decpt` and `sign` valid. Not
    /// thread-safe (one buffer), as in every C library.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn __slate_ld_qecvt(
        value: *const LongDouble,
        ndigit: i32,
        decpt: *mut i32,
        sign: *mut i32,
    ) -> *mut u8 {
        static mut BUF: [u8; QECVT_BUF] = [0; QECVT_BUF];
        let buf = core::ptr::addr_of_mut!(BUF).cast::<u8>();
        // SAFETY: the static buffer's own size, which every result fits: 22
        // digits and a terminator. A value whose expansion finds no memory
        // leaves the buffer as it was, as nothing better can be said here.
        let _ = unsafe { q_r(true, value, ndigit, decpt, sign, buf, QECVT_BUF) };
        buf
    }
    crate::ld_c!(p_lipp "qecvt" => __slate_ld_qecvt);

    /// `qfcvt` (glibc): [`qfcvt_r`](__slate_ld_qfcvt_r) into storage the
    /// next call reuses.
    ///
    /// # Safety
    ///
    /// As [`__slate_ld_qecvt`].
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn __slate_ld_qfcvt(
        value: *const LongDouble,
        ndigit: i32,
        decpt: *mut i32,
        sign: *mut i32,
    ) -> *mut u8 {
        static mut BUF: [u8; QFCVT_BUF] = [0; QFCVT_BUF];
        let buf = core::ptr::addr_of_mut!(BUF).cast::<u8>();
        // SAFETY: as in qecvt; `LDBL_MAX` with 21 fraction digits fits.
        let _ = unsafe { q_r(false, value, ndigit, decpt, sign, buf, QFCVT_BUF) };
        buf
    }
    crate::ld_c!(p_lipp "qfcvt" => __slate_ld_qfcvt);

    /// `qgcvt` (glibc): `sprintf(buf, "%.*Lg", min(ndigit, 21), value)`.
    ///
    /// # Safety
    ///
    /// `value` is a readable `long double`; `buf` has room for the result:
    /// 32 bytes always do.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn __slate_ld_qgcvt(
        value: *const LongDouble,
        ndigit: i32,
        buf: *mut u8,
    ) -> *mut u8 {
        let p = usize::try_from(ndigit.clamp(0, QNDIGIT_MAX)).unwrap_or(0);
        // SAFETY: this function's contract.
        unsafe { crate::printf::format_lg_into(buf, value.read(), p) };
        buf
    }
    crate::ld_c!(p_lip "qgcvt" => __slate_ld_qgcvt);
}
pub use q_forms::{
    __slate_ld_qecvt, __slate_ld_qecvt_r, __slate_ld_qfcvt, __slate_ld_qfcvt_r, __slate_ld_qgcvt,
};

// ---------------------------------------------------------------------------
// getsubopt — parse suboption strings
// ---------------------------------------------------------------------------

/// Own archive member — gnulib replaces `getsubopt`. See string.rs's module header.
mod gnu_getsubopt {
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
                        unsafe {
                            *valuep = opt.add(ep.wrapping_add(1));
                        }
                    } else {
                        unsafe {
                            *valuep = core::ptr::null_mut();
                        }
                    }

                    // Advance optionp past this suboption.
                    if unsafe { *opt.add(end) } == b',' {
                        unsafe {
                            *optionp = opt.add(end.wrapping_add(1));
                        }
                    } else {
                        unsafe {
                            *optionp = opt.add(end);
                        }
                    }

                    // Null-terminate the key portion (write '\0' at '=' or end).
                    unsafe {
                        *opt.add(key_len) = 0;
                    }

                    return idx;
                }
            }

            idx = idx.wrapping_add(1);
        }

        // No match — still advance past this suboption.
        if let Some(ep) = eq_pos {
            unsafe {
                *valuep = opt.add(ep.wrapping_add(1));
            }
        } else {
            unsafe {
                *valuep = core::ptr::null_mut();
            }
        }
        if unsafe { *opt.add(end) } == b',' {
            unsafe {
                *optionp = opt.add(end.wrapping_add(1));
            }
        } else {
            unsafe {
                *optionp = opt.add(end);
            }
        }
        unsafe {
            *opt.add(key_len) = 0;
        }

        -1
    }
}
pub use gnu_getsubopt::getsubopt;

// ---------------------------------------------------------------------------
// a64l / l64a — base-64 encoding (POSIX XSI)
// ---------------------------------------------------------------------------

/// Base-64 digit set for `a64l`/`l64a`.
///
/// POSIX XSI base-64 encoding:
///   '.' = 0, '/' = 1, '0'-'9' = 2-11, 'A'-'Z' = 12-37, 'a'-'z' = 38-63
const BASE64_DIGITS: &[u8; 64] =
    b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Decode a single base-64 digit to its 6-bit value.
///
/// Returns -1 for invalid characters.
fn base64_decode_digit(c: u8) -> i32 {
    // Each match arm gates `c - b'X'` with the corresponding range
    // pattern, so the subtraction is in `0..=25` (or 0..=9).  The
    // subsequent `+ N` adds tiny constants well within i32 range.
    #[allow(clippy::arithmetic_side_effects)]
    match c {
        b'.' => 0,
        b'/' => 1,
        b'0'..=b'9' => i32::from(c - b'0') + 2,
        b'A'..=b'Z' => i32::from(c - b'A') + 12,
        b'a'..=b'z' => i32::from(c - b'a') + 38,
        _ => -1,
    }
}

/// `a64l` — convert a base-64 string to a long integer.
///
/// Decodes up to 6 characters of the POSIX XSI base-64 encoding.
/// Returns 0 for null or empty strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn a64l(s: *const u8) -> i64 {
    if s.is_null() {
        return 0;
    }

    let mut result: i64 = 0;
    let mut shift: u32 = 0;
    let mut i: usize = 0;

    while i < 6 {
        // SAFETY: we stop at the null terminator.
        let c = unsafe { *s.add(i) };
        if c == 0 {
            break;
        }
        let val = base64_decode_digit(c);
        if val < 0 {
            break; // Invalid character — stop.
        }
        result |= i64::from(val) << shift;
        shift = shift.wrapping_add(6);
        i = i.wrapping_add(1);
    }

    result
}

/// Static buffer for `l64a` output (max 7 bytes: 6 digits + null).
///
/// `l64a` returns a pointer *into* this buffer, which makes it the same hazard
/// as `strtok`'s save pointer rather than merely a racy counter: the bytes stay
/// shared while the caller is still reading them, so a concurrent `l64a`
/// rewrites a string another thread is midway through. POSIX says as much — the
/// returned pointer "may be a pointer into a static buffer that is overwritten
/// by each call" — which is what makes serialisation the caller's obligation.
/// `tests::lock_l64a_for_test` discharges it here.
static mut L64A_BUF: [u8; 7] = [0; 7];

/// `l64a` — convert a long integer to a base-64 string.
///
/// Encodes the low 32 bits of `n` into POSIX XSI base-64 format.
/// Returns a pointer to a static buffer (not thread-safe).
/// Returns "" for n == 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn l64a(n: i64) -> *const u8 {
    let buf = &raw mut L64A_BUF;

    if n == 0 {
        unsafe {
            (*buf)[0] = 0;
        }
        return unsafe { (*buf).as_ptr() };
    }

    let mut val = n as u32; // Use low 32 bits.
    let mut i: usize = 0;
    // Loop bound `i < 6` and `digit = val & 0x3F < 64 == BASE64_DIGITS
    // .len()` keep both indices in range; final `(*buf)[i]` after the
    // loop has `i <= 6 < 7 == L64A_BUF.len()`.
    #[allow(clippy::indexing_slicing)]
    while val != 0 && i < 6 {
        let digit = (val & 0x3F) as usize;
        unsafe {
            (*buf)[i] = BASE64_DIGITS[digit];
        }
        val >>= 6;
        i = i.wrapping_add(1);
    }
    #[allow(clippy::indexing_slicing)]
    unsafe {
        (*buf)[i] = 0;
    } // Null terminate.

    unsafe { (*buf).as_ptr() }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- ecvt / fcvt / gcvt against glibc 2.39 --

    /// glibc's answers (`posix/tools/oracle/cvt_harness.py`): one call a
    /// line, `<fn> <bits> <ndigit> = <digits>|<decpt>|<sign> <exact>` for
    /// ecvt and fcvt, `gcvt <bits> <ndigit> = <text>`.
    const CVT_ORACLE: &str = include_str!("cvt_oracle.txt");

    fn cvt(f: &str, x: f64, n: i32) -> (String, i32, i32) {
        let (mut dp, mut sg) = (i32::MIN, i32::MIN);
        // SAFETY: valid out-pointers; the result is the function's buffer,
        // NUL-terminated.
        let s = unsafe {
            let r = if f == "ecvt" {
                ecvt(x, n, &raw mut dp, &raw mut sg)
            } else {
                fcvt(x, n, &raw mut dp, &raw mut sg)
            };
            core::ffi::CStr::from_ptr(r.cast())
                .to_string_lossy()
                .into_owned()
        };
        (s, dp, sg)
    }

    /// What `ecvt` should answer when glibc's own digits are wrong: the
    /// value's first `n` digits, correctly rounded, from Rust's formatter --
    /// an implementation independent of this library's -- written with
    /// glibc's conventions (one digit more when rounding carried).
    fn exact_ecvt(x: f64, n: i32) -> (String, i32) {
        // The value's own decpt, from its exact expansion: 800 digits never
        // round a double, which has at most 767 significant ones.
        let own = if x.is_finite() && x != 0.0 {
            let t = format!("{:.800e}", x.abs());
            t.split_once('e').unwrap().1.parse::<i32>().unwrap() + 1
        } else {
            1
        };
        if n <= 0 {
            return (String::new(), own);
        }
        let n = n.min(17);
        let t = format!("{:.*e}", usize::try_from(n - 1).unwrap(), x.abs());
        let (m, e) = t.split_once('e').unwrap();
        let mut digits: String = m.chars().filter(char::is_ascii_digit).collect();
        let dp = e.parse::<i32>().unwrap() + 1;
        if x != 0.0 && dp > own {
            digits.push('0');
        }
        (digits, dp)
    }

    /// What `fcvt` with a negative `ndigit` should answer when glibc's digits
    /// are wrong (it divides by ten in floating point): |x| rounded, ties to
    /// even, at 10^k -- k = -ndigit, but at most one less than the integer
    /// digits, and 0 below 10, where glibc's loop stops -- from Rust's exact
    /// expansion of the value, then the k zeros.
    fn exact_fcvt(x: f64, n: i32) -> (String, i32) {
        assert!(n < 0, "glibc's fcvt is exact for ndigit >= 0");
        let t = format!("{:.1100}", x.abs());
        let (int, frac) = t.split_once('.').unwrap();
        let k = if x.abs() >= 10.0 {
            usize::try_from(-n).unwrap().min(int.len() - 1)
        } else {
            0
        };
        let (kept, dropped) = int.split_at(int.len() - k);
        let rest = format!("{dropped}{frac}");
        let last_odd = kept.bytes().last().is_some_and(|d| (d - b'0') % 2 == 1);
        let up = match rest.as_bytes().first() {
            Some(b'6'..=b'9') => true,
            Some(b'5') => rest[1..].bytes().any(|d| d != b'0') || last_odd,
            _ => false,
        };
        let mut r = kept.as_bytes().to_vec();
        if up {
            let mut i = r.len();
            loop {
                if i == 0 {
                    r.insert(0, b'1');
                    break;
                }
                i -= 1;
                if r[i] == b'9' {
                    r[i] = b'0';
                } else {
                    r[i] += 1;
                    break;
                }
            }
        }
        let digits = String::from_utf8(r).unwrap() + &"0".repeat(k);
        let dp = i32::try_from(digits.len()).unwrap();
        (digits, dp)
    }

    /// glibc 2.39's `strtod`, `strtof` and `strtold` of every input the
    /// conversion oracle holds, in each of the four rounding directions
    /// (`posix/tools/oracle/conv_harness.py`): the value's bits, the bytes
    /// consumed, and `errno`. `strtold`'s answers include literals of
    /// thousands of digits, exact rounding boundaries written out in full;
    /// `wcstold` must give each of them too, from the same text widened.
    #[test]
    fn strtod_strtof_and_strtold_answer_as_glibc_does_in_every_rounding_mode() {
        let modes = [
            crate::fenv::FE_TONEAREST,
            crate::fenv::FE_UPWARD,
            crate::fenv::FE_DOWNWARD,
            crate::fenv::FE_TOWARDZERO,
        ];
        let mut bad = Vec::new();
        let mut calls = 0;
        for line in crate::decfloat::CONV_ORACLE
            .lines()
            .filter(|l| l.starts_with("s "))
        {
            let (lhs, want) = line.split_once(" = ").unwrap();
            let w: Vec<&str> = lhs.split(' ').collect();
            let f = w[2];
            let mode: usize = w[1].parse().unwrap();
            let mut input: Vec<u8> = (0..w[3].len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&w[3][i..i + 2], 16).unwrap())
                .collect();
            input.push(0);
            assert_eq!(crate::fenv::fesetround(modes[mode]), 0);
            crate::errno::set_errno(0);
            let mut end: *const u8 = core::ptr::null();
            // SAFETY: a NUL-terminated input and an out-pointer of this frame.
            let bits = unsafe {
                match f {
                    "strtod" => format!("{:016x}", strtod(input.as_ptr(), &raw mut end).to_bits()),
                    "strtof" => format!("{:08x}", strtof(input.as_ptr(), &raw mut end).to_bits()),
                    "strtold" => {
                        let v = strtold(input.as_ptr(), &raw mut end);
                        format!("{:04x}:{:016x}", v.sign_exp, v.significand)
                    }
                    other => panic!("oracle function {other}"),
                }
            };
            let err = crate::errno::get_errno();
            let used = end as usize - input.as_ptr() as usize;
            let got = format!("{bits} {used} {err}");
            let wide_got = (f == "strtold").then(|| {
                let wide: Vec<crate::wchar::WcharT> = input
                    .iter()
                    .map(|&b| crate::wchar::WcharT::from(b))
                    .collect();
                crate::errno::set_errno(0);
                let mut wend: *const crate::wchar::WcharT = core::ptr::null();
                // SAFETY: as above, over the widened copy.
                let v = unsafe { crate::wchar::wcstold(wide.as_ptr(), &raw mut wend) };
                let wused = (wend as usize - wide.as_ptr() as usize)
                    / core::mem::size_of::<crate::wchar::WcharT>();
                let werr = crate::errno::get_errno();
                format!("{:04x}:{:016x} {wused} {werr}", v.sign_exp, v.significand)
            });
            assert_eq!(crate::fenv::fesetround(crate::fenv::FE_TONEAREST), 0);
            calls += 1;
            if got != want {
                bad.push((
                    mode,
                    format!("{}\n    ours {got}", &line[..line.len().min(300)]),
                ));
            }
            if let Some(wide_got) = wide_got.filter(|g| g != want) {
                bad.push((
                    mode,
                    format!(
                        "wcstold {}\n    ours {wide_got}",
                        &line[..line.len().min(300)]
                    ),
                ));
            }
        }
        assert!(calls > 1000, "only {calls} calls");
        let mut per_mode = [0usize; 4];
        for (m, _) in &bad {
            per_mode[*m] += 1;
        }
        // Round-to-nearest first: it is the mode almost every program runs in.
        bad.sort_by_key(|(m, _)| *m);
        assert!(
            bad.is_empty(),
            "{} of {calls} differ (by mode {per_mode:?}):\n{}",
            bad.len(),
            bad.iter()
                .take(60)
                .map(|(_, s)| s.clone())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn ecvt_fcvt_and_gcvt_answer_as_glibc_does_but_exactly() {
        let mut bad = Vec::new();
        let mut n_rows = 0;
        for line in CVT_ORACLE.lines().filter(|l| !l.is_empty()) {
            n_rows += 1;
            let (lhs, rhs) = line.split_once(" = ").unwrap();
            let mut w = lhs.split(' ');
            let (f, bits, n) = (w.next().unwrap(), w.next().unwrap(), w.next().unwrap());
            let x = f64::from_bits(u64::from_str_radix(bits, 16).unwrap());
            let n: i32 = n.parse().unwrap();
            if f == "gcvt" {
                let mut buf = [0u8; 64];
                // SAFETY: 64 bytes hold any `%.17g`.
                let got = unsafe {
                    gcvt(x, n, buf.as_mut_ptr());
                    core::ffi::CStr::from_ptr(buf.as_ptr().cast())
                        .to_string_lossy()
                        .into_owned()
                };
                if got != rhs {
                    bad.push(format!("{line}\n    ours {got}"));
                }
                continue;
            }
            let (want, exact) = rhs.rsplit_once(' ').unwrap();
            let mut parts = want.split('|');
            let (wd, wdp, wsg) = (
                parts.next().unwrap(),
                parts.next().unwrap(),
                parts.next().unwrap(),
            );
            let (gd, gdp, gsg) = cvt(f, x, n);
            let ok = if exact == "1" {
                gd == wd && gdp.to_string() == wdp && gsg.to_string() == wsg
            } else {
                let (ed, edp) = if f == "ecvt" {
                    exact_ecvt(x, n)
                } else {
                    exact_fcvt(x, n)
                };
                // Not glibc's length or decpt either: its inexact scaling can
                // carry where the value does not (1e23 to the hundreds is
                // 99999999999999991611400, not "1" and 23 zeros).
                gd == ed && gdp == edp && gsg.to_string() == wsg
            };
            if !ok {
                bad.push(format!("{line}\n    ours {gd}|{gdp}|{gsg}"));
            }
        }
        assert!(n_rows > 5000, "only {n_rows} rows");
        assert!(
            bad.is_empty(),
            "{} of {n_rows} differ:\n{}",
            bad.len(),
            bad.join("\n")
        );
    }

    /// glibc's `q` forms (`posix/tools/oracle/qcvt_harness.py`): one call a
    /// line, `<fn> <sexp>:<significand> <ndigit> = <glibc's digits>|<decpt>|
    /// <sign> <exact digits>|<exact decpt>` for qecvt and qfcvt, `qgcvt ... =
    /// <text>`; `\x` is the empty string.
    const QCVT_ORACLE: &str = include_str!("qcvt_oracle.txt");

    /// Every call of the oracle answered with the value's exact digits --
    /// glibc's own where they are exact, which is all but 156 -- glibc's
    /// sign, and glibc's `qgcvt` text.
    #[test]
    fn qecvt_qfcvt_and_qgcvt_answer_as_glibc_does_but_exactly() {
        let unesc = |t: &str| if t == "\\x" { "" } else { t }.to_owned();
        let (mut n, mut inexact) = (0, 0);
        let mut bad = Vec::new();
        for line in QCVT_ORACLE.lines().filter(|l| !l.starts_with('#')) {
            let (head, rest) = line.split_once(" = ").unwrap();
            let mut h = head.split(' ');
            let (f, bits, nd) = (h.next().unwrap(), h.next().unwrap(), h.next().unwrap());
            let nd: i32 = nd.parse().unwrap();
            let (se, sig) = bits.split_once(':').unwrap();
            let x = crate::x87::LongDouble::from_bits(
                u16::from_str_radix(se, 16).unwrap(),
                u64::from_str_radix(sig, 16).unwrap(),
            );
            n += 1;
            if f == "qgcvt" {
                let mut buf = [0u8; 64];
                // SAFETY: a long double and a buffer of this frame's.
                unsafe { __slate_ld_qgcvt(&raw const x, nd, buf.as_mut_ptr()) };
                let got = core::ffi::CStr::from_bytes_until_nul(&buf).unwrap();
                if got.to_str().unwrap() != unesc(rest) {
                    bad.push(format!("{line}\n    ours {got:?}"));
                }
                continue;
            }
            let (glibc, exact) = rest.split_once(' ').unwrap();
            let mut g = glibc.split('|');
            let (gd, gdp, gsg) = (g.next().unwrap(), g.next().unwrap(), g.next().unwrap());
            let (xd, xdp) = exact.split_once('|').unwrap();
            if (gd, gdp) != (xd, xdp) {
                inexact += 1;
            }
            let (mut dp, mut sg) = (i32::MIN, i32::MIN);
            // SAFETY: a long double and out-pointers of this frame's; the
            // result is the function's buffer, terminated.
            let got = unsafe {
                let r = if f == "qecvt" {
                    __slate_ld_qecvt(&raw const x, nd, &raw mut dp, &raw mut sg)
                } else {
                    __slate_ld_qfcvt(&raw const x, nd, &raw mut dp, &raw mut sg)
                };
                core::ffi::CStr::from_ptr(r.cast())
                    .to_string_lossy()
                    .into_owned()
            };
            if (got.as_str(), dp.to_string(), sg.to_string())
                != (unesc(xd).as_str(), xdp.to_owned(), gsg.to_owned())
            {
                bad.push(format!("{line}\n    ours {got}|{dp}|{sg}"));
            }
        }
        assert_eq!(n, 2160, "calls");
        assert!(
            inexact >= 100,
            "glibc's inexact answers, held to the exact: {inexact}"
        );
        assert!(
            bad.is_empty(),
            "{} of {n} differ:\n{}",
            bad.len(),
            bad.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
        );
    }

    /// The `q` `_r` forms refuse as the double ones do: `EINVAL` for a NULL
    /// buffer, -1 for one too small.
    #[test]
    fn the_q_r_forms_refuse_a_null_or_small_buffer() {
        let x = crate::x87::LongDouble::from_bits(0x3FFF, 0xC000_0000_0000_0000); // 1.5
        let (mut dp, mut sg) = (0, 0);
        let mut small = [0u8; 2];
        // SAFETY: a long double and pointers of this frame's.
        unsafe {
            crate::errno::set_errno(0);
            assert_eq!(
                __slate_ld_qecvt_r(
                    &raw const x,
                    5,
                    &raw mut dp,
                    &raw mut sg,
                    core::ptr::null_mut(),
                    9
                ),
                -1
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
            assert_eq!(
                __slate_ld_qfcvt_r(
                    &raw const x,
                    5,
                    &raw mut dp,
                    &raw mut sg,
                    small.as_mut_ptr(),
                    2
                ),
                -1
            );
            let mut buf = [0u8; 16];
            assert_eq!(
                __slate_ld_qecvt_r(
                    &raw const x,
                    3,
                    &raw mut dp,
                    &raw mut sg,
                    buf.as_mut_ptr(),
                    16
                ),
                0
            );
            assert_eq!(&buf[..4], b"150\0");
            assert_eq!((dp, sg), (1, 0));
        }
    }

    #[test]
    fn the_r_forms_refuse_a_null_or_small_buffer() {
        let (mut dp, mut sg) = (0, 0);
        crate::errno::set_errno(0);
        // SAFETY: a NULL buffer is checked; the others are local.
        unsafe {
            assert_eq!(
                fcvt_r(1.5, 2, &raw mut dp, &raw mut sg, core::ptr::null_mut(), 10),
                -1
            );
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
            let mut small = [0u8; 3];
            assert_eq!(
                ecvt_r(1.5, 5, &raw mut dp, &raw mut sg, small.as_mut_ptr(), 3),
                -1
            );
            let mut ok = [0u8; 8];
            assert_eq!(
                ecvt_r(1.5, 5, &raw mut dp, &raw mut sg, ok.as_mut_ptr(), 8),
                0
            );
            assert_eq!(&ok[..6], b"15000\0");
            assert_eq!((dp, sg), (1, 0));
        }
    }

    // -- Serialising the process-wide state these tests drive -------------
    //
    // `cargo test` runs these on separate threads, and `l64a`'s return buffer
    // is shared *by specification* -- POSIX gives a process one -- so it
    // cannot stop being shared the way a test-only counter can (which would
    // become a `thread_local!`; see `posix::malloc::live_allocations`). The
    // remaining option is to stop the tests overlapping. (The generators'
    // tests, which need the same, are crate::prng's.)
    //
    // The guard must be the FIRST statement of its test and stay bound for
    // the whole body: the indivisible unit is the entire "call it, read the
    // result back" sequence, not any single call inside it.
    //
    // Poison is recovered rather than propagated. A test that genuinely fails
    // while holding it should report once, not poison its siblings and bury
    // the cause under a wall of secondary panics.

    static L64A_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Serialises `L64A_BUF`.
    ///
    /// This one is not merely about a wrong assertion. `l64a` returns a pointer
    /// *into* the shared buffer, so the value is still live in it while the
    /// caller reads -- `test_a64l_l64a_roundtrip` holds it across a second
    /// call. An unserialised `l64a` on another thread rewrites the bytes the
    /// reader is midway through.
    #[must_use = "the guard serialises l64a's shared return buffer; bind it to `_g`"]
    fn lock_l64a_for_test() -> std::sync::MutexGuard<'static, ()> {
        L64A_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

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
                Some(cmp_i32),
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
                Some(cmp_i32),
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
                Some(cmp_i32),
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
                Some(cmp_i32),
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
                Some(cmp_i32),
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
                Some(cmp_i32),
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
                Some(cmp_i32),
            )
        };
        assert!(p.is_null());
    }

    // -- getsubopt tests --

    #[test]
    fn test_getsubopt_match() {
        // Tokens: "ro", "rw", "size"
        let tok0: *const u8 = b"ro\0".as_ptr();
        let tok1: *const u8 = b"rw\0".as_ptr();
        let tok2: *const u8 = b"size\0".as_ptr();
        let tokens: [*const u8; 4] = [tok0, tok1, tok2, core::ptr::null()];

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

    // -----------------------------------------------------------------------
    // strtol edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strtol_overflow_positive() {
        // Value larger than i64::MAX should clamp to LONG_MAX.
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtol(b"9999999999999999999999\0".as_ptr(), &mut endptr, 10) };
        assert_eq!(val, i64::MAX);
    }

    #[test]
    fn test_strtol_overflow_negative() {
        // Value more negative than i64::MIN should clamp to LONG_MIN.
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtol(b"-9999999999999999999999\0".as_ptr(), &mut endptr, 10) };
        assert_eq!(val, i64::MIN);
    }

    #[test]
    fn test_strtol_empty_no_digits() {
        // No valid digits: endptr should point to start, result should be 0.
        let input = b"   abc\0";
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtol(input.as_ptr(), &mut endptr, 10) };
        assert_eq!(val, 0);
    }

    #[test]
    fn test_strtol_base_2() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtol(b"1010\0".as_ptr(), &mut endptr, 2) };
        assert_eq!(val, 10, "binary 1010 = decimal 10");
    }

    #[test]
    fn test_strtol_base_36() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtol(b"z\0".as_ptr(), &mut endptr, 36) };
        assert_eq!(val, 35, "'z' in base 36 = 35");
    }

    #[test]
    fn test_strtol_long_min_exact() {
        // i64::MIN = -9223372036854775808
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtol(b"-9223372036854775808\0".as_ptr(), &mut endptr, 10) };
        assert_eq!(val, i64::MIN);
    }

    #[test]
    fn test_strtol_long_max_exact() {
        // i64::MAX = 9223372036854775807
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtol(b"9223372036854775807\0".as_ptr(), &mut endptr, 10) };
        assert_eq!(val, i64::MAX);
    }

    // -----------------------------------------------------------------------
    // strtoul edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strtoul_overflow() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtoul(b"99999999999999999999999\0".as_ptr(), &mut endptr, 10) };
        assert_eq!(val, u64::MAX);
    }

    #[test]
    fn test_strtoul_max_exact() {
        // u64::MAX = 18446744073709551615
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtoul(b"18446744073709551615\0".as_ptr(), &mut endptr, 10) };
        assert_eq!(val, u64::MAX);
    }

    #[test]
    fn test_strtoul_hex_uppercase() {
        let mut endptr: *const u8 = core::ptr::null();
        let val = unsafe { strtoul(b"0xDEAD\0".as_ptr(), &mut endptr, 0) };
        assert_eq!(val, 0xDEAD);
    }

    // -----------------------------------------------------------------------
    // qsort edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_qsort_empty_array() {
        // qsort on empty array should not crash.
        let mut arr: [i32; 0] = [];
        unsafe extern "C" fn cmp(a: *const u8, b: *const u8) -> i32 {
            unsafe { *(a as *const i32) - *(b as *const i32) }
        }
        unsafe { qsort(arr.as_mut_ptr().cast(), 0, 4, Some(cmp)) };
    }

    #[test]
    fn test_qsort_one_element() {
        let mut arr = [42i32];
        unsafe extern "C" fn cmp(a: *const u8, b: *const u8) -> i32 {
            unsafe { *(a as *const i32) - *(b as *const i32) }
        }
        unsafe { qsort(arr.as_mut_ptr().cast(), 1, 4, Some(cmp)) };
        assert_eq!(arr[0], 42);
    }

    #[test]
    fn test_qsort_presorted() {
        let mut arr = [1i32, 2, 3, 4, 5];
        unsafe extern "C" fn cmp(a: *const u8, b: *const u8) -> i32 {
            unsafe { *(a as *const i32) - *(b as *const i32) }
        }
        unsafe { qsort(arr.as_mut_ptr().cast(), 5, 4, Some(cmp)) };
        assert_eq!(arr, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_qsort_reverse_sorted() {
        let mut arr = [5i32, 4, 3, 2, 1];
        unsafe extern "C" fn cmp(a: *const u8, b: *const u8) -> i32 {
            unsafe { *(a as *const i32) - *(b as *const i32) }
        }
        unsafe { qsort(arr.as_mut_ptr().cast(), 5, 4, Some(cmp)) };
        assert_eq!(arr, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_qsort_duplicates() {
        let mut arr = [3i32, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        unsafe extern "C" fn cmp(a: *const u8, b: *const u8) -> i32 {
            unsafe { *(a as *const i32) - *(b as *const i32) }
        }
        unsafe { qsort(arr.as_mut_ptr().cast(), 11, 4, Some(cmp)) };
        assert_eq!(arr, [1, 1, 2, 3, 3, 4, 5, 5, 5, 6, 9]);
    }

    // -----------------------------------------------------------------------
    // qsort — the introsort engine
    //
    // Everything above this line is at most eleven elements long, so all of it
    // is handled by the insertion-sort leaf and none of it ever reaches a
    // partition.  These exercise the parts the old O(n²) implementation did
    // not have: median-of-three partitioning, the deferred-range stack, and
    // the heapsort fallback.
    // -----------------------------------------------------------------------

    /// Deterministic pseudo-random source (the SplitMix/PCG multiplier pair).
    ///
    /// A fixed sequence rather than a real RNG so a failure is reproducible
    /// from the seed alone — a sort bug that only shows up one run in fifty is
    /// worthless to debug.
    fn qsort_test_rand(state: &mut u64) -> u32 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*state >> 33) as u32
    }

    /// Total-order `int` comparator.
    ///
    /// Deliberately *not* the `cmp_i32` above, which returns `a - b`: that is
    /// the idiom every C tutorial uses and it reports the wrong sign whenever
    /// the difference overflows, which these tests hit constantly because they
    /// draw keys from the whole `i32` range.
    extern "C" fn qcmp_i32(a: *const u8, b: *const u8) -> i32 {
        // SAFETY: only ever passed to `qsort` over an `i32` array.
        let (x, y) = unsafe { (*a.cast::<i32>(), *b.cast::<i32>()) };
        match x.cmp(&y) {
            core::cmp::Ordering::Less => -1,
            core::cmp::Ordering::Equal => 0,
            core::cmp::Ordering::Greater => 1,
        }
    }

    /// Sort `arr` with our `qsort` and assert the result is both ordered *and*
    /// a permutation of the input.
    ///
    /// The permutation half matters specifically for a swap-based sort: a
    /// mis-indexed swap duplicates one element and loses another while leaving
    /// the array perfectly ordered, so an order-only check passes.
    fn check_qsort_i32(mut arr: Vec<i32>) {
        let mut expected = arr.clone();
        expected.sort_unstable();
        let n = arr.len();
        unsafe { qsort(arr.as_mut_ptr().cast(), n, 4, Some(qcmp_i32)) };
        assert_eq!(arr, expected, "n = {n}");
    }

    #[test]
    fn qsort_sorts_random_arrays_across_the_threshold() {
        let mut seed = 0x5EED_1234_u64;
        // Sizes chosen to straddle QSORT_INSERTION_MAX (12) in both directions
        // and to include lengths that are and are not powers of two.
        for n in [
            0usize, 1, 2, 3, 11, 12, 13, 14, 16, 17, 31, 32, 33, 63, 100, 127, 128, 129, 1000, 4096,
        ] {
            let arr: Vec<i32> = (0..n).map(|_| qsort_test_rand(&mut seed) as i32).collect();
            check_qsort_i32(arr);
        }
    }

    #[test]
    fn qsort_handles_all_elements_equal() {
        // The classic quicksort killer: a Lomuto partition peels off one
        // element per pass here and goes quadratic.  Sedgewick's scan stops on
        // equal keys, so this splits down the middle instead.
        check_qsort_i32(vec![7i32; 5000]);
    }

    #[test]
    fn qsort_handles_sorted_and_reversed_input() {
        // Both are worst cases for a first-element pivot; median-of-three
        // turns them into the best case.
        check_qsort_i32((0..2000i32).collect());
        check_qsort_i32((0..2000i32).rev().collect());
    }

    #[test]
    fn qsort_handles_adversarial_shapes() {
        // Organ pipe: ascends then descends.
        let organ: Vec<i32> = (0..1000i32).chain((0..1000i32).rev()).collect();
        check_qsort_i32(organ);
        // Sawtooth: many short runs, each a repeat of the last.
        let saw: Vec<i32> = (0..2000).map(|i| (i % 17) as i32).collect();
        check_qsort_i32(saw);
        // Two values only — maximal ties without being constant.
        let binary: Vec<i32> = (0..3000).map(|i| i32::from(i % 2 == 0)).collect();
        check_qsort_i32(binary);
    }

    #[test]
    fn qsort_handles_elements_larger_than_the_old_stack_buffer() {
        // The previous implementation held one element in a 256-byte stack
        // buffer and `mmap`'d for anything bigger — and silently gave up if
        // that `mmap` failed.  This engine swaps in place and has no such
        // threshold, so a 512-byte element must behave exactly like a 4-byte
        // one.  The element carries its key in the first four bytes and a
        // checksum-ish tail, so a partial swap is detectable.
        const W: usize = 512;
        let mut seed = 0xC0FF_EE01_u64;
        let n = 300usize;
        let mut buf = vec![0u8; n * W];
        let mut keys = Vec::with_capacity(n);
        for i in 0..n {
            let key = qsort_test_rand(&mut seed) as i32;
            keys.push(key);
            let elem = &mut buf[i * W..(i + 1) * W];
            elem[..4].copy_from_slice(&key.to_ne_bytes());
            // Fill the tail deterministically from the key.
            for (j, b) in elem[4..].iter_mut().enumerate() {
                *b = (key as u8).wrapping_add(j as u8);
            }
        }
        unsafe { qsort(buf.as_mut_ptr(), n, W, Some(qcmp_i32)) };

        keys.sort_unstable();
        for (i, want) in keys.iter().enumerate() {
            let elem = &buf[i * W..(i + 1) * W];
            let got = i32::from_ne_bytes([elem[0], elem[1], elem[2], elem[3]]);
            assert_eq!(got, *want, "key at index {i}");
            // The tail must still belong to this key — i.e. the whole element
            // moved, not just its first word.
            for (j, b) in elem[4..].iter().enumerate() {
                assert_eq!(
                    *b,
                    (got as u8).wrapping_add(j as u8),
                    "tail byte {j} at {i}"
                );
            }
        }
    }

    #[test]
    fn qsort_handles_element_sizes_that_are_not_a_multiple_of_a_word() {
        // Three-byte elements: any swap written in terms of `u64` chunks
        // rather than bytes would corrupt neighbours here.
        const W: usize = 3;
        let n = 500usize;
        let mut buf = vec![0u8; n * W];
        let mut seed = 0xABCD_0F0F_u64;
        for i in 0..n {
            let v = (qsort_test_rand(&mut seed) & 0x00FF_FFFF) as u32;
            buf[i * W] = (v >> 16) as u8;
            buf[i * W + 1] = (v >> 8) as u8;
            buf[i * W + 2] = v as u8;
        }
        let mut expected: Vec<[u8; 3]> = (0..n)
            .map(|i| [buf[i * W], buf[i * W + 1], buf[i * W + 2]])
            .collect();
        expected.sort_unstable();

        unsafe extern "C" fn cmp_be3(a: *const u8, b: *const u8) -> i32 {
            for k in 0..3 {
                // SAFETY: the comparator is only ever called with pointers to
                // 3-byte elements of the array under test.
                let (x, y) = unsafe { (*a.add(k), *b.add(k)) };
                if x != y {
                    return if x < y { -1 } else { 1 };
                }
            }
            0
        }
        unsafe { qsort(buf.as_mut_ptr(), n, W, Some(cmp_be3)) };

        for (i, want) in expected.iter().enumerate() {
            assert_eq!(&buf[i * W..i * W + 3], &want[..], "element {i}");
        }
    }

    #[test]
    fn qsort_heapsort_fallback_sorts_correctly() {
        // The depth-limited fallback is unreachable from `qsort` on any input
        // we can construct here (it needs 2·log₂n consecutive bad pivots), so
        // exercise it directly.  If it were wrong, the only symptom through
        // the public entry point would be a rare mis-sort on adversarial data.
        let mut seed = 0x1357_9BDF_u64;
        for n in [2usize, 3, 8, 17, 64, 1000] {
            let mut arr: Vec<i32> = (0..n).map(|_| qsort_test_rand(&mut seed) as i32).collect();
            let mut expected = arr.clone();
            expected.sort_unstable();
            unsafe {
                qsort_heap(arr.as_mut_ptr().cast(), 4, 0, n - 1, &|a, b| qcmp_i32(a, b));
            }
            assert_eq!(arr, expected, "heapsort, n = {n}");
        }
    }

    #[test]
    fn qsort_heapsort_fallback_sorts_a_subrange_only() {
        // `qsort_heap` is handed an inclusive sub-range by the engine, so it
        // must leave everything outside it untouched.
        let mut arr: Vec<i32> = vec![100, 99, 5, 4, 3, 2, 1, 98, 97];
        unsafe {
            qsort_heap(arr.as_mut_ptr().cast(), 4, 2, 6, &|a, b| qcmp_i32(a, b));
        }
        assert_eq!(arr, vec![100, 99, 1, 2, 3, 4, 5, 98, 97]);
    }

    #[test]
    fn qsort_r_sorts_and_passes_its_context_through() {
        // `arg` selects the sort direction, which is both the point of
        // `qsort_r` and a check that the pointer survives every comparison.
        unsafe extern "C" fn cmp_dir(
            a: *const u8,
            b: *const u8,
            arg: *mut core::ffi::c_void,
        ) -> i32 {
            // SAFETY: the caller below passes a live `&mut i32`.
            let dir = unsafe { *arg.cast::<i32>() };
            // SAFETY: elements of the `i32` array under test.
            qcmp_i32(a, b) * dir
        }

        let mut seed = 0x2468_ACE0_u64;
        let mut arr: Vec<i32> = (0..500)
            .map(|_| qsort_test_rand(&mut seed) as i32)
            .collect();
        let mut ascending = arr.clone();
        ascending.sort_unstable();
        let mut descending = ascending.clone();
        descending.reverse();

        let n = arr.len();
        let mut dir: i32 = 1;
        unsafe {
            qsort_r(
                arr.as_mut_ptr().cast(),
                n,
                4,
                Some(cmp_dir),
                (&raw mut dir).cast::<core::ffi::c_void>(),
            );
        }
        assert_eq!(arr, ascending);

        dir = -1;
        unsafe {
            qsort_r(
                arr.as_mut_ptr().cast(),
                n,
                4,
                Some(cmp_dir),
                (&raw mut dir).cast::<core::ffi::c_void>(),
            );
        }
        assert_eq!(arr, descending);
    }

    #[test]
    fn qsort_rejects_degenerate_arguments_without_touching_memory() {
        unsafe extern "C" fn cmp_never(_a: *const u8, _b: *const u8) -> i32 {
            panic!("comparator must not be called");
        }
        let mut arr = [3i32, 1, 2];
        // size == 0: nothing can be swapped meaningfully.
        unsafe { qsort(arr.as_mut_ptr().cast(), 3, 0, Some(cmp_never)) };
        assert_eq!(arr, [3, 1, 2]);
        // A NULL base with elements to move is where glibc's process faults,
        // and it ends this one too (`qsort_comparator`) -- not a test's to
        // take.  It returned without sorting until 2026-09-26.
    }

    // -----------------------------------------------------------------------
    // strtod_l / strtold_l
    // -----------------------------------------------------------------------

    #[test]
    fn strtod_l_matches_strtod_and_ignores_the_locale() {
        for s in [
            &b"3.14159\0"[..],
            &b"-0.5e3\0"[..],
            &b"0x1.8p+1\0"[..],
            &b"  42\0"[..],
            &b"nonsense\0"[..],
        ] {
            let mut end_plain: *const u8 = core::ptr::null();
            let mut end_l: *const u8 = core::ptr::null();
            let plain = unsafe { strtod(s.as_ptr(), &mut end_plain) };
            let with_locale =
                unsafe { strtod_l(s.as_ptr(), &mut end_l, crate::locale::LC_GLOBAL_LOCALE) };
            assert!(
                (plain - with_locale).abs() < f64::EPSILON
                    || (plain.is_nan() && with_locale.is_nan()),
                "value mismatch for {s:?}"
            );
            // The end pointer must advance identically, not merely the value.
            assert_eq!(
                end_plain as usize - s.as_ptr() as usize,
                end_l as usize - s.as_ptr() as usize,
                "endptr mismatch for {s:?}"
            );
        }
    }

    #[test]
    fn strtold_l_matches_strtold() {
        let s = b"2.718281828\0";
        let mut e1: *const u8 = core::ptr::null();
        let mut e2: *const u8 = core::ptr::null();
        let a = unsafe { strtold(s.as_ptr(), &mut e1) };
        let b = unsafe { strtold_l(s.as_ptr(), &mut e2, crate::locale::LC_GLOBAL_LOCALE) };
        assert_eq!((a.sign_exp, a.significand), (b.sign_exp, b.significand));
        assert_eq!(e1, e2);
    }

    // -----------------------------------------------------------------------
    // bsearch edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_bsearch_finds_element() {
        let arr = [1i32, 3, 5, 7, 9, 11];
        let key: i32 = 7;
        unsafe extern "C" fn cmp(a: *const u8, b: *const u8) -> i32 {
            unsafe { *(a as *const i32) - *(b as *const i32) }
        }
        let result = unsafe {
            bsearch(
                (&key as *const i32).cast(),
                arr.as_ptr().cast(),
                6,
                4,
                Some(cmp),
            )
        };
        assert!(!result.is_null());
        assert_eq!(unsafe { *(result as *const i32) }, 7);
    }

    #[test]
    fn test_bsearch_missing_element() {
        let arr = [1i32, 3, 5, 7, 9, 11];
        let key: i32 = 4;
        unsafe extern "C" fn cmp(a: *const u8, b: *const u8) -> i32 {
            unsafe { *(a as *const i32) - *(b as *const i32) }
        }
        let result = unsafe {
            bsearch(
                (&key as *const i32).cast(),
                arr.as_ptr().cast(),
                6,
                4,
                Some(cmp),
            )
        };
        assert!(result.is_null());
    }

    #[test]
    fn test_bsearch_empty() {
        let key: i32 = 42;
        unsafe extern "C" fn cmp(a: *const u8, b: *const u8) -> i32 {
            unsafe { *(a as *const i32) - *(b as *const i32) }
        }
        let result = unsafe {
            bsearch(
                (&key as *const i32).cast(),
                core::ptr::null(),
                0,
                4,
                Some(cmp),
            )
        };
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // abs / labs / llabs
    // -----------------------------------------------------------------------

    #[test]
    fn test_abs_values() {
        assert_eq!(abs(-5), 5);
        assert_eq!(abs(0), 0);
        assert_eq!(abs(5), 5);
    }

    #[test]
    fn test_labs_values() {
        assert_eq!(labs(-100_000), 100_000);
        assert_eq!(labs(0), 0);
    }

    #[test]
    fn test_llabs_values() {
        assert_eq!(llabs(-1_000_000_000_000), 1_000_000_000_000);
        assert_eq!(llabs(0), 0);
    }

    // -----------------------------------------------------------------------
    // div / ldiv / lldiv
    // -----------------------------------------------------------------------

    #[test]
    fn test_div_positive_operands() {
        let result = div(10, 3);
        assert_eq!(result.quot, 3);
        assert_eq!(result.rem, 1);
    }

    #[test]
    fn test_div_negative_operand() {
        let result = div(-10, 3);
        assert_eq!(result.quot, -3);
        assert_eq!(result.rem, -1);
    }

    #[test]
    fn test_lldiv_large_value() {
        let result = lldiv(i64::MAX, 2);
        assert_eq!(result.quot, i64::MAX / 2);
        assert_eq!(result.rem, 1);
    }

    #[test]
    fn test_div_i32_min_by_minus_one() {
        // Division overflow: i32::MIN / -1 can't fit in i32.
        // Must not panic — returns wrapping result.
        let result = div(i32::MIN, -1);
        assert_eq!(result.quot, i32::MIN);
        assert_eq!(result.rem, 0);
    }

    #[test]
    fn test_ldiv_i64_min_by_minus_one() {
        let result = ldiv(i64::MIN, -1);
        assert_eq!(result.quot, i64::MIN);
        assert_eq!(result.rem, 0);
    }

    #[test]
    fn test_div_zero_denom() {
        let result = div(42, 0);
        assert_eq!(result.quot, 0);
        assert_eq!(result.rem, 0);
    }

    // -----------------------------------------------------------------------
    // strtod edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strtod_endptr() {
        let mut end: *const u8 = core::ptr::null();
        let v = unsafe { strtod(b"3.14\0".as_ptr(), &mut end) };
        assert!((v - 3.14).abs() < 1e-10);
        // endptr should point to the null terminator.
        assert_eq!(unsafe { *end }, 0);
    }

    #[test]
    fn test_strtod_neg_sign() {
        let v = unsafe { strtod(b"-2.5\0".as_ptr(), core::ptr::null_mut()) };
        assert!((v - (-2.5)).abs() < 1e-10);
    }

    #[test]
    fn test_strtod_sci_notation() {
        let v = unsafe { strtod(b"1.5e3\0".as_ptr(), core::ptr::null_mut()) };
        assert!((v - 1500.0).abs() < 1e-10);
    }

    #[test]
    fn test_strtod_negative_exponent() {
        let v = unsafe { strtod(b"5e-2\0".as_ptr(), core::ptr::null_mut()) };
        assert!((v - 0.05).abs() < 1e-10);
    }

    #[test]
    fn test_strtod_inf() {
        let v = unsafe { strtod(b"inf\0".as_ptr(), core::ptr::null_mut()) };
        assert!(v.is_infinite() && v > 0.0);
    }

    #[test]
    fn test_strtod_neg_inf() {
        let v = unsafe { strtod(b"-INFINITY\0".as_ptr(), core::ptr::null_mut()) };
        assert!(v.is_infinite() && v < 0.0);
    }

    #[test]
    fn test_strtod_nan() {
        let v = unsafe { strtod(b"nan\0".as_ptr(), core::ptr::null_mut()) };
        assert!(v.is_nan());
    }

    /// glibc 2.39's `strtod`, `strtof`, `wcstod` and `wcstof` on NaN
    /// spellings (`posix/tools/oracle/strtod_nan_harness.py`): the bits -- sign and
    /// payload -- how much was consumed, and `errno`. Until 2026-09-27 every
    /// NaN came back positive and without its payload, and any bytes up to a
    /// `)` were taken for an n-char-sequence.
    // Generated by posix/tools/oracle/strtod_nan_harness.py from glibc 2.39 under WSL:
    // (function, input, result bits, characters consumed, errno).
    const GLIBC_NAN: &[(&str, &str, u64, usize, i32)] = &[
        ("strtod", "nan", 0x7ff8000000000000, 3, 0),
        ("strtof", "nan", 0x7fc00000, 3, 0),
        ("wcstod", "nan", 0x7ff8000000000000, 3, 0),
        ("wcstof", "nan", 0x7fc00000, 3, 0),
        ("strtod", "-nan", 0xfff8000000000000, 4, 0),
        ("strtof", "-nan", 0xffc00000, 4, 0),
        ("wcstod", "-nan", 0xfff8000000000000, 4, 0),
        ("wcstof", "-nan", 0xffc00000, 4, 0),
        ("strtod", "+nan", 0x7ff8000000000000, 4, 0),
        ("strtof", "+nan", 0x7fc00000, 4, 0),
        ("wcstod", "+nan", 0x7ff8000000000000, 4, 0),
        ("wcstof", "+nan", 0x7fc00000, 4, 0),
        ("strtod", "NAN", 0x7ff8000000000000, 3, 0),
        ("strtof", "NAN", 0x7fc00000, 3, 0),
        ("wcstod", "NAN", 0x7ff8000000000000, 3, 0),
        ("wcstof", "NAN", 0x7fc00000, 3, 0),
        ("strtod", "NaN(1234)", 0x7ff80000000004d2, 9, 0),
        ("strtof", "NaN(1234)", 0x7fc004d2, 9, 0),
        ("wcstod", "NaN(1234)", 0x7ff80000000004d2, 9, 0),
        ("wcstof", "NaN(1234)", 0x7fc004d2, 9, 0),
        ("strtod", "nan(0x12)", 0x7ff8000000000012, 9, 0),
        ("strtof", "nan(0x12)", 0x7fc00012, 9, 0),
        ("wcstod", "nan(0x12)", 0x7ff8000000000012, 9, 0),
        ("wcstof", "nan(0x12)", 0x7fc00012, 9, 0),
        ("strtod", "nan(017)", 0x7ff800000000000f, 8, 0),
        ("strtof", "nan(017)", 0x7fc0000f, 8, 0),
        ("wcstod", "nan(017)", 0x7ff800000000000f, 8, 0),
        ("wcstof", "nan(017)", 0x7fc0000f, 8, 0),
        ("strtod", "nan(abc)", 0x7ff8000000000000, 8, 0),
        ("strtof", "nan(abc)", 0x7fc00000, 8, 0),
        ("wcstod", "nan(abc)", 0x7ff8000000000000, 8, 0),
        ("wcstof", "nan(abc)", 0x7fc00000, 8, 0),
        ("strtod", "nan(1 2)", 0x7ff8000000000000, 3, 0),
        ("strtof", "nan(1 2)", 0x7fc00000, 3, 0),
        ("wcstod", "nan(1 2)", 0x7ff8000000000000, 3, 0),
        ("wcstof", "nan(1 2)", 0x7fc00000, 3, 0),
        ("strtod", "nan(", 0x7ff8000000000000, 3, 0),
        ("strtof", "nan(", 0x7fc00000, 3, 0),
        ("wcstod", "nan(", 0x7ff8000000000000, 3, 0),
        ("wcstof", "nan(", 0x7fc00000, 3, 0),
        ("strtod", "nan()", 0x7ff8000000000000, 5, 0),
        ("strtof", "nan()", 0x7fc00000, 5, 0),
        ("wcstod", "nan()", 0x7ff8000000000000, 5, 0),
        ("wcstof", "nan()", 0x7fc00000, 5, 0),
        ("strtod", "-nan(0x8000000000000)", 0xfff8000000000000, 21, 0),
        ("strtof", "-nan(0x8000000000000)", 0xffc00000, 21, 0),
        ("wcstod", "-nan(0x8000000000000)", 0xfff8000000000000, 21, 0),
        ("wcstof", "-nan(0x8000000000000)", 0xffc00000, 21, 0),
        (
            "strtod",
            "nan(99999999999999999999999)",
            0x7fffffffffffffff,
            28,
            34,
        ),
        ("strtof", "nan(99999999999999999999999)", 0x7fffffff, 28, 34),
        (
            "wcstod",
            "nan(99999999999999999999999)",
            0x7fffffffffffffff,
            28,
            34,
        ),
        ("wcstof", "nan(99999999999999999999999)", 0x7fffffff, 28, 34),
        ("strtod", "nan(0x7ffffffffffff)", 0x7fffffffffffffff, 20, 0),
        ("strtof", "nan(0x7ffffffffffff)", 0x7fffffff, 20, 0),
        ("wcstod", "nan(0x7ffffffffffff)", 0x7fffffffffffffff, 20, 0),
        ("wcstof", "nan(0x7ffffffffffff)", 0x7fffffff, 20, 0),
        ("strtod", "  +nan(5)x", 0x7ff8000000000005, 9, 0),
        ("strtof", "  +nan(5)x", 0x7fc00005, 9, 0),
        ("wcstod", "  +nan(5)x", 0x7ff8000000000005, 9, 0),
        ("wcstof", "  +nan(5)x", 0x7fc00005, 9, 0),
        ("strtod", "nanx", 0x7ff8000000000000, 3, 0),
        ("strtof", "nanx", 0x7fc00000, 3, 0),
        ("wcstod", "nanx", 0x7ff8000000000000, 3, 0),
        ("wcstof", "nanx", 0x7fc00000, 3, 0),
        ("strtod", "nan(_)", 0x7ff8000000000000, 6, 0),
        ("strtof", "nan(_)", 0x7fc00000, 6, 0),
        ("wcstod", "nan(_)", 0x7ff8000000000000, 6, 0),
        ("wcstof", "nan(_)", 0x7fc00000, 6, 0),
        ("strtod", "nan(1_2)", 0x7ff8000000000000, 8, 0),
        ("strtof", "nan(1_2)", 0x7fc00000, 8, 0),
        ("wcstod", "nan(1_2)", 0x7ff8000000000000, 8, 0),
        ("wcstof", "nan(1_2)", 0x7fc00000, 8, 0),
        ("strtod", "-nan(42)", 0xfff800000000002a, 8, 0),
        ("strtof", "-nan(42)", 0xffc0002a, 8, 0),
        ("wcstod", "-nan(42)", 0xfff800000000002a, 8, 0),
        ("wcstof", "-nan(42)", 0xffc0002a, 8, 0),
        ("strtod", "nan(0x)", 0x7ff8000000000000, 7, 0),
        ("strtof", "nan(0x)", 0x7fc00000, 7, 0),
        ("wcstod", "nan(0x)", 0x7ff8000000000000, 7, 0),
        ("wcstof", "nan(0x)", 0x7fc00000, 7, 0),
        ("strtod", "nan(08)", 0x7ff8000000000000, 7, 0),
        ("strtof", "nan(08)", 0x7fc00000, 7, 0),
        ("wcstod", "nan(08)", 0x7ff8000000000000, 7, 0),
        ("wcstof", "nan(08)", 0x7fc00000, 7, 0),
        ("strtod", "nan(0x1g)", 0x7ff8000000000000, 9, 0),
        ("strtof", "nan(0x1g)", 0x7fc00000, 9, 0),
        ("wcstod", "nan(0x1g)", 0x7ff8000000000000, 9, 0),
        ("wcstof", "nan(0x1g)", 0x7fc00000, 9, 0),
        ("strtod", "nan(0X1F)", 0x7ff800000000001f, 9, 0),
        ("strtof", "nan(0X1F)", 0x7fc0001f, 9, 0),
        ("wcstod", "nan(0X1F)", 0x7ff800000000001f, 9, 0),
        ("wcstof", "nan(0X1F)", 0x7fc0001f, 9, 0),
        ("strtod", "nan(0x400000)", 0x7ff8000000400000, 13, 0),
        ("strtof", "nan(0x400000)", 0x7fc00000, 13, 0),
        ("wcstod", "nan(0x400000)", 0x7ff8000000400000, 13, 0),
        ("wcstof", "nan(0x400000)", 0x7fc00000, 13, 0),
        ("strtod", "nan(4194303)", 0x7ff80000003fffff, 12, 0),
        ("strtof", "nan(4194303)", 0x7fffffff, 12, 0),
        ("wcstod", "nan(4194303)", 0x7ff80000003fffff, 12, 0),
        ("wcstof", "nan(4194303)", 0x7fffffff, 12, 0),
        ("strtod", "nan(4194304)", 0x7ff8000000400000, 12, 0),
        ("strtof", "nan(4194304)", 0x7fc00000, 12, 0),
        ("wcstod", "nan(4194304)", 0x7ff8000000400000, 12, 0),
        ("wcstof", "nan(4194304)", 0x7fc00000, 12, 0),
        ("strtod", "nan(-1)", 0x7ff8000000000000, 3, 0),
        ("strtof", "nan(-1)", 0x7fc00000, 3, 0),
        ("wcstod", "nan(-1)", 0x7ff8000000000000, 3, 0),
        ("wcstof", "nan(-1)", 0x7fc00000, 3, 0),
    ];

    #[test]
    fn nan_spellings_are_read_as_glibc_reads_them() {
        let mut failures = std::vec::Vec::new();
        for &(func, input, bits, consumed, want_errno) in GLIBC_NAN {
            let mut c = input.as_bytes().to_vec();
            c.push(0);
            let wide: std::vec::Vec<crate::wchar::WcharT> =
                c.iter().map(|&b| crate::wchar::WcharT::from(b)).collect();
            crate::errno::set_errno(0);
            let (got_bits, got_consumed) = match func {
                "strtod" | "strtof" => {
                    let mut end: *const u8 = core::ptr::null();
                    // SAFETY: `c` is NUL-terminated and outlives the call.
                    let b = unsafe {
                        if func == "strtod" {
                            strtod(c.as_ptr(), &raw mut end).to_bits()
                        } else {
                            u64::from(strtof(c.as_ptr(), &raw mut end).to_bits())
                        }
                    };
                    (b, end as usize - c.as_ptr() as usize)
                }
                _ => {
                    let mut end: *const crate::wchar::WcharT = core::ptr::null();
                    // SAFETY: `wide` is NUL-terminated and outlives the call.
                    let b = unsafe {
                        if func == "wcstod" {
                            crate::wchar::wcstod(wide.as_ptr(), &raw mut end).to_bits()
                        } else {
                            u64::from(crate::wchar::wcstof(wide.as_ptr(), &raw mut end).to_bits())
                        }
                    };
                    let width = core::mem::size_of::<crate::wchar::WcharT>();
                    (b, (end as usize - wide.as_ptr() as usize) / width)
                }
            };
            let got_errno = crate::errno::get_errno();
            if (got_bits, got_consumed, got_errno) != (bits, consumed, want_errno) {
                failures.push(format!(
                    "{func}({input:?}): {got_bits:#x} after {got_consumed}, errno {got_errno}; glibc {bits:#x} after {consumed}, errno {want_errno}"
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn test_strtod_nan_payload() {
        let mut end: *const u8 = core::ptr::null();
        let v = unsafe { strtod(b"NAN(1234)x\0".as_ptr(), &mut end) };
        assert!(v.is_nan());
        // endptr should point past "NAN(1234)".
        assert_eq!(unsafe { *end }, b'x');
    }

    #[test]
    fn test_strtod_dot_only_no_conversion() {
        // Just a dot with no digits — no conversion per POSIX.
        let mut end: *const u8 = core::ptr::null();
        let v = unsafe { strtod(b".\0".as_ptr(), &mut end) };
        assert_eq!(v, 0.0);
        assert_eq!(end, b".\0".as_ptr()); // endptr = nptr
    }

    #[test]
    fn test_strtod_leading_dot() {
        let v = unsafe { strtod(b".5\0".as_ptr(), core::ptr::null_mut()) };
        assert!((v - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_strtod_trailing_dot() {
        let mut end: *const u8 = core::ptr::null();
        let v = unsafe { strtod(b"5.x\0".as_ptr(), &mut end) };
        assert!((v - 5.0).abs() < 1e-10);
        assert_eq!(unsafe { *end }, b'x');
    }

    #[test]
    fn test_strtod_e_without_digits_not_consumed() {
        // "1e" — 'e' without exponent digits should not be consumed.
        let mut end: *const u8 = core::ptr::null();
        let v = unsafe { strtod(b"1e\0".as_ptr(), &mut end) };
        assert!((v - 1.0).abs() < 1e-10);
        assert_eq!(unsafe { *end }, b'e');
    }

    #[test]
    fn test_strtod_e_sign_without_digits_not_consumed() {
        // "1e+" — 'e+' without digits should not be consumed.
        let mut end: *const u8 = core::ptr::null();
        let v = unsafe { strtod(b"1e+\0".as_ptr(), &mut end) };
        assert!((v - 1.0).abs() < 1e-10);
        assert_eq!(unsafe { *end }, b'e');
    }

    #[test]
    fn test_strtod_overflow_sets_erange() {
        crate::errno::set_errno(0);
        let v = unsafe { strtod(b"1e999\0".as_ptr(), core::ptr::null_mut()) };
        assert!(v.is_infinite());
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);
    }

    #[test]
    fn test_strtod_whitespace_tabs() {
        let v = unsafe { strtod(b"  \t42\0".as_ptr(), core::ptr::null_mut()) };
        assert!((v - 42.0).abs() < 1e-10);
    }

    #[test]
    fn test_strtod_empty_string() {
        let mut end: *const u8 = core::ptr::null();
        let input = b"\0";
        let v = unsafe { strtod(input.as_ptr(), &mut end) };
        assert_eq!(v, 0.0);
        assert_eq!(end, input.as_ptr());
    }

    #[test]
    fn test_strtof_basic() {
        let v = unsafe { strtof(b"3.14\0".as_ptr(), core::ptr::null_mut()) };
        assert!((v - 3.14f32).abs() < 1e-5);
    }

    // -----------------------------------------------------------------------
    // strtol edge cases: i64::MIN, hex prefix backtracking, ERANGE
    // -----------------------------------------------------------------------

    #[test]
    fn test_strtol_i64_min() {
        crate::errno::set_errno(0);
        let v = unsafe {
            strtol(
                b"-9223372036854775808\0".as_ptr(),
                core::ptr::null_mut(),
                10,
            )
        };
        assert_eq!(v, i64::MIN);
        assert_eq!(crate::errno::get_errno(), 0); // NOT overflow
    }

    #[test]
    fn test_strtol_i64_max() {
        crate::errno::set_errno(0);
        let v = unsafe { strtol(b"9223372036854775807\0".as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, i64::MAX);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    #[test]
    fn test_strtol_positive_overflow() {
        crate::errno::set_errno(0);
        let v = unsafe { strtol(b"9223372036854775808\0".as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, i64::MAX);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);
    }

    #[test]
    fn test_strtol_negative_overflow() {
        crate::errno::set_errno(0);
        let v = unsafe {
            strtol(
                b"-9223372036854775809\0".as_ptr(),
                core::ptr::null_mut(),
                10,
            )
        };
        assert_eq!(v, i64::MIN);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);
    }

    #[test]
    fn test_strtol_hex_0x_backtrack() {
        // "0xG" — invalid hex digit after 0x, backtrack and parse "0".
        let mut end: *const u8 = core::ptr::null();
        let v = unsafe { strtol(b"0xG\0".as_ptr(), &mut end, 0) };
        assert_eq!(v, 0);
        // endptr should point past "0" but before "x".
        let offset = unsafe { end.offset_from(b"0xG\0".as_ptr()) };
        assert_eq!(offset, 1);
    }

    #[test]
    fn test_strtoul_negative_wraps() {
        // POSIX: strtoul("-1") wraps to ULONG_MAX.
        crate::errno::set_errno(0);
        let v = unsafe { strtoul(b"-1\0".as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, u64::MAX);
        assert_eq!(crate::errno::get_errno(), 0); // NOT an error
    }

    #[test]
    fn test_strtoul_overflow_erange() {
        crate::errno::set_errno(0);
        let v = unsafe {
            strtoul(
                b"18446744073709551616\0".as_ptr(),
                core::ptr::null_mut(),
                10,
            )
        };
        assert_eq!(v, u64::MAX);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);
    }

    // -----------------------------------------------------------------------
    // getsubopt — comprehensive tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_getsubopt_no_value() {
        // "ro" matches token 0, no value.
        let tok0: *const u8 = b"ro\0".as_ptr();
        let tok1: *const u8 = b"rw\0".as_ptr();
        let tokens: [*const u8; 3] = [tok0, tok1, core::ptr::null()];

        let mut input = *b"ro\0";
        let mut optionp: *mut u8 = input.as_mut_ptr();
        let mut valuep: *mut u8 = core::ptr::null_mut();

        let idx = unsafe {
            getsubopt(
                &mut optionp,
                tokens.as_ptr().cast::<*const u8>(),
                &mut valuep,
            )
        };
        assert_eq!(idx, 0);
        assert!(valuep.is_null(), "no value expected");
    }

    #[test]
    fn test_getsubopt_with_value() {
        // "size=512" matches "size" at index 2 with value "512".
        let tok0: *const u8 = b"ro\0".as_ptr();
        let tok1: *const u8 = b"rw\0".as_ptr();
        let tok2: *const u8 = b"size\0".as_ptr();
        let tokens: [*const u8; 4] = [tok0, tok1, tok2, core::ptr::null()];

        let mut input = *b"size=512\0";
        let mut optionp: *mut u8 = input.as_mut_ptr();
        let mut valuep: *mut u8 = core::ptr::null_mut();

        let idx = unsafe {
            getsubopt(
                &mut optionp,
                tokens.as_ptr().cast::<*const u8>(),
                &mut valuep,
            )
        };
        assert_eq!(idx, 2);
        assert!(!valuep.is_null());
        // valuep should point to "512".
        assert_eq!(unsafe { *valuep }, b'5');
        assert_eq!(unsafe { *valuep.add(1) }, b'1');
        assert_eq!(unsafe { *valuep.add(2) }, b'2');
    }

    #[test]
    fn test_getsubopt_unrecognized() {
        // "unknown" doesn't match any token → returns -1.
        let tok0: *const u8 = b"ro\0".as_ptr();
        let tokens: [*const u8; 2] = [tok0, core::ptr::null()];

        let mut input = *b"unknown\0";
        let mut optionp: *mut u8 = input.as_mut_ptr();
        let mut valuep: *mut u8 = core::ptr::null_mut();

        let idx = unsafe {
            getsubopt(
                &mut optionp,
                tokens.as_ptr().cast::<*const u8>(),
                &mut valuep,
            )
        };
        assert_eq!(idx, -1);
        assert!(valuep.is_null()); // No '=' → no value.
    }

    #[test]
    fn test_getsubopt_unrecognized_with_value() {
        // "bad=123" doesn't match → returns -1, but valuep points to value.
        let tok0: *const u8 = b"good\0".as_ptr();
        let tokens: [*const u8; 2] = [tok0, core::ptr::null()];

        let mut input = *b"bad=123\0";
        let mut optionp: *mut u8 = input.as_mut_ptr();
        let mut valuep: *mut u8 = core::ptr::null_mut();

        let idx = unsafe {
            getsubopt(
                &mut optionp,
                tokens.as_ptr().cast::<*const u8>(),
                &mut valuep,
            )
        };
        assert_eq!(idx, -1);
        assert!(!valuep.is_null()); // '=' present → value set.
        assert_eq!(unsafe { *valuep }, b'1');
    }

    #[test]
    fn test_getsubopt_multiple_suboptions() {
        // "a,b,c" should parse all three sequentially.
        let tok_a: *const u8 = b"a\0".as_ptr();
        let tok_b: *const u8 = b"b\0".as_ptr();
        let tok_c: *const u8 = b"c\0".as_ptr();
        let tokens: [*const u8; 4] = [tok_a, tok_b, tok_c, core::ptr::null()];

        let mut input = *b"a,b,c\0";
        let mut optionp: *mut u8 = input.as_mut_ptr();
        let mut valuep: *mut u8 = core::ptr::null_mut();

        let idx1 = unsafe {
            getsubopt(
                &mut optionp,
                tokens.as_ptr().cast::<*const u8>(),
                &mut valuep,
            )
        };
        assert_eq!(idx1, 0); // "a"

        let idx2 = unsafe {
            getsubopt(
                &mut optionp,
                tokens.as_ptr().cast::<*const u8>(),
                &mut valuep,
            )
        };
        assert_eq!(idx2, 1); // "b"

        let idx3 = unsafe {
            getsubopt(
                &mut optionp,
                tokens.as_ptr().cast::<*const u8>(),
                &mut valuep,
            )
        };
        assert_eq!(idx3, 2); // "c"
    }

    #[test]
    fn test_getsubopt_empty_value() {
        // "key=" has an empty value.
        let tok0: *const u8 = b"key\0".as_ptr();
        let tokens: [*const u8; 2] = [tok0, core::ptr::null()];

        let mut input = *b"key=\0";
        let mut optionp: *mut u8 = input.as_mut_ptr();
        let mut valuep: *mut u8 = core::ptr::null_mut();

        let idx = unsafe {
            getsubopt(
                &mut optionp,
                tokens.as_ptr().cast::<*const u8>(),
                &mut valuep,
            )
        };
        assert_eq!(idx, 0);
        assert!(!valuep.is_null());
        // Value should be empty (pointing to the null terminator).
        assert_eq!(unsafe { *valuep }, 0);
    }

    #[test]
    fn test_getsubopt_null_optionp() {
        let tok0: *const u8 = b"a\0".as_ptr();
        let tokens: [*const u8; 2] = [tok0, core::ptr::null()];
        let mut valuep: *mut u8 = core::ptr::null_mut();

        let idx = unsafe {
            getsubopt(
                core::ptr::null_mut(),
                tokens.as_ptr().cast::<*const u8>(),
                &mut valuep,
            )
        };
        assert_eq!(idx, -1);
    }

    #[test]
    fn test_getsubopt_null_tokens() {
        let mut input = *b"test\0";
        let mut optionp: *mut u8 = input.as_mut_ptr();
        let mut valuep: *mut u8 = core::ptr::null_mut();

        let idx = unsafe { getsubopt(&mut optionp, core::ptr::null(), &mut valuep) };
        assert_eq!(idx, -1);
    }

    #[test]
    fn test_getsubopt_null_valuep() {
        let tok0: *const u8 = b"a\0".as_ptr();
        let tokens: [*const u8; 2] = [tok0, core::ptr::null()];
        let mut input = *b"a\0";
        let mut optionp: *mut u8 = input.as_mut_ptr();

        let idx = unsafe {
            getsubopt(
                &mut optionp,
                tokens.as_ptr().cast::<*const u8>(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(idx, -1);
    }

    #[test]
    fn test_getsubopt_advances_past_comma() {
        // After parsing "x,y", optionp should point at "y".
        let tok_x: *const u8 = b"x\0".as_ptr();
        let tokens: [*const u8; 2] = [tok_x, core::ptr::null()];

        let mut input = *b"x,remaining\0";
        let mut optionp: *mut u8 = input.as_mut_ptr();
        let mut valuep: *mut u8 = core::ptr::null_mut();

        let _ = unsafe {
            getsubopt(
                &mut optionp,
                tokens.as_ptr().cast::<*const u8>(),
                &mut valuep,
            )
        };
        // optionp should now point to "remaining".
        assert_eq!(unsafe { *optionp }, b'r');
    }

    // -----------------------------------------------------------------------
    // strtol additional edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strtol_base_36_z_and_10() {
        // "z" in base 36 = 35.
        let v = unsafe { strtol(b"z\0".as_ptr(), core::ptr::null_mut(), 36) };
        assert_eq!(v, 35);
        // "10" in base 36 = 36.
        let v = unsafe { strtol(b"10\0".as_ptr(), core::ptr::null_mut(), 36) };
        assert_eq!(v, 36);
    }

    #[test]
    fn test_strtol_invalid_base() {
        crate::errno::set_errno(0);
        let v = unsafe { strtol(b"123\0".as_ptr(), core::ptr::null_mut(), 1) };
        assert_eq!(v, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        crate::errno::set_errno(0);
        let v = unsafe { strtol(b"123\0".as_ptr(), core::ptr::null_mut(), 37) };
        assert_eq!(v, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_strtol_binary() {
        let v = unsafe { strtol(b"1010\0".as_ptr(), core::ptr::null_mut(), 2) };
        assert_eq!(v, 10);
    }

    #[test]
    fn test_strtol_whitespace_only() {
        let mut end: *const u8 = core::ptr::null();
        let v = unsafe { strtol(b"   \0".as_ptr(), &mut end, 10) };
        assert_eq!(v, 0);
        // endptr should equal nptr (no conversion).
        let input = b"   \0".as_ptr();
        let v2 = unsafe { strtol(input, &mut end, 10) };
        assert_eq!(v2, 0);
        assert_eq!(end, input);
    }

    #[test]
    fn test_strtol_plus_sign() {
        let v = unsafe { strtol(b"+42\0".as_ptr(), core::ptr::null_mut(), 10) };
        assert_eq!(v, 42);
    }

    #[test]
    fn test_strtoul_u64_max() {
        crate::errno::set_errno(0);
        let v = unsafe {
            strtoul(
                b"18446744073709551615\0".as_ptr(),
                core::ptr::null_mut(),
                10,
            )
        };
        assert_eq!(v, u64::MAX);
        assert_eq!(crate::errno::get_errno(), 0); // NOT overflow
    }

    #[test]
    fn test_strtoul_hex_mixed_case() {
        let v = unsafe { strtoul(b"0xABCDEF\0".as_ptr(), core::ptr::null_mut(), 0) };
        assert_eq!(v, 0xABCDEF);
    }

    // -----------------------------------------------------------------------
    // abs / labs / llabs edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_abs_zero() {
        assert_eq!(abs(0), 0);
    }

    #[test]
    fn test_abs_positive() {
        assert_eq!(abs(42), 42);
    }

    #[test]
    fn test_abs_negative() {
        assert_eq!(abs(-42), 42);
    }

    #[test]
    fn test_labs_large_values() {
        assert_eq!(labs(i64::MAX), i64::MAX);
        assert_eq!(labs(-1_000_000_000), 1_000_000_000);
    }

    #[test]
    fn test_llabs_large_values() {
        assert_eq!(llabs(i64::MAX), i64::MAX);
        assert_eq!(llabs(-1_000_000_000), 1_000_000_000);
    }

    // -----------------------------------------------------------------------
    // div / ldiv / lldiv
    // -----------------------------------------------------------------------

    #[test]
    fn test_div_negative_numerator() {
        let result = div(-17, 5);
        assert_eq!(result.quot, -3);
        assert_eq!(result.rem, -2);
    }

    #[test]
    fn test_div_negative_denominator() {
        let result = div(17, -5);
        assert_eq!(result.quot, -3);
        assert_eq!(result.rem, 2);
    }

    #[test]
    fn test_div_exact() {
        let result = div(20, 5);
        assert_eq!(result.quot, 4);
        assert_eq!(result.rem, 0);
    }

    #[test]
    fn test_ldiv_large() {
        let result = ldiv(i64::MAX, 3);
        assert_eq!(result.quot, i64::MAX / 3);
        assert_eq!(result.rem, i64::MAX % 3);
    }

    #[test]
    fn test_lldiv_large() {
        let result = lldiv(i64::MAX, 2);
        assert_eq!(result.quot, i64::MAX / 2);
        assert_eq!(result.rem, 1);
    }

    // -------------------------------------------------------------------
    // Stress tests — qsort
    // -------------------------------------------------------------------

    /// Comparison function for i32 elements (stress tests).
    unsafe extern "C" fn cmp_i32_stress(a: *const u8, b: *const u8) -> i32 {
        let va = unsafe { *(a as *const i32) };
        let vb = unsafe { *(b as *const i32) };
        va.cmp(&vb) as i32
    }

    #[test]
    fn test_qsort_all_same() {
        let mut arr = [42i32; 100];
        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                100,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            );
        }
        for &v in &arr {
            assert_eq!(v, 42);
        }
    }

    #[test]
    fn test_qsort_descending_to_ascending() {
        let mut arr: [i32; 64] = core::array::from_fn(|i| (64 - i) as i32);
        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                64,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            );
        }
        for i in 0..64 {
            assert_eq!(arr[i], (i + 1) as i32);
        }
    }

    #[test]
    fn test_qsort_already_sorted_large() {
        let mut arr: [i32; 128] = core::array::from_fn(|i| i as i32);
        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                128,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            );
        }
        for i in 0..128 {
            assert_eq!(arr[i], i as i32);
        }
    }

    #[test]
    fn test_qsort_two_values_alternating() {
        // Array alternating between 0 and 1.
        let mut arr: [i32; 80] = core::array::from_fn(|i| (i & 1) as i32);
        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                80,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            );
        }
        // First 40 should be 0, last 40 should be 1.
        for i in 0..40 {
            assert_eq!(arr[i], 0);
        }
        for i in 40..80 {
            assert_eq!(arr[i], 1);
        }
    }

    #[test]
    fn test_qsort_negative_values() {
        let mut arr = [-5i32, -1, -100, -50, 0, 10, -3, 7, -999, 42];
        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                10,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            );
        }
        assert_eq!(arr, [-999, -100, -50, -5, -3, -1, 0, 7, 10, 42]);
    }

    #[test]
    fn test_qsort_two_elements_swap() {
        let mut arr = [2i32, 1];
        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                2,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            );
        }
        assert_eq!(arr, [1, 2]);
    }

    #[test]
    fn test_qsort_organ_pipe() {
        // "Organ pipe" pattern: ascending then descending.
        let mut arr: [i32; 50] =
            core::array::from_fn(|i| if i < 25 { i as i32 } else { (50 - i) as i32 });
        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                50,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            );
        }
        // Verify sorted.
        for i in 1..50 {
            assert!(arr[i] >= arr[i - 1]);
        }
    }

    #[test]
    fn test_qsort_with_large_elements() {
        // Elements larger than the 256-byte stack buffer.
        // Use 128-byte structs (would still fit, but tests the path).
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Big {
            key: i32,
            _pad: [u8; 60],
        }

        unsafe extern "C" fn cmp_big(a: *const u8, b: *const u8) -> i32 {
            let va = unsafe { (*(a as *const Big)).key };
            let vb = unsafe { (*(b as *const Big)).key };
            va.cmp(&vb) as i32
        }

        let mut arr: [Big; 10] = core::array::from_fn(|i| Big {
            key: (10 - i) as i32,
            _pad: [0; 60],
        });

        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                10,
                core::mem::size_of::<Big>(),
                Some(cmp_big),
            );
        }

        for i in 0..10 {
            assert_eq!(arr[i].key, (i + 1) as i32);
        }
    }

    #[test]
    fn test_qsort_sawtooth_pattern() {
        // Repeating ascending sequences: 0,1,2,3,0,1,2,3,...
        let mut arr: [i32; 60] = core::array::from_fn(|i| (i % 4) as i32);
        unsafe {
            qsort(
                arr.as_mut_ptr().cast(),
                60,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            );
        }
        for i in 1..60 {
            assert!(arr[i] >= arr[i - 1]);
        }
        // Should have 15 zeros, 15 ones, 15 twos, 15 threes.
        assert_eq!(arr[0], 0);
        assert_eq!(arr[14], 0);
        assert_eq!(arr[15], 1);
        assert_eq!(arr[59], 3);
    }

    // -------------------------------------------------------------------
    // Stress tests — bsearch
    // -------------------------------------------------------------------

    #[test]
    fn test_bsearch_first_element() {
        let arr = [1i32, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let key: i32 = 1;
        let ret = unsafe {
            bsearch(
                (&raw const key).cast(),
                arr.as_ptr().cast(),
                10,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            )
        };
        assert!(!ret.is_null());
        assert_eq!(unsafe { *(ret as *const i32) }, 1);
    }

    #[test]
    fn test_bsearch_last_element() {
        let arr = [1i32, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let key: i32 = 10;
        let ret = unsafe {
            bsearch(
                (&raw const key).cast(),
                arr.as_ptr().cast(),
                10,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            )
        };
        assert!(!ret.is_null());
        assert_eq!(unsafe { *(ret as *const i32) }, 10);
    }

    #[test]
    fn test_bsearch_middle_element() {
        let arr = [10i32, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        let key: i32 = 50;
        let ret = unsafe {
            bsearch(
                (&raw const key).cast(),
                arr.as_ptr().cast(),
                10,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            )
        };
        assert!(!ret.is_null());
        assert_eq!(unsafe { *(ret as *const i32) }, 50);
    }

    #[test]
    fn test_bsearch_not_found_below_range() {
        let arr = [10i32, 20, 30, 40, 50];
        let key: i32 = 5;
        let ret = unsafe {
            bsearch(
                (&raw const key).cast(),
                arr.as_ptr().cast(),
                5,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            )
        };
        assert!(ret.is_null());
    }

    #[test]
    fn test_bsearch_not_found_above_range() {
        let arr = [10i32, 20, 30, 40, 50];
        let key: i32 = 55;
        let ret = unsafe {
            bsearch(
                (&raw const key).cast(),
                arr.as_ptr().cast(),
                5,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            )
        };
        assert!(ret.is_null());
    }

    #[test]
    fn test_bsearch_not_found_between_elements() {
        let arr = [10i32, 20, 30, 40, 50];
        let key: i32 = 25;
        let ret = unsafe {
            bsearch(
                (&raw const key).cast(),
                arr.as_ptr().cast(),
                5,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            )
        };
        assert!(ret.is_null());
    }

    #[test]
    fn test_bsearch_single_element_found() {
        let arr = [42i32];
        let key: i32 = 42;
        let ret = unsafe {
            bsearch(
                (&raw const key).cast(),
                arr.as_ptr().cast(),
                1,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            )
        };
        assert!(!ret.is_null());
        assert_eq!(unsafe { *(ret as *const i32) }, 42);
    }

    #[test]
    fn test_bsearch_single_element_not_found() {
        let arr = [42i32];
        let key: i32 = 99;
        let ret = unsafe {
            bsearch(
                (&raw const key).cast(),
                arr.as_ptr().cast(),
                1,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            )
        };
        assert!(ret.is_null());
    }

    #[test]
    fn test_bsearch_large_array() {
        // Search through a 256-element array.
        let arr: [i32; 256] = core::array::from_fn(|i| (i * 3) as i32);
        // Search for element at index 200 (value 600).
        let key: i32 = 600;
        let ret = unsafe {
            bsearch(
                (&raw const key).cast(),
                arr.as_ptr().cast(),
                256,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            )
        };
        assert!(!ret.is_null());
        assert_eq!(unsafe { *(ret as *const i32) }, 600);
    }

    #[test]
    fn test_bsearch_large_array_not_found() {
        let arr: [i32; 256] = core::array::from_fn(|i| (i * 3) as i32);
        // Value 601 doesn't exist (only multiples of 3).
        let key: i32 = 601;
        let ret = unsafe {
            bsearch(
                (&raw const key).cast(),
                arr.as_ptr().cast(),
                256,
                core::mem::size_of::<i32>(),
                Some(cmp_i32_stress),
            )
        };
        assert!(ret.is_null());
    }

    #[test]
    fn test_bsearch_two_elements() {
        let arr = [5i32, 10];
        let key1: i32 = 5;
        let key2: i32 = 10;
        let key3: i32 = 7;

        let r1 = unsafe {
            bsearch(
                (&raw const key1).cast(),
                arr.as_ptr().cast(),
                2,
                4,
                Some(cmp_i32_stress),
            )
        };
        let r2 = unsafe {
            bsearch(
                (&raw const key2).cast(),
                arr.as_ptr().cast(),
                2,
                4,
                Some(cmp_i32_stress),
            )
        };
        let r3 = unsafe {
            bsearch(
                (&raw const key3).cast(),
                arr.as_ptr().cast(),
                2,
                4,
                Some(cmp_i32_stress),
            )
        };

        assert!(!r1.is_null());
        assert!(!r2.is_null());
        assert!(r3.is_null());
        assert_eq!(unsafe { *(r1 as *const i32) }, 5);
        assert_eq!(unsafe { *(r2 as *const i32) }, 10);
    }

    // -------------------------------------------------------------------
    // Stress tests — strtol edge cases
    // -------------------------------------------------------------------

    #[test]
    fn test_strtol_base0_hex_prefix() {
        let s = b"0x1F\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 0) };
        assert_eq!(val, 0x1F);
    }

    #[test]
    fn test_strtol_base0_octal_prefix() {
        let s = b"0777\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 0) };
        assert_eq!(val, 0o777);
    }

    #[test]
    fn test_strtol_base0_decimal() {
        let s = b"123\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 0) };
        assert_eq!(val, 123);
    }

    #[test]
    fn test_strtol_base36() {
        // "z" in base 36 = 35.
        let s = b"z\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 36) };
        assert_eq!(val, 35);
    }

    #[test]
    fn test_strtol_base36_multidigit() {
        // "10" in base 36 = 36.
        let s = b"10\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 36) };
        assert_eq!(val, 36);
    }

    #[test]
    fn test_strtol_leading_whitespace() {
        let s = b"  \t  42\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 10) };
        assert_eq!(val, 42);
    }

    #[test]
    fn test_strtol_stress_neg_overflow() {
        // i64::MIN = -9223372036854775808
        let s = b"-9223372036854775809\0";
        let mut end: *const u8 = core::ptr::null();
        crate::errno::set_errno(0);
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 10) };
        assert_eq!(val, i64::MIN);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);
    }

    #[test]
    fn test_strtol_stress_pos_overflow() {
        // i64::MAX = 9223372036854775807
        let s = b"9223372036854775808\0";
        let mut end: *const u8 = core::ptr::null();
        crate::errno::set_errno(0);
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 10) };
        assert_eq!(val, i64::MAX);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);
    }

    #[test]
    fn test_strtol_just_sign_no_digits() {
        let s = b"+\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 10) };
        assert_eq!(val, 0);
    }

    #[test]
    fn test_strtol_endptr_stops_at_invalid() {
        let s = b"123abc\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 10) };
        assert_eq!(val, 123);
        assert!(!end.is_null());
        // end should point to 'a'.
        assert_eq!(unsafe { *end }, b'a');
    }

    // -------------------------------------------------------------------
    // Stress tests — strtod edge cases
    // -------------------------------------------------------------------

    #[test]
    fn test_strtod_scientific_large_exponent() {
        let s = b"1.5e10\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        let expected = 1.5e10;
        let rel = (val - expected).abs() / expected;
        assert!(rel < 1e-10);
    }

    #[test]
    fn test_strtod_scientific_negative_exponent() {
        let s = b"3.14e-5\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        let expected = 3.14e-5;
        let rel = (val - expected).abs() / expected;
        assert!(rel < 1e-10);
    }

    #[test]
    fn test_strtod_just_zero() {
        let s = b"0.0\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert_eq!(val, 0.0);
    }

    #[test]
    fn test_strtod_stress_negative_val() {
        let s = b"-2.718\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        let expected = -2.718;
        let diff = (val - expected).abs();
        assert!(diff < 1e-10);
    }

    #[test]
    fn test_strtod_stress_leading_dot() {
        let s = b".5\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert!((val - 0.5).abs() < 1e-15);
    }

    #[test]
    fn test_strtod_trailing_garbage() {
        let s = b"3.14xyz\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert!((val - 3.14).abs() < 1e-10);
        assert_eq!(unsafe { *end }, b'x');
    }

    #[test]
    fn test_strtod_stress_infinity() {
        let s = b"inf\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert!(val.is_infinite() && val > 0.0);
    }

    #[test]
    fn test_strtod_stress_nan_val() {
        let s = b"nan\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert!(val.is_nan());
    }

    // -------------------------------------------------------------------
    // Stress tests — abs/labs edge cases
    // -------------------------------------------------------------------

    #[test]
    fn test_abs_min_saturates() {
        // abs(i32::MIN) would overflow. Our impl uses saturating_neg.
        let result = abs(i32::MIN);
        assert_eq!(result, i32::MAX);
    }

    #[test]
    fn test_labs_min_saturates() {
        let result = labs(i64::MIN);
        assert_eq!(result, i64::MAX);
    }

    // -------------------------------------------------------------------
    // Stress tests — div edge cases
    // -------------------------------------------------------------------

    #[test]
    fn test_div_zero_denominator() {
        let result = div(42, 0);
        assert_eq!(result.quot, 0);
        assert_eq!(result.rem, 0);
    }

    #[test]
    fn test_div_min_by_neg_one() {
        // i32::MIN / -1 overflows in C (UB). We return MIN.
        let result = div(i32::MIN, -1);
        assert_eq!(result.quot, i32::MIN);
        assert_eq!(result.rem, 0);
    }

    #[test]
    fn test_ldiv_zero_denominator() {
        let result = ldiv(42, 0);
        assert_eq!(result.quot, 0);
        assert_eq!(result.rem, 0);
    }

    #[test]
    fn test_ldiv_min_by_neg_one() {
        let result = ldiv(i64::MIN, -1);
        assert_eq!(result.quot, i64::MIN);
        assert_eq!(result.rem, 0);
    }

    // -------------------------------------------------------------------
    // Stress tests — strtod endptr edge cases
    // -------------------------------------------------------------------

    #[test]
    fn test_strtod_dot_only() {
        // Just "." — no digits. Should return 0, endptr at start.
        let s = b".\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert_eq!(val, 0.0);
        assert_eq!(end, s.as_ptr()); // no valid conversion
    }

    #[test]
    fn test_strtod_stress_trailing_dot_endptr() {
        // "5." — trailing dot is valid, result is 5.0.
        let s = b"5.\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert!((val - 5.0).abs() < 1e-15);
        // endptr should be past the dot.
        let consumed = end as usize - s.as_ptr() as usize;
        assert_eq!(consumed, 2);
    }

    #[test]
    fn test_strtod_stress_e_rollback() {
        // "3.14e" — 'e' without exponent digits is not consumed.
        let s = b"3.14e\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert!((val - 3.14).abs() < 1e-10);
        let consumed = end as usize - s.as_ptr() as usize;
        assert_eq!(consumed, 4); // stops at 'e'
    }

    #[test]
    fn test_strtod_e_plus_without_digits() {
        // "3.14e+" — 'e+' without exponent digits is not consumed.
        let s = b"3.14e+\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert!((val - 3.14).abs() < 1e-10);
        let consumed = end as usize - s.as_ptr() as usize;
        assert_eq!(consumed, 4); // stops at 'e'
    }

    #[test]
    fn test_strtod_dot_then_e() {
        // ".e5" — dot but no digits on either side. No valid conversion.
        let s = b".e5\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert_eq!(val, 0.0);
        assert_eq!(end, s.as_ptr()); // no valid conversion
    }

    #[test]
    fn test_strtod_neg_infinity() {
        let s = b"-inf\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert!(val.is_infinite() && val < 0.0);
        let consumed = end as usize - s.as_ptr() as usize;
        assert_eq!(consumed, 4);
    }

    #[test]
    fn test_strtod_full_infinity() {
        let s = b"INFINITY\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert!(val.is_infinite() && val > 0.0);
        let consumed = end as usize - s.as_ptr() as usize;
        assert_eq!(consumed, 8); // consumed all of "INFINITY"
    }

    #[test]
    fn test_strtod_only_whitespace() {
        let s = b"   \0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert_eq!(val, 0.0);
        assert_eq!(end, s.as_ptr()); // no valid conversion
    }

    #[test]
    fn test_strtod_zero_exponent() {
        let s = b"5e0\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtod(s.as_ptr(), &raw mut end) };
        assert!((val - 5.0).abs() < 1e-15);
    }

    // -------------------------------------------------------------------
    // Stress tests — strtoul boundary values
    // -------------------------------------------------------------------

    #[test]
    fn test_strtoul_max() {
        let s = b"18446744073709551615\0"; // u64::MAX
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtoul(s.as_ptr(), &raw mut end, 10) };
        assert_eq!(val, u64::MAX);
    }

    #[test]
    fn test_strtoul_stress_overflow_by_one() {
        let s = b"18446744073709551616\0"; // u64::MAX + 1
        let mut end: *const u8 = core::ptr::null();
        crate::errno::set_errno(0);
        let val = unsafe { strtoul(s.as_ptr(), &raw mut end, 10) };
        assert_eq!(val, u64::MAX);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);
    }

    #[test]
    fn test_strtol_exact_min() {
        let s = b"-9223372036854775808\0"; // i64::MIN
        let mut end: *const u8 = core::ptr::null();
        crate::errno::set_errno(0);
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 10) };
        assert_eq!(val, i64::MIN);
        // Exact min is valid, should NOT set ERANGE.
        assert_eq!(crate::errno::get_errno(), 0);
    }

    #[test]
    fn test_strtol_exact_max() {
        let s = b"9223372036854775807\0"; // i64::MAX
        let mut end: *const u8 = core::ptr::null();
        crate::errno::set_errno(0);
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 10) };
        assert_eq!(val, i64::MAX);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    #[test]
    fn test_strtol_stress_invalid_base_37() {
        let s = b"42\0";
        let mut end: *const u8 = core::ptr::null();
        crate::errno::set_errno(0);
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 37) };
        assert_eq!(val, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_strtol_base2() {
        let s = b"1010\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 2) };
        assert_eq!(val, 10);
    }

    #[test]
    fn test_strtol_base0_just_zero() {
        // "0" in base 0 should parse as 0 (not octal prefix).
        let s = b"0\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtol(s.as_ptr(), &raw mut end, 0) };
        assert_eq!(val, 0);
    }

    // -----------------------------------------------------------------------
    // strtoll / strtoull — LP64 aliases
    // -----------------------------------------------------------------------

    #[test]
    fn test_strtoll_positive() {
        let s = b"12345\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtoll(s.as_ptr(), &raw mut end, 10) };
        assert_eq!(val, 12345);
    }

    #[test]
    fn test_strtoll_negative() {
        let s = b"-42\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtoll(s.as_ptr(), &raw mut end, 10) };
        assert_eq!(val, -42);
    }

    #[test]
    fn test_strtoull_positive() {
        let s = b"999\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtoull(s.as_ptr(), &raw mut end, 10) };
        assert_eq!(val, 999);
    }

    #[test]
    fn test_strtoull_hex() {
        let s = b"0xFF\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtoull(s.as_ptr(), &raw mut end, 0) };
        assert_eq!(val, 255);
    }

    // -----------------------------------------------------------------------
    // strtold -- at the long double's own precision
    // -----------------------------------------------------------------------

    /// A `long double`'s bits, `(sign_exp, significand)`.
    fn ld_bits(v: crate::x87::LongDouble) -> (u16, u64) {
        (v.sign_exp, v.significand)
    }

    #[test]
    fn test_strtold_basic() {
        let s = b"3.14\0";
        let mut end: *const u8 = core::ptr::null();
        let val = unsafe { strtold(s.as_ptr(), &raw mut end) };
        // 3.14 to 64 bits: 0xc8f5c28f5c28f5c3 * 2^-62, rounded up.
        assert_eq!(ld_bits(val), (0x4000, 0xC8F5_C28F_5C28_F5C3));
        assert_eq!(end, unsafe { s.as_ptr().add(4) });
    }

    #[test]
    fn test_strtold_null() {
        let val = unsafe { strtold(core::ptr::null(), core::ptr::null_mut()) };
        assert_eq!(ld_bits(val), (0, 0));
    }

    /// The digits a `double` cannot hold: 2^64 + 1 is a `long double`, and
    /// 0.1 has eleven more bits than a `double` gives it.
    #[test]
    fn strtold_keeps_all_64_bits() {
        let v = unsafe { strtold(b"18446744073709551617\0".as_ptr(), core::ptr::null_mut()) };
        // 2^64 + 1 needs 65 bits: a tie between 2^64 and 2^64 + 2, to even.
        assert_eq!(ld_bits(v), (0x403F, 0x8000_0000_0000_0000));
        let v = unsafe { strtold(b"18446744073709551615\0".as_ptr(), core::ptr::null_mut()) };
        assert_eq!(ld_bits(v), (0x403E, u64::MAX));
        let v = unsafe { strtold(b"0.1\0".as_ptr(), core::ptr::null_mut()) };
        assert_eq!(ld_bits(v), (0x3FFB, 0xCCCC_CCCC_CCCC_CCCD));
        let v = unsafe {
            strtold(
                b"-0x1.fffffffffffffffep+16383\0".as_ptr(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(ld_bits(v), (0xFFFE, u64::MAX));
    }

    /// The range ends: the least subnormal, the greatest finite value, and
    /// one past each; `ERANGE` as glibc sets it.
    #[test]
    fn strtold_range_ends() {
        let conv = |t: &[u8]| {
            crate::errno::set_errno(0);
            let v = unsafe { strtold(t.as_ptr(), core::ptr::null_mut()) };
            (
                ld_bits(v),
                crate::errno::get_errno() == crate::errno::ERANGE,
            )
        };
        assert_eq!(
            conv(b"1.18973149535723176502e+4932\0"),
            ((0x7FFE, u64::MAX), false)
        );
        assert_eq!(conv(b"1.2e4932\0"), ((0x7FFF, 1 << 63), true));
        assert_eq!(conv(b"3.64519953188247460253e-4951\0"), ((0, 1), true));
        assert_eq!(conv(b"1e-4952\0"), ((0, 0), true));
        assert_eq!(conv(b"1e-10000\0"), ((0, 0), true));
        assert_eq!(conv(b"1e10000\0"), ((0x7FFF, 1 << 63), true));
        // The least normal number is no error, and neither is an exact
        // subnormal.
        assert_eq!(conv(b"0x1p-16382\0"), ((0x0001, 1 << 63), false));
        assert_eq!(conv(b"0x1p-16445\0"), ((0, 1), false));
    }

    /// NaN and infinity keep their sign; a payload lands in the low 62 bits.
    #[test]
    fn strtold_nan_and_infinity() {
        let v = unsafe { strtold(b"-inf\0".as_ptr(), core::ptr::null_mut()) };
        assert_eq!(ld_bits(v), (0xFFFF, 1 << 63));
        let v = unsafe { strtold(b"nan\0".as_ptr(), core::ptr::null_mut()) };
        assert_eq!(ld_bits(v), (0x7FFF, 0xC000_0000_0000_0000));
        let v = unsafe { strtold(b"-nan(0x12)\0".as_ptr(), core::ptr::null_mut()) };
        assert_eq!(ld_bits(v), (0xFFFF, 0xC000_0000_0000_0012));
    }

    /// A literal longer than a `double`'s 768 digits spills its digits to
    /// the heap, and every one of them still counts: this is the midpoint
    /// between 1 and the next `long double` up, `1 + 2^-64`, written out in
    /// full and then nudged either side.
    #[test]
    fn strtold_digits_past_a_doubles_worth() {
        // 2^-64 = 5.42101086242752217003726400434970855712890625e-20 exactly,
        // so 1 + 2^-64 has 64 significant decimal digits; padded with
        // zeroes to 2000 digits it is still the exact tie.
        let mut tie =
            b"1.0000000000000000000542101086242752217003726400434970855712890625".to_vec();
        tie.resize(2002, b'0');
        let mut above = tie.clone();
        above.push(b'1');
        tie.push(0);
        above.push(0);
        let v = unsafe { strtold(tie.as_ptr(), core::ptr::null_mut()) };
        assert_eq!(ld_bits(v), (0x3FFF, 1 << 63), "a tie goes to even");
        let v = unsafe { strtold(above.as_ptr(), core::ptr::null_mut()) };
        assert_eq!(ld_bits(v), (0x3FFF, (1 << 63) | 1), "a hair above goes up");
    }

    // -----------------------------------------------------------------------
    // atof
    // -----------------------------------------------------------------------

    #[test]
    fn test_atof_basic() {
        let val = unsafe { atof(b"2.5\0".as_ptr()) };
        assert!((val - 2.5).abs() < 0.001);
    }

    #[test]
    fn test_atof_negative() {
        let val = unsafe { atof(b"-1.5\0".as_ptr()) };
        assert!((val - (-1.5)).abs() < 0.001);
    }

    #[test]
    fn test_atof_zero() {
        let val = unsafe { atof(b"0\0".as_ptr()) };
        assert_eq!(val, 0.0);
    }

    #[test]
    fn test_atof_with_exponent() {
        let val = unsafe { atof(b"1e3\0".as_ptr()) };
        assert!((val - 1000.0).abs() < 0.1);
    }

    // -----------------------------------------------------------------------
    // mktemp / mkstemp / mkostemp
    // -----------------------------------------------------------------------

    #[test]
    fn test_mktemp_null_returns_null() {
        let ret = unsafe { mktemp(core::ptr::null_mut()) };
        assert!(ret.is_null());
    }

    #[test]
    fn test_mktemp_too_short() {
        let mut tmpl = *b"XX\0";
        crate::errno::set_errno(0);
        let ret = unsafe { mktemp(tmpl.as_mut_ptr()) };
        // Should fail — template needs at least 6 trailing X's.
        assert!(ret.is_null() || unsafe { *ret } == 0);
    }

    #[test]
    fn test_mkstemp_null_returns_error() {
        crate::errno::set_errno(0);
        let ret = unsafe { mkstemp(core::ptr::null_mut()) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mkstemp_too_short_template() {
        let mut tmpl = *b"ab\0";
        crate::errno::set_errno(0);
        let ret = unsafe { mkstemp(tmpl.as_mut_ptr()) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mkostemp_null_returns_error() {
        crate::errno::set_errno(0);
        let ret = unsafe { mkostemp(core::ptr::null_mut(), 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // tmpfile
    // -----------------------------------------------------------------------

    #[test]
    fn test_tmpfile_no_crash() {
        // tmpfile tries to create a temp file. On test host it may or
        // may not succeed depending on filesystem access.
        let ret = tmpfile();
        if !ret.is_null() {
            // If it succeeded, close the FILE*.
            crate::stdio::fclose(ret);
        }
    }

    // -----------------------------------------------------------------------
    // system
    // -----------------------------------------------------------------------

    #[test]
    fn test_system_null_checks_shell() {
        // system(NULL) checks if a command processor is available.
        // On test host, /bin/sh might not exist → returns 0.
        let ret = system(core::ptr::null());
        // Result is either 0 (no shell) or non-zero (shell available).
        let _ = ret;
    }

    extern "C" fn on_int(_: i32) {}

    /// Whatever happens to the shell, the caller gets back `SIGINT`'s and
    /// `SIGQUIT`'s actions and its mask as they were.  On the host no
    /// program can start, so this is the failed spawn's path: an exit of
    /// 127 as a wait status, with `errno` the spawn's reason.
    #[test]
    fn test_system_restores_what_it_changed() {
        use crate::signal::{
            SIG_BLOCK, SIG_IGN, SIG_SETMASK, SIGCHLD, SIGINT, SIGQUIT, SIGUSR1, Sigaction, SigsetT,
            sigaction, sigaddset, sigismember, sigprocmask,
        };
        let handler = on_int as *const () as usize;
        let mine = Sigaction {
            sa_handler: handler,
            ..crate::signal::DEFAULT_SIGACTION
        };
        let ign = Sigaction {
            sa_handler: SIG_IGN,
            ..crate::signal::DEFAULT_SIGACTION
        };
        let mut usr1 = SigsetT::EMPTY;
        let mut before = SigsetT::EMPTY;
        // SAFETY: valid actions and locals.
        unsafe {
            assert_eq!(sigaction(SIGINT, &raw const mine, core::ptr::null_mut()), 0);
            assert_eq!(sigaction(SIGQUIT, &raw const ign, core::ptr::null_mut()), 0);
            assert_eq!(sigaddset(&raw mut usr1, SIGUSR1), 0);
        }
        assert_eq!(sigprocmask(SIG_BLOCK, &raw const usr1, &raw mut before), 0);

        crate::errno::set_errno(0);
        let status = system(c"exit 3".as_ptr().cast());
        assert_eq!(status, 127 << 8, "the shell could not start here");
        assert_ne!(crate::errno::get_errno(), 0, "and errno says why");

        let mut now = crate::signal::DEFAULT_SIGACTION;
        // SAFETY: an enquiry into a local.
        assert_eq!(
            unsafe { sigaction(SIGINT, core::ptr::null(), &raw mut now) },
            0
        );
        assert_eq!(now.sa_handler, handler, "SIGINT's handler is back");
        // SAFETY: as above.
        assert_eq!(
            unsafe { sigaction(SIGQUIT, core::ptr::null(), &raw mut now) },
            0
        );
        assert_eq!(now.sa_handler, SIG_IGN, "SIGQUIT stays ignored");
        let mut mask = SigsetT::EMPTY;
        assert_eq!(
            sigprocmask(SIG_SETMASK, core::ptr::null(), &raw mut mask),
            0
        );
        // SAFETY: a local.
        unsafe {
            assert_eq!(
                sigismember(&raw const mask, SIGUSR1),
                1,
                "the caller's own block kept"
            );
            assert_eq!(
                sigismember(&raw const mask, SIGCHLD),
                0,
                "SIGCHLD unblocked again"
            );
        }
        assert_eq!(
            with_system_save(|save| save.users),
            0,
            "no call left running"
        );

        // Put back what this test changed.
        let dfl = crate::signal::DEFAULT_SIGACTION;
        // SAFETY: valid actions.
        unsafe {
            assert_eq!(sigaction(SIGINT, &raw const dfl, core::ptr::null_mut()), 0);
            assert_eq!(sigaction(SIGQUIT, &raw const dfl, core::ptr::null_mut()), 0);
        }
        assert_eq!(
            sigprocmask(SIG_SETMASK, &raw const before, core::ptr::null_mut()),
            0
        );
    }

    // -----------------------------------------------------------------------
    // mkstemps
    // -----------------------------------------------------------------------

    #[test]
    fn test_mkstemps_null_template() {
        let ret = unsafe { mkstemps(core::ptr::null_mut(), 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mkstemps_negative_suffix() {
        let mut tmpl = *b"/tmp/testXXXXXX.txt\0";
        crate::errno::set_errno(0);
        let ret = unsafe { mkstemps(tmpl.as_mut_ptr(), -1) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mkstemps_too_short() {
        // Template with suffix but fewer than 6 X's.
        let mut tmpl = *b"abXX.c\0";
        crate::errno::set_errno(0);
        let ret = unsafe { mkstemps(tmpl.as_mut_ptr(), 2) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mkstemps_missing_x_before_suffix() {
        // 6 chars before suffix are not all X.
        let mut tmpl = *b"/tmp/testABCDEF.txt\0";
        crate::errno::set_errno(0);
        let ret = unsafe { mkstemps(tmpl.as_mut_ptr(), 4) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // mkostemps
    // -----------------------------------------------------------------------

    #[test]
    fn test_mkostemps_null_template() {
        let ret = unsafe { mkostemps(core::ptr::null_mut(), 0, 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mkostemps_negative_suffix() {
        let mut tmpl = *b"/tmp/testXXXXXX.txt\0";
        crate::errno::set_errno(0);
        let ret = unsafe { mkostemps(tmpl.as_mut_ptr(), -1, 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mkostemps_too_short() {
        let mut tmpl = *b"X.c\0";
        crate::errno::set_errno(0);
        let ret = unsafe { mkostemps(tmpl.as_mut_ptr(), 2, 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mkostemps_valid_suffix_validation() {
        // Exactly 6 X's before a 4-char suffix — should pass validation
        // (the actual file creation may fail on the test host, but that's ok).
        let mut tmpl = *b"/tmp/testXXXXXX.txt\0";
        let _ret = unsafe { mkostemps(tmpl.as_mut_ptr(), 4, 0) };
        // Just verify no crash.
    }

    // -----------------------------------------------------------------------
    // a64l — base-64 string to long
    // -----------------------------------------------------------------------

    #[test]
    fn test_a64l_null() {
        assert_eq!(a64l(core::ptr::null()), 0);
    }

    #[test]
    fn test_a64l_empty() {
        assert_eq!(a64l(b"\0".as_ptr()), 0);
    }

    #[test]
    fn test_a64l_dot() {
        // '.' encodes 0.
        assert_eq!(a64l(b".\0".as_ptr()), 0);
    }

    #[test]
    fn test_a64l_slash() {
        // '/' encodes 1.
        assert_eq!(a64l(b"/\0".as_ptr()), 1);
    }

    #[test]
    fn test_a64l_zero_char() {
        // '0' encodes 2.
        assert_eq!(a64l(b"0\0".as_ptr()), 2);
    }

    #[test]
    fn test_a64l_nine_char() {
        // '9' encodes 11.
        assert_eq!(a64l(b"9\0".as_ptr()), 11);
    }

    #[test]
    fn test_a64l_uppercase_a() {
        // 'A' encodes 12.
        assert_eq!(a64l(b"A\0".as_ptr()), 12);
    }

    #[test]
    fn test_a64l_lowercase_a() {
        // 'a' encodes 38.
        assert_eq!(a64l(b"a\0".as_ptr()), 38);
    }

    #[test]
    fn test_a64l_two_chars() {
        // "//" → 1 | (1 << 6) = 1 + 64 = 65.
        assert_eq!(a64l(b"//\0".as_ptr()), 65);
    }

    // -----------------------------------------------------------------------
    // l64a — long to base-64 string
    // -----------------------------------------------------------------------

    #[test]
    fn test_l64a_zero() {
        let _g = lock_l64a_for_test();
        let result = l64a(0);
        assert!(!result.is_null());
        assert_eq!(unsafe { *result }, 0, "l64a(0) should return empty string");
    }

    #[test]
    fn test_l64a_one() {
        let _g = lock_l64a_for_test();
        let result = l64a(1);
        assert!(!result.is_null());
        assert_eq!(unsafe { *result }, b'/', "l64a(1) should be '/'");
    }

    #[test]
    fn test_l64a_two() {
        let _g = lock_l64a_for_test();
        let result = l64a(2);
        assert!(!result.is_null());
        assert_eq!(unsafe { *result }, b'0', "l64a(2) should be '0'");
    }

    #[test]
    fn test_a64l_l64a_roundtrip() {
        let _g = lock_l64a_for_test();
        // Encode then decode should give back the original.
        for val in [1i64, 42, 255, 1000, 12345, 0x3FFFF] {
            let encoded = l64a(val);
            let decoded = a64l(encoded);
            assert_eq!(decoded, val, "roundtrip failed for {val}");
        }
    }

    #[test]
    fn test_l64a_63() {
        let _g = lock_l64a_for_test();
        // 63 → 'z' (last base-64 digit).
        let result = l64a(63);
        assert_eq!(unsafe { *result }, b'z');
    }

    // ===================================================================
    // Additional coverage — atoll
    // ===================================================================

    #[test]
    fn test_atoll_large_value() {
        assert_eq!(unsafe { atoll(b"9876543210\0".as_ptr()) }, 9_876_543_210);
    }

    #[test]
    fn test_atoll_negative_large() {
        assert_eq!(unsafe { atoll(b"-9876543210\0".as_ptr()) }, -9_876_543_210);
    }

    #[test]
    fn test_atoi_plus_sign() {
        assert_eq!(unsafe { atoi(b"+5\0".as_ptr()) }, 5);
    }

    #[test]
    fn test_lldiv_large_values() {
        let r = lldiv(1_000_000_007, 1000);
        assert_eq!(r.quot, 1_000_000);
        assert_eq!(r.rem, 7);
    }

    #[test]
    fn test_lldiv_negative_dividend() {
        let r = lldiv(-1_000_000_007, 1000);
        assert_eq!(r.quot, -1_000_000);
        assert_eq!(r.rem, -7);
    }

    // -- strtod is correctly rounded --
    //
    // Rust's own `str::parse::<f64>()` is correctly rounded, so it is an
    // independent oracle: any disagreement is a bug in one of the two, and
    // these inputs are the ones that historically broke the old
    // multiply-by-a-power-of-ten parser.

    fn parse(text: &str) -> f64 {
        let mut z = text.as_bytes().to_vec();
        z.push(0);
        unsafe { strtod(z.as_ptr(), core::ptr::null_mut()) }
    }

    // -----------------------------------------------------------------------
    // Hexadecimal floats (C99 7.20.1.3)
    //
    // Every expectation is glibc's own behaviour for the same input, captured
    // by running the equivalent C program.  Hex is the exact form: the radix
    // is a power of the base, so parsing is pure bit assembly with no base
    // conversion, and %a output must read back bit-for-bit identical.
    // -----------------------------------------------------------------------

    /// Parse and report where the scan stopped.
    fn parse_end(text: &str) -> (f64, usize) {
        let mut z = text.as_bytes().to_vec();
        z.push(0);
        let mut end: *const u8 = core::ptr::null();
        let v = unsafe { strtod(z.as_ptr(), &raw mut end) };
        let consumed = if end.is_null() {
            0
        } else {
            (end as usize).wrapping_sub(z.as_ptr() as usize)
        };
        (v, consumed)
    }

    #[test]
    fn strtod_reads_hex_floats() {
        assert_eq!(parse("0x1p+0"), 1.0);
        assert_eq!(parse("0x1"), 1.0); // the binary exponent is optional
        assert_eq!(parse("0X1P0"), 1.0);
        assert_eq!(parse("0x1.8p+1"), 3.0);
        assert_eq!(parse("0x.8p1"), 1.0); // a point may lead the significand
        assert_eq!(parse("0x1.8"), 1.5);
        assert_eq!(parse("0x0p+0"), 0.0);
        assert_eq!(parse("-0x1.8p+0"), -1.5);
        assert_eq!(parse("0xabcdefp0"), 11_259_375.0);
        assert_eq!(parse("0xABCDEFp0"), 11_259_375.0);
        // 'e' is a hex digit here, not an exponent marker.
        assert_eq!(parse("0x1e5"), 485.0);
        assert_eq!(parse("0x0.5p1"), 0.625);
    }

    #[test]
    fn strtod_hex_rounds_ties_to_even() {
        // Exactly one ulp below the tie, at the tie, and just above it.
        assert_eq!(parse("0x1.00000000000008p+0"), 1.0); // tie, 1.0 is even
        assert_eq!(
            parse("0x1.00000000000018p+0"),
            f64::from_bits(1.0f64.to_bits() + 2)
        );
        // Half of the least subnormal is a tie against zero.
        assert_eq!(parse("0x1p-1075"), 0.0);
        assert_eq!(parse("0x1.8p-1075"), f64::from_bits(1));
        assert_eq!(parse("0x1p-1074"), f64::from_bits(1));
        assert_eq!(parse("0x0.0000000000001p-1022"), f64::from_bits(1));
    }

    #[test]
    fn strtod_hex_spans_the_whole_range() {
        assert_eq!(parse("0x1.fffffffffffffp+1023"), f64::MAX);
        assert_eq!(parse("0x1.fffffffffffff8p+1023"), f64::INFINITY);
        assert_eq!(parse("0x1p+1024"), f64::INFINITY);
        assert_eq!(parse("0x1p-1022"), f64::MIN_POSITIVE);
        // Exponents far outside anything representable must saturate, not wrap.
        assert_eq!(parse("0x1p1000000000000"), f64::INFINITY);
        assert_eq!(parse("0x1p-1000000000000"), 0.0);
        // 26 digits: past the 20 that are stored, so the rest ride the
        // exponent.  16^25 is 2^100 exactly.
        assert_eq!(parse("0x10000000000000000000000000p0"), 2f64.powi(100));
    }

    #[test]
    fn strtod_backs_out_of_an_incomplete_hex_prefix() {
        // "0x" with no digit is not a prefix at all: the subject sequence is
        // the single "0" and the "x" is left for the caller.
        assert_eq!(parse_end("0xg"), (0.0, 1));
        assert_eq!(parse_end("0x"), (0.0, 1));
        assert_eq!(parse_end("0x.p1"), (0.0, 1));
        // A "p" with no digits is likewise not consumed.
        assert_eq!(parse_end("0x1p"), (1.0, 3));
        assert_eq!(parse_end("0x1pz"), (1.0, 3));
        assert_eq!(parse_end("0x1.8p+1"), (3.0, 8));
        assert_eq!(parse_end("0x1x"), (1.0, 3));
    }

    #[test]
    fn strtod_hex_sets_erange_like_the_decimal_path() {
        crate::errno::set_errno(0);
        assert_eq!(parse("0x1p+1024"), f64::INFINITY);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);

        crate::errno::set_errno(0);
        assert_eq!(parse("0x1p-2000"), 0.0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);

        crate::errno::set_errno(0);
        assert_eq!(parse("0x1p+0"), 1.0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    #[test]
    fn strtof_reads_hex_floats_in_its_own_precision() {
        let f = |text: &str| {
            let mut z = text.as_bytes().to_vec();
            z.push(0);
            unsafe { strtof(z.as_ptr(), core::ptr::null_mut()) }
        };
        assert_eq!(f("0x1p+0"), 1.0f32);
        assert_eq!(f("0x1.8p+1"), 3.0f32);
        // Rounded to f32 straight from the digits, as glibc's strtof does.
        assert_eq!(f("0x1.999999999999ap-4"), 0.1f32);
        assert_eq!(f("0x1p+128"), f32::INFINITY);
        assert_eq!(f("0x1p-149"), f32::from_bits(1));
        assert_eq!(f("0x1p-150"), 0.0f32); // tie against zero, rounds down
    }

    #[test]
    fn strtod_reaches_dbl_max() {
        // The old parser multiplied by 10^308 and overflowed to infinity.
        let s = "1.7976931348623157e308";
        assert_eq!(parse(s), f64::MAX);
        assert_eq!(parse(s), s.parse::<f64>().unwrap());
        assert!(parse(s).is_finite());
    }

    #[test]
    fn strtod_reaches_the_subnormals() {
        // The old parser divided by 10^324, which underflowed to zero, so the
        // entire subnormal range parsed as 0.
        for s in [
            "5e-324",
            "4.9406564584124654e-324",
            "1e-310",
            "2.2250738585072011e-308",
            "1.2345678901234e-320",
        ] {
            let want = s.parse::<f64>().unwrap();
            assert!(want != 0.0, "oracle says {s} is zero");
            assert_eq!(parse(s), want, "strtod({s})");
        }
    }

    #[test]
    fn strtod_rounds_the_boundary_cases() {
        // Exactly half an ulp above zero: a tie, and zero is the even side.
        let half = "2.470328229206232720882843964341106861825e-324";
        assert_eq!(parse(half), 0.0);
        // A hair more, so no longer a tie.
        let more = "2.470328229206232720882843964341106861826e-324";
        assert_eq!(parse(more), f64::from_bits(1));
        // Half-way between 2^53 and 2^53+2 is a tie that must round down.
        assert_eq!(parse("9007199254740993"), 9007199254740992.0);
        // 2^53+3 is nearer the odd neighbour above.
        assert_eq!(parse("9007199254740995"), 9007199254740996.0);
        // Overflow is decided by rounding, not by magnitude: this is under
        // the half-way point to infinity and must stay finite.
        assert_eq!(parse("1.7976931348623158e308"), f64::MAX);
        assert!(parse("1.7976931348623159e308").is_infinite());
    }

    #[test]
    fn strtod_ignores_digits_past_the_deciding_one() {
        // A long tail cannot move the result, but it can break a tie, and the
        // parser must not lose track of it once its digit buffer fills.
        let mut s = String::from("9007199254740992.");
        s.push_str(&"0".repeat(900));
        assert_eq!(parse(&s), 9007199254740992.0);
        let mut s = String::from("9007199254740992.");
        s.push_str(&"0".repeat(900));
        s.push('1');
        assert_eq!(parse(&s), 9007199254740992.0);
        // 1 followed by 800 zeros, then scaled back: still exactly 1.
        let mut s = String::from("1");
        s.push_str(&"0".repeat(800));
        s.push_str("e-800");
        assert_eq!(parse(&s), 1.0);
    }

    #[test]
    fn strtod_sets_erange_only_when_out_of_range() {
        crate::errno::set_errno(0);
        assert_eq!(parse("0"), 0.0);
        assert_eq!(crate::errno::get_errno(), 0, "plain zero is not ERANGE");

        crate::errno::set_errno(0);
        assert!(parse("1e400").is_infinite());
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);

        crate::errno::set_errno(0);
        assert_eq!(parse("1e-400"), 0.0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);

        // Gradual underflow: nonzero but subnormal, which glibc flags too.
        crate::errno::set_errno(0);
        assert_ne!(parse("1e-320"), 0.0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);
    }

    #[test]
    fn strtod_matches_rusts_parser_over_a_sweep() {
        // Pseudo-random bit patterns, each fed back in three textual forms:
        // shortest round-trip, a 30-digit expansion, and the Debug form.
        let mut st: u64 = 0x2545_F491_4F6C_DD1D;
        for _ in 0..4000 {
            st ^= st << 13;
            st ^= st >> 7;
            st ^= st << 17;
            let v = f64::from_bits(st);
            if !v.is_finite() {
                continue;
            }
            let v = v.abs();
            for text in [format!("{v:e}"), format!("{v:.30e}"), format!("{v:?}")] {
                let want = text.parse::<f64>().unwrap();
                assert_eq!(parse(&text), want, "strtod({text})");
            }
        }
    }

    #[test]
    fn strtof_rounds_once_not_twice() {
        // strtof used to be strtod plus a cast, which rounds twice.  This
        // input is a hair above the midpoint between 1.0f and its successor,
        // but rounds to exactly that midpoint in f64, where ties-to-even then
        // sends it back down.
        let text = "1.000000059604644830901776231257827021181583404541015625";
        assert_eq!(
            parse(text) as f32,
            1.0_f32,
            "the trap this test guards against"
        );
        let mut z = text.as_bytes().to_vec();
        z.push(0);
        let got = unsafe { strtof(z.as_ptr(), core::ptr::null_mut()) };
        assert_eq!(got.to_bits(), 1.0_f32.to_bits() + 1);
    }

    #[test]
    fn strtof_uses_the_f32_range_for_erange() {
        fn parse32(text: &str) -> f32 {
            let mut z = text.as_bytes().to_vec();
            z.push(0);
            unsafe { strtof(z.as_ptr(), core::ptr::null_mut()) }
        }
        crate::errno::set_errno(0);
        assert_eq!(parse32("3.4028235e38"), f32::MAX);
        assert_eq!(crate::errno::get_errno(), 0);

        crate::errno::set_errno(0);
        assert!(parse32("1e39").is_infinite());
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);

        crate::errno::set_errno(0);
        assert_eq!(parse32("1e-60"), 0.0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ERANGE);

        // Named values still work through the shared scanner.
        assert!(parse32("-INFINITY").is_infinite() && parse32("-INFINITY") < 0.0);
        assert!(parse32("nan(x)").is_nan());
    }

    #[test]
    fn strtof_matches_rusts_parser_over_a_sweep() {
        let mut st: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..4000 {
            st ^= st << 13;
            st ^= st >> 7;
            st ^= st << 17;
            let v = f32::from_bits((st >> 32) as u32);
            if !v.is_finite() {
                continue;
            }
            let v = v.abs();
            for text in [format!("{v:e}"), format!("{v:.25e}"), format!("{v:?}")] {
                let mut z = text.as_bytes().to_vec();
                z.push(0);
                let got = unsafe { strtof(z.as_ptr(), core::ptr::null_mut()) };
                assert_eq!(got, text.parse::<f32>().unwrap(), "strtof({text})");
            }
        }
    }

    // -- A NULL comparison function (design-decisions.md §1115) --

    /// Fewer than two elements are sorted without a comparison, so they take
    /// a NULL `compar`, as in glibc; so does an empty `bsearch`.  (A NULL one
    /// with work to do ends the process, which is not a test's to take.)
    #[test]
    fn a_null_compar_is_harmless_with_nothing_to_compare() {
        let mut one = [9i32];
        // SAFETY: a one-element array; nothing is compared.
        unsafe {
            qsort(one.as_mut_ptr().cast(), 1, 4, None);
            qsort(core::ptr::null_mut(), 0, 4, None);
            qsort_r(one.as_mut_ptr().cast(), 1, 4, None, core::ptr::null_mut());
            assert!(bsearch(one.as_ptr().cast(), one.as_ptr().cast(), 0, 4, None).is_null());
        }
        assert_eq!(one, [9]);
    }

    /// glibc's bsearch computes every element from `base`, so a size of 0
    /// compares `base` itself -- it was "not found" without a comparison.
    #[test]
    fn bsearch_of_size_zero_compares_the_base() {
        extern "C" fn always_equal(_: *const u8, _: *const u8) -> i32 {
            0
        }
        let base = 64 as *const u8;
        // SAFETY: `always_equal` reads neither pointer.
        let got = unsafe { bsearch(core::ptr::null(), base, 3, 0, Some(always_equal)) };
        assert_eq!(got.cast_const(), base);
    }

    /// The `_l` conversions and the BSD `q` names answer as the functions
    /// they stand for, end pointer included.
    #[test]
    fn the_locale_and_q_forms_are_their_functions() {
        let s = b"  -0x1Fg\0".as_ptr();
        let (mut a, mut b): (*const u8, *const u8) = (core::ptr::null(), core::ptr::null());
        // SAFETY: a NUL-terminated string; the end pointers are locals.
        unsafe {
            assert_eq!(strtol_l(s, &raw mut a, 16, 0), strtol(s, &raw mut b, 16));
            assert_eq!(a, b);
            assert_eq!(strtoll_l(s, core::ptr::null_mut(), 0, 0), -31);
            assert_eq!(strtoq(s, core::ptr::null_mut(), 0), -31);
            assert_eq!(
                strtoul_l(s, core::ptr::null_mut(), 16, 0),
                strtoul(s, core::ptr::null_mut(), 16)
            );
            assert_eq!(
                strtoull_l(s, core::ptr::null_mut(), 16, 0),
                strtouq(s, core::ptr::null_mut(), 16)
            );
        }
    }
}
