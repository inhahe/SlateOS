//! `<linux/kern_levels.h>` — Kernel printk log level constants.
//!
//! Every kernel log message has a priority level encoded as a
//! single digit in the message prefix. These constants define the
//! priority levels from emergency (most critical) to debug (least).

// ---------------------------------------------------------------------------
// Kernel log levels (KERN_*)
// ---------------------------------------------------------------------------

/// System is unusable (panic imminent).
pub const KERN_EMERG: u32 = 0;
/// Action must be taken immediately.
pub const KERN_ALERT: u32 = 1;
/// Critical conditions.
pub const KERN_CRIT: u32 = 2;
/// Error conditions.
pub const KERN_ERR: u32 = 3;
/// Warning conditions.
pub const KERN_WARNING: u32 = 4;
/// Normal but significant condition.
pub const KERN_NOTICE: u32 = 5;
/// Informational.
pub const KERN_INFO: u32 = 6;
/// Debug-level messages.
pub const KERN_DEBUG: u32 = 7;

// ---------------------------------------------------------------------------
// Default log level settings
// ---------------------------------------------------------------------------

/// Default kernel message log level.
pub const MESSAGE_LOGLEVEL_DEFAULT: u32 = KERN_WARNING;
/// Default console log level threshold.
pub const CONSOLE_LOGLEVEL_DEFAULT_VAL: u32 = 7;
/// Minimum printk log level (everything above is printed).
pub const MINIMUM_CONSOLE_LOGLEVEL: u32 = 1;

// ---------------------------------------------------------------------------
// Log facility flags (for structured logging / dev_printk)
// ---------------------------------------------------------------------------

/// Log message is from a device driver.
pub const LOG_FACILITY_DRIVER: u32 = 0;
/// Log message is from a subsystem.
pub const LOG_FACILITY_SUBSYSTEM: u32 = 1;
/// Log message is from kernel core.
pub const LOG_FACILITY_KERNEL: u32 = 2;

// ---------------------------------------------------------------------------
// printk rate limiting
// ---------------------------------------------------------------------------

/// Default rate limit interval (5 seconds).
pub const PRINTK_RATELIMIT_INTERVAL_MS: u32 = 5000;
/// Default rate limit burst (10 messages).
pub const PRINTK_RATELIMIT_BURST: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levels_sequential() {
        assert_eq!(KERN_EMERG, 0);
        assert_eq!(KERN_ALERT, 1);
        assert_eq!(KERN_CRIT, 2);
        assert_eq!(KERN_ERR, 3);
        assert_eq!(KERN_WARNING, 4);
        assert_eq!(KERN_NOTICE, 5);
        assert_eq!(KERN_INFO, 6);
        assert_eq!(KERN_DEBUG, 7);
    }

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
    fn test_severity_ordering() {
        // Lower number = more severe
        assert!(KERN_EMERG < KERN_DEBUG);
        assert!(KERN_ERR < KERN_INFO);
    }

    #[test]
    fn test_default_loglevel() {
        assert_eq!(MESSAGE_LOGLEVEL_DEFAULT, KERN_WARNING);
    }

    #[test]
    fn test_facility_distinct() {
        let facilities = [
            LOG_FACILITY_DRIVER,
            LOG_FACILITY_SUBSYSTEM,
            LOG_FACILITY_KERNEL,
        ];
        for i in 0..facilities.len() {
            for j in (i + 1)..facilities.len() {
                assert_ne!(facilities[i], facilities[j]);
            }
        }
    }

    #[test]
    fn test_ratelimit() {
        assert_eq!(PRINTK_RATELIMIT_INTERVAL_MS, 5000);
        assert_eq!(PRINTK_RATELIMIT_BURST, 10);
    }
}
