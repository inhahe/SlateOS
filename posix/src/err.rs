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
