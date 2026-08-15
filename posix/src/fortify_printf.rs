//! `_FORTIFY_SOURCE` printf-family wrappers (`__*_chk`).
//!
//! When a program is compiled with `_FORTIFY_SOURCE` (the default at `-O1`
//! and above on most glibc-based distributions), the libc headers rewrite
//! calls to the printf family into the fortified `__*_chk` entry points, e.g.
//!
//! ```c
//! printf(fmt, ...)            → __printf_chk(flag, fmt, ...)
//! fprintf(fp, fmt, ...)       → __fprintf_chk(fp, flag, fmt, ...)
//! dprintf(fd, fmt, ...)       → __dprintf_chk(fd, flag, fmt, ...)
//! sprintf(s, fmt, ...)        → __sprintf_chk(s, flag, slen, fmt, ...)
//! snprintf(s, n, fmt, ...)    → __snprintf_chk(s, n, flag, slen, fmt, ...)
//! asprintf(&p, fmt, ...)      → __asprintf_chk(&p, flag, fmt, ...)
//! ```
//! plus the `__v*_chk` forms for the `va_list` variants.  An object file
//! compiled this way references the `__*_chk` symbols, so a libc that omits
//! them cannot link those programs.
//!
//! ## Semantics
//!
//! - `flag` is the fortify level; we accept and ignore it (the bounds we
//!   enforce below already provide the safety it requests).
//! - `slen` is the compiler's `__builtin_object_size` of the destination
//!   buffer.  For the buffer-writing wrappers we treat it as a hard bound:
//!   `__sprintf_chk` behaves like `snprintf(s, slen, …)` and `__snprintf_chk`
//!   uses `min(maxlen, slen)`.  glibc instead calls `__chk_fail()` (abort) on
//!   overflow; truncating is a safe deviation — it never writes out of
//!   bounds, and the return value is still the would-be length, so callers
//!   that check it detect the truncation.  When `slen` is unknown the
//!   compiler passes `(size_t)-1`, which leaves the wrapper effectively
//!   unbounded, exactly like the un-fortified call.
//!
//! ## Architecture
//!
//! Identical to [`crate::printf`]: the variadic `__*_chk` entry points are
//! assembly trampolines that perform a real `va_start` — spilling the argument
//! registers into a System V register save area and building a `va_list` over
//! it — and then call the matching `__v*_chk`.  Those are plain Rust, so the
//! fortify-specific bounding (`slen`, `min(maxlen, slen)`) and all of the
//! formatting are host-testable, and there is exactly one argument-delivery
//! path into the engine.
//!
//! Delegating rather than flattening the varargs into fixed integer/float
//! arrays is what makes `%Lf` reachable here: a `long double` is X87/X87UP and
//! therefore MEMORY-class, so it is passed only in the overflow area and no
//! register-array representation can carry it.  See BUG-POSIX-LONG-DOUBLE-ABI
//! in `known-issues.md`.

// The `_*_impl` symbols this module calls in `crate::printf` are deliberately
// underscore-prefixed: they are "private-but-exported" linkage symbols rather
// than ordinary public API.
#![allow(clippy::used_underscore_items)]

use crate::printf::{self, VaList};

// ---------------------------------------------------------------------------
// Assembly trampolines — perform a real `va_start` and tail into `__v*_chk`.
//
// Each entry point spills the six integer and eight SSE argument registers
// into a System V register save area, builds a `va_list` describing it, and
// calls the matching `__v*_chk`.  That keeps a single argument-delivery path
// through the formatting engine: a `long double` is MEMORY-class and lives
// only in the overflow area, which no flattened register array could ever
// represent.
//
// The macro arguments are (symbol, target, initial `gp_offset`, register that
// carries the `va_list*`).  `gp_offset` is 8 × the number of *named integer*
// arguments, which is also what fixes the register the `va_list*` goes in:
//
//   __printf_chk   (flag, fmt, …)             : 2 fixed → gp 16, ap in rdx
//   __fprintf_chk  (fp, flag, fmt, …)         : 3 fixed → gp 24, ap in rcx
//   __dprintf_chk  (fd, flag, fmt, …)         : 3 fixed → gp 24, ap in rcx
//   __asprintf_chk (&p, flag, fmt, …)         : 3 fixed → gp 24, ap in rcx
//   __sprintf_chk  (s, flag, slen, fmt, …)    : 4 fixed → gp 32, ap in r8
//   __snprintf_chk (s, n, flag, slen, fmt, …) : 5 fixed → gp 40, ap in r9
//
// The `slen`/`maxlen` bounding that distinguishes the fortified wrappers from
// the plain ones lives in the `__v*_chk` functions below, so it is applied
// exactly once and is host-testable.
// ---------------------------------------------------------------------------

#[cfg(target_os = "none")]
use crate::printf::va_trampoline;

#[cfg(target_os = "none")]
va_trampoline!("__printf_chk", "__vprintf_chk", "16", "rdx");
#[cfg(target_os = "none")]
va_trampoline!("__fprintf_chk", "__vfprintf_chk", "24", "rcx");
#[cfg(target_os = "none")]
va_trampoline!("__dprintf_chk", "__vdprintf_chk", "24", "rcx");
#[cfg(target_os = "none")]
va_trampoline!("__asprintf_chk", "__vasprintf_chk", "24", "rcx");
#[cfg(target_os = "none")]
va_trampoline!("__sprintf_chk", "__vsprintf_chk", "32", "r8");
#[cfg(target_os = "none")]
va_trampoline!("__snprintf_chk", "__vsnprintf_chk", "40", "r9");

// ---------------------------------------------------------------------------
// __v*_chk variants — take a `va_list` (pointer); pure Rust, host-testable.
// ---------------------------------------------------------------------------

/// `__vprintf_chk(flag, fmt, ap)`.
///
/// # Safety
/// `fmt` must be a valid format string (or null) and `ap` a valid `va_list`
/// matching it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __vprintf_chk(_flag: i32, fmt: *const u8, ap: *mut VaList) -> i32 {
    // SAFETY: the caller guarantees `ap` is a valid va_list matching `fmt`;
    // a null one is rendered as zero arguments rather than a fault.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    printf::_printf_impl(fmt, &mut args)
}

/// `__vfprintf_chk(fp, flag, fmt, ap)`.
///
/// # Safety
/// As [`__vprintf_chk`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __vfprintf_chk(
    stream: *mut u8,
    _flag: i32,
    fmt: *const u8,
    ap: *mut VaList,
) -> i32 {
    // SAFETY: as for `__vprintf_chk`.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    printf::_fprintf_impl(stream, fmt, &mut args)
}

/// `__vdprintf_chk(fd, flag, fmt, ap)`.
///
/// # Safety
/// As [`__vprintf_chk`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __vdprintf_chk(
    fd: i32,
    _flag: i32,
    fmt: *const u8,
    ap: *mut VaList,
) -> i32 {
    // SAFETY: as for `__vprintf_chk`.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    printf::_dprintf_impl(fd, fmt, &mut args)
}

/// `__vasprintf_chk(&p, flag, fmt, ap)`.
///
/// # Safety
/// As [`__vprintf_chk`]; `strp` must be a valid `char**` (or null).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __vasprintf_chk(
    strp: *mut *mut u8,
    _flag: i32,
    fmt: *const u8,
    ap: *mut VaList,
) -> i32 {
    // SAFETY: as for `__vprintf_chk`; `asprintf` walks the format string
    // twice, so it takes a snapshot by value and replays it (C's `va_copy`).
    let snapshot = if ap.is_null() {
        None
    } else {
        Some(unsafe { *ap })
    };
    unsafe { printf::_asprintf_impl(strp, fmt, snapshot) }
}

/// `__vsprintf_chk(s, flag, slen, fmt, ap)`.
///
/// # Safety
/// As [`__vprintf_chk`]; `s` must point to at least `slen` writable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __vsprintf_chk(
    s: *mut u8,
    _flag: i32,
    slen: usize,
    fmt: *const u8,
    ap: *mut VaList,
) -> i32 {
    // SAFETY: as for `__vprintf_chk`.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    printf::_snprintf_impl(s, slen, fmt, &mut args)
}

/// `__vsnprintf_chk(s, maxlen, flag, slen, fmt, ap)`.
///
/// # Safety
/// As [`__vprintf_chk`]; `s` must point to at least `min(maxlen, slen)`
/// writable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __vsnprintf_chk(
    s: *mut u8,
    maxlen: usize,
    _flag: i32,
    slen: usize,
    fmt: *const u8,
    ap: *mut VaList,
) -> i32 {
    let bound = maxlen.min(slen);
    // SAFETY: as for `__vprintf_chk`.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    printf::_snprintf_impl(s, bound, fmt, &mut args)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic SysV `va_list` with up to 6 integer args.
    fn with_valist<R>(ints: &[u64], f: impl FnOnce(*mut VaList) -> R) -> R {
        let mut reg = [0u8; 176];
        for (i, &v) in ints.iter().enumerate().take(6) {
            let off = i * 8;
            reg[off..off + 8].copy_from_slice(&v.to_le_bytes());
        }
        let mut overflow = [0u8; 64];
        let mut va = VaList {
            gp_offset: 0,
            fp_offset: 48,
            overflow_arg_area: overflow.as_mut_ptr(),
            reg_save_area: reg.as_mut_ptr(),
        };
        f(&mut va)
    }

    /// Read a C string up to its NUL into a Rust slice (for assertions).
    fn cstr(buf: &[u8]) -> &[u8] {
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        &buf[..end]
    }

    #[test]
    fn sprintf_chk_expands_format() {
        let mut buf = [0u8; 64];
        let n = with_valist(&[42], |va| unsafe {
            __vsprintf_chk(buf.as_mut_ptr(), 1, 64, b"n=%d\0".as_ptr(), va)
        });
        assert_eq!(cstr(&buf), b"n=42");
        assert_eq!(n, 4);
    }

    #[test]
    fn sprintf_chk_string_arg() {
        let mut buf = [0u8; 64];
        let s = b"world\0";
        let n = with_valist(&[s.as_ptr() as u64], |va| unsafe {
            __vsprintf_chk(buf.as_mut_ptr(), 1, 64, b"hi %s\0".as_ptr(), va)
        });
        assert_eq!(cstr(&buf), b"hi world");
        assert_eq!(n, 8);
    }

    #[test]
    fn snprintf_chk_no_truncation_when_fits() {
        let mut buf = [0u8; 64];
        let n = with_valist(&[7], |va| unsafe {
            __vsnprintf_chk(buf.as_mut_ptr(), 64, 1, 64, b"x=%d\0".as_ptr(), va)
        });
        assert_eq!(cstr(&buf), b"x=7");
        assert_eq!(n, 3);
    }

    #[test]
    fn vsprintf_chk_expands_via_valist() {
        let mut buf = [0u8; 64];
        with_valist(&[99], |va| {
            let n =
                unsafe { __vsprintf_chk(buf.as_mut_ptr(), 1, buf.len(), b"v=%d\0".as_ptr(), va) };
            assert_eq!(n, 4);
        });
        assert_eq!(cstr(&buf), b"v=99");
    }

    #[test]
    fn vsnprintf_chk_clamps_to_min_bound() {
        let mut buf = [0u8; 64];
        with_valist(&[123456], |va| {
            let n = unsafe { __vsnprintf_chk(buf.as_mut_ptr(), 64, 1, 4, b"%d\0".as_ptr(), va) };
            assert_eq!(n, 6);
        });
        assert_eq!(cstr(&buf), b"123");
    }

    #[test]
    fn vsprintf_chk_null_va_no_args() {
        let mut buf = [0u8; 64];
        // No conversions, so a null va_list is fine.
        let n = unsafe {
            __vsprintf_chk(
                buf.as_mut_ptr(),
                1,
                buf.len(),
                b"literal\0".as_ptr(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(cstr(&buf), b"literal");
        assert_eq!(n, 7);
    }

    #[test]
    fn vprintf_chk_returns_length() {
        // _printf_impl writes to stdout (fd 1); just verify the return value
        // and that it doesn't crash.  Format expansion is tested above.
        let n = unsafe { __vprintf_chk(1, b"\0".as_ptr(), core::ptr::null_mut()) };
        assert_eq!(n, 0);
    }

    #[test]
    fn vasprintf_chk_null_strp_returns_error() {
        // The host test allocator is inert, so we can't assert a successful
        // allocation here; instead verify the wrapper forwards to
        // `_asprintf_impl`, which rejects a null `strp` with -1.  (Successful
        // asprintf allocation is exercised on the bare-metal target.)
        let n = unsafe {
            __vasprintf_chk(
                core::ptr::null_mut(),
                1,
                b"x\0".as_ptr(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(n, -1);
    }
}
