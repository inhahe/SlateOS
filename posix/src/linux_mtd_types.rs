//! `<linux/mtd/mtd-abi.h>` — Memory Technology Device (MTD) constants.
//!
//! MTD provides a common interface for raw flash memory devices
//! (NOR flash, NAND flash, DataFlash, etc.). Unlike block devices,
//! MTD exposes flash-specific operations: erase (in blocks), write
//! (in pages), and read. Used by JFFS2, UBIFS, and flash management
//! tools (mtd-utils).

// ---------------------------------------------------------------------------
// MTD type flags
// ---------------------------------------------------------------------------

/// Absent/missing MTD device.
pub const MTD_ABSENT: u32 = 0;
/// RAM-backed (battery-backed SRAM, etc.).
pub const MTD_RAM: u32 = 1;
/// ROM (read-only, no erase needed).
pub const MTD_ROM: u32 = 2;
/// NOR flash.
pub const MTD_NORFLASH: u32 = 3;
/// NAND flash.
pub const MTD_NANDFLASH: u32 = 4;
/// DataFlash (SPI-connected serial flash).
pub const MTD_DATAFLASH: u32 = 6;
/// UBI volume (virtual MTD over UBI).
pub const MTD_UBIVOLUME: u32 = 7;
/// MLC NAND flash.
pub const MTD_MLCNANDFLASH: u32 = 8;

// ---------------------------------------------------------------------------
// MTD flags
// ---------------------------------------------------------------------------

/// Device is writeable.
pub const MTD_WRITEABLE: u32 = 0x0400;
/// Device has bitflip correction (ECC).
pub const MTD_BIT_WRITEABLE: u32 = 0x0800;
/// No erase needed before write.
pub const MTD_NO_ERASE: u32 = 0x1000;
/// Power-loss safe writes (FTL or journaling).
pub const MTD_POWERUP_LOCK: u32 = 0x2000;

// ---------------------------------------------------------------------------
// MTD ioctl commands
// ---------------------------------------------------------------------------

/// Get MTD device info.
pub const MEMGETINFO: u32 = 0x8020_4D01;
/// Erase a region.
pub const MEMERASE: u32 = 0x4008_4D02;
/// Write OOB (out-of-band) data.
pub const MEMWRITEOOB: u32 = 0xC010_4D03;
/// Read OOB data.
pub const MEMREADOOB: u32 = 0xC010_4D04;
/// Lock a region (write protect).
pub const MEMLOCK: u32 = 0x4008_4D05;
/// Unlock a region.
pub const MEMUNLOCK: u32 = 0x4008_4D06;
/// Get region count.
pub const MEMGETREGIONCOUNT: u32 = 0x8004_4D07;
/// Get region info.
pub const MEMGETREGIONINFO: u32 = 0xC010_4D08;
/// Check if region is locked.
pub const MEMISLOCKED: u32 = 0x4008_4D17;
/// Get bad block table.
pub const MEMGETBADBLOCK: u32 = 0x4008_4D0B;
/// Mark a block as bad.
pub const MEMSETBADBLOCK: u32 = 0x4008_4D0C;
/// Erase (64-bit offset version).
pub const MEMERASE64: u32 = 0x4010_4D14;

// ---------------------------------------------------------------------------
// OOB modes
// ---------------------------------------------------------------------------

/// Place OOB data in the standard location.
pub const MTD_OPS_PLACE_OOB: u32 = 0;
/// Auto-place OOB using ECC layout.
pub const MTD_OPS_AUTO_OOB: u32 = 1;
/// Raw OOB access (no ECC processing).
pub const MTD_OPS_RAW: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_types_distinct() {
        let types = [
            MTD_ABSENT, MTD_RAM, MTD_ROM, MTD_NORFLASH,
            MTD_NANDFLASH, MTD_DATAFLASH, MTD_UBIVOLUME,
            MTD_MLCNANDFLASH,
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
            MTD_WRITEABLE, MTD_BIT_WRITEABLE,
            MTD_NO_ERASE, MTD_POWERUP_LOCK,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            MEMGETINFO, MEMERASE, MEMWRITEOOB, MEMREADOOB,
            MEMLOCK, MEMUNLOCK, MEMGETREGIONCOUNT,
            MEMGETREGIONINFO, MEMISLOCKED, MEMGETBADBLOCK,
            MEMSETBADBLOCK, MEMERASE64,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_oob_modes_distinct() {
        let modes = [MTD_OPS_PLACE_OOB, MTD_OPS_AUTO_OOB, MTD_OPS_RAW];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
