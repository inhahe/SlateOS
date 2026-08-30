//! `<linux/zonefs.h>` — Zonefs, a tiny filesystem for zoned block devices.
//!
//! Zonefs exposes each zone of a ZNS SSD or SMR HDD as a fixed-size
//! file under `cnv/` (conventional zones), `seq/` (sequential write
//! zones). It is the kernel's officially blessed way to expose raw
//! zones to userspace databases (RocksDB, Ceph, NVMe-oF targets).

// ---------------------------------------------------------------------------
// Filesystem name and magic
// ---------------------------------------------------------------------------

pub const ZONEFS_FS_NAME: &str = "zonefs";

/// `statfs.f_type` for zonefs (chosen by the maintainers; not a printable
/// ASCII fourcc).
pub const ZONEFS_MAGIC: u32 = 0x5A4F_4653; // "ZOFS" big-endian

// ---------------------------------------------------------------------------
// Mount points carved out per zone type
// ---------------------------------------------------------------------------

pub const ZONEFS_DIR_CNV: &str = "cnv";
pub const ZONEFS_DIR_SEQ: &str = "seq";

// ---------------------------------------------------------------------------
// Mount options accepted by zonefs
// ---------------------------------------------------------------------------

pub const ZONEFS_OPT_ERRORS_REMOUNT_RO: &str = "errors=remount-ro";
pub const ZONEFS_OPT_ERRORS_ZONE_RO: &str = "errors=zone-ro";
pub const ZONEFS_OPT_ERRORS_ZONE_OFFLINE: &str = "errors=zone-offline";
pub const ZONEFS_OPT_ERRORS_REPAIR: &str = "errors=repair";
pub const ZONEFS_OPT_EXPLICIT_OPEN: &str = "explicit-open";
pub const ZONEFS_OPT_NO_EXPLICIT_OPEN: &str = "noexplicit-open";

// ---------------------------------------------------------------------------
// Zone condition states (from `enum blk_zone_cond` in `<linux/blkzoned.h>`,
// re-exposed by zonefs over sysfs)
// ---------------------------------------------------------------------------

pub const BLK_ZONE_COND_NOT_WP: u32 = 0;
pub const BLK_ZONE_COND_EMPTY: u32 = 1;
pub const BLK_ZONE_COND_IMP_OPEN: u32 = 2;
pub const BLK_ZONE_COND_EXP_OPEN: u32 = 3;
pub const BLK_ZONE_COND_CLOSED: u32 = 4;
pub const BLK_ZONE_COND_READONLY: u32 = 13;
pub const BLK_ZONE_COND_FULL: u32 = 14;
pub const BLK_ZONE_COND_OFFLINE: u32 = 15;

// ---------------------------------------------------------------------------
// Per-zone sysfs attributes (under `/sys/fs/zonefs/<dev>/`)
// ---------------------------------------------------------------------------

pub const ZONEFS_ATTR_MAX_WRO_SEQ_FILES: &str = "max_wro_seq_files";
pub const ZONEFS_ATTR_MAX_ACTIVE_SEQ_FILES: &str = "max_active_seq_files";
pub const ZONEFS_ATTR_NR_FILES_CNV: &str = "nr_files_cnv";
pub const ZONEFS_ATTR_NR_FILES_SEQ: &str = "nr_files_seq";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_name_and_magic() {
        assert_eq!(ZONEFS_FS_NAME, "zonefs");
        // Magic encodes "ZOFS" in big-endian byte order.
        assert_eq!(ZONEFS_MAGIC.to_be_bytes(), *b"ZOFS");
    }

    #[test]
    fn test_dir_names_short_and_distinct() {
        assert_eq!(ZONEFS_DIR_CNV, "cnv");
        assert_eq!(ZONEFS_DIR_SEQ, "seq");
        assert_ne!(ZONEFS_DIR_CNV, ZONEFS_DIR_SEQ);
        // 3-character names — match the kernel's choice for brevity.
        assert_eq!(ZONEFS_DIR_CNV.len(), 3);
        assert_eq!(ZONEFS_DIR_SEQ.len(), 3);
    }

    #[test]
    fn test_mount_options_distinct() {
        let o = [
            ZONEFS_OPT_ERRORS_REMOUNT_RO,
            ZONEFS_OPT_ERRORS_ZONE_RO,
            ZONEFS_OPT_ERRORS_ZONE_OFFLINE,
            ZONEFS_OPT_ERRORS_REPAIR,
            ZONEFS_OPT_EXPLICIT_OPEN,
            ZONEFS_OPT_NO_EXPLICIT_OPEN,
        ];
        for i in 0..o.len() {
            for j in (i + 1)..o.len() {
                assert_ne!(o[i], o[j]);
            }
        }
        // The four error= variants share a common prefix.
        for v in &o[..4] {
            assert!(v.starts_with("errors="));
        }
    }

    #[test]
    fn test_zone_cond_values_match_blkzoned_h() {
        // Low values are the "active" states (0..4); high values (13..15)
        // are terminal states. There is a deliberate gap at 5..12 that
        // the kernel reserves.
        assert_eq!(BLK_ZONE_COND_NOT_WP, 0);
        assert_eq!(BLK_ZONE_COND_EMPTY, 1);
        assert_eq!(BLK_ZONE_COND_IMP_OPEN, 2);
        assert_eq!(BLK_ZONE_COND_EXP_OPEN, 3);
        assert_eq!(BLK_ZONE_COND_CLOSED, 4);
        assert_eq!(BLK_ZONE_COND_READONLY, 13);
        assert_eq!(BLK_ZONE_COND_FULL, 14);
        assert_eq!(BLK_ZONE_COND_OFFLINE, 15);
        // All fit in a nibble.
        assert!(BLK_ZONE_COND_OFFLINE < 16);
    }

    #[test]
    fn test_sysfs_attrs_distinct() {
        let a = [
            ZONEFS_ATTR_MAX_WRO_SEQ_FILES,
            ZONEFS_ATTR_MAX_ACTIVE_SEQ_FILES,
            ZONEFS_ATTR_NR_FILES_CNV,
            ZONEFS_ATTR_NR_FILES_SEQ,
        ];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
    }
}
