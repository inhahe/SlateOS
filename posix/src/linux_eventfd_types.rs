//! `<linux/eventfd.h>` — eventfd() file descriptor constants.
//!
//! eventfd creates a file descriptor for event notification. It
//! maintains an internal 64-bit counter. Writes add to the counter,
//! reads either return the counter (and reset to zero) or return 1
//! (in semaphore mode, decrementing by 1). Used for lightweight
//! signaling between threads or between kernel and userspace.

// ---------------------------------------------------------------------------
// eventfd flags
// ---------------------------------------------------------------------------

/// Set close-on-exec on the new fd.
pub const EFD_CLOEXEC: u32 = 0o200_0000;
/// Set non-blocking on the new fd.
pub const EFD_NONBLOCK: u32 = 0o000_4000;
/// Semaphore mode: read returns 1 and decrements.
pub const EFD_SEMAPHORE: u32 = 0x0000_0001;

// ---------------------------------------------------------------------------
// eventfd limits
// ---------------------------------------------------------------------------

/// Maximum value that can be stored (2^64 - 2).
pub const EFD_MAX_VALUE: u64 = 0xFFFF_FFFF_FFFF_FFFE;

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
    fn test_semaphore_is_bit_zero() {
        assert_eq!(EFD_SEMAPHORE, 1);
    }

    #[test]
    fn test_max_value() {
        assert_eq!(EFD_MAX_VALUE, u64::MAX - 1);
    }
}
