//! GNU error-reporting functions (`<error.h>`).
//!
//! Provides `error`, `error_at_line`, their `va_list` variants `verror` and
//! `verror_at_line`, and the three global variables `error_message_count`,
//! `error_one_per_line`, and `error_print_progname`.  These are GNU
//! extensions (not POSIX) but are used pervasively by GNU coreutils and a
//! large fraction of ported command-line tools, so a libc that omits them
//! cannot link those programs.
//!
//! ## Behavior (matching glibc)
//!
//! `error(status, errnum, format, ...)`:
//! 1. Print the program name followed by `: ` — unless
//!    [`error_print_progname`] is set, in which case that callback is invoked
//!    instead.
//! 2. Print the printf-expanded `format`.
//! 3. If `errnum != 0`, print `: strerror(errnum)`.
//! 4. Print a newline and increment [`error_message_count`].
//! 5. If `status != 0`, call `exit(status)`.
//!
//! `error_at_line(status, errnum, filename, linenum, format, ...)` is the
//! same but inserts `filename:linenum: ` after the program-name prefix.  When
//! [`error_one_per_line`] is nonzero it suppresses a message whose
//! `filename`/`linenum` matches the immediately preceding `error_at_line`
//! call (returning without printing, incrementing, or exiting — exactly as
//! glibc does).
//!
//! ## Implementation
//!
//! The C prototypes are variadic.  Like [`crate::printf`] and [`crate::err`],
//! the variadic entry points (`error`, `error_at_line`) are assembly
//! trampolines that perform a real `va_start` — spilling the argument
//! registers into a System V register save area and building a `va_list` over
//! it — and then call the matching `v*` variant.  Those are plain Rust and
//! host-testable, and they funnel through [`do_error`], which expands the
//! format with the tested `snprintf` engine.
//!
//! Streaming the arguments straight out of the `va_list` rather than
//! flattening them into fixed arrays is what lifts the old eight-integer /
//! eight-float ceiling and makes `%Lf` reachable here; see
//! BUG-POSIX-PRINTF-ARG-ARRAY-OOB and BUG-POSIX-LONG-DOUBLE-ABI in
//! `known-issues.md`.
//!
//! ## Limitation
//!
//! As with every real libc, a literal `%` in the format must be written `%%`.

// Calls `printf::_snprintf_impl`; underscore is the ABI convention for
// libc impl trampoline targets, not a privacy marker.
#![allow(clippy::used_underscore_items)]

use crate::printf::{self, VaList};

#[cfg(target_os = "none")]
use crate::printf::va_trampoline;

// ---------------------------------------------------------------------------
// Global variables (exported C symbols).
// ---------------------------------------------------------------------------

/// Number of messages printed by `error`/`error_at_line` so far.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut error_message_count: u32 = 0;

/// When nonzero, `error_at_line` suppresses consecutive duplicate
/// `filename`/`linenum` messages.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut error_one_per_line: i32 = 0;

/// Optional callback that replaces the default `progname: ` prefix.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut error_print_progname: Option<extern "C" fn()> = None;

/// Last `filename` passed to `error_at_line` (for `error_one_per_line`).
static mut OLD_FILENAME: *const u8 = core::ptr::null();
/// Last `linenum` passed to `error_at_line` (for `error_one_per_line`).
static mut OLD_LINENUM: u32 = 0;

/// Stack buffer for the expanded format-string body.
const MSG_BUF_SIZE: usize = 1024;

// ---------------------------------------------------------------------------
// Assembly trampolines — `va_start`, then call the matching `v*` variant.
//
// The named-argument counts decide the initial `gp_offset` and which register
// carries the `va_list*`:
//   error(status, errnum, fmt, ...)                  : 3 named args
//   error_at_line(status, errnum, file, line, fmt, …): 5 named args
// ---------------------------------------------------------------------------

#[cfg(target_os = "none")]
va_trampoline!("error", "verror", "24", "rcx");
#[cfg(target_os = "none")]
va_trampoline!("error_at_line", "verror_at_line", "40", "r9");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write a byte slice to stderr.
fn write_stderr(buf: &[u8]) {
    let _ = crate::file::write(2, buf.as_ptr(), buf.len());
}

/// Write a NUL-terminated C string to stderr.
fn write_cstr(s: *const u8) {
    if s.is_null() {
        return;
    }
    // SAFETY: `s` is a non-null C string per the caller.
    let len = unsafe { crate::string::strlen(s) };
    let _ = crate::file::write(2, s, len);
}

/// Write an unsigned integer in decimal to stderr.
fn write_u32_dec(mut val: u32) {
    let mut buf = [0u8; 10];
    if val == 0 {
        write_stderr(b"0");
        return;
    }
    let mut pos = buf.len();
    while val > 0 && pos > 0 {
        pos = pos.wrapping_sub(1);
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0'.wrapping_add((val % 10) as u8);
        }
        val = val.wrapping_div(10);
    }
    if let Some(slice) = buf.get(pos..) {
        write_stderr(slice);
    }
}

/// Shared body for the whole family.
///
/// `with_line` selects the `error_at_line` framing (the `filename:linenum: `
/// insert and the `error_one_per_line` dedup).  `args` is the argument source
/// the format engine pulls from; it is left untouched when `fmt` is null.
fn do_error(
    status: i32,
    errnum: i32,
    filename: *const u8,
    linenum: u32,
    with_line: bool,
    fmt: *const u8,
    args: &mut printf::Args,
) {
    // error_one_per_line dedup (only meaningful for error_at_line).
    if with_line {
        let one_per_line = unsafe { *core::ptr::addr_of!(error_one_per_line) };
        if one_per_line != 0 {
            let old_file = unsafe { *core::ptr::addr_of!(OLD_FILENAME) };
            let old_line = unsafe { *core::ptr::addr_of!(OLD_LINENUM) };
            let same_file = filename == old_file
                || (!old_file.is_null()
                    && !filename.is_null()
                    // SAFETY: both pointers are non-null C strings here.
                    && unsafe { crate::string::strcmp(old_file, filename) } == 0);
            if old_line == linenum && same_file {
                // Matches the previous call — print nothing, change nothing.
                return;
            }
            unsafe {
                core::ptr::addr_of_mut!(OLD_FILENAME).write(filename);
                core::ptr::addr_of_mut!(OLD_LINENUM).write(linenum);
            }
        }
    }

    // Program-name prefix, or the user-supplied callback.
    let cb = unsafe { *core::ptr::addr_of!(error_print_progname) };
    if let Some(print_progname) = cb {
        print_progname();
    } else {
        // SAFETY: __progname is always a valid C string (defaults to
        // "unknown\0" before __libc_start_main sets it).
        let prog = unsafe { core::ptr::addr_of!(crate::crt::__progname).read() };
        if !prog.is_null() {
            write_cstr(prog);
        }
        write_stderr(b": ");
    }

    // error_at_line inserts "filename:linenum: ".
    if with_line && !filename.is_null() {
        write_cstr(filename);
        write_stderr(b":");
        write_u32_dec(linenum);
        write_stderr(b": ");
    }

    // Expanded format body.
    if !fmt.is_null() {
        let mut body = [0u8; MSG_BUF_SIZE];
        let n = printf::_snprintf_impl(body.as_mut_ptr(), MSG_BUF_SIZE, fmt, args);
        let len = if n >= 0 && (n as usize) < MSG_BUF_SIZE {
            n as usize
        } else if n >= 0 {
            MSG_BUF_SIZE.wrapping_sub(1)
        } else {
            0
        };
        if let Some(slice) = body.get(..len) {
            write_stderr(slice);
        }
    }

    // ": strerror(errnum)" when an errno was supplied.
    if errnum != 0 {
        write_stderr(b": ");
        let msg = crate::string::strerror(errnum);
        write_cstr(msg);
    }

    write_stderr(b"\n");

    // Count the message.
    unsafe {
        let c = core::ptr::addr_of!(error_message_count).read();
        core::ptr::addr_of_mut!(error_message_count).write(c.wrapping_add(1));
    }

    if status != 0 {
        crate::crt::exit(status);
    }
}

// ---------------------------------------------------------------------------
// v* variants — take a `va_list` (pointer); pure Rust, host-testable, and the
// delegation target of the trampolines above.
// ---------------------------------------------------------------------------

/// `verror(status, errnum, format, ap)` — `error` with a `va_list`.
///
/// # Safety
///
/// `fmt` must be a valid NUL-terminated format string (or null) and `ap` a
/// valid `va_list` whose arguments match `fmt`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn verror(status: i32, errnum: i32, fmt: *const u8, ap: *mut VaList) {
    // SAFETY: the caller guarantees `ap` is a valid va_list matching `fmt`;
    // a null one is rendered as zero arguments rather than a fault.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    do_error(status, errnum, core::ptr::null(), 0, false, fmt, &mut args);
}

/// `verror_at_line(status, errnum, filename, linenum, format, ap)`.
///
/// # Safety
///
/// As [`verror`]; `filename` must be null or a valid C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn verror_at_line(
    status: i32,
    errnum: i32,
    filename: *const u8,
    linenum: u32,
    fmt: *const u8,
    ap: *mut VaList,
) {
    // SAFETY: as `verror`.
    let mut args = unsafe { printf::Args::from_raw(ap) };
    do_error(status, errnum, filename, linenum, true, fmt, &mut args);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errno;

    // These functions write to fd 2; the format expansion itself is tested
    // exhaustively in `printf.rs`.  Here we exercise the wiring and the
    // bookkeeping (error_message_count, error_one_per_line dedup) that is
    // observable without capturing stderr.  Calls use status == 0 so they
    // never invoke exit().
    //
    // Phase 220 (2026-06-03): every test in this module mutates global
    // state (`error_message_count`, `error_one_per_line`, `OLD_FILENAME`,
    // `OLD_LINENUM`, `error_print_progname`).  Without serialization,
    // cargo's parallel runner can interleave a `read_count()` /
    // `verror()` / `read_count()` triple, producing flaky
    // "left: 1 / right: 2" failures when a sibling test slips an
    // increment in between.  TEST_LOCK serialises every test that
    // touches the bookkeeping so the assertions remain deterministic.
    use std::sync::Mutex;
    static TEST_LOCK: Mutex<()> = Mutex::new(());

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

    fn read_count() -> u32 {
        unsafe { core::ptr::addr_of!(error_message_count).read() }
    }

    #[test]
    fn verror_increments_count() {
        let _guard = TEST_LOCK.lock().unwrap();
        let before = read_count();
        // SAFETY: the format string has no conversions, so no args are read.
        unsafe {
            verror(0, 0, b"plain message\0".as_ptr(), core::ptr::null_mut());
        }
        assert_eq!(read_count(), before.wrapping_add(1));
    }

    #[test]
    fn verror_with_errnum_no_crash() {
        let _guard = TEST_LOCK.lock().unwrap();
        let before = read_count();
        // SAFETY: no conversions in the format string.
        unsafe {
            verror(
                0,
                errno::ENOENT,
                b"cannot stat\0".as_ptr(),
                core::ptr::null_mut(),
            );
        }
        assert_eq!(read_count(), before.wrapping_add(1));
    }

    #[test]
    fn verror_with_format_args_no_crash() {
        let _guard = TEST_LOCK.lock().unwrap();
        let path = b"/tmp/x\0";
        let before = read_count();
        with_valist(&[path.as_ptr() as u64], |va| {
            // SAFETY: va is a valid synthetic va_list with one pointer arg.
            unsafe { verror(0, 0, b"open %s failed\0".as_ptr(), va) };
        });
        assert_eq!(read_count(), before.wrapping_add(1));
    }

    /// More conversions than the retired flat `[u64; 8]` arrays could hold —
    /// a regression guard for BUG-POSIX-PRINTF-ARG-ARRAY-OOB.
    #[test]
    fn verror_more_than_eight_int_args_no_crash() {
        let _guard = TEST_LOCK.lock().unwrap();
        let before = read_count();
        with_valist(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], |va| {
            // SAFETY: va is a valid synthetic va_list with ten int args.
            unsafe { verror(0, 0, b"%d%d%d%d%d%d%d%d%d%d\0".as_ptr(), va) };
        });
        assert_eq!(read_count(), before.wrapping_add(1));
    }

    #[test]
    fn verror_null_fmt_no_crash() {
        let _guard = TEST_LOCK.lock().unwrap();
        // SAFETY: a null format string is handled as "no body".
        unsafe {
            verror(0, errno::EIO, core::ptr::null(), core::ptr::null_mut());
        }
    }

    #[test]
    fn verror_at_line_no_crash() {
        let _guard = TEST_LOCK.lock().unwrap();
        let before = read_count();
        // SAFETY: no conversions in the format string.
        unsafe {
            verror_at_line(
                0,
                errno::EACCES,
                b"main.c\0".as_ptr(),
                42,
                b"bad token\0".as_ptr(),
                core::ptr::null_mut(),
            );
        }
        assert_eq!(read_count(), before.wrapping_add(1));
    }

    #[test]
    fn error_one_per_line_suppresses_duplicate() {
        let _guard = TEST_LOCK.lock().unwrap();
        // Enable dedup, then call twice with identical file/line.
        unsafe {
            core::ptr::addr_of_mut!(error_one_per_line).write(1);
        }
        // Reset the remembered location to something that won't match.
        unsafe {
            core::ptr::addr_of_mut!(OLD_FILENAME).write(core::ptr::null());
            core::ptr::addr_of_mut!(OLD_LINENUM).write(0);
        }
        let before = read_count();
        let file = b"dup.c\0";
        // SAFETY: none of these format strings have conversions.
        unsafe {
            verror_at_line(
                0,
                0,
                file.as_ptr(),
                7,
                b"first\0".as_ptr(),
                core::ptr::null_mut(),
            );
        }
        // First call prints and increments.
        assert_eq!(read_count(), before.wrapping_add(1));
        // SAFETY: as above.
        unsafe {
            verror_at_line(
                0,
                0,
                file.as_ptr(),
                7,
                b"second\0".as_ptr(),
                core::ptr::null_mut(),
            );
        }
        // Second identical call is suppressed: count unchanged.
        assert_eq!(read_count(), before.wrapping_add(1));
        // A different line is not suppressed.
        // SAFETY: as above.
        unsafe {
            verror_at_line(
                0,
                0,
                file.as_ptr(),
                8,
                b"third\0".as_ptr(),
                core::ptr::null_mut(),
            );
        }
        assert_eq!(read_count(), before.wrapping_add(2));
        // Restore.
        unsafe {
            core::ptr::addr_of_mut!(error_one_per_line).write(0);
        }
    }

    #[test]
    fn verror_null_va_no_crash() {
        let _guard = TEST_LOCK.lock().unwrap();
        let before = read_count();
        unsafe { verror(0, errno::EPERM, b"denied\0".as_ptr(), core::ptr::null_mut()) };
        assert_eq!(read_count(), before.wrapping_add(1));
    }

    #[test]
    fn verror_with_valist_expands_args() {
        let _guard = TEST_LOCK.lock().unwrap();
        let msg = b"world\0";
        let before = read_count();
        with_valist(&[msg.as_ptr() as u64], |va| {
            // SAFETY: va is a valid synthetic va_list with one pointer arg.
            unsafe { verror(0, 0, b"hello %s\0".as_ptr(), va) };
        });
        assert_eq!(read_count(), before.wrapping_add(1));
    }

    #[test]
    fn verror_at_line_with_valist_int_arg() {
        let _guard = TEST_LOCK.lock().unwrap();
        let before = read_count();
        with_valist(&[99], |va| {
            // SAFETY: va is a valid synthetic va_list with one int arg.
            unsafe { verror_at_line(0, 0, b"f.c\0".as_ptr(), 3, b"value %d\0".as_ptr(), va) };
        });
        assert_eq!(read_count(), before.wrapping_add(1));
    }

    #[test]
    fn error_print_progname_callback_invoked() {
        let _guard = TEST_LOCK.lock().unwrap();
        use core::sync::atomic::{AtomicU32, Ordering};
        static CALLS: AtomicU32 = AtomicU32::new(0);
        extern "C" fn cb() {
            CALLS.fetch_add(1, Ordering::SeqCst);
        }
        // Reset CALLS for re-runs (static persists across invocations
        // within the same test binary).
        CALLS.store(0, Ordering::SeqCst);
        unsafe {
            core::ptr::addr_of_mut!(error_print_progname).write(Some(cb));
        }
        // SAFETY: no conversions in the format string.
        unsafe {
            verror(0, 0, b"msg\0".as_ptr(), core::ptr::null_mut());
        }
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        // Restore default behavior.
        unsafe {
            core::ptr::addr_of_mut!(error_print_progname).write(None);
        }
    }

    #[test]
    fn write_u32_dec_no_crash() {
        write_u32_dec(0);
        write_u32_dec(7);
        write_u32_dec(12345);
        write_u32_dec(u32::MAX);
    }
}
