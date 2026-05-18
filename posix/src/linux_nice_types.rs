//! `<sys/resource.h>` — Process priority (nice) constants.
//!
//! `nice()`, `getpriority()`, and `setpriority()` control process
//! scheduling priority.  These constants define the priority
//! range, the `which` parameter, and related values.

// ---------------------------------------------------------------------------
// Nice value range
// ---------------------------------------------------------------------------

/// Minimum nice value (highest priority).
pub const PRIO_MIN: i32 = -20;
/// Maximum nice value (lowest priority).
pub const PRIO_MAX: i32 = 19;
/// Default nice value.
pub const PRIO_DEFAULT: i32 = 0;

// ---------------------------------------------------------------------------
// getpriority/setpriority which parameter
// ---------------------------------------------------------------------------

/// Apply to a process (identified by PID).
pub const PRIO_PROCESS: u32 = 0;
/// Apply to a process group (identified by PGID).
pub const PRIO_PGRP: u32 = 1;
/// Apply to a user (identified by UID).
pub const PRIO_USER: u32 = 2;

// ---------------------------------------------------------------------------
// nice() return value handling
// ---------------------------------------------------------------------------

/// nice() error indicator (errno must be checked since nice can return -1 legitimately).
pub const NICE_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// Autogroup nice range (Linux extension)
// ---------------------------------------------------------------------------

/// Minimum autogroup nice.
pub const AUTOGROUP_NICE_MIN: i32 = -20;
/// Maximum autogroup nice.
pub const AUTOGROUP_NICE_MAX: i32 = 19;

// ---------------------------------------------------------------------------
// ionice / ioprio integration
// ---------------------------------------------------------------------------

/// Number of nice levels.
pub const NICE_LEVELS: u32 = 40; // -20 to +19

// ---------------------------------------------------------------------------
// Scheduling priority conversion
// ---------------------------------------------------------------------------

/// Convert nice value to kernel static priority: prio = MAX_RT_PRIO + nice + 20.
pub const NICE_TO_PRIO_OFFSET: u32 = 120;
/// Maximum RT priority (boundary between RT and normal).
pub const MAX_RT_PRIO: u32 = 100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prio_range() {
        assert!(PRIO_MIN < PRIO_MAX);
        assert_eq!(PRIO_MIN, -20);
        assert_eq!(PRIO_MAX, 19);
    }

    #[test]
    fn test_prio_default_is_zero() {
        assert_eq!(PRIO_DEFAULT, 0);
    }

    #[test]
    fn test_which_values_distinct() {
        let vals = [PRIO_PROCESS, PRIO_PGRP, PRIO_USER];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_process_is_zero() {
        assert_eq!(PRIO_PROCESS, 0);
    }

    #[test]
    fn test_nice_levels() {
        assert_eq!(NICE_LEVELS, 40);
        assert_eq!(NICE_LEVELS as i32, PRIO_MAX - PRIO_MIN + 1);
    }

    #[test]
    fn test_nice_to_prio_offset() {
        assert_eq!(NICE_TO_PRIO_OFFSET, 120);
    }

    #[test]
    fn test_max_rt_prio() {
        assert_eq!(MAX_RT_PRIO, 100);
    }

    #[test]
    fn test_autogroup_range() {
        assert_eq!(AUTOGROUP_NICE_MIN, PRIO_MIN);
        assert_eq!(AUTOGROUP_NICE_MAX, PRIO_MAX);
    }
}
