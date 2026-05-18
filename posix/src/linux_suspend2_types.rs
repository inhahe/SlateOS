//! `<linux/suspend.h>` — Additional suspend/resume constants.
//!
//! Supplementary suspend constants covering sleep states,
//! PM events, and autosuspend parameters.

// ---------------------------------------------------------------------------
// Sleep states (PM_SUSPEND_*)
// ---------------------------------------------------------------------------

/// On (no suspend).
pub const PM_SUSPEND_ON: u32 = 0;
/// Freeze (s2idle).
pub const PM_SUSPEND_FREEZE: u32 = 1;
/// Standby (S1).
pub const PM_SUSPEND_STANDBY: u32 = 2;
/// Memory (S3/STR).
pub const PM_SUSPEND_MEM: u32 = 3;
/// Minimum valid.
pub const PM_SUSPEND_MIN: u32 = PM_SUSPEND_FREEZE;
/// Maximum valid.
pub const PM_SUSPEND_MAX: u32 = PM_SUSPEND_MEM;

// ---------------------------------------------------------------------------
// PM events
// ---------------------------------------------------------------------------

/// Suspend event.
pub const PM_EVENT_SUSPEND: u32 = 0x0002;
/// Resume event.
pub const PM_EVENT_RESUME: u32 = 0x0010;
/// Freeze event.
pub const PM_EVENT_FREEZE: u32 = 0x0001;
/// Thaw event.
pub const PM_EVENT_THAW: u32 = 0x0008;
/// Hibernate event.
pub const PM_EVENT_HIBERNATE: u32 = 0x0004;
/// Restore event.
pub const PM_EVENT_RESTORE: u32 = 0x0020;
/// Recover event.
pub const PM_EVENT_RECOVER: u32 = 0x0040;

// ---------------------------------------------------------------------------
// PM QoS flags
// ---------------------------------------------------------------------------

/// PM QoS: resume latency no constraint.
pub const PM_QOS_RESUME_LATENCY_NO_CONSTRAINT: i32 = 0;
/// PM QoS: resume latency no constraint value (s32 max).
pub const PM_QOS_RESUME_LATENCY_NO_CONSTRAINT_NS: i32 = i32::MAX;
/// Default latency tolerance.
pub const PM_QOS_LATENCY_TOLERANCE_DEFAULT_VALUE: i32 = 0;
/// No constraint latency tolerance.
pub const PM_QOS_LATENCY_TOLERANCE_NO_CONSTRAINT: i32 = -1;
/// Use default constraint.
pub const PM_QOS_LATENCY_ANY: i32 = 0;

// ---------------------------------------------------------------------------
// Autosuspend parameters
// ---------------------------------------------------------------------------

/// Autosuspend disabled (delay = -1).
pub const PM_AUTOSUSPEND_DISABLED: i32 = -1;
/// Minimum autosuspend delay (ms).
pub const PM_AUTOSUSPEND_DELAY_MIN: i32 = 0;

// ---------------------------------------------------------------------------
// Wakeup source flags
// ---------------------------------------------------------------------------

/// Wakeup active.
pub const PM_WAKEUP_ACTIVE: u32 = 1 << 0;
/// Wakeup pending.
pub const PM_WAKEUP_PENDING: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sleep_states_distinct() {
        let states = [
            PM_SUSPEND_ON, PM_SUSPEND_FREEZE,
            PM_SUSPEND_STANDBY, PM_SUSPEND_MEM,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_suspend_bounds() {
        assert_eq!(PM_SUSPEND_MIN, PM_SUSPEND_FREEZE);
        assert_eq!(PM_SUSPEND_MAX, PM_SUSPEND_MEM);
        assert!(PM_SUSPEND_MIN <= PM_SUSPEND_MAX);
    }

    #[test]
    fn test_pm_events_distinct() {
        let events = [
            PM_EVENT_FREEZE, PM_EVENT_SUSPEND, PM_EVENT_HIBERNATE,
            PM_EVENT_THAW, PM_EVENT_RESUME, PM_EVENT_RESTORE,
            PM_EVENT_RECOVER,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_pm_events_power_of_two() {
        let events = [
            PM_EVENT_FREEZE, PM_EVENT_SUSPEND, PM_EVENT_HIBERNATE,
            PM_EVENT_THAW, PM_EVENT_RESUME, PM_EVENT_RESTORE,
            PM_EVENT_RECOVER,
        ];
        for e in &events {
            assert!(e.is_power_of_two(), "0x{:04x} not power of two", e);
        }
    }

    #[test]
    fn test_wakeup_flags() {
        assert!(PM_WAKEUP_ACTIVE.is_power_of_two());
        assert!(PM_WAKEUP_PENDING.is_power_of_two());
        assert_eq!(PM_WAKEUP_ACTIVE & PM_WAKEUP_PENDING, 0);
    }

    #[test]
    fn test_autosuspend() {
        assert!(PM_AUTOSUSPEND_DISABLED < PM_AUTOSUSPEND_DELAY_MIN);
    }
}
