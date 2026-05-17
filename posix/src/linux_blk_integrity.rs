//! `<linux/blk-integrity.h>` — Block layer data integrity constants.
//!
//! Block data integrity (DIF/DIX) provides end-to-end protection of
//! data between the application and storage device. Each sector gets
//! a protection information (PI) tuple containing a guard tag (CRC),
//! application tag, and reference tag to detect silent data corruption.

// ---------------------------------------------------------------------------
// Integrity profile types (T10-DIF)
// ---------------------------------------------------------------------------

/// Type 1: guard + app tag + ref tag (ref tag = LBA).
pub const BLK_INTEGRITY_TYPE1: u8 = 1;
/// Type 2: guard + app tag + ref tag (ref tag = seed from command).
pub const BLK_INTEGRITY_TYPE2: u8 = 2;
/// Type 3: guard + app tag only (no ref tag checking).
pub const BLK_INTEGRITY_TYPE3: u8 = 3;

// ---------------------------------------------------------------------------
// Guard tag types
// ---------------------------------------------------------------------------

/// CRC16 guard (T10-DIF standard).
pub const BLK_INTEGRITY_GUARD_CRC: u8 = 0;
/// IP checksum guard (cheaper, less protection).
pub const BLK_INTEGRITY_GUARD_IP: u8 = 1;
/// CRC64 guard (NVMe extended).
pub const BLK_INTEGRITY_GUARD_CRC64: u8 = 2;

// ---------------------------------------------------------------------------
// Integrity flags
// ---------------------------------------------------------------------------

/// Generate guard tags on write.
pub const BLK_INTEGRITY_GENERATE: u32 = 1 << 0;
/// Verify guard tags on read.
pub const BLK_INTEGRITY_VERIFY: u32 = 1 << 1;
/// Device supports DIF.
pub const BLK_INTEGRITY_DEVICE_CAPABLE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Protection information sizes
// ---------------------------------------------------------------------------

/// T10-DIF PI tuple size (8 bytes: 2 guard + 2 app + 4 ref).
pub const BLK_INTEGRITY_PI_SIZE_DIF: u8 = 8;
/// NVMe extended PI size (16 bytes: 8 guard + 2 app + 6 ref).
pub const BLK_INTEGRITY_PI_SIZE_NVME: u8 = 16;

// ---------------------------------------------------------------------------
// Interval sizes
// ---------------------------------------------------------------------------

/// 512-byte protection interval.
pub const BLK_INTEGRITY_INTERVAL_512: u16 = 512;
/// 4096-byte protection interval.
pub const BLK_INTEGRITY_INTERVAL_4096: u16 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integrity_types_distinct() {
        let types = [BLK_INTEGRITY_TYPE1, BLK_INTEGRITY_TYPE2, BLK_INTEGRITY_TYPE3];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_guard_types_distinct() {
        let guards = [
            BLK_INTEGRITY_GUARD_CRC, BLK_INTEGRITY_GUARD_IP,
            BLK_INTEGRITY_GUARD_CRC64,
        ];
        for i in 0..guards.len() {
            for j in (i + 1)..guards.len() {
                assert_ne!(guards[i], guards[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            BLK_INTEGRITY_GENERATE, BLK_INTEGRITY_VERIFY,
            BLK_INTEGRITY_DEVICE_CAPABLE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_pi_sizes() {
        assert_eq!(BLK_INTEGRITY_PI_SIZE_DIF, 8);
        assert_eq!(BLK_INTEGRITY_PI_SIZE_NVME, 16);
        assert!(BLK_INTEGRITY_PI_SIZE_DIF < BLK_INTEGRITY_PI_SIZE_NVME);
    }

    #[test]
    fn test_intervals() {
        assert!(BLK_INTEGRITY_INTERVAL_512 < BLK_INTEGRITY_INTERVAL_4096);
        assert!(BLK_INTEGRITY_INTERVAL_512.is_power_of_two());
        assert!(BLK_INTEGRITY_INTERVAL_4096.is_power_of_two());
    }
}
