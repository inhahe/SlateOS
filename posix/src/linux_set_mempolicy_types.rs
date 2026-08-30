//! `<linux/mempolicy.h>` — NUMA memory-policy syscall constants.
//!
//! `set_mempolicy(2)`, `mbind(2)`, `get_mempolicy(2)`, and
//! `move_pages(2)` use the constants below to select a NUMA
//! allocation policy. numactl, libnuma, and JVM/HPC runtimes
//! consume these values.

// ---------------------------------------------------------------------------
// Policy modes (MPOL_*)
// ---------------------------------------------------------------------------

/// Default policy — inherit from parent, then system default.
pub const MPOL_DEFAULT: u32 = 0;
/// Allocate from a single preferred node, fall back if full.
pub const MPOL_PREFERRED: u32 = 1;
/// Allocate strictly from the given nodemask.
pub const MPOL_BIND: u32 = 2;
/// Interleave allocations across the nodemask in page order.
pub const MPOL_INTERLEAVE: u32 = 3;
/// Allocate from the node of the CPU that touched the page first.
pub const MPOL_LOCAL: u32 = 4;
/// Multi-node preferred (5.15+).
pub const MPOL_PREFERRED_MANY: u32 = 5;
/// Weighted interleave (6.9+).
pub const MPOL_WEIGHTED_INTERLEAVE: u32 = 6;

/// Highest defined mode + 1 — used as bounds-check upper limit.
pub const MPOL_MAX: u32 = 7;

// ---------------------------------------------------------------------------
// Mode flags (OR'd into the high bits of the mode argument)
// ---------------------------------------------------------------------------

/// Interpret nodemask as relative to the cpuset.
pub const MPOL_F_STATIC_NODES: u32 = 1 << 15;
/// Renumber nodemask when the cpuset changes.
pub const MPOL_F_RELATIVE_NODES: u32 = 1 << 14;
/// Apply the mode to the numa-balancing infrastructure.
pub const MPOL_F_NUMA_BALANCING: u32 = 1 << 13;

/// Mask covering all valid mode-flag bits.
pub const MPOL_MODE_FLAGS: u32 =
    MPOL_F_STATIC_NODES | MPOL_F_RELATIVE_NODES | MPOL_F_NUMA_BALANCING;

// ---------------------------------------------------------------------------
// get_mempolicy() flags
// ---------------------------------------------------------------------------

/// Return node for the address (rather than the policy).
pub const MPOL_F_NODE: u32 = 1 << 0;
/// Look up the policy attached to a virtual address.
pub const MPOL_F_ADDR: u32 = 1 << 1;
/// Look up the per-process-default policy (ignore VMA).
pub const MPOL_F_MEMS_ALLOWED: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// mbind() flags
// ---------------------------------------------------------------------------

/// Move pages that don't follow the policy.
pub const MPOL_MF_MOVE: u32 = 1 << 1;
/// Move all pages (including shared) — needs CAP_SYS_NICE.
pub const MPOL_MF_MOVE_ALL: u32 = 1 << 2;
/// Strict — return -EIO if pages can't be moved.
pub const MPOL_MF_STRICT: u32 = 1 << 0;
/// Lazily migrate pages on the next NUMA fault.
pub const MPOL_MF_LAZY: u32 = 1 << 3;
/// Internal flag — paired with one of the above to indicate "non-VMA
/// driven" (used by the kernel; userspace must mask it out).
pub const MPOL_MF_INTERNAL: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// move_pages() status / error sentinels
// ---------------------------------------------------------------------------

/// `move_pages` failure: pages were not migrated.
pub const MPOL_MF_VALID: u32 = MPOL_MF_STRICT | MPOL_MF_MOVE | MPOL_MF_MOVE_ALL;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_dense_and_default_zero() {
        // Modes must be a dense enum starting at DEFAULT=0, otherwise
        // set_mempolicy()'s `mode < MPOL_MAX` validation would let
        // unintended numeric values slip through.
        let modes = [
            MPOL_DEFAULT,
            MPOL_PREFERRED,
            MPOL_BIND,
            MPOL_INTERLEAVE,
            MPOL_LOCAL,
            MPOL_PREFERRED_MANY,
            MPOL_WEIGHTED_INTERLEAVE,
        ];
        for (i, &m) in modes.iter().enumerate() {
            assert_eq!(m as usize, i);
        }
        assert_eq!(MPOL_DEFAULT, 0);
        assert_eq!(MPOL_MAX as usize, modes.len());
    }

    #[test]
    fn test_mode_flags_pow2_distinct_and_high() {
        let f = [
            MPOL_F_STATIC_NODES,
            MPOL_F_RELATIVE_NODES,
            MPOL_F_NUMA_BALANCING,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
            // Mode flags must sit in the high bits so they don't
            // collide with mode numbers in the low bits.
            assert!(b >= 1 << 13);
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
        // Combined mask must include every flag bit.
        assert_eq!(
            MPOL_MODE_FLAGS,
            MPOL_F_STATIC_NODES | MPOL_F_RELATIVE_NODES | MPOL_F_NUMA_BALANCING
        );
    }

    #[test]
    fn test_get_mempolicy_flags_distinct_pow2() {
        let f = [MPOL_F_NODE, MPOL_F_ADDR, MPOL_F_MEMS_ALLOWED];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_mbind_flags_distinct_and_valid_mask() {
        let f = [
            MPOL_MF_MOVE,
            MPOL_MF_MOVE_ALL,
            MPOL_MF_STRICT,
            MPOL_MF_LAZY,
            MPOL_MF_INTERNAL,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
        // VALID mask must include exactly the userspace-permitted bits.
        assert_eq!(
            MPOL_MF_VALID,
            MPOL_MF_STRICT | MPOL_MF_MOVE | MPOL_MF_MOVE_ALL
        );
        assert_eq!(MPOL_MF_VALID & MPOL_MF_INTERNAL, 0);
    }
}
