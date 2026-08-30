//! `<linux/cpuset.h>` — cpuset cgroup controller constants.
//!
//! The cpuset controller constrains which CPUs and memory nodes a
//! group of processes can use. This enables CPU and memory partitioning
//! for performance isolation: assign different cpusets to different
//! workloads so they don't compete for cache or memory bandwidth.
//! cpuset supports exclusive assignment (a CPU can belong to only one
//! exclusive cpuset), memory migration, and load balancing control.

// ---------------------------------------------------------------------------
// cpuset flags
// ---------------------------------------------------------------------------

/// CPUs are exclusively assigned (no other cpuset can use them).
pub const CPUSET_CPU_EXCLUSIVE: u32 = 0x0000_0001;
/// Memory nodes are exclusively assigned.
pub const CPUSET_MEM_EXCLUSIVE: u32 = 0x0000_0002;
/// Hardwall: kernel allocations also restricted to this cpuset.
pub const CPUSET_MEM_HARDWALL: u32 = 0x0000_0004;
/// Enable load balancing within this cpuset.
pub const CPUSET_SCHED_LOAD_BALANCE: u32 = 0x0000_0008;
/// Spread memory allocation across all allowed nodes.
pub const CPUSET_MEMORY_SPREAD_PAGE: u32 = 0x0000_0010;
/// Spread slab allocations across all allowed nodes.
pub const CPUSET_MEMORY_SPREAD_SLAB: u32 = 0x0000_0020;

// ---------------------------------------------------------------------------
// cpuset memory migration modes
// ---------------------------------------------------------------------------

/// No memory migration (processes keep pages where allocated).
pub const CPUSET_MEM_MIGRATE_NONE: u32 = 0;
/// Migrate pages when cpuset memory nodes change.
pub const CPUSET_MEM_MIGRATE_ON_CHANGE: u32 = 1;
/// Migrate pages on next touch after cpuset change.
pub const CPUSET_MEM_MIGRATE_ON_TOUCH: u32 = 2;

// ---------------------------------------------------------------------------
// cpuset partition types (cgroup v2)
// ---------------------------------------------------------------------------

/// Member partition (non-isolated, default).
pub const CPUSET_PARTITION_MEMBER: u32 = 0;
/// Root partition (owns CPUs exclusively).
pub const CPUSET_PARTITION_ROOT: u32 = 1;
/// Isolated partition (like root but no load balancing).
pub const CPUSET_PARTITION_ISOLATED: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            CPUSET_CPU_EXCLUSIVE,
            CPUSET_MEM_EXCLUSIVE,
            CPUSET_MEM_HARDWALL,
            CPUSET_SCHED_LOAD_BALANCE,
            CPUSET_MEMORY_SPREAD_PAGE,
            CPUSET_MEMORY_SPREAD_SLAB,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_migrate_modes_distinct() {
        let modes = [
            CPUSET_MEM_MIGRATE_NONE,
            CPUSET_MEM_MIGRATE_ON_CHANGE,
            CPUSET_MEM_MIGRATE_ON_TOUCH,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_partition_types_distinct() {
        let types = [
            CPUSET_PARTITION_MEMBER,
            CPUSET_PARTITION_ROOT,
            CPUSET_PARTITION_ISOLATED,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
