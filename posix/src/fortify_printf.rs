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
//! assembly trampolines that flatten the register/stack varargs into a single
//! 16-slot `u64` array (8 integer slots followed by 8 float slots) and call a
//! Rust `_*_chk_impl`, which splits the halves and delegates to the matching
//! tested `crate::printf::_*_impl`.  The `__v*_chk` forms take a real
//! `va_list` (a pointer on x86_64 System V), so they are plain Rust and
//! host-testable.

use crate::printf::{self, VaList};

// ---------------------------------------------------------------------------
// Assembly trampolines — flatten varargs into a combined 16-slot [u64] array.
//
// Fixed-argument counts (which determine where the varargs start):
//   __printf_chk   (flag, fmt, …)            : 2 fixed
//   __fprintf_chk  (fp, flag, fmt, …)         : 3 fixed
//   __dprintf_chk  (fd, flag, fmt, …)         : 3 fixed
//   __asprintf_chk (&p, flag, fmt, …)         : 3 fixed
//   __sprintf_chk  (s, flag, slen, fmt, …)    : 4 fixed
//   __snprintf_chk (s, n, flag, slen, fmt, …) : 5 fixed
//
// The slot array lives at [rsp..rsp+128]; the Rust impl receives a single
// pointer to it (passed in the first free integer register) and splits the
// int/float halves internally.
// ---------------------------------------------------------------------------

#[cfg(target_os = "none")]
core::arch::global_asm!(
    // __printf_chk(flag, fmt, ...) → _printf_chk_impl(flag, fmt, args)
    // Fixed: rdi=flag, rsi=fmt.  Varargs: rdx,rcx,r8,r9,stack.
    ".global __printf_chk",
    ".type __printf_chk, @function",
    "__printf_chk:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rdx",        // int vararg 0
    "mov [rsp+8], rcx",      // int vararg 1
    "mov [rsp+16], r8",      // int vararg 2
    "mov [rsp+24], r9",      // int vararg 3
    "mov rax, [rbp+16]",     // int vararg 4 (stack)
    "mov [rsp+32], rax",
    "mov rax, [rbp+24]",     // int vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+32]",     // int vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+40]",     // int vararg 7
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    // rdi=flag, rsi=fmt already set.
    "mov rdx, rsp",          // args pointer
    "call _printf_chk_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",

    // __fprintf_chk(fp, flag, fmt, ...) → _fprintf_chk_impl(fp, flag, fmt, args)
    // Fixed: rdi=fp, rsi=flag, rdx=fmt.  Varargs: rcx,r8,r9,stack.
    ".global __fprintf_chk",
    ".type __fprintf_chk, @function",
    "__fprintf_chk:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rcx",        // int vararg 0
    "mov [rsp+8], r8",       // int vararg 1
    "mov [rsp+16], r9",      // int vararg 2
    "mov rax, [rbp+16]",     // int vararg 3 (stack)
    "mov [rsp+24], rax",
    "mov rax, [rbp+24]",     // int vararg 4
    "mov [rsp+32], rax",
    "mov rax, [rbp+32]",     // int vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+40]",     // int vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+48]",     // int vararg 7
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    // rdi=fp, rsi=flag, rdx=fmt already set.
    "mov rcx, rsp",          // args pointer
    "call _fprintf_chk_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",

    // __dprintf_chk(fd, flag, fmt, ...) → _dprintf_chk_impl(fd, flag, fmt, args)
    // Same fixed/vararg layout as __fprintf_chk.
    ".global __dprintf_chk",
    ".type __dprintf_chk, @function",
    "__dprintf_chk:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rcx",
    "mov [rsp+8], r8",
    "mov [rsp+16], r9",
    "mov rax, [rbp+16]",
    "mov [rsp+24], rax",
    "mov rax, [rbp+24]",
    "mov [rsp+32], rax",
    "mov rax, [rbp+32]",
    "mov [rsp+40], rax",
    "mov rax, [rbp+40]",
    "mov [rsp+48], rax",
    "mov rax, [rbp+48]",
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    "mov rcx, rsp",
    "call _dprintf_chk_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",

    // __asprintf_chk(&p, flag, fmt, ...) → _asprintf_chk_impl(&p, flag, fmt, args)
    // Same fixed/vararg layout as __fprintf_chk.
    ".global __asprintf_chk",
    ".type __asprintf_chk, @function",
    "__asprintf_chk:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rcx",
    "mov [rsp+8], r8",
    "mov [rsp+16], r9",
    "mov rax, [rbp+16]",
    "mov [rsp+24], rax",
    "mov rax, [rbp+24]",
    "mov [rsp+32], rax",
    "mov rax, [rbp+32]",
    "mov [rsp+40], rax",
    "mov rax, [rbp+40]",
    "mov [rsp+48], rax",
    "mov rax, [rbp+48]",
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    "mov rcx, rsp",
    "call _asprintf_chk_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",

    // __sprintf_chk(s, flag, slen, fmt, ...) → _sprintf_chk_impl(s, flag, slen, fmt, args)
    // Fixed: rdi=s, rsi=flag, rdx=slen, rcx=fmt.  Varargs: r8,r9,stack.
    ".global __sprintf_chk",
    ".type __sprintf_chk, @function",
    "__sprintf_chk:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], r8",         // int vararg 0
    "mov [rsp+8], r9",       // int vararg 1
    "mov rax, [rbp+16]",     // int vararg 2 (stack)
    "mov [rsp+16], rax",
    "mov rax, [rbp+24]",     // int vararg 3
    "mov [rsp+24], rax",
    "mov rax, [rbp+32]",     // int vararg 4
    "mov [rsp+32], rax",
    "mov rax, [rbp+40]",     // int vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+48]",     // int vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+56]",     // int vararg 7
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    // rdi=s, rsi=flag, rdx=slen, rcx=fmt already set.
    "mov r8, rsp",           // args pointer
    "call _sprintf_chk_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",

    // __snprintf_chk(s, maxlen, flag, slen, fmt, ...)
    //   → _snprintf_chk_impl(s, maxlen, flag, slen, fmt, args)
    // Fixed: rdi=s, rsi=maxlen, rdx=flag, rcx=slen, r8=fmt.  Varargs: r9,stack.
    ".global __snprintf_chk",
    ".type __snprintf_chk, @function",
    "__snprintf_chk:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], r9",         // int vararg 0
    "mov rax, [rbp+16]",     // int vararg 1 (stack)
    "mov [rsp+8], rax",
    "mov rax, [rbp+24]",     // int vararg 2
    "mov [rsp+16], rax",
    "mov rax, [rbp+32]",     // int vararg 3
    "mov [rsp+24], rax",
    "mov rax, [rbp+40]",     // int vararg 4
    "mov [rsp+32], rax",
    "mov rax, [rbp+48]",     // int vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+56]",     // int vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+64]",     // int vararg 7
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    // rdi=s, rsi=maxlen, rdx=flag, rcx=slen, r8=fmt already set.
    "mov r9, rsp",           // args pointer
    "call _snprintf_chk_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",
);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Split the trampoline's combined 16-slot array into integer/float halves.
fn split_args(args: *const u64) -> (*const u64, *const u64) {
    if args.is_null() {
        (core::ptr::null(), core::ptr::null())
    } else {
        // SAFETY: the trampoline always provides 16 contiguous slots, so
        // `args + 8` is in-bounds.  Null is handled above.
        (args, unsafe { args.add(8) })
    }
}

// ---------------------------------------------------------------------------
// Rust entry points (called by the assembly trampolines)
// ---------------------------------------------------------------------------

/// Backing implementation for `__printf_chk`.
///
/// # Safety
/// `fmt` must be a valid NUL-terminated C string (or null); `args` must point
/// to at least 16 valid `u64` slots (the trampoline always provides them).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _printf_chk_impl(_flag: i32, fmt: *const u8, args: *const u64) -> i32 {
    let (iargs, fargs) = split_args(args);
    printf::_printf_impl(fmt, iargs, fargs)
}

/// Backing implementation for `__fprintf_chk`.
///
/// # Safety
/// As [`_printf_chk_impl`]; `stream` is an opaque `FILE*` handled by
/// `_fprintf_impl`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _fprintf_chk_impl(
    stream: *mut u8,
    _flag: i32,
    fmt: *const u8,
    args: *const u64,
) -> i32 {
    let (iargs, fargs) = split_args(args);
    printf::_fprintf_impl(stream, fmt, iargs, fargs)
}

/// Backing implementation for `__dprintf_chk`.
///
/// # Safety
/// As [`_printf_chk_impl`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _dprintf_chk_impl(
    fd: i32,
    _flag: i32,
    fmt: *const u8,
    args: *const u64,
) -> i32 {
    let (iargs, fargs) = split_args(args);
    printf::_dprintf_impl(fd, fmt, iargs, fargs)
}

/// Backing implementation for `__asprintf_chk`.
///
/// # Safety
/// As [`_printf_chk_impl`]; `strp` must be a valid `char**` (or null, which
/// `_asprintf_impl` rejects).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _asprintf_chk_impl(
    strp: *mut *mut u8,
    _flag: i32,
    fmt: *const u8,
    args: *const u64,
) -> i32 {
    let (iargs, fargs) = split_args(args);
    printf::_asprintf_impl(strp, fmt, iargs, fargs)
}

/// Backing implementation for `__sprintf_chk`.
///
/// Bounds the output to `slen` (the destination object size), behaving like
/// `snprintf(s, slen, fmt, …)`.  Returns the would-be length.
///
/// # Safety
/// As [`_printf_chk_impl`]; `s` must point to at least `slen` writable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _sprintf_chk_impl(
    s: *mut u8,
    _flag: i32,
    slen: usize,
    fmt: *const u8,
    args: *const u64,
) -> i32 {
    let (iargs, fargs) = split_args(args);
    printf::_snprintf_impl(s, slen, fmt, iargs, fargs)
}

/// Backing implementation for `__snprintf_chk`.
///
/// Uses `min(maxlen, slen)` as the bound so the wrapper never writes past the
/// destination object even if the caller passed a `maxlen` larger than the
/// buffer.  Returns the would-be length.
///
/// # Safety
/// As [`_printf_chk_impl`]; `s` must point to at least `min(maxlen, slen)`
/// writable bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _snprintf_chk_impl(
    s: *mut u8,
    maxlen: usize,
    _flag: i32,
    slen: usize,
    fmt: *const u8,
    args: *const u64,
) -> i32 {
    let (iargs, fargs) = split_args(args);
    let bound = maxlen.min(slen);
    printf::_snprintf_impl(s, bound, fmt, iargs, fargs)
}

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
    if ap.is_null() {
        return printf::_printf_impl(fmt, core::ptr::null(), core::ptr::null());
    }
    // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
    let (iargs, fargs) = unsafe { printf::va_collect(fmt, &mut *ap) };
    printf::_printf_impl(fmt, iargs.as_ptr(), fargs.as_ptr())
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
    if ap.is_null() {
        return printf::_fprintf_impl(stream, fmt, core::ptr::null(), core::ptr::null());
    }
    // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
    let (iargs, fargs) = unsafe { printf::va_collect(fmt, &mut *ap) };
    printf::_fprintf_impl(stream, fmt, iargs.as_ptr(), fargs.as_ptr())
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
    if ap.is_null() {
        return printf::_dprintf_impl(fd, fmt, core::ptr::null(), core::ptr::null());
    }
    // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
    let (iargs, fargs) = unsafe { printf::va_collect(fmt, &mut *ap) };
    printf::_dprintf_impl(fd, fmt, iargs.as_ptr(), fargs.as_ptr())
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
    if ap.is_null() {
        return printf::_asprintf_impl(strp, fmt, core::ptr::null(), core::ptr::null());
    }
    // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
    let (iargs, fargs) = unsafe { printf::va_collect(fmt, &mut *ap) };
    printf::_asprintf_impl(strp, fmt, iargs.as_ptr(), fargs.as_ptr())
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
    if ap.is_null() {
        return printf::_snprintf_impl(s, slen, fmt, core::ptr::null(), core::ptr::null());
    }
    // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
    let (iargs, fargs) = unsafe { printf::va_collect(fmt, &mut *ap) };
    printf::_snprintf_impl(s, slen, fmt, iargs.as_ptr(), fargs.as_ptr())
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
    if ap.is_null() {
        return printf::_snprintf_impl(s, bound, fmt, core::ptr::null(), core::ptr::null());
    }
    // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
    let (iargs, fargs) = unsafe { printf::va_collect(fmt, &mut *ap) };
    printf::_snprintf_impl(s, bound, fmt, iargs.as_ptr(), fargs.as_ptr())
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
        let mut slots = [0u64; 16];
        slots[0] = 42;
        // _sprintf_chk_impl(s, flag, slen, fmt, args)
        let n = unsafe {
            _sprintf_chk_impl(buf.as_mut_ptr(), 1, buf.len(), b"n=%d\0".as_ptr(), slots.as_ptr())
        };
        assert_eq!(cstr(&buf), b"n=42");
        assert_eq!(n, 4);
    }

    #[test]
    fn sprintf_chk_string_arg() {
        let mut buf = [0u8; 64];
        let s = b"world\0";
        let mut slots = [0u64; 16];
        slots[0] = s.as_ptr() as u64;
        let n = unsafe {
            _sprintf_chk_impl(buf.as_mut_ptr(), 1, buf.len(), b"hi %s\0".as_ptr(), slots.as_ptr())
        };
        assert_eq!(cstr(&buf), b"hi world");
        assert_eq!(n, 8);
    }

    #[test]
    fn snprintf_chk_uses_min_bound() {
        // maxlen is large but slen (object size) is tiny: must clamp to slen.
        let mut buf = [0u8; 64];
        let mut slots = [0u64; 16];
        slots[0] = 123456;
        // bound = min(maxlen=64, slen=4) = 4 → "12" + space for NUL.
        let n = unsafe {
            _snprintf_chk_impl(buf.as_mut_ptr(), 64, 1, 4, b"%d\0".as_ptr(), slots.as_ptr())
        };
        // snprintf writes at most bound-1 = 3 chars + NUL: "123".
        assert_eq!(cstr(&buf), b"123");
        // Return value is the would-be length (6 for "123456").
        assert_eq!(n, 6);
    }

    #[test]
    fn snprintf_chk_no_truncation_when_fits() {
        let mut buf = [0u8; 64];
        let mut slots = [0u64; 16];
        slots[0] = 7;
        let n = unsafe {
            _snprintf_chk_impl(buf.as_mut_ptr(), 64, 1, 64, b"x=%d\0".as_ptr(), slots.as_ptr())
        };
        assert_eq!(cstr(&buf), b"x=7");
        assert_eq!(n, 3);
    }

    #[test]
    fn vsprintf_chk_expands_via_valist() {
        let mut buf = [0u8; 64];
        with_valist(&[99], |va| {
            let n = unsafe {
                __vsprintf_chk(buf.as_mut_ptr(), 1, buf.len(), b"v=%d\0".as_ptr(), va)
            };
            assert_eq!(n, 4);
        });
        assert_eq!(cstr(&buf), b"v=99");
    }

    #[test]
    fn vsnprintf_chk_clamps_to_min_bound() {
        let mut buf = [0u8; 64];
        with_valist(&[123456], |va| {
            let n = unsafe {
                __vsnprintf_chk(buf.as_mut_ptr(), 64, 1, 4, b"%d\0".as_ptr(), va)
            };
            assert_eq!(n, 6);
        });
        assert_eq!(cstr(&buf), b"123");
    }

    #[test]
    fn vsprintf_chk_null_va_no_args() {
        let mut buf = [0u8; 64];
        // No conversions, so a null va_list is fine.
        let n = unsafe {
            __vsprintf_chk(buf.as_mut_ptr(), 1, buf.len(), b"literal\0".as_ptr(), core::ptr::null_mut())
        };
        assert_eq!(cstr(&buf), b"literal");
        assert_eq!(n, 7);
    }

    #[test]
    fn printf_chk_impl_returns_length() {
        // _printf_impl writes to stdout (fd 1); just verify the return value
        // and that it doesn't crash.  Format expansion is tested above.
        let n = unsafe {
            _printf_chk_impl(1, b"\0".as_ptr(), core::ptr::null())
        };
        assert_eq!(n, 0);
    }

    #[test]
    fn asprintf_chk_null_strp_returns_error() {
        // The host test allocator is inert, so we can't assert a successful
        // allocation here; instead verify the wrapper forwards to
        // `_asprintf_impl`, which rejects a null `strp` with -1.  (Successful
        // asprintf allocation is exercised on the bare-metal target.)
        let n = unsafe {
            _asprintf_chk_impl(core::ptr::null_mut(), 1, b"x\0".as_ptr(), core::ptr::null())
        };
        assert_eq!(n, -1);
    }
}
