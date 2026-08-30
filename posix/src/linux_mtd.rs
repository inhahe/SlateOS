//! `<linux/mtd/mtd-abi.h>` — Memory Technology Device constants.
//!
//! MTD provides a uniform interface to raw flash memory (NOR, NAND,
//! DataFlash). Unlike block devices, MTD exposes erase blocks and
//! write-page granularity. Used by JFFS2, UBIFS, and flashrom.

// ---------------------------------------------------------------------------
// MTD device types
// ---------------------------------------------------------------------------

/// Absent/none.
pub const MTD_ABSENT: u8 = 0;
/// RAM-like (battery-backed SRAM).
pub const MTD_RAM: u8 = 1;
/// ROM.
pub const MTD_ROM: u8 = 2;
/// NOR flash.
pub const MTD_NORFLASH: u8 = 3;
/// NAND flash.
pub const MTD_NANDFLASH: u8 = 4;
/// DataFlash (serial NOR).
pub const MTD_DATAFLASH: u8 = 6;
/// UBI volume.
pub const MTD_UBIVOLUME: u8 = 7;
/// MLC NAND.
pub const MTD_MLCNANDFLASH: u8 = 8;

// ---------------------------------------------------------------------------
// MTD flags
// ---------------------------------------------------------------------------

/// Device is writeable.
pub const MTD_WRITEABLE: u32 = 0x400;
/// Device has bit-flippable region.
pub const MTD_BIT_WRITEABLE: u32 = 0x800;
/// Device has no erase function.
pub const MTD_NO_ERASE: u32 = 0x1000;
/// Power-up lock supported.
pub const MTD_POWERUP_LOCK: u32 = 0x2000;
/// SLC on MLC NAND emulation.
pub const MTD_SLC_ON_MLC_EMULATION: u32 = 0x4000;

// ---------------------------------------------------------------------------
// OOB (Out-of-Band) modes
// ---------------------------------------------------------------------------

/// Place OOB automatically.
pub const MTD_OPS_PLACE_OOB: u32 = 0;
/// Auto OOB placement.
pub const MTD_OPS_AUTO_OOB: u32 = 1;
/// Raw (no ECC) access.
pub const MTD_OPS_RAW: u32 = 2;

// ---------------------------------------------------------------------------
// MTD ioctl commands
// ---------------------------------------------------------------------------

/// Get MTD info.
pub const MEMGETINFO: u32 = 0x8020_4D01;
/// Erase segment.
pub const MEMERASE: u32 = 0x4008_4D02;
/// Write OOB data.
pub const MEMWRITEOOB: u32 = 0xC010_4D03;
/// Read OOB data.
pub const MEMREADOOB: u32 = 0xC010_4D04;
/// Lock segment.
pub const MEMLOCK: u32 = 0x4008_4D05;
/// Unlock segment.
pub const MEMUNLOCK: u32 = 0x4008_4D06;
/// Get region count.
pub const MEMGETREGIONCOUNT: u32 = 0x8004_4D07;
/// Get region info.
pub const MEMGETREGIONINFO: u32 = 0xC010_4D08;
/// Get OOB size.
pub const MEMGETOOBSEL: u32 = 0x80C8_4D0A;
/// Check if block is bad.
pub const MEMGETBADBLOCK: u32 = 0x4008_4D0B;
/// Mark block as bad.
pub const MEMSETBADBLOCK: u32 = 0x4008_4D0C;
/// Write OOB data (64-bit version).
pub const MEMWRITEOOB64: u32 = 0xC018_4D03;
/// Read OOB data (64-bit version).
pub const MEMREADOOB64: u32 = 0xC018_4D04;
/// Erase (64-bit version).
pub const MEMERASE64: u32 = 0x4010_4D14;
/// Is partition locked.
pub const MEMISLOCKED: u32 = 0x4008_4D17;

// ---------------------------------------------------------------------------
// ECC stats
// ---------------------------------------------------------------------------

/// No ECC.
pub const MTD_ECC_NONE: u32 = 0;
/// Software ECC.
pub const MTD_ECC_SW: u32 = 1;
/// Hardware ECC.
pub const MTD_ECC_HW: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_types_distinct() {
        let types = [
            MTD_ABSENT,
            MTD_RAM,
            MTD_ROM,
            MTD_NORFLASH,
            MTD_NANDFLASH,
            MTD_DATAFLASH,
            MTD_UBIVOLUME,
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
            MTD_WRITEABLE,
            MTD_BIT_WRITEABLE,
            MTD_NO_ERASE,
            MTD_POWERUP_LOCK,
            MTD_SLC_ON_MLC_EMULATION,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(
                    flags[i] & flags[j],
                    0,
                    "overlap: 0x{:x} & 0x{:x}",
                    flags[i],
                    flags[j]
                );
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

    #[test]
    fn test_ecc_modes_distinct() {
        let modes = [MTD_ECC_NONE, MTD_ECC_SW, MTD_ECC_HW];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_type_values() {
        assert_eq!(MTD_ABSENT, 0);
        assert_eq!(MTD_NORFLASH, 3);
        assert_eq!(MTD_NANDFLASH, 4);
    }

    #[test]
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            MEMGETINFO,
            MEMERASE,
            MEMWRITEOOB,
            MEMREADOOB,
            MEMLOCK,
            MEMUNLOCK,
            MEMGETREGIONCOUNT,
            MEMGETREGIONINFO,
            MEMGETOOBSEL,
            MEMGETBADBLOCK,
            MEMSETBADBLOCK,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}
