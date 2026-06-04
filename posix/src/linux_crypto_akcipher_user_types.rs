//! `<crypto/akcipher.h>` — asymmetric (public-key) cipher interface.
//!
//! akcipher is the kernel's RSA/ECDSA/etc. interface used by IMA,
//! kernel module signing, fs-verity, and userspace via AF_ALG. The
//! API exposes four operations: encrypt, decrypt, sign, verify.

// ---------------------------------------------------------------------------
// Algorithm type identifier
// ---------------------------------------------------------------------------

pub const ALG_TYPE_AKCIPHER: &str = "akcipher";

// ---------------------------------------------------------------------------
// Common algorithm names
// ---------------------------------------------------------------------------

pub const AKCIPHER_NAME_RSA: &str = "rsa";
pub const AKCIPHER_NAME_PKCS1PAD_RSA: &str = "pkcs1pad(rsa)";
pub const AKCIPHER_NAME_ECDSA_NIST_P256: &str = "ecdsa-nist-p256";
pub const AKCIPHER_NAME_ECDSA_NIST_P384: &str = "ecdsa-nist-p384";
pub const AKCIPHER_NAME_ECDSA_NIST_P521: &str = "ecdsa-nist-p521";
pub const AKCIPHER_NAME_ECRDSA: &str = "ecrdsa";
pub const AKCIPHER_NAME_SM2: &str = "sm2";

// ---------------------------------------------------------------------------
// Common key sizes (bits)
// ---------------------------------------------------------------------------

pub const RSA_MIN_KEY_BITS: u32 = 1024;
pub const RSA_MAX_KEY_BITS: u32 = 8192;
pub const RSA_RECOMMENDED_KEY_BITS: u32 = 3072;

pub const ECDSA_P256_KEY_BITS: u32 = 256;
pub const ECDSA_P384_KEY_BITS: u32 = 384;
pub const ECDSA_P521_KEY_BITS: u32 = 521;

// ---------------------------------------------------------------------------
// Operation codes for AF_ALG akcipher
// ---------------------------------------------------------------------------

pub const ALG_OP_DECRYPT: u32 = 0;
pub const ALG_OP_ENCRYPT: u32 = 1;
pub const ALG_OP_SIGN: u32 = 2;
pub const ALG_OP_VERIFY: u32 = 3;

// ---------------------------------------------------------------------------
// Errors returned by akcipher_request operations
// ---------------------------------------------------------------------------

/// EINPROGRESS — async operation queued.
pub const AKCIPHER_EINPROGRESS: i32 = 115;
/// EBUSY — queue full (set CRYPTO_TFM_REQ_MAY_BACKLOG).
pub const AKCIPHER_EBUSY: i32 = 16;
/// EBADMSG — signature verification failed.
pub const AKCIPHER_EBADMSG: i32 = 74;
/// EKEYREJECTED — public/private key invalid.
pub const AKCIPHER_EKEYREJECTED: i32 = 129;

// ---------------------------------------------------------------------------
// RSA padding scheme strings (used inside pkcs1pad(...))
// ---------------------------------------------------------------------------

pub const RSA_PADDING_PKCS1: &str = "pkcs1pad";
pub const RSA_PADDING_OAEP_SHA1: &str = "pkcs1pad(rsa,sha1)";
pub const RSA_PADDING_OAEP_SHA256: &str = "pkcs1pad(rsa,sha256)";
pub const RSA_PADDING_OAEP_SHA512: &str = "pkcs1pad(rsa,sha512)";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_name_is_akcipher() {
        assert_eq!(ALG_TYPE_AKCIPHER, "akcipher");
    }

    #[test]
    fn test_alg_names_distinct() {
        let n = [
            AKCIPHER_NAME_RSA,
            AKCIPHER_NAME_PKCS1PAD_RSA,
            AKCIPHER_NAME_ECDSA_NIST_P256,
            AKCIPHER_NAME_ECDSA_NIST_P384,
            AKCIPHER_NAME_ECDSA_NIST_P521,
            AKCIPHER_NAME_ECRDSA,
            AKCIPHER_NAME_SM2,
        ];
        for (i, &x) in n.iter().enumerate() {
            for &y in &n[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_rsa_key_bit_bounds() {
        assert!(RSA_MIN_KEY_BITS <= RSA_RECOMMENDED_KEY_BITS);
        assert!(RSA_RECOMMENDED_KEY_BITS <= RSA_MAX_KEY_BITS);
        assert_eq!(RSA_MIN_KEY_BITS, 1024);
        assert_eq!(RSA_RECOMMENDED_KEY_BITS, 3072);
        assert_eq!(RSA_MAX_KEY_BITS, 8192);
    }

    #[test]
    fn test_ecdsa_key_bits_match_curve_names() {
        assert_eq!(ECDSA_P256_KEY_BITS, 256);
        assert_eq!(ECDSA_P384_KEY_BITS, 384);
        assert_eq!(ECDSA_P521_KEY_BITS, 521);
        // P-521 is the only curve whose modulus isn't byte-aligned.
        assert_ne!(ECDSA_P521_KEY_BITS % 8, 0);
        assert_eq!(ECDSA_P256_KEY_BITS % 8, 0);
        assert_eq!(ECDSA_P384_KEY_BITS % 8, 0);
    }

    #[test]
    fn test_op_codes_dense_0_to_3() {
        let o = [
            ALG_OP_DECRYPT,
            ALG_OP_ENCRYPT,
            ALG_OP_SIGN,
            ALG_OP_VERIFY,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_errnos_distinct_standard() {
        let e = [
            AKCIPHER_EINPROGRESS,
            AKCIPHER_EBUSY,
            AKCIPHER_EBADMSG,
            AKCIPHER_EKEYREJECTED,
        ];
        for (i, &x) in e.iter().enumerate() {
            for &y in &e[i + 1..] {
                assert_ne!(x, y);
            }
        }
        assert_eq!(AKCIPHER_EINPROGRESS, 115);
        assert_eq!(AKCIPHER_EBUSY, 16);
        assert_eq!(AKCIPHER_EBADMSG, 74);
        assert_eq!(AKCIPHER_EKEYREJECTED, 129);
    }

    #[test]
    fn test_oaep_padding_strings_contain_hash() {
        assert!(RSA_PADDING_OAEP_SHA1.contains("sha1"));
        assert!(RSA_PADDING_OAEP_SHA256.contains("sha256"));
        assert!(RSA_PADDING_OAEP_SHA512.contains("sha512"));
        // All start with pkcs1pad(.
        for p in [
            RSA_PADDING_OAEP_SHA1,
            RSA_PADDING_OAEP_SHA256,
            RSA_PADDING_OAEP_SHA512,
        ] {
            assert!(p.starts_with("pkcs1pad("));
        }
    }
}
