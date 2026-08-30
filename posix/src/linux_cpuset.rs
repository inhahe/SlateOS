//! `<linux/cpuset.h>` — Cpuset cgroup controller constants.
//!
//! The cpuset controller constrains which CPUs and memory nodes
//! a cgroup's tasks can use. Essential for NUMA-aware workload
//! placement, CPU isolation, and real-time scheduling.

// ---------------------------------------------------------------------------
// Cpuset file names (cgroup v2)
// ---------------------------------------------------------------------------

/// Effective CPUs.
pub const CPUSET_CPUS_EFFECTIVE: &str = "cpuset.cpus.effective";
/// Requested CPUs.
pub const CPUSET_CPUS: &str = "cpuset.cpus";
/// Effective memory nodes.
pub const CPUSET_MEMS_EFFECTIVE: &str = "cpuset.mems.effective";
/// Requested memory nodes.
pub const CPUSET_MEMS: &str = "cpuset.mems";
/// CPU partition type.
pub const CPUSET_CPUS_PARTITION: &str = "cpuset.cpus.partition";

// ---------------------------------------------------------------------------
// Partition types
// ---------------------------------------------------------------------------

/// Member (default, non-isolated).
pub const CPUSET_PARTITION_MEMBER: &str = "member";
/// Root partition (exclusive CPUs).
pub const CPUSET_PARTITION_ROOT: &str = "root";
/// Isolated root partition.
pub const CPUSET_PARTITION_ISOLATED: &str = "isolated";

// ---------------------------------------------------------------------------
// Cpuset flags (cgroup v1)
// ---------------------------------------------------------------------------

/// CPU exclusive (no sharing with siblings).
pub const CPUSET_CPU_EXCLUSIVE: u32 = 1 << 0;
/// Memory exclusive.
pub const CPUSET_MEM_EXCLUSIVE: u32 = 1 << 1;
/// Memory hardwall (no kernel allocations outside nodes).
pub const CPUSET_MEM_HARDWALL: u32 = 1 << 2;
/// Spread pages across nodes.
pub const CPUSET_SPREAD_PAGE: u32 = 1 << 3;
/// Spread slab across nodes.
pub const CPUSET_SPREAD_SLAB: u32 = 1 << 4;
/// Memory migration on cpuset change.
pub const CPUSET_MEM_MIGRATE: u32 = 1 << 5;
/// Scheduler load balancing.
pub const CPUSET_SCHED_LOAD_BALANCE: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_names_distinct() {
        let files = [
            CPUSET_CPUS_EFFECTIVE,
            CPUSET_CPUS,
            CPUSET_MEMS_EFFECTIVE,
            CPUSET_MEMS,
            CPUSET_CPUS_PARTITION,
        ];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
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

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            CPUSET_CPU_EXCLUSIVE,
            CPUSET_MEM_EXCLUSIVE,
            CPUSET_MEM_HARDWALL,
            CPUSET_SPREAD_PAGE,
            CPUSET_SPREAD_SLAB,
            CPUSET_MEM_MIGRATE,
            CPUSET_SCHED_LOAD_BALANCE,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            CPUSET_CPU_EXCLUSIVE,
            CPUSET_MEM_EXCLUSIVE,
            CPUSET_MEM_HARDWALL,
            CPUSET_SPREAD_PAGE,
            CPUSET_SPREAD_SLAB,
            CPUSET_MEM_MIGRATE,
            CPUSET_SCHED_LOAD_BALANCE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
