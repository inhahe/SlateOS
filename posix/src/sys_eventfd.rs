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

    /// `eventfd` rejects `EFD_SEMAPHORE` (not yet implemented).
    #[test]
    fn test_eventfd_semaphore_rejected() {
        crate::errno::set_errno(0);
        assert_eq!(eventfd(0, EFD_SEMAPHORE), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// `eventfd_read` on an invalid fd returns -1 with EFAULT (null buf).
    #[test]
    fn test_eventfd_read_null_returns_efault() {
        crate::errno::set_errno(0);
        assert_eq!(eventfd_read(-1, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// `eventfd_write` with `u64::MAX` is rejected (Linux EINVAL).
    #[test]
    fn test_eventfd_write_max_rejected() {
        crate::errno::set_errno(0);
        assert_eq!(eventfd_write(-1, u64::MAX), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_eventfd_t_size() {
        assert_eq!(core::mem::size_of::<EventfdT>(), 8);
    }
}
