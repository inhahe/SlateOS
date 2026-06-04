//! `<linux/blk-integrity.h>` — block-layer data-integrity extensions
//! (T10 PI / DIF).
//!
//! Storage devices that implement T10 Protection Information stash a
//! per-sector tuple (guard / app-tag / reference-tag) alongside the
//! payload. The block layer exposes this through `bio_integrity`
//! ioctls and per-device sysfs files.

// ---------------------------------------------------------------------------
// Protection profiles (DIF types 0..3)
// ---------------------------------------------------------------------------

pub const BLK_INTEGRITY_T10_DIF_TYPE0: u8 = 0;
pub const BLK_INTEGRITY_T10_DIF_TYPE1: u8 = 1;
pub const BLK_INTEGRITY_T10_DIF_TYPE2: u8 = 2;
pub const BLK_INTEGRITY_T10_DIF_TYPE3: u8 = 3;

// ---------------------------------------------------------------------------
// Integrity feature flags
// ---------------------------------------------------------------------------

pub const BLK_INTEGRITY_VERIFY: u32 = 1 << 0;
pub const BLK_INTEGRITY_GENERATE: u32 = 1 << 1;
pub const BLK_INTEGRITY_DEVICE_CAPABLE: u32 = 1 << 2;
pub const BLK_INTEGRITY_REF_TAG: u32 = 1 << 3;
pub const BLK_INTEGRITY_STAMP_SECTOR: u32 = 1 << 4;
pub const BLK_INTEGRITY_NOGENERATE: u32 = 1 << 5;
pub const BLK_INTEGRITY_NOVERIFY: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// PI tuple field sizes (bytes per protected sector)
// ---------------------------------------------------------------------------

/// Guard field (CRC over payload + IP-style checksum, depending on profile).
pub const BLK_INTEGRITY_GUARD_SIZE: usize = 2;
/// Application tag — host-set 2-byte label.
pub const BLK_INTEGRITY_APP_TAG_SIZE: usize = 2;
/// Reference tag — incrementing LBA-derived 4-byte stamp.
pub const BLK_INTEGRITY_REF_TAG_SIZE: usize = 4;
/// Total per-sector PI tuple size.
pub const BLK_INTEGRITY_TUPLE_SIZE: usize =
    BLK_INTEGRITY_GUARD_SIZE + BLK_INTEGRITY_APP_TAG_SIZE + BLK_INTEGRITY_REF_TAG_SIZE;

// ---------------------------------------------------------------------------
// Guard algorithm identifiers
// ---------------------------------------------------------------------------

pub const BLK_INTEGRITY_CSUM_NONE: u8 = 0;
pub const BLK_INTEGRITY_CSUM_IP: u8 = 1;
pub const BLK_INTEGRITY_CSUM_CRC: u8 = 2;
pub const BLK_INTEGRITY_CSUM_CRC64: u8 = 3;

// ---------------------------------------------------------------------------
// sysfs attribute names (under `/sys/block/<dev>/integrity/`)
// ---------------------------------------------------------------------------

pub const SYSFS_INTEGRITY_FORMAT: &str = "format";
pub const SYSFS_INTEGRITY_READ_VERIFY: &str = "read_verify";
pub const SYSFS_INTEGRITY_WRITE_GENERATE: &str = "write_generate";
pub const SYSFS_INTEGRITY_DEVICE_IS_INTEGRITY_CAPABLE: &str =
    "device_is_integrity_capable";
pub const SYSFS_INTEGRITY_PROTECTION_INTERVAL_BYTES: &str =
    "protection_interval_bytes";
pub const SYSFS_INTEGRITY_TAG_SIZE: &str = "tag_size";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dif_types_dense_0_to_3() {
        let t = [
            BLK_INTEGRITY_T10_DIF_TYPE0,
            BLK_INTEGRITY_T10_DIF_TYPE1,
            BLK_INTEGRITY_T10_DIF_TYPE2,
            BLK_INTEGRITY_T10_DIF_TYPE3,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // TYPE0 is the "no PI" profile.
        assert_eq!(BLK_INTEGRITY_T10_DIF_TYPE0, 0);
    }

    #[test]
    fn test_feature_flag_bits_each_single_bit() {
        let f = [
            BLK_INTEGRITY_VERIFY,
            BLK_INTEGRITY_GENERATE,
            BLK_INTEGRITY_DEVICE_CAPABLE,
            BLK_INTEGRITY_REF_TAG,
            BLK_INTEGRITY_STAMP_SECTOR,
            BLK_INTEGRITY_NOGENERATE,
            BLK_INTEGRITY_NOVERIFY,
        ];
        let mut or = 0;
        for &v in &f {
            assert!(v.is_power_of_two());
            or |= v;
        }
        // 7 contiguous low bits.
        assert_eq!(or, 0x7F);
    }

    #[test]
    fn test_pi_tuple_size_is_eight() {
        assert_eq!(BLK_INTEGRITY_GUARD_SIZE, 2);
        assert_eq!(BLK_INTEGRITY_APP_TAG_SIZE, 2);
        assert_eq!(BLK_INTEGRITY_REF_TAG_SIZE, 4);
        // T10 PI tuple is always 8 bytes per protected interval.
        assert_eq!(BLK_INTEGRITY_TUPLE_SIZE, 8);
    }

    #[test]
    fn test_csum_algorithms_dense_0_to_3() {
        let c = [
            BLK_INTEGRITY_CSUM_NONE,
            BLK_INTEGRITY_CSUM_IP,
            BLK_INTEGRITY_CSUM_CRC,
            BLK_INTEGRITY_CSUM_CRC64,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // NONE=0 means "no integrity computation".
        assert_eq!(BLK_INTEGRITY_CSUM_NONE, 0);
    }

    #[test]
    fn test_sysfs_attribute_names_distinct() {
        let a = [
            SYSFS_INTEGRITY_FORMAT,
            SYSFS_INTEGRITY_READ_VERIFY,
            SYSFS_INTEGRITY_WRITE_GENERATE,
            SYSFS_INTEGRITY_DEVICE_IS_INTEGRITY_CAPABLE,
            SYSFS_INTEGRITY_PROTECTION_INTERVAL_BYTES,
            SYSFS_INTEGRITY_TAG_SIZE,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
            assert!(!x.contains('/'));
        }
        // read_verify / write_generate form a complementary pair.
        assert!(SYSFS_INTEGRITY_READ_VERIFY.starts_with("read_"));
        assert!(SYSFS_INTEGRITY_WRITE_GENERATE.starts_with("write_"));
    }

    #[test]
    fn test_nogenerate_and_noverify_oppose_generate_verify() {
        // NOGENERATE / NOVERIFY are mutually exclusive with GENERATE /
        // VERIFY when set on the same device; they share no bits.
        assert_eq!(BLK_INTEGRITY_VERIFY & BLK_INTEGRITY_NOVERIFY, 0);
        assert_eq!(BLK_INTEGRITY_GENERATE & BLK_INTEGRITY_NOGENERATE, 0);
        // NOVERIFY sits one bit above NOGENERATE.
        assert_eq!(BLK_INTEGRITY_NOVERIFY, BLK_INTEGRITY_NOGENERATE << 1);
    }
}
