//! `<sys/eventfd.h>` — Eventfd flag and limit constants.
//!
//! Eventfd provides a lightweight inter-thread/process notification
//! mechanism using a uint64 counter. Reading returns and resets
//! (or decrements) the counter; writing increments it.

// ---------------------------------------------------------------------------
// eventfd flags
// ---------------------------------------------------------------------------

/// Set close-on-exec flag.
pub const EFD_CLOEXEC: u32 = 0o2000000;
/// Set non-blocking I/O.
pub const EFD_NONBLOCK: u32 = 0o4000;
/// Use semaphore semantics (read returns 1, decrement counter).
pub const EFD_SEMAPHORE: u32 = 1;

// ---------------------------------------------------------------------------
// eventfd counter limits
// ---------------------------------------------------------------------------

/// Maximum value writable to eventfd (2^64 - 2).
pub const EFD_MAX_VALUE: u64 = u64::MAX - 1;

// ---------------------------------------------------------------------------
// eventfd read/write sizes
// ---------------------------------------------------------------------------

/// Size of the eventfd counter value (always 8 bytes / u64).
pub const EFD_COUNTER_SIZE: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_distinct() {
        let flags = [EFD_CLOEXEC, EFD_NONBLOCK, EFD_SEMAPHORE];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_semaphore_is_one() {
        assert_eq!(EFD_SEMAPHORE, 1);
    }

    #[test]
    fn test_max_value() {
        assert_eq!(EFD_MAX_VALUE, u64::MAX - 1);
    }

    #[test]
    fn test_counter_size() {
        assert_eq!(EFD_COUNTER_SIZE, 8);
    }

    #[test]
    fn test_cloexec() {
        assert_eq!(EFD_CLOEXEC, 0o2000000);
    }
}
