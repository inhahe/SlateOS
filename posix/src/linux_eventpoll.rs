//! `<linux/eventpoll.h>` — epoll constants.
//!
//! epoll is Linux's scalable I/O event notification mechanism.
//! It monitors multiple file descriptors for readiness events
//! using an efficient kernel data structure (red-black tree +
//! ready list), scaling O(1) with the number of monitored fds.

// ---------------------------------------------------------------------------
// epoll_ctl operations
// ---------------------------------------------------------------------------

/// Add a file descriptor to the epoll set.
pub const EPOLL_CTL_ADD: u32 = 1;
/// Remove a file descriptor from the epoll set.
pub const EPOLL_CTL_DEL: u32 = 2;
/// Modify events for an existing file descriptor.
pub const EPOLL_CTL_MOD: u32 = 3;

// ---------------------------------------------------------------------------
// epoll event flags
// ---------------------------------------------------------------------------

/// Ready for read.
pub const EPOLLIN: u32 = 0x001;
/// Ready for write.
pub const EPOLLOUT: u32 = 0x004;
/// Urgent data (OOB) available.
pub const EPOLLPRI: u32 = 0x002;
/// Error condition.
pub const EPOLLERR: u32 = 0x008;
/// Hang up (peer closed connection).
pub const EPOLLHUP: u32 = 0x010;
/// Read hang up (peer closed writing end).
pub const EPOLLRDHUP: u32 = 0x2000;
/// Edge-triggered mode.
pub const EPOLLET: u32 = 1 << 31;
/// One-shot mode (disable after one event).
pub const EPOLLONESHOT: u32 = 1 << 30;
/// Wake-up exclusive (avoid thundering herd).
pub const EPOLLEXCLUSIVE: u32 = 1 << 28;
/// Busy-poll hint.
pub const EPOLLNVAL: u32 = 0x020;

// ---------------------------------------------------------------------------
// epoll_create1 flags
// ---------------------------------------------------------------------------

/// Close-on-exec flag for epoll fd.
pub const EPOLL_CLOEXEC: u32 = 0x80000;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Default max epoll watches per user (sysctl).
pub const EPOLL_MAX_WATCHES_DEFAULT: u32 = 1_048_576;

/// Maximum events returned per epoll_wait call.
pub const EPOLL_MAX_EVENTS: u32 = 0x7FFFFFFF;

// ---------------------------------------------------------------------------
// Sysctl paths
// ---------------------------------------------------------------------------

/// Maximum user watches sysctl.
pub const SYSCTL_EPOLL_MAX_USER_WATCHES: &str = "fs.epoll.max_user_watches";

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
    fn test_event_flags_distinct() {
        let flags = [
            EPOLLIN, EPOLLOUT, EPOLLPRI, EPOLLERR,
            EPOLLHUP, EPOLLRDHUP, EPOLLET, EPOLLONESHOT,
            EPOLLEXCLUSIVE, EPOLLNVAL,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_edge_triggered_high_bit() {
        assert_eq!(EPOLLET, 1 << 31);
    }

    #[test]
    fn test_oneshot_high_bit() {
        assert_eq!(EPOLLONESHOT, 1 << 30);
    }

    #[test]
    fn test_cloexec() {
        assert!(EPOLL_CLOEXEC > 0);
    }

    #[test]
    fn test_max_events() {
        assert!(EPOLL_MAX_EVENTS > 0);
    }

    #[test]
    fn test_basic_flags_no_overlap() {
        // Core read/write/error flags should not overlap
        let basic = [EPOLLIN, EPOLLOUT, EPOLLPRI, EPOLLERR, EPOLLHUP, EPOLLNVAL];
        for i in 0..basic.len() {
            for j in (i + 1)..basic.len() {
                assert_eq!(basic[i] & basic[j], 0, "0x{:x} & 0x{:x}", basic[i], basic[j]);
            }
        }
    }
}
