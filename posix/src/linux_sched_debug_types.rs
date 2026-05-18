//! `<linux/sched.h>` — Scheduler debug and statistics constants.
//!
//! These constants relate to scheduler accounting, debug interfaces
//! (`/proc/schedstat`, `/proc/<pid>/sched`), and task scheduling
//! statistics fields.

// ---------------------------------------------------------------------------
// Scheduler statistics fields (schedstat)
// ---------------------------------------------------------------------------

/// Number of times a task was run on a CPU.
pub const SCHEDSTAT_RUN_COUNT: u32 = 0;
/// Total run time in nanoseconds.
pub const SCHEDSTAT_RUN_TIME: u32 = 1;
/// Total wait time in nanoseconds.
pub const SCHEDSTAT_WAIT_TIME: u32 = 2;
/// Number of timeslice expirations.
pub const SCHEDSTAT_TIMESLICES: u32 = 3;

// ---------------------------------------------------------------------------
// Scheduler accounting version
// ---------------------------------------------------------------------------

/// schedstat version number (from /proc/schedstat header).
pub const SCHEDSTAT_VERSION: u32 = 15;

// ---------------------------------------------------------------------------
// Scheduler domain flags (SD_*)
// ---------------------------------------------------------------------------

/// Load balancing enabled on this domain.
pub const SD_LOAD_BALANCE: u32 = 1 << 0;
/// Balance when a task wakes up.
pub const SD_BALANCE_NEWIDLE: u32 = 1 << 1;
/// Balance on exec.
pub const SD_BALANCE_EXEC: u32 = 1 << 2;
/// Balance on fork.
pub const SD_BALANCE_FORK: u32 = 1 << 3;
/// Balance when CPU goes idle.
pub const SD_BALANCE_WAKE: u32 = 1 << 4;
/// Allow wakeup affinity override.
pub const SD_WAKE_AFFINE: u32 = 1 << 5;
/// Prefer to place tasks on this domain.
pub const SD_PREFER_LOCAL: u32 = 1 << 6;
/// Share CPU capacity across groups.
pub const SD_SHARE_CPUCAPACITY: u32 = 1 << 7;
/// Share power domain.
pub const SD_SHARE_POWERDOMAIN: u32 = 1 << 8;
/// Share package resources (LLC).
pub const SD_SHARE_PKG_RESOURCES: u32 = 1 << 9;
/// Serialize load balancing.
pub const SD_SERIALIZE: u32 = 1 << 10;
/// Prefer siblings in this domain.
pub const SD_PREFER_SIBLING: u32 = 1 << 11;
/// NUMA distance domain.
pub const SD_NUMA: u32 = 1 << 12;

// ---------------------------------------------------------------------------
// CPU load index
// ---------------------------------------------------------------------------

/// Number of CPU load tracking indices.
pub const CPU_LOAD_IDX_MAX: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schedstat_fields_distinct() {
        let fields = [
            SCHEDSTAT_RUN_COUNT, SCHEDSTAT_RUN_TIME,
            SCHEDSTAT_WAIT_TIME, SCHEDSTAT_TIMESLICES,
        ];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }

    #[test]
    fn test_schedstat_version() {
        assert_eq!(SCHEDSTAT_VERSION, 15);
    }

    #[test]
    fn test_sd_flags_power_of_two() {
        let flags = [
            SD_LOAD_BALANCE, SD_BALANCE_NEWIDLE, SD_BALANCE_EXEC,
            SD_BALANCE_FORK, SD_BALANCE_WAKE, SD_WAKE_AFFINE,
            SD_PREFER_LOCAL, SD_SHARE_CPUCAPACITY, SD_SHARE_POWERDOMAIN,
            SD_SHARE_PKG_RESOURCES, SD_SERIALIZE, SD_PREFER_SIBLING,
            SD_NUMA,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_sd_flags_no_overlap() {
        let flags = [
            SD_LOAD_BALANCE, SD_BALANCE_NEWIDLE, SD_BALANCE_EXEC,
            SD_BALANCE_FORK, SD_BALANCE_WAKE, SD_WAKE_AFFINE,
            SD_PREFER_LOCAL, SD_SHARE_CPUCAPACITY, SD_SHARE_POWERDOMAIN,
            SD_SHARE_PKG_RESOURCES, SD_SERIALIZE, SD_PREFER_SIBLING,
            SD_NUMA,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_cpu_load_idx_max() {
        assert_eq!(CPU_LOAD_IDX_MAX, 5);
    }

    #[test]
    fn test_sd_load_balance() {
        assert_eq!(SD_LOAD_BALANCE, 1);
    }

    #[test]
    fn test_sd_numa() {
        assert_eq!(SD_NUMA, 1 << 12);
    }
}
