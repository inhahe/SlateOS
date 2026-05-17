//! `<linux/zonefs.h>` — zonefs (Zoned Block Device filesystem) constants.
//!
//! zonefs is a simple filesystem exposing zones of a zoned block device
//! (ZNS SSDs, SMR HDDs) as files. Each zone is a file with sequential
//! write constraints. It provides direct access to zoned storage with
//! minimal filesystem overhead, suitable for databases and log-structured
//! applications that manage their own data placement.

// ---------------------------------------------------------------------------
// zonefs magic
// ---------------------------------------------------------------------------

/// zonefs superblock magic.
pub const ZONEFS_MAGIC: u32 = 0x5A4F_4E46;

// ---------------------------------------------------------------------------
// Zone types
// ---------------------------------------------------------------------------

/// Conventional zone (random write).
pub const ZONEFS_ZONE_CNV: u8 = 0;
/// Sequential write required zone.
pub const ZONEFS_ZONE_SEQ: u8 = 1;

// ---------------------------------------------------------------------------
// Zone conditions (from block layer)
// ---------------------------------------------------------------------------

/// Zone not write-pointer managed.
pub const BLK_ZONE_COND_NOT_WP: u8 = 0x0;
/// Zone is empty.
pub const BLK_ZONE_COND_EMPTY: u8 = 0x1;
/// Zone is implicitly open.
pub const BLK_ZONE_COND_IMP_OPEN: u8 = 0x2;
/// Zone is explicitly open.
pub const BLK_ZONE_COND_EXP_OPEN: u8 = 0x3;
/// Zone is closed.
pub const BLK_ZONE_COND_CLOSED: u8 = 0x4;
/// Zone is full.
pub const BLK_ZONE_COND_FULL: u8 = 0xE;
/// Zone is read-only.
pub const BLK_ZONE_COND_READONLY: u8 = 0xD;
/// Zone is offline.
pub const BLK_ZONE_COND_OFFLINE: u8 = 0xF;

// ---------------------------------------------------------------------------
// Zone report types
// ---------------------------------------------------------------------------

/// Report all zones.
pub const BLK_ZONE_REP_CAPACITY: u8 = 0;

// ---------------------------------------------------------------------------
// Mount options flags
// ---------------------------------------------------------------------------

/// Expose conventional zones.
pub const ZONEFS_MNTOPT_CNV: u32 = 1 << 0;
/// Enable error recovery (zone offline → file offline).
pub const ZONEFS_MNTOPT_ERRORS_REPAIR: u32 = 1 << 1;
/// Make zone files read-only on error.
pub const ZONEFS_MNTOPT_ERRORS_RO: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic() {
        assert_eq!(ZONEFS_MAGIC, 0x5A4F_4E46);
    }

    #[test]
    fn test_zone_types_distinct() {
        assert_ne!(ZONEFS_ZONE_CNV, ZONEFS_ZONE_SEQ);
    }

    #[test]
    fn test_zone_conditions_distinct() {
        let conds = [
            BLK_ZONE_COND_NOT_WP, BLK_ZONE_COND_EMPTY,
            BLK_ZONE_COND_IMP_OPEN, BLK_ZONE_COND_EXP_OPEN,
            BLK_ZONE_COND_CLOSED, BLK_ZONE_COND_FULL,
            BLK_ZONE_COND_READONLY, BLK_ZONE_COND_OFFLINE,
        ];
        for i in 0..conds.len() {
            for j in (i + 1)..conds.len() {
                assert_ne!(conds[i], conds[j]);
            }
        }
    }

    #[test]
    fn test_mount_opts_no_overlap() {
        let opts = [
            ZONEFS_MNTOPT_CNV, ZONEFS_MNTOPT_ERRORS_REPAIR,
            ZONEFS_MNTOPT_ERRORS_RO,
        ];
        for i in 0..opts.len() {
            assert!(opts[i].is_power_of_two());
            for j in (i + 1)..opts.len() {
                assert_eq!(opts[i] & opts[j], 0);
            }
        }
    }
}
