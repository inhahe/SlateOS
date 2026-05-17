//! `<linux/blk-integrity.h>` — Block integrity (DIF/DIX) constants.
//!
//! Block integrity protection (T10 DIF/DIX) adds checksums to each
//! disk sector to detect silent data corruption. A protection
//! information (PI) tuple is attached to each sector containing a
//! guard tag (CRC), application tag, and reference tag. The integrity
//! can be generated/verified by hardware (DIF), software (DIX), or
//! a combination. This catches bit-rot, firmware bugs, and transport
//! errors that would otherwise corrupt data silently.

// ---------------------------------------------------------------------------
// Integrity profile types
// ---------------------------------------------------------------------------

/// No integrity protection.
pub const BLK_INTEGRITY_NONE: u32 = 0;
/// T10 DIF Type 1 (guard + ref tag, ref tag is LBA).
pub const BLK_INTEGRITY_T10_DIF_TYPE1: u32 = 1;
/// T10 DIF Type 2 (guard + ref tag, ref tag from application).
pub const BLK_INTEGRITY_T10_DIF_TYPE2: u32 = 2;
/// T10 DIF Type 3 (guard only, no ref tag checking).
pub const BLK_INTEGRITY_T10_DIF_TYPE3: u32 = 3;
/// IP checksum (Linux software-only integrity).
pub const BLK_INTEGRITY_IP_CHECKSUM: u32 = 4;
/// CRC-64 (NVMe enhanced integrity).
pub const BLK_INTEGRITY_CRC64: u32 = 5;

// ---------------------------------------------------------------------------
// Integrity flags
// ---------------------------------------------------------------------------

/// Generate integrity data (on write path).
pub const BLK_INTEGRITY_GENERATE: u32 = 0x01;
/// Verify integrity data (on read path).
pub const BLK_INTEGRITY_VERIFY: u32 = 0x02;
/// Device can transfer integrity data inline.
pub const BLK_INTEGRITY_DEVICE_CAPABLE: u32 = 0x04;
/// Integrity metadata is interleaved with data.
pub const BLK_INTEGRITY_IP_CHECKSUM_FLAG: u32 = 0x08;

// ---------------------------------------------------------------------------
// Protection information tuple sizes
// ---------------------------------------------------------------------------

/// T10 DIF protection info size (8 bytes per sector).
pub const PI_TUPLE_SIZE_DIF: u32 = 8;
/// NVMe PI (CRC-64) size (16 bytes per sector).
pub const PI_TUPLE_SIZE_CRC64: u32 = 16;

// ---------------------------------------------------------------------------
// Guard tag types
// ---------------------------------------------------------------------------

/// CRC-16 guard tag (T10 DIF standard).
pub const PI_GUARD_CRC16: u32 = 0;
/// IP checksum guard tag.
pub const PI_GUARD_IP: u32 = 1;
/// CRC-64 guard tag (NVMe).
pub const PI_GUARD_CRC64: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_types_distinct() {
        let types = [
            BLK_INTEGRITY_NONE, BLK_INTEGRITY_T10_DIF_TYPE1,
            BLK_INTEGRITY_T10_DIF_TYPE2, BLK_INTEGRITY_T10_DIF_TYPE3,
            BLK_INTEGRITY_IP_CHECKSUM, BLK_INTEGRITY_CRC64,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            BLK_INTEGRITY_GENERATE, BLK_INTEGRITY_VERIFY,
            BLK_INTEGRITY_DEVICE_CAPABLE, BLK_INTEGRITY_IP_CHECKSUM_FLAG,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_tuple_sizes() {
        assert!(PI_TUPLE_SIZE_DIF > 0);
        assert!(PI_TUPLE_SIZE_CRC64 > PI_TUPLE_SIZE_DIF);
    }

    #[test]
    fn test_guard_types_distinct() {
        let guards = [PI_GUARD_CRC16, PI_GUARD_IP, PI_GUARD_CRC64];
        for i in 0..guards.len() {
            for j in (i + 1)..guards.len() {
                assert_ne!(guards[i], guards[j]);
            }
        }
    }
}
