//! `<linux/pidfd.h>` — Additional pidfd constants.
//!
//! Supplementary pidfd constants covering open flags, signal flags, the
//! `waitid` id types and `clone3`'s pidfd flag, as Linux defines them
//! (`PIDFD_THREAD` and the `PIDFD_SIGNAL_*` flags are Linux 6.9's).
//!
//! Until 2026-09-26 this module also held `PIDFD_THREAD` as 0x10000000
//! (Linux's is `O_EXCL`), two ioctl numbers Linux does not have
//! (`PIDFD_GET_PID`, and a `PIDFD_GET_INFO` with the wrong number and size)
//! and two names for zero it does not define.  Nothing used them;
//! `process.rs` keeps the `pidfd_open` flags it validates itself.

// ---------------------------------------------------------------------------
// pidfd_open flags
// ---------------------------------------------------------------------------

/// Non-blocking pidfd: `O_NONBLOCK`.
pub const PIDFD_NONBLOCK: u32 = 0x800;
/// A thread's pidfd, not its process's: `O_EXCL` (Linux 6.9).
pub const PIDFD_THREAD: u32 = 0o200;

// ---------------------------------------------------------------------------
// pidfd_send_signal flags (Linux 6.9)
// ---------------------------------------------------------------------------

/// Signal the thread the pidfd names.
pub const PIDFD_SIGNAL_THREAD: u32 = 1 << 0;
/// Signal its thread group.
pub const PIDFD_SIGNAL_THREAD_GROUP: u32 = 1 << 1;
/// Signal its process group.
pub const PIDFD_SIGNAL_PROCESS_GROUP: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// waitid/P_PIDFD
// ---------------------------------------------------------------------------

/// Wait on pidfd.
pub const P_PIDFD: u32 = 3;
/// Wait on PID.
pub const P_PID: u32 = 1;
/// Wait on PGID.
pub const P_PGID: u32 = 2;
/// Wait on any.
pub const P_ALL: u32 = 0;

// ---------------------------------------------------------------------------
// Clone3 pidfd flags
// ---------------------------------------------------------------------------

/// Return pidfd in clone3.
pub const CLONE_PIDFD: u64 = 0x00001000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Linux's values, header for header: `<linux/pidfd.h>` (6.9),
    /// glibc's `idtype_t`, `<linux/sched.h>`.
    #[test]
    fn test_values_are_linuxs() {
        assert_eq!(PIDFD_NONBLOCK, crate::fcntl::O_NONBLOCK as u32);
        assert_eq!(PIDFD_THREAD, crate::fcntl::O_EXCL as u32);
        assert_eq!(
            [
                PIDFD_SIGNAL_THREAD,
                PIDFD_SIGNAL_THREAD_GROUP,
                PIDFD_SIGNAL_PROCESS_GROUP
            ],
            [1, 2, 4]
        );
        assert_eq!([P_ALL, P_PID, P_PGID, P_PIDFD], [0, 1, 2, 3]);
        assert_eq!(CLONE_PIDFD, 0x1000);
    }

    #[test]
    fn test_process_rs_agrees() {
        assert_eq!(PIDFD_THREAD, crate::process::PIDFD_THREAD);
    }
}
