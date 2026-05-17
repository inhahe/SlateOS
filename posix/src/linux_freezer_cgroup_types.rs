//! `<linux/cgroup_freezer.h>` — Freezer cgroup controller constants.
//!
//! The freezer cgroup controller suspends and resumes groups of
//! processes. When a cgroup is frozen, all tasks in it are put into
//! an uninterruptible sleep state (TASK_FROZEN) — they stop executing
//! and consuming CPU, but retain their memory and other resources.
//! Used for container pause/unpause, checkpointing (CRIU), and
//! batch job scheduling. In cgroup v2, freezing is built into the
//! core cgroup interface (cgroup.freeze file).

// ---------------------------------------------------------------------------
// Freezer states
// ---------------------------------------------------------------------------

/// Cgroup is thawed (all tasks running normally).
pub const FREEZER_STATE_THAWED: u32 = 0;
/// Cgroup is in the process of freezing.
pub const FREEZER_STATE_FREEZING: u32 = 1;
/// Cgroup is fully frozen (all tasks stopped).
pub const FREEZER_STATE_FROZEN: u32 = 2;

// ---------------------------------------------------------------------------
// Freezer v2 control values (written to cgroup.freeze)
// ---------------------------------------------------------------------------

/// Unfreeze the cgroup (resume all tasks).
pub const CGROUP_FREEZE_OFF: u32 = 0;
/// Freeze the cgroup (suspend all tasks).
pub const CGROUP_FREEZE_ON: u32 = 1;

// ---------------------------------------------------------------------------
// Freezer event types
// ---------------------------------------------------------------------------

/// Event: cgroup became frozen.
pub const FREEZER_EVENT_FROZEN: u32 = 0;
/// Event: cgroup became thawed.
pub const FREEZER_EVENT_THAWED: u32 = 1;
/// Event: freeze operation failed (some tasks couldn't be frozen).
pub const FREEZER_EVENT_ERROR: u32 = 2;

// ---------------------------------------------------------------------------
// Freezer flags (kernel internal)
// ---------------------------------------------------------------------------

/// Freeze is self-initiated (e.g., SIGSTOP equivalent).
pub const FREEZER_FLAG_SELF: u32 = 0x01;
/// Freeze is parent-initiated (container pause).
pub const FREEZER_FLAG_PARENT: u32 = 0x02;
/// Freeze is system-initiated (suspend-to-disk).
pub const FREEZER_FLAG_SYSTEM: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_ordered() {
        assert!(FREEZER_STATE_THAWED < FREEZER_STATE_FREEZING);
        assert!(FREEZER_STATE_FREEZING < FREEZER_STATE_FROZEN);
    }

    #[test]
    fn test_v2_control_values() {
        assert_eq!(CGROUP_FREEZE_OFF, 0);
        assert_eq!(CGROUP_FREEZE_ON, 1);
        assert_ne!(CGROUP_FREEZE_OFF, CGROUP_FREEZE_ON);
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            FREEZER_EVENT_FROZEN, FREEZER_EVENT_THAWED,
            FREEZER_EVENT_ERROR,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [FREEZER_FLAG_SELF, FREEZER_FLAG_PARENT, FREEZER_FLAG_SYSTEM];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
