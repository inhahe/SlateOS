//! `<linux/tls.h>` — Additional TLS (Transport Layer Security) constants.
//!
//! Supplementary TLS constants covering protocol versions,
//! cipher types, and configuration options.

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
// TLS cipher types
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
// TLS content types
// ---------------------------------------------------------------------------

/// Change cipher spec.
pub const TLS_CONTENT_TYPE_CHANGE_CIPHER_SPEC: u8 = 20;
/// Alert.
pub const TLS_CONTENT_TYPE_ALERT: u8 = 21;
/// Handshake.
pub const TLS_CONTENT_TYPE_HANDSHAKE: u8 = 22;
/// Application data.
pub const TLS_CONTENT_TYPE_APP_DATA: u8 = 23;
/// Heartbeat.
pub const TLS_CONTENT_TYPE_HEARTBEAT: u8 = 24;

// ---------------------------------------------------------------------------
// TLS socket option levels
// ---------------------------------------------------------------------------

/// TLS ULP (Upper Layer Protocol) name.
pub const TLS_TX: i32 = 1;
/// TLS RX direction.
pub const TLS_RX: i32 = 2;
/// TLS TX + ZeroCopy.
pub const TLS_TX_ZEROCOPY_RO: i32 = 3;
/// TLS RX expect no pad.
pub const TLS_RX_EXPECT_NO_PAD: i32 = 4;

// ---------------------------------------------------------------------------
// TLS GCM constants
// ---------------------------------------------------------------------------

/// AES-128-GCM IV size.
pub const TLS_CIPHER_AES_GCM_128_IV_SIZE: u32 = 8;
/// AES-128-GCM key size.
pub const TLS_CIPHER_AES_GCM_128_KEY_SIZE: u32 = 16;
/// AES-128-GCM salt size.
pub const TLS_CIPHER_AES_GCM_128_SALT_SIZE: u32 = 4;
/// AES-128-GCM tag size.
pub const TLS_CIPHER_AES_GCM_128_TAG_SIZE: u32 = 16;
/// AES-128-GCM record sequence size.
pub const TLS_CIPHER_AES_GCM_128_REC_SEQ_SIZE: u32 = 8;

/// AES-256-GCM IV size.
pub const TLS_CIPHER_AES_GCM_256_IV_SIZE: u32 = 8;
/// AES-256-GCM key size.
pub const TLS_CIPHER_AES_GCM_256_KEY_SIZE: u32 = 32;
/// AES-256-GCM salt size.
pub const TLS_CIPHER_AES_GCM_256_SALT_SIZE: u32 = 4;
/// AES-256-GCM tag size.
pub const TLS_CIPHER_AES_GCM_256_TAG_SIZE: u32 = 16;
/// AES-256-GCM record sequence size.
pub const TLS_CIPHER_AES_GCM_256_REC_SEQ_SIZE: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions_distinct() {
        let versions = [
            TLS_1_0_VERSION,
            TLS_1_1_VERSION,
            TLS_1_2_VERSION,
            TLS_1_3_VERSION,
        ];
        for i in 0..versions.len() {
            for j in (i + 1)..versions.len() {
                assert_ne!(versions[i], versions[j]);
            }
        }
    }

    #[test]
    fn test_versions_ordered() {
        assert!(TLS_1_0_VERSION < TLS_1_1_VERSION);
        assert!(TLS_1_1_VERSION < TLS_1_2_VERSION);
        assert!(TLS_1_2_VERSION < TLS_1_3_VERSION);
    }

    #[test]
    fn test_ciphers_distinct() {
        let ciphers = [
            TLS_CIPHER_AES_GCM_128,
            TLS_CIPHER_AES_GCM_256,
            TLS_CIPHER_AES_CCM_128,
            TLS_CIPHER_CHACHA20_POLY1305,
            TLS_CIPHER_SM4_GCM,
            TLS_CIPHER_SM4_CCM,
        ];
        for i in 0..ciphers.len() {
            for j in (i + 1)..ciphers.len() {
                assert_ne!(ciphers[i], ciphers[j]);
            }
        }
    }

    #[test]
    fn test_content_types_distinct() {
        let types = [
            TLS_CONTENT_TYPE_CHANGE_CIPHER_SPEC,
            TLS_CONTENT_TYPE_ALERT,
            TLS_CONTENT_TYPE_HANDSHAKE,
            TLS_CONTENT_TYPE_APP_DATA,
            TLS_CONTENT_TYPE_HEARTBEAT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_directions_distinct() {
        let dirs = [TLS_TX, TLS_RX, TLS_TX_ZEROCOPY_RO, TLS_RX_EXPECT_NO_PAD];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }

    #[test]
    fn test_gcm128_sizes() {
        assert_eq!(TLS_CIPHER_AES_GCM_128_KEY_SIZE, 16);
        assert_eq!(TLS_CIPHER_AES_GCM_128_TAG_SIZE, 16);
        assert_eq!(TLS_CIPHER_AES_GCM_128_IV_SIZE, 8);
    }

    #[test]
    fn test_gcm256_key_larger() {
        assert!(TLS_CIPHER_AES_GCM_256_KEY_SIZE > TLS_CIPHER_AES_GCM_128_KEY_SIZE);
    }
}
