//! `<sys/resource.h>` — `getrlimit(2)` / `setrlimit(2)` / `prlimit64(2)`.
//!
//! Resource limits cap per-process CPU time, file size, address space,
//! open file descriptors, etc. `RLIMIT_*` numbers are also used by
//! `ulimit`, `pam_limits`, and systemd's `LimitNOFILE=` etc.

// ---------------------------------------------------------------------------
// Resource ids (`RLIMIT_*`)
// ---------------------------------------------------------------------------

pub const RLIMIT_CPU: u32 = 0;
pub const RLIMIT_FSIZE: u32 = 1;
pub const RLIMIT_DATA: u32 = 2;
pub const RLIMIT_STACK: u32 = 3;
pub const RLIMIT_CORE: u32 = 4;
pub const RLIMIT_RSS: u32 = 5;
pub const RLIMIT_NPROC: u32 = 6;
pub const RLIMIT_NOFILE: u32 = 7;
pub const RLIMIT_MEMLOCK: u32 = 8;
pub const RLIMIT_AS: u32 = 9;
pub const RLIMIT_LOCKS: u32 = 10;
pub const RLIMIT_SIGPENDING: u32 = 11;
pub const RLIMIT_MSGQUEUE: u32 = 12;
pub const RLIMIT_NICE: u32 = 13;
pub const RLIMIT_RTPRIO: u32 = 14;
pub const RLIMIT_RTTIME: u32 = 15;
pub const RLIM_NLIMITS: u32 = 16;

// ---------------------------------------------------------------------------
// Sentinels
// ---------------------------------------------------------------------------

/// "No limit" — passed as `rlim_cur` or `rlim_max` to disable a cap.
pub const RLIM_INFINITY: u64 = u64::MAX;
/// Sentinel meaning "leave this side of the pair unchanged".
pub const RLIM_SAVED_CUR: u64 = u64::MAX;
pub const RLIM_SAVED_MAX: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// `getrusage(2)` "who" argument
// ---------------------------------------------------------------------------

pub const RUSAGE_SELF: i32 = 0;
pub const RUSAGE_CHILDREN: i32 = -1;
pub const RUSAGE_THREAD: i32 = 1;
pub const RUSAGE_BOTH: i32 = -2;

// ---------------------------------------------------------------------------
// `getpriority(2)` "which" argument
// ---------------------------------------------------------------------------

pub const PRIO_PROCESS: u32 = 0;
pub const PRIO_PGRP: u32 = 1;
pub const PRIO_USER: u32 = 2;

pub const PRIO_MIN: i32 = -20;
pub const PRIO_MAX: i32 = 19;

// ---------------------------------------------------------------------------
// Syscall numbers
// ---------------------------------------------------------------------------

pub const NR_GETRLIMIT: u32 = 97;
pub const NR_SETRLIMIT: u32 = 160;
pub const NR_PRLIMIT64: u32 = 302;
pub const NR_GETRUSAGE: u32 = 98;
pub const NR_GETPRIORITY: u32 = 140;
pub const NR_SETPRIORITY: u32 = 141;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rlimit_ids_dense_0_to_15() {
        let r = [
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
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v, i as u32);
        }
        assert_eq!(RLIM_NLIMITS, r.len() as u32);
    }

    #[test]
    fn test_infinity_is_all_ones() {
        // "No limit" must be the maximum representable rlim_t.
        assert_eq!(RLIM_INFINITY, u64::MAX);
        assert_eq!(RLIM_SAVED_CUR, u64::MAX);
        assert_eq!(RLIM_SAVED_MAX, u64::MAX);
    }

    #[test]
    fn test_rusage_who_distinct() {
        let w = [RUSAGE_SELF, RUSAGE_CHILDREN, RUSAGE_THREAD, RUSAGE_BOTH];
        for a in 0..w.len() {
            for b in (a + 1)..w.len() {
                assert_ne!(w[a], w[b]);
            }
        }
        // CHILDREN is the famous -1.
        assert_eq!(RUSAGE_CHILDREN, -1);
    }

    #[test]
    fn test_prio_which_dense_0_to_2() {
        assert_eq!(PRIO_PROCESS, 0);
        assert_eq!(PRIO_PGRP, 1);
        assert_eq!(PRIO_USER, 2);
    }

    #[test]
    fn test_nice_range_is_minus20_to_19() {
        // The historical UNIX nice range: -20..=19 (40 values).
        assert_eq!(PRIO_MIN, -20);
        assert_eq!(PRIO_MAX, 19);
        assert_eq!((PRIO_MAX - PRIO_MIN + 1) as u32, 40);
    }

    #[test]
    fn test_syscall_numbers_distinct() {
        let n = [
            NR_GETRLIMIT,
            NR_SETRLIMIT,
            NR_PRLIMIT64,
            NR_GETRUSAGE,
            NR_GETPRIORITY,
            NR_SETPRIORITY,
        ];
        for a in 0..n.len() {
            for b in (a + 1)..n.len() {
                assert_ne!(n[a], n[b]);
            }
        }
        // prlimit64 is the 64-bit-clean modern replacement.
        assert_eq!(NR_PRLIMIT64, 302);
    }
}
