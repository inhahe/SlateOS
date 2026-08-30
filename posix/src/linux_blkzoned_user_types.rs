//! `<linux/blkzoned.h>` — block-device zone-management user ioctls.
//!
//! Zoned block devices (SMR HDDs and NVMe ZNS SSDs) expose per-zone
//! state and reset operations to userspace via `BLKREPORTZONE`,
//! `BLKRESETZONE`, `BLKOPENZONE`, `BLKCLOSEZONE`, and `BLKFINISHZONE`
//! ioctls. `blkzone`, libzbd, libzns, and fs-utility tools consume
//! the constants below.

// ---------------------------------------------------------------------------
// Zone types (struct blk_zone.type)
// ---------------------------------------------------------------------------

/// Conventional zone (random writes allowed).
pub const BLK_ZONE_TYPE_CONVENTIONAL: u8 = 0x1;
/// Sequential-write-required zone.
pub const BLK_ZONE_TYPE_SEQWRITE_REQ: u8 = 0x2;
/// Sequential-write-preferred zone (host-aware).
pub const BLK_ZONE_TYPE_SEQWRITE_PREF: u8 = 0x3;

// ---------------------------------------------------------------------------
// Zone conditions (struct blk_zone.cond — 4-bit field)
// ---------------------------------------------------------------------------

/// Zone is not write-pointer-bound.
pub const BLK_ZONE_COND_NOT_WP: u8 = 0x0;
/// Zone is empty (ready to write).
pub const BLK_ZONE_COND_EMPTY: u8 = 0x1;
/// Zone is implicitly open.
pub const BLK_ZONE_COND_IMP_OPEN: u8 = 0x2;
/// Zone is explicitly open.
pub const BLK_ZONE_COND_EXP_OPEN: u8 = 0x3;
/// Zone is closed.
pub const BLK_ZONE_COND_CLOSED: u8 = 0x4;
/// Zone is read-only.
pub const BLK_ZONE_COND_READONLY: u8 = 0xd;
/// Zone is full.
pub const BLK_ZONE_COND_FULL: u8 = 0xe;
/// Zone is offline.
pub const BLK_ZONE_COND_OFFLINE: u8 = 0xf;

// ---------------------------------------------------------------------------
// Zone flags (struct blk_zone.flags / reset/open ioctl flags)
// ---------------------------------------------------------------------------

/// Reset the write pointer.
pub const BLK_ZONE_REP_CAPACITY: u32 = 1 << 0;

/// Apply the action to all zones on the device (paired with `RESET`/
/// `OPEN`/`CLOSE`/`FINISH`).
pub const BLKZONED_OP_ALL: u64 = 1 << 0;

// ---------------------------------------------------------------------------
// ioctl numbers (encoded `_IO('?', N)` or `_IOWR('?', N, ...)`)
// ---------------------------------------------------------------------------

/// `BLKREPORTZONE` — query the zone descriptors.
pub const BLKREPORTZONE: u32 = 0xC0181282;
/// `BLKRESETZONE` — reset zones to empty.
pub const BLKRESETZONE: u32 = 0x40101283;
/// `BLKGETZONESZ` — query zone size (sectors).
pub const BLKGETZONESZ: u32 = 0x80041284;
/// `BLKGETNRZONES` — query number of zones on the device.
pub const BLKGETNRZONES: u32 = 0x80041285;
/// `BLKOPENZONE` — explicit-open zones in a range.
pub const BLKOPENZONE: u32 = 0x40101286;
/// `BLKCLOSEZONE` — close zones in a range.
pub const BLKCLOSEZONE: u32 = 0x40101287;
/// `BLKFINISHZONE` — transition zones to FULL.
pub const BLKFINISHZONE: u32 = 0x40101288;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zone_types_distinct_in_4bit_field() {
        let t = [
            BLK_ZONE_TYPE_CONVENTIONAL,
            BLK_ZONE_TYPE_SEQWRITE_REQ,
            BLK_ZONE_TYPE_SEQWRITE_PREF,
        ];
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                assert_ne!(t[i], t[j]);
            }
            // Zone type lives in a 4-bit field.
            assert!(t[i] <= 0xf);
        }
    }

    #[test]
    fn test_zone_conditions_distinct_4bit() {
        let c = [
            BLK_ZONE_COND_NOT_WP,
            BLK_ZONE_COND_EMPTY,
            BLK_ZONE_COND_IMP_OPEN,
            BLK_ZONE_COND_EXP_OPEN,
            BLK_ZONE_COND_CLOSED,
            BLK_ZONE_COND_READONLY,
            BLK_ZONE_COND_FULL,
            BLK_ZONE_COND_OFFLINE,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
            assert!(c[i] <= 0xf);
        }
    }

    #[test]
    fn test_zone_flag_bits_pow2() {
        assert!(BLK_ZONE_REP_CAPACITY.is_power_of_two());
        assert!(BLKZONED_OP_ALL.is_power_of_two());
    }

    #[test]
    fn test_ioctls_distinct() {
        let i = [
            BLKREPORTZONE,
            BLKRESETZONE,
            BLKGETZONESZ,
            BLKGETNRZONES,
            BLKOPENZONE,
            BLKCLOSEZONE,
            BLKFINISHZONE,
        ];
        for x in 0..i.len() {
            for y in (x + 1)..i.len() {
                assert_ne!(i[x], i[y]);
            }
        }
    }
}
