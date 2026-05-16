//! `<linux/pidfd.h>` — PID file descriptor interface.
//!
//! Re-exports `pidfd_open()`, `pidfd_send_signal()`, and
//! `pidfd_getfd()` from the `process` module.

pub use crate::process::pidfd_open;
pub use crate::process::pidfd_send_signal;
pub use crate::process::pidfd_getfd;
pub use crate::process::PIDFD_NONBLOCK;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pidfd_nonblock() {
        assert_ne!(PIDFD_NONBLOCK, 0);
    }

    #[test]
    fn test_pidfd_open_stub() {
        assert_eq!(pidfd_open(1, 0), -1);
    }

    #[test]
    fn test_pidfd_send_signal_stub() {
        assert_eq!(pidfd_send_signal(3, 9, core::ptr::null(), 0), -1);
    }

    #[test]
    fn test_pidfd_getfd_stub() {
        assert_eq!(pidfd_getfd(3, 0, 0), -1);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(PIDFD_NONBLOCK, crate::process::PIDFD_NONBLOCK);
    }
}
