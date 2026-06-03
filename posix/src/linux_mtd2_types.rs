//! `<linux/mtd/mtd-abi.h>` — Additional MTD (Memory Technology Device) constants.
//!
//! Supplementary MTD constants covering device types,
//! flash flags, OTP modes, and NAND ECC modes.

// ---------------------------------------------------------------------------
// MTD device types
// ---------------------------------------------------------------------------

/// Absent device.
pub const MTD_ABSENT: u8 = 0;
/// RAM device.
pub const MTD_RAM: u8 = 1;
/// ROM device.
pub const MTD_ROM: u8 = 2;
/// NOR flash.
pub const MTD_NORFLASH: u8 = 3;
/// NAND flash.
pub const MTD_NANDFLASH: u8 = 4;
/// Dataflash.
pub const MTD_DATAFLASH: u8 = 6;
/// UBI volume (virtual).
pub const MTD_UBIVOLUME: u8 = 7;
/// MLC NAND flash.
pub const MTD_MLCNANDFLASH: u8 = 8;

// ---------------------------------------------------------------------------
// MTD flags
// ---------------------------------------------------------------------------

/// Device is writable.
pub const MTD_WRITEABLE: u32 = 0x400;
/// Device supports bit-flips.
pub const MTD_BIT_WRITEABLE: u32 = 0x800;
/// No erase needed.
pub const MTD_NO_ERASE: u32 = 0x1000;
/// Power-up lock.
pub const MTD_POWERUP_LOCK: u32 = 0x2000;
/// SLC-on-MLC mode.
pub const MTD_SLC_ON_MLC_EMULATION: u32 = 0x4000;

// ---------------------------------------------------------------------------
// OTP modes
// ---------------------------------------------------------------------------

/// Factory OTP.
pub const MTD_OTP_FACTORY: u32 = 1;
/// User OTP.
pub const MTD_OTP_USER: u32 = 2;
/// OTP off.
pub const MTD_OTP_OFF: u32 = 0;

// ---------------------------------------------------------------------------
// NAND ECC modes
// ---------------------------------------------------------------------------

/// No ECC.
pub const MTD_NANDECC_OFF: u32 = 0;
/// Place ECC.
pub const MTD_NANDECC_PLACE: u32 = 1;
/// Auto-place ECC.
pub const MTD_NANDECC_AUTOPLACE: u32 = 2;
/// Place ECC only.
pub const MTD_NANDECC_PLACEONLY: u32 = 3;
/// Auto OOB.
pub const MTD_NANDECC_AUTOPL_USR: u32 = 4;

// ---------------------------------------------------------------------------
// MTD IOCTL commands
// ---------------------------------------------------------------------------

/// Get MTD info.
pub const MEMGETINFO: u32 = 0x80204D01;
/// Erase.
pub const MEMERASE: u32 = 0x40084D02;
/// Write OOB.
pub const MEMWRITEOOB: u32 = 0xC00C4D03;
/// Read OOB.
pub const MEMREADOOB: u32 = 0xC00C4D04;
/// Lock.
pub const MEMLOCK: u32 = 0x40084D05;
/// Unlock.
pub const MEMUNLOCK: u32 = 0x40084D06;
/// Get region count.
pub const MEMGETREGIONCOUNT: u32 = 0x80044D07;
/// Get bad block.
pub const MEMGETBADBLOCK: u32 = 0x40084D0B;
/// Set bad block.
pub const MEMSETBADBLOCK: u32 = 0x40084D0C;
/// Is locked.
pub const MEMISLOCKED: u32 = 0x40084D17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_types_distinct() {
        let types: [u8; 8] = [
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
    fn test_flags_power_of_two() {
        let flags = [
            MTD_WRITEABLE,
            MTD_BIT_WRITEABLE,
            MTD_NO_ERASE,
            MTD_POWERUP_LOCK,
            MTD_SLC_ON_MLC_EMULATION,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:04x} not power of two", f);
        }
    }

    #[test]
    fn test_otp_modes_distinct() {
        let modes = [MTD_OTP_OFF, MTD_OTP_FACTORY, MTD_OTP_USER];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_ecc_modes_sequential() {
        assert_eq!(MTD_NANDECC_OFF, 0);
        assert_eq!(MTD_NANDECC_PLACE, 1);
        assert_eq!(MTD_NANDECC_AUTOPLACE, 2);
        assert_eq!(MTD_NANDECC_PLACEONLY, 3);
        assert_eq!(MTD_NANDECC_AUTOPL_USR, 4);
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            MEMGETINFO,
            MEMERASE,
            MEMWRITEOOB,
            MEMREADOOB,
            MEMLOCK,
            MEMUNLOCK,
            MEMGETREGIONCOUNT,
            MEMGETBADBLOCK,
            MEMSETBADBLOCK,
            MEMISLOCKED,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }
}
