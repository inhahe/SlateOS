//! `<linux/mempolicy.h>` — NUMA memory policy ABI.
//!
//! `set_mempolicy(2)`, `get_mempolicy(2)`, `mbind(2)`, and
//! `set_mempolicy_home_node(2)` configure how the kernel places
//! anonymous pages on NUMA nodes. HPC schedulers (Slurm, LSF), the
//! JVM's NUMA-aware GC, and `numactl(8)` are the primary clients.

// ---------------------------------------------------------------------------
// Policy modes (passed as `int mode`)
// ---------------------------------------------------------------------------

pub const MPOL_DEFAULT: u32 = 0;
pub const MPOL_PREFERRED: u32 = 1;
pub const MPOL_BIND: u32 = 2;
pub const MPOL_INTERLEAVE: u32 = 3;
pub const MPOL_LOCAL: u32 = 4;
pub const MPOL_PREFERRED_MANY: u32 = 5;
pub const MPOL_WEIGHTED_INTERLEAVE: u32 = 6;
pub const MPOL_MAX: u32 = MPOL_WEIGHTED_INTERLEAVE;

// ---------------------------------------------------------------------------
// Mode flags (high bits of `mode`)
// ---------------------------------------------------------------------------

/// Bit position of the mode-flags field.
pub const MPOL_MODE_FLAGS_SHIFT: u32 = 14;
/// The node list is interpreted as relative-to-cpuset.
pub const MPOL_F_STATIC_NODES: u32 = 1 << 15;
/// The node list is interpreted as relative-to-cpuset (relative form).
pub const MPOL_F_RELATIVE_NODES: u32 = 1 << 14;
/// Apply the policy to all pages in the range, not just new ones.
pub const MPOL_F_NUMA_BALANCING: u32 = 1 << 13;

/// Mask covering all valid mode flags.
pub const MPOL_MODE_FLAGS: u32 =
    MPOL_F_STATIC_NODES | MPOL_F_RELATIVE_NODES | MPOL_F_NUMA_BALANCING;

// ---------------------------------------------------------------------------
// `get_mempolicy(2)` flags (passed in `unsigned long flags`)
// ---------------------------------------------------------------------------

pub const MPOL_F_NODE: u32 = 1 << 0;
pub const MPOL_F_ADDR: u32 = 1 << 1;
pub const MPOL_F_MEMS_ALLOWED: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// `mbind(2)` flags
// ---------------------------------------------------------------------------

pub const MPOL_MF_STRICT: u32 = 1 << 0;
pub const MPOL_MF_MOVE: u32 = 1 << 1;
pub const MPOL_MF_MOVE_ALL: u32 = 1 << 2;
pub const MPOL_MF_LAZY: u32 = 1 << 3;
pub const MPOL_MF_INTERNAL: u32 = 1 << 4;

/// All user-visible mbind flags ORed together.
pub const MPOL_MF_VALID: u32 = MPOL_MF_STRICT | MPOL_MF_MOVE | MPOL_MF_MOVE_ALL | MPOL_MF_LAZY;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_MBIND: u32 = 237;
pub const NR_SET_MEMPOLICY: u32 = 238;
pub const NR_GET_MEMPOLICY: u32 = 239;
pub const NR_MIGRATE_PAGES: u32 = 256;
pub const NR_MOVE_PAGES: u32 = 279;
/// Linux 5.17+.
pub const NR_SET_MEMPOLICY_HOME_NODE: u32 = 450;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_modes_dense_0_to_6() {
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
        assert_eq!(MPOL_MAX, MPOL_WEIGHTED_INTERLEAVE);
    }

    #[test]
    fn test_mode_flags_high_bits() {
        // Mode flags live in bits 13..16; they must not collide with the
        // 0..7 policy mode field.
        for f in [
            MPOL_F_STATIC_NODES,
            MPOL_F_RELATIVE_NODES,
            MPOL_F_NUMA_BALANCING,
        ] {
            assert!(f.is_power_of_two());
            assert!(f > MPOL_MAX);
        }
        assert_eq!(
            MPOL_MODE_FLAGS,
            MPOL_F_STATIC_NODES | MPOL_F_RELATIVE_NODES | MPOL_F_NUMA_BALANCING
        );
    }

    #[test]
    fn test_get_mempolicy_flags_dense() {
        let f = [MPOL_F_NODE, MPOL_F_ADDR, MPOL_F_MEMS_ALLOWED];
        for v in f {
            assert!(v.is_power_of_two());
        }
        assert_eq!(MPOL_F_NODE | MPOL_F_ADDR | MPOL_F_MEMS_ALLOWED, 0x7);
    }

    #[test]
    fn test_mbind_flags_dense_and_valid_mask() {
        let f = [
            MPOL_MF_STRICT,
            MPOL_MF_MOVE,
            MPOL_MF_MOVE_ALL,
            MPOL_MF_LAZY,
            MPOL_MF_INTERNAL,
        ];
        for v in f {
            assert!(v.is_power_of_two());
        }
        // Five dense bits 0..4.
        let or = f.iter().fold(0, |a, b| a | b);
        assert_eq!(or, 0x1F);
        // VALID excludes the kernel-private INTERNAL bit.
        assert_eq!(MPOL_MF_VALID & MPOL_MF_INTERNAL, 0);
        assert_eq!(MPOL_MF_VALID | MPOL_MF_INTERNAL, 0x1F);
    }

    #[test]
    fn test_syscall_numbers_monotone() {
        let ns = [
            NR_MBIND,
            NR_SET_MEMPOLICY,
            NR_GET_MEMPOLICY,
            NR_MIGRATE_PAGES,
            NR_MOVE_PAGES,
            NR_SET_MEMPOLICY_HOME_NODE,
        ];
        for w in ns.windows(2) {
            assert!(w[0] < w[1]);
        }
        // Linux 5.17 added set_mempolicy_home_node at 450.
        assert_eq!(NR_SET_MEMPOLICY_HOME_NODE, 450);
    }
}
