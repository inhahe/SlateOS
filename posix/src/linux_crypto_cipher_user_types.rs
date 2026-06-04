//! `<crypto/cipher.h>` — single-block sync cipher interface.
//!
//! `cipher` exposes the raw block transform — encrypt/decrypt one
//! block at a time with no chaining mode. It's mostly used as a
//! building block for higher-level constructions (CTR, GCM, …).

// ---------------------------------------------------------------------------
// Algorithm type identifier
// ---------------------------------------------------------------------------

pub const ALG_TYPE_CIPHER: &str = "cipher";

// ---------------------------------------------------------------------------
// Common cipher algorithm names
// ---------------------------------------------------------------------------

pub const CIPHER_NAME_AES: &str = "aes";
pub const CIPHER_NAME_DES: &str = "des";
pub const CIPHER_NAME_DES3_EDE: &str = "des3_ede";
pub const CIPHER_NAME_CAMELLIA: &str = "camellia";
pub const CIPHER_NAME_CAST5: &str = "cast5";
pub const CIPHER_NAME_CAST6: &str = "cast6";
pub const CIPHER_NAME_BLOWFISH: &str = "blowfish";
pub const CIPHER_NAME_TWOFISH: &str = "twofish";
pub const CIPHER_NAME_SERPENT: &str = "serpent";
pub const CIPHER_NAME_SM4: &str = "sm4";
pub const CIPHER_NAME_ARIA: &str = "aria";

// ---------------------------------------------------------------------------
// AES key sizes
// ---------------------------------------------------------------------------

pub const AES_BLOCK_SIZE: usize = 16;
pub const AES_KEYSIZE_128: usize = 16;
pub const AES_KEYSIZE_192: usize = 24;
pub const AES_KEYSIZE_256: usize = 32;
pub const AES_MIN_KEY_SIZE: usize = AES_KEYSIZE_128;
pub const AES_MAX_KEY_SIZE: usize = AES_KEYSIZE_256;

// ---------------------------------------------------------------------------
// DES / 3DES sizes
// ---------------------------------------------------------------------------

pub const DES_BLOCK_SIZE: usize = 8;
pub const DES_KEY_SIZE: usize = 8;
pub const DES3_EDE_KEY_SIZE: usize = 24;
pub const DES3_EDE_BLOCK_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// Camellia / Twofish / Serpent / SM4 sizes
// ---------------------------------------------------------------------------

pub const CAMELLIA_BLOCK_SIZE: usize = 16;
pub const CAMELLIA_MIN_KEY_SIZE: usize = 16;
pub const CAMELLIA_MAX_KEY_SIZE: usize = 32;
pub const TWOFISH_BLOCK_SIZE: usize = 16;
pub const SERPENT_BLOCK_SIZE: usize = 16;
pub const SERPENT_MIN_KEY_SIZE: usize = 0;
pub const SERPENT_MAX_KEY_SIZE: usize = 32;
pub const SM4_BLOCK_SIZE: usize = 16;
pub const SM4_KEY_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// EINVAL — wrong key size.
pub const CIPHER_EINVAL: i32 = 22;
/// EKEYREJECTED — weak/forbidden key.
pub const CIPHER_EKEYREJECTED: i32 = 129;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_name_is_cipher() {
        assert_eq!(ALG_TYPE_CIPHER, "cipher");
    }

    #[test]
    fn test_algorithm_names_distinct_lowercase() {
        let n = [
            CIPHER_NAME_AES,
            CIPHER_NAME_DES,
            CIPHER_NAME_DES3_EDE,
            CIPHER_NAME_CAMELLIA,
            CIPHER_NAME_CAST5,
            CIPHER_NAME_CAST6,
            CIPHER_NAME_BLOWFISH,
            CIPHER_NAME_TWOFISH,
            CIPHER_NAME_SERPENT,
            CIPHER_NAME_SM4,
            CIPHER_NAME_ARIA,
        ];
        for (i, &x) in n.iter().enumerate() {
            for &y in &n[i + 1..] {
                assert_ne!(x, y);
            }
            for c in x.chars() {
                assert!(c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit());
            }
        }
    }

    #[test]
    fn test_aes_sizes() {
        assert_eq!(AES_BLOCK_SIZE, 16);
        assert_eq!(AES_KEYSIZE_128, 16);
        assert_eq!(AES_KEYSIZE_192, 24);
        assert_eq!(AES_KEYSIZE_256, 32);
        assert_eq!(AES_MIN_KEY_SIZE, 16);
        assert_eq!(AES_MAX_KEY_SIZE, 32);
        // Key sizes form an arithmetic progression of 8.
        assert_eq!(AES_KEYSIZE_192 - AES_KEYSIZE_128, 8);
        assert_eq!(AES_KEYSIZE_256 - AES_KEYSIZE_192, 8);
    }

    #[test]
    fn test_des_3des_sizes() {
        assert_eq!(DES_BLOCK_SIZE, 8);
        assert_eq!(DES_KEY_SIZE, 8);
        assert_eq!(DES3_EDE_BLOCK_SIZE, 8);
        // 3DES key is exactly three DES keys.
        assert_eq!(DES3_EDE_KEY_SIZE, 3 * DES_KEY_SIZE);
    }

    #[test]
    fn test_camellia_twofish_serpent_blocks_16() {
        assert_eq!(CAMELLIA_BLOCK_SIZE, 16);
        assert_eq!(TWOFISH_BLOCK_SIZE, 16);
        assert_eq!(SERPENT_BLOCK_SIZE, 16);
        assert_eq!(SM4_BLOCK_SIZE, 16);
        // Camellia and Serpent share the 16–32 byte key range.
        assert_eq!(CAMELLIA_MAX_KEY_SIZE, SERPENT_MAX_KEY_SIZE);
    }

    #[test]
    fn test_sm4_key_is_128_bits() {
        assert_eq!(SM4_KEY_SIZE, 16);
        // SM4 is a Chinese national standard; only one key length defined.
        assert_eq!(SM4_KEY_SIZE * 8, 128);
    }

    #[test]
    fn test_errno_values_distinct() {
        assert_ne!(CIPHER_EINVAL, CIPHER_EKEYREJECTED);
        assert_eq!(CIPHER_EINVAL, 22);
        assert_eq!(CIPHER_EKEYREJECTED, 129);
    }
}
