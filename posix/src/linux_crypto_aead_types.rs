//! `<crypto/aead.h>` — AEAD (Authenticated Encryption with Associated Data) constants.
//!
//! AEAD ciphers provide both confidentiality and integrity in a single
//! operation. They encrypt the plaintext and produce an authentication
//! tag that covers both the ciphertext and optional associated data
//! (headers/metadata that must be integrity-protected but not encrypted).
//! AEAD is the recommended mode for all modern encryption: TLS 1.3,
//! WireGuard, IPsec ESP, and fscrypt all use AEAD exclusively.

// ---------------------------------------------------------------------------
// AEAD algorithm identifiers
// ---------------------------------------------------------------------------

/// AES-GCM (Galois/Counter Mode, most widely used).
pub const AEAD_ALG_AES_GCM: u32 = 0;
/// AES-CCM (Counter with CBC-MAC, used in WiFi/Bluetooth).
pub const AEAD_ALG_AES_CCM: u32 = 1;
/// ChaCha20-Poly1305 (software-friendly, used in WireGuard/TLS).
pub const AEAD_ALG_CHACHA20_POLY1305: u32 = 2;
/// AES-GCM-SIV (nonce-misuse resistant).
pub const AEAD_ALG_AES_GCM_SIV: u32 = 3;
/// XChaCha20-Poly1305 (extended nonce, used in filesystem encryption).
pub const AEAD_ALG_XCHACHA20_POLY1305: u32 = 4;
/// AES-SIV (Synthetic Initialization Vector, deterministic).
pub const AEAD_ALG_AES_SIV: u32 = 5;
/// RFC 7539 ChaCha20-Poly1305 (IETF version).
pub const AEAD_ALG_RFC7539: u32 = 6;

// ---------------------------------------------------------------------------
// AEAD parameters
// ---------------------------------------------------------------------------

/// GCM standard nonce size (96 bits).
pub const AEAD_NONCE_GCM: u32 = 12;
/// CCM nonce size range minimum.
pub const AEAD_NONCE_CCM_MIN: u32 = 7;
/// CCM nonce size range maximum.
pub const AEAD_NONCE_CCM_MAX: u32 = 13;
/// ChaCha20-Poly1305 nonce size (96 bits IETF).
pub const AEAD_NONCE_CHACHA: u32 = 12;
/// XChaCha20 extended nonce size (192 bits).
pub const AEAD_NONCE_XCHACHA: u32 = 24;
/// Standard authentication tag size (128 bits).
pub const AEAD_TAG_SIZE_128: u32 = 16;
/// Shortened tag (96 bits, used in some protocols).
pub const AEAD_TAG_SIZE_96: u32 = 12;
/// Maximum AAD (Associated Data) size.
pub const AEAD_MAX_AAD_SIZE: u32 = 65536;

// ---------------------------------------------------------------------------
// AEAD flags
// ---------------------------------------------------------------------------

/// Algorithm supports in-place encryption.
pub const AEAD_FLAG_INPLACE: u32 = 0x01;
/// Algorithm supports scatter-gather lists.
pub const AEAD_FLAG_SG: u32 = 0x02;
/// Algorithm is nonce-misuse resistant.
pub const AEAD_FLAG_NONCE_MISUSE_RESIST: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algorithms_distinct() {
        let algs = [
            AEAD_ALG_AES_GCM, AEAD_ALG_AES_CCM,
            AEAD_ALG_CHACHA20_POLY1305, AEAD_ALG_AES_GCM_SIV,
            AEAD_ALG_XCHACHA20_POLY1305, AEAD_ALG_AES_SIV,
            AEAD_ALG_RFC7539,
        ];
        for i in 0..algs.len() {
            for j in (i + 1)..algs.len() {
                assert_ne!(algs[i], algs[j]);
            }
        }
    }

    #[test]
    fn test_nonce_sizes() {
        assert_eq!(AEAD_NONCE_GCM, 12);
        assert_eq!(AEAD_NONCE_CHACHA, 12);
        assert!(AEAD_NONCE_XCHACHA > AEAD_NONCE_CHACHA);
        assert!(AEAD_NONCE_CCM_MIN < AEAD_NONCE_CCM_MAX);
    }

    #[test]
    fn test_tag_sizes() {
        assert!(AEAD_TAG_SIZE_96 < AEAD_TAG_SIZE_128);
        assert!(AEAD_TAG_SIZE_128 > 0);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            AEAD_FLAG_INPLACE, AEAD_FLAG_SG,
            AEAD_FLAG_NONCE_MISUSE_RESIST,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
