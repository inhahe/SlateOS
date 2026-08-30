//! `<linux/resource.h>` — Resource limit constants.
//!
//! Process resource limits (`rlimit`) control how much of various
//! system resources a process may consume. Limits are set per-process
//! and inherited across fork/exec. Each limit has a soft (enforced)
//! and hard (ceiling) value.
//!
//! This module is a header transcription and nothing more: resource ids and
//! `prlimit64` flag values, which are ABI. It deliberately declares no
//! *default* limit values — what a process starts with is the kernel's policy,
//! and [`crate::resource`] reads it from the kernel rather than from any table
//! in libc.

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
// Default limits: deliberately absent
// ---------------------------------------------------------------------------
//
// This file used to declare `RLIMIT_STACK_DEFAULT`, `RLIMIT_CORE_DEFAULT`,
// `RLIMIT_NOFILE_DEFAULT` (1024) and `RLIMIT_NOFILE_HARD_DEFAULT` (4096).  They
// are gone, and nothing should reintroduce them here.
//
// A *default* is a policy choice about what a fresh process starts with, and
// the kernel is the only thing that gets to make it — `resource.rs` now asks
// the kernel via `SYS_RLIMIT_GET` rather than answering from a table of its
// own.  These four were a second, stale copy of that policy: the 1024/4096
// pair was the kernel's value from before it changed, so libc shipped a
// default that disagreed both with the kernel and with `resource.rs`'s own
// (256, 256).  Nothing outside this file's own unit test ever read them, which
// is why the disagreement went unnoticed for as long as it did.
//
// Everything else this file declares is different in kind: resource *ids* and
// `prlimit64` flag values are ABI, fixed by Linux, not policy we could get
// wrong by falling out of date.  That is why they stay.

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

    // There is no `test_default_limits` any more.  It asserted that
    // `RLIMIT_STACK_DEFAULT == 8 * 1024 * 1024` — restating a literal from
    // twenty lines up, which is duplication, not verification — and that
    // `RLIMIT_NOFILE_DEFAULT < RLIMIT_NOFILE_HARD_DEFAULT`, which held
    // perfectly while both numbers were wrong.  The constants are gone; see
    // the note above where they used to be.

    #[test]
    fn test_prlimit_flags_distinct() {
        assert_ne!(PRLIMIT_GET, PRLIMIT_SET);
    }
}
