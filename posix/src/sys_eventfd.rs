//! `<sys/eventfd.h>` — event notification file descriptor.
//!
//! Re-exports from the `epoll` module.

pub use crate::epoll::eventfd;
pub use crate::epoll::eventfd_read;
pub use crate::epoll::eventfd_write;
pub use crate::epoll::EFD_CLOEXEC;
pub use crate::epoll::EFD_NONBLOCK;
pub use crate::epoll::EFD_SEMAPHORE;

/// Type alias for the eventfd counter value.
pub type EventfdT = u64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_efd_flags() {
        assert_ne!(EFD_CLOEXEC, 0);
        assert_ne!(EFD_NONBLOCK, 0);
        assert_eq!(EFD_SEMAPHORE, 1);
    }

    #[test]
    fn test_efd_flags_distinct() {
        assert_ne!(EFD_CLOEXEC, EFD_NONBLOCK);
        assert_ne!(EFD_CLOEXEC, EFD_SEMAPHORE);
        assert_ne!(EFD_NONBLOCK, EFD_SEMAPHORE);
    }

    #[test]
    fn test_eventfd_stub() {
        let fd = eventfd(0, 0);
        assert_eq!(fd, -1);
    }

    #[test]
    fn test_eventfd_read_stub() {
        let ret = eventfd_read(-1, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_eventfd_write_stub() {
        let ret = eventfd_write(-1, 1);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_eventfd_t_size() {
        assert_eq!(core::mem::size_of::<EventfdT>(), 8);
    }
}
