//! `<crypto/skcipher.h>` — Symmetric cipher algorithm constants.
//!
//! Symmetric ciphers encrypt/decrypt data using the same key. The
//! kernel uses them for disk encryption (dm-crypt, fscrypt), network
//! encryption (IPsec ESP, WireGuard), and encrypted filesystems. Block
//! ciphers (AES) operate on fixed-size blocks; stream ciphers (ChaCha20)
//! encrypt byte-by-byte. Modes of operation (CBC, CTR, XTS) define
//! how block ciphers handle data larger than one block.

// ---------------------------------------------------------------------------
// Cipher algorithms
// ---------------------------------------------------------------------------

/// AES (Advanced Encryption Standard, 128/192/256-bit key).
pub const CIPHER_ALG_AES: u32 = 0;
/// ChaCha20 (256-bit key, stream cipher).
pub const CIPHER_ALG_CHACHA20: u32 = 1;
/// Camellia (128/192/256-bit key).
pub const CIPHER_ALG_CAMELLIA: u32 = 2;
/// Serpent (128/192/256-bit key).
pub const CIPHER_ALG_SERPENT: u32 = 3;
/// Twofish (128/192/256-bit key).
pub const CIPHER_ALG_TWOFISH: u32 = 4;
/// SM4 (Chinese national standard, 128-bit key).
pub const CIPHER_ALG_SM4: u32 = 5;
/// ARIA (Korean standard, 128/192/256-bit key).
pub const CIPHER_ALG_ARIA: u32 = 6;
/// DES (legacy, 56-bit key, insecure).
pub const CIPHER_ALG_DES: u32 = 7;
/// 3DES / Triple-DES (168-bit effective key, legacy).
pub const CIPHER_ALG_DES3: u32 = 8;

// ---------------------------------------------------------------------------
// Cipher modes of operation
// ---------------------------------------------------------------------------

/// ECB (Electronic Codebook, no chaining — insecure for most uses).
pub const CIPHER_MODE_ECB: u32 = 0;
/// CBC (Cipher Block Chaining).
pub const CIPHER_MODE_CBC: u32 = 1;
/// CTR (Counter mode, turns block cipher into stream cipher).
pub const CIPHER_MODE_CTR: u32 = 2;
/// XTS (XEX-based Tweaked codebook with Stealing, for disk encryption).
pub const CIPHER_MODE_XTS: u32 = 3;
/// CTS (Ciphertext Stealing, for non-block-aligned data).
pub const CIPHER_MODE_CTS: u32 = 4;
/// OFB (Output Feedback).
pub const CIPHER_MODE_OFB: u32 = 5;
/// CFB (Cipher Feedback).
pub const CIPHER_MODE_CFB: u32 = 6;

// ---------------------------------------------------------------------------
// Key sizes (bits)
// ---------------------------------------------------------------------------

/// AES-128 key size.
pub const CIPHER_KEY_AES128: u32 = 128;
/// AES-192 key size.
pub const CIPHER_KEY_AES192: u32 = 192;
/// AES-256 key size.
pub const CIPHER_KEY_AES256: u32 = 256;
/// ChaCha20 key size.
pub const CIPHER_KEY_CHACHA20: u32 = 256;

// ---------------------------------------------------------------------------
// Block sizes (bytes)
// ---------------------------------------------------------------------------

/// AES block size.
pub const CIPHER_BLOCK_AES: u32 = 16;
/// DES block size.
pub const CIPHER_BLOCK_DES: u32 = 8;
/// ChaCha20 block size (conceptual, it's a stream cipher).
pub const CIPHER_BLOCK_CHACHA20: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algorithms_distinct() {
        let algs = [
            CIPHER_ALG_AES, CIPHER_ALG_CHACHA20, CIPHER_ALG_CAMELLIA,
            CIPHER_ALG_SERPENT, CIPHER_ALG_TWOFISH, CIPHER_ALG_SM4,
            CIPHER_ALG_ARIA, CIPHER_ALG_DES, CIPHER_ALG_DES3,
        ];
        for i in 0..algs.len() {
            for j in (i + 1)..algs.len() {
                assert_ne!(algs[i], algs[j]);
            }
        }
    }

    #[test]
    fn test_modes_distinct() {
        let modes = [
            CIPHER_MODE_ECB, CIPHER_MODE_CBC, CIPHER_MODE_CTR,
            CIPHER_MODE_XTS, CIPHER_MODE_CTS, CIPHER_MODE_OFB,
            CIPHER_MODE_CFB,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_key_sizes() {
        assert!(CIPHER_KEY_AES128 < CIPHER_KEY_AES192);
        assert!(CIPHER_KEY_AES192 < CIPHER_KEY_AES256);
    }

    #[test]
    fn test_block_sizes_positive() {
        assert!(CIPHER_BLOCK_AES > 0);
        assert!(CIPHER_BLOCK_DES > 0);
        assert!(CIPHER_BLOCK_CHACHA20 > 0);
    }
}
