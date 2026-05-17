//! `<linux/random.h>` — getrandom() and entropy pool constants.
//!
//! The kernel random number generator maintains an entropy pool fed by
//! hardware events (interrupts, timing jitter, etc.). getrandom() is
//! the modern interface for obtaining cryptographically secure random
//! bytes. It blocks until sufficient entropy is available (unless
//! GRND_NONBLOCK is specified).

// ---------------------------------------------------------------------------
// getrandom() flags
// ---------------------------------------------------------------------------

/// Use /dev/urandom source (doesn't block after init).
pub const GRND_NONBLOCK: u32 = 0x0001;
/// Use /dev/random source (may block for entropy).
pub const GRND_RANDOM: u32 = 0x0002;
/// Don't block during early boot (return error instead).
pub const GRND_INSECURE: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Entropy pool ioctl commands (on /dev/random)
// ---------------------------------------------------------------------------

/// Get entropy count (bits).
pub const RNDGETENTCNT: u32 = 0x8004_5200;
/// Add to entropy count.
pub const RNDADDTOENTCNT: u32 = 0x4004_5201;
/// Get random pool size.
pub const RNDGETPOOL: u32 = 0x8002_5202;
/// Add entropy to pool.
pub const RNDADDENTROPY: u32 = 0x4008_5203;
/// Clear entropy pool (requires CAP_SYS_ADMIN).
pub const RNDZAPENTCNT: u32 = 0x5204;
/// Reseed CRNG from entropy pool.
pub const RNDRESEEDCRNG: u32 = 0x5207;

// ---------------------------------------------------------------------------
// Entropy pool sizes
// ---------------------------------------------------------------------------

/// Entropy pool size in bits (Linux 5.18+: 256 bits CRNG).
pub const POOL_BITS: u32 = 256;
/// Minimum entropy bits needed for CRNG readiness.
pub const CRNG_INIT_THRESHOLD: u32 = 256;

// ---------------------------------------------------------------------------
// Random device minor numbers
// ---------------------------------------------------------------------------

/// /dev/random minor number.
pub const RANDOM_MINOR: u32 = 8;
/// /dev/urandom minor number.
pub const URANDOM_MINOR: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_getrandom_flags_no_overlap() {
        let flags = [GRND_NONBLOCK, GRND_RANDOM, GRND_INSECURE];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            RNDGETENTCNT, RNDADDTOENTCNT, RNDGETPOOL,
            RNDADDENTROPY, RNDZAPENTCNT, RNDRESEEDCRNG,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_pool_size() {
        assert_eq!(POOL_BITS, 256);
        assert_eq!(CRNG_INIT_THRESHOLD, 256);
    }

    #[test]
    fn test_minor_numbers_distinct() {
        assert_ne!(RANDOM_MINOR, URANDOM_MINOR);
    }
}
