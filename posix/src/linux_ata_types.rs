//! `<linux/ata.h>` — ATA/SATA command and register constants.
//!
//! ATA (AT Attachment) and its serial successor SATA define the
//! interface between host controllers and disk drives. While NVMe
//! is replacing SATA for SSDs, SATA remains dominant for HDDs and
//! many consumer SSDs.

// ---------------------------------------------------------------------------
// ATA command opcodes
// ---------------------------------------------------------------------------

/// Read sectors (28-bit LBA).
pub const ATA_CMD_READ_SECTORS: u8 = 0x20;
/// Write sectors (28-bit LBA).
pub const ATA_CMD_WRITE_SECTORS: u8 = 0x30;
/// Read sectors ext (48-bit LBA).
pub const ATA_CMD_READ_SECTORS_EXT: u8 = 0x24;
/// Write sectors ext (48-bit LBA).
pub const ATA_CMD_WRITE_SECTORS_EXT: u8 = 0x34;
/// Read DMA.
pub const ATA_CMD_READ_DMA: u8 = 0xC8;
/// Write DMA.
pub const ATA_CMD_WRITE_DMA: u8 = 0xCA;
/// Read DMA ext (48-bit).
pub const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
/// Write DMA ext (48-bit).
pub const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;
/// Read FPDMA (NCQ read).
pub const ATA_CMD_READ_FPDMA: u8 = 0x60;
/// Write FPDMA (NCQ write).
pub const ATA_CMD_WRITE_FPDMA: u8 = 0x61;
/// Identify device.
pub const ATA_CMD_IDENTIFY: u8 = 0xEC;
/// Set features.
pub const ATA_CMD_SET_FEATURES: u8 = 0xEF;
/// Flush cache.
pub const ATA_CMD_FLUSH_CACHE: u8 = 0xE7;
/// Flush cache ext (48-bit).
pub const ATA_CMD_FLUSH_CACHE_EXT: u8 = 0xEA;
/// SMART.
pub const ATA_CMD_SMART: u8 = 0xB0;
/// Standby immediate.
pub const ATA_CMD_STANDBY_IMMEDIATE: u8 = 0xE0;
/// Idle immediate.
pub const ATA_CMD_IDLE_IMMEDIATE: u8 = 0xE1;
/// Data Set Management (TRIM).
pub const ATA_CMD_DSM: u8 = 0x06;

// ---------------------------------------------------------------------------
// ATA status register bits
// ---------------------------------------------------------------------------

/// Busy.
pub const ATA_STATUS_BSY: u8 = 1 << 7;
/// Device ready.
pub const ATA_STATUS_DRDY: u8 = 1 << 6;
/// Device fault.
pub const ATA_STATUS_DF: u8 = 1 << 5;
/// Data request.
pub const ATA_STATUS_DRQ: u8 = 1 << 3;
/// Error.
pub const ATA_STATUS_ERR: u8 = 1 << 0;

// ---------------------------------------------------------------------------
// ATA device types
// ---------------------------------------------------------------------------

/// ATA device (hard disk).
pub const ATA_DEV_ATA: u8 = 0;
/// ATAPI device (CD/DVD).
pub const ATA_DEV_ATAPI: u8 = 1;
/// Port multiplier.
pub const ATA_DEV_PMP: u8 = 2;
/// SEMB (enclosure).
pub const ATA_DEV_SEMB: u8 = 3;
/// Unknown/no device.
pub const ATA_DEV_UNKNOWN: u8 = 0xFF;

// ---------------------------------------------------------------------------
// SATA speed generations
// ---------------------------------------------------------------------------

/// SATA Gen 1 (1.5 Gbps).
pub const SATA_SPEED_GEN1: u8 = 1;
/// SATA Gen 2 (3.0 Gbps).
pub const SATA_SPEED_GEN2: u8 = 2;
/// SATA Gen 3 (6.0 Gbps).
pub const SATA_SPEED_GEN3: u8 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            ATA_CMD_READ_SECTORS, ATA_CMD_WRITE_SECTORS,
            ATA_CMD_READ_SECTORS_EXT, ATA_CMD_WRITE_SECTORS_EXT,
            ATA_CMD_READ_DMA, ATA_CMD_WRITE_DMA,
            ATA_CMD_READ_DMA_EXT, ATA_CMD_WRITE_DMA_EXT,
            ATA_CMD_READ_FPDMA, ATA_CMD_WRITE_FPDMA,
            ATA_CMD_IDENTIFY, ATA_CMD_SET_FEATURES,
            ATA_CMD_FLUSH_CACHE, ATA_CMD_FLUSH_CACHE_EXT,
            ATA_CMD_SMART, ATA_CMD_STANDBY_IMMEDIATE,
            ATA_CMD_IDLE_IMMEDIATE, ATA_CMD_DSM,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_status_bits_selected_no_overlap() {
        let bits = [ATA_STATUS_BSY, ATA_STATUS_DRDY, ATA_STATUS_DF, ATA_STATUS_DRQ, ATA_STATUS_ERR];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_device_types_distinct() {
        let types = [ATA_DEV_ATA, ATA_DEV_ATAPI, ATA_DEV_PMP, ATA_DEV_SEMB, ATA_DEV_UNKNOWN];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_sata_speeds_distinct() {
        let speeds = [SATA_SPEED_GEN1, SATA_SPEED_GEN2, SATA_SPEED_GEN3];
        for i in 0..speeds.len() {
            for j in (i + 1)..speeds.len() {
                assert_ne!(speeds[i], speeds[j]);
            }
        }
    }
}
