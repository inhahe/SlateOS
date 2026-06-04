//! `<linux/cryptouser.h>` & `<crypto/algapi.h>` — netlink CRYPTO_USER
//! interface for enumerating and configuring kernel crypto algorithms.

// ---------------------------------------------------------------------------
// Netlink family / message group
// ---------------------------------------------------------------------------

pub const NETLINK_CRYPTO: u32 = 21;

// ---------------------------------------------------------------------------
// Message types (crypto_msg)
// ---------------------------------------------------------------------------

pub const CRYPTO_MSG_BASE: u32 = 0x10;
pub const CRYPTO_MSG_NEWALG: u32 = 0x10;
pub const CRYPTO_MSG_DELALG: u32 = 0x11;
pub const CRYPTO_MSG_UPDATEALG: u32 = 0x12;
pub const CRYPTO_MSG_GETALG: u32 = 0x13;
pub const CRYPTO_MSG_DELRNG: u32 = 0x14;
pub const CRYPTO_MSG_GETSTAT: u32 = 0x15;
pub const CRYPTO_MSG_MAX: u32 = 0x15;

// ---------------------------------------------------------------------------
// Common name lengths
// ---------------------------------------------------------------------------

pub const CRYPTO_MAX_NAME: usize = 64;
pub const CRYPTO_MAX_ALG_NAME: usize = 128;
/// Driver name (e.g. "sha256-ssse3").
pub const CRYPTO_MAX_DRIVER_NAME: usize = 128;

// ---------------------------------------------------------------------------
// Attribute type for netlink CRYPTOCFGA_* (subset)
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

// ---------------------------------------------------------------------------
// Algorithm type (CRYPTO_ALG_TYPE_*)
// ---------------------------------------------------------------------------

pub const CRYPTO_ALG_TYPE_MASK: u32 = 0x0000_000F;
pub const CRYPTO_ALG_TYPE_CIPHER: u32 = 0x01;
pub const CRYPTO_ALG_TYPE_COMPRESS: u32 = 0x02;
pub const CRYPTO_ALG_TYPE_AEAD: u32 = 0x03;
pub const CRYPTO_ALG_TYPE_LSKCIPHER: u32 = 0x04;
pub const CRYPTO_ALG_TYPE_AKCIPHER: u32 = 0x06;
pub const CRYPTO_ALG_TYPE_SIG: u32 = 0x07;
pub const CRYPTO_ALG_TYPE_KPP: u32 = 0x08;
pub const CRYPTO_ALG_TYPE_ACOMPRESS: u32 = 0x0a;
pub const CRYPTO_ALG_TYPE_SHASH: u32 = 0x0e;
pub const CRYPTO_ALG_TYPE_AHASH: u32 = 0x0f;

// ---------------------------------------------------------------------------
// Algorithm flags
// ---------------------------------------------------------------------------

pub const CRYPTO_ALG_LARVAL: u32 = 1 << 4;
pub const CRYPTO_ALG_DEAD: u32 = 1 << 5;
pub const CRYPTO_ALG_DYING: u32 = 1 << 6;
pub const CRYPTO_ALG_ASYNC: u32 = 1 << 7;
pub const CRYPTO_ALG_NEED_FALLBACK: u32 = 1 << 8;
pub const CRYPTO_ALG_TESTED: u32 = 1 << 10;
pub const CRYPTO_ALG_INSTANCE: u32 = 1 << 11;
pub const CRYPTO_ALG_KERN_DRIVER_ONLY: u32 = 1 << 12;

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
    fn test_msg_base_range_contiguous() {
        let m = [
            CRYPTO_MSG_NEWALG,
            CRYPTO_MSG_DELALG,
            CRYPTO_MSG_UPDATEALG,
            CRYPTO_MSG_GETALG,
            CRYPTO_MSG_DELRNG,
            CRYPTO_MSG_GETSTAT,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v, CRYPTO_MSG_BASE + (i as u32));
        }
        assert_eq!(CRYPTO_MSG_MAX, CRYPTO_MSG_GETSTAT);
    }

    #[test]
    fn test_name_lengths_match_kernel() {
        assert_eq!(CRYPTO_MAX_NAME, 64);
        assert_eq!(CRYPTO_MAX_ALG_NAME, 128);
        assert_eq!(CRYPTO_MAX_DRIVER_NAME, 128);
    }

    #[test]
    fn test_cryptocfga_attrs_dense_0_to_13() {
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
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_alg_type_within_type_mask() {
        let t = [
            CRYPTO_ALG_TYPE_CIPHER,
            CRYPTO_ALG_TYPE_COMPRESS,
            CRYPTO_ALG_TYPE_AEAD,
            CRYPTO_ALG_TYPE_LSKCIPHER,
            CRYPTO_ALG_TYPE_AKCIPHER,
            CRYPTO_ALG_TYPE_SIG,
            CRYPTO_ALG_TYPE_KPP,
            CRYPTO_ALG_TYPE_ACOMPRESS,
            CRYPTO_ALG_TYPE_SHASH,
            CRYPTO_ALG_TYPE_AHASH,
        ];
        for &v in &t {
            assert_eq!(v & CRYPTO_ALG_TYPE_MASK, v);
        }
    }

    #[test]
    fn test_alg_flag_bits_distinct_single() {
        let f = [
            CRYPTO_ALG_LARVAL,
            CRYPTO_ALG_DEAD,
            CRYPTO_ALG_DYING,
            CRYPTO_ALG_ASYNC,
            CRYPTO_ALG_NEED_FALLBACK,
            CRYPTO_ALG_TESTED,
            CRYPTO_ALG_INSTANCE,
            CRYPTO_ALG_KERN_DRIVER_ONLY,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            // None of the algorithm flags overlap the low-nibble type mask.
            assert_eq!(x & CRYPTO_ALG_TYPE_MASK, 0);
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
    }
}
