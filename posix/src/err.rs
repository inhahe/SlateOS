//! BSD error/warning functions (`<err.h>`).
//!
//! Provides `err`, `errx`, `warn`, `warnx`, `verr`, `verrx`, `vwarn`,
//! `vwarnx` for formatted error messages to stderr.  These are not
//! strictly POSIX but are very widely used by Unix utilities (BSD,
//! macOS, and glibc all provide them).
//!
//! ## Behavior
//!
//! - `warn`/`vwarn`: prints `progname: fmt-args: strerror(errno)\n`
//! - `warnx`/`vwarnx`: prints `progname: fmt-args\n` (no errno)
//! - `err`/`verr`: like `warn` + `exit(eval)`
//! - `errx`/`verrx`: like `warnx` + `exit(eval)`
//!
//! ## Implementation
//!
//! The C prototypes are variadic, e.g. `void err(int, const char *fmt, ...)`.
//! Like [`crate::printf`], the variadic entry points are assembly trampolines
//! that perform a real `va_start` — spilling the argument registers into a
//! System V register save area and building a `va_list` over it — and then
//! call the matching `v*` variant.  Those variants are plain Rust and take
//! the `va_list` directly, so they are host-testable and there is exactly one
//! argument-delivery path.  Both funnel through [`emit`], which expands the
//! format string with the tested `snprintf` engine before adding the
//! `progname:` prefix and optional `: strerror(errno)` suffix.
//!
//! Streaming the arguments straight out of the `va_list` rather than
//! flattening them into fixed arrays is what lifts the old eight-integer /
//! eight-float ceiling and makes `%Lf` reachable here: a `long double` is
//! X87/X87UP and therefore MEMORY-class, so it is passed only in the overflow
//! area and no register-array representation can carry it.  See
//! BUG-POSIX-PRINTF-ARG-ARRAY-OOB and BUG-POSIX-LONG-DOUBLE-ABI in
//! `known-issues.md`.
//!
//! Earlier this layer printed the format string *literally* (it declared the
//! functions as non-variadic and dropped the arguments), so `err(1, "open
//! %s", path)` produced `open %s: ...` instead of expanding `%s`.  That is
//! now fixed.

// Calls `printf::_snprintf_impl` (an underscore-prefixed ABI trampoline
// target).  The underscore is part of the printf-impl naming convention,
// not a "private" marker — see `crate::printf` for details.
#![allow(clippy::used_underscore_items)]

use crate::errno;
use crate::printf::{self, VaList};

#[cfg(target_os = "none")]
use crate::printf::va_trampoline;

// ---------------------------------------------------------------------------
// Assembly trampolines — `va_start`, then call the matching `v*` variant.
//
// The named-argument counts decide the initial `gp_offset` and which register
// carries the `va_list*`:
//   warn/warnx  : 1 named arg (fmt)        — same shape as `printf`
//   err/errx    : 2 named args (eval, fmt) — same shape as `fprintf`
// ---------------------------------------------------------------------------

#[cfg(target_os = "none")]
va_trampoline!("warn", "vwarn", "8", "rsi");
#[cfg(target_os = "none")]
va_trampoline!("warnx", "vwarnx", "8", "rsi");
#[cfg(target_os = "none")]
va_trampoline!("err", "verr", "16", "rdx");
#[cfg(target_os = "none")]
va_trampoline!("errx", "verrx", "16", "rdx");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Stack buffer for the expanded format-string body.
const MSG_BUF_SIZE: usize = 1024;

/// Write a byte slice to stderr.
fn write_stderr(buf: &[u8]) {
    let _ = crate::file::write(2, buf.as_ptr(), buf.len());
}

/// Write a C string (null-terminated) to stderr.
fn write_cstr(s: *const u8) {
    if s.is_null() {
        return;
    }
    // SAFETY: `s` is a non-null C string per the caller.
    let len = unsafe { crate::string::strlen(s) };
    let _ = crate::file::write(2, s, len);
}

/// Common emitter for the whole family.
///
/// Prints `progname: <expanded fmt>` then, when `with_errno` is set,
/// `: strerror(errno)`, and finally a newline — matching glibc's `warn`
/// (errno) and `warnx` (no errno) output exactly.
///
/// `args` is the argument source the formatting engine pulls from; it is left
/// untouched when `fmt` is null, since nothing is expanded in that case.
fn emit(fmt: *const u8, args: &mut printf::Args, with_errno: bool) {
    // Capture errno up front: the message we report is the error that was
    // current at the call site, not whatever the writes below might set.
    let saved_errno = errno::get_errno();

    // Program-name prefix.
    // SAFETY: __progname is set by __libc_start_main; before that it points
    // at the static "unknown\0" string, so the read is always valid.
    let prog = unsafe { core::ptr::addr_of!(crate::crt::__progname).read() };
    if !prog.is_null() {
        write_cstr(prog);
        write_stderr(b": ");
    }

    // Expanded format body.
    if !fmt.is_null() {
        let mut body = [0u8; MSG_BUF_SIZE];
        let n = printf::_snprintf_impl(body.as_mut_ptr(), MSG_BUF_SIZE, fmt, args);
        let len = if n >= 0 && (n as usize) < MSG_BUF_SIZE {
            n as usize
        } else if n >= 0 {
            // Truncated: the buffer holds MSG_BUF_SIZE-1 chars + NUL.
            MSG_BUF_SIZE.wrapping_sub(1)
        } else {
            0
        };
        if let Some(slice) = body.get(..len) {
            write_stderr(slice);
        }
        if with_errno {
            write_stderr(b": ");
        }
    }

    // Errno description.
    if with_errno {
        let msg = crate::string::strerror(saved_errno);
        write_cstr(msg);
    }

    write_stderr(b"\n");
}

// ---------------------------------------------------------------------------
// v* variants — take a `va_list` (pointer); pure Rust, host-testable, and the
// delegation target of the trampolines above.
// ---------------------------------------------------------------------------

/// `vwarn(fmt, ap)` — `warn` with a `va_list`.
///
/// # Safety
/// `fmt` must be a valid NUL-terminated format string and `ap` a valid
/// `va_list` whose arguments match `fmt`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vwarn(fmt: *const u8, ap: *mut VaList) {
    // SAFETY: the caller guarantees `ap` is a valid va_list matching `fmt`;
    // a null one is rendered as zero arguments rather than a fault.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    emit(fmt, &mut args, true);
}

/// `vwarnx(fmt, ap)` — `warnx` with a `va_list`.
///
/// # Safety
/// As [`vwarn`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vwarnx(fmt: *const u8, ap: *mut VaList) {
    // SAFETY: as `vwarn`.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    emit(fmt, &mut args, false);
}

/// `verr(eval, fmt, ap)` — `err` with a `va_list`.
///
/// # Safety
/// As [`vwarn`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn verr(eval: i32, fmt: *const u8, ap: *mut VaList) -> ! {
    // SAFETY: as `vwarn`.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    emit(fmt, &mut args, true);
    crate::crt::exit(eval);
}

/// `verrx(eval, fmt, ap)` — `errx` with a `va_list`.
///
/// # Safety
/// As [`vwarn`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn verrx(eval: i32, fmt: *const u8, ap: *mut VaList) -> ! {
    // SAFETY: as `vwarn`.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    emit(fmt, &mut args, false);
    crate::crt::exit(eval);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // The emitter and the v* entry points write to fd 2; we can't capture that
    // output here, so most tests verify "doesn't crash" with a variety of
    // inputs.  The format *expansion* itself is exhaustively tested in
    // `printf.rs`; here we exercise the wiring from a `va_list` through
    // `vwarn`/`vwarnx` into the shared emitter.
    //
    // `err`/`errx`/`verr`/`verrx` are not called directly because they call
    // `exit()` and would terminate the test process.

    /// Build a synthetic SysV `va_list` with up to 6 integer args in the GP
    /// register save area (sufficient for these tests).
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

    #[test]
    fn vwarn_null_fmt_no_crash() {
        crate::errno::set_errno(crate::errno::EINVAL);
        // SAFETY: a null format string is handled by `emit` as "no body".
        unsafe { vwarn(core::ptr::null(), core::ptr::null_mut()) };
    }

    #[test]
    fn vwarn_plain_message_no_crash() {
        crate::errno::set_errno(crate::errno::ENOENT);
        // SAFETY: the format string has no conversions, so no args are read.
        unsafe { vwarn(b"test warning\0".as_ptr(), core::ptr::null_mut()) };
    }

    #[test]
    fn vwarn_with_format_args_no_crash() {
        // Exercises the format-expansion path: "%s" + a string arg.
        crate::errno::set_errno(crate::errno::ENOENT);
        let path = b"/etc/passwd\0";
        with_valist(&[path.as_ptr() as u64], |va| {
            // SAFETY: va is a valid synthetic va_list with one pointer arg.
            unsafe { vwarn(b"cannot open %s\0".as_ptr(), va) };
        });
    }

    #[test]
    fn vwarnx_null_fmt_no_crash() {
        // SAFETY: a null format string is handled by `emit` as "no body".
        unsafe { vwarnx(core::ptr::null(), core::ptr::null_mut()) };
    }

    #[test]
    fn vwarnx_with_int_arg_no_crash() {
        with_valist(&[42], |va| {
            // SAFETY: va is a valid synthetic va_list with one int arg.
            unsafe { vwarnx(b"code %d\0".as_ptr(), va) };
        });
    }

    /// More conversions than the retired flat `[u64; 8]` arrays could hold.
    /// Before BUG-POSIX-PRINTF-ARG-ARRAY-OOB was fixed this read past the end
    /// of those arrays; now every argument comes from the `va_list` itself.
    #[test]
    fn vwarnx_more_than_eight_int_args_no_crash() {
        with_valist(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], |va| {
            // SAFETY: va is a valid synthetic va_list with ten int args.
            unsafe { vwarnx(b"%d%d%d%d%d%d%d%d%d%d\0".as_ptr(), va) };
        });
    }

    #[test]
    fn vwarn_null_va_no_crash() {
        crate::errno::set_errno(crate::errno::EIO);
        // SAFETY: vwarn handles a null va_list by formatting with no args.
        unsafe { vwarn(b"plain\0".as_ptr(), core::ptr::null_mut()) };
    }

    #[test]
    fn vwarn_with_valist_expands_args_no_crash() {
        crate::errno::set_errno(crate::errno::EPERM);
        let msg = b"denied\0";
        with_valist(&[msg.as_ptr() as u64], |va| {
            // SAFETY: va is a valid synthetic va_list with one pointer arg.
            unsafe { vwarn(b"access: %s\0".as_ptr(), va) };
        });
    }

    #[test]
    fn vwarnx_with_valist_int_arg_no_crash() {
        with_valist(&[7], |va| {
            // SAFETY: va is a valid synthetic va_list with one int arg.
            unsafe { vwarnx(b"step %d\0".as_ptr(), va) };
        });
    }

    #[test]
    fn vwarnx_null_va_no_crash() {
        // SAFETY: vwarnx handles a null va_list by formatting with no args.
        unsafe { vwarnx(b"%d (literal, no arg)\0".as_ptr(), core::ptr::null_mut()) };
    }

    // -- Helper behavior --

    #[test]
    fn write_cstr_null_no_crash() {
        write_cstr(core::ptr::null());
    }

    #[test]
    fn write_cstr_empty_string_no_crash() {
        write_cstr(b"\0".as_ptr());
    }

    #[test]
    fn write_stderr_empty_no_crash() {
        write_stderr(b"");
    }

    #[test]
    fn emit_various_errno_no_crash() {
        for e in [
            0,
            crate::errno::EACCES,
            crate::errno::EIO,
            crate::errno::ENOMEM,
        ] {
            crate::errno::set_errno(e);
            emit(b"testing\0".as_ptr(), &mut printf::Args::empty(), true);
            emit(b"testing\0".as_ptr(), &mut printf::Args::empty(), false);
        }
    }
}
