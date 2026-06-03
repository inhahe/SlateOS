//! `<numaif.h>` / `<linux/mempolicy.h>` — NUMA userspace ABI.
//!
//! `libnuma`, `numactl`, and database engines (Postgres, MySQL,
//! Oracle) use these constants to pin allocations to specific NUMA
//! nodes via `mbind(2)`, `set_mempolicy(2)`, and `move_pages(2)`.

// ---------------------------------------------------------------------------
// `mbind` policy modes (`MPOL_*`)
// ---------------------------------------------------------------------------
//
// Re-exposed here alongside the syscall numbers because the NUMA
// userland tools see them as a single API surface (not the same as
// the lower-level `<linux/mempolicy.h>` view in
// `linux_mempolicy_user_types`).

pub const MPOL_DEFAULT: u32 = 0;
pub const MPOL_PREFERRED: u32 = 1;
pub const MPOL_BIND: u32 = 2;
pub const MPOL_INTERLEAVE: u32 = 3;
pub const MPOL_LOCAL: u32 = 4;
pub const MPOL_PREFERRED_MANY: u32 = 5;
pub const MPOL_WEIGHTED_INTERLEAVE: u32 = 6;

// ---------------------------------------------------------------------------
// `move_pages` flags
// ---------------------------------------------------------------------------

pub const MPOL_MF_MOVE: u32 = 1 << 1;
pub const MPOL_MF_MOVE_ALL: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// `numa_maps` / `/proc/<pid>/numa_maps` sysfs paths
// ---------------------------------------------------------------------------

pub const SYSFS_NODE_ROOT: &str = "/sys/devices/system/node";
pub const PROC_PID_NUMA_MAPS: &str = "/proc/self/numa_maps";

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Compile-time cap on NUMA nodes the userspace ABI exposes. The kernel
/// internally supports more on giant machines, but `libnuma` interfaces
/// truncate to this for backward compatibility.
pub const NUMA_NUM_NODES_USERLAND: u32 = 1024;

/// Sentinel returned by `numa_node_of_cpu()` when the CPU isn't in any
/// node — i.e. the system isn't NUMA.
pub const NUMA_NO_NODE: i32 = -1;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_MBIND: u32 = 237;
pub const NR_GET_MEMPOLICY: u32 = 239;
pub const NR_SET_MEMPOLICY: u32 = 238;
pub const NR_MIGRATE_PAGES: u32 = 256;
pub const NR_MOVE_PAGES: u32 = 279;
pub const NR_SET_MEMPOLICY_HOME_NODE: u32 = 450;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mpol_modes_dense_0_to_6() {
        let m = [
            MPOL_DEFAULT,
            MPOL_PREFERRED,
            MPOL_BIND,
            MPOL_INTERLEAVE,
            MPOL_LOCAL,
            MPOL_PREFERRED_MANY,
            MPOL_WEIGHTED_INTERLEAVE,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_move_flags_disjoint_bits() {
        // MOVE and MOVE_ALL each pick a single distinct bit.
        assert!(MPOL_MF_MOVE.is_power_of_two());
        assert!(MPOL_MF_MOVE_ALL.is_power_of_two());
        assert_ne!(MPOL_MF_MOVE, MPOL_MF_MOVE_ALL);
    }

    #[test]
    fn test_sysfs_paths_look_right() {
        assert!(SYSFS_NODE_ROOT.starts_with("/sys/devices/system/node"));
        assert!(PROC_PID_NUMA_MAPS.ends_with("/numa_maps"));
    }

    #[test]
    fn test_no_node_is_negative_one() {
        assert_eq!(NUMA_NO_NODE, -1);
        // 1024 is a power of two and matches glibc's MAX_NUMNODES_USERLAND.
        assert_eq!(NUMA_NUM_NODES_USERLAND, 1024);
        assert!(NUMA_NUM_NODES_USERLAND.is_power_of_two());
    }

    #[test]
    fn test_syscall_numbers_distinct() {
        let n = [
            NR_MBIND,
            NR_GET_MEMPOLICY,
            NR_SET_MEMPOLICY,
            NR_MIGRATE_PAGES,
            NR_MOVE_PAGES,
            NR_SET_MEMPOLICY_HOME_NODE,
        ];
        for i in 0..n.len() {
            for j in (i + 1)..n.len() {
                assert_ne!(n[i], n[j]);
            }
        }
        // SET_MEMPOLICY_HOME_NODE was added in 5.17 (syscall 450).
        assert_eq!(NR_SET_MEMPOLICY_HOME_NODE, 450);
    }
}
