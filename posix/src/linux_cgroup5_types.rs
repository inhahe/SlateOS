//! `<linux/cgroup.h>` — Additional cgroup constants (part 5).
//!
//! Supplementary cgroup constants covering cgroup2 file types,
//! notification types, and migration flags.

// ---------------------------------------------------------------------------
// Cgroup v2 file types (cgroupfs)
// ---------------------------------------------------------------------------

/// Procs file.
pub const CGROUP_FILE_PROCS: u32 = 0;
/// Controllers file.
pub const CGROUP_FILE_CONTROLLERS: u32 = 1;
/// Subtree control.
pub const CGROUP_FILE_SUBTREE_CONTROL: u32 = 2;
/// Events file.
pub const CGROUP_FILE_EVENTS: u32 = 3;
/// Type file.
pub const CGROUP_FILE_TYPE: u32 = 4;
/// Stat file.
pub const CGROUP_FILE_STAT: u32 = 5;
/// IO stat.
pub const CGROUP_FILE_IO_STAT: u32 = 6;

// ---------------------------------------------------------------------------
// Cgroup migration flags
// ---------------------------------------------------------------------------

/// No cgroup migration flags.
pub const CGROUP_MIGRATION_NONE: u32 = 0;
/// Task may fork during migration.
pub const CGROUP_MIGRATION_ALLOW_FORK: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Cgroup controller types
// ---------------------------------------------------------------------------

/// CPU controller.
pub const CGROUP_CTRL_CPU: u32 = 0;
/// Memory controller.
pub const CGROUP_CTRL_MEMORY: u32 = 1;
/// IO controller.
pub const CGROUP_CTRL_IO: u32 = 2;
/// PID controller.
pub const CGROUP_CTRL_PIDS: u32 = 3;
/// RDMA controller.
pub const CGROUP_CTRL_RDMA: u32 = 4;
/// Misc controller.
pub const CGROUP_CTRL_MISC: u32 = 5;
/// HugeTLB controller.
pub const CGROUP_CTRL_HUGETLB: u32 = 6;
/// Cpuset controller.
pub const CGROUP_CTRL_CPUSET: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_types_distinct() {
        let types = [
            CGROUP_FILE_PROCS,
            CGROUP_FILE_CONTROLLERS,
            CGROUP_FILE_SUBTREE_CONTROL,
            CGROUP_FILE_EVENTS,
            CGROUP_FILE_TYPE,
            CGROUP_FILE_STAT,
            CGROUP_FILE_IO_STAT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_migration_flag() {
        assert!(CGROUP_MIGRATION_ALLOW_FORK.is_power_of_two());
    }

    #[test]
    fn test_controllers_distinct() {
        let ctrls = [
            CGROUP_CTRL_CPU,
            CGROUP_CTRL_MEMORY,
            CGROUP_CTRL_IO,
            CGROUP_CTRL_PIDS,
            CGROUP_CTRL_RDMA,
            CGROUP_CTRL_MISC,
            CGROUP_CTRL_HUGETLB,
            CGROUP_CTRL_CPUSET,
        ];
        for i in 0..ctrls.len() {
            for j in (i + 1)..ctrls.len() {
                assert_ne!(ctrls[i], ctrls[j]);
            }
        }
    }
}
