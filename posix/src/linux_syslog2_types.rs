//! `<linux/syslog.h>` — Additional syslog constants.
//!
//! Supplementary syslog constants covering action types,
//! log levels, and facility codes.

// ---------------------------------------------------------------------------
// Syslog action types (SYSLOG_ACTION_*)
// ---------------------------------------------------------------------------

/// Close log.
pub const SYSLOG_ACTION_CLOSE: u32 = 0;
/// Open log.
pub const SYSLOG_ACTION_OPEN: u32 = 1;
/// Read from log.
pub const SYSLOG_ACTION_READ: u32 = 2;
/// Read all (ring buffer).
pub const SYSLOG_ACTION_READ_ALL: u32 = 3;
/// Read and clear.
pub const SYSLOG_ACTION_READ_CLEAR: u32 = 4;
/// Clear ring buffer.
pub const SYSLOG_ACTION_CLEAR: u32 = 5;
/// Console off.
pub const SYSLOG_ACTION_CONSOLE_OFF: u32 = 6;
/// Console on.
pub const SYSLOG_ACTION_CONSOLE_ON: u32 = 7;
/// Console level.
pub const SYSLOG_ACTION_CONSOLE_LEVEL: u32 = 8;
/// Size unread.
pub const SYSLOG_ACTION_SIZE_UNREAD: u32 = 9;
/// Size buffer.
pub const SYSLOG_ACTION_SIZE_BUFFER: u32 = 10;

// ---------------------------------------------------------------------------
// Log levels (KERN_*)
// ---------------------------------------------------------------------------

/// Emergency.
pub const KERN_EMERG: u32 = 0;
/// Alert.
pub const KERN_ALERT: u32 = 1;
/// Critical.
pub const KERN_CRIT: u32 = 2;
/// Error.
pub const KERN_ERR: u32 = 3;
/// Warning.
pub const KERN_WARNING: u32 = 4;
/// Notice.
pub const KERN_NOTICE: u32 = 5;
/// Informational.
pub const KERN_INFO: u32 = 6;
/// Debug.
pub const KERN_DEBUG: u32 = 7;

// ---------------------------------------------------------------------------
// Log facilities (LOG_*)
// ---------------------------------------------------------------------------

/// Kernel messages.
pub const LOG_KERN: u32 = 0 << 3;
/// User-level messages.
pub const LOG_USER: u32 = 1 << 3;
/// Mail system.
pub const LOG_MAIL: u32 = 2 << 3;
/// System daemons.
pub const LOG_DAEMON: u32 = 3 << 3;
/// Security/auth.
pub const LOG_AUTH: u32 = 4 << 3;
/// Syslogd internal.
pub const LOG_SYSLOG: u32 = 5 << 3;
/// Printer.
pub const LOG_LPR: u32 = 6 << 3;
/// News.
pub const LOG_NEWS: u32 = 7 << 3;
/// UUCP.
pub const LOG_UUCP: u32 = 8 << 3;
/// Clock daemon.
pub const LOG_CRON: u32 = 9 << 3;
/// Auth (private).
pub const LOG_AUTHPRIV: u32 = 10 << 3;
/// FTP.
pub const LOG_FTP: u32 = 11 << 3;
/// Local 0.
pub const LOG_LOCAL0: u32 = 16 << 3;
/// Local 7.
pub const LOG_LOCAL7: u32 = 23 << 3;

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
    fn test_log_levels_sequential() {
        assert_eq!(KERN_EMERG, 0);
        assert_eq!(KERN_DEBUG, 7);
        assert!(KERN_EMERG < KERN_ALERT);
        assert!(KERN_ALERT < KERN_CRIT);
        assert!(KERN_CRIT < KERN_ERR);
    }

    #[test]
    fn test_facilities_distinct() {
        let facs = [
            LOG_KERN,
            LOG_USER,
            LOG_MAIL,
            LOG_DAEMON,
            LOG_AUTH,
            LOG_SYSLOG,
            LOG_LPR,
            LOG_NEWS,
            LOG_UUCP,
            LOG_CRON,
            LOG_AUTHPRIV,
            LOG_FTP,
            LOG_LOCAL0,
            LOG_LOCAL7,
        ];
        for i in 0..facs.len() {
            for j in (i + 1)..facs.len() {
                assert_ne!(facs[i], facs[j]);
            }
        }
    }

    #[test]
    fn test_facility_encoding() {
        // Facility is encoded shifted left by 3
        assert_eq!(LOG_KERN, 0);
        assert_eq!(LOG_USER, 8);
        assert_eq!(LOG_MAIL, 16);
    }
}
