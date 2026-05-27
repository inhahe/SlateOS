//! `<linux/eventfd.h>` — event file descriptor (kernel view).
//!
//! Re-exports from `sys_eventfd` / `epoll`.

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use crate::epoll::eventfd;
pub use crate::epoll::eventfd_read;
pub use crate::epoll::eventfd_write;
pub use crate::epoll::EFD_CLOEXEC;
pub use crate::epoll::EFD_NONBLOCK;
pub use crate::epoll::EFD_SEMAPHORE;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_efd_flags() {
        assert_ne!(EFD_CLOEXEC, EFD_NONBLOCK);
        assert_ne!(EFD_CLOEXEC, EFD_SEMAPHORE);
        assert_ne!(EFD_NONBLOCK, EFD_SEMAPHORE);
    }

    /// `EFD_SEMAPHORE` is rejected before any syscall — verifies the
    /// re-export reaches the real implementation that does the check.
    #[test]
    fn test_eventfd_semaphore_rejected() {
        crate::errno::set_errno(0);
        assert_eq!(eventfd(0, EFD_SEMAPHORE), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(EFD_CLOEXEC, crate::epoll::EFD_CLOEXEC);
        assert_eq!(EFD_NONBLOCK, crate::epoll::EFD_NONBLOCK);
        assert_eq!(EFD_SEMAPHORE, crate::epoll::EFD_SEMAPHORE);
    }
}
