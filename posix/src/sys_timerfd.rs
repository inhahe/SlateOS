//! `<sys/timerfd.h>` — timer notification file descriptor.
//!
//! Re-exports from the `epoll` module.

pub use crate::epoll::timerfd_create;
pub use crate::epoll::timerfd_settime;
pub use crate::epoll::timerfd_gettime;
pub use crate::epoll::Itimerspec;
pub use crate::epoll::TFD_CLOEXEC;
pub use crate::epoll::TFD_NONBLOCK;
pub use crate::epoll::TFD_TIMER_ABSTIME;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tfd_flags() {
        assert_ne!(TFD_CLOEXEC, 0);
        assert_ne!(TFD_NONBLOCK, 0);
        assert_eq!(TFD_TIMER_ABSTIME, 1);
    }

    #[test]
    fn test_tfd_flags_distinct() {
        assert_ne!(TFD_CLOEXEC, TFD_NONBLOCK);
        assert_ne!(TFD_CLOEXEC, TFD_TIMER_ABSTIME);
        assert_ne!(TFD_NONBLOCK, TFD_TIMER_ABSTIME);
    }

    #[test]
    fn test_timerfd_create_stub() {
        let fd = timerfd_create(0, 0);
        assert_eq!(fd, -1);
    }

    #[test]
    fn test_itimerspec_size() {
        assert!(core::mem::size_of::<Itimerspec>() > 0);
    }
}
