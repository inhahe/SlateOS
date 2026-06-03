//! `<linux/crypto.h>` — Kernel crypto API algorithm type and flag constants.
//!
//! The kernel crypto API organizes algorithms by type (cipher, hash,
//! AEAD, etc.) and flags (async, internal, tested). Drivers register
//! algorithm implementations which the framework selects based on
//! priority and type matching.

// ---------------------------------------------------------------------------
// Algorithm type mask and values
// ---------------------------------------------------------------------------

/// Mask for algorithm type field.
pub const CRYPTO_ALG_TYPE_MASK: u32 = 0x0000_000F;
/// Cipher (block cipher transform).
pub const CRYPTO_ALG_TYPE_CIPHER: u32 = 0x01;
/// Compress (compression algorithm).
pub const CRYPTO_ALG_TYPE_COMPRESS: u32 = 0x02;
/// AEAD (Authenticated Encryption with Associated Data).
pub const CRYPTO_ALG_TYPE_AEAD: u32 = 0x03;
/// Block cipher (multi-block chaining mode).
pub const CRYPTO_ALG_TYPE_BLKCIPHER: u32 = 0x04;
/// Ablkcipher (async block cipher, deprecated).
pub const CRYPTO_ALG_TYPE_ABLKCIPHER: u32 = 0x05;
/// Skcipher (symmetric key cipher, modern).
pub const CRYPTO_ALG_TYPE_SKCIPHER: u32 = 0x05;
/// Givcipher (IV-generating cipher, deprecated).
pub const CRYPTO_ALG_TYPE_GIVCIPHER: u32 = 0x06;
/// KPP (Key-agreement Protocol Primitives).
pub const CRYPTO_ALG_TYPE_KPP: u32 = 0x08;
/// Hash (synchronous message digest).
pub const CRYPTO_ALG_TYPE_HASH: u32 = 0x0E;
/// Shash (synchronous hash).
pub const CRYPTO_ALG_TYPE_SHASH: u32 = 0x0E;
/// Ahash (async hash).
pub const CRYPTO_ALG_TYPE_AHASH: u32 = 0x0F;
/// RNG (random number generator).
pub const CRYPTO_ALG_TYPE_RNG: u32 = 0x0C;
/// Akcipher (asymmetric key cipher).
pub const CRYPTO_ALG_TYPE_AKCIPHER: u32 = 0x0D;

// ---------------------------------------------------------------------------
// Algorithm flags
// ---------------------------------------------------------------------------

/// Algorithm performs all operations asynchronously.
pub const CRYPTO_ALG_ASYNC: u32 = 0x0000_0080;
/// Algorithm needs a fallback for some operations.
pub const CRYPTO_ALG_NEED_FALLBACK: u32 = 0x0000_0100;
/// Algorithm has been tested (self-test passed).
pub const CRYPTO_ALG_TESTED: u32 = 0x0000_0400;
/// Algorithm is internal (not directly user-accessible).
pub const CRYPTO_ALG_INTERNAL: u32 = 0x0000_2000;
/// Algorithm supports optional key.
pub const CRYPTO_ALG_OPTIONAL_KEY: u32 = 0x0000_4000;
/// Algorithm allocations may use GFP_KERNEL.
pub const CRYPTO_ALG_ALLOCATES_MEMORY: u32 = 0x0001_0000;

// ---------------------------------------------------------------------------
// Priority constants
// ---------------------------------------------------------------------------

/// Default software implementation priority.
pub const CRYPTO_PRIORITY_SW_DEFAULT: u32 = 100;
/// Hardware-accelerated implementation priority.
pub const CRYPTO_PRIORITY_HW: u32 = 300;
/// Fallback implementation priority.
pub const CRYPTO_PRIORITY_FALLBACK: u32 = 50;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_values_fit_mask() {
        let types = [
            CRYPTO_ALG_TYPE_CIPHER,
            CRYPTO_ALG_TYPE_COMPRESS,
            CRYPTO_ALG_TYPE_AEAD,
            CRYPTO_ALG_TYPE_BLKCIPHER,
            CRYPTO_ALG_TYPE_KPP,
            CRYPTO_ALG_TYPE_HASH,
            CRYPTO_ALG_TYPE_AHASH,
            CRYPTO_ALG_TYPE_RNG,
            CRYPTO_ALG_TYPE_AKCIPHER,
        ];
        for t in types {
            assert_eq!(t & !CRYPTO_ALG_TYPE_MASK, 0);
        }
    }

    #[test]
    fn test_flags_no_overlap_with_type() {
        let flags = [
            CRYPTO_ALG_ASYNC,
            CRYPTO_ALG_NEED_FALLBACK,
            CRYPTO_ALG_TESTED,
            CRYPTO_ALG_INTERNAL,
            CRYPTO_ALG_OPTIONAL_KEY,
            CRYPTO_ALG_ALLOCATES_MEMORY,
        ];
        for f in flags {
            assert_eq!(f & CRYPTO_ALG_TYPE_MASK, 0);
        }
    }

    #[test]
    fn test_flags_distinct() {
        let flags = [
            CRYPTO_ALG_ASYNC,
            CRYPTO_ALG_NEED_FALLBACK,
            CRYPTO_ALG_TESTED,
            CRYPTO_ALG_INTERNAL,
            CRYPTO_ALG_OPTIONAL_KEY,
            CRYPTO_ALG_ALLOCATES_MEMORY,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_priority_ordering() {
        assert!(CRYPTO_PRIORITY_FALLBACK < CRYPTO_PRIORITY_SW_DEFAULT);
        assert!(CRYPTO_PRIORITY_SW_DEFAULT < CRYPTO_PRIORITY_HW);
    }
}
