//! BSD error/warning functions (`<err.h>`).
//!
//! Provides `err`, `errx`, `warn`, `warnx`, `verr`, `verrx`, `vwarn`,
//! `vwarnx` for formatted error messages to stderr.  These are not
//! strictly POSIX but are very widely used by Unix utilities (BSD,
//! macOS, and glibc all provide them).
//!
//! ## Behavior
//!
//! - `warn`/`vwarn`: prints "progname: fmt-args: strerror(errno)\n"
//! - `warnx`/`vwarnx`: prints "progname: fmt-args\n" (no errno)
//! - `err`/`verr`: like `warn` + `exit(eval)`
//! - `errx`/`verrx`: like `warnx` + `exit(eval)`
//!
//! Since we don't have C variadic support in Rust, the `v*` variants
//! accept a pre-formatted message buffer.  The plain variants are
//! stubs that print the format string as-is (not expanded).

use crate::errno;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write a byte slice to stderr.
fn write_stderr(buf: &[u8]) {
    let _ = crate::file::write(2, buf.as_ptr(), buf.len());
}

/// Write a C string (null-terminated) to stderr.
fn write_cstr(s: *const u8) {
    if s.is_null() {
        return;
    }
    let len = unsafe { crate::string::strlen(s) };
    let _ = crate::file::write(2, s, len);
}

/// Print the "progname: msg: strerror(errno)\n" pattern to stderr.
fn emit_warn(fmt: *const u8) {
    // Print program name prefix.
    // SAFETY: __progname is set by __libc_start_main; if not yet
    // initialized it points to the static "unknown\0" string.
    let prog = unsafe { core::ptr::addr_of!(crate::crt::__progname).read() };
    if !prog.is_null() {
        write_cstr(prog);
        write_stderr(b": ");
    }

    if !fmt.is_null() {
        write_cstr(fmt);
        write_stderr(b": ");
    }

    // Append "<strerror>\n".
    let err = errno::get_errno();
    let msg = crate::string::strerror(err);
    write_cstr(msg);
    write_stderr(b"\n");
}

/// Print "progname: msg\n" to stderr (no errno).
fn emit_warnx(fmt: *const u8) {
    // SAFETY: __progname is set by __libc_start_main; if not yet
    // initialized it points to the static "unknown\0" string.
    let prog = unsafe { core::ptr::addr_of!(crate::crt::__progname).read() };
    if !prog.is_null() {
        write_cstr(prog);
        write_stderr(b": ");
    }

    if !fmt.is_null() {
        write_cstr(fmt);
    }
    write_stderr(b"\n");
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Print a warning message with errno description and exit.
///
/// Prints `fmt: strerror(errno)\n` to stderr, then calls `exit(eval)`.
/// The `fmt` string is printed as-is (no printf-style expansion).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn err(eval: i32, fmt: *const u8) -> ! {
    emit_warn(fmt);
    crate::crt::exit(eval);
}

/// Print a warning message (no errno) and exit.
///
/// Prints `fmt\n` to stderr, then calls `exit(eval)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn errx(eval: i32, fmt: *const u8) -> ! {
    emit_warnx(fmt);
    crate::crt::exit(eval);
}

/// Print a warning message with errno description.
///
/// Prints `fmt: strerror(errno)\n` to stderr.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn warn(fmt: *const u8) {
    emit_warn(fmt);
}

/// Print a warning message (no errno).
///
/// Prints `fmt\n` to stderr.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn warnx(fmt: *const u8) {
    emit_warnx(fmt);
}

/// Print a warning with errno using a pre-formatted message and exit.
///
/// `msg` should be the result of formatting the original fmt string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn verr(eval: i32, msg: *const u8) -> ! {
    emit_warn(msg);
    crate::crt::exit(eval);
}

/// Print a warning (no errno) using a pre-formatted message and exit.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn verrx(eval: i32, msg: *const u8) -> ! {
    emit_warnx(msg);
    crate::crt::exit(eval);
}

/// Print a warning with errno using a pre-formatted message.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn vwarn(msg: *const u8) {
    emit_warn(msg);
}

/// Print a warning (no errno) using a pre-formatted message.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn vwarnx(msg: *const u8) {
    emit_warnx(msg);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- warn / warnx / vwarn / vwarnx don't crash with null fmt --
    //
    // These functions write to stderr. We can't easily capture the
    // output in unit tests, but we can verify they don't panic or
    // crash when called with null pointers and various inputs.

    #[test]
    fn warn_null_fmt_no_crash() {
        // Set a known errno so the output is deterministic.
        crate::errno::set_errno(crate::errno::EINVAL);
        warn(core::ptr::null());
        // Survived — no crash.
    }

    #[test]
    fn warn_with_message_no_crash() {
        crate::errno::set_errno(crate::errno::ENOENT);
        warn(b"test warning\0".as_ptr());
    }

    #[test]
    fn warnx_null_fmt_no_crash() {
        warnx(core::ptr::null());
    }

    #[test]
    fn warnx_with_message_no_crash() {
        warnx(b"test warnx\0".as_ptr());
    }

    #[test]
    fn vwarn_null_msg_no_crash() {
        crate::errno::set_errno(crate::errno::EIO);
        vwarn(core::ptr::null());
    }

    #[test]
    fn vwarn_with_message_no_crash() {
        crate::errno::set_errno(crate::errno::EPERM);
        vwarn(b"formatted message\0".as_ptr());
    }

    #[test]
    fn vwarnx_null_msg_no_crash() {
        vwarnx(core::ptr::null());
    }

    #[test]
    fn vwarnx_with_message_no_crash() {
        vwarnx(b"formatted warnx\0".as_ptr());
    }

    // -- Helper function behavior --

    #[test]
    fn write_cstr_null_no_crash() {
        // write_cstr with null should be a no-op.
        write_cstr(core::ptr::null());
    }

    #[test]
    fn write_cstr_empty_string_no_crash() {
        write_cstr(b"\0".as_ptr());
    }

    // -- warn with various errno values --

    #[test]
    fn warn_with_eperm_no_crash() {
        crate::errno::set_errno(crate::errno::EPERM);
        warn(b"permission check\0".as_ptr());
    }

    #[test]
    fn warn_with_zero_errno_no_crash() {
        crate::errno::set_errno(0);
        warn(b"no error\0".as_ptr());
    }

    // -- warnx with various messages --

    #[test]
    fn warnx_empty_message_no_crash() {
        warnx(b"\0".as_ptr());
    }

    #[test]
    fn warnx_long_message_no_crash() {
        warnx(b"this is a somewhat longer warning message for testing purposes\0".as_ptr());
    }

    // -- vwarn / vwarnx with various messages --

    #[test]
    fn vwarn_empty_message_no_crash() {
        crate::errno::set_errno(crate::errno::ENOSYS);
        vwarn(b"\0".as_ptr());
    }

    #[test]
    fn vwarnx_empty_message_no_crash() {
        vwarnx(b"\0".as_ptr());
    }

    // -- write_stderr helper --

    #[test]
    fn write_stderr_empty_no_crash() {
        write_stderr(b"");
    }

    #[test]
    fn write_stderr_with_content_no_crash() {
        write_stderr(b"test stderr output\n");
    }

    // -- emit_warn / emit_warnx internals --

    #[test]
    fn emit_warn_null_fmt_no_crash() {
        crate::errno::set_errno(crate::errno::ENOENT);
        emit_warn(core::ptr::null());
    }

    #[test]
    fn emit_warnx_null_fmt_no_crash() {
        emit_warnx(core::ptr::null());
    }

    #[test]
    fn emit_warn_with_message_no_crash() {
        crate::errno::set_errno(crate::errno::EIO);
        emit_warn(b"disk error\0".as_ptr());
    }

    #[test]
    fn emit_warnx_with_message_no_crash() {
        emit_warnx(b"invalid argument\0".as_ptr());
    }
}
