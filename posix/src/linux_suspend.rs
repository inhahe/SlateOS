//! `<linux/suspend.h>` + `<linux/pm.h>` — Power management constants.
//!
//! Defines system suspend states and power management events used
//! by the PM subsystem, device drivers, and userspace (via sysfs
//! `/sys/power/state`).

// ---------------------------------------------------------------------------
// Suspend states (writable to /sys/power/state)
// ---------------------------------------------------------------------------

/// Freeze (suspend-to-idle, s2idle).
pub const PM_SUSPEND_FREEZE: u32 = 1;
/// Standby (power-on suspend, S1).
pub const PM_SUSPEND_STANDBY: u32 = 2;
/// Suspend to RAM (S3).
pub const PM_SUSPEND_MEM: u32 = 3;
/// Maximum suspend state.
pub const PM_SUSPEND_MAX: u32 = 4;

// ---------------------------------------------------------------------------
// Hibernate modes
// ---------------------------------------------------------------------------

/// Platform hibernate (ACPI S4).
pub const HIBERNATION_PLATFORM: u32 = 1;
/// Shutdown after hibernate.
pub const HIBERNATION_SHUTDOWN: u32 = 2;
/// Reboot after hibernate.
pub const HIBERNATION_REBOOT: u32 = 3;
/// Suspend after creating hibernate image.
pub const HIBERNATION_SUSPEND: u32 = 4;
/// Test resume (no actual hibernate).
pub const HIBERNATION_TEST_RESUME: u32 = 5;

// ---------------------------------------------------------------------------
// PM events (sent to device drivers)
// ---------------------------------------------------------------------------

/// No event.
pub const PM_EVENT_ON: u32 = 0x0000;
/// Prepare for freeze.
pub const PM_EVENT_FREEZE: u32 = 0x0001;
/// Suspend.
pub const PM_EVENT_SUSPEND: u32 = 0x0002;
/// Hibernate.
pub const PM_EVENT_HIBERNATE: u32 = 0x0004;
/// User-space suspend.
pub const PM_EVENT_QUIESCE: u32 = 0x0008;
/// Resume.
pub const PM_EVENT_RESUME: u32 = 0x0010;
/// Thaw (resume from freeze).
pub const PM_EVENT_THAW: u32 = 0x0020;
/// Restore (resume from hibernate).
pub const PM_EVENT_RESTORE: u32 = 0x0040;
/// Recover (resume from failed suspend).
pub const PM_EVENT_RECOVER: u32 = 0x0080;

// ---------------------------------------------------------------------------
// Power state strings (for sysfs)
// ---------------------------------------------------------------------------

/// Freeze state name.
pub const PM_STATE_FREEZE: &str = "freeze";
/// Standby state name.
pub const PM_STATE_STANDBY: &str = "standby";
/// Memory state name (suspend-to-RAM).
pub const PM_STATE_MEM: &str = "mem";
/// Disk state name (hibernate).
pub const PM_STATE_DISK: &str = "disk";

// ---------------------------------------------------------------------------
// Wakeup source flags
// ---------------------------------------------------------------------------

/// Wakeup capable.
pub const PM_WAKEUP_CAPABLE: u32 = 1 << 0;
/// Wakeup enabled.
pub const PM_WAKEUP_ENABLED: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suspend_states_distinct() {
        let states = [
            PM_SUSPEND_FREEZE,
            PM_SUSPEND_STANDBY,
            PM_SUSPEND_MEM,
            PM_SUSPEND_MAX,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_hibernate_modes_distinct() {
        let modes = [
            HIBERNATION_PLATFORM,
            HIBERNATION_SHUTDOWN,
            HIBERNATION_REBOOT,
            HIBERNATION_SUSPEND,
            HIBERNATION_TEST_RESUME,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_pm_events_no_overlap() {
        let events = [
            PM_EVENT_FREEZE,
            PM_EVENT_SUSPEND,
            PM_EVENT_HIBERNATE,
            PM_EVENT_QUIESCE,
            PM_EVENT_RESUME,
            PM_EVENT_THAW,
            PM_EVENT_RESTORE,
            PM_EVENT_RECOVER,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_eq!(
                    events[i] & events[j],
                    0,
                    "overlap: 0x{:x} & 0x{:x}",
                    events[i],
                    events[j]
                );
            }
        }
    }

    #[test]
    fn test_state_strings() {
        assert_eq!(PM_STATE_FREEZE, "freeze");
        assert_eq!(PM_STATE_MEM, "mem");
        assert_eq!(PM_STATE_DISK, "disk");
    }

    #[test]
    fn test_wakeup_flags() {
        assert!(PM_WAKEUP_CAPABLE.is_power_of_two());
        assert!(PM_WAKEUP_ENABLED.is_power_of_two());
        assert_ne!(PM_WAKEUP_CAPABLE, PM_WAKEUP_ENABLED);
    }
}
