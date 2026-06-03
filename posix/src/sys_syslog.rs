//! `<sys/syslog.h>` — system logging re-exports.
//!
//! Re-exports the syslog API from the `syslog` module.  Programs
//! that include `<sys/syslog.h>` find everything here.

pub use crate::syslog::LOG_ALERT;
pub use crate::syslog::LOG_CRIT;
pub use crate::syslog::LOG_DEBUG;
pub use crate::syslog::LOG_EMERG;
pub use crate::syslog::LOG_ERR;
pub use crate::syslog::LOG_INFO;
pub use crate::syslog::LOG_NOTICE;
pub use crate::syslog::LOG_WARNING;

pub use crate::syslog::LOG_AUTH;
pub use crate::syslog::LOG_DAEMON;
pub use crate::syslog::LOG_KERN;
pub use crate::syslog::LOG_LOCAL0;
pub use crate::syslog::LOG_SYSLOG;
pub use crate::syslog::LOG_USER;

pub use crate::syslog::LOG_NDELAY;
pub use crate::syslog::LOG_PERROR;
pub use crate::syslog::LOG_PID;

pub use crate::syslog::openlog;
// `syslog` itself is an assembly trampoline (variadic) defined only on the
// bare-metal target; the Rust-callable forms are `_syslog_impl` and `vsyslog`.
pub use crate::syslog::closelog;
pub use crate::syslog::setlogmask;
pub use crate::syslog::vsyslog;

// ---------------------------------------------------------------------------
// LOG_MASK / LOG_UPTO macros as functions
// ---------------------------------------------------------------------------

/// Create a mask for a single priority.
#[inline]
pub const fn log_mask(pri: i32) -> i32 {
    1 << pri
}

/// Create a mask for all priorities up to and including `pri`.
#[inline]
pub const fn log_upto(pri: i32) -> i32 {
    (1 << (pri + 1)) - 1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_priorities() {
        assert_eq!(LOG_EMERG, 0);
        assert_eq!(LOG_DEBUG, 7);
    }

    #[test]
    fn test_log_mask() {
        assert_eq!(log_mask(LOG_ERR), 1 << 3);
    }

    #[test]
    fn test_log_upto() {
        let mask = log_upto(LOG_WARNING);
        // Should include EMERG, ALERT, CRIT, ERR, WARNING (bits 0-4).
        assert_eq!(mask, 0x1F);
    }

    #[test]
    fn test_log_upto_debug() {
        let mask = log_upto(LOG_DEBUG);
        assert_eq!(mask, 0xFF); // All 8 priorities.
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(LOG_ERR, crate::syslog::LOG_ERR);
        assert_eq!(LOG_PID, crate::syslog::LOG_PID);
    }
}
