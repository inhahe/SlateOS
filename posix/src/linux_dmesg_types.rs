//! Kernel message (dmesg) formatting and level constants.
//!
//! The kernel log uses structured messages with facility and
//! severity levels.  These constants define the format for
//! parsing /dev/kmsg output and dmesg-style log entries.

// ---------------------------------------------------------------------------
// Kernel log levels (severity, low 3 bits of priority)
// ---------------------------------------------------------------------------

/// Emergency: system is unusable.
pub const KERN_EMERG: u32 = 0;
/// Alert: action must be taken immediately.
pub const KERN_ALERT: u32 = 1;
/// Critical: critical conditions.
pub const KERN_CRIT: u32 = 2;
/// Error: error conditions.
pub const KERN_ERR: u32 = 3;
/// Warning: warning conditions.
pub const KERN_WARNING: u32 = 4;
/// Notice: normal but significant.
pub const KERN_NOTICE: u32 = 5;
/// Info: informational messages.
pub const KERN_INFO: u32 = 6;
/// Debug: debug-level messages.
pub const KERN_DEBUG: u32 = 7;

/// Number of kernel log levels.
pub const KERN_LEVEL_COUNT: u32 = 8;

// ---------------------------------------------------------------------------
// Default console log level
// ---------------------------------------------------------------------------

/// Default console log level (messages at this level and below are shown).
pub const CONSOLE_LOGLEVEL_DEFAULT: u32 = 7;
/// Minimum console log level.
pub const CONSOLE_LOGLEVEL_MIN: u32 = 1;
/// Quiet console log level (only EMERG).
pub const CONSOLE_LOGLEVEL_QUIET: u32 = 4;

// ---------------------------------------------------------------------------
// /dev/kmsg record format
// ---------------------------------------------------------------------------

/// Maximum length of a single kmsg record (bytes).
pub const KMSG_RECORD_MAX: u32 = 8192;
/// Separator between kmsg fields.
pub const KMSG_FIELD_SEP: u8 = b',';
/// Terminator for kmsg records.
pub const KMSG_RECORD_TERM: u8 = b'\n';
/// Prefix for kmsg subsystem info.
pub const KMSG_SUBSYS_PREFIX: u8 = b' ';

// ---------------------------------------------------------------------------
// dmesg timestamp format
// ---------------------------------------------------------------------------

/// Timestamp precision (microseconds, 6 decimal digits).
pub const DMESG_TIMESTAMP_PRECISION: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levels_distinct() {
        let levels = [
            KERN_EMERG,
            KERN_ALERT,
            KERN_CRIT,
            KERN_ERR,
            KERN_WARNING,
            KERN_NOTICE,
            KERN_INFO,
            KERN_DEBUG,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_emerg_is_zero() {
        assert_eq!(KERN_EMERG, 0);
    }

    #[test]
    fn test_debug_is_seven() {
        assert_eq!(KERN_DEBUG, 7);
    }

    #[test]
    fn test_level_count() {
        assert_eq!(KERN_LEVEL_COUNT, 8);
    }

    #[test]
    fn test_console_levels() {
        assert!(CONSOLE_LOGLEVEL_MIN < CONSOLE_LOGLEVEL_QUIET);
        assert!(CONSOLE_LOGLEVEL_QUIET < CONSOLE_LOGLEVEL_DEFAULT);
    }

    #[test]
    fn test_kmsg_record_max() {
        assert_eq!(KMSG_RECORD_MAX, 8192);
    }

    #[test]
    fn test_kmsg_separators() {
        assert_eq!(KMSG_FIELD_SEP, b',');
        assert_eq!(KMSG_RECORD_TERM, b'\n');
    }

    #[test]
    fn test_timestamp_precision() {
        assert_eq!(DMESG_TIMESTAMP_PRECISION, 6);
    }
}
