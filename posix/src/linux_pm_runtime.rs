//! `<linux/pm_runtime.h>` — Runtime power management constants.
//!
//! Runtime PM allows individual devices to be powered down when idle
//! without affecting the rest of the system. Devices transition
//! between active, suspended, and error states based on usage
//! counting and autosuspend timers.

// ---------------------------------------------------------------------------
// Runtime PM status
// ---------------------------------------------------------------------------

/// Device is active (powered on, in use).
pub const RPM_ACTIVE: u8 = 0;
/// Device is resuming (transitioning to active).
pub const RPM_RESUMING: u8 = 1;
/// Device is suspended (powered down).
pub const RPM_SUSPENDED: u8 = 2;
/// Device is suspending (transitioning to suspended).
pub const RPM_SUSPENDING: u8 = 3;

// ---------------------------------------------------------------------------
// Runtime PM request types
// ---------------------------------------------------------------------------

/// No pending request.
pub const RPM_REQ_NONE: u8 = 0;
/// Idle notification pending.
pub const RPM_REQ_IDLE: u8 = 1;
/// Suspend request pending.
pub const RPM_REQ_SUSPEND: u8 = 2;
/// Autosuspend request pending.
pub const RPM_REQ_AUTOSUSPEND: u8 = 3;
/// Resume request pending.
pub const RPM_REQ_RESUME: u8 = 4;

// ---------------------------------------------------------------------------
// Runtime PM flags
// ---------------------------------------------------------------------------

/// Device supports runtime PM.
pub const RPM_FLAG_CAPABLE: u32 = 1 << 0;
/// Runtime PM is enabled for this device.
pub const RPM_FLAG_ENABLED: u32 = 1 << 1;
/// Device will not be suspended.
pub const RPM_FLAG_FORBIDDEN: u32 = 1 << 2;
/// Autosuspend enabled.
pub const RPM_FLAG_AUTOSUSPEND: u32 = 1 << 3;
/// No callbacks (for power domains).
pub const RPM_FLAG_NO_CALLBACKS: u32 = 1 << 4;
/// IRQ-safe (can suspend/resume in IRQ context).
pub const RPM_FLAG_IRQ_SAFE: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Autosuspend defaults
// ---------------------------------------------------------------------------

/// Default autosuspend delay (milliseconds): 2000ms.
pub const RPM_AUTOSUSPEND_DELAY_DEFAULT: i32 = 2000;
/// Disable autosuspend (negative value).
pub const RPM_AUTOSUSPEND_DISABLED: i32 = -1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_distinct() {
        let statuses = [RPM_ACTIVE, RPM_RESUMING, RPM_SUSPENDED, RPM_SUSPENDING];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_request_types_distinct() {
        let reqs = [
            RPM_REQ_NONE, RPM_REQ_IDLE, RPM_REQ_SUSPEND,
            RPM_REQ_AUTOSUSPEND, RPM_REQ_RESUME,
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            RPM_FLAG_CAPABLE, RPM_FLAG_ENABLED, RPM_FLAG_FORBIDDEN,
            RPM_FLAG_AUTOSUSPEND, RPM_FLAG_NO_CALLBACKS, RPM_FLAG_IRQ_SAFE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_autosuspend_defaults() {
        assert!(RPM_AUTOSUSPEND_DELAY_DEFAULT > 0);
        assert!(RPM_AUTOSUSPEND_DISABLED < 0);
    }
}
