//! `<crypto/kpp.h>` — Key Protocol Primitives (Diffie-Hellman, ECDH, etc.).
//!
//! KPP wraps key-agreement protocols. Each protocol has a `set_secret`
//! step (consume own private), a `generate_public_key` step (produce
//! local public to send), and a `compute_shared_secret` step (combine
//! own private with peer's public).

// ---------------------------------------------------------------------------
// Algorithm type identifier
// ---------------------------------------------------------------------------

pub const ALG_TYPE_KPP: &str = "kpp";

// ---------------------------------------------------------------------------
// Common KPP algorithm names
// ---------------------------------------------------------------------------

pub const KPP_NAME_DH: &str = "dh";
pub const KPP_NAME_ECDH_NIST_P192: &str = "ecdh-nist-p192";
pub const KPP_NAME_ECDH_NIST_P256: &str = "ecdh-nist-p256";
pub const KPP_NAME_ECDH_NIST_P384: &str = "ecdh-nist-p384";
pub const KPP_NAME_CURVE25519: &str = "curve25519";
pub const KPP_NAME_X25519: &str = "x25519";
pub const KPP_NAME_X448: &str = "x448";

// ---------------------------------------------------------------------------
// ECDH curve identifiers used by struct ecdh / ECDH_SET_SECRET
// ---------------------------------------------------------------------------

/// Curve ID for NIST P-192.
pub const ECC_CURVE_NIST_P192: u32 = 1;
/// Curve ID for NIST P-256.
pub const ECC_CURVE_NIST_P256: u32 = 2;
/// Curve ID for NIST P-384.
pub const ECC_CURVE_NIST_P384: u32 = 3;
/// Curve ID for Curve25519 (X25519).
pub const ECC_CURVE_25519: u32 = 4;

// ---------------------------------------------------------------------------
// Key sizes (bytes)
// ---------------------------------------------------------------------------

pub const ECDH_P192_KEY_SIZE: usize = 24;
pub const ECDH_P256_KEY_SIZE: usize = 32;
pub const ECDH_P384_KEY_SIZE: usize = 48;
pub const ECDH_P256_SHARED_SIZE: usize = 32;
pub const ECDH_P384_SHARED_SIZE: usize = 48;

pub const X25519_KEY_SIZE: usize = 32;
pub const X25519_SHARED_SIZE: usize = 32;
pub const X448_KEY_SIZE: usize = 56;
pub const X448_SHARED_SIZE: usize = 56;

// ---------------------------------------------------------------------------
// DH parameter sizes (RFC 5114 / RFC 7919 group sizes)
// ---------------------------------------------------------------------------

/// MODP-2048 (ffdhe2048) prime size in bytes.
pub const DH_FFDHE2048_BYTES: usize = 256;
/// MODP-3072 (ffdhe3072) prime size in bytes.
pub const DH_FFDHE3072_BYTES: usize = 384;
/// MODP-4096 prime size in bytes.
pub const DH_FFDHE4096_BYTES: usize = 512;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_name_is_kpp() {
        assert_eq!(ALG_TYPE_KPP, "kpp");
    }

    #[test]
    fn test_algorithm_names_distinct() {
        let n = [
            KPP_NAME_DH,
            KPP_NAME_ECDH_NIST_P192,
            KPP_NAME_ECDH_NIST_P256,
            KPP_NAME_ECDH_NIST_P384,
            KPP_NAME_CURVE25519,
            KPP_NAME_X25519,
            KPP_NAME_X448,
        ];
        for (i, &x) in n.iter().enumerate() {
            for &y in &n[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_curve_ids_distinct_dense_1_to_4() {
        let c = [
            ECC_CURVE_NIST_P192,
            ECC_CURVE_NIST_P256,
            ECC_CURVE_NIST_P384,
            ECC_CURVE_25519,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_ecdh_key_sizes_match_curve_bits() {
        // Curve bit length / 8 == byte size.
        assert_eq!(ECDH_P192_KEY_SIZE * 8, 192);
        assert_eq!(ECDH_P256_KEY_SIZE * 8, 256);
        assert_eq!(ECDH_P384_KEY_SIZE * 8, 384);
    }

    #[test]
    fn test_shared_secret_matches_key_size_for_nist_curves() {
        assert_eq!(ECDH_P256_SHARED_SIZE, ECDH_P256_KEY_SIZE);
        assert_eq!(ECDH_P384_SHARED_SIZE, ECDH_P384_KEY_SIZE);
    }

    #[test]
    fn test_x25519_x448_sizes() {
        assert_eq!(X25519_KEY_SIZE, 32);
        assert_eq!(X25519_SHARED_SIZE, 32);
        assert_eq!(X448_KEY_SIZE, 56);
        assert_eq!(X448_SHARED_SIZE, 56);
        // X448 keys are larger.
        assert!(X448_KEY_SIZE > X25519_KEY_SIZE);
    }

    #[test]
    fn test_ffdhe_byte_sizes_scale_with_bits() {
        assert_eq!(DH_FFDHE2048_BYTES, 256);
        assert_eq!(DH_FFDHE3072_BYTES, 384);
        assert_eq!(DH_FFDHE4096_BYTES, 512);
        // Bytes == bits / 8.
        assert_eq!(DH_FFDHE2048_BYTES * 8, 2048);
        assert_eq!(DH_FFDHE3072_BYTES * 8, 3072);
        assert_eq!(DH_FFDHE4096_BYTES * 8, 4096);
    }
}
