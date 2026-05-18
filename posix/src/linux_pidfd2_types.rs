//! `<linux/pidfd.h>` — Additional pidfd constants.
//!
//! Supplementary pidfd constants covering open flags,
//! signal flags, and getfd options.

// ---------------------------------------------------------------------------
// pidfd_open flags
// ---------------------------------------------------------------------------

/// Non-blocking pidfd.
pub const PIDFD_NONBLOCK: u32 = 0x800;
/// Thread pidfd (not process).
pub const PIDFD_THREAD: u32 = 0x10000000;

// ---------------------------------------------------------------------------
// pidfd_send_signal flags
// ---------------------------------------------------------------------------

/// No flags.
pub const PIDFD_SIGNAL_NONE: u32 = 0;
/// Thread signal.
pub const PIDFD_SIGNAL_THREAD: u32 = 1 << 0;
/// Thread group signal.
pub const PIDFD_SIGNAL_THREAD_GROUP: u32 = 1 << 1;
/// Process group signal.
pub const PIDFD_SIGNAL_PROCESS_GROUP: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// pidfd_getfd flags
// ---------------------------------------------------------------------------

/// No flags for getfd.
pub const PIDFD_GETFD_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// Process file descriptor ioctl
// ---------------------------------------------------------------------------

/// Get pid from pidfd.
pub const PIDFD_GET_PID: u32 = 0x8004_FF00;
/// Get info.
pub const PIDFD_GET_INFO: u32 = 0xC058_FF03;

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

    #[test]
    fn test_open_flags() {
        assert_ne!(PIDFD_NONBLOCK, PIDFD_THREAD);
    }

    #[test]
    fn test_signal_flags_no_overlap() {
        let flags = [
            PIDFD_SIGNAL_THREAD, PIDFD_SIGNAL_THREAD_GROUP,
            PIDFD_SIGNAL_PROCESS_GROUP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_signal_flags_power_of_two() {
        let flags = [
            PIDFD_SIGNAL_THREAD, PIDFD_SIGNAL_THREAD_GROUP,
            PIDFD_SIGNAL_PROCESS_GROUP,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_wait_types_distinct() {
        let types = [P_ALL, P_PID, P_PGID, P_PIDFD];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(PIDFD_GET_PID, PIDFD_GET_INFO);
    }

    #[test]
    fn test_clone_pidfd() {
        assert!(CLONE_PIDFD.is_power_of_two());
    }
}
