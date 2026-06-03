//! `<linux/hw_random.h>` — Hardware random number generator constants.
//!
//! HW RNG constants covering quality levels,
//! device flags, and RNG source types.

// ---------------------------------------------------------------------------
// RNG quality levels
// ---------------------------------------------------------------------------

/// No entropy (for testing).
pub const RNG_QUALITY_NONE: u32 = 0;
/// Low quality (PRNG).
pub const RNG_QUALITY_LOW: u32 = 1;
/// Medium quality (some hardware).
pub const RNG_QUALITY_MEDIUM: u32 = 2;
/// High quality (dedicated TRNG).
pub const RNG_QUALITY_HIGH: u32 = 3;
/// Perfect quality (quantum/physical).
pub const RNG_QUALITY_PERFECT: u32 = 4;

// ---------------------------------------------------------------------------
// RNG device flags
// ---------------------------------------------------------------------------

/// Seed from boot.
pub const HWRNG_FLAG_SEED_BOOT: u32 = 1 << 0;
/// Best available.
pub const HWRNG_FLAG_BEST_AVAILABLE: u32 = 1 << 1;
/// Supplement only (don't replace).
pub const HWRNG_FLAG_SUPPLEMENT: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Entropy pool parameters
// ---------------------------------------------------------------------------

/// Entropy pool size (bits).
pub const POOL_ENTROPY_BITS: u32 = 4096;
/// Input pool word count.
pub const INPUT_POOL_WORDS: u32 = 128;
/// Minimum entropy bits to reseed.
pub const MIN_RESEED_ENTROPY: u32 = 256;

// ---------------------------------------------------------------------------
// /dev/random ioctl commands
// ---------------------------------------------------------------------------

/// Get entropy count.
pub const RNDGETENTCNT: u32 = 0x8004_5200;
/// Add to entropy count.
pub const RNDADDTOENTCNT: u32 = 0x4004_5201;
/// Get pool (available entropy).
pub const RNDGETPOOL: u32 = 0x8002_5202;
/// Add entropy.
pub const RNDADDENTROPY: u32 = 0x4008_5203;
/// Clear pool.
pub const RNDZAPENTCNT: u32 = 0x5204;
/// Clear pool (alias).
pub const RNDCLEARPOOL: u32 = 0x5206;
/// Reseed CRNG.
pub const RNDRESEEDCRNG: u32 = 0x5207;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_levels_distinct() {
        let levels = [
            RNG_QUALITY_NONE,
            RNG_QUALITY_LOW,
            RNG_QUALITY_MEDIUM,
            RNG_QUALITY_HIGH,
            RNG_QUALITY_PERFECT,
        ];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_quality_ordering() {
        assert!(RNG_QUALITY_NONE < RNG_QUALITY_LOW);
        assert!(RNG_QUALITY_LOW < RNG_QUALITY_MEDIUM);
        assert!(RNG_QUALITY_MEDIUM < RNG_QUALITY_HIGH);
        assert!(RNG_QUALITY_HIGH < RNG_QUALITY_PERFECT);
    }

    #[test]
    fn test_device_flags_power_of_two() {
        let flags = [
            HWRNG_FLAG_SEED_BOOT,
            HWRNG_FLAG_BEST_AVAILABLE,
            HWRNG_FLAG_SUPPLEMENT,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_device_flags_no_overlap() {
        let flags = [
            HWRNG_FLAG_SEED_BOOT,
            HWRNG_FLAG_BEST_AVAILABLE,
            HWRNG_FLAG_SUPPLEMENT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_pool_params() {
        assert_eq!(POOL_ENTROPY_BITS, 4096);
        assert_eq!(INPUT_POOL_WORDS, 128);
        assert!(MIN_RESEED_ENTROPY <= POOL_ENTROPY_BITS);
    }

    #[test]
    fn test_ioctl_distinct() {
        let cmds = [
            RNDGETENTCNT,
            RNDADDTOENTCNT,
            RNDGETPOOL,
            RNDADDENTROPY,
            RNDZAPENTCNT,
            RNDCLEARPOOL,
            RNDRESEEDCRNG,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}
