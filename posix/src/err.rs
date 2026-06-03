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
//! Like [`crate::printf`], the variadic entry points are assembly
//! trampolines that capture register/stack varargs into flat integer and
//! float arrays and call a Rust `_*_impl`.  The `v*` variants take a real
//! `va_list` (which decays to a pointer on the x86_64 System V ABI) and are
//! therefore plain Rust functions — host-testable.  Both paths flatten their
//! arguments into the same `[u64; 8]` arrays and funnel through [`emit`],
//! which expands the format string with the tested `snprintf` engine before
//! adding the `progname:` prefix and optional `: strerror(errno)` suffix.
//!
//! Earlier this layer printed the format string *literally* (it declared the
//! functions as non-variadic and dropped the arguments), so `err(1, "open
//! %s", path)` produced `open %s: ...` instead of expanding `%s`.  That is
//! now fixed.

use crate::errno;
use crate::printf::{self, VaList};

// ---------------------------------------------------------------------------
// Assembly trampolines — capture varargs, then call the matching `_*_impl`.
//
// Layouts mirror the proven trampolines in `printf.rs`:
//   warn/warnx  : 1 fixed arg (fmt)        — same as `printf`
//   err/errx    : 2 fixed args (eval, fmt) — same as `fprintf`
// ---------------------------------------------------------------------------

#[cfg(target_os = "none")]
core::arch::global_asm!(
    // warn(fmt, ...) → _warn_impl(fmt, int_args, float_args)
    ".global warn",
    ".type warn, @function",
    "warn:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rsi",    // int vararg 0
    "mov [rsp+8], rdx",  // int vararg 1
    "mov [rsp+16], rcx", // int vararg 2
    "mov [rsp+24], r8",  // int vararg 3
    "mov [rsp+32], r9",  // int vararg 4
    "mov rax, [rbp+16]", // int vararg 5 (stack)
    "mov [rsp+40], rax",
    "mov rax, [rbp+24]", // int vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+32]", // int vararg 7
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    // rdi = fmt (already set)
    "mov rsi, rsp",      // int_args
    "lea rdx, [rsp+64]", // float_args
    "call _warn_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",
    // warnx(fmt, ...) → _warnx_impl(fmt, int_args, float_args)
    ".global warnx",
    ".type warnx, @function",
    "warnx:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rsi",
    "mov [rsp+8], rdx",
    "mov [rsp+16], rcx",
    "mov [rsp+24], r8",
    "mov [rsp+32], r9",
    "mov rax, [rbp+16]",
    "mov [rsp+40], rax",
    "mov rax, [rbp+24]",
    "mov [rsp+48], rax",
    "mov rax, [rbp+32]",
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    "mov rsi, rsp",
    "lea rdx, [rsp+64]",
    "call _warnx_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",
    // err(eval, fmt, ...) → _err_impl(eval, fmt, int_args, float_args)
    ".global err",
    ".type err, @function",
    "err:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rdx",    // int vararg 0
    "mov [rsp+8], rcx",  // int vararg 1
    "mov [rsp+16], r8",  // int vararg 2
    "mov [rsp+24], r9",  // int vararg 3
    "mov rax, [rbp+16]", // int vararg 4 (stack)
    "mov [rsp+32], rax",
    "mov rax, [rbp+24]", // int vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+32]", // int vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+40]", // int vararg 7
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    // rdi = eval, rsi = fmt (already set)
    "mov rdx, rsp",      // int_args
    "lea rcx, [rsp+64]", // float_args
    "call _err_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",
    // errx(eval, fmt, ...) → _errx_impl(eval, fmt, int_args, float_args)
    ".global errx",
    ".type errx, @function",
    "errx:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rdx",
    "mov [rsp+8], rcx",
    "mov [rsp+16], r8",
    "mov [rsp+24], r9",
    "mov rax, [rbp+16]",
    "mov [rsp+32], rax",
    "mov rax, [rbp+24]",
    "mov [rsp+40], rax",
    "mov rax, [rbp+32]",
    "mov [rsp+48], rax",
    "mov rax, [rbp+40]",
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    "mov rdx, rsp",
    "lea rcx, [rsp+64]",
    "call _errx_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",
);

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
/// `iargs`/`fargs` are the flattened integer and float argument arrays the
/// formatting engine consumes; either may be null when there are no args.
fn emit(fmt: *const u8, iargs: *const u64, fargs: *const u64, with_errno: bool) {
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
        let n = printf::_snprintf_impl(body.as_mut_ptr(), MSG_BUF_SIZE, fmt, iargs, fargs);
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
// Rust entry points (called by the assembly trampolines)
// ---------------------------------------------------------------------------

/// Backing implementation for `warn` — prints `fmt: strerror(errno)\n`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _warn_impl(fmt: *const u8, iargs: *const u64, fargs: *const u64) {
    emit(fmt, iargs, fargs, true);
}

/// Backing implementation for `warnx` — prints `fmt\n` (no errno).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _warnx_impl(fmt: *const u8, iargs: *const u64, fargs: *const u64) {
    emit(fmt, iargs, fargs, false);
}

/// Backing implementation for `err` — like `warn`, then `exit(eval)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _err_impl(eval: i32, fmt: *const u8, iargs: *const u64, fargs: *const u64) -> ! {
    emit(fmt, iargs, fargs, true);
    crate::crt::exit(eval);
}

/// Backing implementation for `errx` — like `warnx`, then `exit(eval)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn _errx_impl(eval: i32, fmt: *const u8, iargs: *const u64, fargs: *const u64) -> ! {
    emit(fmt, iargs, fargs, false);
    crate::crt::exit(eval);
}

// ---------------------------------------------------------------------------
// v* variants — take a `va_list` (pointer); pure Rust, host-testable.
// ---------------------------------------------------------------------------

/// `vwarn(fmt, ap)` — `warn` with a `va_list`.
///
/// # Safety
/// `fmt` must be a valid NUL-terminated format string and `ap` a valid
/// `va_list` whose arguments match `fmt`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vwarn(fmt: *const u8, ap: *mut VaList) {
    if ap.is_null() {
        emit(fmt, core::ptr::null(), core::ptr::null(), true);
        return;
    }
    // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
    let (iargs, fargs) = unsafe { printf::va_collect(fmt, &mut *ap) };
    emit(fmt, iargs.as_ptr(), fargs.as_ptr(), true);
}

/// `vwarnx(fmt, ap)` — `warnx` with a `va_list`.
///
/// # Safety
/// As [`vwarn`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vwarnx(fmt: *const u8, ap: *mut VaList) {
    if ap.is_null() {
        emit(fmt, core::ptr::null(), core::ptr::null(), false);
        return;
    }
    // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
    let (iargs, fargs) = unsafe { printf::va_collect(fmt, &mut *ap) };
    emit(fmt, iargs.as_ptr(), fargs.as_ptr(), false);
}

/// `verr(eval, fmt, ap)` — `err` with a `va_list`.
///
/// # Safety
/// As [`vwarn`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn verr(eval: i32, fmt: *const u8, ap: *mut VaList) -> ! {
    if ap.is_null() {
        emit(fmt, core::ptr::null(), core::ptr::null(), true);
    } else {
        // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
        let (iargs, fargs) = unsafe { printf::va_collect(fmt, &mut *ap) };
        emit(fmt, iargs.as_ptr(), fargs.as_ptr(), true);
    }
    crate::crt::exit(eval);
}

/// `verrx(eval, fmt, ap)` — `errx` with a `va_list`.
///
/// # Safety
/// As [`vwarn`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn verrx(eval: i32, fmt: *const u8, ap: *mut VaList) -> ! {
    if ap.is_null() {
        emit(fmt, core::ptr::null(), core::ptr::null(), false);
    } else {
        // SAFETY: ap is non-null; caller guarantees it is a valid va_list.
        let (iargs, fargs) = unsafe { printf::va_collect(fmt, &mut *ap) };
        emit(fmt, iargs.as_ptr(), fargs.as_ptr(), false);
    }
    crate::crt::exit(eval);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // The emitter and the `_*_impl`/v* entry points write to fd 2; we can't
    // capture that output here, so most tests verify "doesn't crash" with a
    // variety of inputs.  The format *expansion* itself is exhaustively
    // tested in `printf.rs`; here we exercise the wiring (flat-array path via
    // the `_*_impl` functions, and the `va_list` path via `vwarn`/`vwarnx`).
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
    fn warn_impl_null_fmt_no_crash() {
        crate::errno::set_errno(crate::errno::EINVAL);
        _warn_impl(core::ptr::null(), core::ptr::null(), core::ptr::null());
    }

    #[test]
    fn warn_impl_plain_message_no_crash() {
        crate::errno::set_errno(crate::errno::ENOENT);
        _warn_impl(
            b"test warning\0".as_ptr(),
            core::ptr::null(),
            core::ptr::null(),
        );
    }

    #[test]
    fn warn_impl_with_format_args_no_crash() {
        // Exercises the format-expansion path: "%s" + a string arg.
        crate::errno::set_errno(crate::errno::ENOENT);
        let path = b"/etc/passwd\0";
        let iargs = [path.as_ptr() as u64];
        _warn_impl(
            b"cannot open %s\0".as_ptr(),
            iargs.as_ptr(),
            core::ptr::null(),
        );
    }

    #[test]
    fn warnx_impl_null_fmt_no_crash() {
        _warnx_impl(core::ptr::null(), core::ptr::null(), core::ptr::null());
    }

    #[test]
    fn warnx_impl_with_int_arg_no_crash() {
        let iargs = [42u64];
        _warnx_impl(b"code %d\0".as_ptr(), iargs.as_ptr(), core::ptr::null());
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
            emit(
                b"testing\0".as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                true,
            );
            emit(
                b"testing\0".as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                false,
            );
        }
    }
}
