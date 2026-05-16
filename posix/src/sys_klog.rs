//! `<sys/klog.h>` — kernel log control.
//!
//! Re-exports `klogctl()` from the `unistd` module and provides
//! the SYSLOG_ACTION_* constants for the `type` argument.

pub use crate::unistd::klogctl;

// ---------------------------------------------------------------------------
// SYSLOG_ACTION_* constants (type argument to klogctl)
// ---------------------------------------------------------------------------

/// Close the log (currently a no-op).
pub const SYSLOG_ACTION_CLOSE: i32 = 0;
/// Open the log (currently a no-op).
pub const SYSLOG_ACTION_OPEN: i32 = 1;
/// Read from the log.
pub const SYSLOG_ACTION_READ: i32 = 2;
/// Read all messages remaining in the ring buffer.
pub const SYSLOG_ACTION_READ_ALL: i32 = 3;
/// Read and clear all messages.
pub const SYSLOG_ACTION_READ_CLEAR: i32 = 4;
/// Clear ring buffer.
pub const SYSLOG_ACTION_CLEAR: i32 = 5;
/// Disable printk to console.
pub const SYSLOG_ACTION_CONSOLE_OFF: i32 = 6;
/// Enable printk to console.
pub const SYSLOG_ACTION_CONSOLE_ON: i32 = 7;
/// Set console log level.
pub const SYSLOG_ACTION_CONSOLE_LEVEL: i32 = 8;
/// Return number of unread characters.
pub const SYSLOG_ACTION_SIZE_UNREAD: i32 = 9;
/// Return size of the log buffer.
pub const SYSLOG_ACTION_SIZE_BUFFER: i32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_constants() {
        assert_eq!(SYSLOG_ACTION_CLOSE, 0);
        assert_eq!(SYSLOG_ACTION_OPEN, 1);
        assert_eq!(SYSLOG_ACTION_SIZE_BUFFER, 10);
    }

    #[test]
    fn test_actions_sequential() {
        let actions = [
            SYSLOG_ACTION_CLOSE, SYSLOG_ACTION_OPEN, SYSLOG_ACTION_READ,
            SYSLOG_ACTION_READ_ALL, SYSLOG_ACTION_READ_CLEAR,
            SYSLOG_ACTION_CLEAR, SYSLOG_ACTION_CONSOLE_OFF,
            SYSLOG_ACTION_CONSOLE_ON, SYSLOG_ACTION_CONSOLE_LEVEL,
            SYSLOG_ACTION_SIZE_UNREAD, SYSLOG_ACTION_SIZE_BUFFER,
        ];
        for i in 0..actions.len() {
            assert_eq!(actions[i], i as i32, "SYSLOG_ACTION values should be sequential");
        }
    }

    #[test]
    fn test_klogctl_stub() {
        assert_eq!(klogctl(0, core::ptr::null_mut(), 0), -1);
    }

    #[test]
    fn test_cross_module() {
        // klogctl should be the same as crate::unistd::klogctl
        let ptr = klogctl as *const ();
        let expected = crate::unistd::klogctl as *const ();
        assert_eq!(ptr, expected);
    }
}
