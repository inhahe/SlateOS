//! `<linux/limits.h>` and `<sys/limits.h>` — POSIX numeric limits.
//!
//! These caps appear in `getconf(1)`, `<limits.h>` of every libc, and
//! the SUS/POSIX conformance test suite. They are also the targets
//! every fuzzer mutates input lengths against, so getting the numbers
//! exact matters.

// ---------------------------------------------------------------------------
// Path and filename limits
// ---------------------------------------------------------------------------

/// `NAME_MAX` — longest single path component (no NUL).
pub const NAME_MAX: usize = 255;
/// `PATH_MAX` — longest full path including the trailing NUL.
pub const PATH_MAX: usize = 4096;
/// `PIPE_BUF` — POSIX-atomic write size for pipes.
pub const PIPE_BUF: usize = 4096;
/// `XATTR_NAME_MAX` — extended-attribute name length cap.
pub const XATTR_NAME_MAX: usize = 255;
/// `XATTR_SIZE_MAX` — extended-attribute value size cap.
pub const XATTR_SIZE_MAX: usize = 65_536;
/// `XATTR_LIST_MAX` — total xattr listing size cap.
pub const XATTR_LIST_MAX: usize = 65_536;

// ---------------------------------------------------------------------------
// Process and exec limits
// ---------------------------------------------------------------------------

/// `ARG_MAX` — argv+envp byte cap for `execve(2)`.
pub const ARG_MAX: usize = 131_072;
/// `MAX_ARG_STRINGS` — argv+envp string count cap (binprm limit).
pub const MAX_ARG_STRINGS: u32 = 0x7FFF_FFFF;
/// `NGROUPS_MAX` — supplementary groups per process.
pub const NGROUPS_MAX: usize = 65_536;
/// `CHILD_MAX` — fork()'able children per uid (kernel default).
pub const CHILD_MAX: usize = 4096;
/// `RTSIG_MAX` — number of real-time signals.
pub const RTSIG_MAX: u32 = 32;

// ---------------------------------------------------------------------------
// `mq_open(3)` / `msgget(2)` limits
// ---------------------------------------------------------------------------

/// `MQ_PRIO_MAX` — POSIX message queue priorities.
pub const MQ_PRIO_MAX: u32 = 32_768;
/// `MSGQNUM_MAX` — SysV message queue entry cap.
pub const MSGQNUM_MAX: u32 = 32_768;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_and_name_limits() {
        // 255 bytes is the historical ext{2,3,4} d_name cap.
        assert_eq!(NAME_MAX, 255);
        // 4 KiB is the historical PATH_MAX.
        assert_eq!(PATH_MAX, 4096);
        assert!(NAME_MAX < PATH_MAX);
    }

    #[test]
    fn test_pipe_buf_is_4k() {
        // POSIX requires PIPE_BUF >= 512; Linux uses 4 KiB.
        assert_eq!(PIPE_BUF, 4096);
        assert!(PIPE_BUF >= 512);
    }

    #[test]
    fn test_xattr_limits() {
        // 255 chars (no NUL) for attribute names.
        assert_eq!(XATTR_NAME_MAX, 255);
        // 64 KiB for a value and for the total listing.
        assert_eq!(XATTR_SIZE_MAX, 65_536);
        assert_eq!(XATTR_LIST_MAX, XATTR_SIZE_MAX);
        // Name fits inside a single byte length prefix.
        assert!(XATTR_NAME_MAX <= u8::MAX as usize);
    }

    #[test]
    fn test_exec_limits() {
        // 128 KiB is Linux's ARG_MAX baseline.
        assert_eq!(ARG_MAX, 131_072);
        // MAX_ARG_STRINGS uses the high bit as a sentinel ⇒ 0x7FFFFFFF.
        assert_eq!(MAX_ARG_STRINGS, i32::MAX as u32);
        // Linux NGROUPS_MAX has been 65,536 since 2.6.4.
        assert_eq!(NGROUPS_MAX, 65_536);
        // Real-time signals: SIGRTMIN..SIGRTMAX inclusive = 32.
        assert_eq!(RTSIG_MAX, 32);
    }

    #[test]
    fn test_mqueue_limits() {
        // Both POSIX MQ priorities and SysV msg counts max at 32768.
        assert_eq!(MQ_PRIO_MAX, 32_768);
        assert_eq!(MSGQNUM_MAX, 32_768);
    }
}
