//! POSIX system logging.
//!
//! Implements `openlog`, `syslog`, `closelog`, `setlogmask` stubs.
//!
//! ## Implementation
//!
//! Our OS doesn't have a syslog daemon.  Messages are written to stderr
//! (fd 2) with a priority prefix so they're visible on the console.
//!
//! `syslog` is variadic in C (`void syslog(int, const char *fmt, ...)`), so —
//! like [`crate::printf`] — its entry point is an assembly trampoline that
//! captures register/stack varargs into flat arrays and calls `_syslog_impl`,
//! which expands the printf-style format with the tested snprintf engine
//! before adding the priority/ident/PID framing.  `vsyslog` takes a real
//! `va_list` (a pointer on the x86_64 System V ABI) and is plain Rust.
//!
//! Under `_FORTIFY_SOURCE` the libc headers redirect `syslog`/`vsyslog` to
//! `__syslog_chk`/`__vsyslog_chk` (an extra leading `flag` argument); both
//! are provided here and discard the flag, delegating to the same engine.
//!
//! ## Limitations
//!
//! - No actual syslog daemon or log rotation.
//! - `openlog` ident string is stored but the facility is ignored.
//! - `setlogmask` filtering works correctly.
//! - The format string is now printf-expanded: a literal `%` in a message
//!   must be written `%%` (matching every real libc).

// Calls `printf::_snprintf_impl`; underscore is the ABI convention for libc
// impl trampoline targets, not a privacy marker.  Also defines a local
// `_syslog_impl` trampoline target with the same convention.
#![allow(clippy::used_underscore_items)]

use crate::file;
use crate::printf::{self, VaList};
use crate::string;

// ---------------------------------------------------------------------------
// Assembly trampoline — capture varargs, then call `_syslog_impl`.
// Layout matches `fprintf`: 2 fixed args (priority, fmt), rest varargs.
// ---------------------------------------------------------------------------

#[cfg(target_os = "none")]
core::arch::global_asm!(
    // syslog(priority, fmt, ...) → _syslog_impl(priority, fmt, int_args, float_args)
    ".global syslog",
    ".type syslog, @function",
    "syslog:",
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
    // rdi = priority, rsi = fmt (already set)
    "mov rdx, rsp",      // int_args
    "lea rcx, [rsp+64]", // float_args
    "call _syslog_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",
    // __syslog_chk(priority, flag, fmt, ...) → _syslog_impl(priority, fmt, int_args, float_args)
    // The _FORTIFY_SOURCE redirect target for syslog().  Fixed args:
    // rdi=priority, rsi=flag, rdx=fmt; varargs start at rcx (like fprintf).
    // The fortify `flag` is discarded.
    ".global __syslog_chk",
    ".type __syslog_chk, @function",
    "__syslog_chk:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 128",
    "mov [rsp], rcx",    // int vararg 0
    "mov [rsp+8], r8",   // int vararg 1
    "mov [rsp+16], r9",  // int vararg 2
    "mov rax, [rbp+16]", // int vararg 3 (stack)
    "mov [rsp+24], rax",
    "mov rax, [rbp+24]", // int vararg 4
    "mov [rsp+32], rax",
    "mov rax, [rbp+32]", // int vararg 5
    "mov [rsp+40], rax",
    "mov rax, [rbp+40]", // int vararg 6
    "mov [rsp+48], rax",
    "mov rax, [rbp+48]", // int vararg 7
    "mov [rsp+56], rax",
    "movsd [rsp+64], xmm0",
    "movsd [rsp+72], xmm1",
    "movsd [rsp+80], xmm2",
    "movsd [rsp+88], xmm3",
    "movsd [rsp+96], xmm4",
    "movsd [rsp+104], xmm5",
    "movsd [rsp+112], xmm6",
    "movsd [rsp+120], xmm7",
    // rdi=priority already set; move fmt into rsi.
    "mov rsi, rdx",      // fmt
    "mov rdx, rsp",      // int_args
    "lea rcx, [rsp+64]", // float_args
    "call _syslog_impl",
    "add rsp, 128",
    "pop rbp",
    "ret",
);

/// Stack buffer for the expanded format-string body.
const SYSLOG_MSG_BUF: usize = 1024;

// ---------------------------------------------------------------------------
// Priority levels
// ---------------------------------------------------------------------------

/// System is unusable.
pub const LOG_EMERG: i32 = 0;
/// Action must be taken immediately.
pub const LOG_ALERT: i32 = 1;
/// Critical conditions.
pub const LOG_CRIT: i32 = 2;
/// Error conditions.
pub const LOG_ERR: i32 = 3;
/// Warning conditions.
pub const LOG_WARNING: i32 = 4;
/// Normal but significant.
pub const LOG_NOTICE: i32 = 5;
/// Informational.
pub const LOG_INFO: i32 = 6;
/// Debug-level messages.
pub const LOG_DEBUG: i32 = 7;

// ---------------------------------------------------------------------------
// Facility codes
// ---------------------------------------------------------------------------

/// Kernel messages.
pub const LOG_KERN: i32 = 0;
/// User-level messages.
pub const LOG_USER: i32 = 1 << 3;
/// Mail system.
pub const LOG_MAIL: i32 = 2 << 3;
/// System daemons.
pub const LOG_DAEMON: i32 = 3 << 3;
/// Security/authorization.
pub const LOG_AUTH: i32 = 4 << 3;
/// syslogd internal.
pub const LOG_SYSLOG: i32 = 5 << 3;
/// Line printer.
pub const LOG_LPR: i32 = 6 << 3;
/// Network news.
pub const LOG_NEWS: i32 = 7 << 3;
/// UUCP.
pub const LOG_UUCP: i32 = 8 << 3;
/// Clock daemon.
pub const LOG_CRON: i32 = 9 << 3;
/// Local use 0-7.
pub const LOG_LOCAL0: i32 = 16 << 3;
pub const LOG_LOCAL1: i32 = 17 << 3;
pub const LOG_LOCAL2: i32 = 18 << 3;
pub const LOG_LOCAL3: i32 = 19 << 3;
pub const LOG_LOCAL4: i32 = 20 << 3;
pub const LOG_LOCAL5: i32 = 21 << 3;
pub const LOG_LOCAL6: i32 = 22 << 3;
pub const LOG_LOCAL7: i32 = 23 << 3;

// ---------------------------------------------------------------------------
// Option flags
// ---------------------------------------------------------------------------

/// Log to stderr as well.
pub const LOG_PERROR: i32 = 0x20;
/// Log the PID with each message.
pub const LOG_PID: i32 = 0x01;
/// Don't delay open.
pub const LOG_NDELAY: i32 = 0x08;

/// Extract the priority from a combined priority+facility value.
#[inline]
const fn log_pri(p: i32) -> i32 {
    p & 0x07
}

/// Create a mask for setlogmask that includes priority `p`.
#[inline]
#[must_use]
pub const fn log_mask(p: i32) -> i32 {
    1 << p
}

/// Create a mask for all priorities up to and including `p`.
#[inline]
#[must_use]
#[allow(clippy::arithmetic_side_effects)]
// Shift and subtract are safe: p is always 0-7 (LOG_EMERG..LOG_DEBUG),
// so (p+1) is at most 8, and 1<<8 = 256, well within i32 range.
pub const fn log_upto(p: i32) -> i32 {
    (1 << (p + 1)) - 1
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Ident string pointer from openlog (may be null).
static mut SYSLOG_IDENT: *const u8 = core::ptr::null();
/// Log options from openlog.
static mut SYSLOG_OPTIONS: i32 = 0;
/// Log mask — defaults to allowing everything.
static mut SYSLOG_MASK: i32 = 0xFF;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Open a connection to the system logger.
///
/// `ident` is prepended to each message.  `option` controls logging
/// behavior (LOG_PID, LOG_PERROR, etc.).  `facility` is the default
/// facility for subsequent syslog() calls.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn openlog(ident: *const u8, option: i32, _facility: i32) {
    // SAFETY: Single-threaded access to static state.
    unsafe {
        core::ptr::addr_of_mut!(SYSLOG_IDENT).write(ident);
        core::ptr::addr_of_mut!(SYSLOG_OPTIONS).write(option);
    }
}

/// Implementation behind the `syslog` assembly trampoline.
///
/// Expands the printf-style `fmt`/varargs into a stack buffer, then frames
/// the result with the priority prefix, ident, and (optionally) PID before
/// writing to stderr.  `iargs`/`fargs` are the captured integer/float
/// vararg arrays (each up to 8 entries) produced by the trampoline.
///
/// # Safety
///
/// `fmt` must be a valid NUL-terminated C string (or null), and
/// `iargs`/`fargs` must each point to at least 8 `u64` slots when `fmt`
/// references that many conversions (the trampoline always provides 8).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _syslog_impl(
    priority: i32,
    fmt: *const u8,
    iargs: *const u64,
    fargs: *const u64,
) {
    do_syslog(priority, fmt, iargs, fargs);
}

/// `vsyslog(int priority, const char *fmt, va_list ap)` — the va_list form.
///
/// On the x86_64 System V ABI a `va_list` is `__va_list_tag[1]`, so the
/// `va_list` parameter decays to a pointer and this function is plain Rust.
///
/// # Safety
///
/// `fmt` must be a valid NUL-terminated C string (or null) and `ap` must
/// point to a valid, initialised `va_list` matching `fmt`'s conversions.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn vsyslog(priority: i32, fmt: *const u8, ap: *mut VaList) {
    if ap.is_null() {
        return;
    }
    // SAFETY: caller guarantees `ap` is a valid va_list and `fmt` matches it.
    let (iargs, fargs) = unsafe { printf::va_collect(fmt, &mut *ap) };
    do_syslog(priority, fmt, iargs.as_ptr(), fargs.as_ptr());
}

/// `__vsyslog_chk(priority, flag, fmt, ap)` — the `_FORTIFY_SOURCE` redirect
/// target for `vsyslog`.  The fortify `flag` is accepted and ignored.
///
/// # Safety
///
/// As [`vsyslog`]: `fmt` must be a valid NUL-terminated C string (or null)
/// and `ap` a valid `va_list` matching `fmt`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __vsyslog_chk(priority: i32, _flag: i32, fmt: *const u8, ap: *mut VaList) {
    // SAFETY: forwards to vsyslog, which validates `ap`.
    unsafe { vsyslog(priority, fmt, ap) };
}

/// Shared body for `_syslog_impl` and `vsyslog`: mask-filter, printf-expand
/// the format into a stack buffer, then write the framed line to stderr.
fn do_syslog(priority: i32, fmt: *const u8, iargs: *const u64, fargs: *const u64) {
    let pri = log_pri(priority);

    // Check log mask before doing any work.
    let mask = unsafe { *core::ptr::addr_of!(SYSLOG_MASK) };
    if mask & (1 << pri) == 0 {
        return; // Filtered out.
    }

    if fmt.is_null() {
        return;
    }

    // Expand the printf-style format into a stack buffer.  `_snprintf_impl`
    // is a safe wrapper that bounds all writes to the buffer and always
    // NUL-terminates; it tolerates null `iargs`/`fargs` when `fmt` has no
    // matching conversions.
    let mut body = [0u8; SYSLOG_MSG_BUF];
    let written = printf::_snprintf_impl(body.as_mut_ptr(), SYSLOG_MSG_BUF, fmt, iargs, fargs);
    // Clamp to the buffer (snprintf returns the would-be length, which may
    // exceed the buffer when truncated).
    let body_len = if written < 0 {
        0
    } else {
        (written as usize).min(SYSLOG_MSG_BUF.wrapping_sub(1))
    };

    // Priority prefix.
    let prefix = match pri {
        LOG_EMERG => b"<EMERG> " as &[u8],
        LOG_ALERT => b"<ALERT> ",
        LOG_CRIT => b"<CRIT> ",
        LOG_ERR => b"<ERR> ",
        LOG_WARNING => b"<WARNING> ",
        LOG_NOTICE => b"<NOTICE> ",
        LOG_INFO => b"<INFO> ",
        LOG_DEBUG => b"<DEBUG> ",
        _ => b"<LOG> ",
    };

    let fd = 2; // stderr

    // Write ident if set.
    let ident = unsafe { *core::ptr::addr_of!(SYSLOG_IDENT) };
    if !ident.is_null() {
        let ident_len = unsafe { string::strlen(ident) };
        file::write(fd, ident, ident_len);
        file::write(fd, b": ".as_ptr(), 2);
    }

    // Write priority prefix.
    file::write(fd, prefix.as_ptr(), prefix.len());

    // Write PID if requested.
    let options = unsafe { *core::ptr::addr_of!(SYSLOG_OPTIONS) };
    if options & LOG_PID != 0 {
        file::write(fd, b"[".as_ptr(), 1);
        let pid = crate::process::getpid();
        let mut pid_buf = [0u8; 16];
        let pid_len = write_u32(pid as u32, &mut pid_buf);
        let start = pid_buf.len().wrapping_sub(pid_len);
        if let Some(slice) = pid_buf.get(start..) {
            file::write(fd, slice.as_ptr(), pid_len);
        }
        file::write(fd, b"] ".as_ptr(), 2);
    }

    // Write the expanded message body.
    if let Some(slice) = body.get(..body_len) {
        file::write(fd, slice.as_ptr(), body_len);
    }
    file::write(fd, b"\n".as_ptr(), 1);
}

/// Close the connection to the system logger.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn closelog() {
    unsafe {
        core::ptr::addr_of_mut!(SYSLOG_IDENT).write(core::ptr::null());
        core::ptr::addr_of_mut!(SYSLOG_OPTIONS).write(0);
    }
}

/// Set the log priority mask.
///
/// Returns the previous mask value.  Only messages whose priority
/// is set in the mask will be logged.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setlogmask(mask: i32) -> i32 {
    let old = unsafe { *core::ptr::addr_of!(SYSLOG_MASK) };
    if mask != 0 {
        unsafe {
            core::ptr::addr_of_mut!(SYSLOG_MASK).write(mask);
        }
    }
    old
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write a u32 as decimal into a buffer (right-aligned).
/// Returns the number of digits written.
fn write_u32(mut val: u32, buf: &mut [u8; 16]) -> usize {
    if val == 0 {
        if let Some(slot) = buf.last_mut() {
            *slot = b'0';
        }
        return 1;
    }

    let mut pos = buf.len();
    while val > 0 && pos > 0 {
        pos = pos.wrapping_sub(1);
        if let Some(slot) = buf.get_mut(pos) {
            *slot = b'0'.wrapping_add((val % 10) as u8);
        }
        val = val.wrapping_div(10);
    }

    buf.len().wrapping_sub(pos)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Pure helper tests --

    #[test]
    fn test_log_pri_extracts_priority() {
        // Priority is the low 3 bits.
        assert_eq!(log_pri(LOG_ERR), LOG_ERR);
        assert_eq!(log_pri(LOG_DEBUG), LOG_DEBUG);
        assert_eq!(log_pri(LOG_EMERG), LOG_EMERG);
    }

    #[test]
    fn test_log_pri_strips_facility() {
        // LOG_USER | LOG_ERR = (1<<3) | 3 = 11
        assert_eq!(log_pri(LOG_USER | LOG_ERR), LOG_ERR);
        // LOG_DAEMON | LOG_WARNING = (3<<3) | 4 = 28
        assert_eq!(log_pri(LOG_DAEMON | LOG_WARNING), LOG_WARNING);
        // LOG_LOCAL7 | LOG_DEBUG = (23<<3) | 7 = 191
        assert_eq!(log_pri(LOG_LOCAL7 | LOG_DEBUG), LOG_DEBUG);
    }

    #[test]
    fn test_log_mask_single_priority() {
        assert_eq!(log_mask(LOG_EMERG), 1); // 1 << 0
        assert_eq!(log_mask(LOG_ERR), 1 << 3); // 1 << 3
        assert_eq!(log_mask(LOG_DEBUG), 1 << 7);
    }

    #[test]
    fn test_log_upto_includes_lower() {
        // LOG_UPTO(LOG_ERR) should include EMERG, ALERT, CRIT, ERR
        let mask = log_upto(LOG_ERR);
        assert_ne!(mask & log_mask(LOG_EMERG), 0);
        assert_ne!(mask & log_mask(LOG_ALERT), 0);
        assert_ne!(mask & log_mask(LOG_CRIT), 0);
        assert_ne!(mask & log_mask(LOG_ERR), 0);
        // But not WARNING or above.
        assert_eq!(mask & log_mask(LOG_WARNING), 0);
        assert_eq!(mask & log_mask(LOG_INFO), 0);
        assert_eq!(mask & log_mask(LOG_DEBUG), 0);
    }

    #[test]
    fn test_log_upto_all() {
        let mask = log_upto(LOG_DEBUG); // All priorities
        for p in 0..=7 {
            assert_ne!(mask & log_mask(p), 0, "priority {p} should be set");
        }
    }

    // -- write_u32 tests --

    #[test]
    fn test_write_u32_zero() {
        let mut buf = [0u8; 16];
        let len = write_u32(0, &mut buf);
        assert_eq!(len, 1);
        assert_eq!(buf[15], b'0');
    }

    #[test]
    fn test_write_u32_single_digit() {
        let mut buf = [0u8; 16];
        let len = write_u32(7, &mut buf);
        assert_eq!(len, 1);
        assert_eq!(buf[15], b'7');
    }

    #[test]
    fn test_write_u32_multi_digit() {
        let mut buf = [0u8; 16];
        let len = write_u32(12345, &mut buf);
        assert_eq!(len, 5);
        assert_eq!(&buf[11..16], b"12345");
    }

    #[test]
    fn test_write_u32_max() {
        let mut buf = [0u8; 16];
        let len = write_u32(u32::MAX, &mut buf);
        // 4294967295 = 10 digits
        assert_eq!(len, 10);
        assert_eq!(&buf[6..16], b"4294967295");
    }

    // -- setlogmask tests --

    #[test]
    fn test_setlogmask_returns_previous() {
        // Reset to known state.
        unsafe {
            core::ptr::addr_of_mut!(SYSLOG_MASK).write(0xFF);
        }

        let old = setlogmask(log_upto(LOG_ERR));
        assert_eq!(old, 0xFF);

        let old2 = setlogmask(0xFF);
        assert_eq!(old2, log_upto(LOG_ERR));
    }

    #[test]
    fn test_setlogmask_zero_queries() {
        // setlogmask(0) queries without changing.
        unsafe {
            core::ptr::addr_of_mut!(SYSLOG_MASK).write(0xFF);
        }
        let mask = setlogmask(0);
        assert_eq!(mask, 0xFF);
        // Should still be 0xFF.
        let mask2 = setlogmask(0);
        assert_eq!(mask2, 0xFF);
    }

    // -- Constants match glibc values --

    #[test]
    fn test_syslog_priority_values() {
        assert_eq!(LOG_EMERG, 0);
        assert_eq!(LOG_ALERT, 1);
        assert_eq!(LOG_CRIT, 2);
        assert_eq!(LOG_ERR, 3);
        assert_eq!(LOG_WARNING, 4);
        assert_eq!(LOG_NOTICE, 5);
        assert_eq!(LOG_INFO, 6);
        assert_eq!(LOG_DEBUG, 7);
    }

    #[test]
    fn test_syslog_facility_values() {
        assert_eq!(LOG_KERN, 0);
        assert_eq!(LOG_USER, 8);
        assert_eq!(LOG_MAIL, 16);
        assert_eq!(LOG_DAEMON, 24);
        assert_eq!(LOG_AUTH, 32);
        assert_eq!(LOG_LOCAL0, 128);
        assert_eq!(LOG_LOCAL7, 184);
    }

    #[test]
    fn test_syslog_option_values() {
        assert_eq!(LOG_PID, 0x01);
        assert_eq!(LOG_NDELAY, 0x08);
        assert_eq!(LOG_PERROR, 0x20);
    }

    // -------------------------------------------------------------------
    // openlog / closelog — state management
    // -------------------------------------------------------------------

    #[test]
    fn test_openlog_sets_ident() {
        let ident = b"test_prog\0";
        openlog(ident.as_ptr(), 0, LOG_USER);
        // Verify ident was stored.
        let stored = unsafe { *core::ptr::addr_of!(SYSLOG_IDENT) };
        assert_eq!(stored, ident.as_ptr());
        // Clean up.
        closelog();
    }

    #[test]
    fn test_openlog_sets_options() {
        openlog(core::ptr::null(), LOG_PID | LOG_PERROR, LOG_DAEMON);
        let stored = unsafe { *core::ptr::addr_of!(SYSLOG_OPTIONS) };
        assert_eq!(stored, LOG_PID | LOG_PERROR);
        closelog();
    }

    #[test]
    fn test_closelog_clears_state() {
        let ident = b"test\0";
        openlog(ident.as_ptr(), LOG_PID, LOG_USER);
        closelog();
        let stored_ident = unsafe { *core::ptr::addr_of!(SYSLOG_IDENT) };
        let stored_opts = unsafe { *core::ptr::addr_of!(SYSLOG_OPTIONS) };
        assert!(stored_ident.is_null());
        assert_eq!(stored_opts, 0);
    }

    #[test]
    fn test_openlog_null_ident() {
        openlog(core::ptr::null(), 0, 0);
        let stored = unsafe { *core::ptr::addr_of!(SYSLOG_IDENT) };
        assert!(stored.is_null());
        closelog();
    }

    // -- Priority and facility encoding --

    #[test]
    fn test_priority_in_low_3_bits() {
        for p in 0..=7 {
            assert_eq!(log_pri(p), p);
        }
    }

    #[test]
    fn test_log_mask_each_priority() {
        for p in 0..=7 {
            let mask = log_mask(p);
            // Only bit p should be set.
            assert_eq!(mask, 1 << p);
        }
    }

    #[test]
    fn test_log_upto_emerg_only() {
        let mask = log_upto(LOG_EMERG);
        assert_ne!(mask & log_mask(LOG_EMERG), 0);
        assert_eq!(mask & log_mask(LOG_ALERT), 0);
    }

    #[test]
    fn test_log_upto_notice() {
        let mask = log_upto(LOG_NOTICE);
        for p in 0..=LOG_NOTICE {
            assert_ne!(mask & log_mask(p), 0, "priority {p} should be in mask");
        }
        assert_eq!(mask & log_mask(LOG_INFO), 0);
        assert_eq!(mask & log_mask(LOG_DEBUG), 0);
    }

    // -- write_u32 edge cases --

    #[test]
    fn test_write_u32_one() {
        let mut buf = [0u8; 16];
        let len = write_u32(1, &mut buf);
        assert_eq!(len, 1);
        assert_eq!(buf[15], b'1');
    }

    #[test]
    fn test_write_u32_ten() {
        let mut buf = [0u8; 16];
        let len = write_u32(10, &mut buf);
        assert_eq!(len, 2);
        assert_eq!(&buf[14..16], b"10");
    }

    #[test]
    fn test_write_u32_hundred() {
        let mut buf = [0u8; 16];
        let len = write_u32(100, &mut buf);
        assert_eq!(len, 3);
        assert_eq!(&buf[13..16], b"100");
    }

    // -- Facility values: all 8-byte aligned --

    #[test]
    fn test_facility_values_alignment() {
        // All facility values should be multiples of 8 (shifted by 3).
        let facilities = [
            LOG_KERN, LOG_USER, LOG_MAIL, LOG_DAEMON, LOG_AUTH, LOG_LOCAL0, LOG_LOCAL7,
        ];
        for f in facilities {
            assert_eq!(f & 0x07, 0, "facility {f} should have low 3 bits zero");
        }
    }

    // -- syslog writes to stderr (no crash tests) --

    #[test]
    fn test_syslog_no_crash() {
        unsafe {
            core::ptr::addr_of_mut!(SYSLOG_MASK).write(0xFF);
        }
        unsafe {
            _syslog_impl(
                LOG_INFO,
                b"test syslog message\0".as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
            );
        }
    }

    #[test]
    fn test_syslog_filtered_by_mask() {
        // Set mask to only allow ERR and below.
        unsafe {
            core::ptr::addr_of_mut!(SYSLOG_MASK).write(log_upto(LOG_ERR) as i32);
        }
        unsafe {
            // This should be filtered out (LOG_INFO > LOG_ERR).
            _syslog_impl(
                LOG_INFO,
                b"this should be filtered\0".as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
            );
            // This should get through.
            _syslog_impl(
                LOG_ERR,
                b"this should print\0".as_ptr(),
                core::ptr::null(),
                core::ptr::null(),
            );
        }
        // Restore.
        unsafe {
            core::ptr::addr_of_mut!(SYSLOG_MASK).write(0xFF);
        }
    }

    #[test]
    fn test_syslog_null_message_no_crash() {
        unsafe {
            core::ptr::addr_of_mut!(SYSLOG_MASK).write(0xFF);
        }
        unsafe {
            _syslog_impl(
                LOG_ERR,
                core::ptr::null(),
                core::ptr::null(),
                core::ptr::null(),
            );
        }
    }

    #[test]
    fn test_syslog_expands_format() {
        // Build a synthetic va_list and verify vsyslog formats without crashing.
        unsafe {
            core::ptr::addr_of_mut!(SYSLOG_MASK).write(0xFF);
        }
        let mut reg = [0u8; 176];
        // First GP arg = 42 (used by %d).
        reg[0..8].copy_from_slice(&42u64.to_le_bytes());
        let mut overflow = [0u8; 128];
        let mut va = VaList {
            gp_offset: 0,
            fp_offset: 48,
            overflow_arg_area: overflow.as_mut_ptr(),
            reg_save_area: reg.as_mut_ptr(),
        };
        unsafe {
            vsyslog(LOG_INFO, b"value=%d\0".as_ptr(), &mut va);
        }
    }

    #[test]
    fn test_vsyslog_null_va_no_crash() {
        unsafe {
            core::ptr::addr_of_mut!(SYSLOG_MASK).write(0xFF);
        }
        unsafe {
            vsyslog(LOG_INFO, b"hi\0".as_ptr(), core::ptr::null_mut());
        }
    }

    #[test]
    fn test_vsyslog_chk_expands_format() {
        // The _FORTIFY_SOURCE form: extra `flag` arg, otherwise like vsyslog.
        unsafe {
            core::ptr::addr_of_mut!(SYSLOG_MASK).write(0xFF);
        }
        let mut reg = [0u8; 176];
        reg[0..8].copy_from_slice(&7u64.to_le_bytes());
        let mut overflow = [0u8; 128];
        let mut va = VaList {
            gp_offset: 0,
            fp_offset: 48,
            overflow_arg_area: overflow.as_mut_ptr(),
            reg_save_area: reg.as_mut_ptr(),
        };
        unsafe {
            __vsyslog_chk(LOG_INFO, 1, b"step=%d\0".as_ptr(), &mut va);
        }
    }

    // -- setlogmask with specific masks --

    #[test]
    fn test_setlogmask_specific_mask() {
        unsafe {
            core::ptr::addr_of_mut!(SYSLOG_MASK).write(0xFF);
        }
        // Set to only LOG_ERR.
        let old = setlogmask(log_mask(LOG_ERR) as i32);
        assert_eq!(old, 0xFF);
        let current = setlogmask(0); // Query.
        assert_eq!(current, log_mask(LOG_ERR) as i32);
        // Restore.
        setlogmask(0xFF);
    }
}
