//! `<linux/nodemask.h>` — NUMA topology extended constants.
//!
//! On NUMA (Non-Uniform Memory Access) systems, memory and CPUs are
//! organized into nodes. Accessing memory local to a CPU's node is
//! fast; accessing remote node memory incurs additional latency
//! (typically 1.5-3x). The kernel's NUMA-aware allocator, scheduler,
//! and page migration policies work together to keep data close to
//! the CPUs that use it. These constants supplement the basic NUMA
//! types with node distance, memory policy, and topology information.

// ---------------------------------------------------------------------------
// NUMA memory policies (set_mempolicy / mbind)
// ---------------------------------------------------------------------------

/// Default policy (allocate from local node).
pub const MPOL_DEFAULT: u32 = 0;
/// Preferred node (try this node first, fallback to others).
pub const MPOL_PREFERRED: u32 = 1;
/// Bind to specified nodes only (fail if no memory available).
pub const MPOL_BIND: u32 = 2;
/// Interleave across specified nodes (round-robin pages).
pub const MPOL_INTERLEAVE: u32 = 3;
/// Local allocation (same as default but explicit).
pub const MPOL_LOCAL: u32 = 4;
/// Preferred many (try multiple preferred nodes before fallback).
pub const MPOL_PREFERRED_MANY: u32 = 5;
/// Maximum policy value.
pub const MPOL_MAX: u32 = 6;

// ---------------------------------------------------------------------------
// NUMA policy flags
// ---------------------------------------------------------------------------

/// Apply policy statically (don't move pages on migration).
pub const MPOL_F_STATIC_NODES: u32 = 0x01;
/// Remap nodes on policy rebind (relative node numbering).
pub const MPOL_F_RELATIVE_NODES: u32 = 0x02;
/// NUMA balancing enabled for this range.
pub const MPOL_F_NUMA_BALANCING: u32 = 0x04;

// ---------------------------------------------------------------------------
// NUMA node distances
// ---------------------------------------------------------------------------

/// Local distance (same node).
pub const NUMA_DISTANCE_LOCAL: u32 = 10;
/// Remote distance (adjacent node, typical).
pub const NUMA_DISTANCE_REMOTE: u32 = 20;
/// Maximum representable distance.
pub const NUMA_DISTANCE_MAX: u32 = 255;
/// Unreachable (no path between nodes).
pub const NUMA_DISTANCE_UNREACHABLE: u32 = 0;

// ---------------------------------------------------------------------------
// NUMA limits
// ---------------------------------------------------------------------------

/// Maximum number of NUMA nodes.
pub const MAX_NUMNODES: u32 = 1024;
/// Bits needed for node mask.
pub const NODES_SHIFT: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policies_distinct() {
        let policies = [
            MPOL_DEFAULT, MPOL_PREFERRED, MPOL_BIND,
            MPOL_INTERLEAVE, MPOL_LOCAL, MPOL_PREFERRED_MANY,
        ];
        for i in 0..policies.len() {
            assert!(policies[i] < MPOL_MAX);
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
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
    fn test_distances() {
        assert!(NUMA_DISTANCE_LOCAL < NUMA_DISTANCE_REMOTE);
        assert!(NUMA_DISTANCE_REMOTE < NUMA_DISTANCE_MAX);
        assert_eq!(NUMA_DISTANCE_UNREACHABLE, 0);
    }

    #[test]
    fn test_limits() {
        assert!(MAX_NUMNODES > 0);
        assert_eq!(MAX_NUMNODES, 1 << NODES_SHIFT);
    }
}
