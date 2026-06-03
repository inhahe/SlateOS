//! `<sys/eventfd.h>` — event notification file descriptor.
//!
//! Re-exports from the `epoll` module.

pub use crate::epoll::EFD_CLOEXEC;
pub use crate::epoll::EFD_NONBLOCK;
pub use crate::epoll::EFD_SEMAPHORE;
pub use crate::epoll::eventfd;
pub use crate::epoll::eventfd_read;
pub use crate::epoll::eventfd_write;

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

    /// `EFD_SEMAPHORE` is in the allowed-flag set (kernel now
    /// implements semaphore-mode reads).  Functional success requires
    /// a live kernel — exercised by the integration tests.
    #[test]
    fn test_eventfd_semaphore_flag_accepted() {
        // Just check the bit definition matches Linux.
        assert_eq!(EFD_SEMAPHORE, 1);
    }

    /// Phase 144: `eventfd_read(-1, NULL)` resolves the fd before
    /// the pointer, so a bad fd returns EBADF rather than the
    /// pre-Phase-144 EFAULT.  Renamed from
    /// `test_eventfd_read_null_returns_efault` to reflect the new
    /// precedence — see `epoll::tests` for the full Phase 144
    /// coverage including the valid-fd-NULL-pointer EFAULT case.
    #[test]
    fn test_eventfd_read_bad_fd_beats_null_pointer_efault() {
        crate::errno::set_errno(0);
        assert_eq!(eventfd_read(-1, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    /// Phase 144: `eventfd_write(-1, u64::MAX)` resolves the fd
    /// before the value, so a bad fd returns EBADF rather than the
    /// pre-Phase-144 value-EINVAL.  The U64_MAX rejection path is
    /// still tested via a valid eventfd in
    /// `epoll::tests::test_eventfd_write_phase144_valid_fd_u64_max_is_einval`.
    #[test]
    fn test_eventfd_write_bad_fd_beats_value_max_einval() {
        crate::errno::set_errno(0);
        assert_eq!(eventfd_write(-1, u64::MAX), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_eventfd_t_size() {
        assert_eq!(core::mem::size_of::<EventfdT>(), 8);
    }
}
