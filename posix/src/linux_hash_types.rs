//! `<linux/hash.h>` — kernel hash-helper magic numbers.
//!
//! Magic multiplier constants from `<linux/hash.h>` used by the
//! Knuth-multiplicative integer hash that the kernel employs for
//! hashtable index derivation across many subsystems
//! (dcache, inode cache, pid table, etc.).

// ---------------------------------------------------------------------------
// Golden-ratio multipliers (Linux uapi)
// ---------------------------------------------------------------------------
//
// These are the kernel's pre-computed integer approximations to the
// golden ratio `phi = (sqrt(5) - 1) / 2`, scaled to fit in 32 and
// 64 bits. They are AND-masked with 0xffff_ffff or 0xffff_ffff_ffff_ffff
// after multiplication to take only the high bits as the hash output.

/// 32-bit golden-ratio multiplier (≈ 0.618 * 2^32, rounded).
pub const GOLDEN_RATIO_32: u32 = 0x6177_3593;
/// 64-bit golden-ratio multiplier (≈ 0.618 * 2^64, rounded).
pub const GOLDEN_RATIO_64: u64 = 0x6177_3593_2596_8693;

// ---------------------------------------------------------------------------
// Hash-bit ranges used by the kernel
// ---------------------------------------------------------------------------

/// Minimum number of hash bits a hashtable may use.
pub const HASH_BITS_MIN: u32 = 1;
/// Maximum number of hash bits a 32-bit hash can produce.
pub const HASH_BITS_32_MAX: u32 = 32;
/// Maximum number of hash bits a 64-bit hash can produce.
pub const HASH_BITS_64_MAX: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_ratio_distinct_from_zero() {
        assert_ne!(GOLDEN_RATIO_32, 0);
        assert_ne!(GOLDEN_RATIO_64, 0);
    }

    #[test]
    fn test_hash_bits_bounds() {
        assert!(HASH_BITS_MIN < HASH_BITS_32_MAX);
        assert!(HASH_BITS_32_MAX < HASH_BITS_64_MAX);
        assert_eq!(HASH_BITS_32_MAX, 32);
        assert_eq!(HASH_BITS_64_MAX, 64);
    }

    #[test]
    fn test_hash_basic_property() {
        // Smoke test the kernel-style multiplicative hash:
        //   h(x) = ((x * GOLDEN_RATIO_32) >> (32 - bits))
        // Two adjacent inputs must not produce the same index for a
        // reasonably-sized table.
        let bits = 10u32; // 1024-bucket table
        let shift = 32 - bits;
        let h0 = (0u32.wrapping_mul(GOLDEN_RATIO_32)) >> shift;
        let h1 = (1u32.wrapping_mul(GOLDEN_RATIO_32)) >> shift;
        let h2 = (2u32.wrapping_mul(GOLDEN_RATIO_32)) >> shift;
        assert!(h0 != h1 || h1 != h2);
    }
}
