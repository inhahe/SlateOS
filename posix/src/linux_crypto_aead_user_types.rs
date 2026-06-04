//! `<crypto/aead.h>` — Authenticated Encryption with Associated Data interface.
//!
//! AEAD combines a cipher with an authenticator (AES-GCM, ChaCha20-Poly1305,
//! AES-CCM, …) in a single algorithm that emits both ciphertext and an auth
//! tag. The userspace AF_ALG layer exposes the same operations to apps.

// ---------------------------------------------------------------------------
// AF_ALG socket family + AEAD type name
// ---------------------------------------------------------------------------

pub const AF_ALG: u32 = 38;
pub const ALG_TYPE_AEAD: &str = "aead";

// ---------------------------------------------------------------------------
// Common AEAD algorithm names (struct sockaddr_alg::salg_name)
// ---------------------------------------------------------------------------

pub const AEAD_NAME_GCM_AES: &str = "gcm(aes)";
pub const AEAD_NAME_GCM_AES_RFC4106: &str = "rfc4106(gcm(aes))";
pub const AEAD_NAME_CCM_AES: &str = "ccm(aes)";
pub const AEAD_NAME_CCM_AES_RFC4309: &str = "rfc4309(ccm(aes))";
pub const AEAD_NAME_CHACHA20_POLY1305: &str = "chacha20poly1305";
pub const AEAD_NAME_RFC7539_CHACHA: &str = "rfc7539(chacha20,poly1305)";

// ---------------------------------------------------------------------------
// AES-GCM parameters (RFC 5116 §5.1)
// ---------------------------------------------------------------------------

pub const GCM_AES_BLOCK_SIZE: usize = 16;
pub const GCM_AES_IV_SIZE: usize = 12;
pub const GCM_AES_TAG_SIZE: usize = 16;
pub const GCM_AES_AAD_MAX: u64 = (1 << 61) - 1; // 2^61 - 1 bytes
pub const GCM_AES_PLAINTEXT_MAX: u64 = (1u64 << 36) - 32;

// ---------------------------------------------------------------------------
// AES-CCM parameters
// ---------------------------------------------------------------------------

pub const CCM_AES_BLOCK_SIZE: usize = 16;
pub const CCM_AES_IV_SIZE_MIN: usize = 7;
pub const CCM_AES_IV_SIZE_MAX: usize = 13;
pub const CCM_AES_TAG_SIZE_MIN: usize = 4;
pub const CCM_AES_TAG_SIZE_MAX: usize = 16;

// ---------------------------------------------------------------------------
// ChaCha20-Poly1305 parameters (RFC 8439)
// ---------------------------------------------------------------------------

pub const CHACHA_KEY_SIZE: usize = 32;
pub const CHACHA_NONCE_SIZE: usize = 12;
pub const POLY1305_TAG_SIZE: usize = 16;
pub const CHACHA_BLOCK_SIZE: usize = 64;

// ---------------------------------------------------------------------------
// cmsg level/type for the AF_ALG AEAD layer
// ---------------------------------------------------------------------------

pub const SOL_ALG: u32 = 279;
pub const ALG_SET_KEY: u32 = 1;
pub const ALG_SET_IV: u32 = 2;
pub const ALG_SET_OP: u32 = 3;
pub const ALG_SET_AEAD_ASSOCLEN: u32 = 4;
pub const ALG_SET_AEAD_AUTHSIZE: u32 = 5;
pub const ALG_OP_DECRYPT: u32 = 0;
pub const ALG_OP_ENCRYPT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_alg_is_38() {
        assert_eq!(AF_ALG, 38);
        assert_eq!(ALG_TYPE_AEAD, "aead");
    }

    #[test]
    fn test_aead_names_distinct() {
        let n = [
            AEAD_NAME_GCM_AES,
            AEAD_NAME_GCM_AES_RFC4106,
            AEAD_NAME_CCM_AES,
            AEAD_NAME_CCM_AES_RFC4309,
            AEAD_NAME_CHACHA20_POLY1305,
            AEAD_NAME_RFC7539_CHACHA,
        ];
        for (i, &x) in n.iter().enumerate() {
            for &y in &n[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_gcm_aes_parameters() {
        assert_eq!(GCM_AES_BLOCK_SIZE, 16);
        assert_eq!(GCM_AES_IV_SIZE, 12);
        assert_eq!(GCM_AES_TAG_SIZE, 16);
        // GCM plaintext cap < AAD cap.
        assert!(GCM_AES_PLAINTEXT_MAX < GCM_AES_AAD_MAX);
    }

    #[test]
    fn test_ccm_iv_range() {
        assert!(CCM_AES_IV_SIZE_MIN <= CCM_AES_IV_SIZE_MAX);
        assert_eq!(CCM_AES_IV_SIZE_MIN, 7);
        assert_eq!(CCM_AES_IV_SIZE_MAX, 13);
    }

    #[test]
    fn test_ccm_tag_range_even() {
        assert!(CCM_AES_TAG_SIZE_MIN <= CCM_AES_TAG_SIZE_MAX);
        // CCM tag sizes are always even.
        for t in [CCM_AES_TAG_SIZE_MIN, CCM_AES_TAG_SIZE_MAX] {
            assert_eq!(t % 2, 0);
        }
    }

    #[test]
    fn test_chacha_poly_parameters() {
        assert_eq!(CHACHA_KEY_SIZE, 32);
        assert_eq!(CHACHA_NONCE_SIZE, 12);
        assert_eq!(POLY1305_TAG_SIZE, 16);
        assert_eq!(CHACHA_BLOCK_SIZE, 64);
        assert!(CHACHA_KEY_SIZE < CHACHA_BLOCK_SIZE);
    }

    #[test]
    fn test_sol_alg_and_alg_set_codes_distinct() {
        let c = [
            ALG_SET_KEY,
            ALG_SET_IV,
            ALG_SET_OP,
            ALG_SET_AEAD_ASSOCLEN,
            ALG_SET_AEAD_AUTHSIZE,
        ];
        for (i, &x) in c.iter().enumerate() {
            for &y in &c[i + 1..] {
                assert_ne!(x, y);
            }
        }
        assert_eq!(SOL_ALG, 279);
    }

    #[test]
    fn test_alg_op_codes_binary() {
        assert_eq!(ALG_OP_DECRYPT, 0);
        assert_eq!(ALG_OP_ENCRYPT, 1);
    }
}
