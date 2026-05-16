//! `<linux/virtio_crypto.h>` — Virtio crypto device constants.
//!
//! Virtio-crypto provides hardware-accelerated cryptographic
//! operations to guest VMs. It supports symmetric ciphers,
//! hashes, MACs, and AEAD algorithms via a paravirtualized
//! interface to the host's crypto hardware.

// ---------------------------------------------------------------------------
// Service types
// ---------------------------------------------------------------------------

/// Cipher service (encrypt/decrypt).
pub const VIRTIO_CRYPTO_SERVICE_CIPHER: u32 = 0;
/// Hash service.
pub const VIRTIO_CRYPTO_SERVICE_HASH: u32 = 1;
/// MAC service.
pub const VIRTIO_CRYPTO_SERVICE_MAC: u32 = 2;
/// AEAD service.
pub const VIRTIO_CRYPTO_SERVICE_AEAD: u32 = 3;
/// Asymmetric key service.
pub const VIRTIO_CRYPTO_SERVICE_AKCIPHER: u32 = 4;

// ---------------------------------------------------------------------------
// Cipher algorithms
// ---------------------------------------------------------------------------

/// AES-CBC.
pub const VIRTIO_CRYPTO_CIPHER_AES_CBC: u32 = 1;
/// AES-CTR.
pub const VIRTIO_CRYPTO_CIPHER_AES_CTR: u32 = 2;
/// AES-ECB.
pub const VIRTIO_CRYPTO_CIPHER_AES_ECB: u32 = 3;
/// AES-XTS.
pub const VIRTIO_CRYPTO_CIPHER_AES_XTS: u32 = 4;
/// DES3-CBC.
pub const VIRTIO_CRYPTO_CIPHER_3DES_CBC: u32 = 5;
/// DES3-ECB.
pub const VIRTIO_CRYPTO_CIPHER_3DES_ECB: u32 = 6;

// ---------------------------------------------------------------------------
// Hash algorithms
// ---------------------------------------------------------------------------

/// SHA-1.
pub const VIRTIO_CRYPTO_HASH_SHA1: u32 = 1;
/// SHA-256.
pub const VIRTIO_CRYPTO_HASH_SHA256: u32 = 2;
/// SHA-384.
pub const VIRTIO_CRYPTO_HASH_SHA384: u32 = 3;
/// SHA-512.
pub const VIRTIO_CRYPTO_HASH_SHA512: u32 = 4;

// ---------------------------------------------------------------------------
// AEAD algorithms
// ---------------------------------------------------------------------------

/// AES-GCM.
pub const VIRTIO_CRYPTO_AEAD_AES_GCM: u32 = 1;
/// AES-CCM.
pub const VIRTIO_CRYPTO_AEAD_AES_CCM: u32 = 2;
/// ChaCha20-Poly1305.
pub const VIRTIO_CRYPTO_AEAD_CHACHA20_POLY1305: u32 = 3;

// ---------------------------------------------------------------------------
// Operation types
// ---------------------------------------------------------------------------

/// Encrypt operation.
pub const VIRTIO_CRYPTO_OP_ENCRYPT: u32 = 0;
/// Decrypt operation.
pub const VIRTIO_CRYPTO_OP_DECRYPT: u32 = 1;

// ---------------------------------------------------------------------------
// Status codes
// ---------------------------------------------------------------------------

/// Operation successful.
pub const VIRTIO_CRYPTO_OK: u32 = 0;
/// Bad message.
pub const VIRTIO_CRYPTO_BADMSG: u32 = 1;
/// Not implemented.
pub const VIRTIO_CRYPTO_NOTSUPP: u32 = 2;
/// Internal error.
pub const VIRTIO_CRYPTO_INVSESS: u32 = 3;
/// Key rejected.
pub const VIRTIO_CRYPTO_ERR: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_services_distinct() {
        let svcs = [
            VIRTIO_CRYPTO_SERVICE_CIPHER, VIRTIO_CRYPTO_SERVICE_HASH,
            VIRTIO_CRYPTO_SERVICE_MAC, VIRTIO_CRYPTO_SERVICE_AEAD,
            VIRTIO_CRYPTO_SERVICE_AKCIPHER,
        ];
        for i in 0..svcs.len() {
            for j in (i + 1)..svcs.len() {
                assert_ne!(svcs[i], svcs[j]);
            }
        }
    }

    #[test]
    fn test_cipher_algos_distinct() {
        let algos = [
            VIRTIO_CRYPTO_CIPHER_AES_CBC, VIRTIO_CRYPTO_CIPHER_AES_CTR,
            VIRTIO_CRYPTO_CIPHER_AES_ECB, VIRTIO_CRYPTO_CIPHER_AES_XTS,
            VIRTIO_CRYPTO_CIPHER_3DES_CBC, VIRTIO_CRYPTO_CIPHER_3DES_ECB,
        ];
        for i in 0..algos.len() {
            for j in (i + 1)..algos.len() {
                assert_ne!(algos[i], algos[j]);
            }
        }
    }

    #[test]
    fn test_hash_algos_distinct() {
        let algos = [
            VIRTIO_CRYPTO_HASH_SHA1, VIRTIO_CRYPTO_HASH_SHA256,
            VIRTIO_CRYPTO_HASH_SHA384, VIRTIO_CRYPTO_HASH_SHA512,
        ];
        for i in 0..algos.len() {
            for j in (i + 1)..algos.len() {
                assert_ne!(algos[i], algos[j]);
            }
        }
    }

    #[test]
    fn test_aead_algos_distinct() {
        let algos = [
            VIRTIO_CRYPTO_AEAD_AES_GCM,
            VIRTIO_CRYPTO_AEAD_AES_CCM,
            VIRTIO_CRYPTO_AEAD_CHACHA20_POLY1305,
        ];
        for i in 0..algos.len() {
            for j in (i + 1)..algos.len() {
                assert_ne!(algos[i], algos[j]);
            }
        }
    }

    #[test]
    fn test_ops_distinct() {
        assert_ne!(VIRTIO_CRYPTO_OP_ENCRYPT, VIRTIO_CRYPTO_OP_DECRYPT);
    }

    #[test]
    fn test_status_codes_distinct() {
        let codes = [
            VIRTIO_CRYPTO_OK, VIRTIO_CRYPTO_BADMSG,
            VIRTIO_CRYPTO_NOTSUPP, VIRTIO_CRYPTO_INVSESS,
            VIRTIO_CRYPTO_ERR,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }
}
