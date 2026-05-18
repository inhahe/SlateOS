//! `<crypto/rng.h>` — Cryptographic random number generator constants.
//!
//! The kernel maintains multiple RNG sources: the primary CRNG
//! (Cryptographic Random Number Generator) based on ChaCha20, fed by
//! hardware entropy (RDRAND, interrupt timing, device noise). DRBG
//! (Deterministic Random Bit Generator) implementations provide
//! NIST-compliant RNG for FIPS environments. The kernel RNG is the
//! source for /dev/urandom, getrandom(), and all internal random needs.

// ---------------------------------------------------------------------------
// RNG types
// ---------------------------------------------------------------------------

/// CRNG (kernel primary ChaCha20-based CSPRNG).
pub const RNG_TYPE_CRNG: u32 = 0;
/// DRBG-HMAC (NIST SP 800-90A HMAC-DRBG).
pub const RNG_TYPE_DRBG_HMAC: u32 = 1;
/// DRBG-CTR (NIST SP 800-90A CTR-DRBG, AES-based).
pub const RNG_TYPE_DRBG_CTR: u32 = 2;
/// DRBG-HASH (NIST SP 800-90A Hash-DRBG).
pub const RNG_TYPE_DRBG_HASH: u32 = 3;
/// JITTER (CPU jitter-based entropy source).
pub const RNG_TYPE_JITTER: u32 = 4;

// ---------------------------------------------------------------------------
// Entropy sources
// ---------------------------------------------------------------------------

/// Hardware RNG (RDRAND/RDSEED on x86).
pub const ENTROPY_SRC_HWRNG: u32 = 0;
/// Interrupt timing entropy.
pub const ENTROPY_SRC_IRQ: u32 = 1;
/// Disk I/O timing entropy.
pub const ENTROPY_SRC_DISK: u32 = 2;
/// Input device timing (keyboard/mouse).
pub const ENTROPY_SRC_INPUT: u32 = 3;
/// CPU jitter entropy.
pub const ENTROPY_SRC_JITTER: u32 = 4;
/// Architecture-specific (e.g., ARM TRNG).
pub const ENTROPY_SRC_ARCH: u32 = 5;

// ---------------------------------------------------------------------------
// RNG states
// ---------------------------------------------------------------------------

/// RNG is not seeded (insufficient entropy, blocking).
pub const RNG_STATE_UNSEEDED: u32 = 0;
/// RNG is partially seeded (usable but not fully trusted).
pub const RNG_STATE_EARLY: u32 = 1;
/// RNG is fully seeded (cryptographically secure).
pub const RNG_STATE_READY: u32 = 2;

// ---------------------------------------------------------------------------
// getrandom() flags
// ---------------------------------------------------------------------------

/// Block until RNG is fully seeded.
pub const GRND_DEFAULT: u32 = 0x00;
/// Non-blocking (fail with EAGAIN if not seeded).
pub const GRND_NONBLOCK: u32 = 0x01;
/// Use /dev/random pool (higher quality, may block longer).
pub const GRND_RANDOM: u32 = 0x02;
/// Insecure (return bytes even if unseeded, for early boot).
pub const GRND_INSECURE: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rng_types_distinct() {
        let types = [
            RNG_TYPE_CRNG, RNG_TYPE_DRBG_HMAC, RNG_TYPE_DRBG_CTR,
            RNG_TYPE_DRBG_HASH, RNG_TYPE_JITTER,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_entropy_sources_distinct() {
        let sources = [
            ENTROPY_SRC_HWRNG, ENTROPY_SRC_IRQ, ENTROPY_SRC_DISK,
            ENTROPY_SRC_INPUT, ENTROPY_SRC_JITTER, ENTROPY_SRC_ARCH,
        ];
        for i in 0..sources.len() {
            for j in (i + 1)..sources.len() {
                assert_ne!(sources[i], sources[j]);
            }
        }
    }

    #[test]
    fn test_states_ordered() {
        assert!(RNG_STATE_UNSEEDED < RNG_STATE_EARLY);
        assert!(RNG_STATE_EARLY < RNG_STATE_READY);
    }

    #[test]
    fn test_grnd_flags() {
        assert_eq!(GRND_DEFAULT, 0);
        assert!(GRND_NONBLOCK.is_power_of_two());
        assert!(GRND_RANDOM.is_power_of_two());
        assert!(GRND_INSECURE.is_power_of_two());
        assert_eq!(GRND_NONBLOCK & GRND_RANDOM, 0);
        assert_eq!(GRND_RANDOM & GRND_INSECURE, 0);
    }
}
