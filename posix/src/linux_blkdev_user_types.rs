//! `<linux/blkdev.h>` — gendisk / block-device sysfs surface.
//!
//! This module covers the per-device sysfs tree at `/sys/block/<dev>/`
//! (partition listing, removable flag, size, ranges) plus the
//! partition-table type identifiers used by `blkid`.

// ---------------------------------------------------------------------------
// /sys/block/<dev>/ attribute names
// ---------------------------------------------------------------------------

pub const SYSFS_DEV_SIZE: &str = "size";
pub const SYSFS_DEV_RO: &str = "ro";
pub const SYSFS_DEV_REMOVABLE: &str = "removable";
pub const SYSFS_DEV_RANGE: &str = "range";
pub const SYSFS_DEV_EXT_RANGE: &str = "ext_range";
pub const SYSFS_DEV_DEV: &str = "dev";
pub const SYSFS_DEV_HIDDEN: &str = "hidden";
pub const SYSFS_DEV_ALIGNMENT_OFFSET: &str = "alignment_offset";
pub const SYSFS_DEV_CAPABILITY: &str = "capability";
pub const SYSFS_DEV_DISKSEQ: &str = "diskseq";

// ---------------------------------------------------------------------------
// gendisk flags (`GENHD_FL_*`)
// ---------------------------------------------------------------------------

pub const GENHD_FL_REMOVABLE: u32 = 1 << 0;
pub const GENHD_FL_HIDDEN: u32 = 1 << 1;
pub const GENHD_FL_NO_PART_SCAN: u32 = 1 << 2;
pub const GENHD_FL_EXT_DEVT: u32 = 1 << 3;
pub const GENHD_FL_NATIVE_CAPACITY: u32 = 1 << 4;
pub const GENHD_FL_BLOCK_EVENTS_ON_EXCL_WRITE: u32 = 1 << 5;
pub const GENHD_FL_NO_PART: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Partition-table type strings (as reported by `blkid -p -s PTTYPE`)
// ---------------------------------------------------------------------------

pub const PT_TYPE_DOS: &str = "dos";
pub const PT_TYPE_GPT: &str = "gpt";
pub const PT_TYPE_BSD: &str = "bsd";
pub const PT_TYPE_MAC: &str = "mac";
pub const PT_TYPE_SUN: &str = "sun";
pub const PT_TYPE_SGI: &str = "sgi";
pub const PT_TYPE_AIX: &str = "aix";
pub const PT_TYPE_ATARI: &str = "atari";

// ---------------------------------------------------------------------------
// Per-disk constants
// ---------------------------------------------------------------------------

/// Maximum partitions on a single gendisk (legacy MS-DOS scheme: 4
/// primary + 60 extended).
pub const DISK_MAX_PARTS: u32 = 256;

/// Maximum length of a disk name (e.g. "nvme0n1p15").
pub const DISK_NAME_LEN: usize = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_dev_attr_names_distinct() {
        let a = [
            SYSFS_DEV_SIZE,
            SYSFS_DEV_RO,
            SYSFS_DEV_REMOVABLE,
            SYSFS_DEV_RANGE,
            SYSFS_DEV_EXT_RANGE,
            SYSFS_DEV_DEV,
            SYSFS_DEV_HIDDEN,
            SYSFS_DEV_ALIGNMENT_OFFSET,
            SYSFS_DEV_CAPABILITY,
            SYSFS_DEV_DISKSEQ,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
            assert!(!x.contains('/'));
            assert!(!x.is_empty());
        }
        // ext_range extends range with a prefix.
        assert!(SYSFS_DEV_EXT_RANGE.ends_with(SYSFS_DEV_RANGE));
    }

    #[test]
    fn test_genhd_flags_each_single_bit() {
        let f = [
            GENHD_FL_REMOVABLE,
            GENHD_FL_HIDDEN,
            GENHD_FL_NO_PART_SCAN,
            GENHD_FL_EXT_DEVT,
            GENHD_FL_NATIVE_CAPACITY,
            GENHD_FL_BLOCK_EVENTS_ON_EXCL_WRITE,
            GENHD_FL_NO_PART,
        ];
        let mut or = 0u32;
        for (i, &v) in f.iter().enumerate() {
            assert!(v.is_power_of_two());
            assert_eq!(v, 1u32 << i);
            or |= v;
        }
        // 7 contiguous bits.
        assert_eq!(or, 0x7F);
    }

    #[test]
    fn test_pt_type_strings_distinct_and_lowercase() {
        let t = [
            PT_TYPE_DOS,
            PT_TYPE_GPT,
            PT_TYPE_BSD,
            PT_TYPE_MAC,
            PT_TYPE_SUN,
            PT_TYPE_SGI,
            PT_TYPE_AIX,
            PT_TYPE_ATARI,
        ];
        for (i, &x) in t.iter().enumerate() {
            for &y in &t[i + 1..] {
                assert_ne!(x, y);
            }
            // blkid-style strings are all lowercase ASCII.
            for ch in x.chars() {
                assert!(ch.is_ascii_lowercase());
            }
        }
        // The two dominant modern formats.
        assert_eq!(PT_TYPE_DOS, "dos");
        assert_eq!(PT_TYPE_GPT, "gpt");
    }

    #[test]
    fn test_disk_limits() {
        assert_eq!(DISK_MAX_PARTS, 256);
        assert_eq!(DISK_NAME_LEN, 32);
        assert!(DISK_MAX_PARTS.is_power_of_two());
        assert!(DISK_NAME_LEN.is_power_of_two());
    }
}
