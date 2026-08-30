//! `<sys/klog.h>` — Kernel log (klogctl/syslog) constants.
//!
//! `klogctl()` (the `syslog()` syscall) provides access to the
//! kernel ring buffer.  These constants define the command codes
//! and buffer size limits.

// ---------------------------------------------------------------------------
// klogctl/syslog commands (type parameter)
// ---------------------------------------------------------------------------

/// Close the log (unused on Linux).
pub const SYSLOG_ACTION_CLOSE: u32 = 0;
/// Open the log (unused on Linux).
pub const SYSLOG_ACTION_OPEN: u32 = 1;
/// Read from the kernel log.
pub const SYSLOG_ACTION_READ: u32 = 2;
/// Read all messages remaining in the ring buffer.
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
/// Return number of unread characters in the log.
pub const SYSLOG_ACTION_SIZE_UNREAD: u32 = 9;
/// Return total size of the ring buffer.
pub const SYSLOG_ACTION_SIZE_BUFFER: u32 = 10;

// ---------------------------------------------------------------------------
// Ring buffer sizes
// ---------------------------------------------------------------------------

/// Default kernel ring buffer size (bytes, CONFIG_LOG_BUF_SHIFT=17).
pub const KLOG_BUF_DEFAULT: u32 = 131072; // 128 KiB
/// Minimum kernel ring buffer size.
pub const KLOG_BUF_MIN: u32 = 4096;
/// Maximum kernel ring buffer size.
pub const KLOG_BUF_MAX: u32 = 2097152; // 2 MiB

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let actions = [
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
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_close_is_zero() {
        assert_eq!(SYSLOG_ACTION_CLOSE, 0);
    }

    #[test]
    fn test_read_is_two() {
        assert_eq!(SYSLOG_ACTION_READ, 2);
    }

    #[test]
    fn test_buffer_sizes() {
        assert!(KLOG_BUF_MIN < KLOG_BUF_DEFAULT);
        assert!(KLOG_BUF_DEFAULT < KLOG_BUF_MAX);
    }

    #[test]
    fn test_default_buf_size() {
        assert_eq!(KLOG_BUF_DEFAULT, 131072);
    }
}
