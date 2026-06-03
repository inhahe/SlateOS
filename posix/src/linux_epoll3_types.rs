//! `<linux/eventpoll.h>` — Additional epoll constants (batch 3).
//!
//! Supplementary epoll constants covering epoll internal states,
//! polling modes, and ctl-batch operations.

// ---------------------------------------------------------------------------
// Epoll create flags
// ---------------------------------------------------------------------------

/// Set close-on-exec on the epoll fd.
pub const EPOLL_CLOEXEC: u32 = 0o2000000;

// ---------------------------------------------------------------------------
// Epoll ctl operations (additional)
// ---------------------------------------------------------------------------

/// Add an entry.
pub const EPOLL_CTL_ADD: u32 = 1;
/// Delete an entry.
pub const EPOLL_CTL_DEL: u32 = 2;
/// Modify an entry.
pub const EPOLL_CTL_MOD: u32 = 3;

// ---------------------------------------------------------------------------
// Epoll event flags (additional)
// ---------------------------------------------------------------------------

/// Available for read.
pub const EPOLLIN: u32 = 0x00000001;
/// Urgent data available.
pub const EPOLLPRI: u32 = 0x00000002;
/// Available for write.
pub const EPOLLOUT: u32 = 0x00000004;
/// Read half of connection closed.
pub const EPOLLRDNORM: u32 = 0x00000040;
/// Same as EPOLLRDNORM for read band.
pub const EPOLLRDBAND: u32 = 0x00000080;
/// Writing now won't block.
pub const EPOLLWRNORM: u32 = 0x00000100;
/// Out-of-band data available.
pub const EPOLLWRBAND: u32 = 0x00000200;
/// Message available.
pub const EPOLLMSG: u32 = 0x00000400;
/// Error condition.
pub const EPOLLERR: u32 = 0x00000008;
/// Hang up.
pub const EPOLLHUP: u32 = 0x00000010;
/// Read hang up.
pub const EPOLLRDHUP: u32 = 0x00002000;
/// Exclusive wakeup.
pub const EPOLLEXCLUSIVE: u32 = 1 << 28;
/// Wakeup on event only once.
pub const EPOLLWAKEUP: u32 = 1 << 29;
/// One-shot event.
pub const EPOLLONESHOT: u32 = 1 << 30;
/// Edge-triggered.
pub const EPOLLET: u32 = 1 << 31;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctl_operations_distinct() {
        let ops = [EPOLL_CTL_ADD, EPOLL_CTL_DEL, EPOLL_CTL_MOD];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_event_flags_values() {
        assert_eq!(EPOLLIN, 0x001);
        assert_eq!(EPOLLPRI, 0x002);
        assert_eq!(EPOLLOUT, 0x004);
        assert_eq!(EPOLLERR, 0x008);
        assert_eq!(EPOLLHUP, 0x010);
    }

    #[test]
    fn test_basic_flags_no_overlap() {
        let flags = [EPOLLIN, EPOLLPRI, EPOLLOUT, EPOLLERR, EPOLLHUP];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_special_flags_power_of_two() {
        assert!(EPOLLEXCLUSIVE.is_power_of_two());
        assert!(EPOLLWAKEUP.is_power_of_two());
        assert!(EPOLLONESHOT.is_power_of_two());
        assert!(EPOLLET.is_power_of_two());
    }

    #[test]
    fn test_special_flags_no_overlap() {
        let flags = [EPOLLEXCLUSIVE, EPOLLWAKEUP, EPOLLONESHOT, EPOLLET];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_rw_flags_distinct() {
        let flags = [
            EPOLLRDNORM,
            EPOLLRDBAND,
            EPOLLWRNORM,
            EPOLLWRBAND,
            EPOLLMSG,
            EPOLLRDHUP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
