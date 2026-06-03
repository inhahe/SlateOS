//! `<syslog.h>` — BSD/POSIX syslog API constants.
//!
//! `openlog()`, `syslog()`, `closelog()`, and `setlogmask()`
//! provide the standard userspace logging interface.  These
//! constants define facility codes, severity levels, and
//! option flags for the syslog API.

// ---------------------------------------------------------------------------
// Syslog severity levels (LOG_*)
// ---------------------------------------------------------------------------

/// Emergency: system is unusable.
pub const LOG_EMERG: u32 = 0;
/// Alert: action must be taken immediately.
pub const LOG_ALERT: u32 = 1;
/// Critical: critical conditions.
pub const LOG_CRIT: u32 = 2;
/// Error: error conditions.
pub const LOG_ERR: u32 = 3;
/// Warning: warning conditions.
pub const LOG_WARNING: u32 = 4;
/// Notice: normal but significant.
pub const LOG_NOTICE: u32 = 5;
/// Info: informational messages.
pub const LOG_INFO: u32 = 6;
/// Debug: debug-level messages.
pub const LOG_DEBUG: u32 = 7;

// ---------------------------------------------------------------------------
// openlog() option flags (logopt parameter)
// ---------------------------------------------------------------------------

/// Log the PID with each message.
pub const LOG_PID: u32 = 0x01;
/// Log to stderr as well.
pub const LOG_CONS: u32 = 0x02;
/// Open connection immediately (not lazy).
pub const LOG_ODELAY: u32 = 0x04;
/// Open connection immediately.
pub const LOG_NDELAY: u32 = 0x08;
/// Do not wait for child processes.
pub const LOG_NOWAIT: u32 = 0x10;
/// Log to stderr (glibc extension).
pub const LOG_PERROR: u32 = 0x20;

// ---------------------------------------------------------------------------
// setlogmask() helpers
// ---------------------------------------------------------------------------

/// Mask for a single priority level.
pub const LOG_MASK_SHIFT: u32 = 1;
/// Number of priority levels.
pub const LOG_NLEVELS: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levels_distinct() {
        let levels = [
            LOG_EMERG,
            LOG_ALERT,
            LOG_CRIT,
            LOG_ERR,
            LOG_WARNING,
            LOG_NOTICE,
            LOG_INFO,
            LOG_DEBUG,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_emerg_is_zero() {
        assert_eq!(LOG_EMERG, 0);
    }

    #[test]
    fn test_debug_is_seven() {
        assert_eq!(LOG_DEBUG, 7);
    }

    #[test]
    fn test_option_flags_no_overlap() {
        let flags = [
            LOG_PID, LOG_CONS, LOG_ODELAY, LOG_NDELAY, LOG_NOWAIT, LOG_PERROR,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_pid_is_one() {
        assert_eq!(LOG_PID, 1);
    }

    #[test]
    fn test_nlevels() {
        assert_eq!(LOG_NLEVELS, 8);
    }
}
