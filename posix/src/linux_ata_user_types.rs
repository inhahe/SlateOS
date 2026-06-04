//! `<linux/ata.h>` — ATA/SATA command codes and Task File register layout.
//!
//! The Linux ATA layer exposes raw ATA passthrough through SG_IO and
//! `/dev/sg*` for tools like hdparm, smartctl, and sg3_utils. This
//! module gathers the command opcodes those tools issue.

// ---------------------------------------------------------------------------
// Block / sector geometry
// ---------------------------------------------------------------------------

pub const ATA_SECT_SIZE: usize = 512;
pub const ATA_ID_WORDS: usize = 256;
pub const ATA_ID_SIZE: usize = ATA_ID_WORDS * 2; // bytes

// ---------------------------------------------------------------------------
// ATA command opcodes (subset commonly issued from userspace)
// ---------------------------------------------------------------------------

pub const ATA_CMD_CHK_POWER: u8 = 0xE5;
pub const ATA_CMD_STANDBY: u8 = 0xE2;
pub const ATA_CMD_STANDBYNOW1: u8 = 0xE0;
pub const ATA_CMD_IDLE: u8 = 0xE3;
pub const ATA_CMD_IDLEIMMEDIATE: u8 = 0xE1;
pub const ATA_CMD_SLEEP: u8 = 0xE6;

pub const ATA_CMD_ID_ATA: u8 = 0xEC;
pub const ATA_CMD_ID_ATAPI: u8 = 0xA1;
pub const ATA_CMD_PACKET: u8 = 0xA0;

pub const ATA_CMD_READ: u8 = 0x20;
pub const ATA_CMD_READ_EXT: u8 = 0x24;
pub const ATA_CMD_WRITE: u8 = 0x30;
pub const ATA_CMD_WRITE_EXT: u8 = 0x34;
pub const ATA_CMD_FLUSH: u8 = 0xE7;
pub const ATA_CMD_FLUSH_EXT: u8 = 0xEA;
pub const ATA_CMD_DSM: u8 = 0x06; // TRIM uses this with feature=0x01

pub const ATA_CMD_SMART: u8 = 0xB0;
pub const ATA_CMD_SET_FEATURES: u8 = 0xEF;
pub const ATA_CMD_SEC_ERASE_UNIT: u8 = 0xF4;
pub const ATA_CMD_SEC_ERASE_PREP: u8 = 0xF3;

// ---------------------------------------------------------------------------
// SMART subcommand `feature` values (with `ATA_CMD_SMART`)
// ---------------------------------------------------------------------------

pub const SMART_READ_VALUES: u8 = 0xD0;
pub const SMART_READ_THRESHOLDS: u8 = 0xD1;
pub const SMART_AUTOSAVE: u8 = 0xD2;
pub const SMART_SAVE: u8 = 0xD3;
pub const SMART_IMMEDIATE_OFFLINE: u8 = 0xD4;
pub const SMART_READ_LOG: u8 = 0xD5;
pub const SMART_WRITE_LOG: u8 = 0xD6;
pub const SMART_ENABLE: u8 = 0xD8;
pub const SMART_DISABLE: u8 = 0xD9;
pub const SMART_STATUS: u8 = 0xDA;

/// Cylinder-high/low signature that confirms SMART_STATUS "OK".
pub const SMART_LBA_MID_OK: u8 = 0x4F;
pub const SMART_LBA_HIGH_OK: u8 = 0xC2;
/// Failure signature.
pub const SMART_LBA_MID_FAIL: u8 = 0xF4;
pub const SMART_LBA_HIGH_FAIL: u8 = 0x2C;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sector_and_id_sizes() {
        assert_eq!(ATA_SECT_SIZE, 512);
        assert_eq!(ATA_ID_WORDS, 256);
        assert_eq!(ATA_ID_SIZE, 512);
        // IDENTIFY transfer fills exactly one sector.
        assert_eq!(ATA_ID_SIZE, ATA_SECT_SIZE);
    }

    #[test]
    fn test_power_management_opcodes_in_0xE0_range() {
        let p = [
            ATA_CMD_CHK_POWER,
            ATA_CMD_STANDBY,
            ATA_CMD_STANDBYNOW1,
            ATA_CMD_IDLE,
            ATA_CMD_IDLEIMMEDIATE,
            ATA_CMD_SLEEP,
        ];
        for v in p {
            assert_eq!(v & 0xF0, 0xE0);
        }
        // Pairs that mean "now" end in 0/1, the "deferred" forms in 2/3.
        assert_eq!(ATA_CMD_STANDBYNOW1, 0xE0);
        assert_eq!(ATA_CMD_STANDBY, 0xE2);
        assert_eq!(ATA_CMD_IDLEIMMEDIATE, 0xE1);
        assert_eq!(ATA_CMD_IDLE, 0xE3);
    }

    #[test]
    fn test_read_write_pairs_offset_by_0x10() {
        // WRITE = READ + 0x10 (both 28-bit and EXT 48-bit forms).
        assert_eq!(ATA_CMD_WRITE - ATA_CMD_READ, 0x10);
        assert_eq!(ATA_CMD_WRITE_EXT - ATA_CMD_READ_EXT, 0x10);
        // EXT variant adds 0x04 to the base opcode.
        assert_eq!(ATA_CMD_READ_EXT - ATA_CMD_READ, 0x04);
        assert_eq!(ATA_CMD_WRITE_EXT - ATA_CMD_WRITE, 0x04);
    }

    #[test]
    fn test_flush_pair_and_dsm() {
        // FLUSH = 0xE7, FLUSH_EXT = 0xEA (not strict +0x04 — historical).
        assert_eq!(ATA_CMD_FLUSH, 0xE7);
        assert_eq!(ATA_CMD_FLUSH_EXT, 0xEA);
        // DSM (TRIM) is a single byte 0x06.
        assert_eq!(ATA_CMD_DSM, 0x06);
    }

    #[test]
    fn test_smart_subcmds_in_0xD_block() {
        let s = [
            SMART_READ_VALUES,
            SMART_READ_THRESHOLDS,
            SMART_AUTOSAVE,
            SMART_SAVE,
            SMART_IMMEDIATE_OFFLINE,
            SMART_READ_LOG,
            SMART_WRITE_LOG,
            SMART_ENABLE,
            SMART_DISABLE,
            SMART_STATUS,
        ];
        for v in s {
            assert_eq!(v & 0xF0, 0xD0);
        }
        // ENABLE/DISABLE form a pair.
        assert_eq!(SMART_DISABLE, SMART_ENABLE + 1);
    }

    #[test]
    fn test_smart_status_signature_pair_disjoint() {
        // OK signature: (mid=0x4F, high=0xC2). Fail: (mid=0xF4, high=0x2C).
        assert_ne!(SMART_LBA_MID_OK, SMART_LBA_MID_FAIL);
        assert_ne!(SMART_LBA_HIGH_OK, SMART_LBA_HIGH_FAIL);
        // Fail signature is the OK signature's bytes swapped — historical convention.
        assert_eq!(SMART_LBA_MID_OK, SMART_LBA_HIGH_FAIL ^ 0x63);
    }
}
