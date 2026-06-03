//! `<linux/resource.h>` — Resource limit constants.
//!
//! Process resource limits (`rlimit`) control how much of various
//! system resources a process may consume. Limits are set per-process
//! and inherited across fork/exec. Each limit has a soft (enforced)
//! and hard (ceiling) value.

// ---------------------------------------------------------------------------
// Resource limit IDs (RLIMIT_*)
// ---------------------------------------------------------------------------

/// Maximum CPU time (seconds).
pub const RLIMIT_CPU: u32 = 0;
/// Maximum file size (bytes).
pub const RLIMIT_FSIZE: u32 = 1;
/// Maximum data segment size (bytes).
pub const RLIMIT_DATA: u32 = 2;
/// Maximum stack size (bytes).
pub const RLIMIT_STACK: u32 = 3;
/// Maximum core dump size (bytes).
pub const RLIMIT_CORE: u32 = 4;
/// Maximum resident set size (bytes).
pub const RLIMIT_RSS: u32 = 5;
/// Maximum number of processes (per real UID).
pub const RLIMIT_NPROC: u32 = 6;
/// Maximum number of open file descriptors.
pub const RLIMIT_NOFILE: u32 = 7;
/// Maximum locked memory (bytes).
pub const RLIMIT_MEMLOCK: u32 = 8;
/// Maximum address space size (bytes).
pub const RLIMIT_AS: u32 = 9;
/// Maximum file locks.
pub const RLIMIT_LOCKS: u32 = 10;
/// Maximum pending signals.
pub const RLIMIT_SIGPENDING: u32 = 11;
/// Maximum POSIX message queue bytes.
pub const RLIMIT_MSGQUEUE: u32 = 12;
/// Maximum nice priority (inverted: 20 - nice).
pub const RLIMIT_NICE: u32 = 13;
/// Maximum real-time priority.
pub const RLIMIT_RTPRIO: u32 = 14;
/// Maximum real-time timeout (microseconds).
pub const RLIMIT_RTTIME: u32 = 15;

/// Total number of resource limit types.
pub const RLIM_NLIMITS: u32 = 16;

// ---------------------------------------------------------------------------
// Special values
// ---------------------------------------------------------------------------

/// Unlimited resource (both soft and hard).
pub const RLIM_INFINITY: u64 = u64::MAX;

/// Old-style unlimited (32-bit).
pub const RLIM_INFINITY_32: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// prlimit64 flags
// ---------------------------------------------------------------------------

/// Get old limit.
pub const PRLIMIT_GET: u32 = 0;
/// Set new limit.
pub const PRLIMIT_SET: u32 = 1;

// ---------------------------------------------------------------------------
// Default limits
// ---------------------------------------------------------------------------

/// Default RLIMIT_STACK (8 MiB).
pub const RLIMIT_STACK_DEFAULT: u64 = 8 * 1024 * 1024;
/// Default RLIMIT_CORE (0 = no core dumps).
pub const RLIMIT_CORE_DEFAULT: u64 = 0;
/// Default RLIMIT_NOFILE (1024).
pub const RLIMIT_NOFILE_DEFAULT: u64 = 1024;
/// Hard limit for RLIMIT_NOFILE (4096).
pub const RLIMIT_NOFILE_HARD_DEFAULT: u64 = 4096;

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
        // All IDs should be < RLIM_NLIMITS
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
        for id in &ids {
            assert!(*id < RLIM_NLIMITS, "RLIMIT {} >= RLIM_NLIMITS", id);
        }
    }

    #[test]
    fn test_rlim_infinity() {
        assert_eq!(RLIM_INFINITY, u64::MAX);
        assert_eq!(RLIM_INFINITY_32, u32::MAX);
    }

    #[test]
    fn test_default_limits() {
        assert_eq!(RLIMIT_STACK_DEFAULT, 8 * 1024 * 1024);
        assert_eq!(RLIMIT_CORE_DEFAULT, 0);
        assert!(RLIMIT_NOFILE_DEFAULT < RLIMIT_NOFILE_HARD_DEFAULT);
    }

    #[test]
    fn test_prlimit_flags_distinct() {
        assert_ne!(PRLIMIT_GET, PRLIMIT_SET);
    }
}
