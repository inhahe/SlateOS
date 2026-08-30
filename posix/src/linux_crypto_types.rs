//! `<linux/crypto.h>` — Kernel cryptographic API framework constants.
//!
//! The Linux crypto API provides a unified interface for cryptographic
//! operations: hashing, symmetric ciphers, AEAD, key agreement, and
//! random number generation. Algorithms are registered as "transforms"
//! and can be implemented in software or accelerated by hardware
//! (AES-NI, SHA extensions, crypto engines). Users (dm-crypt, IPsec,
//! fscrypt, AF_ALG sockets) allocate transforms by name and use them
//! through a generic API.

// ---------------------------------------------------------------------------
// Algorithm types
// ---------------------------------------------------------------------------

/// Hash / message digest algorithm.
pub const CRYPTO_ALG_TYPE_HASH: u32 = 0x0000_0000;
/// Symmetric block cipher.
pub const CRYPTO_ALG_TYPE_BLKCIPHER: u32 = 0x0000_0004;
/// Asymmetric cipher (RSA, ECDSA).
pub const CRYPTO_ALG_TYPE_AKCIPHER: u32 = 0x0000_000D;
/// AEAD (Authenticated Encryption with Associated Data).
pub const CRYPTO_ALG_TYPE_AEAD: u32 = 0x0000_0003;
/// Compression algorithm.
pub const CRYPTO_ALG_TYPE_COMPRESS: u32 = 0x0000_0002;
/// Random number generator.
pub const CRYPTO_ALG_TYPE_RNG: u32 = 0x0000_000C;
/// Key derivation function.
pub const CRYPTO_ALG_TYPE_KDF: u32 = 0x0000_000E;
/// Symmetric cipher (skcipher, new API).
pub const CRYPTO_ALG_TYPE_SKCIPHER: u32 = 0x0000_0005;
/// Key agreement (DH, ECDH).
pub const CRYPTO_ALG_TYPE_KPP: u32 = 0x0000_0008;
/// Algorithm type mask.
pub const CRYPTO_ALG_TYPE_MASK: u32 = 0x0000_000F;

// ---------------------------------------------------------------------------
// Algorithm flags
// ---------------------------------------------------------------------------

/// Algorithm is hardware-accelerated.
pub const CRYPTO_ALG_ASYNC: u32 = 0x0000_0080;
/// Algorithm needs fallback (software backup).
pub const CRYPTO_ALG_NEED_FALLBACK: u32 = 0x0000_0100;
/// Algorithm is internal (not user-accessible).
pub const CRYPTO_ALG_INTERNAL: u32 = 0x0000_0200;
/// Algorithm uses optional key.
pub const CRYPTO_ALG_OPTIONAL_KEY: u32 = 0x0000_0400;
/// Algorithm is being tested.
pub const CRYPTO_ALG_TESTED: u32 = 0x0000_0800;

// ---------------------------------------------------------------------------
// Crypto operation results
// ---------------------------------------------------------------------------

/// Operation completed successfully.
pub const CRYPTO_OK: i32 = 0;
/// Operation failed (generic error).
pub const CRYPTO_ERR: i32 = -1;
/// Operation in progress (async).
pub const CRYPTO_EINPROGRESS: i32 = -115;
/// Algorithm not found.
pub const CRYPTO_ENOENT: i32 = -2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alg_types_masked() {
        let types = [
            CRYPTO_ALG_TYPE_HASH,
            CRYPTO_ALG_TYPE_COMPRESS,
            CRYPTO_ALG_TYPE_AEAD,
            CRYPTO_ALG_TYPE_BLKCIPHER,
            CRYPTO_ALG_TYPE_SKCIPHER,
            CRYPTO_ALG_TYPE_KPP,
            CRYPTO_ALG_TYPE_RNG,
            CRYPTO_ALG_TYPE_AKCIPHER,
            CRYPTO_ALG_TYPE_KDF,
        ];
        for t in &types {
            assert_eq!(*t & !CRYPTO_ALG_TYPE_MASK, 0);
        }
    }

    #[test]
    fn test_alg_types_distinct() {
        let types = [
            CRYPTO_ALG_TYPE_HASH,
            CRYPTO_ALG_TYPE_COMPRESS,
            CRYPTO_ALG_TYPE_AEAD,
            CRYPTO_ALG_TYPE_BLKCIPHER,
            CRYPTO_ALG_TYPE_SKCIPHER,
            CRYPTO_ALG_TYPE_KPP,
            CRYPTO_ALG_TYPE_RNG,
            CRYPTO_ALG_TYPE_AKCIPHER,
            CRYPTO_ALG_TYPE_KDF,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            CRYPTO_ALG_ASYNC,
            CRYPTO_ALG_NEED_FALLBACK,
            CRYPTO_ALG_INTERNAL,
            CRYPTO_ALG_OPTIONAL_KEY,
            CRYPTO_ALG_TESTED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
