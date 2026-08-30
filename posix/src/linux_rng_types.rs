//! `<linux/random.h>` — Random number subsystem constants.
//!
//! Random subsystem constants covering getrandom flags,
//! GRND flags, and entropy source identifiers.

// ---------------------------------------------------------------------------
// getrandom() flags (GRND_*)
// ---------------------------------------------------------------------------

/// Non-blocking (return error if insufficient entropy).
pub const GRND_NONBLOCK: u32 = 0x0001;
/// Use /dev/random pool instead of /dev/urandom.
pub const GRND_RANDOM: u32 = 0x0002;
/// Use insecure random (for boot).
pub const GRND_INSECURE: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Entropy source identifiers
// ---------------------------------------------------------------------------

/// Keyboard entropy source.
pub const RANDOM_INPUT_KEYBOARD: u32 = 0;
/// Mouse entropy source.
pub const RANDOM_INPUT_MOUSE: u32 = 1;
/// Disk entropy source.
pub const RANDOM_INPUT_DISK: u32 = 2;
/// Network entropy source.
pub const RANDOM_INPUT_NET: u32 = 3;
/// Interrupt entropy source.
pub const RANDOM_INPUT_IRQ: u32 = 4;
/// Timer entropy source.
pub const RANDOM_INPUT_TIMER: u32 = 5;

// ---------------------------------------------------------------------------
// CRNG state
// ---------------------------------------------------------------------------

/// CRNG empty (no entropy).
pub const CRNG_EMPTY: u32 = 0;
/// CRNG early (some entropy, not fully seeded).
pub const CRNG_EARLY: u32 = 1;
/// CRNG ready (fully seeded).
pub const CRNG_READY: u32 = 2;

// ---------------------------------------------------------------------------
// UUID generation
// ---------------------------------------------------------------------------

/// UUID string length (with hyphens and null).
pub const UUID_STRING_LEN: u32 = 37;
/// UUID binary size.
pub const UUID_SIZE: u32 = 16;

// ---------------------------------------------------------------------------
// Jitter entropy parameters
// ---------------------------------------------------------------------------

/// Number of loops for jitter timing.
pub const JENT_LOOP_COUNT: u32 = 64;
/// APT cutoff for health test.
pub const JENT_APT_CUTOFF: u32 = 325;
/// RCT cutoff for health test.
pub const JENT_RCT_CUTOFF: u32 = 30;
/// LFSR poly.
pub const JENT_LFSR_POLY: u32 = 0xF4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grnd_flags_power_of_two() {
        let flags = [GRND_NONBLOCK, GRND_RANDOM, GRND_INSECURE];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:04x} not power of two", f);
        }
    }

    #[test]
    fn test_grnd_flags_no_overlap() {
        let flags = [GRND_NONBLOCK, GRND_RANDOM, GRND_INSECURE];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_input_sources_distinct() {
        let sources = [
            RANDOM_INPUT_KEYBOARD,
            RANDOM_INPUT_MOUSE,
            RANDOM_INPUT_DISK,
            RANDOM_INPUT_NET,
            RANDOM_INPUT_IRQ,
            RANDOM_INPUT_TIMER,
        ];
        for i in 0..sources.len() {
            for j in (i + 1)..sources.len() {
                assert_ne!(sources[i], sources[j]);
            }
        }
    }

    #[test]
    fn test_crng_state_ordering() {
        assert!(CRNG_EMPTY < CRNG_EARLY);
        assert!(CRNG_EARLY < CRNG_READY);
    }

    #[test]
    fn test_uuid_sizes() {
        assert_eq!(UUID_SIZE, 16);
        assert_eq!(UUID_STRING_LEN, 37);
    }

    #[test]
    fn test_jent_params() {
        assert!(JENT_LOOP_COUNT > 0);
        assert!(JENT_APT_CUTOFF > 0);
        assert!(JENT_RCT_CUTOFF > 0);
    }
}
