//! `<crypto/akcipher.h>` — Asymmetric cipher (public-key) constants.
//!
//! Asymmetric cryptography uses key pairs (public + private) for
//! encryption, digital signatures, and key exchange. The kernel uses
//! asymmetric crypto for module signature verification, IMA appraisal,
//! PKCS#7/CMS message verification, TLS handshakes (in kernel TLS),
//! and dm-verity. Keys are typically stored in DER/PEM format and
//! parsed by the kernel's ASN.1 decoder.

// ---------------------------------------------------------------------------
// Asymmetric algorithm identifiers
// ---------------------------------------------------------------------------

/// RSA (Rivest-Shamir-Adleman).
pub const AKCIPHER_ALG_RSA: u32 = 0;
/// ECDSA (Elliptic Curve Digital Signature Algorithm).
pub const AKCIPHER_ALG_ECDSA: u32 = 1;
/// EdDSA (Edwards-curve Digital Signature Algorithm, Ed25519/Ed448).
pub const AKCIPHER_ALG_EDDSA: u32 = 2;
/// SM2 (Chinese national standard elliptic curve).
pub const AKCIPHER_ALG_SM2: u32 = 3;
/// DH (Diffie-Hellman key exchange).
pub const AKCIPHER_ALG_DH: u32 = 4;
/// ECDH (Elliptic Curve Diffie-Hellman).
pub const AKCIPHER_ALG_ECDH: u32 = 5;

// ---------------------------------------------------------------------------
// RSA key sizes (bits)
// ---------------------------------------------------------------------------

/// RSA-2048 (minimum recommended).
pub const RSA_KEY_2048: u32 = 2048;
/// RSA-3072.
pub const RSA_KEY_3072: u32 = 3072;
/// RSA-4096 (high security).
pub const RSA_KEY_4096: u32 = 4096;

// ---------------------------------------------------------------------------
// Elliptic curve identifiers
// ---------------------------------------------------------------------------

/// NIST P-256 (secp256r1, prime256v1).
pub const EC_CURVE_P256: u32 = 0;
/// NIST P-384 (secp384r1).
pub const EC_CURVE_P384: u32 = 1;
/// NIST P-521 (secp521r1).
pub const EC_CURVE_P521: u32 = 2;
/// Curve25519 (for ECDH / X25519).
pub const EC_CURVE_25519: u32 = 3;
/// Curve448 (for ECDH / X448).
pub const EC_CURVE_448: u32 = 4;
/// SM2 curve (Chinese standard).
pub const EC_CURVE_SM2: u32 = 5;

// ---------------------------------------------------------------------------
// RSA padding schemes
// ---------------------------------------------------------------------------

/// PKCS#1 v1.5 padding (legacy, widely used).
pub const RSA_PAD_PKCS1_V15: u32 = 0;
/// OAEP padding (for encryption, recommended).
pub const RSA_PAD_OAEP: u32 = 1;
/// PSS padding (for signatures, recommended).
pub const RSA_PAD_PSS: u32 = 2;
/// No padding (raw RSA, dangerous).
pub const RSA_PAD_NONE: u32 = 3;

// ---------------------------------------------------------------------------
// Asymmetric operation types
// ---------------------------------------------------------------------------

/// Encrypt with public key.
pub const AKCIPHER_OP_ENCRYPT: u32 = 0;
/// Decrypt with private key.
pub const AKCIPHER_OP_DECRYPT: u32 = 1;
/// Sign with private key.
pub const AKCIPHER_OP_SIGN: u32 = 2;
/// Verify with public key.
pub const AKCIPHER_OP_VERIFY: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algorithms_distinct() {
        let algs = [
            AKCIPHER_ALG_RSA, AKCIPHER_ALG_ECDSA, AKCIPHER_ALG_EDDSA,
            AKCIPHER_ALG_SM2, AKCIPHER_ALG_DH, AKCIPHER_ALG_ECDH,
        ];
        for i in 0..algs.len() {
            for j in (i + 1)..algs.len() {
                assert_ne!(algs[i], algs[j]);
            }
        }
    }

    #[test]
    fn test_curves_distinct() {
        let curves = [
            EC_CURVE_P256, EC_CURVE_P384, EC_CURVE_P521,
            EC_CURVE_25519, EC_CURVE_448, EC_CURVE_SM2,
        ];
        for i in 0..curves.len() {
            for j in (i + 1)..curves.len() {
                assert_ne!(curves[i], curves[j]);
            }
        }
    }

    #[test]
    fn test_rsa_key_sizes_ordered() {
        assert!(RSA_KEY_2048 < RSA_KEY_3072);
        assert!(RSA_KEY_3072 < RSA_KEY_4096);
    }

    #[test]
    fn test_padding_schemes_distinct() {
        let pads = [RSA_PAD_PKCS1_V15, RSA_PAD_OAEP, RSA_PAD_PSS, RSA_PAD_NONE];
        for i in 0..pads.len() {
            for j in (i + 1)..pads.len() {
                assert_ne!(pads[i], pads[j]);
            }
        }
    }

    #[test]
    fn test_operations_distinct() {
        let ops = [
            AKCIPHER_OP_ENCRYPT, AKCIPHER_OP_DECRYPT,
            AKCIPHER_OP_SIGN, AKCIPHER_OP_VERIFY,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }
}
