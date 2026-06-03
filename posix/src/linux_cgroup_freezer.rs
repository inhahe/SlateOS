//! `<linux/cgroup_freezer.h>` — Cgroup freezer controller constants.
//!
//! The cgroup freezer controller can suspend (freeze) all tasks
//! in a cgroup, preventing them from being scheduled. Used for
//! batch job management, container checkpointing (CRIU), and
//! system suspend.

// ---------------------------------------------------------------------------
// Freezer states (cgroup v1)
// ---------------------------------------------------------------------------

/// Tasks are running.
pub const CGROUP_THAWED: u32 = 0;
/// Freeze in progress.
pub const CGROUP_FREEZING: u32 = 1;
/// All tasks frozen.
pub const CGROUP_FROZEN: u32 = 2;

// ---------------------------------------------------------------------------
// Freezer state strings (cgroup v1 freezer.state)
// ---------------------------------------------------------------------------

/// Thawed state string.
pub const FREEZER_STATE_THAWED: &str = "THAWED";
/// Freezing state string.
pub const FREEZER_STATE_FREEZING: &str = "FREEZING";
/// Frozen state string.
pub const FREEZER_STATE_FROZEN: &str = "FROZEN";

// ---------------------------------------------------------------------------
// Cgroup v2 freeze interface
// ---------------------------------------------------------------------------

/// Freeze file name (cgroup v2).
pub const CGROUP_FREEZE_FILE: &str = "cgroup.freeze";
/// Events file name (cgroup v2).
pub const CGROUP_EVENTS_FILE: &str = "cgroup.events";

/// Unfreeze value (write to cgroup.freeze).
pub const CGROUP_FREEZE_OFF: u32 = 0;
/// Freeze value (write to cgroup.freeze).
pub const CGROUP_FREEZE_ON: u32 = 1;

// ---------------------------------------------------------------------------
// Freezer flags
// ---------------------------------------------------------------------------

/// Self-freezing (task froze itself).
pub const FREEZER_SELF_FREEZING: u32 = 1 << 0;
/// Kernel freezing (system suspend).
pub const FREEZER_KERNEL_FREEZING: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [CGROUP_THAWED, CGROUP_FREEZING, CGROUP_FROZEN];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_state_strings_distinct() {
        let strings = [
            FREEZER_STATE_THAWED,
            FREEZER_STATE_FREEZING,
            FREEZER_STATE_FROZEN,
        ];
        for i in 0..strings.len() {
            for j in (i + 1)..strings.len() {
                assert_ne!(strings[i], strings[j]);
            }
        }
    }

    #[test]
    fn test_v2_freeze_values() {
        assert_ne!(CGROUP_FREEZE_OFF, CGROUP_FREEZE_ON);
    }

    #[test]
    fn test_freezer_flags_no_overlap() {
        assert_eq!(FREEZER_SELF_FREEZING & FREEZER_KERNEL_FREEZING, 0);
    }
}
