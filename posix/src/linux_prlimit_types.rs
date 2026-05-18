//! `<sys/resource.h>` — Resource limit (rlimit) constants.
//!
//! Resource limits control the maximum amount of a particular
//! resource that a process can consume. These are queried and
//! set via `getrlimit()`, `setrlimit()`, and `prlimit64()`.

// ---------------------------------------------------------------------------
// Resource limit identifiers (RLIMIT_*)
// ---------------------------------------------------------------------------

/// Maximum size of the process's virtual memory (bytes).
pub const RLIMIT_AS: u32 = 9;
/// Maximum core dump file size (bytes).
pub const RLIMIT_CORE: u32 = 4;
/// Maximum CPU time (seconds).
pub const RLIMIT_CPU: u32 = 0;
/// Maximum size of the data segment (bytes).
pub const RLIMIT_DATA: u32 = 2;
/// Maximum file size (bytes).
pub const RLIMIT_FSIZE: u32 = 1;
/// Maximum number of file locks.
pub const RLIMIT_LOCKS: u32 = 10;
/// Maximum bytes in POSIX message queues.
pub const RLIMIT_MSGQUEUE: u32 = 12;
/// Maximum nice priority value (ceiling).
pub const RLIMIT_NICE: u32 = 13;
/// Maximum number of open file descriptors.
pub const RLIMIT_NOFILE: u32 = 7;
/// Maximum number of processes (threads).
pub const RLIMIT_NPROC: u32 = 6;
/// Maximum resident set size (bytes).
pub const RLIMIT_RSS: u32 = 5;
/// Maximum real-time priority.
pub const RLIMIT_RTPRIO: u32 = 14;
/// Maximum real-time CPU time without blocking (microseconds).
pub const RLIMIT_RTTIME: u32 = 15;
/// Maximum number of pending signals.
pub const RLIMIT_SIGPENDING: u32 = 11;
/// Maximum stack size (bytes).
pub const RLIMIT_STACK: u32 = 3;
/// Maximum locked memory (bytes, via mlock).
pub const RLIMIT_MEMLOCK: u32 = 8;

// ---------------------------------------------------------------------------
// Special values
// ---------------------------------------------------------------------------

/// Unlimited resource value.
pub const RLIM_INFINITY: u64 = u64::MAX;
/// Number of distinct resource limits.
pub const RLIM_NLIMITS: u32 = 16;

// ---------------------------------------------------------------------------
// prlimit64 flag constants
// ---------------------------------------------------------------------------

/// Read the current limit.
pub const PRLIMIT_GET: u32 = 0;
/// Set a new limit.
pub const PRLIMIT_SET: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rlimits_distinct() {
        let rlimits = [
            RLIMIT_CPU, RLIMIT_FSIZE, RLIMIT_DATA, RLIMIT_STACK,
            RLIMIT_CORE, RLIMIT_RSS, RLIMIT_NPROC, RLIMIT_NOFILE,
            RLIMIT_MEMLOCK, RLIMIT_AS, RLIMIT_LOCKS, RLIMIT_SIGPENDING,
            RLIMIT_MSGQUEUE, RLIMIT_NICE, RLIMIT_RTPRIO, RLIMIT_RTTIME,
        ];
        for i in 0..rlimits.len() {
            for j in (i + 1)..rlimits.len() {
                assert_ne!(rlimits[i], rlimits[j]);
            }
        }
    }

    #[test]
    fn test_rlimit_cpu() {
        assert_eq!(RLIMIT_CPU, 0);
    }

    #[test]
    fn test_rlimit_nofile() {
        assert_eq!(RLIMIT_NOFILE, 7);
    }

    #[test]
    fn test_rlim_nlimits() {
        assert_eq!(RLIM_NLIMITS, 16);
    }

    #[test]
    fn test_rlim_infinity() {
        assert_eq!(RLIM_INFINITY, u64::MAX);
    }

    #[test]
    fn test_prlimit_ops() {
        assert_ne!(PRLIMIT_GET, PRLIMIT_SET);
    }

    #[test]
    fn test_all_rlimits_below_nlimits() {
        let rlimits = [
            RLIMIT_CPU, RLIMIT_FSIZE, RLIMIT_DATA, RLIMIT_STACK,
            RLIMIT_CORE, RLIMIT_RSS, RLIMIT_NPROC, RLIMIT_NOFILE,
            RLIMIT_MEMLOCK, RLIMIT_AS, RLIMIT_LOCKS, RLIMIT_SIGPENDING,
            RLIMIT_MSGQUEUE, RLIMIT_NICE, RLIMIT_RTPRIO, RLIMIT_RTTIME,
        ];
        for r in &rlimits {
            assert!(*r < RLIM_NLIMITS);
        }
    }
}
