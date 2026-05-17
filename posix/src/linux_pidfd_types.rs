//! `<linux/pidfd.h>` — pidfd (process file descriptor) constants.
//!
//! pidfds provide a race-free handle to a process, replacing PID-based
//! APIs that are vulnerable to PID recycling. Created via pidfd_open()
//! or clone3(CLONE_PIDFD). Used for signaling (pidfd_send_signal),
//! waiting (poll/epoll), and obtaining process information without
//! TOCTOU races.

// ---------------------------------------------------------------------------
// pidfd_open flags
// ---------------------------------------------------------------------------

/// Open a pidfd in non-blocking mode.
pub const PIDFD_NONBLOCK: u32 = 0x0000_0800;
/// Open a pidfd referencing the thread, not the thread group leader.
pub const PIDFD_THREAD: u32 = 0x1000_0000;

// ---------------------------------------------------------------------------
// pidfd_send_signal flags
// ---------------------------------------------------------------------------

/// Send signal to entire thread group (default behavior).
pub const PIDFD_SIGNAL_THREAD_GROUP: u32 = 0;
/// Send signal to specific thread only.
pub const PIDFD_SIGNAL_THREAD: u32 = 1;

// ---------------------------------------------------------------------------
// pidfd poll events
// ---------------------------------------------------------------------------

/// pidfd is readable (process has exited).
pub const PIDFD_POLLIN: u32 = 0x0001;

// ---------------------------------------------------------------------------
// pidfd getfd flags (pidfd_getfd)
// ---------------------------------------------------------------------------

/// No special flags for pidfd_getfd.
pub const PIDFD_GETFD_NO_FLAGS: u32 = 0;

// ---------------------------------------------------------------------------
// clone3 related pidfd flags
// ---------------------------------------------------------------------------

/// Request a pidfd from clone3().
pub const CLONE_PIDFD: u64 = 0x0000_1000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pidfd_open_flags_distinct() {
        assert_ne!(PIDFD_NONBLOCK, PIDFD_THREAD);
        assert_eq!(PIDFD_NONBLOCK & PIDFD_THREAD, 0);
    }

    #[test]
    fn test_signal_flags_distinct() {
        assert_ne!(PIDFD_SIGNAL_THREAD_GROUP, PIDFD_SIGNAL_THREAD);
    }

    #[test]
    fn test_pidfd_nonblock_value() {
        // O_NONBLOCK is traditionally 0x800 on Linux.
        assert_eq!(PIDFD_NONBLOCK, 0x800);
    }

    #[test]
    fn test_clone_pidfd_value() {
        assert_eq!(CLONE_PIDFD, 0x1000);
        assert!(CLONE_PIDFD.is_power_of_two());
    }

    #[test]
    fn test_getfd_no_flags() {
        assert_eq!(PIDFD_GETFD_NO_FLAGS, 0);
    }
}
