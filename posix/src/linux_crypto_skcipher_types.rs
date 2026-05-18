//! `<crypto/skcipher.h>` — Symmetric key cipher (skcipher) constants.
//!
//! The skcipher API handles symmetric encryption modes (AES-CBC,
//! AES-CTR, AES-XTS, ChaCha20, etc.). It replaces the older
//! blkcipher/ablkcipher APIs with a unified async interface for
//! both single-block and multi-block operations.

// ---------------------------------------------------------------------------
// Skcipher request flags
// ---------------------------------------------------------------------------

/// Request may sleep (not in atomic context).
pub const CRYPTO_SKCIPHER_REQ_MAY_SLEEP: u32 = 1 << 0;
/// Request may use hardware fallback.
pub const CRYPTO_SKCIPHER_REQ_MAY_BACKLOG: u32 = 1 << 1;
/// Request needs IV output (for chaining).
pub const CRYPTO_SKCIPHER_REQ_IV_OUTPUT: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Common symmetric cipher modes
// ---------------------------------------------------------------------------

/// ECB mode (Electronic Codebook — no IV, parallel blocks).
pub const CRYPTO_MODE_ECB: u32 = 0;
/// CBC mode (Cipher Block Chaining — IV, sequential).
pub const CRYPTO_MODE_CBC: u32 = 1;
/// CTR mode (Counter — IV as nonce+counter, parallel).
pub const CRYPTO_MODE_CTR: u32 = 2;
/// XTS mode (XEX Tweakable Block Cipher with Ciphertext Stealing).
pub const CRYPTO_MODE_XTS: u32 = 3;
/// CTS mode (Ciphertext Stealing on top of CBC).
pub const CRYPTO_MODE_CTS: u32 = 4;
/// OFB mode (Output Feedback — stream cipher from block cipher).
pub const CRYPTO_MODE_OFB: u32 = 5;
/// CFB mode (Cipher Feedback — stream cipher).
pub const CRYPTO_MODE_CFB: u32 = 6;

// ---------------------------------------------------------------------------
// Common key sizes (bytes)
// ---------------------------------------------------------------------------

/// AES-128 key size.
pub const CRYPTO_AES_128_KEY_SIZE: u32 = 16;
/// AES-192 key size.
pub const CRYPTO_AES_192_KEY_SIZE: u32 = 24;
/// AES-256 key size.
pub const CRYPTO_AES_256_KEY_SIZE: u32 = 32;
/// ChaCha20 key size.
pub const CRYPTO_CHACHA20_KEY_SIZE: u32 = 32;
/// 3DES key size (3 × 8 bytes).
pub const CRYPTO_3DES_KEY_SIZE: u32 = 24;

// ---------------------------------------------------------------------------
// Common IV sizes (bytes)
// ---------------------------------------------------------------------------

/// AES block/IV size.
pub const CRYPTO_AES_IV_SIZE: u32 = 16;
/// ChaCha20 IV/nonce size.
pub const CRYPTO_CHACHA20_IV_SIZE: u32 = 16;
/// 3DES IV size.
pub const CRYPTO_3DES_IV_SIZE: u32 = 8;

// ---------------------------------------------------------------------------
// Block sizes (bytes)
// ---------------------------------------------------------------------------

/// AES block size.
pub const CRYPTO_AES_BLOCK_SIZE: u32 = 16;
/// 3DES block size.
pub const CRYPTO_3DES_BLOCK_SIZE: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_req_flags_no_overlap() {
        let flags = [
            CRYPTO_SKCIPHER_REQ_MAY_SLEEP,
            CRYPTO_SKCIPHER_REQ_MAY_BACKLOG,
            CRYPTO_SKCIPHER_REQ_IV_OUTPUT,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_modes_distinct() {
        let modes = [
            CRYPTO_MODE_ECB, CRYPTO_MODE_CBC, CRYPTO_MODE_CTR,
            CRYPTO_MODE_XTS, CRYPTO_MODE_CTS, CRYPTO_MODE_OFB,
            CRYPTO_MODE_CFB,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_aes_key_sizes() {
        assert_eq!(CRYPTO_AES_128_KEY_SIZE, 16);
        assert_eq!(CRYPTO_AES_192_KEY_SIZE, 24);
        assert_eq!(CRYPTO_AES_256_KEY_SIZE, 32);
    }

    #[test]
    fn test_aes_block_iv_match() {
        assert_eq!(CRYPTO_AES_BLOCK_SIZE, CRYPTO_AES_IV_SIZE);
    }

    #[test]
    fn test_3des_sizes() {
        assert_eq!(CRYPTO_3DES_KEY_SIZE, 24);
        assert_eq!(CRYPTO_3DES_BLOCK_SIZE, 8);
        assert_eq!(CRYPTO_3DES_IV_SIZE, 8);
    }
}
