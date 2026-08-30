//! `<linux/eventfd.h>` — Additional eventfd constants (batch 3).
//!
//! Supplementary eventfd constants covering create flags,
//! value limits, and semaphore mode behavior.

// ---------------------------------------------------------------------------
// Eventfd create flags
// ---------------------------------------------------------------------------

/// Set close-on-exec.
pub const EFD_CLOEXEC: u32 = 0o2000000;
/// Set non-blocking.
pub const EFD_NONBLOCK: u32 = 0o4000;
/// Semaphore mode.
pub const EFD_SEMAPHORE: u32 = 1;

// ---------------------------------------------------------------------------
// Eventfd value limits
// ---------------------------------------------------------------------------

/// Maximum counter value (2^64 - 2).
pub const EFD_MAX_VALUE: u64 = 0xFFFFFFFFFFFFFFFE;

/// Size of eventfd read/write (u64 = 8 bytes).
pub const EFD_VALSIZE: u32 = 8;

// ---------------------------------------------------------------------------
// Eventfd states
// ---------------------------------------------------------------------------

/// Counter is zero (would block on read).
pub const EFD_STATE_ZERO: u32 = 0;
/// Counter is non-zero (readable).
pub const EFD_STATE_NONZERO: u32 = 1;
/// Counter is at max (would block on write).
pub const EFD_STATE_MAX: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_flags_distinct() {
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
        // Max value is u64::MAX - 1
        assert_eq!(EFD_MAX_VALUE, u64::MAX - 1);
    }

    #[test]
    fn test_valsize() {
        assert_eq!(EFD_VALSIZE, 8);
        assert_eq!(EFD_VALSIZE as usize, core::mem::size_of::<u64>());
    }

    #[test]
    fn test_states_distinct() {
        let states = [EFD_STATE_ZERO, EFD_STATE_NONZERO, EFD_STATE_MAX];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
