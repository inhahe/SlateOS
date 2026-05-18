//! `<crypto/kpp.h>` — Key-agreement Protocol Primitives (KPP) constants.
//!
//! The KPP API provides key agreement operations (Diffie-Hellman,
//! ECDH). These are used for establishing shared secrets between
//! parties without transmitting the secret itself. The kernel uses
//! KPP for TLS handshake offload and IKE/IPsec key negotiation.

// ---------------------------------------------------------------------------
// KPP algorithm IDs
// ---------------------------------------------------------------------------

/// Diffie-Hellman key agreement.
pub const CRYPTO_KPP_DH: u32 = 0;
/// ECDH (Elliptic Curve Diffie-Hellman).
pub const CRYPTO_KPP_ECDH: u32 = 1;
/// Curve25519 key agreement.
pub const CRYPTO_KPP_CURVE25519: u32 = 2;

// ---------------------------------------------------------------------------
// DH group IDs (RFC 3526 / RFC 7919)
// ---------------------------------------------------------------------------

/// DH Group 14 (2048-bit MODP).
pub const CRYPTO_DH_GROUP_14: u32 = 14;
/// DH Group 15 (3072-bit MODP).
pub const CRYPTO_DH_GROUP_15: u32 = 15;
/// DH Group 16 (4096-bit MODP).
pub const CRYPTO_DH_GROUP_16: u32 = 16;
/// DH Group 17 (6144-bit MODP).
pub const CRYPTO_DH_GROUP_17: u32 = 17;
/// DH Group 18 (8192-bit MODP).
pub const CRYPTO_DH_GROUP_18: u32 = 18;

// ---------------------------------------------------------------------------
// ECDH curve IDs (NIST curves)
// ---------------------------------------------------------------------------

/// NIST P-192 (secp192r1).
pub const CRYPTO_ECDH_CURVE_P192: u32 = 1;
/// NIST P-256 (secp256r1).
pub const CRYPTO_ECDH_CURVE_P256: u32 = 2;
/// NIST P-384 (secp384r1).
pub const CRYPTO_ECDH_CURVE_P384: u32 = 3;
/// NIST P-521 (secp521r1).
pub const CRYPTO_ECDH_CURVE_P521: u32 = 4;

// ---------------------------------------------------------------------------
// Key sizes (bytes)
// ---------------------------------------------------------------------------

/// Curve25519 key size (32 bytes = 256 bits).
pub const CRYPTO_CURVE25519_KEY_SIZE: u32 = 32;
/// P-256 private key size.
pub const CRYPTO_ECDH_P256_KEY_SIZE: u32 = 32;
/// P-384 private key size.
pub const CRYPTO_ECDH_P384_KEY_SIZE: u32 = 48;
/// P-521 private key size.
pub const CRYPTO_ECDH_P521_KEY_SIZE: u32 = 66;

// ---------------------------------------------------------------------------
// KPP operation flags
// ---------------------------------------------------------------------------

/// Generate ephemeral key pair.
pub const CRYPTO_KPP_FLAG_GENERATE: u32 = 1 << 0;
/// Compute shared secret.
pub const CRYPTO_KPP_FLAG_COMPUTE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kpp_algorithms_distinct() {
        assert_ne!(CRYPTO_KPP_DH, CRYPTO_KPP_ECDH);
        assert_ne!(CRYPTO_KPP_ECDH, CRYPTO_KPP_CURVE25519);
        assert_ne!(CRYPTO_KPP_DH, CRYPTO_KPP_CURVE25519);
    }

    #[test]
    fn test_dh_groups_distinct() {
        let groups = [
            CRYPTO_DH_GROUP_14, CRYPTO_DH_GROUP_15,
            CRYPTO_DH_GROUP_16, CRYPTO_DH_GROUP_17,
            CRYPTO_DH_GROUP_18,
        ];
        for i in 0..groups.len() {
            for j in (i + 1)..groups.len() {
                assert_ne!(groups[i], groups[j]);
            }
        }
    }

    #[test]
    fn test_ecdh_curves_distinct() {
        let curves = [
            CRYPTO_ECDH_CURVE_P192, CRYPTO_ECDH_CURVE_P256,
            CRYPTO_ECDH_CURVE_P384, CRYPTO_ECDH_CURVE_P521,
        ];
        for i in 0..curves.len() {
            for j in (i + 1)..curves.len() {
                assert_ne!(curves[i], curves[j]);
            }
        }
    }

    #[test]
    fn test_key_sizes() {
        assert_eq!(CRYPTO_CURVE25519_KEY_SIZE, 32);
        assert_eq!(CRYPTO_ECDH_P256_KEY_SIZE, 32);
        assert!(CRYPTO_ECDH_P384_KEY_SIZE > CRYPTO_ECDH_P256_KEY_SIZE);
        assert!(CRYPTO_ECDH_P521_KEY_SIZE > CRYPTO_ECDH_P384_KEY_SIZE);
    }

    #[test]
    fn test_operation_flags_no_overlap() {
        assert!(CRYPTO_KPP_FLAG_GENERATE.is_power_of_two());
        assert!(CRYPTO_KPP_FLAG_COMPUTE.is_power_of_two());
        assert_eq!(CRYPTO_KPP_FLAG_GENERATE & CRYPTO_KPP_FLAG_COMPUTE, 0);
    }
}
