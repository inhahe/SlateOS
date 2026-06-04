//! `<linux/sched.h>` — CLONE_* flags (extended set for clone3).
//!
//! clone(2)/clone3(2) accept a bitmask of CLONE_* flags that control
//! which resources are shared between the calling thread and the new
//! thread/process. This module enumerates the flag bits, including
//! newer additions only available via clone3.

// ---------------------------------------------------------------------------
// Classic CLONE flags (clone(2))
// ---------------------------------------------------------------------------

pub const CLONE_VM: u64 = 0x0000_0100;
pub const CLONE_FS: u64 = 0x0000_0200;
pub const CLONE_FILES: u64 = 0x0000_0400;
pub const CLONE_SIGHAND: u64 = 0x0000_0800;
pub const CLONE_PIDFD: u64 = 0x0000_1000;
pub const CLONE_PTRACE: u64 = 0x0000_2000;
pub const CLONE_VFORK: u64 = 0x0000_4000;
pub const CLONE_PARENT: u64 = 0x0000_8000;
pub const CLONE_THREAD: u64 = 0x0001_0000;
pub const CLONE_NEWNS: u64 = 0x0002_0000;
pub const CLONE_SYSVSEM: u64 = 0x0004_0000;
pub const CLONE_SETTLS: u64 = 0x0008_0000;
pub const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
pub const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;
pub const CLONE_DETACHED: u64 = 0x0040_0000;
pub const CLONE_UNTRACED: u64 = 0x0080_0000;
pub const CLONE_CHILD_SETTID: u64 = 0x0100_0000;
pub const CLONE_NEWCGROUP: u64 = 0x0200_0000;
pub const CLONE_NEWUTS: u64 = 0x0400_0000;
pub const CLONE_NEWIPC: u64 = 0x0800_0000;
pub const CLONE_NEWUSER: u64 = 0x1000_0000;
pub const CLONE_NEWPID: u64 = 0x2000_0000;
pub const CLONE_NEWNET: u64 = 0x4000_0000;
pub const CLONE_IO: u64 = 0x8000_0000;

// ---------------------------------------------------------------------------
// clone3-only flags (require __u64 flags)
// ---------------------------------------------------------------------------

/// Clear the child's signal handler set (clone3 only).
pub const CLONE_CLEAR_SIGHAND: u64 = 0x1_0000_0000;
/// Place the child directly into the cgroup pointed to by `cgroup`.
pub const CLONE_INTO_CGROUP: u64 = 0x2_0000_0000;
/// Allocate a new time namespace.
pub const CLONE_NEWTIME: u64 = 0x80;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classic_flags_distinct_single_bit() {
        let f = [
            CLONE_VM,
            CLONE_FS,
            CLONE_FILES,
            CLONE_SIGHAND,
            CLONE_PIDFD,
            CLONE_PTRACE,
            CLONE_VFORK,
            CLONE_PARENT,
            CLONE_THREAD,
            CLONE_NEWNS,
            CLONE_SYSVSEM,
            CLONE_SETTLS,
            CLONE_PARENT_SETTID,
            CLONE_CHILD_CLEARTID,
            CLONE_DETACHED,
            CLONE_UNTRACED,
            CLONE_CHILD_SETTID,
            CLONE_NEWCGROUP,
            CLONE_NEWUTS,
            CLONE_NEWIPC,
            CLONE_NEWUSER,
            CLONE_NEWPID,
            CLONE_NEWNET,
            CLONE_IO,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
    }

    #[test]
    fn test_classic_flags_in_low_32_bits() {
        // Every classic flag fits in 32 bits.
        for f in [
            CLONE_VM,
            CLONE_FS,
            CLONE_FILES,
            CLONE_SIGHAND,
            CLONE_THREAD,
            CLONE_IO,
        ] {
            assert!(f <= 0xFFFF_FFFF);
        }
    }

    #[test]
    fn test_clone3_flags_above_32_bits() {
        // CLONE_CLEAR_SIGHAND and CLONE_INTO_CGROUP require u64 flags.
        assert!(CLONE_CLEAR_SIGHAND > 0xFFFF_FFFF);
        assert!(CLONE_INTO_CGROUP > 0xFFFF_FFFF);
        assert_eq!(CLONE_CLEAR_SIGHAND, 1 << 32);
        assert_eq!(CLONE_INTO_CGROUP, 1 << 33);
    }

    #[test]
    fn test_newtime_in_low_byte() {
        // CLONE_NEWTIME (0x80) is the only flag in the low byte —
        // bits 0-7 are otherwise the exit signal.
        assert_eq!(CLONE_NEWTIME, 0x80);
        assert!(CLONE_NEWTIME.is_power_of_two());
    }

    #[test]
    fn test_namespace_flags_distinct() {
        let ns = [
            CLONE_NEWNS,
            CLONE_NEWCGROUP,
            CLONE_NEWUTS,
            CLONE_NEWIPC,
            CLONE_NEWUSER,
            CLONE_NEWPID,
            CLONE_NEWNET,
            CLONE_NEWTIME,
        ];
        for (i, &x) in ns.iter().enumerate() {
            for &y in &ns[i + 1..] {
                assert_ne!(x, y);
                assert_eq!(x & y, 0);
            }
        }
    }

    #[test]
    fn test_thread_implies_vm_and_sighand_typically_set_together() {
        // CLONE_THREAD must be set with CLONE_SIGHAND and CLONE_VM
        // (kernel enforces this). Verify they are distinct bits so the
        // OR'd combination is well-formed.
        assert_eq!(CLONE_THREAD & CLONE_SIGHAND, 0);
        assert_eq!(CLONE_THREAD & CLONE_VM, 0);
        let combo = CLONE_THREAD | CLONE_SIGHAND | CLONE_VM;
        assert_eq!(combo.count_ones(), 3);
    }
}
