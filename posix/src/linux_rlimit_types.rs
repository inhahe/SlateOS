//! `<linux/resource.h>` — Resource limit (rlimit) constants.
//!
//! Resource limits (rlimits) control the maximum amount of various
//! resources a process can consume. Each resource has a soft limit
//! (current enforcement) and hard limit (ceiling the soft limit can
//! be raised to). getrlimit/setrlimit/prlimit64 read and modify these.
//! Limits prevent runaway processes from consuming all system resources.

// ---------------------------------------------------------------------------
// Resource limit identifiers (RLIMIT_*)
// ---------------------------------------------------------------------------

/// Maximum CPU time in seconds.
pub const RLIMIT_CPU: u32 = 0;
/// Maximum file size (bytes).
pub const RLIMIT_FSIZE: u32 = 1;
/// Maximum data segment size (bytes).
pub const RLIMIT_DATA: u32 = 2;
/// Maximum stack size (bytes).
pub const RLIMIT_STACK: u32 = 3;
/// Maximum core file size (bytes).
pub const RLIMIT_CORE: u32 = 4;
/// Maximum resident set size (bytes, advisory).
pub const RLIMIT_RSS: u32 = 5;
/// Maximum number of processes (threads).
pub const RLIMIT_NPROC: u32 = 6;
/// Maximum number of open file descriptors.
pub const RLIMIT_NOFILE: u32 = 7;
/// Maximum locked memory (bytes, mlock).
pub const RLIMIT_MEMLOCK: u32 = 8;
/// Maximum address space size (bytes).
pub const RLIMIT_AS: u32 = 9;
/// Maximum file locks held.
pub const RLIMIT_LOCKS: u32 = 10;
/// Maximum pending signals.
pub const RLIMIT_SIGPENDING: u32 = 11;
/// Maximum bytes in POSIX message queues.
pub const RLIMIT_MSGQUEUE: u32 = 12;
/// Maximum nice priority (ceiling).
pub const RLIMIT_NICE: u32 = 13;
/// Maximum real-time priority.
pub const RLIMIT_RTPRIO: u32 = 14;
/// Maximum real-time CPU time (microseconds) without blocking.
pub const RLIMIT_RTTIME: u32 = 15;

/// Number of resource limit types.
pub const RLIM_NLIMITS: u32 = 16;

// ---------------------------------------------------------------------------
// Special limit values
// ---------------------------------------------------------------------------

/// Infinity (no limit).
pub const RLIM_INFINITY: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// prlimit64 / getrusage who values
// ---------------------------------------------------------------------------

/// Resource usage of calling process.
pub const RUSAGE_SELF: i32 = 0;
/// Resource usage of children.
pub const RUSAGE_CHILDREN: i32 = -1;
/// Resource usage of calling thread.
pub const RUSAGE_THREAD: i32 = 1;

// ---------------------------------------------------------------------------
// Priority (nice) constants
// ---------------------------------------------------------------------------

/// Minimum priority (most favorable scheduling).
pub const PRIO_MIN: i32 = -20;
/// Maximum priority (least favorable scheduling).
pub const PRIO_MAX: i32 = 20;
/// Get/set priority for process.
pub const PRIO_PROCESS: u32 = 0;
/// Get/set priority for process group.
pub const PRIO_PGRP: u32 = 1;
/// Get/set priority for user.
pub const PRIO_USER: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rlimit_ids_distinct() {
        let ids = [
            RLIMIT_CPU,
            RLIMIT_FSIZE,
            RLIMIT_DATA,
            RLIMIT_STACK,
            RLIMIT_CORE,
            RLIMIT_RSS,
            RLIMIT_NPROC,
            RLIMIT_NOFILE,
            RLIMIT_MEMLOCK,
            RLIMIT_AS,
            RLIMIT_LOCKS,
            RLIMIT_SIGPENDING,
            RLIMIT_MSGQUEUE,
            RLIMIT_NICE,
            RLIMIT_RTPRIO,
            RLIMIT_RTTIME,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_rlimit_count() {
        assert_eq!(RLIM_NLIMITS, 16);
    }

    #[test]
    fn test_infinity() {
        assert_eq!(RLIM_INFINITY, u64::MAX);
    }

    #[test]
    fn test_rusage_who_distinct() {
        let who = [RUSAGE_SELF, RUSAGE_CHILDREN, RUSAGE_THREAD];
        for i in 0..who.len() {
            for j in (i + 1)..who.len() {
                assert_ne!(who[i], who[j]);
            }
        }
    }

    #[test]
    fn test_priority_range() {
        assert!(PRIO_MIN < 0);
        assert!(PRIO_MAX > 0);
        assert!(PRIO_MIN < PRIO_MAX);
    }

    #[test]
    fn test_prio_who_distinct() {
        let who = [PRIO_PROCESS, PRIO_PGRP, PRIO_USER];
        for i in 0..who.len() {
            for j in (i + 1)..who.len() {
                assert_ne!(who[i], who[j]);
            }
        }
    }
}
