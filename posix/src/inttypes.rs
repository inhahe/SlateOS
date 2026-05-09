//! `<inttypes.h>` — integer type conversion functions.
//!
//! On x86_64, `intmax_t` is `int64_t` (= `i64`) and `uintmax_t` is
//! `uint64_t` (= `u64`).  `strtoimax` and `strtoumax` therefore
//! delegate directly to `strtoll`/`strtoull`.
//!
//! Also provides `imaxabs` and `imaxdiv`.

/// Absolute value of an `intmax_t`.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strtoumax(
    nptr: *const u8,
    endptr: *mut *const u8,
    base: i32,
) -> u64 {
    unsafe { crate::stdlib::strtoull(nptr, endptr, base) }
}

/// Convert a `wchar_t` string to `intmax_t`.
///
/// Stub: returns 0 (wide string parsing not implemented).
#[unsafe(no_mangle)]
pub extern "C" fn wcstoimax(
    _nptr: *const i32,
    _endptr: *mut *const i32,
    _base: i32,
) -> i64 {
    0
}

/// Convert a `wchar_t` string to `uintmax_t`.
///
/// Stub: returns 0 (wide string parsing not implemented).
#[unsafe(no_mangle)]
pub extern "C" fn wcstoumax(
    _nptr: *const i32,
    _endptr: *mut *const i32,
    _base: i32,
) -> u64 {
    0
}
