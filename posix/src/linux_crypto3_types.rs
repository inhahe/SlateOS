//! `<linux/if_alg.h>` — Additional crypto API constants (batch 3).
//!
//! Supplementary crypto constants covering algorithm types,
//! AEAD flags, and hash algorithm IDs.

// ---------------------------------------------------------------------------
// Crypto algorithm types (CRYPTO_ALG_TYPE_*)
// ---------------------------------------------------------------------------

/// Cipher algorithm.
pub const CRYPTO_ALG_TYPE_CIPHER: u32 = 0x00000001;
/// Compress algorithm.
pub const CRYPTO_ALG_TYPE_COMPRESS: u32 = 0x00000002;
/// AEAD algorithm.
pub const CRYPTO_ALG_TYPE_AEAD: u32 = 0x00000003;
/// Block cipher (skcipher).
pub const CRYPTO_ALG_TYPE_SKCIPHER: u32 = 0x00000005;
/// Key-derivation function.
pub const CRYPTO_ALG_TYPE_KDF: u32 = 0x00000006;
/// Hash algorithm.
pub const CRYPTO_ALG_TYPE_HASH: u32 = 0x0000000E;
/// Shared hash algorithm.
pub const CRYPTO_ALG_TYPE_SHASH: u32 = 0x0000000E;
/// Async hash algorithm.
pub const CRYPTO_ALG_TYPE_AHASH: u32 = 0x0000000F;
/// RNG algorithm.
pub const CRYPTO_ALG_TYPE_RNG: u32 = 0x0000000C;
/// Asym key agreement.
pub const CRYPTO_ALG_TYPE_AKCIPHER: u32 = 0x0000000D;

// ---------------------------------------------------------------------------
// Crypto algorithm flags
// ---------------------------------------------------------------------------

/// Algorithm requires fallback.
pub const CRYPTO_ALG_NEED_FALLBACK: u32 = 1 << 0;
/// Algorithm is internal.
pub const CRYPTO_ALG_INTERNAL: u32 = 1 << 1;
/// Algorithm is optional (for boot).
pub const CRYPTO_ALG_OPTIONAL_KEY: u32 = 1 << 2;
/// Algorithm is dead (being unregistered).
pub const CRYPTO_ALG_DEAD: u32 = 1 << 3;
/// Algorithm is dying (ref count at zero).
pub const CRYPTO_ALG_DYING: u32 = 1 << 4;
/// Algorithm needs async.
pub const CRYPTO_ALG_ASYNC: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Crypto TFM flags
// ---------------------------------------------------------------------------

/// Transform: request needs fallback.
pub const CRYPTO_TFM_NEED_KEY: u32 = 1 << 0;
/// Transform: request is in-place.
pub const CRYPTO_TFM_REQ_MAY_SLEEP: u32 = 1 << 1;
/// Transform: request may backlog.
pub const CRYPTO_TFM_REQ_MAY_BACKLOG: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alg_types_cover_range() {
        assert_eq!(CRYPTO_ALG_TYPE_CIPHER, 1);
        assert_eq!(CRYPTO_ALG_TYPE_COMPRESS, 2);
        assert_eq!(CRYPTO_ALG_TYPE_AEAD, 3);
        assert_eq!(CRYPTO_ALG_TYPE_SKCIPHER, 5);
    }

    #[test]
    fn test_hash_shash_same() {
        // HASH and SHASH share the same type value
        assert_eq!(CRYPTO_ALG_TYPE_HASH, CRYPTO_ALG_TYPE_SHASH);
    }

    #[test]
    fn test_alg_flags_power_of_two() {
        let flags = [
            CRYPTO_ALG_NEED_FALLBACK, CRYPTO_ALG_INTERNAL,
            CRYPTO_ALG_OPTIONAL_KEY, CRYPTO_ALG_DEAD,
            CRYPTO_ALG_DYING, CRYPTO_ALG_ASYNC,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_alg_flags_no_overlap() {
        let flags = [
            CRYPTO_ALG_NEED_FALLBACK, CRYPTO_ALG_INTERNAL,
            CRYPTO_ALG_OPTIONAL_KEY, CRYPTO_ALG_DEAD,
            CRYPTO_ALG_DYING, CRYPTO_ALG_ASYNC,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_tfm_flags_power_of_two() {
        let flags = [
            CRYPTO_TFM_NEED_KEY, CRYPTO_TFM_REQ_MAY_SLEEP,
            CRYPTO_TFM_REQ_MAY_BACKLOG,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_tfm_flags_no_overlap() {
        let flags = [
            CRYPTO_TFM_NEED_KEY, CRYPTO_TFM_REQ_MAY_SLEEP,
            CRYPTO_TFM_REQ_MAY_BACKLOG,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
