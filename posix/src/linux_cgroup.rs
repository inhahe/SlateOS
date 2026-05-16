//! `<linux/cgroup.h>` — cgroup (control group) constants.
//!
//! Cgroups organize processes into hierarchical groups for resource
//! management (CPU, memory, I/O, network). Used by systemd, Docker,
//! Kubernetes, and other container runtimes. This covers cgroup v2.

// ---------------------------------------------------------------------------
// cgroup2 file names
// ---------------------------------------------------------------------------

/// cgroup v2 filesystem type.
pub const CGROUP2_SUPER_MAGIC: u64 = 0x63677270;

/// cgroup v1 filesystem type.
pub const CGROUP_SUPER_MAGIC: u64 = 0x27e0eb;

// ---------------------------------------------------------------------------
// Controller types
// ---------------------------------------------------------------------------

/// CPU controller.
pub const CGROUP_CTRL_CPU: u32 = 1 << 0;
/// Memory controller.
pub const CGROUP_CTRL_MEMORY: u32 = 1 << 1;
/// I/O controller.
pub const CGROUP_CTRL_IO: u32 = 1 << 2;
/// PID controller.
pub const CGROUP_CTRL_PIDS: u32 = 1 << 3;
/// RDMA controller.
pub const CGROUP_CTRL_RDMA: u32 = 1 << 4;
/// HugeTLB controller.
pub const CGROUP_CTRL_HUGETLB: u32 = 1 << 5;
/// CPU set controller.
pub const CGROUP_CTRL_CPUSET: u32 = 1 << 6;
/// Misc controller.
pub const CGROUP_CTRL_MISC: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// cgroup v2 thread modes
// ---------------------------------------------------------------------------

/// Domain cgroup (normal).
pub const CGROUP_TYPE_DOMAIN: u32 = 0;
/// Threaded cgroup.
pub const CGROUP_TYPE_THREADED: u32 = 1;
/// Domain-threaded (root of threaded subtree).
pub const CGROUP_TYPE_DOMAIN_THREADED: u32 = 2;
/// Domain invalid (threaded subtree, not threaded root).
pub const CGROUP_TYPE_DOMAIN_INVALID: u32 = 3;

// ---------------------------------------------------------------------------
// Freeze state
// ---------------------------------------------------------------------------

/// Not frozen.
pub const CGROUP_FREEZE_UNFROZEN: u32 = 0;
/// Frozen.
pub const CGROUP_FREEZE_FROZEN: u32 = 1;

// ---------------------------------------------------------------------------
// cgroup.events keys (bit positions in cgroup.events bitmask)
// ---------------------------------------------------------------------------

/// cgroup is populated (has live processes).
pub const CGROUP_POPULATED: u32 = 1 << 0;
/// cgroup is frozen.
pub const CGROUP_FROZEN: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cgroup_magic() {
        assert_eq!(CGROUP2_SUPER_MAGIC, 0x63677270);
        assert_ne!(CGROUP_SUPER_MAGIC, CGROUP2_SUPER_MAGIC);
    }

    #[test]
    fn test_controllers_powers_of_two() {
        let ctrls = [
            CGROUP_CTRL_CPU, CGROUP_CTRL_MEMORY, CGROUP_CTRL_IO,
            CGROUP_CTRL_PIDS, CGROUP_CTRL_RDMA, CGROUP_CTRL_HUGETLB,
            CGROUP_CTRL_CPUSET, CGROUP_CTRL_MISC,
        ];
        for c in &ctrls {
            assert!(c.is_power_of_two(), "ctrl {c:#x} not power of 2");
        }
    }

    #[test]
    fn test_types_distinct() {
        let types = [
            CGROUP_TYPE_DOMAIN, CGROUP_TYPE_THREADED,
            CGROUP_TYPE_DOMAIN_THREADED, CGROUP_TYPE_DOMAIN_INVALID,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_freeze_states() {
        assert_eq!(CGROUP_FREEZE_UNFROZEN, 0);
        assert_eq!(CGROUP_FREEZE_FROZEN, 1);
    }
}
