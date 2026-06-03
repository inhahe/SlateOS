//! `<linux/sched.h>` (clone3 portion) — `clone3()` struct flags.
//!
//! `clone3()` (Linux 5.3+) extends `clone()` with a typed argument
//! struct so future flags can grow without changing the syscall ABI.
//! These constants name the flag bits that may appear in
//! `clone_args.flags`, the per-pidfd output settings, and the various
//! size variants of the args struct itself.

// ---------------------------------------------------------------------------
// Sizes of struct clone_args
// ---------------------------------------------------------------------------

/// Size of the original `struct clone_args` (v0, kernel 5.3).
pub const CLONE_ARGS_SIZE_VER0: u32 = 64;
/// Size after adding `set_tid`/`set_tid_size` (v1, 5.5).
pub const CLONE_ARGS_SIZE_VER1: u32 = 80;
/// Size after adding `cgroup` (v2, 5.7).
pub const CLONE_ARGS_SIZE_VER2: u32 = 88;

// ---------------------------------------------------------------------------
// Clone flag bits shared with the legacy `clone(2)` syscall
// ---------------------------------------------------------------------------

/// Share virtual memory between parent and child.
pub const CLONE_VM: u64 = 0x0000_0100;
/// Share filesystem info (cwd, umask).
pub const CLONE_FS: u64 = 0x0000_0200;
/// Share open files.
pub const CLONE_FILES: u64 = 0x0000_0400;
/// Share signal handlers.
pub const CLONE_SIGHAND: u64 = 0x0000_0800;
/// Return a pidfd in `pidfd` out param.
pub const CLONE_PIDFD: u64 = 0x0000_1000;
/// Allow ptrace to trace child.
pub const CLONE_PTRACE: u64 = 0x0000_2000;
/// Stop until child execs or exits.
pub const CLONE_VFORK: u64 = 0x0000_4000;
/// Parent of new child is parent of caller.
pub const CLONE_PARENT: u64 = 0x0000_8000;
/// Same thread group as caller.
pub const CLONE_THREAD: u64 = 0x0001_0000;
/// New mount namespace.
pub const CLONE_NEWNS: u64 = 0x0002_0000;
/// Share SysV semaphore undo state.
pub const CLONE_SYSVSEM: u64 = 0x0004_0000;
/// Establish new TLS.
pub const CLONE_SETTLS: u64 = 0x0008_0000;
/// Write child TID at `parent_tid`.
pub const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
/// Clear child TID at `child_tid` (futex wake).
pub const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;
/// Detached child (no SIGCHLD).
pub const CLONE_DETACHED: u64 = 0x0040_0000;
/// Set untraced bit.
pub const CLONE_UNTRACED: u64 = 0x0080_0000;
/// Write child TID at `child_tid`.
pub const CLONE_CHILD_SETTID: u64 = 0x0100_0000;
/// New cgroup namespace.
pub const CLONE_NEWCGROUP: u64 = 0x0200_0000;
/// New UTS namespace.
pub const CLONE_NEWUTS: u64 = 0x0400_0000;
/// New IPC namespace.
pub const CLONE_NEWIPC: u64 = 0x0800_0000;
/// New user namespace.
pub const CLONE_NEWUSER: u64 = 0x1000_0000;
/// New PID namespace.
pub const CLONE_NEWPID: u64 = 0x2000_0000;
/// New network namespace.
pub const CLONE_NEWNET: u64 = 0x4000_0000;
/// Share I/O context.
pub const CLONE_IO: u64 = 0x8000_0000;

// ---------------------------------------------------------------------------
// clone3-only flags (upper 32 bits)
// ---------------------------------------------------------------------------

/// Clear the SIGHAND signal mask for the child.
pub const CLONE_CLEAR_SIGHAND: u64 = 1 << 32;
/// Place child in target cgroup `cgroup` rather than inheriting.
pub const CLONE_INTO_CGROUP: u64 = 1 << 33;
/// New time namespace.
pub const CLONE_NEWTIME: u64 = 1 << 7;

// ---------------------------------------------------------------------------
// set_tid array limit
// ---------------------------------------------------------------------------

/// Maximum number of nested PID namespaces, hence maximum entries in
/// `clone_args.set_tid` (`MAX_PID_NS_LEVEL`).
pub const CLONE_SET_TID_MAX: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_struct_sizes_monotonic_and_8_aligned() {
        assert!(CLONE_ARGS_SIZE_VER0 < CLONE_ARGS_SIZE_VER1);
        assert!(CLONE_ARGS_SIZE_VER1 < CLONE_ARGS_SIZE_VER2);
        // All clone3 args sizes are multiples of 8 (the struct must
        // be 8-byte aligned to satisfy u64 fields).
        for s in [
            CLONE_ARGS_SIZE_VER0,
            CLONE_ARGS_SIZE_VER1,
            CLONE_ARGS_SIZE_VER2,
        ] {
            assert_eq!(s % 8, 0);
        }
    }

    #[test]
    fn test_legacy_flags_are_single_distinct_bits() {
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
        for &b in &f {
            assert!(b.is_power_of_two());
            // All legacy flags fit in the low 32 bits.
            assert!(b <= u32::MAX as u64);
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_clone3_only_flags_distinct_pow2() {
        let f = [CLONE_CLEAR_SIGHAND, CLONE_INTO_CGROUP, CLONE_NEWTIME];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
        // CLEAR_SIGHAND and INTO_CGROUP live above bit 31 (clone3-only).
        assert!(CLONE_CLEAR_SIGHAND > u32::MAX as u64);
        assert!(CLONE_INTO_CGROUP > u32::MAX as u64);
        // NEWTIME shares the legacy 32-bit space at bit 7.
        assert_eq!(CLONE_NEWTIME, 1 << 7);
    }

    #[test]
    fn test_set_tid_max_matches_pidns_level() {
        // 32 levels of PID-namespace nesting → 32 set_tid entries max.
        assert_eq!(CLONE_SET_TID_MAX, 32);
    }
}
