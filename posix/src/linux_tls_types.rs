//! `<linux/tls.h>` — Kernel TLS (kTLS) constants.
//!
//! Kernel TLS offloads TLS record-layer encryption/decryption to the
//! kernel (or NIC hardware). After userspace completes the TLS handshake,
//! it installs the session keys via setsockopt(SOL_TLS), and subsequent
//! send/recv are transparently encrypted/decrypted. This enables
//! sendfile() over TLS and hardware offload.

// ---------------------------------------------------------------------------
// TLS versions
// ---------------------------------------------------------------------------

/// TLS 1.2.
pub const TLS_1_2_VERSION: u16 = 0x0303;
/// TLS 1.3.
pub const TLS_1_3_VERSION: u16 = 0x0304;

// ---------------------------------------------------------------------------
// Cipher types
// ---------------------------------------------------------------------------

/// AES-128-GCM.
pub const TLS_CIPHER_AES_GCM_128: u16 = 51;
/// AES-256-GCM.
pub const TLS_CIPHER_AES_GCM_256: u16 = 52;
/// ChaCha20-Poly1305.
pub const TLS_CIPHER_CHACHA20_POLY1305: u16 = 54;
/// AES-128-CCM.
pub const TLS_CIPHER_AES_CCM_128: u16 = 53;
/// SM4-GCM (Chinese national cipher).
pub const TLS_CIPHER_SM4_GCM: u16 = 55;
/// SM4-CCM.
pub const TLS_CIPHER_SM4_CCM: u16 = 56;

// ---------------------------------------------------------------------------
// Socket option levels and options
// ---------------------------------------------------------------------------

/// SOL_TLS socket option level.
pub const SOL_TLS: u32 = 282;
/// Set transmit crypto info.
pub const TLS_TX: u32 = 1;
/// Set receive crypto info.
pub const TLS_RX: u32 = 2;
/// Set transmit zerocopy mode.
pub const TLS_TX_ZEROCOPY_RO: u32 = 3;
/// Expect no padding in received records.
pub const TLS_RX_EXPECT_NO_PAD: u32 = 4;

// ---------------------------------------------------------------------------
// Crypto info sizes
// ---------------------------------------------------------------------------

/// AES-128-GCM crypto info size.
pub const TLS_CIPHER_AES_GCM_128_IV_SIZE: u32 = 8;
/// AES-128-GCM key size.
pub const TLS_CIPHER_AES_GCM_128_KEY_SIZE: u32 = 16;
/// AES-128-GCM salt size.
pub const TLS_CIPHER_AES_GCM_128_SALT_SIZE: u32 = 4;
/// AES-128-GCM tag size.
pub const TLS_CIPHER_AES_GCM_128_TAG_SIZE: u32 = 16;
/// AES-256-GCM key size.
pub const TLS_CIPHER_AES_GCM_256_KEY_SIZE: u32 = 32;
/// ChaCha20-Poly1305 key size.
pub const TLS_CIPHER_CHACHA20_POLY1305_KEY_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_versions_distinct() {
        assert_ne!(TLS_1_2_VERSION, TLS_1_3_VERSION);
        assert!(TLS_1_2_VERSION < TLS_1_3_VERSION);
    }

    #[test]
    fn test_cipher_types_distinct() {
        let ciphers = [
            TLS_CIPHER_AES_GCM_128, TLS_CIPHER_AES_GCM_256,
            TLS_CIPHER_AES_CCM_128, TLS_CIPHER_CHACHA20_POLY1305,
            TLS_CIPHER_SM4_GCM, TLS_CIPHER_SM4_CCM,
        ];
        for i in 0..ciphers.len() {
            for j in (i + 1)..ciphers.len() {
                assert_ne!(ciphers[i], ciphers[j]);
            }
        }
    }

    #[test]
    fn test_directions_distinct() {
        assert_ne!(TLS_TX, TLS_RX);
    }

    #[test]
    fn test_key_sizes() {
        assert_eq!(TLS_CIPHER_AES_GCM_128_KEY_SIZE, 16);
        assert_eq!(TLS_CIPHER_AES_GCM_256_KEY_SIZE, 32);
        assert_eq!(TLS_CIPHER_CHACHA20_POLY1305_KEY_SIZE, 32);
    }
}
