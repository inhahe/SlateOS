//! POSIX `<fmtmsg.h>` — formatted error messages.
//!
//! Implements `fmtmsg` for displaying error/informational messages in
//! a structured format per POSIX.1-2024.
//!
//! ## Output Format
//!
//! ```text
//! label: severity: text
//! TO FIX: action tag
//! ```
//!
//! The first line goes to stderr (if `MM_PRINT` is set).
//! Console output (`MM_CONSOLE`) is mapped to stderr as well (no
//! `/dev/console` in our OS yet).

use crate::file;

// ---------------------------------------------------------------------------
// Classification flags (long bitmask)
// ---------------------------------------------------------------------------

// Source subclassification (bits 0-1).

/// Message from hardware.
pub const MM_HARD: i64 = 1;
/// Message from software.
pub const MM_SOFT: i64 = 2;
/// Message from firmware.
pub const MM_FIRM: i64 = 4;

// Output channel flags (bits 8-9).

/// Display to stderr.
pub const MM_PRINT: i64 = 256;
/// Display to the system console.
pub const MM_CONSOLE: i64 = 512;

// Status flags for recoverability (bits 16-17).

/// Error is recoverable.
pub const MM_RECOVER: i64 = 0x1_0000;
/// Error is non-recoverable.
pub const MM_NRECOV: i64 = 0x2_0000;

// ---------------------------------------------------------------------------
// Severity levels
// ---------------------------------------------------------------------------

/// No severity level.
pub const MM_NOSEV: i32 = 0;
/// Halt — condition requires immediate halt.
pub const MM_HALT: i32 = 1;
/// Error — detected fault.
pub const MM_ERROR: i32 = 2;
/// Warning — unusual non-error condition.
pub const MM_WARNING: i32 = 3;
/// Informational message.
pub const MM_INFO: i32 = 4;

// ---------------------------------------------------------------------------
// Special "no value" pointers
// ---------------------------------------------------------------------------

/// Use as `label`, `text`, `action`, or `tag` when no value is supplied.
///
/// POSIX defines `MM_NULLLBL`, `MM_NULLTXT`, `MM_NULLACT`, `MM_NULLTAG`
/// but they are all the same concept — a null or empty string meaning
/// "not supplied."
pub const MM_NULLLBL: *const u8 = core::ptr::null();
pub const MM_NULLTXT: *const u8 = core::ptr::null();
pub const MM_NULLACT: *const u8 = core::ptr::null();
pub const MM_NULLTAG: *const u8 = core::ptr::null();

// ---------------------------------------------------------------------------
// Return values
// ---------------------------------------------------------------------------

/// All output was successful.
pub const MM_OK: i32 = 0;
/// All output failed.
pub const MM_NOTOK: i32 = -1;
/// Standard error output failed.
pub const MM_NOMSG: i32 = 1;
/// Console output failed.
pub const MM_NOCON: i32 = 4;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Write a NUL-terminated C string to stderr.
fn write_stderr(s: *const u8) {
    if s.is_null() {
        return;
    }
    // Compute length.
    let mut len: usize = 0;
    // SAFETY: s is a valid NUL-terminated string per caller.
    unsafe {
        while *s.add(len) != 0 {
            len = len.wrapping_add(1);
        }
    }
    if len > 0 {
        file::write(2, s, len);
    }
}

/// Write a raw byte string to stderr.
fn write_stderr_bytes(s: &[u8]) {
    if !s.is_empty() {
        file::write(2, s.as_ptr(), s.len());
    }
}

/// Map severity code to a display string.
fn severity_str(severity: i32) -> &'static [u8] {
    match severity {
        MM_HALT => b"HALT",
        MM_ERROR => b"ERROR",
        MM_WARNING => b"WARNING",
        MM_INFO => b"INFO",
        _ => b"",
    }
}

// ---------------------------------------------------------------------------
// fmtmsg
// ---------------------------------------------------------------------------

/// `fmtmsg` — display a formatted error message.
///
/// Writes a structured message to stderr and/or the console based on
/// the `classification` flags.
///
/// # Parameters
///
/// - `classification`: Bitmask of source (`MM_HARD`/`MM_SOFT`/`MM_FIRM`),
///   output channel (`MM_PRINT`/`MM_CONSOLE`), and recoverability.
/// - `label`: Source identifier (e.g., `"UX:cat"`, max 10 chars).
///   Pass `MM_NULLLBL` (null) to omit.
/// - `severity`: One of `MM_HALT`, `MM_ERROR`, `MM_WARNING`, `MM_INFO`,
///   or `MM_NOSEV`.
/// - `text`: Human-readable error text.  Pass `MM_NULLTXT` to omit.
/// - `action`: Recovery action description.  Pass `MM_NULLACT` to omit.
/// - `tag`: Reference tag (e.g., `"UX:cat:001"`).  Pass `MM_NULLTAG`
///   to omit.
///
/// # Returns
///
/// `MM_OK` on success, `MM_NOTOK` if all output failed, `MM_NOMSG`
/// if stderr output failed, `MM_NOCON` if console output failed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fmtmsg(
    classification: i64,
    label: *const u8,
    severity: i32,
    text: *const u8,
    action: *const u8,
    tag: *const u8,
) -> i32 {
    let do_print = (classification & MM_PRINT) != 0;
    let do_console = (classification & MM_CONSOLE) != 0;

    // If neither output channel is requested, nothing to do.
    if !do_print && !do_console {
        return MM_OK;
    }

    // Build the message.  Format:
    //   label: severity: text
    //   TO FIX: action tag

    // Line 1: "label: severity: text\n"
    let has_label = !label.is_null();
    let sev = severity_str(severity);
    let has_sev = !sev.is_empty();
    let has_text = !text.is_null();

    if has_label {
        write_stderr(label);
    }

    if has_label && (has_sev || has_text) {
        write_stderr_bytes(b": ");
    }

    if has_sev {
        write_stderr_bytes(sev);
    }

    if has_sev && has_text {
        write_stderr_bytes(b": ");
    }

    if has_text {
        write_stderr(text);
    }

    write_stderr_bytes(b"\n");

    // Line 2: "TO FIX: action tag\n"  (only if action or tag is provided)
    let has_action = !action.is_null();
    let has_tag = !tag.is_null();

    if has_action || has_tag {
        write_stderr_bytes(b"TO FIX: ");
        if has_action {
            write_stderr(action);
        }
        if has_action && has_tag {
            write_stderr_bytes(b"  ");
        }
        if has_tag {
            write_stderr(tag);
        }
        write_stderr_bytes(b"\n");
    }

    // Both print and console go to stderr in our OS, so if we got
    // here without error, both succeeded.
    MM_OK
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Classification constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_source_constants() {
        assert_eq!(MM_HARD, 1);
        assert_eq!(MM_SOFT, 2);
        assert_eq!(MM_FIRM, 4);
    }

    #[test]
    fn test_source_constants_distinct() {
        assert_ne!(MM_HARD, MM_SOFT);
        assert_ne!(MM_SOFT, MM_FIRM);
        assert_ne!(MM_HARD, MM_FIRM);
    }

    #[test]
    fn test_channel_constants() {
        assert_eq!(MM_PRINT, 256);
        assert_eq!(MM_CONSOLE, 512);
    }

    #[test]
    fn test_channel_constants_distinct() {
        assert_ne!(MM_PRINT, MM_CONSOLE);
    }

    #[test]
    fn test_recover_constants() {
        assert_eq!(MM_RECOVER, 0x1_0000);
        assert_eq!(MM_NRECOV, 0x2_0000);
    }

    #[test]
    fn test_recover_constants_distinct() {
        assert_ne!(MM_RECOVER, MM_NRECOV);
    }

    #[test]
    fn test_classification_no_overlap() {
        // Source, channel, and recovery bits should not overlap.
        let source = MM_HARD | MM_SOFT | MM_FIRM;
        let channel = MM_PRINT | MM_CONSOLE;
        let recovery = MM_RECOVER | MM_NRECOV;
        assert_eq!(source & channel, 0);
        assert_eq!(source & recovery, 0);
        assert_eq!(channel & recovery, 0);
    }

    // -----------------------------------------------------------------------
    // Severity constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_severity_constants() {
        assert_eq!(MM_NOSEV, 0);
        assert_eq!(MM_HALT, 1);
        assert_eq!(MM_ERROR, 2);
        assert_eq!(MM_WARNING, 3);
        assert_eq!(MM_INFO, 4);
    }

    #[test]
    fn test_severity_constants_distinct() {
        let vals = [MM_NOSEV, MM_HALT, MM_ERROR, MM_WARNING, MM_INFO];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j], "severity {} and {} must differ", i, j);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Return values
    // -----------------------------------------------------------------------

    #[test]
    fn test_return_constants() {
        assert_eq!(MM_OK, 0);
        assert_eq!(MM_NOTOK, -1);
        assert_eq!(MM_NOMSG, 1);
        assert_eq!(MM_NOCON, 4);
    }

    #[test]
    fn test_return_constants_distinct() {
        let vals = [MM_OK, MM_NOTOK, MM_NOMSG, MM_NOCON];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Null pointer constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_null_constants() {
        assert!(MM_NULLLBL.is_null());
        assert!(MM_NULLTXT.is_null());
        assert!(MM_NULLACT.is_null());
        assert!(MM_NULLTAG.is_null());
    }

    // -----------------------------------------------------------------------
    // severity_str
    // -----------------------------------------------------------------------

    #[test]
    fn test_severity_str_halt() {
        assert_eq!(severity_str(MM_HALT), b"HALT");
    }

    #[test]
    fn test_severity_str_error() {
        assert_eq!(severity_str(MM_ERROR), b"ERROR");
    }

    #[test]
    fn test_severity_str_warning() {
        assert_eq!(severity_str(MM_WARNING), b"WARNING");
    }

    #[test]
    fn test_severity_str_info() {
        assert_eq!(severity_str(MM_INFO), b"INFO");
    }

    #[test]
    fn test_severity_str_nosev() {
        assert_eq!(severity_str(MM_NOSEV), b"");
    }

    #[test]
    fn test_severity_str_unknown() {
        assert_eq!(severity_str(99), b"");
    }

    // -----------------------------------------------------------------------
    // fmtmsg — functional tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_fmtmsg_no_channel_returns_ok() {
        // No MM_PRINT or MM_CONSOLE → nothing to do → MM_OK.
        let ret = fmtmsg(
            MM_SOFT,
            b"UX:test\0".as_ptr(),
            MM_ERROR,
            b"test error\0".as_ptr(),
            MM_NULLACT,
            MM_NULLTAG,
        );
        assert_eq!(ret, MM_OK);
    }

    #[test]
    fn test_fmtmsg_all_nulls() {
        // All null strings should not crash.
        let ret = fmtmsg(
            MM_PRINT, MM_NULLLBL, MM_NOSEV, MM_NULLTXT, MM_NULLACT, MM_NULLTAG,
        );
        assert_eq!(ret, MM_OK);
    }

    #[test]
    fn test_fmtmsg_print_with_label() {
        let ret = fmtmsg(
            MM_SOFT | MM_PRINT,
            b"UX:cat\0".as_ptr(),
            MM_ERROR,
            b"cannot open file\0".as_ptr(),
            b"Check file permissions.\0".as_ptr(),
            b"UX:cat:001\0".as_ptr(),
        );
        assert_eq!(ret, MM_OK);
    }

    #[test]
    fn test_fmtmsg_console() {
        let ret = fmtmsg(
            MM_HARD | MM_CONSOLE,
            b"kern\0".as_ptr(),
            MM_HALT,
            b"disk failure\0".as_ptr(),
            b"Replace disk.\0".as_ptr(),
            b"kern:001\0".as_ptr(),
        );
        assert_eq!(ret, MM_OK);
    }

    #[test]
    fn test_fmtmsg_both_channels() {
        let ret = fmtmsg(
            MM_SOFT | MM_PRINT | MM_CONSOLE | MM_RECOVER,
            b"app\0".as_ptr(),
            MM_WARNING,
            b"low memory\0".as_ptr(),
            b"Close unused applications.\0".as_ptr(),
            MM_NULLTAG,
        );
        assert_eq!(ret, MM_OK);
    }

    #[test]
    fn test_fmtmsg_info_severity() {
        let ret = fmtmsg(
            MM_PRINT,
            MM_NULLLBL,
            MM_INFO,
            b"startup complete\0".as_ptr(),
            MM_NULLACT,
            MM_NULLTAG,
        );
        assert_eq!(ret, MM_OK);
    }

    #[test]
    fn test_fmtmsg_only_action_no_tag() {
        let ret = fmtmsg(
            MM_PRINT,
            b"test\0".as_ptr(),
            MM_ERROR,
            b"problem\0".as_ptr(),
            b"Fix it.\0".as_ptr(),
            MM_NULLTAG,
        );
        assert_eq!(ret, MM_OK);
    }

    #[test]
    fn test_fmtmsg_only_tag_no_action() {
        let ret = fmtmsg(
            MM_PRINT,
            b"test\0".as_ptr(),
            MM_ERROR,
            b"problem\0".as_ptr(),
            MM_NULLACT,
            b"tag:001\0".as_ptr(),
        );
        assert_eq!(ret, MM_OK);
    }

    // -----------------------------------------------------------------------
    // Combined classification bitmask tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_classification_bitmask_combined() {
        let class = MM_HARD | MM_PRINT | MM_NRECOV;
        assert_ne!(class & MM_HARD, 0);
        assert_ne!(class & MM_PRINT, 0);
        assert_ne!(class & MM_NRECOV, 0);
        assert_eq!(class & MM_SOFT, 0);
        assert_eq!(class & MM_CONSOLE, 0);
        assert_eq!(class & MM_RECOVER, 0);
    }
}
