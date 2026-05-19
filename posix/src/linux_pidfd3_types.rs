//! `<linux/pidfd.h>` — Additional pidfd constants (part 3).
//!
//! Supplementary pidfd constants covering ioctl commands,
//! pidfd flags, and waitid extensions.

// ---------------------------------------------------------------------------
// pidfd open flags
// ---------------------------------------------------------------------------

/// Non-blocking pidfd.
pub const PIDFD_NONBLOCK: u32 = 0o00004000;
/// Thread pidfd.
pub const PIDFD_THREAD: u32 = 0o00000001;

// ---------------------------------------------------------------------------
// pidfd send signal flags
// ---------------------------------------------------------------------------

/// No flags.
pub const PIDFD_SIGNAL_NONE: u32 = 0;
/// Thread group signal.
pub const PIDFD_SIGNAL_THREAD_GROUP: u32 = 1 << 0;
/// Process group signal.
pub const PIDFD_SIGNAL_PROC_GROUP: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// pidfd getfd flags
// ---------------------------------------------------------------------------

/// No flags for pidfd_getfd.
pub const PIDFD_GETFD_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// P_PIDFD for waitid
// ---------------------------------------------------------------------------

/// P_PIDFD identifier type for waitid.
pub const P_PIDFD: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_flags_distinct() {
        assert_ne!(PIDFD_NONBLOCK, PIDFD_THREAD);
    }

    #[test]
    fn test_signal_flags_no_overlap() {
        assert_eq!(PIDFD_SIGNAL_THREAD_GROUP & PIDFD_SIGNAL_PROC_GROUP, 0);
    }

    #[test]
    fn test_p_pidfd() {
        assert_eq!(P_PIDFD, 3);
    }
}
