//! `<sys/resource.h>` — Resource usage and priority constants.
//!
//! The getrusage/setrlimit interface provides per-process resource
//! accounting and limits. Constants identify which resource is being
//! queried or limited, and which process/group the query targets.

// ---------------------------------------------------------------------------
// Resource usage targets (getrusage who)
// ---------------------------------------------------------------------------

/// Resource usage of calling process.
pub const RUSAGE_SELF: i32 = 0;
/// Resource usage of child processes (waited-for).
pub const RUSAGE_CHILDREN: i32 = -1;
/// Resource usage of calling thread.
pub const RUSAGE_THREAD: i32 = 1;

// ---------------------------------------------------------------------------
// Priority targets (getpriority/setpriority who)
// ---------------------------------------------------------------------------

/// Priority of a process.
pub const PRIO_PROCESS: u32 = 0;
/// Priority of a process group.
pub const PRIO_PGRP: u32 = 1;
/// Priority of a user.
pub const PRIO_USER: u32 = 2;

// ---------------------------------------------------------------------------
// Priority bounds
// ---------------------------------------------------------------------------

/// Minimum nice value (highest priority).
pub const PRIO_MIN: i32 = -20;
/// Maximum nice value (lowest priority).
pub const PRIO_MAX: i32 = 19;

// ---------------------------------------------------------------------------
// rlimit resource types (getrlimit/setrlimit)
// ---------------------------------------------------------------------------

/// CPU time limit (seconds).
pub const RLIMIT_CPU: u32 = 0;
/// Maximum file size (bytes).
pub const RLIMIT_FSIZE: u32 = 1;
/// Data segment size limit (bytes).
pub const RLIMIT_DATA: u32 = 2;
/// Stack size limit (bytes).
pub const RLIMIT_STACK: u32 = 3;
/// Core dump size limit (bytes).
pub const RLIMIT_CORE: u32 = 4;
/// Resident set size limit (bytes, not enforced on Linux).
pub const RLIMIT_RSS: u32 = 5;
/// Number of processes limit (per-user).
pub const RLIMIT_NPROC: u32 = 6;
/// Number of open files limit.
pub const RLIMIT_NOFILE: u32 = 7;
/// Locked memory limit (bytes).
pub const RLIMIT_MEMLOCK: u32 = 8;
/// Address space limit (bytes).
pub const RLIMIT_AS: u32 = 9;
/// File locks limit.
pub const RLIMIT_LOCKS: u32 = 10;
/// Pending signals limit.
pub const RLIMIT_SIGPENDING: u32 = 11;
/// POSIX message queue bytes limit.
pub const RLIMIT_MSGQUEUE: u32 = 12;
/// Nice ceiling (20 - rlim_cur = min nice).
pub const RLIMIT_NICE: u32 = 13;
/// Real-time priority ceiling.
pub const RLIMIT_RTPRIO: u32 = 14;
/// Real-time CPU time limit (microseconds).
pub const RLIMIT_RTTIME: u32 = 15;
/// Number of rlimit types.
pub const RLIMIT_NLIMITS: u32 = 16;

// ---------------------------------------------------------------------------
// Special rlimit value
// ---------------------------------------------------------------------------

/// Infinity (no limit).
pub const RLIM_INFINITY: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rusage_targets_distinct() {
        assert_ne!(RUSAGE_SELF, RUSAGE_CHILDREN);
        assert_ne!(RUSAGE_SELF, RUSAGE_THREAD);
        assert_ne!(RUSAGE_CHILDREN, RUSAGE_THREAD);
    }

    #[test]
    fn test_prio_targets_distinct() {
        assert_ne!(PRIO_PROCESS, PRIO_PGRP);
        assert_ne!(PRIO_PGRP, PRIO_USER);
    }

    #[test]
    fn test_prio_bounds() {
        assert!(PRIO_MIN < 0);
        assert!(PRIO_MAX > 0);
        assert!(PRIO_MIN < PRIO_MAX);
    }

    #[test]
    fn test_rlimit_resources_distinct() {
        let res = [
            RLIMIT_CPU, RLIMIT_FSIZE, RLIMIT_DATA, RLIMIT_STACK,
            RLIMIT_CORE, RLIMIT_RSS, RLIMIT_NPROC, RLIMIT_NOFILE,
            RLIMIT_MEMLOCK, RLIMIT_AS, RLIMIT_LOCKS, RLIMIT_SIGPENDING,
            RLIMIT_MSGQUEUE, RLIMIT_NICE, RLIMIT_RTPRIO, RLIMIT_RTTIME,
        ];
        for i in 0..res.len() {
            for j in (i + 1)..res.len() {
                assert_ne!(res[i], res[j]);
            }
        }
    }

    #[test]
    fn test_rlimit_count() {
        assert_eq!(RLIMIT_NLIMITS, 16);
        assert_eq!(RLIMIT_RTTIME + 1, RLIMIT_NLIMITS);
    }

    #[test]
    fn test_rlim_infinity() {
        assert_eq!(RLIM_INFINITY, u64::MAX);
    }
}
