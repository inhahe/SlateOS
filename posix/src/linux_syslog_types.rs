//! `<linux/syslog.h>` — Kernel syslog (printk ring buffer) command constants.
//!
//! The `syslog()` syscall (not to be confused with the C library
//! syslog) accesses the kernel's printk ring buffer. These
//! constants define the command codes for reading, clearing, and
//! configuring the kernel log.

// ---------------------------------------------------------------------------
// syslog() commands (type argument)
// ---------------------------------------------------------------------------

/// Close the log (unused on Linux).
pub const SYSLOG_ACTION_CLOSE: u32 = 0;
/// Open the log (unused on Linux).
pub const SYSLOG_ACTION_OPEN: u32 = 1;
/// Read from the log (blocking, consumes messages).
pub const SYSLOG_ACTION_READ: u32 = 2;
/// Read all messages remaining in ring buffer.
pub const SYSLOG_ACTION_READ_ALL: u32 = 3;
/// Read and clear all messages.
pub const SYSLOG_ACTION_READ_CLEAR: u32 = 4;
/// Clear the ring buffer.
pub const SYSLOG_ACTION_CLEAR: u32 = 5;
/// Disable printk to console.
pub const SYSLOG_ACTION_CONSOLE_OFF: u32 = 6;
/// Enable printk to console.
pub const SYSLOG_ACTION_CONSOLE_ON: u32 = 7;
/// Set console log level.
pub const SYSLOG_ACTION_CONSOLE_LEVEL: u32 = 8;
/// Return number of unread characters.
pub const SYSLOG_ACTION_SIZE_UNREAD: u32 = 9;
/// Return total size of the log buffer.
pub const SYSLOG_ACTION_SIZE_BUFFER: u32 = 10;

// ---------------------------------------------------------------------------
// Console log levels (for SYSLOG_ACTION_CONSOLE_LEVEL)
// ---------------------------------------------------------------------------

/// Minimum console log level.
pub const CONSOLE_LOGLEVEL_MIN: u32 = 1;
/// Default console log level.
pub const CONSOLE_LOGLEVEL_DEFAULT: u32 = 7;
/// Quiet boot log level.
pub const CONSOLE_LOGLEVEL_QUIET: u32 = 4;
/// Debug log level (all messages).
pub const CONSOLE_LOGLEVEL_DEBUG: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            SYSLOG_ACTION_CLOSE,
            SYSLOG_ACTION_OPEN,
            SYSLOG_ACTION_READ,
            SYSLOG_ACTION_READ_ALL,
            SYSLOG_ACTION_READ_CLEAR,
            SYSLOG_ACTION_CLEAR,
            SYSLOG_ACTION_CONSOLE_OFF,
            SYSLOG_ACTION_CONSOLE_ON,
            SYSLOG_ACTION_CONSOLE_LEVEL,
            SYSLOG_ACTION_SIZE_UNREAD,
            SYSLOG_ACTION_SIZE_BUFFER,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_close_is_zero() {
        assert_eq!(SYSLOG_ACTION_CLOSE, 0);
    }

    #[test]
    fn test_loglevel_ordering() {
        assert!(CONSOLE_LOGLEVEL_MIN < CONSOLE_LOGLEVEL_QUIET);
        assert!(CONSOLE_LOGLEVEL_QUIET < CONSOLE_LOGLEVEL_DEFAULT);
        assert!(CONSOLE_LOGLEVEL_DEFAULT < CONSOLE_LOGLEVEL_DEBUG);
    }

    #[test]
    fn test_loglevel_values() {
        assert_eq!(CONSOLE_LOGLEVEL_MIN, 1);
        assert_eq!(CONSOLE_LOGLEVEL_DEFAULT, 7);
    }
}
