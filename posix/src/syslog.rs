//! POSIX system logging.
//!
//! Implements `openlog`, `syslog`, `closelog`, `setlogmask` stubs.
//!
//! ## Implementation
//!
//! Our OS doesn't have a syslog daemon.  Messages are written to stderr
//! (fd 2) with a priority prefix so they're visible on the console.
//!
//! ## Limitations
//!
//! - No actual syslog daemon or log rotation.
//! - `openlog` ident string is stored but the facility is ignored.
//! - `setlogmask` filtering works correctly.

use crate::file;
use crate::string;

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
#[unsafe(no_mangle)]
pub extern "C" fn openlog(ident: *const u8, option: i32, _facility: i32) {
    // SAFETY: Single-threaded access to static state.
    unsafe {
        core::ptr::addr_of_mut!(SYSLOG_IDENT).write(ident);
        core::ptr::addr_of_mut!(SYSLOG_OPTIONS).write(option);
    }
}

/// Write a message to the system log.
///
/// This simplified version writes the message directly to stderr (fd 2).
/// The `priority` parameter combines facility and level.
///
/// Note: This does NOT support printf-style format strings in `msg`.
/// The caller should format the message before calling syslog, or use
/// our printf to format into a buffer first.
#[unsafe(no_mangle)]
pub extern "C" fn syslog(priority: i32, msg: *const u8) {
    let pri = log_pri(priority);

    // Check log mask.
    let mask = unsafe { *core::ptr::addr_of!(SYSLOG_MASK) };
    if mask & (1 << pri) == 0 {
        return; // Filtered out.
    }

    if msg.is_null() {
        return;
    }

    // Write priority prefix.
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

    // Write the message.
    let msg_len = unsafe { string::strlen(msg) };
    file::write(fd, msg, msg_len);
    file::write(fd, b"\n".as_ptr(), 1);
}

/// Close the connection to the system logger.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn setlogmask(mask: i32) -> i32 {
    let old = unsafe { *core::ptr::addr_of!(SYSLOG_MASK) };
    if mask != 0 {
        unsafe { core::ptr::addr_of_mut!(SYSLOG_MASK).write(mask); }
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
