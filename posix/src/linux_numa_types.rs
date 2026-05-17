//! `<linux/mempolicy.h>` — NUMA memory policy constants.
//!
//! NUMA (Non-Uniform Memory Access) policies control which memory
//! nodes the kernel uses for allocations. Policies can be set per-process
//! (set_mempolicy), per-VMA (mbind), or inherited. They determine
//! whether allocations are local, interleaved across nodes, bound to
//! specific nodes, or preferring particular nodes.

// ---------------------------------------------------------------------------
// Memory policy modes (set_mempolicy / mbind first argument)
// ---------------------------------------------------------------------------

/// Default policy (allocate on local node).
pub const MPOL_DEFAULT: u32 = 0;
/// Prefer allocations on specified node(s).
pub const MPOL_PREFERRED: u32 = 1;
/// Restrict allocations to specified nodes.
pub const MPOL_BIND: u32 = 2;
/// Interleave allocations round-robin across nodes.
pub const MPOL_INTERLEAVE: u32 = 3;
/// Local allocation (like DEFAULT but explicit).
pub const MPOL_LOCAL: u32 = 4;
/// Preferred many nodes (kernel picks best from set).
pub const MPOL_PREFERRED_MANY: u32 = 5;

// ---------------------------------------------------------------------------
// Memory policy flags (OR with mode)
// ---------------------------------------------------------------------------

/// Apply policy to existing pages (move them).
pub const MPOL_F_STATIC_NODES: u32 = 1 << 15;
/// Remap node IDs relative to cpuset.
pub const MPOL_F_RELATIVE_NODES: u32 = 1 << 14;
/// NUMA balancing hint.
pub const MPOL_F_NUMA_BALANCING: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// mbind flags
// ---------------------------------------------------------------------------

/// Verify existing pages conform to policy.
pub const MPOL_MF_STRICT: u32 = 1 << 0;
/// Move existing pages to conform.
pub const MPOL_MF_MOVE: u32 = 1 << 1;
/// Move all pages (even shared, requires CAP_SYS_NICE).
pub const MPOL_MF_MOVE_ALL: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// get_mempolicy flags
// ---------------------------------------------------------------------------

/// Return next interleave node.
pub const MPOL_F_NODE: u32 = 1 << 0;
/// Return policy for address, not task.
pub const MPOL_F_ADDR: u32 = 1 << 1;
/// Return node mask of allowed nodes.
pub const MPOL_F_MEMS_ALLOWED: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// NUMA node limits
// ---------------------------------------------------------------------------

/// Maximum NUMA nodes supported.
pub const MAX_NUMNODES: u32 = 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_modes_distinct() {
        let modes = [
            MPOL_DEFAULT, MPOL_PREFERRED, MPOL_BIND,
            MPOL_INTERLEAVE, MPOL_LOCAL, MPOL_PREFERRED_MANY,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_policy_flags_no_overlap() {
        let flags = [
            MPOL_F_STATIC_NODES, MPOL_F_RELATIVE_NODES,
            MPOL_F_NUMA_BALANCING,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_mbind_flags_no_overlap() {
        let flags = [MPOL_MF_STRICT, MPOL_MF_MOVE, MPOL_MF_MOVE_ALL];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_get_flags_no_overlap() {
        let flags = [MPOL_F_NODE, MPOL_F_ADDR, MPOL_F_MEMS_ALLOWED];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_max_numnodes() {
        assert!(MAX_NUMNODES.is_power_of_two());
    }
}
