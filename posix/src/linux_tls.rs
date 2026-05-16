//! `<linux/tls.h>` — Kernel TLS (kTLS) offload constants.
//!
//! Kernel TLS offloads TLS encryption/decryption to the kernel or NIC,
//! allowing sendfile() and splice() to work with TLS sockets without
//! copying data through userspace. Used by nginx, HAProxy, and other
//! high-performance servers.

// ---------------------------------------------------------------------------
// TLS versions
// ---------------------------------------------------------------------------

/// TLS 1.0.
pub const TLS_1_0_VERSION: u16 = 0x0301;
/// TLS 1.1.
pub const TLS_1_1_VERSION: u16 = 0x0302;
/// TLS 1.2.
pub const TLS_1_2_VERSION: u16 = 0x0303;
/// TLS 1.3.
pub const TLS_1_3_VERSION: u16 = 0x0304;

// ---------------------------------------------------------------------------
// Socket option level and names
// ---------------------------------------------------------------------------

/// SOL_TLS level for setsockopt.
pub const SOL_TLS: i32 = 282;

/// Set TX crypto info.
pub const TLS_TX: i32 = 1;
/// Set RX crypto info.
pub const TLS_RX: i32 = 2;
/// Get TX zeroes sent.
pub const TLS_TX_ZEROCOPY_RO: i32 = 3;
/// Expect no pending data.
pub const TLS_RX_EXPECT_NO_PAD: i32 = 4;

// ---------------------------------------------------------------------------
// Cipher types
// ---------------------------------------------------------------------------

/// AES-128-GCM.
pub const TLS_CIPHER_AES_GCM_128: u16 = 51;
/// AES-256-GCM.
pub const TLS_CIPHER_AES_GCM_256: u16 = 52;
/// AES-128-CCM.
pub const TLS_CIPHER_AES_CCM_128: u16 = 53;
/// ChaCha20-Poly1305.
pub const TLS_CIPHER_CHACHA20_POLY1305: u16 = 54;
/// SM4-GCM.
pub const TLS_CIPHER_SM4_GCM: u16 = 55;
/// SM4-CCM.
pub const TLS_CIPHER_SM4_CCM: u16 = 56;

// ---------------------------------------------------------------------------
// Key sizes
// ---------------------------------------------------------------------------

/// AES-128-GCM key size.
pub const TLS_CIPHER_AES_GCM_128_KEY_SIZE: usize = 16;
/// AES-128-GCM IV size.
pub const TLS_CIPHER_AES_GCM_128_IV_SIZE: usize = 8;
/// AES-128-GCM salt size.
pub const TLS_CIPHER_AES_GCM_128_SALT_SIZE: usize = 4;
/// AES-128-GCM tag size.
pub const TLS_CIPHER_AES_GCM_128_TAG_SIZE: usize = 16;
/// AES-128-GCM record sequence size.
pub const TLS_CIPHER_AES_GCM_128_REC_SEQ_SIZE: usize = 8;

/// AES-256-GCM key size.
pub const TLS_CIPHER_AES_GCM_256_KEY_SIZE: usize = 32;
/// AES-256-GCM IV size.
pub const TLS_CIPHER_AES_GCM_256_IV_SIZE: usize = 8;

/// ChaCha20-Poly1305 key size.
pub const TLS_CIPHER_CHACHA20_POLY1305_KEY_SIZE: usize = 32;
/// ChaCha20-Poly1305 IV size.
pub const TLS_CIPHER_CHACHA20_POLY1305_IV_SIZE: usize = 12;

// ---------------------------------------------------------------------------
// Content types (record type)
// ---------------------------------------------------------------------------

/// Change cipher spec.
pub const TLS_CONTENT_TYPE_CHANGE_CIPHER_SPEC: u8 = 20;
/// Alert.
pub const TLS_CONTENT_TYPE_ALERT: u8 = 21;
/// Handshake.
pub const TLS_CONTENT_TYPE_HANDSHAKE: u8 = 22;
/// Application data.
pub const TLS_CONTENT_TYPE_APPLICATION_DATA: u8 = 23;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_versions() {
        assert_eq!(TLS_1_0_VERSION, 0x0301);
        assert_eq!(TLS_1_2_VERSION, 0x0303);
        assert_eq!(TLS_1_3_VERSION, 0x0304);
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
    fn test_key_sizes() {
        assert_eq!(TLS_CIPHER_AES_GCM_128_KEY_SIZE, 16);
        assert_eq!(TLS_CIPHER_AES_GCM_256_KEY_SIZE, 32);
        assert_eq!(TLS_CIPHER_CHACHA20_POLY1305_KEY_SIZE, 32);
    }

    #[test]
    fn test_content_types_distinct() {
        let types = [
            TLS_CONTENT_TYPE_CHANGE_CIPHER_SPEC,
            TLS_CONTENT_TYPE_ALERT,
            TLS_CONTENT_TYPE_HANDSHAKE,
            TLS_CONTENT_TYPE_APPLICATION_DATA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_sol_tls() {
        assert_eq!(SOL_TLS, 282);
    }
}
