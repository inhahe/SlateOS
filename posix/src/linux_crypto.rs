//! `<linux/if_alg.h>` + `<linux/cryptouser.h>` — Kernel crypto API constants.
//!
//! The Linux kernel crypto API (AF_ALG) exposes hardware and software
//! crypto engines to userspace via sockets. Used by OpenSSL's afalg
//! engine, cryptsetup, and dm-crypt.

// ---------------------------------------------------------------------------
// AF_ALG socket option levels
// ---------------------------------------------------------------------------

/// AF_ALG socket level.
pub const SOL_ALG: i32 = 279;

// ---------------------------------------------------------------------------
// ALG operations (setsockopt names)
// ---------------------------------------------------------------------------

/// Set algorithm key.
pub const ALG_SET_KEY: u32 = 1;
/// Set IV.
pub const ALG_SET_IV: u32 = 2;
/// Set operation type (encrypt/decrypt).
pub const ALG_SET_OP: u32 = 3;
/// Set AEAD authentication tag size.
pub const ALG_SET_AEAD_ASSOCLEN: u32 = 4;
/// Set AEAD authentication tag size.
pub const ALG_SET_AEAD_AUTHSIZE: u32 = 5;
/// Set DRBGs additional data.
pub const ALG_SET_DRBG_ENTROPY: u32 = 6;

// ---------------------------------------------------------------------------
// Operation types
// ---------------------------------------------------------------------------

/// Encrypt operation.
pub const ALG_OP_DECRYPT: u32 = 0;
/// Decrypt operation.
pub const ALG_OP_ENCRYPT: u32 = 1;

// ---------------------------------------------------------------------------
// Crypto algorithm types (CRYPTO_ALG_TYPE_*)
// ---------------------------------------------------------------------------

/// Cipher (single block).
pub const CRYPTO_ALG_TYPE_CIPHER: u32 = 0x00000001;
/// Compression.
pub const CRYPTO_ALG_TYPE_COMPRESS: u32 = 0x00000002;
/// AEAD (authenticated encryption).
pub const CRYPTO_ALG_TYPE_AEAD: u32 = 0x00000003;
/// Block cipher.
pub const CRYPTO_ALG_TYPE_BLKCIPHER: u32 = 0x00000004;
/// Ablkcipher (async block cipher).
pub const CRYPTO_ALG_TYPE_ABLKCIPHER: u32 = 0x00000005;
/// Skcipher (symmetric key cipher).
pub const CRYPTO_ALG_TYPE_SKCIPHER: u32 = 0x00000005;
/// Givcipher (IV generation cipher).
pub const CRYPTO_ALG_TYPE_GIVCIPHER: u32 = 0x00000006;
/// Key agreement (KPP).
pub const CRYPTO_ALG_TYPE_KPP: u32 = 0x00000008;
/// ACOMP (async compression).
pub const CRYPTO_ALG_TYPE_ACOMPRESS: u32 = 0x0000000a;
/// SCOMP (sync compression).
pub const CRYPTO_ALG_TYPE_SCOMPRESS: u32 = 0x0000000b;
/// RNG.
pub const CRYPTO_ALG_TYPE_RNG: u32 = 0x0000000c;
/// Akcipher (asymmetric key cipher).
pub const CRYPTO_ALG_TYPE_AKCIPHER: u32 = 0x0000000d;
/// Hash.
pub const CRYPTO_ALG_TYPE_HASH: u32 = 0x0000000e;
/// SHASH (synchronous hash).
pub const CRYPTO_ALG_TYPE_SHASH: u32 = 0x0000000e;
/// AHASH (async hash).
pub const CRYPTO_ALG_TYPE_AHASH: u32 = 0x0000000f;

/// Type mask.
pub const CRYPTO_ALG_TYPE_MASK: u32 = 0x0000000f;

// ---------------------------------------------------------------------------
// Algorithm flags
// ---------------------------------------------------------------------------

/// Needs key.
pub const CRYPTO_ALG_NEED_FALLBACK: u32 = 0x00000010;
/// Tested.
pub const CRYPTO_ALG_TESTED: u32 = 0x00000400;
/// Internal (not user-accessible).
pub const CRYPTO_ALG_INTERNAL: u32 = 0x00002000;
/// Optional key.
pub const CRYPTO_ALG_OPTIONAL_KEY: u32 = 0x00004000;
/// Allocate in high memory.
pub const CRYPTO_ALG_ALLOCATES_MEMORY: u32 = 0x00010000;

// ---------------------------------------------------------------------------
// Crypto user config commands (CRYPTO_MSG_*)
// ---------------------------------------------------------------------------

/// Base command number.
pub const CRYPTO_MSG_BASE: u8 = 0x10;
/// New algorithm.
pub const CRYPTO_MSG_NEWALG: u8 = 0x10;
/// Delete algorithm.
pub const CRYPTO_MSG_DELALG: u8 = 0x11;
/// Update algorithm.
pub const CRYPTO_MSG_UPDATEALG: u8 = 0x12;
/// Get algorithm.
pub const CRYPTO_MSG_GETALG: u8 = 0x13;
/// Delete RNG.
pub const CRYPTO_MSG_DELRNG: u8 = 0x14;
/// Get statistics.
pub const CRYPTO_MSG_GETSTAT: u8 = 0x15;

// ---------------------------------------------------------------------------
// Maximum name lengths
// ---------------------------------------------------------------------------

/// Maximum algorithm name length.
pub const CRYPTO_MAX_ALG_NAME: usize = 128;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alg_set_distinct() {
        let sets = [
            ALG_SET_KEY,
            ALG_SET_IV,
            ALG_SET_OP,
            ALG_SET_AEAD_ASSOCLEN,
            ALG_SET_AEAD_AUTHSIZE,
            ALG_SET_DRBG_ENTROPY,
        ];
        for i in 0..sets.len() {
            for j in (i + 1)..sets.len() {
                assert_ne!(sets[i], sets[j]);
            }
        }
    }

    #[test]
    fn test_op_types() {
        assert_eq!(ALG_OP_DECRYPT, 0);
        assert_eq!(ALG_OP_ENCRYPT, 1);
    }

    #[test]
    fn test_msg_cmds_distinct() {
        let cmds = [
            CRYPTO_MSG_NEWALG,
            CRYPTO_MSG_DELALG,
            CRYPTO_MSG_UPDATEALG,
            CRYPTO_MSG_GETALG,
            CRYPTO_MSG_DELRNG,
            CRYPTO_MSG_GETSTAT,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_alg_flags_no_overlap() {
        // Non-type flags should not overlap with the type mask.
        assert_eq!(CRYPTO_ALG_NEED_FALLBACK & CRYPTO_ALG_TYPE_MASK, 0);
        assert_eq!(CRYPTO_ALG_TESTED & CRYPTO_ALG_TYPE_MASK, 0);
        assert_eq!(CRYPTO_ALG_INTERNAL & CRYPTO_ALG_TYPE_MASK, 0);
        assert_eq!(CRYPTO_ALG_OPTIONAL_KEY & CRYPTO_ALG_TYPE_MASK, 0);
    }

    #[test]
    fn test_sol_alg() {
        assert_eq!(SOL_ALG, 279);
    }

    #[test]
    fn test_crypto_max_name() {
        assert_eq!(CRYPTO_MAX_ALG_NAME, 128);
    }

    #[test]
    fn test_type_mask_extracts_type() {
        assert_eq!(
            CRYPTO_ALG_TYPE_CIPHER & CRYPTO_ALG_TYPE_MASK,
            CRYPTO_ALG_TYPE_CIPHER
        );
        assert_eq!(
            CRYPTO_ALG_TYPE_AEAD & CRYPTO_ALG_TYPE_MASK,
            CRYPTO_ALG_TYPE_AEAD
        );
        assert_eq!(
            CRYPTO_ALG_TYPE_HASH & CRYPTO_ALG_TYPE_MASK,
            CRYPTO_ALG_TYPE_HASH
        );
    }
}
