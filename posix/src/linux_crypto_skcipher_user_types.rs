//! `<crypto/skcipher.h>` — symmetric block-cipher chaining-mode interface.
//!
//! skcipher wraps a base block cipher with a chaining mode (CBC, CTR,
//! XTS, …) and handles IV management. It's the workhorse for dm-crypt,
//! IPsec ESP, and userspace AF_ALG `skcipher` sockets.

// ---------------------------------------------------------------------------
// Algorithm type identifier
// ---------------------------------------------------------------------------

pub const ALG_TYPE_SKCIPHER: &str = "skcipher";
pub const ALG_TYPE_LSKCIPHER: &str = "lskcipher";

// ---------------------------------------------------------------------------
// Mode + cipher name templates
// ---------------------------------------------------------------------------

pub const SKCIPHER_NAME_CBC_AES: &str = "cbc(aes)";
pub const SKCIPHER_NAME_CTR_AES: &str = "ctr(aes)";
pub const SKCIPHER_NAME_XTS_AES: &str = "xts(aes)";
pub const SKCIPHER_NAME_ECB_AES: &str = "ecb(aes)";
pub const SKCIPHER_NAME_CFB_AES: &str = "cfb(aes)";
pub const SKCIPHER_NAME_OFB_AES: &str = "ofb(aes)";
pub const SKCIPHER_NAME_CTS_CBC_AES: &str = "cts(cbc(aes))";
pub const SKCIPHER_NAME_ESSIV_CBC_AES: &str = "essiv(cbc(aes),sha256)";
pub const SKCIPHER_NAME_ADIANTUM: &str = "adiantum(xchacha12,aes)";

// ---------------------------------------------------------------------------
// IV sizes for AES modes (bytes)
// ---------------------------------------------------------------------------

pub const AES_BLOCK_SIZE: usize = 16;
pub const CBC_AES_IV_SIZE: usize = AES_BLOCK_SIZE;
pub const CTR_AES_IV_SIZE: usize = AES_BLOCK_SIZE;
pub const XTS_AES_IV_SIZE: usize = AES_BLOCK_SIZE;
/// ECB has no IV.
pub const ECB_AES_IV_SIZE: usize = 0;

// ---------------------------------------------------------------------------
// XTS key sizing — XTS uses a "double" key (two AES keys concatenated)
// ---------------------------------------------------------------------------

pub const XTS_AES_128_KEY_SIZE: usize = 32;
pub const XTS_AES_256_KEY_SIZE: usize = 64;

// ---------------------------------------------------------------------------
// dm-crypt sector / chunk constants
// ---------------------------------------------------------------------------

pub const DM_CRYPT_SECTOR_SIZE: usize = 512;
pub const DM_CRYPT_MAX_KEY_SIZE: usize = 256;

// ---------------------------------------------------------------------------
// Errors specific to skcipher operations
// ---------------------------------------------------------------------------

pub const SKCIPHER_EINVAL: i32 = 22;
pub const SKCIPHER_EOVERFLOW: i32 = 75;
pub const SKCIPHER_EINPROGRESS: i32 = 115;
pub const SKCIPHER_EBUSY: i32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_names_distinct() {
        assert_ne!(ALG_TYPE_SKCIPHER, ALG_TYPE_LSKCIPHER);
        assert!(ALG_TYPE_LSKCIPHER.contains(ALG_TYPE_SKCIPHER));
    }

    #[test]
    fn test_mode_names_match_kernel_format() {
        // All names are of the form "<mode>(<cipher>)".
        for n in [
            SKCIPHER_NAME_CBC_AES,
            SKCIPHER_NAME_CTR_AES,
            SKCIPHER_NAME_XTS_AES,
            SKCIPHER_NAME_ECB_AES,
            SKCIPHER_NAME_CFB_AES,
            SKCIPHER_NAME_OFB_AES,
        ] {
            assert!(n.contains('('));
            assert!(n.ends_with(')'));
            assert!(n.contains("aes"));
        }
    }

    #[test]
    fn test_aes_iv_sizes_match_block_or_zero() {
        assert_eq!(CBC_AES_IV_SIZE, AES_BLOCK_SIZE);
        assert_eq!(CTR_AES_IV_SIZE, AES_BLOCK_SIZE);
        assert_eq!(XTS_AES_IV_SIZE, AES_BLOCK_SIZE);
        assert_eq!(ECB_AES_IV_SIZE, 0);
    }

    #[test]
    fn test_xts_keys_are_double_aes_keys() {
        // XTS-AES-128 = AES-128 + AES-128 concatenated = 32 bytes.
        assert_eq!(XTS_AES_128_KEY_SIZE, 16 * 2);
        // XTS-AES-256 = AES-256 + AES-256 = 64 bytes.
        assert_eq!(XTS_AES_256_KEY_SIZE, 32 * 2);
    }

    #[test]
    fn test_dm_crypt_sector_size_512() {
        assert_eq!(DM_CRYPT_SECTOR_SIZE, 512);
        assert!(DM_CRYPT_SECTOR_SIZE.is_power_of_two());
        // Sector cleanly contains AES blocks.
        assert_eq!(DM_CRYPT_SECTOR_SIZE % AES_BLOCK_SIZE, 0);
    }

    #[test]
    fn test_dm_crypt_max_key_fits_xts_256() {
        assert!(DM_CRYPT_MAX_KEY_SIZE >= XTS_AES_256_KEY_SIZE);
    }

    #[test]
    fn test_special_modes_well_formed() {
        // CTS wraps CBC; ESSIV uses SHA-256 hash; Adiantum is hash-then-encrypt.
        assert!(SKCIPHER_NAME_CTS_CBC_AES.starts_with("cts("));
        assert!(SKCIPHER_NAME_ESSIV_CBC_AES.contains("sha256"));
        assert!(SKCIPHER_NAME_ADIANTUM.contains("xchacha12"));
    }

    #[test]
    fn test_errno_values_distinct_standard() {
        let e = [
            SKCIPHER_EINVAL,
            SKCIPHER_EOVERFLOW,
            SKCIPHER_EINPROGRESS,
            SKCIPHER_EBUSY,
        ];
        for (i, &x) in e.iter().enumerate() {
            for &y in &e[i + 1..] {
                assert_ne!(x, y);
            }
        }
        assert_eq!(SKCIPHER_EINVAL, 22);
        assert_eq!(SKCIPHER_EOVERFLOW, 75);
        assert_eq!(SKCIPHER_EINPROGRESS, 115);
        assert_eq!(SKCIPHER_EBUSY, 16);
    }
}
