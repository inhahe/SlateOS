//! `<sys/epoll.h>` — event polling interface.
//!
//! Re-exports the primary epoll API from the `epoll` module.
//! Programs that include `<sys/epoll.h>` can find everything here.

pub use crate::epoll::EpollEvent;
pub use crate::epoll::epoll_create;
pub use crate::epoll::epoll_create1;
pub use crate::epoll::epoll_ctl;
pub use crate::epoll::epoll_wait;

pub use crate::epoll::EPOLLERR;
pub use crate::epoll::EPOLLET;
pub use crate::epoll::EPOLLHUP;
pub use crate::epoll::EPOLLIN;
pub use crate::epoll::EPOLLOUT;

/// Remote peer closed connection or shut down writing half.
pub const EPOLLRDHUP: u32 = 0x2000;

/// One-shot notification: after one event, disable the fd.
pub const EPOLLONESHOT: u32 = 1 << 30;

/// Set close-on-exec on the epoll file descriptor.
pub const EPOLL_CLOEXEC: i32 = 0o2_000_000;

pub use crate::epoll::EPOLL_CTL_ADD;
pub use crate::epoll::EPOLL_CTL_DEL;
pub use crate::epoll::EPOLL_CTL_MOD;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epoll_events() {
        assert_ne!(EPOLLIN, 0);
        assert_ne!(EPOLLOUT, 0);
        assert_ne!(EPOLLERR, 0);
        assert_ne!(EPOLLHUP, 0);
    }

    #[test]
    fn test_epoll_ctl_ops() {
        assert_eq!(EPOLL_CTL_ADD, 1);
        assert_eq!(EPOLL_CTL_DEL, 2);
        assert_eq!(EPOLL_CTL_MOD, 3);
    }

    #[test]
    fn test_epoll_create_returns_valid_fd() {
        // epoll_create with size > 0 should return a valid userspace fd.
        let fd = epoll_create(10);
        assert!(fd >= 0, "expected valid fd, got {fd}");
        crate::file::close(fd);
    }

    #[test]
    fn test_epoll_event_size() {
        assert!(core::mem::size_of::<EpollEvent>() > 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(EPOLLIN, crate::epoll::EPOLLIN);
        assert_eq!(EPOLLOUT, crate::epoll::EPOLLOUT);
        assert_eq!(EPOLL_CTL_ADD, crate::epoll::EPOLL_CTL_ADD);
    }
}
