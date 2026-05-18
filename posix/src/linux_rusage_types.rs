//! `<sys/resource.h>` — Resource usage (rusage) constants.
//!
//! `getrusage()` returns resource usage statistics for a process
//! or its children.  These constants define the `who` parameter
//! and the field offsets within `struct rusage`.

// ---------------------------------------------------------------------------
// getrusage() who parameter
// ---------------------------------------------------------------------------

/// Resource usage for the calling process.
pub const RUSAGE_SELF: i32 = 0;
/// Resource usage for all terminated children.
pub const RUSAGE_CHILDREN: i32 = -1;
/// Resource usage for the calling thread (Linux extension).
pub const RUSAGE_THREAD: i32 = 1;

// ---------------------------------------------------------------------------
// struct rusage field offsets (bytes, Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of ru_utime (user time) in struct rusage.
pub const RUSAGE_OFF_UTIME: u32 = 0;
/// Offset of ru_stime (system time) in struct rusage.
pub const RUSAGE_OFF_STIME: u32 = 16;
/// Offset of ru_maxrss (max resident set size) in struct rusage.
pub const RUSAGE_OFF_MAXRSS: u32 = 32;
/// Offset of ru_minflt (minor page faults) in struct rusage.
pub const RUSAGE_OFF_MINFLT: u32 = 56;
/// Offset of ru_majflt (major page faults) in struct rusage.
pub const RUSAGE_OFF_MAJFLT: u32 = 64;
/// Offset of ru_nswap (swaps) in struct rusage.
pub const RUSAGE_OFF_NSWAP: u32 = 72;
/// Offset of ru_inblock (block input operations) in struct rusage.
pub const RUSAGE_OFF_INBLOCK: u32 = 80;
/// Offset of ru_oublock (block output operations) in struct rusage.
pub const RUSAGE_OFF_OUBLOCK: u32 = 88;
/// Offset of ru_msgsnd (IPC messages sent) in struct rusage.
pub const RUSAGE_OFF_MSGSND: u32 = 96;
/// Offset of ru_msgrcv (IPC messages received) in struct rusage.
pub const RUSAGE_OFF_MSGRCV: u32 = 104;
/// Offset of ru_nsignals (signals received) in struct rusage.
pub const RUSAGE_OFF_NSIGNALS: u32 = 112;
/// Offset of ru_nvcsw (voluntary context switches) in struct rusage.
pub const RUSAGE_OFF_NVCSW: u32 = 120;
/// Offset of ru_nivcsw (involuntary context switches) in struct rusage.
pub const RUSAGE_OFF_NIVCSW: u32 = 128;

/// Size of struct rusage on Linux x86_64 (bytes).
pub const RUSAGE_SIZE: u32 = 144;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_who_values_distinct() {
        let whos = [RUSAGE_SELF, RUSAGE_CHILDREN, RUSAGE_THREAD];
        for i in 0..whos.len() {
            for j in (i + 1)..whos.len() {
                assert_ne!(whos[i], whos[j]);
            }
        }
    }

    #[test]
    fn test_self_is_zero() {
        assert_eq!(RUSAGE_SELF, 0);
    }

    #[test]
    fn test_children_is_negative() {
        assert_eq!(RUSAGE_CHILDREN, -1);
    }

    #[test]
    fn test_thread_is_one() {
        assert_eq!(RUSAGE_THREAD, 1);
    }

    #[test]
    fn test_offsets_ascending() {
        let offsets = [
            RUSAGE_OFF_UTIME, RUSAGE_OFF_STIME, RUSAGE_OFF_MAXRSS,
            RUSAGE_OFF_MINFLT, RUSAGE_OFF_MAJFLT, RUSAGE_OFF_NSWAP,
            RUSAGE_OFF_INBLOCK, RUSAGE_OFF_OUBLOCK, RUSAGE_OFF_MSGSND,
            RUSAGE_OFF_MSGRCV, RUSAGE_OFF_NSIGNALS,
            RUSAGE_OFF_NVCSW, RUSAGE_OFF_NIVCSW,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_offsets_within_struct() {
        assert!(RUSAGE_OFF_NIVCSW < RUSAGE_SIZE);
    }

    #[test]
    fn test_struct_size() {
        assert_eq!(RUSAGE_SIZE, 144);
    }

    #[test]
    fn test_utime_at_start() {
        assert_eq!(RUSAGE_OFF_UTIME, 0);
    }
}
