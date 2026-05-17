//! `<linux/pm_runtime.h>` — Runtime power management constants.
//!
//! Runtime PM allows individual devices to be powered down when
//! idle, independent of system sleep state. When a device has no
//! pending work, its driver calls pm_runtime_put() and the device
//! enters a low-power state. When work arrives, pm_runtime_get()
//! resumes it. This saves significant power on systems with many
//! devices that are only intermittently used (USB, PCIe, GPU, etc.).

// ---------------------------------------------------------------------------
// Runtime PM status
// ---------------------------------------------------------------------------

/// Device is active (powered on, operational).
pub const RPM_ACTIVE: u32 = 0;
/// Device is resuming (transitioning to active).
pub const RPM_RESUMING: u32 = 1;
/// Device is suspended (powered off/low-power).
pub const RPM_SUSPENDED: u32 = 2;
/// Device is suspending (transitioning to suspended).
pub const RPM_SUSPENDING: u32 = 3;

// ---------------------------------------------------------------------------
// Runtime PM request types
// ---------------------------------------------------------------------------

/// No pending request.
pub const RPM_REQ_NONE: u32 = 0;
/// Idle check requested.
pub const RPM_REQ_IDLE: u32 = 1;
/// Suspend requested.
pub const RPM_REQ_SUSPEND: u32 = 2;
/// Auto-suspend requested (with delay).
pub const RPM_REQ_AUTOSUSPEND: u32 = 3;
/// Resume requested.
pub const RPM_REQ_RESUME: u32 = 4;

// ---------------------------------------------------------------------------
// Runtime PM flags
// ---------------------------------------------------------------------------

/// Device is runtime PM enabled.
pub const RPM_FLAG_ENABLED: u32 = 0x01;
/// Device should auto-suspend after idle timeout.
pub const RPM_FLAG_AUTOSUSPEND: u32 = 0x02;
/// Device is in a no-callbacks state.
pub const RPM_FLAG_NO_CALLBACKS: u32 = 0x04;
/// Device IRQ is safe during suspend.
pub const RPM_FLAG_IRQ_SAFE: u32 = 0x08;

// ---------------------------------------------------------------------------
// Auto-suspend delay
// ---------------------------------------------------------------------------

/// Default auto-suspend delay (milliseconds, 0 = immediate).
pub const RPM_AUTOSUSPEND_DELAY_DEFAULT: i32 = 0;
/// Disable auto-suspend (negative = never auto-suspend).
pub const RPM_AUTOSUSPEND_DISABLED: i32 = -1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_distinct() {
        let states = [RPM_ACTIVE, RPM_RESUMING, RPM_SUSPENDED, RPM_SUSPENDING];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_requests_distinct() {
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
            RPM_FLAG_ENABLED, RPM_FLAG_AUTOSUSPEND,
            RPM_FLAG_NO_CALLBACKS, RPM_FLAG_IRQ_SAFE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
