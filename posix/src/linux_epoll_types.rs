//! `<linux/eventpoll.h>` — epoll event constants.
//!
//! epoll is Linux's scalable I/O event notification mechanism.
//! Unlike poll/select which scan all fds every call, epoll maintains
//! a kernel-side interest list and returns only ready fds. It scales
//! O(1) with the number of monitored fds, making it suitable for
//! servers handling thousands of connections.

// ---------------------------------------------------------------------------
// epoll_create1 flags
// ---------------------------------------------------------------------------

/// Set close-on-exec on the epoll fd.
pub const EPOLL_CLOEXEC: u32 = 0o200_0000;

// ---------------------------------------------------------------------------
// epoll_ctl operations
// ---------------------------------------------------------------------------

/// Add fd to interest list.
pub const EPOLL_CTL_ADD: u32 = 1;
/// Remove fd from interest list.
pub const EPOLL_CTL_DEL: u32 = 2;
/// Modify events for existing fd.
pub const EPOLL_CTL_MOD: u32 = 3;

// ---------------------------------------------------------------------------
// epoll event flags (events/revents in struct epoll_event)
// ---------------------------------------------------------------------------

/// Available for read (data available, connection accepted, etc).
pub const EPOLLIN: u32 = 0x0000_0001;
/// Available for write (buffer space available).
pub const EPOLLOUT: u32 = 0x0000_0004;
/// Urgent data available (TCP OOB data).
pub const EPOLLPRI: u32 = 0x0000_0002;
/// Error condition (always reported).
pub const EPOLLERR: u32 = 0x0000_0008;
/// Hang up (always reported).
pub const EPOLLHUP: u32 = 0x0000_0010;
/// Peer closed connection (or shutdown write side).
pub const EPOLLRDHUP: u32 = 0x0000_2000;
/// Edge-triggered mode (default is level-triggered).
pub const EPOLLET: u32 = 0x8000_0000;
/// One-shot mode (disable after one event, re-arm with CTL_MOD).
pub const EPOLLONESHOT: u32 = 0x4000_0000;
/// Wake only one waiter (for thundering herd avoidance).
pub const EPOLLEXCLUSIVE: u32 = 0x1000_0000;
/// Wakeup source (prevents system suspend).
pub const EPOLLWAKEUP: u32 = 0x2000_0000;

// ---------------------------------------------------------------------------
// epoll limits
// ---------------------------------------------------------------------------

/// Maximum events returned per epoll_wait call (kernel default).
pub const EPOLL_MAX_EVENTS: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctl_ops_distinct() {
        let ops = [EPOLL_CTL_ADD, EPOLL_CTL_DEL, EPOLL_CTL_MOD];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_event_flags_no_overlap() {
        let flags = [
            EPOLLIN, EPOLLPRI, EPOLLOUT, EPOLLERR, EPOLLHUP,
            EPOLLRDHUP, EPOLLET, EPOLLONESHOT, EPOLLEXCLUSIVE,
            EPOLLWAKEUP,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_epollin_epollout_distinct() {
        assert_ne!(EPOLLIN, EPOLLOUT);
    }

    #[test]
    fn test_max_events() {
        assert!(EPOLL_MAX_EVENTS > 0);
    }
}
