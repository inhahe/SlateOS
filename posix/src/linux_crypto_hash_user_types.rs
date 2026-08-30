//! `<crypto/hash.h>` — synchronous and async hash interfaces.
//!
//! shash/ahash are the kernel's message-digest APIs (init/update/final).
//! Userspace reaches them via AF_ALG `hash` sockets.

// ---------------------------------------------------------------------------
// Algorithm type identifiers
// ---------------------------------------------------------------------------

pub const ALG_TYPE_HASH: &str = "hash";
pub const ALG_TYPE_SHASH: &str = "shash";
pub const ALG_TYPE_AHASH: &str = "ahash";

// ---------------------------------------------------------------------------
// Common hash algorithm names
// ---------------------------------------------------------------------------

pub const HASH_NAME_MD5: &str = "md5";
pub const HASH_NAME_SHA1: &str = "sha1";
pub const HASH_NAME_SHA224: &str = "sha224";
pub const HASH_NAME_SHA256: &str = "sha256";
pub const HASH_NAME_SHA384: &str = "sha384";
pub const HASH_NAME_SHA512: &str = "sha512";
pub const HASH_NAME_SHA3_224: &str = "sha3-224";
pub const HASH_NAME_SHA3_256: &str = "sha3-256";
pub const HASH_NAME_SHA3_384: &str = "sha3-384";
pub const HASH_NAME_SHA3_512: &str = "sha3-512";
pub const HASH_NAME_BLAKE2B_512: &str = "blake2b-512";
pub const HASH_NAME_BLAKE2S_256: &str = "blake2s-256";
pub const HASH_NAME_CRC32: &str = "crc32";
pub const HASH_NAME_CRC32C: &str = "crc32c";
pub const HASH_NAME_SM3: &str = "sm3";

// ---------------------------------------------------------------------------
// Digest output sizes in bytes
// ---------------------------------------------------------------------------

pub const MD5_DIGEST_SIZE: usize = 16;
pub const SHA1_DIGEST_SIZE: usize = 20;
pub const SHA224_DIGEST_SIZE: usize = 28;
pub const SHA256_DIGEST_SIZE: usize = 32;
pub const SHA384_DIGEST_SIZE: usize = 48;
pub const SHA512_DIGEST_SIZE: usize = 64;
pub const SHA3_224_DIGEST_SIZE: usize = 28;
pub const SHA3_256_DIGEST_SIZE: usize = 32;
pub const SHA3_384_DIGEST_SIZE: usize = 48;
pub const SHA3_512_DIGEST_SIZE: usize = 64;
pub const BLAKE2B_OUT_MAX: usize = 64;
pub const BLAKE2S_OUT_MAX: usize = 32;
pub const SM3_DIGEST_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// Internal block sizes
// ---------------------------------------------------------------------------

pub const MD5_BLOCK_SIZE: usize = 64;
pub const SHA1_BLOCK_SIZE: usize = 64;
pub const SHA256_BLOCK_SIZE: usize = 64;
pub const SHA512_BLOCK_SIZE: usize = 128;
pub const SHA3_224_BLOCK_SIZE: usize = 144;
pub const SHA3_256_BLOCK_SIZE: usize = 136;
pub const SHA3_384_BLOCK_SIZE: usize = 104;
pub const SHA3_512_BLOCK_SIZE: usize = 72;

// ---------------------------------------------------------------------------
// HMAC parameters
// ---------------------------------------------------------------------------

/// "hmac(sha256)" style construction prefix.
pub const HMAC_NAME_PREFIX: &str = "hmac";
/// HMAC key may be any length; keys longer than the block are pre-hashed.
pub const HMAC_OPAD_BYTE: u8 = 0x5c;
pub const HMAC_IPAD_BYTE: u8 = 0x36;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_names_distinct() {
        assert_ne!(ALG_TYPE_HASH, ALG_TYPE_SHASH);
        assert_ne!(ALG_TYPE_SHASH, ALG_TYPE_AHASH);
        assert!(ALG_TYPE_SHASH.contains(ALG_TYPE_HASH));
        assert!(ALG_TYPE_AHASH.contains(ALG_TYPE_HASH));
    }

    #[test]
    fn test_digest_sizes_match_published_specs() {
        assert_eq!(MD5_DIGEST_SIZE, 16);
        assert_eq!(SHA1_DIGEST_SIZE, 20);
        assert_eq!(SHA224_DIGEST_SIZE, 28);
        assert_eq!(SHA256_DIGEST_SIZE, 32);
        assert_eq!(SHA384_DIGEST_SIZE, 48);
        assert_eq!(SHA512_DIGEST_SIZE, 64);
        // SHA-3 sizes mirror SHA-2.
        assert_eq!(SHA3_224_DIGEST_SIZE, SHA224_DIGEST_SIZE);
        assert_eq!(SHA3_256_DIGEST_SIZE, SHA256_DIGEST_SIZE);
        assert_eq!(SHA3_384_DIGEST_SIZE, SHA384_DIGEST_SIZE);
        assert_eq!(SHA3_512_DIGEST_SIZE, SHA512_DIGEST_SIZE);
    }

    #[test]
    fn test_block_sizes_match_specs() {
        // MD5/SHA-1/SHA-256 use 512-bit blocks; SHA-512 uses 1024-bit.
        assert_eq!(MD5_BLOCK_SIZE, 64);
        assert_eq!(SHA1_BLOCK_SIZE, 64);
        assert_eq!(SHA256_BLOCK_SIZE, 64);
        assert_eq!(SHA512_BLOCK_SIZE, 128);
        // SHA-3 rates: bigger output → smaller rate (Keccak-f[1600], capacity = 2*output).
        assert!(SHA3_224_BLOCK_SIZE > SHA3_256_BLOCK_SIZE);
        assert!(SHA3_256_BLOCK_SIZE > SHA3_384_BLOCK_SIZE);
        assert!(SHA3_384_BLOCK_SIZE > SHA3_512_BLOCK_SIZE);
        // Rate + capacity = 1600 bits = 200 bytes for all SHA-3 variants.
        assert_eq!(SHA3_224_BLOCK_SIZE + 2 * SHA3_224_DIGEST_SIZE, 200);
        assert_eq!(SHA3_256_BLOCK_SIZE + 2 * SHA3_256_DIGEST_SIZE, 200);
        assert_eq!(SHA3_384_BLOCK_SIZE + 2 * SHA3_384_DIGEST_SIZE, 200);
        assert_eq!(SHA3_512_BLOCK_SIZE + 2 * SHA3_512_DIGEST_SIZE, 200);
    }

    #[test]
    fn test_blake2_max_outputs() {
        assert_eq!(BLAKE2B_OUT_MAX, 64);
        assert_eq!(BLAKE2S_OUT_MAX, 32);
        assert!(BLAKE2B_OUT_MAX > BLAKE2S_OUT_MAX);
    }

    #[test]
    fn test_sm3_is_256_bit() {
        assert_eq!(SM3_DIGEST_SIZE, 32);
        assert_eq!(SM3_DIGEST_SIZE * 8, 256);
    }

    #[test]
    fn test_algorithm_names_distinct() {
        let n = [
            HASH_NAME_MD5,
            HASH_NAME_SHA1,
            HASH_NAME_SHA224,
            HASH_NAME_SHA256,
            HASH_NAME_SHA384,
            HASH_NAME_SHA512,
            HASH_NAME_SHA3_224,
            HASH_NAME_SHA3_256,
            HASH_NAME_SHA3_384,
            HASH_NAME_SHA3_512,
            HASH_NAME_BLAKE2B_512,
            HASH_NAME_BLAKE2S_256,
            HASH_NAME_CRC32,
            HASH_NAME_CRC32C,
            HASH_NAME_SM3,
        ];
        for (i, &x) in n.iter().enumerate() {
            for &y in &n[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_hmac_constants() {
        assert_eq!(HMAC_NAME_PREFIX, "hmac");
        // The two pads must differ in every bit (XOR == 0x6A in HMAC spec).
        assert_eq!(HMAC_OPAD_BYTE ^ HMAC_IPAD_BYTE, 0x6A);
    }
}
