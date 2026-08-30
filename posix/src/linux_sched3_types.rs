//! `<linux/sched.h>` — Additional scheduler constants (batch 3).
//!
//! Supplementary scheduler constants covering clone3 flags,
//! CPU affinity helpers, and sched_attr fields.

// ---------------------------------------------------------------------------
// clone3 flags (additional)
// ---------------------------------------------------------------------------

/// Create in new cgroup namespace.
pub const CLONE_NEWCGROUP_S3: u64 = 0x02000000;
/// Create in new UTS namespace.
pub const CLONE_NEWUTS_S3: u64 = 0x04000000;
/// Create in new IPC namespace.
pub const CLONE_NEWIPC_S3: u64 = 0x08000000;
/// Create in new user namespace.
pub const CLONE_NEWUSER_S3: u64 = 0x10000000;
/// Create in new PID namespace.
pub const CLONE_NEWPID_S3: u64 = 0x20000000;
/// Create in new network namespace.
pub const CLONE_NEWNET_S3: u64 = 0x40000000;
/// Create in new time namespace.
pub const CLONE_NEWTIME_S3: u64 = 0x00000080;
/// Clear the TID in child memory.
pub const CLONE_CHILD_CLEARTID_S3: u64 = 0x00200000;
/// Set the TID in child memory.
pub const CLONE_CHILD_SETTID_S3: u64 = 0x01000000;
/// Into cgroup.
pub const CLONE_INTO_CGROUP_S3: u64 = 0x200000000;

// ---------------------------------------------------------------------------
// sched_attr size/version
// ---------------------------------------------------------------------------

/// Size of sched_attr v0.
pub const SCHED_ATTR_SIZE_VER0: u32 = 48;
/// Size of sched_attr v1.
pub const SCHED_ATTR_SIZE_VER1: u32 = 56;

// ---------------------------------------------------------------------------
// CPU set operations
// ---------------------------------------------------------------------------

/// Maximum number of CPUs in a cpu_set_t.
pub const CPU_SETSIZE: u32 = 1024;
/// Bits per long (64-bit).
pub const CPU_BITS_PER_LONG: u32 = 64;
/// Number of longs in cpu_set_t.
pub const CPU_SET_LONGS: u32 = CPU_SETSIZE / CPU_BITS_PER_LONG;

// ---------------------------------------------------------------------------
// Scheduling utilities
// ---------------------------------------------------------------------------

/// Minimum nice value (highest priority).
pub const SCHED_NICE_MIN: i32 = -20;
/// Maximum nice value (lowest priority).
pub const SCHED_NICE_MAX: i32 = 19;
/// Default nice value.
pub const SCHED_NICE_DEFAULT: i32 = 0;

/// RT priority range minimum.
pub const SCHED_RT_PRIO_MIN: u32 = 1;
/// RT priority range maximum.
pub const SCHED_RT_PRIO_MAX: u32 = 99;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_ns_flags_distinct() {
        let flags = [
            CLONE_NEWCGROUP_S3,
            CLONE_NEWUTS_S3,
            CLONE_NEWIPC_S3,
            CLONE_NEWUSER_S3,
            CLONE_NEWPID_S3,
            CLONE_NEWNET_S3,
            CLONE_NEWTIME_S3,
            CLONE_CHILD_CLEARTID_S3,
            CLONE_CHILD_SETTID_S3,
            CLONE_INTO_CGROUP_S3,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_sched_attr_sizes() {
        assert!(SCHED_ATTR_SIZE_VER0 < SCHED_ATTR_SIZE_VER1);
        assert_eq!(SCHED_ATTR_SIZE_VER0, 48);
    }

    #[test]
    fn test_cpu_set_arithmetic() {
        assert_eq!(CPU_SET_LONGS, CPU_SETSIZE / CPU_BITS_PER_LONG);
        assert_eq!(CPU_SET_LONGS, 16);
    }

    #[test]
    fn test_nice_range() {
        assert!(SCHED_NICE_MIN < SCHED_NICE_DEFAULT);
        assert!(SCHED_NICE_DEFAULT < SCHED_NICE_MAX);
        assert_eq!(SCHED_NICE_MIN, -20);
        assert_eq!(SCHED_NICE_MAX, 19);
    }

    #[test]
    fn test_rt_prio_range() {
        assert!(SCHED_RT_PRIO_MIN < SCHED_RT_PRIO_MAX);
        assert_eq!(SCHED_RT_PRIO_MIN, 1);
        assert_eq!(SCHED_RT_PRIO_MAX, 99);
    }
}
