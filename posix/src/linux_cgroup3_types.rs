//! `<linux/cgroup.h>` — Additional cgroup v2 constants.
//!
//! Supplementary cgroup constants covering controller types,
//! freeze states, pressure stall types, and thread modes.

// ---------------------------------------------------------------------------
// Cgroup controllers (bit positions)
// ---------------------------------------------------------------------------

/// CPU controller.
pub const CGROUP_CTRL_CPU: u32 = 1 << 0;
/// Memory controller.
pub const CGROUP_CTRL_MEMORY: u32 = 1 << 1;
/// IO controller.
pub const CGROUP_CTRL_IO: u32 = 1 << 2;
/// PID controller.
pub const CGROUP_CTRL_PID: u32 = 1 << 3;
/// RDMA controller.
pub const CGROUP_CTRL_RDMA: u32 = 1 << 4;
/// HugeTLB controller.
pub const CGROUP_CTRL_HUGETLB: u32 = 1 << 5;
/// Cpuset controller.
pub const CGROUP_CTRL_CPUSET: u32 = 1 << 6;
/// Misc controller.
pub const CGROUP_CTRL_MISC: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Cgroup freeze states
// ---------------------------------------------------------------------------

/// Not frozen.
pub const CGROUP_FREEZE_NOT_FROZEN: u32 = 0;
/// Freezing in progress.
pub const CGROUP_FREEZE_FREEZING: u32 = 1;
/// Fully frozen.
pub const CGROUP_FREEZE_FROZEN: u32 = 2;

// ---------------------------------------------------------------------------
// Cgroup event types
// ---------------------------------------------------------------------------

/// Populated event.
pub const CGROUP_EVENT_POPULATED: u32 = 1 << 0;
/// Frozen event.
pub const CGROUP_EVENT_FROZEN: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Cgroup thread modes
// ---------------------------------------------------------------------------

/// Domain mode (default).
pub const CGROUP_TYPE_DOMAIN: u32 = 0;
/// Threaded mode.
pub const CGROUP_TYPE_THREADED: u32 = 1;
/// Domain threaded.
pub const CGROUP_TYPE_DOMAIN_THREADED: u32 = 2;
/// Domain invalid.
pub const CGROUP_TYPE_DOMAIN_INVALID: u32 = 3;

// ---------------------------------------------------------------------------
// PSI (Pressure Stall Information) types
// ---------------------------------------------------------------------------

/// Some tasks stalled.
pub const PSI_SOME: u32 = 0;
/// Full stall (all tasks).
pub const PSI_FULL: u32 = 1;

// ---------------------------------------------------------------------------
// PSI resource types
// ---------------------------------------------------------------------------

/// CPU pressure.
pub const PSI_CPU: u32 = 0;
/// Memory pressure.
pub const PSI_MEM: u32 = 1;
/// IO pressure.
pub const PSI_IO: u32 = 2;
/// IRQ pressure.
pub const PSI_IRQ: u32 = 3;

// ---------------------------------------------------------------------------
// Cgroup file types (kernfs)
// ---------------------------------------------------------------------------

/// Regular file.
pub const CGROUP_FILE_REGULAR: u32 = 1;
/// Link.
pub const CGROUP_FILE_LINK: u32 = 2;
/// Directory.
pub const CGROUP_FILE_DIR: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controllers_power_of_two() {
        let ctrls = [
            CGROUP_CTRL_CPU, CGROUP_CTRL_MEMORY, CGROUP_CTRL_IO,
            CGROUP_CTRL_PID, CGROUP_CTRL_RDMA, CGROUP_CTRL_HUGETLB,
            CGROUP_CTRL_CPUSET, CGROUP_CTRL_MISC,
        ];
        for c in &ctrls {
            assert!(c.is_power_of_two(), "0x{:02x} not power of two", c);
        }
    }

    #[test]
    fn test_controllers_no_overlap() {
        let ctrls = [
            CGROUP_CTRL_CPU, CGROUP_CTRL_MEMORY, CGROUP_CTRL_IO,
            CGROUP_CTRL_PID, CGROUP_CTRL_RDMA, CGROUP_CTRL_HUGETLB,
            CGROUP_CTRL_CPUSET, CGROUP_CTRL_MISC,
        ];
        for i in 0..ctrls.len() {
            for j in (i + 1)..ctrls.len() {
                assert_eq!(ctrls[i] & ctrls[j], 0);
            }
        }
    }

    #[test]
    fn test_freeze_states_distinct() {
        let states = [
            CGROUP_FREEZE_NOT_FROZEN, CGROUP_FREEZE_FREEZING,
            CGROUP_FREEZE_FROZEN,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_thread_modes_distinct() {
        let modes = [
            CGROUP_TYPE_DOMAIN, CGROUP_TYPE_THREADED,
            CGROUP_TYPE_DOMAIN_THREADED, CGROUP_TYPE_DOMAIN_INVALID,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_psi_types() {
        assert_eq!(PSI_SOME, 0);
        assert_eq!(PSI_FULL, 1);
    }

    #[test]
    fn test_psi_resources_distinct() {
        let res = [PSI_CPU, PSI_MEM, PSI_IO, PSI_IRQ];
        for i in 0..res.len() {
            for j in (i + 1)..res.len() {
                assert_ne!(res[i], res[j]);
            }
        }
    }
}
