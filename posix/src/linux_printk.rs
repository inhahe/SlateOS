//! `<linux/printk.h>` — Kernel log level constants.
//!
//! printk is the kernel's logging facility. Messages are tagged
//! with a priority level and routed to the kernel ring buffer,
//! console, and/or syslog. The log level determines visibility
//! and urgency.

// ---------------------------------------------------------------------------
// Log levels (KERN_*)
// ---------------------------------------------------------------------------

/// Emergency — system is unusable.
pub const KERN_EMERG: u32 = 0;
/// Alert — action must be taken immediately.
pub const KERN_ALERT: u32 = 1;
/// Critical — critical conditions.
pub const KERN_CRIT: u32 = 2;
/// Error — error conditions.
pub const KERN_ERR: u32 = 3;
/// Warning — warning conditions.
pub const KERN_WARNING: u32 = 4;
/// Notice — normal but significant condition.
pub const KERN_NOTICE: u32 = 5;
/// Info — informational.
pub const KERN_INFO: u32 = 6;
/// Debug — debug-level messages.
pub const KERN_DEBUG: u32 = 7;

/// Default log level.
pub const KERN_DEFAULT: u32 = KERN_WARNING;

/// Number of log levels.
pub const KERN_LEVEL_COUNT: u32 = 8;

// ---------------------------------------------------------------------------
// Log level strings (for /proc/sys/kernel/printk)
// ---------------------------------------------------------------------------

/// Emergency string.
pub const KERN_EMERG_STR: &str = "emerg";
/// Alert string.
pub const KERN_ALERT_STR: &str = "alert";
/// Critical string.
pub const KERN_CRIT_STR: &str = "crit";
/// Error string.
pub const KERN_ERR_STR: &str = "err";
/// Warning string.
pub const KERN_WARNING_STR: &str = "warning";
/// Notice string.
pub const KERN_NOTICE_STR: &str = "notice";
/// Info string.
pub const KERN_INFO_STR: &str = "info";
/// Debug string.
pub const KERN_DEBUG_STR: &str = "debug";

// ---------------------------------------------------------------------------
// Console log level defaults
// ---------------------------------------------------------------------------

/// Minimum console log level.
pub const CONSOLE_LOGLEVEL_MIN: u32 = 1;
/// Default console log level.
pub const CONSOLE_LOGLEVEL_DEFAULT: u32 = KERN_WARNING;
/// Quiet console log level.
pub const CONSOLE_LOGLEVEL_QUIET: u32 = KERN_WARNING;
/// Debug console log level.
pub const CONSOLE_LOGLEVEL_DEBUG: u32 = 10;
/// Motormouth (show everything).
pub const CONSOLE_LOGLEVEL_MOTORMOUTH: u32 = 15;

// ---------------------------------------------------------------------------
// Kernel ring buffer
// ---------------------------------------------------------------------------

/// Default log buffer size (shift, 2^17 = 128K).
pub const LOG_BUF_SHIFT: u32 = 17;
/// Maximum single message length.
pub const LOG_LINE_MAX: usize = 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levels_distinct() {
        let levels = [
            KERN_EMERG, KERN_ALERT, KERN_CRIT, KERN_ERR,
            KERN_WARNING, KERN_NOTICE, KERN_INFO, KERN_DEBUG,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_levels_ordered() {
        // Lower number = higher severity.
        assert!(KERN_EMERG < KERN_ALERT);
        assert!(KERN_ALERT < KERN_CRIT);
        assert!(KERN_CRIT < KERN_ERR);
        assert!(KERN_ERR < KERN_WARNING);
        assert!(KERN_WARNING < KERN_NOTICE);
        assert!(KERN_NOTICE < KERN_INFO);
        assert!(KERN_INFO < KERN_DEBUG);
    }

    #[test]
    fn test_level_count() {
        assert_eq!(KERN_LEVEL_COUNT, 8);
        assert!(KERN_DEBUG < KERN_LEVEL_COUNT);
    }

    #[test]
    fn test_level_strings_distinct() {
        let strings = [
            KERN_EMERG_STR, KERN_ALERT_STR, KERN_CRIT_STR,
            KERN_ERR_STR, KERN_WARNING_STR, KERN_NOTICE_STR,
            KERN_INFO_STR, KERN_DEBUG_STR,
        ];
        for i in 0..strings.len() {
            for j in (i + 1)..strings.len() {
                assert_ne!(strings[i], strings[j]);
            }
        }
    }

    #[test]
    fn test_console_levels() {
        assert!(CONSOLE_LOGLEVEL_MIN < CONSOLE_LOGLEVEL_DEFAULT);
        assert!(CONSOLE_LOGLEVEL_DEFAULT <= CONSOLE_LOGLEVEL_DEBUG);
    }

    #[test]
    fn test_log_line_max() {
        assert_eq!(LOG_LINE_MAX, 1024);
    }
}
