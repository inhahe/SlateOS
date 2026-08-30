//! `<crypto/hash.h>` — Cryptographic hash algorithm constants.
//!
//! Hash algorithms produce fixed-size digests from arbitrary-length
//! input. They're used for data integrity (checksums), authentication
//! (HMAC, digital signatures), key derivation, and content addressing.
//! The kernel uses hashes extensively: file integrity (IMA/fsverity),
//! network protocols (TCP MD5 option), disk encryption (key wrapping),
//! and BPF program tagging.

// ---------------------------------------------------------------------------
// Hash algorithm identifiers
// ---------------------------------------------------------------------------

/// MD5 (128-bit, deprecated for security, still used in legacy).
pub const HASH_ALG_MD5: u32 = 0;
/// SHA-1 (160-bit, deprecated for collision resistance).
pub const HASH_ALG_SHA1: u32 = 1;
/// SHA-224 (224-bit).
pub const HASH_ALG_SHA224: u32 = 2;
/// SHA-256 (256-bit, most common).
pub const HASH_ALG_SHA256: u32 = 3;
/// SHA-384 (384-bit).
pub const HASH_ALG_SHA384: u32 = 4;
/// SHA-512 (512-bit).
pub const HASH_ALG_SHA512: u32 = 5;
/// SHA3-256 (256-bit, Keccak sponge).
pub const HASH_ALG_SHA3_256: u32 = 6;
/// SHA3-512 (512-bit).
pub const HASH_ALG_SHA3_512: u32 = 7;
/// BLAKE2b-256 (256-bit, fast).
pub const HASH_ALG_BLAKE2B_256: u32 = 8;
/// BLAKE2b-512 (512-bit).
pub const HASH_ALG_BLAKE2B_512: u32 = 9;
/// SM3 (256-bit, Chinese national standard).
pub const HASH_ALG_SM3: u32 = 10;
/// CRC32 (32-bit, non-cryptographic).
pub const HASH_ALG_CRC32: u32 = 11;
/// CRC32C (32-bit, Castagnoli, used by ext4/btrfs).
pub const HASH_ALG_CRC32C: u32 = 12;
/// xxHash (64-bit, non-cryptographic, fast).
pub const HASH_ALG_XXHASH: u32 = 13;

// ---------------------------------------------------------------------------
// Hash digest sizes (bytes)
// ---------------------------------------------------------------------------

/// MD5 digest size.
pub const HASH_SIZE_MD5: u32 = 16;
/// SHA-1 digest size.
pub const HASH_SIZE_SHA1: u32 = 20;
/// SHA-256 digest size.
pub const HASH_SIZE_SHA256: u32 = 32;
/// SHA-384 digest size.
pub const HASH_SIZE_SHA384: u32 = 48;
/// SHA-512 digest size.
pub const HASH_SIZE_SHA512: u32 = 64;
/// BLAKE2b-256 digest size.
pub const HASH_SIZE_BLAKE2B_256: u32 = 32;
/// Maximum digest size across all algorithms.
pub const HASH_SIZE_MAX: u32 = 64;

// ---------------------------------------------------------------------------
// Hash flags
// ---------------------------------------------------------------------------

/// Algorithm supports HMAC construction.
pub const HASH_FLAG_HMAC: u32 = 0x01;
/// Algorithm has hardware acceleration.
pub const HASH_FLAG_HW_ACCEL: u32 = 0x02;
/// Algorithm is for non-cryptographic checksums only.
pub const HASH_FLAG_NON_CRYPTO: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algorithms_distinct() {
        let algs = [
            HASH_ALG_MD5,
            HASH_ALG_SHA1,
            HASH_ALG_SHA224,
            HASH_ALG_SHA256,
            HASH_ALG_SHA384,
            HASH_ALG_SHA512,
            HASH_ALG_SHA3_256,
            HASH_ALG_SHA3_512,
            HASH_ALG_BLAKE2B_256,
            HASH_ALG_BLAKE2B_512,
            HASH_ALG_SM3,
            HASH_ALG_CRC32,
            HASH_ALG_CRC32C,
            HASH_ALG_XXHASH,
        ];
        for i in 0..algs.len() {
            for j in (i + 1)..algs.len() {
                assert_ne!(algs[i], algs[j]);
            }
        }
    }

    #[test]
    fn test_digest_sizes() {
        assert!(HASH_SIZE_MD5 < HASH_SIZE_SHA1);
        assert!(HASH_SIZE_SHA1 < HASH_SIZE_SHA256);
        assert!(HASH_SIZE_SHA256 < HASH_SIZE_SHA384);
        assert!(HASH_SIZE_SHA384 < HASH_SIZE_SHA512);
        assert!(HASH_SIZE_SHA512 <= HASH_SIZE_MAX);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [HASH_FLAG_HMAC, HASH_FLAG_HW_ACCEL, HASH_FLAG_NON_CRYPTO];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
