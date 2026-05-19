//! `<linux/mtd/mtd-abi.h>` — Additional MTD (Memory Technology Device) constants (part 3).
//!
//! Supplementary MTD constants covering device types,
//! OTP flags, and erase operation status.

// ---------------------------------------------------------------------------
// MTD device types
// ---------------------------------------------------------------------------

/// Absent (not present).
pub const MTD_ABSENT: u8 = 0;
/// RAM.
pub const MTD_RAM: u8 = 1;
/// ROM.
pub const MTD_ROM: u8 = 2;
/// NOR flash.
pub const MTD_NORFLASH: u8 = 3;
/// NAND flash.
pub const MTD_NANDFLASH: u8 = 4;
/// Dataflash.
pub const MTD_DATAFLASH: u8 = 6;
/// UBI volume.
pub const MTD_UBIVOLUME: u8 = 7;
/// MLC NAND flash.
pub const MTD_MLCNANDFLASH: u8 = 8;

// ---------------------------------------------------------------------------
// MTD capability flags
// ---------------------------------------------------------------------------

/// Writable.
pub const MTD_WRITEABLE: u32 = 0x400;
/// Bit writable (can clear individual bits).
pub const MTD_BIT_WRITEABLE: u32 = 0x800;
/// No erase needed.
pub const MTD_NO_ERASE: u32 = 0x1000;
/// Power-up lock supported.
pub const MTD_POWERUP_LOCK: u32 = 0x2000;
/// SLC on MLC emulation.
pub const MTD_SLC_ON_MLC_EMULATION: u32 = 0x4000;

// ---------------------------------------------------------------------------
// MTD OTP modes
// ---------------------------------------------------------------------------

/// No OTP.
pub const MTD_OTP_OFF: u32 = 0;
/// Factory OTP.
pub const MTD_OTP_FACTORY: u32 = 1;
/// User OTP.
pub const MTD_OTP_USER: u32 = 2;

// ---------------------------------------------------------------------------
// MTD file mode
// ---------------------------------------------------------------------------

/// Normal mode.
pub const MTD_FILE_MODE_NORMAL: u32 = 0;
/// OTP factory mode.
pub const MTD_FILE_MODE_OTP_FACTORY: u32 = 1;
/// OTP user mode.
pub const MTD_FILE_MODE_OTP_USER: u32 = 2;
/// Raw mode.
pub const MTD_FILE_MODE_RAW: u32 = 3;

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
    fn test_capability_flags_no_overlap() {
        let flags = [
            MTD_WRITEABLE, MTD_BIT_WRITEABLE,
            MTD_NO_ERASE, MTD_POWERUP_LOCK,
            MTD_SLC_ON_MLC_EMULATION,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
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
    fn test_file_modes_distinct() {
        let modes = [
            MTD_FILE_MODE_NORMAL, MTD_FILE_MODE_OTP_FACTORY,
            MTD_FILE_MODE_OTP_USER, MTD_FILE_MODE_RAW,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
