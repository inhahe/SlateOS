//! `<linux/mmc/ioctl.h>` — userspace SD/MMC raw command interface.
//!
//! `/dev/mmcblkN` exposes a passthrough ioctl so userspace tools
//! (mmc-utils, fwupd, hwclock-on-an-eMMC-RPMB-partition) can send
//! arbitrary CMDs straight to the card. The constants here describe
//! the response/format flags and ioctl numbers.

// ---------------------------------------------------------------------------
// ioctl group letter
// ---------------------------------------------------------------------------

/// Magic letter for /dev/mmcblk ioctls (block layer 'B' is reused but
/// the MMC-specific subgroup uses 'M').
pub const MMC_IOC_MAGIC: u8 = b'M';

// ---------------------------------------------------------------------------
// ioctl numbers
// ---------------------------------------------------------------------------

/// `MMC_IOC_CMD` — send a single mmc_ioc_cmd struct.
pub const MMC_IOC_CMD: u32 = 0xC048_4D00;
/// `MMC_IOC_MULTI_CMD` — send an array of mmc_ioc_cmd via mmc_ioc_multi_cmd.
pub const MMC_IOC_MULTI_CMD: u32 = 0xC008_4D01;

// ---------------------------------------------------------------------------
// Response flags (mmc_ioc_cmd.flags)
// ---------------------------------------------------------------------------

/// Response present.
pub const MMC_RSP_PRESENT: u32 = 1 << 0;
/// Response has a 136-bit length (R2).
pub const MMC_RSP_136: u32 = 1 << 1;
/// Response is CRC-protected.
pub const MMC_RSP_CRC: u32 = 1 << 2;
/// Response includes busy signaling (R1b).
pub const MMC_RSP_BUSY: u32 = 1 << 3;
/// Response includes the opcode in bits.
pub const MMC_RSP_OPCODE: u32 = 1 << 4;

/// R1 response = PRESENT|CRC|OPCODE.
pub const MMC_RSP_R1: u32 = MMC_RSP_PRESENT | MMC_RSP_CRC | MMC_RSP_OPCODE;
/// R1b response = R1 + BUSY.
pub const MMC_RSP_R1B: u32 = MMC_RSP_R1 | MMC_RSP_BUSY;
/// R2 response = PRESENT|136|CRC.
pub const MMC_RSP_R2: u32 = MMC_RSP_PRESENT | MMC_RSP_136 | MMC_RSP_CRC;
/// R3 response = PRESENT only.
pub const MMC_RSP_R3: u32 = MMC_RSP_PRESENT;

// ---------------------------------------------------------------------------
// Data direction flags
// ---------------------------------------------------------------------------

/// Transfer reads data from the card.
pub const MMC_DATA_READ: u32 = 1 << 0;
/// Transfer writes data to the card.
pub const MMC_DATA_WRITE: u32 = 1 << 1;
/// Block-mode transfer (vs stream).
pub const MMC_DATA_STREAM: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum data transfer per single ioctl (512 KiB).
pub const MMC_IOC_MAX_BYTES: u32 = 512 * 1024;
/// Maximum number of commands in MMC_IOC_MULTI_CMD.
pub const MMC_IOC_MAX_CMDS: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_letter_m() {
        assert_eq!(MMC_IOC_MAGIC, b'M');
        // Type byte 'M' (0x4D) in bits 8..15.
        assert_eq!((MMC_IOC_CMD >> 8) & 0xff, b'M' as u32);
        assert_eq!((MMC_IOC_MULTI_CMD >> 8) & 0xff, b'M' as u32);
    }

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(MMC_IOC_CMD, MMC_IOC_MULTI_CMD);
    }

    #[test]
    fn test_rsp_bits_pow2_distinct() {
        let f = [
            MMC_RSP_PRESENT,
            MMC_RSP_136,
            MMC_RSP_CRC,
            MMC_RSP_BUSY,
            MMC_RSP_OPCODE,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_rsp_composites() {
        // R1b = R1 + BUSY (busy signal after command finishes).
        assert_eq!(MMC_RSP_R1B, MMC_RSP_R1 | MMC_RSP_BUSY);
        // R2 carries CID/CSD (136-bit response, no opcode echo).
        assert!((MMC_RSP_R2 & MMC_RSP_136) != 0);
        assert!((MMC_RSP_R2 & MMC_RSP_OPCODE) == 0);
    }

    #[test]
    fn test_data_bits_pow2_distinct() {
        let f = [MMC_DATA_READ, MMC_DATA_WRITE, MMC_DATA_STREAM];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        // READ and WRITE are mutually exclusive but stored in the
        // same flags field — check distinctness.
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_limits() {
        assert_eq!(MMC_IOC_MAX_BYTES, 524_288);
        assert!(MMC_IOC_MAX_BYTES.is_power_of_two());
        assert_eq!(MMC_IOC_MAX_CMDS, 255);
    }
}
