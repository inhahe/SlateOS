//! `<linux/cryptouser.h>` — CRYPTO_USER netlink stat-reporting structures.
//!
//! The CRYPTO_USER protocol over NETLINK_CRYPTO reports per-algorithm
//! statistics: error counts, byte counters, request counts. Used by
//! `iproute2`'s `ip crypto` subcommand and userspace HSM tools.

// ---------------------------------------------------------------------------
// Netlink family for CRYPTO_USER messages
// ---------------------------------------------------------------------------

pub const NETLINK_CRYPTO: u32 = 21;

// ---------------------------------------------------------------------------
// crypto_user attribute identifiers (CRYPTOCFGA_*)
// ---------------------------------------------------------------------------

pub const CRYPTOCFGA_UNSPEC: u32 = 0;
pub const CRYPTOCFGA_PRIORITY_VAL: u32 = 1;
pub const CRYPTOCFGA_REPORT_LARVAL: u32 = 2;
pub const CRYPTOCFGA_REPORT_HASH: u32 = 3;
pub const CRYPTOCFGA_REPORT_BLKCIPHER: u32 = 4;
pub const CRYPTOCFGA_REPORT_AEAD: u32 = 5;
pub const CRYPTOCFGA_REPORT_COMPRESS: u32 = 6;
pub const CRYPTOCFGA_REPORT_RNG: u32 = 7;
pub const CRYPTOCFGA_REPORT_CIPHER: u32 = 8;
pub const CRYPTOCFGA_REPORT_AKCIPHER: u32 = 9;
pub const CRYPTOCFGA_REPORT_KPP: u32 = 10;
pub const CRYPTOCFGA_REPORT_ACOMP: u32 = 11;
pub const CRYPTOCFGA_STAT_LARVAL: u32 = 12;
pub const CRYPTOCFGA_STAT_HASH: u32 = 13;
pub const CRYPTOCFGA_STAT_BLKCIPHER: u32 = 14;
pub const CRYPTOCFGA_STAT_AEAD: u32 = 15;
pub const CRYPTOCFGA_STAT_COMPRESS: u32 = 16;
pub const CRYPTOCFGA_STAT_RNG: u32 = 17;
pub const CRYPTOCFGA_STAT_CIPHER: u32 = 18;
pub const CRYPTOCFGA_STAT_AKCIPHER: u32 = 19;
pub const CRYPTOCFGA_STAT_KPP: u32 = 20;
pub const CRYPTOCFGA_STAT_ACOMP: u32 = 21;

// ---------------------------------------------------------------------------
// Field offsets within struct crypto_stat_*
// ---------------------------------------------------------------------------

/// struct crypto_user_alg::cru_name length.
pub const CRYPTO_MAX_NAME: usize = 64;
/// struct crypto_user_alg::cru_driver_name length.
pub const CRYPTO_MAX_DRIVER_NAME: usize = 128;
/// struct crypto_user_alg::cru_module length.
pub const CRYPTO_MAX_MODULE_NAME: usize = 64;
/// struct crypto_user_alg size: 64+128+64+4+4+4 = 268 bytes.
pub const CRYPTO_USER_ALG_SIZE: usize =
    CRYPTO_MAX_NAME + CRYPTO_MAX_DRIVER_NAME + CRYPTO_MAX_MODULE_NAME + 4 + 4 + 4;

// ---------------------------------------------------------------------------
// crypto_stat_aead / crypto_stat_hash field count
// ---------------------------------------------------------------------------

/// Each stat structure is "name[CRYPTO_MAX_NAME] + N u64 counters".
pub const CRYPTO_STAT_NAME_HEAD_SIZE: usize = CRYPTO_MAX_NAME;
/// Common counters: encrypt_cnt, encrypt_tlen, decrypt_cnt, decrypt_tlen, err_cnt.
pub const CRYPTO_STAT_COMMON_COUNTERS: usize = 5;

// ---------------------------------------------------------------------------
// Maximum CRYPTOCFGA attribute (for nla_parse nla_policy[] sizing).
// ---------------------------------------------------------------------------

pub const CRYPTOCFGA_MAX: u32 = CRYPTOCFGA_STAT_ACOMP;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_netlink_family_is_21() {
        assert_eq!(NETLINK_CRYPTO, 21);
    }

    #[test]
    fn test_cryptocfga_attrs_dense_0_to_21() {
        let a = [
            CRYPTOCFGA_UNSPEC,
            CRYPTOCFGA_PRIORITY_VAL,
            CRYPTOCFGA_REPORT_LARVAL,
            CRYPTOCFGA_REPORT_HASH,
            CRYPTOCFGA_REPORT_BLKCIPHER,
            CRYPTOCFGA_REPORT_AEAD,
            CRYPTOCFGA_REPORT_COMPRESS,
            CRYPTOCFGA_REPORT_RNG,
            CRYPTOCFGA_REPORT_CIPHER,
            CRYPTOCFGA_REPORT_AKCIPHER,
            CRYPTOCFGA_REPORT_KPP,
            CRYPTOCFGA_REPORT_ACOMP,
            CRYPTOCFGA_STAT_LARVAL,
            CRYPTOCFGA_STAT_HASH,
            CRYPTOCFGA_STAT_BLKCIPHER,
            CRYPTOCFGA_STAT_AEAD,
            CRYPTOCFGA_STAT_COMPRESS,
            CRYPTOCFGA_STAT_RNG,
            CRYPTOCFGA_STAT_CIPHER,
            CRYPTOCFGA_STAT_AKCIPHER,
            CRYPTOCFGA_STAT_KPP,
            CRYPTOCFGA_STAT_ACOMP,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_report_and_stat_have_matching_subtypes() {
        // For each REPORT_X there's a corresponding STAT_X exactly 10 apart.
        assert_eq!(CRYPTOCFGA_STAT_HASH, CRYPTOCFGA_REPORT_HASH + 10);
        assert_eq!(CRYPTOCFGA_STAT_AEAD, CRYPTOCFGA_REPORT_AEAD + 10);
        assert_eq!(CRYPTOCFGA_STAT_RNG, CRYPTOCFGA_REPORT_RNG + 10);
        assert_eq!(CRYPTOCFGA_STAT_AKCIPHER, CRYPTOCFGA_REPORT_AKCIPHER + 10);
        assert_eq!(CRYPTOCFGA_STAT_KPP, CRYPTOCFGA_REPORT_KPP + 10);
    }

    #[test]
    fn test_max_attr_is_stat_acomp() {
        assert_eq!(CRYPTOCFGA_MAX, CRYPTOCFGA_STAT_ACOMP);
        assert_eq!(CRYPTOCFGA_MAX, 21);
    }

    #[test]
    fn test_alg_struct_size_268() {
        // Three name strings plus three u32 fields.
        assert_eq!(CRYPTO_USER_ALG_SIZE, 64 + 128 + 64 + 4 + 4 + 4);
        assert_eq!(CRYPTO_USER_ALG_SIZE, 268);
    }

    #[test]
    fn test_name_lengths_match_kernel() {
        assert_eq!(CRYPTO_MAX_NAME, 64);
        assert_eq!(CRYPTO_MAX_DRIVER_NAME, 128);
        assert_eq!(CRYPTO_MAX_MODULE_NAME, 64);
    }

    #[test]
    fn test_stat_common_counters_count() {
        // 5 u64 counters: encrypt_cnt, encrypt_tlen, decrypt_cnt, decrypt_tlen, err_cnt.
        assert_eq!(CRYPTO_STAT_COMMON_COUNTERS, 5);
        assert_eq!(CRYPTO_STAT_NAME_HEAD_SIZE, CRYPTO_MAX_NAME);
    }
}
