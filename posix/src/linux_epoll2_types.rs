//! `<sys/epoll.h>` — Epoll event and flag constants.
//!
//! Epoll is Linux's scalable I/O event notification mechanism.
//! These constants define event types, epoll_create1 flags,
//! and epoll_ctl operations.

// ---------------------------------------------------------------------------
// epoll_create1 flags
// ---------------------------------------------------------------------------

/// Set close-on-exec on the epoll fd.
pub const EPOLL_CLOEXEC: u32 = 0o2000000;

// ---------------------------------------------------------------------------
// epoll_ctl operations
// ---------------------------------------------------------------------------

/// Add a file descriptor to the epoll set.
pub const EPOLL_CTL_ADD: u32 = 1;
/// Remove a file descriptor from the epoll set.
pub const EPOLL_CTL_DEL: u32 = 2;
/// Modify the events for a file descriptor.
pub const EPOLL_CTL_MOD: u32 = 3;

// ---------------------------------------------------------------------------
// Epoll event flags
// ---------------------------------------------------------------------------

/// File descriptor is ready for reading.
pub const EPOLLIN: u32 = 0x001;
/// File descriptor is ready for writing.
pub const EPOLLOUT: u32 = 0x004;
/// Error condition on file descriptor.
pub const EPOLLERR: u32 = 0x008;
/// Hang up on file descriptor.
pub const EPOLLHUP: u32 = 0x010;
/// Urgent/priority data available.
pub const EPOLLPRI: u32 = 0x002;
/// Remote peer closed connection (or shut down writing half).
pub const EPOLLRDHUP: u32 = 0x2000;
/// Set edge-triggered mode (default is level-triggered).
pub const EPOLLET: u32 = 1 << 31;
/// Set one-shot mode (auto-disable after one event).
pub const EPOLLONESHOT: u32 = 1 << 30;
/// Wake only one waiter (for exclusive wakeup).
pub const EPOLLEXCLUSIVE: u32 = 1 << 28;
/// Wakeup is required (for epoll_pwait2).
pub const EPOLLWAKEUP: u32 = 1 << 29;

// ---------------------------------------------------------------------------
// Epoll limits
// ---------------------------------------------------------------------------

/// Maximum number of events per epoll_wait call.
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
    fn test_event_flags_basic_no_overlap() {
        let flags = [EPOLLIN, EPOLLPRI, EPOLLOUT, EPOLLERR, EPOLLHUP];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_modifier_flags_no_overlap() {
        let flags = [EPOLLET, EPOLLONESHOT, EPOLLEXCLUSIVE, EPOLLWAKEUP];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_modifier_flags_power_of_two() {
        assert!(EPOLLET.is_power_of_two());
        assert!(EPOLLONESHOT.is_power_of_two());
        assert!(EPOLLEXCLUSIVE.is_power_of_two());
        assert!(EPOLLWAKEUP.is_power_of_two());
    }

    #[test]
    fn test_cloexec() {
        assert_eq!(EPOLL_CLOEXEC, 0o2000000);
    }

    #[test]
    fn test_epollin() {
        assert_eq!(EPOLLIN, 1);
    }

    #[test]
    fn test_epollet() {
        assert_eq!(EPOLLET, 1 << 31);
    }

    #[test]
    fn test_max_events() {
        assert_eq!(EPOLL_MAX_EVENTS, 4096);
    }
}
