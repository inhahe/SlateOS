//! `<linux/cgroup.h>` — Cgroup freezer and controller constants.
//!
//! The cgroup freezer allows suspending and resuming groups
//! of processes.  These constants define freezer states,
//! cgroup controller types, and cgroup v2 interface parameters.

// ---------------------------------------------------------------------------
// Cgroup freezer states
// ---------------------------------------------------------------------------

/// Thawed (running normally).
pub const CGROUP_FREEZER_THAWED: u32 = 0;
/// Freezing (in progress).
pub const CGROUP_FREEZER_FREEZING: u32 = 1;
/// Frozen (all tasks suspended).
pub const CGROUP_FREEZER_FROZEN: u32 = 2;

// ---------------------------------------------------------------------------
// Cgroup controller IDs
// ---------------------------------------------------------------------------

/// CPU controller.
pub const CGROUP_CTRL_CPU: u32 = 0;
/// Memory controller.
pub const CGROUP_CTRL_MEMORY: u32 = 1;
/// IO controller.
pub const CGROUP_CTRL_IO: u32 = 2;
/// PID controller.
pub const CGROUP_CTRL_PID: u32 = 3;
/// RDMA controller.
pub const CGROUP_CTRL_RDMA: u32 = 4;
/// HugeTLB controller.
pub const CGROUP_CTRL_HUGETLB: u32 = 5;
/// Cpuset controller.
pub const CGROUP_CTRL_CPUSET: u32 = 6;
/// Misc controller.
pub const CGROUP_CTRL_MISC: u32 = 7;

// ---------------------------------------------------------------------------
// Cgroup v2 subtree control flags
// ---------------------------------------------------------------------------

/// Enable CPU.
pub const CGRP_CTRL_CPU_BIT: u32 = 1 << 0;
/// Enable memory.
pub const CGRP_CTRL_MEMORY_BIT: u32 = 1 << 1;
/// Enable IO.
pub const CGRP_CTRL_IO_BIT: u32 = 1 << 2;
/// Enable PID.
pub const CGRP_CTRL_PID_BIT: u32 = 1 << 3;
/// Enable RDMA.
pub const CGRP_CTRL_RDMA_BIT: u32 = 1 << 4;
/// Enable HugeTLB.
pub const CGRP_CTRL_HUGETLB_BIT: u32 = 1 << 5;
/// Enable cpuset.
pub const CGRP_CTRL_CPUSET_BIT: u32 = 1 << 6;
/// Enable misc.
pub const CGRP_CTRL_MISC_BIT: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Cgroup event types
// ---------------------------------------------------------------------------

/// Populated event (cgroup has/lost tasks).
pub const CGROUP_EVENT_POPULATED: u32 = 0;
/// Frozen event.
pub const CGROUP_EVENT_FROZEN: u32 = 1;

// ---------------------------------------------------------------------------
// Cgroup type
// ---------------------------------------------------------------------------

/// Domain (default, no restrictions).
pub const CGROUP_TYPE_DOMAIN: u32 = 0;
/// Threaded (thread-level control).
pub const CGROUP_TYPE_THREADED: u32 = 1;
/// Domain threaded (transitional).
pub const CGROUP_TYPE_DOMAIN_THREADED: u32 = 2;
/// Domain invalid.
pub const CGROUP_TYPE_DOMAIN_INVALID: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freezer_states_distinct() {
        let states = [
            CGROUP_FREEZER_THAWED,
            CGROUP_FREEZER_FREEZING,
            CGROUP_FREEZER_FROZEN,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_ctrl_ids_distinct() {
        let ids = [
            CGROUP_CTRL_CPU,
            CGROUP_CTRL_MEMORY,
            CGROUP_CTRL_IO,
            CGROUP_CTRL_PID,
            CGROUP_CTRL_RDMA,
            CGROUP_CTRL_HUGETLB,
            CGROUP_CTRL_CPUSET,
            CGROUP_CTRL_MISC,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_ctrl_bits_powers_of_two() {
        let bits = [
            CGRP_CTRL_CPU_BIT,
            CGRP_CTRL_MEMORY_BIT,
            CGRP_CTRL_IO_BIT,
            CGRP_CTRL_PID_BIT,
            CGRP_CTRL_RDMA_BIT,
            CGRP_CTRL_HUGETLB_BIT,
            CGRP_CTRL_CPUSET_BIT,
            CGRP_CTRL_MISC_BIT,
        ];
        for b in &bits {
            assert!(b.is_power_of_two());
        }
    }

    #[test]
    fn test_ctrl_bits_no_overlap() {
        let bits = [
            CGRP_CTRL_CPU_BIT,
            CGRP_CTRL_MEMORY_BIT,
            CGRP_CTRL_IO_BIT,
            CGRP_CTRL_PID_BIT,
            CGRP_CTRL_RDMA_BIT,
            CGRP_CTRL_HUGETLB_BIT,
            CGRP_CTRL_CPUSET_BIT,
            CGRP_CTRL_MISC_BIT,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_types_distinct() {
        let types = [
            CGROUP_TYPE_DOMAIN,
            CGROUP_TYPE_THREADED,
            CGROUP_TYPE_DOMAIN_THREADED,
            CGROUP_TYPE_DOMAIN_INVALID,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_thawed_is_zero() {
        assert_eq!(CGROUP_FREEZER_THAWED, 0);
    }

    #[test]
    fn test_events_distinct() {
        assert_ne!(CGROUP_EVENT_POPULATED, CGROUP_EVENT_FROZEN);
    }
}
