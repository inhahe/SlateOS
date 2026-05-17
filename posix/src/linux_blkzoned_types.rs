//! `<linux/blkzoned.h>` — Zoned block device constants.
//!
//! Zoned block devices (ZBDs) like Shingled Magnetic Recording (SMR)
//! HDDs and Zoned Namespaces (ZNS) NVMe SSDs organize storage into
//! sequential write zones. Data must be written sequentially within
//! each zone and the zone must be reset before rewriting. This model
//! improves storage density and SSD endurance. The kernel provides
//! IOCTLs to report zone information, manage zone states, and handle
//! zone append operations.

// ---------------------------------------------------------------------------
// Zone types
// ---------------------------------------------------------------------------

/// Conventional zone (random read/write, like regular storage).
pub const BLK_ZONE_TYPE_CONVENTIONAL: u32 = 1;
/// Sequential write required (must write sequentially).
pub const BLK_ZONE_TYPE_SEQWRITE_REQ: u32 = 2;
/// Sequential write preferred (can random write, but sequential is faster).
pub const BLK_ZONE_TYPE_SEQWRITE_PREF: u32 = 3;

// ---------------------------------------------------------------------------
// Zone conditions (states)
// ---------------------------------------------------------------------------

/// Zone not write pointer (conventional zone).
pub const BLK_ZONE_COND_NOT_WP: u32 = 0;
/// Zone is empty (no data written).
pub const BLK_ZONE_COND_EMPTY: u32 = 1;
/// Zone is implicitly opened (write started without explicit open).
pub const BLK_ZONE_COND_IMP_OPEN: u32 = 2;
/// Zone is explicitly opened.
pub const BLK_ZONE_COND_EXP_OPEN: u32 = 3;
/// Zone is closed (partially written, can be reopened).
pub const BLK_ZONE_COND_CLOSED: u32 = 4;
/// Zone is full (write pointer at end).
pub const BLK_ZONE_COND_FULL: u32 = 0xE;
/// Zone is read-only.
pub const BLK_ZONE_COND_READONLY: u32 = 0xD;
/// Zone is offline (not accessible).
pub const BLK_ZONE_COND_OFFLINE: u32 = 0xF;

// ---------------------------------------------------------------------------
// Zone IOCTLs
// ---------------------------------------------------------------------------

/// Report zones.
pub const BLKREPORTZONE: u32 = 0xC020_1282;
/// Reset zone write pointer (erase zone data).
pub const BLKRESETZONE: u32 = 0x4010_1283;
/// Open zone.
pub const BLKOPENZONE: u32 = 0x4010_1284;
/// Close zone.
pub const BLKCLOSEZONE: u32 = 0x4010_1285;
/// Finish zone (transition to full).
pub const BLKFINISHZONE: u32 = 0x4010_1286;

// ---------------------------------------------------------------------------
// Zone report flags
// ---------------------------------------------------------------------------

/// Report all zones.
pub const BLK_ZONE_REP_ALL: u32 = 0;
/// Report only empty zones.
pub const BLK_ZONE_REP_EMPTY: u32 = 1;
/// Report only open zones (implicit + explicit).
pub const BLK_ZONE_REP_OPEN: u32 = 2;
/// Report only closed zones.
pub const BLK_ZONE_REP_CLOSED: u32 = 3;
/// Report only full zones.
pub const BLK_ZONE_REP_FULL: u32 = 4;
/// Report only read-only zones.
pub const BLK_ZONE_REP_READONLY: u32 = 5;
/// Report only offline zones.
pub const BLK_ZONE_REP_OFFLINE: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zone_types_distinct() {
        let types = [
            BLK_ZONE_TYPE_CONVENTIONAL,
            BLK_ZONE_TYPE_SEQWRITE_REQ,
            BLK_ZONE_TYPE_SEQWRITE_PREF,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
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
    fn test_ioctls_distinct() {
        let ioctls = [
            BLKREPORTZONE, BLKRESETZONE, BLKOPENZONE,
            BLKCLOSEZONE, BLKFINISHZONE,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_report_filters_distinct() {
        let filters = [
            BLK_ZONE_REP_ALL, BLK_ZONE_REP_EMPTY,
            BLK_ZONE_REP_OPEN, BLK_ZONE_REP_CLOSED,
            BLK_ZONE_REP_FULL, BLK_ZONE_REP_READONLY,
            BLK_ZONE_REP_OFFLINE,
        ];
        for i in 0..filters.len() {
            for j in (i + 1)..filters.len() {
                assert_ne!(filters[i], filters[j]);
            }
        }
    }
}
