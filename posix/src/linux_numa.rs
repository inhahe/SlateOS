//! `<linux/numa.h>` — NUMA (Non-Uniform Memory Access) constants.
//!
//! NUMA systems have multiple memory nodes with different access
//! latencies depending on which CPU accesses which node. The kernel
//! tracks NUMA topology for memory allocation and scheduling
//! decisions. This module defines node limits and policy constants.

// ---------------------------------------------------------------------------
// Node limits
// ---------------------------------------------------------------------------

/// Maximum number of NUMA nodes.
pub const MAX_NUMNODES: u32 = 1024;

/// No node preference.
pub const NUMA_NO_NODE: i32 = -1;

// ---------------------------------------------------------------------------
// Memory policy constants (from <linux/mempolicy.h>)
// ---------------------------------------------------------------------------

/// Default policy (local allocation).
pub const MPOL_DEFAULT: u32 = 0;
/// Preferred node.
pub const MPOL_PREFERRED: u32 = 1;
/// Bind to a set of nodes.
pub const MPOL_BIND: u32 = 2;
/// Interleave across nodes.
pub const MPOL_INTERLEAVE: u32 = 3;
/// Local allocation.
pub const MPOL_LOCAL: u32 = 4;
/// Preferred-many (multiple preferred nodes).
pub const MPOL_PREFERRED_MANY: u32 = 5;
/// Weighted interleave.
pub const MPOL_WEIGHTED_INTERLEAVE: u32 = 6;
/// Maximum policy value.
pub const MPOL_MAX: u32 = 7;

// ---------------------------------------------------------------------------
// Memory policy flags
// ---------------------------------------------------------------------------

/// Use static nodes (don't remap on hotplug).
pub const MPOL_F_STATIC_NODES: u32 = 1 << 15;
/// Use relative node IDs.
pub const MPOL_F_RELATIVE_NODES: u32 = 1 << 14;
/// NUMA balancing.
pub const MPOL_F_NUMA_BALANCING: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// mbind / set_mempolicy mode flags
// ---------------------------------------------------------------------------

/// Move existing pages to match policy.
pub const MPOL_MF_STRICT: u32 = 1 << 0;
/// Move pages even if they don't match.
pub const MPOL_MF_MOVE: u32 = 1 << 1;
/// Move pages for all processes.
pub const MPOL_MF_MOVE_ALL: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Distance constants
// ---------------------------------------------------------------------------

/// Local node distance.
pub const LOCAL_DISTANCE: u32 = 10;
/// Remote node distance (typical).
pub const REMOTE_DISTANCE: u32 = 20;
/// Unreachable distance.
pub const DISTANCE_UNREACHABLE: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_numnodes() {
        assert_eq!(MAX_NUMNODES, 1024);
    }

    #[test]
    fn test_no_node() {
        assert_eq!(NUMA_NO_NODE, -1);
    }

    #[test]
    fn test_policies_distinct() {
        let policies = [
            MPOL_DEFAULT,
            MPOL_PREFERRED,
            MPOL_BIND,
            MPOL_INTERLEAVE,
            MPOL_LOCAL,
            MPOL_PREFERRED_MANY,
            MPOL_WEIGHTED_INTERLEAVE,
        ];
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
            }
        }
    }

    #[test]
    fn test_policies_below_max() {
        let policies = [
            MPOL_DEFAULT,
            MPOL_PREFERRED,
            MPOL_BIND,
            MPOL_INTERLEAVE,
            MPOL_LOCAL,
            MPOL_PREFERRED_MANY,
            MPOL_WEIGHTED_INTERLEAVE,
        ];
        for p in &policies {
            assert!(*p < MPOL_MAX);
        }
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
    fn test_mf_flags_powers_of_two() {
        let flags = [MPOL_MF_STRICT, MPOL_MF_MOVE, MPOL_MF_MOVE_ALL];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_distances() {
        assert!(LOCAL_DISTANCE < REMOTE_DISTANCE);
        assert!(REMOTE_DISTANCE < DISTANCE_UNREACHABLE);
    }

    #[test]
    fn test_distance_values() {
        assert_eq!(LOCAL_DISTANCE, 10);
        assert_eq!(REMOTE_DISTANCE, 20);
        assert_eq!(DISTANCE_UNREACHABLE, 255);
    }
}
