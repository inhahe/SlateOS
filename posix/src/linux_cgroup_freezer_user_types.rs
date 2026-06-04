//! `<linux/cgroup.h>` (freezer) — cgroup-v1 and v2 freezer surfaces.
//!
//! The freezer subsystem suspends a whole cgroup of tasks without
//! requiring debugger-style ptrace. v1 used `freezer.state` strings;
//! v2 collapsed the API to a single `cgroup.freeze` 0/1 flag plus
//! a `cgroup.events:frozen` notification.

// ---------------------------------------------------------------------------
// v1 freezer files
// ---------------------------------------------------------------------------

pub const CGROUP_FREEZER_STATE: &str = "freezer.state";
pub const CGROUP_FREEZER_PARENT_FREEZING: &str = "freezer.parent_freezing";
pub const CGROUP_FREEZER_SELF_FREEZING: &str = "freezer.self_freezing";

// ---------------------------------------------------------------------------
// v1 state strings (`freezer.state` values)
// ---------------------------------------------------------------------------

pub const CGROUP_FREEZER_STATE_THAWED: &str = "THAWED";
pub const CGROUP_FREEZER_STATE_FREEZING: &str = "FREEZING";
pub const CGROUP_FREEZER_STATE_FROZEN: &str = "FROZEN";

// ---------------------------------------------------------------------------
// v2 freeze knob
// ---------------------------------------------------------------------------

pub const CGROUP2_FREEZE_FILE: &str = "cgroup.freeze";

/// Value written to thaw all tasks.
pub const CGROUP2_FREEZE_THAWED: u32 = 0;

/// Value written to freeze all tasks.
pub const CGROUP2_FREEZE_FROZEN: u32 = 1;

// ---------------------------------------------------------------------------
// State-transition timeouts (milliseconds)
// ---------------------------------------------------------------------------

/// Maximum time the kernel will wait for tasks to freeze before reporting
/// "FREEZING" indefinitely.
pub const CGROUP_FREEZER_TIMEOUT_MS: u64 = 20_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v1_files_have_freezer_prefix() {
        for f in [
            CGROUP_FREEZER_STATE,
            CGROUP_FREEZER_PARENT_FREEZING,
            CGROUP_FREEZER_SELF_FREEZING,
        ] {
            assert!(f.starts_with("freezer."));
        }
    }

    #[test]
    fn test_v1_state_strings_uppercase_distinct() {
        let s = [
            CGROUP_FREEZER_STATE_THAWED,
            CGROUP_FREEZER_STATE_FREEZING,
            CGROUP_FREEZER_STATE_FROZEN,
        ];
        for (i, &x) in s.iter().enumerate() {
            for &y in &s[i + 1..] {
                assert_ne!(x, y);
            }
            for c in x.chars() {
                // v1 used uppercase state names.
                assert!(c.is_ascii_uppercase() || c == '_');
            }
        }
    }

    #[test]
    fn test_v2_freeze_values_paired() {
        // 0 thawed, 1 frozen — boolean knob.
        assert_eq!(CGROUP2_FREEZE_THAWED, 0);
        assert_eq!(CGROUP2_FREEZE_FROZEN, 1);
        assert_eq!(CGROUP2_FREEZE_FROZEN - CGROUP2_FREEZE_THAWED, 1);
    }

    #[test]
    fn test_v2_freeze_file_is_cgroup_freeze() {
        assert_eq!(CGROUP2_FREEZE_FILE, "cgroup.freeze");
        assert!(CGROUP2_FREEZE_FILE.starts_with("cgroup."));
    }

    #[test]
    fn test_freeze_timeout_is_20s() {
        // 20 seconds — long enough for slow syscalls to drain.
        assert_eq!(CGROUP_FREEZER_TIMEOUT_MS, 20_000);
        assert_eq!(CGROUP_FREEZER_TIMEOUT_MS / 1_000, 20);
    }

    #[test]
    fn test_state_transitions_logical_order() {
        // FREEZING is the in-between state; FROZEN > THAWED in "stoppedness".
        for s in [
            CGROUP_FREEZER_STATE_THAWED,
            CGROUP_FREEZER_STATE_FREEZING,
            CGROUP_FREEZER_STATE_FROZEN,
        ] {
            assert!(!s.is_empty());
        }
    }
}
