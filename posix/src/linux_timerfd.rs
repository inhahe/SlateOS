//! `<linux/timerfd.h>` — timer file descriptor (kernel view).
//!
//! Re-exports from `sys_timerfd` (the POSIX-facing header).

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use crate::epoll::TFD_CLOEXEC;
pub use crate::epoll::TFD_NONBLOCK;
pub use crate::epoll::TFD_TIMER_ABSTIME;
pub use crate::epoll::timerfd_create;
pub use crate::epoll::timerfd_settime;
pub use crate::epoll::timerfd_gettime;

// ---------------------------------------------------------------------------
// Additional TFD flags (Linux 4.x+)
// ---------------------------------------------------------------------------

/// Cancel-on-set: timer is cancelled when CLOCK_REALTIME changes.
pub const TFD_TIMER_CANCEL_ON_SET: i32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tfd_flags() {
        assert_ne!(TFD_CLOEXEC, TFD_NONBLOCK);
        assert_ne!(TFD_CLOEXEC, 0);
        assert_ne!(TFD_NONBLOCK, 0);
    }

    #[test]
    fn test_tfd_timer_flags() {
        assert_ne!(TFD_TIMER_ABSTIME, TFD_TIMER_CANCEL_ON_SET);
    }

    #[test]
    fn test_timerfd_create_stub() {
        let fd = timerfd_create(0, 0);
        assert_eq!(fd, -1);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(TFD_CLOEXEC, crate::epoll::TFD_CLOEXEC);
        assert_eq!(TFD_NONBLOCK, crate::epoll::TFD_NONBLOCK);
    }
}
