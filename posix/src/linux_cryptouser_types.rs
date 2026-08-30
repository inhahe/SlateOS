//! `<linux/cryptouser.h>` — Crypto API userspace interface constants.
//!
//! The Linux crypto user API allows userspace to query and configure
//! the kernel's cryptographic subsystem via netlink. Applications can
//! enumerate available algorithms, check their properties, and add
//! or remove algorithm implementations. Used by cryptsetup, IPsec
//! configuration tools, and kernel crypto testing utilities.

// ---------------------------------------------------------------------------
// Crypto netlink message types
// ---------------------------------------------------------------------------

/// Get algorithm information.
pub const CRYPTO_MSG_GETALG: u32 = 0x13;
/// Delete an algorithm.
pub const CRYPTO_MSG_DELALG: u32 = 0x11;
/// Update algorithm priority.
pub const CRYPTO_MSG_UPDATEALG: u32 = 0x14;
/// New algorithm notification.
pub const CRYPTO_MSG_NEWALG: u32 = 0x10;
/// Delete RNG instance.
pub const CRYPTO_MSG_DELRNG: u32 = 0x15;
/// Get RNG status.
pub const CRYPTO_MSG_GETSTAT: u32 = 0x16;

// ---------------------------------------------------------------------------
// Algorithm types
// ---------------------------------------------------------------------------

/// Cipher algorithm (block cipher).
pub const CRYPTO_ALG_TYPE_CIPHER: u32 = 0x0000_0001;
/// Compression algorithm.
pub const CRYPTO_ALG_TYPE_COMPRESS: u32 = 0x0000_0002;
/// AEAD (Authenticated Encryption with Associated Data).
pub const CRYPTO_ALG_TYPE_AEAD: u32 = 0x0000_0003;
/// Block cipher (skcipher).
pub const CRYPTO_ALG_TYPE_SKCIPHER: u32 = 0x0000_0005;
/// Hash/digest algorithm.
pub const CRYPTO_ALG_TYPE_HASH: u32 = 0x0000_000E;
/// Shared hash (same as HASH).
pub const CRYPTO_ALG_TYPE_SHASH: u32 = 0x0000_000E;
/// Async hash.
pub const CRYPTO_ALG_TYPE_AHASH: u32 = 0x0000_000F;
/// Random number generator.
pub const CRYPTO_ALG_TYPE_RNG: u32 = 0x0000_000C;
/// Key agreement (KPP).
pub const CRYPTO_ALG_TYPE_KPP: u32 = 0x0000_0008;
/// Asymmetric (public key) algorithm.
pub const CRYPTO_ALG_TYPE_AKCIPHER: u32 = 0x0000_000D;
/// Type mask.
pub const CRYPTO_ALG_TYPE_MASK: u32 = 0x0000_000F;

// ---------------------------------------------------------------------------
// Algorithm flags
// ---------------------------------------------------------------------------

/// Algorithm is being tested (may not be usable).
pub const CRYPTO_ALG_TESTED: u32 = 0x0000_0400;
/// Algorithm needs a key to operate.
pub const CRYPTO_ALG_NEED_FALLBACK: u32 = 0x0000_0004;
/// Internal algorithm (not for direct user access).
pub const CRYPTO_ALG_INTERNAL: u32 = 0x0000_2000;
/// Algorithm is async.
pub const CRYPTO_ALG_ASYNC: u32 = 0x0000_0080;

// ---------------------------------------------------------------------------
// Maximum sizes
// ---------------------------------------------------------------------------

/// Maximum algorithm name length.
pub const CRYPTO_MAX_ALG_NAME: u32 = 128;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_distinct() {
        let msgs = [
            CRYPTO_MSG_NEWALG,
            CRYPTO_MSG_DELALG,
            CRYPTO_MSG_GETALG,
            CRYPTO_MSG_UPDATEALG,
            CRYPTO_MSG_DELRNG,
            CRYPTO_MSG_GETSTAT,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_alg_types_within_mask() {
        let types = [
            CRYPTO_ALG_TYPE_CIPHER,
            CRYPTO_ALG_TYPE_COMPRESS,
            CRYPTO_ALG_TYPE_AEAD,
            CRYPTO_ALG_TYPE_SKCIPHER,
            CRYPTO_ALG_TYPE_KPP,
            CRYPTO_ALG_TYPE_RNG,
            CRYPTO_ALG_TYPE_AKCIPHER,
            CRYPTO_ALG_TYPE_HASH,
            CRYPTO_ALG_TYPE_AHASH,
        ];
        for t in types {
            assert_eq!(t & !CRYPTO_ALG_TYPE_MASK, 0);
        }
    }

    #[test]
    fn test_max_alg_name() {
        assert_eq!(CRYPTO_MAX_ALG_NAME, 128);
    }

    #[test]
    fn test_flags_distinct() {
        let flags = [
            CRYPTO_ALG_NEED_FALLBACK,
            CRYPTO_ALG_ASYNC,
            CRYPTO_ALG_TESTED,
            CRYPTO_ALG_INTERNAL,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
