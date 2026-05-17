//! `<linux/cgroup.h>` — Control group (cgroup) v2 constants.
//!
//! cgroups organize processes into hierarchical groups for resource
//! management. cgroup v2 provides a unified hierarchy with controllers
//! for CPU, memory, I/O, PID limits, and more. Processes are placed
//! in cgroups by writing PIDs to cgroup.procs files, and resource
//! limits are set via controller-specific interface files.

// ---------------------------------------------------------------------------
// cgroup v2 controller types
// ---------------------------------------------------------------------------

/// CPU controller (bandwidth, weight).
pub const CGROUP_CTRL_CPU: u32 = 1 << 0;
/// Memory controller (usage limits, OOM).
pub const CGROUP_CTRL_MEMORY: u32 = 1 << 1;
/// I/O controller (bandwidth, IOPS limits).
pub const CGROUP_CTRL_IO: u32 = 1 << 2;
/// PID controller (process count limits).
pub const CGROUP_CTRL_PIDS: u32 = 1 << 3;
/// RDMA controller (resource limits for RDMA devices).
pub const CGROUP_CTRL_RDMA: u32 = 1 << 4;
/// HugeTLB controller (huge page limits).
pub const CGROUP_CTRL_HUGETLB: u32 = 1 << 5;
/// CPU set controller (CPU/NUMA affinity).
pub const CGROUP_CTRL_CPUSET: u32 = 1 << 6;
/// Misc controller (scalar resources).
pub const CGROUP_CTRL_MISC: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// cgroup type constants
// ---------------------------------------------------------------------------

/// Domain cgroup (default type).
pub const CGROUP_TYPE_DOMAIN: u32 = 0;
/// Threaded cgroup (for per-thread resource control).
pub const CGROUP_TYPE_THREADED: u32 = 1;

// ---------------------------------------------------------------------------
// Memory controller constants
// ---------------------------------------------------------------------------

/// Memory usage high (throttling boundary, not hard limit).
pub const CGROUP_MEM_HIGH_MAX: u64 = u64::MAX;
/// Memory usage max (hard OOM limit).
pub const CGROUP_MEM_MAX_MAX: u64 = u64::MAX;
/// Minimum memory guarantee.
pub const CGROUP_MEM_MIN_DEFAULT: u64 = 0;

// ---------------------------------------------------------------------------
// cgroup freeze/thaw
// ---------------------------------------------------------------------------

/// cgroup is not frozen.
pub const CGROUP_FROZEN_FALSE: u32 = 0;
/// cgroup is frozen (all processes SIGSTOPped).
pub const CGROUP_FROZEN_TRUE: u32 = 1;

// ---------------------------------------------------------------------------
// cgroup event types (from cgroup.events)
// ---------------------------------------------------------------------------

/// cgroup has live processes.
pub const CGROUP_EVENT_POPULATED: u32 = 1;
/// cgroup is frozen.
pub const CGROUP_EVENT_FROZEN: u32 = 2;

// ---------------------------------------------------------------------------
// PID controller limits
// ---------------------------------------------------------------------------

/// Maximum value for pids.max (unlimited).
pub const CGROUP_PIDS_MAX: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_flags_no_overlap() {
        let ctrls = [
            CGROUP_CTRL_CPU, CGROUP_CTRL_MEMORY, CGROUP_CTRL_IO,
            CGROUP_CTRL_PIDS, CGROUP_CTRL_RDMA, CGROUP_CTRL_HUGETLB,
            CGROUP_CTRL_CPUSET, CGROUP_CTRL_MISC,
        ];
        for i in 0..ctrls.len() {
            assert!(ctrls[i].is_power_of_two());
            for j in (i + 1)..ctrls.len() {
                assert_eq!(ctrls[i] & ctrls[j], 0);
            }
        }
    }

    #[test]
    fn test_types_distinct() {
        assert_ne!(CGROUP_TYPE_DOMAIN, CGROUP_TYPE_THREADED);
    }

    #[test]
    fn test_freeze_states_distinct() {
        assert_ne!(CGROUP_FROZEN_FALSE, CGROUP_FROZEN_TRUE);
    }

    #[test]
    fn test_events_distinct() {
        assert_ne!(CGROUP_EVENT_POPULATED, CGROUP_EVENT_FROZEN);
    }

    #[test]
    fn test_memory_defaults() {
        assert_eq!(CGROUP_MEM_HIGH_MAX, u64::MAX);
        assert_eq!(CGROUP_MEM_MAX_MAX, u64::MAX);
        assert_eq!(CGROUP_MEM_MIN_DEFAULT, 0);
    }
}
