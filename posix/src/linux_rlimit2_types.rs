//! `<linux/resource.h>` — Additional resource limit constants.
//!
//! Supplementary rlimit constants covering resource types,
//! priority ranges, and rusage categories.

// ---------------------------------------------------------------------------
// Resource limit types (RLIMIT_*)
// ---------------------------------------------------------------------------

/// CPU time (seconds).
pub const RLIMIT_CPU: u32 = 0;
/// File size (bytes).
pub const RLIMIT_FSIZE: u32 = 1;
/// Data segment size.
pub const RLIMIT_DATA: u32 = 2;
/// Stack size.
pub const RLIMIT_STACK: u32 = 3;
/// Core dump size.
pub const RLIMIT_CORE: u32 = 4;
/// Resident set size.
pub const RLIMIT_RSS: u32 = 5;
/// Number of processes.
pub const RLIMIT_NPROC: u32 = 6;
/// Open files.
pub const RLIMIT_NOFILE: u32 = 7;
/// Locked memory.
pub const RLIMIT_MEMLOCK: u32 = 8;
/// Address space.
pub const RLIMIT_AS: u32 = 9;
/// File locks.
pub const RLIMIT_LOCKS: u32 = 10;
/// Pending signals.
pub const RLIMIT_SIGPENDING: u32 = 11;
/// POSIX message queue bytes.
pub const RLIMIT_MSGQUEUE: u32 = 12;
/// Nice ceiling.
pub const RLIMIT_NICE: u32 = 13;
/// RT priority ceiling.
pub const RLIMIT_RTPRIO: u32 = 14;
/// RT time limit (microseconds).
pub const RLIMIT_RTTIME: u32 = 15;
/// Number of resource limits.
pub const RLIM_NLIMITS: u32 = 16;

// ---------------------------------------------------------------------------
// Resource limit special values
// ---------------------------------------------------------------------------

/// Infinity (no limit).
pub const RLIM_INFINITY: u64 = u64::MAX;
/// Saved max.
pub const RLIM64_INFINITY: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Process priority ranges
// ---------------------------------------------------------------------------

/// Minimum nice value.
pub const PRIO_MIN: i32 = -20;
/// Maximum nice value.
pub const PRIO_MAX: i32 = 20;
/// Priority for process.
pub const PRIO_PROCESS: u32 = 0;
/// Priority for process group.
pub const PRIO_PGRP: u32 = 1;
/// Priority for user.
pub const PRIO_USER: u32 = 2;

// ---------------------------------------------------------------------------
// Rusage categories
// ---------------------------------------------------------------------------

/// Self.
pub const RUSAGE_SELF: i32 = 0;
/// Children.
pub const RUSAGE_CHILDREN: i32 = -1;
/// Both.
pub const RUSAGE_BOTH: i32 = -2;
/// Thread.
pub const RUSAGE_THREAD: i32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rlimit_types_distinct() {
        let types = [
            RLIMIT_CPU, RLIMIT_FSIZE, RLIMIT_DATA, RLIMIT_STACK,
            RLIMIT_CORE, RLIMIT_RSS, RLIMIT_NPROC, RLIMIT_NOFILE,
            RLIMIT_MEMLOCK, RLIMIT_AS, RLIMIT_LOCKS,
            RLIMIT_SIGPENDING, RLIMIT_MSGQUEUE, RLIMIT_NICE,
            RLIMIT_RTPRIO, RLIMIT_RTTIME,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_nlimits() {
        assert_eq!(RLIM_NLIMITS, 16);
        assert_eq!(RLIM_NLIMITS as u32, RLIMIT_RTTIME + 1);
    }

    #[test]
    fn test_infinity() {
        assert_eq!(RLIM_INFINITY, u64::MAX);
        assert_eq!(RLIM64_INFINITY, u64::MAX);
    }

    #[test]
    fn test_prio_range() {
        assert!(PRIO_MIN < 0);
        assert!(PRIO_MAX > 0);
        assert!(PRIO_MIN < PRIO_MAX);
    }

    #[test]
    fn test_prio_who_distinct() {
        let whos = [PRIO_PROCESS, PRIO_PGRP, PRIO_USER];
        for i in 0..whos.len() {
            for j in (i + 1)..whos.len() {
                assert_ne!(whos[i], whos[j]);
            }
        }
    }

    #[test]
    fn test_rusage_distinct() {
        let usages = [RUSAGE_SELF, RUSAGE_CHILDREN, RUSAGE_BOTH, RUSAGE_THREAD];
        for i in 0..usages.len() {
            for j in (i + 1)..usages.len() {
                assert_ne!(usages[i], usages[j]);
            }
        }
    }
}
