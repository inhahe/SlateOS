//! `<linux/mempolicy.h>` — NUMA memory policy constants.
//!
//! Memory policies control how the kernel allocates physical pages
//! across NUMA nodes. They can be set per-process (`set_mempolicy`)
//! or per-VMA (`mbind`).

// ---------------------------------------------------------------------------
// Memory policy modes
// ---------------------------------------------------------------------------

/// Default policy (allocate on local node).
pub const MPOL_DEFAULT: u32 = 0;
/// Prefer a specific node.
pub const MPOL_PREFERRED: u32 = 1;
/// Strict binding to a set of nodes.
pub const MPOL_BIND: u32 = 2;
/// Interleave across nodes (round-robin).
pub const MPOL_INTERLEAVE: u32 = 3;
/// Local allocation (like default but explicit).
pub const MPOL_LOCAL: u32 = 4;
/// Preferred many nodes (kernel picks best).
pub const MPOL_PREFERRED_MANY: u32 = 5;
/// Weighted interleave across nodes.
pub const MPOL_WEIGHTED_INTERLEAVE: u32 = 6;

// ---------------------------------------------------------------------------
// Memory policy flags
// ---------------------------------------------------------------------------

/// Apply policy to existing pages (move them).
pub const MPOL_F_STATIC_NODES: u32 = 1 << 15;
/// Relative node numbering (within cpuset).
pub const MPOL_F_RELATIVE_NODES: u32 = 1 << 14;
/// Return NUMA node information.
pub const MPOL_F_NUMA_BALANCING: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// mbind flags
// ---------------------------------------------------------------------------

/// Verify existing pages match policy.
pub const MPOL_MF_STRICT: u32 = 1 << 0;
/// Move existing pages to match policy.
pub const MPOL_MF_MOVE: u32 = 1 << 1;
/// Move all pages (requires CAP_SYS_NICE).
pub const MPOL_MF_MOVE_ALL: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// get_mempolicy flags
// ---------------------------------------------------------------------------

/// Return the policy mode.
pub const MPOL_F_NODE: u32 = 1 << 0;
/// Return the node mask.
pub const MPOL_F_ADDR: u32 = 1 << 1;
/// Return the policy of the vma containing addr.
pub const MPOL_F_MEMS_ALLOWED: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            MPOL_DEFAULT,
            MPOL_PREFERRED,
            MPOL_BIND,
            MPOL_INTERLEAVE,
            MPOL_LOCAL,
            MPOL_PREFERRED_MANY,
            MPOL_WEIGHTED_INTERLEAVE,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_default_is_zero() {
        assert_eq!(MPOL_DEFAULT, 0);
    }

    #[test]
    fn test_policy_flags_no_overlap() {
        let flags = [
            MPOL_F_STATIC_NODES,
            MPOL_F_RELATIVE_NODES,
            MPOL_F_NUMA_BALANCING,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_mbind_flags_no_overlap() {
        let flags = [MPOL_MF_STRICT, MPOL_MF_MOVE, MPOL_MF_MOVE_ALL];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_get_flags_no_overlap() {
        let flags = [MPOL_F_NODE, MPOL_F_ADDR, MPOL_F_MEMS_ALLOWED];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_mbind_flags_power_of_two() {
        assert!(MPOL_MF_STRICT.is_power_of_two());
        assert!(MPOL_MF_MOVE.is_power_of_two());
        assert!(MPOL_MF_MOVE_ALL.is_power_of_two());
    }
}
