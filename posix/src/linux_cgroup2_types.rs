//! `<linux/cgroup.h>` — cgroup v2 (unified hierarchy) constants.
//!
//! cgroup v2 is the unified resource management framework in Linux.
//! A single hierarchy of cgroups controls CPU, memory, I/O, PID
//! limits, and more for process groups. Each cgroup directory
//! exposes controller-specific knobs as files. Used by systemd,
//! container runtimes (Docker, Podman), and orchestrators
//! (Kubernetes) for resource isolation and accounting.

// ---------------------------------------------------------------------------
// cgroup v2 controller types (for cgroup.controllers / cgroup.subtree_control)
// ---------------------------------------------------------------------------

/// CPU controller — CPU bandwidth/weight limits.
pub const CGROUP_CTRL_CPU: u32 = 1 << 0;
/// Memory controller — memory limits and accounting.
pub const CGROUP_CTRL_MEMORY: u32 = 1 << 1;
/// I/O controller — block I/O bandwidth/IOPS limits.
pub const CGROUP_CTRL_IO: u32 = 1 << 2;
/// PID controller — process count limits.
pub const CGROUP_CTRL_PIDS: u32 = 1 << 3;
/// RDMA controller — RDMA resource limits.
pub const CGROUP_CTRL_RDMA: u32 = 1 << 4;
/// HugeTLB controller — huge page limits.
pub const CGROUP_CTRL_HUGETLB: u32 = 1 << 5;
/// cpuset controller — CPU/memory node affinity.
pub const CGROUP_CTRL_CPUSET: u32 = 1 << 6;
/// Misc controller — miscellaneous scalar resources.
pub const CGROUP_CTRL_MISC: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// cgroup.type values
// ---------------------------------------------------------------------------

/// Domain cgroup (default — resource controllers enabled).
pub const CGROUP_TYPE_DOMAIN: u32 = 0;
/// Threaded cgroup (per-thread resource control).
pub const CGROUP_TYPE_THREADED: u32 = 1;
/// Domain-threaded cgroup (domain that acts as threaded root).
pub const CGROUP_TYPE_DOMAIN_THREADED: u32 = 2;
/// Domain-invalid (threaded child in a non-threaded subtree — error state).
pub const CGROUP_TYPE_DOMAIN_INVALID: u32 = 3;

// ---------------------------------------------------------------------------
// cgroup.freeze
// ---------------------------------------------------------------------------

/// cgroup is not frozen (processes running normally).
pub const CGROUP_FREEZE_OFF: u32 = 0;
/// cgroup is frozen (all processes stopped).
pub const CGROUP_FREEZE_ON: u32 = 1;

// ---------------------------------------------------------------------------
// Memory controller — memory.events fields
// ---------------------------------------------------------------------------

/// Memory usage hit the high boundary.
pub const CGROUP_MEM_EVENT_HIGH: u32 = 0;
/// Memory usage hit the max boundary (OOM imminent).
pub const CGROUP_MEM_EVENT_MAX: u32 = 1;
/// OOM killer was invoked in this cgroup.
pub const CGROUP_MEM_EVENT_OOM: u32 = 2;
/// OOM kill event.
pub const CGROUP_MEM_EVENT_OOM_KILL: u32 = 3;
/// OOM was prevented by an OOM group kill.
pub const CGROUP_MEM_EVENT_OOM_GROUP_KILL: u32 = 4;

// ---------------------------------------------------------------------------
// PSI (Pressure Stall Information) — shared with cgroups
// ---------------------------------------------------------------------------

/// Some tasks stalled (partial stall).
pub const CGROUP_PSI_SOME: u32 = 0;
/// All tasks stalled (full stall).
pub const CGROUP_PSI_FULL: u32 = 1;

// ---------------------------------------------------------------------------
// cgroup file types (cgroupfs)
// ---------------------------------------------------------------------------

/// cgroup.procs — list of PIDs.
pub const CGROUP_FILE_PROCS: u32 = 1;
/// cgroup.threads — list of TIDs.
pub const CGROUP_FILE_THREADS: u32 = 2;
/// cgroup.controllers — available controllers.
pub const CGROUP_FILE_CONTROLLERS: u32 = 3;
/// cgroup.subtree_control — enabled controllers for children.
pub const CGROUP_FILE_SUBTREE_CONTROL: u32 = 4;
/// cgroup.events — event counters.
pub const CGROUP_FILE_EVENTS: u32 = 5;
/// cgroup.stat — basic statistics.
pub const CGROUP_FILE_STAT: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controllers_no_overlap() {
        let ctrls = [
            CGROUP_CTRL_CPU,
            CGROUP_CTRL_MEMORY,
            CGROUP_CTRL_IO,
            CGROUP_CTRL_PIDS,
            CGROUP_CTRL_RDMA,
            CGROUP_CTRL_HUGETLB,
            CGROUP_CTRL_CPUSET,
            CGROUP_CTRL_MISC,
        ];
        for i in 0..ctrls.len() {
            assert!(ctrls[i].is_power_of_two());
            for j in (i + 1)..ctrls.len() {
                assert_eq!(ctrls[i] & ctrls[j], 0);
            }
        }
    }

    #[test]
    fn test_cgroup_types_distinct() {
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
    fn test_freeze_values() {
        assert_eq!(CGROUP_FREEZE_OFF, 0);
        assert_eq!(CGROUP_FREEZE_ON, 1);
        assert_ne!(CGROUP_FREEZE_OFF, CGROUP_FREEZE_ON);
    }

    #[test]
    fn test_mem_events_distinct() {
        let events = [
            CGROUP_MEM_EVENT_HIGH,
            CGROUP_MEM_EVENT_MAX,
            CGROUP_MEM_EVENT_OOM,
            CGROUP_MEM_EVENT_OOM_KILL,
            CGROUP_MEM_EVENT_OOM_GROUP_KILL,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_psi_distinct() {
        assert_ne!(CGROUP_PSI_SOME, CGROUP_PSI_FULL);
    }

    #[test]
    fn test_file_types_distinct() {
        let files = [
            CGROUP_FILE_PROCS,
            CGROUP_FILE_THREADS,
            CGROUP_FILE_CONTROLLERS,
            CGROUP_FILE_SUBTREE_CONTROL,
            CGROUP_FILE_EVENTS,
            CGROUP_FILE_STAT,
        ];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }
}
