//! `<sys/eventfd.h>` — eventfd(2) flags and semantics.
//!
//! eventfd is a kernel-managed 64-bit counter exposed as a file
//! descriptor. epoll-based event loops in libevent, libuv, glib,
//! tokio, and asyncio use it for cross-thread wakeups, since
//! writing to the fd is a single atomic operation and reading
//! returns the accumulated count.

// ---------------------------------------------------------------------------
// eventfd2() flags
// ---------------------------------------------------------------------------

/// Set the O_CLOEXEC flag on the new fd.
pub const EFD_CLOEXEC: u32 = 0o2_000_000;
/// Mark the new fd as non-blocking.
pub const EFD_NONBLOCK: u32 = 0o0_004_000;
/// Provide semaphore-like semantics: read decrements by 1, not all.
pub const EFD_SEMAPHORE: u32 = 0o0_000_001;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Each read/write transfers an 8-byte u64.
pub const EFD_TRANSFER_BYTES: usize = 8;
/// Max counter value before write() returns -EAGAIN/-EINVAL.
pub const EFD_MAX_COUNTER: u64 = u64::MAX - 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_values_match_kernel() {
        // EFD_CLOEXEC == O_CLOEXEC = 0o2000000 by definition.
        assert_eq!(EFD_CLOEXEC, 0o2_000_000);
        // EFD_NONBLOCK == O_NONBLOCK = 0o4000.
        assert_eq!(EFD_NONBLOCK, 0o4_000);
        // EFD_SEMAPHORE = 1.
        assert_eq!(EFD_SEMAPHORE, 1);
    }

    #[test]
    fn test_flags_distinct() {
        assert_ne!(EFD_CLOEXEC, EFD_NONBLOCK);
        assert_ne!(EFD_CLOEXEC, EFD_SEMAPHORE);
        assert_ne!(EFD_NONBLOCK, EFD_SEMAPHORE);
        // Composed: all three OR'd cannot collide.
        let all = EFD_CLOEXEC | EFD_NONBLOCK | EFD_SEMAPHORE;
        assert_eq!(all.count_ones(), 3);
    }

    #[test]
    fn test_transfer_size() {
        // The kernel always transfers an 8-byte counter — short reads
        // and writes do not exist on eventfd.
        assert_eq!(EFD_TRANSFER_BYTES, 8);
        // u64::MAX is reserved as "would overflow"; one below it is
        // the highest legal counter value.
        assert_eq!(EFD_MAX_COUNTER, u64::MAX - 1);
    }
}
