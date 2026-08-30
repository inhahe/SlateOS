//! `<linux/hdreg.h>` — IDE/ATA register layout and ioctls.
//!
//! Even with modern AHCI and NVMe, `hdparm`, `smartctl`, and several
//! libata pass-through tools still issue legacy `HDIO_*` ioctls to
//! query geometry, drive identity, and ATA command status. The
//! register/command bit definitions below are the historical kernel
//! values.

// ---------------------------------------------------------------------------
// HDIO_* ioctl numbers (no _IO encoding; raw values)
// ---------------------------------------------------------------------------

/// `HDIO_GETGEO` — return drive CHS geometry.
pub const HDIO_GETGEO: u32 = 0x0301;
/// `HDIO_GET_UNMASKINTR` — get IRQ unmask flag.
pub const HDIO_GET_UNMASKINTR: u32 = 0x0302;
/// `HDIO_GET_MULTCOUNT` — get multi-sector I/O count.
pub const HDIO_GET_MULTCOUNT: u32 = 0x0304;
/// `HDIO_GET_IDENTITY` — return ATA IDENTIFY DEVICE response.
pub const HDIO_GET_IDENTITY: u32 = 0x030d;
/// `HDIO_GET_DMA` — get DMA-enabled flag.
pub const HDIO_GET_DMA: u32 = 0x030b;
/// `HDIO_DRIVE_CMD` — send a generic command and read result.
pub const HDIO_DRIVE_CMD: u32 = 0x031f;
/// `HDIO_DRIVE_TASK` — submit a task-file command.
pub const HDIO_DRIVE_TASK: u32 = 0x031e;
/// `HDIO_DRIVE_TASKFILE` — full 48-bit task-file command.
pub const HDIO_DRIVE_TASKFILE: u32 = 0x031d;
/// `HDIO_DRIVE_RESET` — reset the drive.
pub const HDIO_DRIVE_RESET: u32 = 0x031c;

// ---------------------------------------------------------------------------
// ATA Status register bits (read from 0x1F7)
// ---------------------------------------------------------------------------

/// Drive ready.
pub const ATA_STAT_READY: u8 = 0x40;
/// Drive busy (no other status bit valid while set).
pub const ATA_STAT_BUSY: u8 = 0x80;
/// Write fault.
pub const ATA_STAT_WRERR: u8 = 0x20;
/// Seek complete.
pub const ATA_STAT_SEEK: u8 = 0x10;
/// Data request — host should transfer.
pub const ATA_STAT_DRQ: u8 = 0x08;
/// Corrected ECC error.
pub const ATA_STAT_ECC: u8 = 0x04;
/// Index pulse seen.
pub const ATA_STAT_INDEX: u8 = 0x02;
/// Error — check error register.
pub const ATA_STAT_ERR: u8 = 0x01;

// ---------------------------------------------------------------------------
// ATA Error register bits (read from 0x1F1 when ATA_STAT_ERR is set)
// ---------------------------------------------------------------------------

/// Bad block detected.
pub const ATA_ERR_BBK: u8 = 0x80;
/// Uncorrectable ECC.
pub const ATA_ERR_UNC: u8 = 0x40;
/// Media changed.
pub const ATA_ERR_MC: u8 = 0x20;
/// ID not found.
pub const ATA_ERR_IDNF: u8 = 0x10;
/// Media-change request.
pub const ATA_ERR_MCR: u8 = 0x08;
/// Aborted command.
pub const ATA_ERR_ABRT: u8 = 0x04;
/// Track 0 not found.
pub const ATA_ERR_TK0NF: u8 = 0x02;
/// Address mark not found.
pub const ATA_ERR_AMNF: u8 = 0x01;

// ---------------------------------------------------------------------------
// ATA command opcodes (Command Register, 0x1F7) — historical names
// ---------------------------------------------------------------------------

/// `WIN_NOP` — no operation.
pub const WIN_NOP: u8 = 0x00;
/// `WIN_READ` — read sectors with retry.
pub const WIN_READ: u8 = 0x20;
/// `WIN_READ_EXT` — 48-bit LBA read.
pub const WIN_READ_EXT: u8 = 0x24;
/// `WIN_WRITE` — write sectors with retry.
pub const WIN_WRITE: u8 = 0x30;
/// `WIN_WRITE_EXT` — 48-bit LBA write.
pub const WIN_WRITE_EXT: u8 = 0x34;
/// `WIN_VERIFY` — verify sectors.
pub const WIN_VERIFY: u8 = 0x40;
/// `WIN_IDENTIFY` — IDENTIFY DEVICE.
pub const WIN_IDENTIFY: u8 = 0xEC;
/// `WIN_SETFEATURES` — SET FEATURES.
pub const WIN_SETFEATURES: u8 = 0xEF;
/// `WIN_FLUSH_CACHE` — FLUSH CACHE.
pub const WIN_FLUSH_CACHE: u8 = 0xE7;
/// `WIN_FLUSH_CACHE_EXT` — FLUSH CACHE EXT (48-bit).
pub const WIN_FLUSH_CACHE_EXT: u8 = 0xEA;
/// `WIN_SMART` — SMART command.
pub const WIN_SMART: u8 = 0xB0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hdio_ioctls_in_0x03xx_range() {
        // Historical pre-_IO() era: all HDIO_* are in 0x0300-0x033F.
        for &io in &[
            HDIO_GETGEO,
            HDIO_GET_UNMASKINTR,
            HDIO_GET_MULTCOUNT,
            HDIO_GET_IDENTITY,
            HDIO_GET_DMA,
            HDIO_DRIVE_CMD,
            HDIO_DRIVE_TASK,
            HDIO_DRIVE_TASKFILE,
            HDIO_DRIVE_RESET,
        ] {
            assert_eq!(io & 0xFF00, 0x0300);
        }
    }

    #[test]
    fn test_status_bits_pow2_and_busy_high() {
        let s = [
            ATA_STAT_READY,
            ATA_STAT_BUSY,
            ATA_STAT_WRERR,
            ATA_STAT_SEEK,
            ATA_STAT_DRQ,
            ATA_STAT_ECC,
            ATA_STAT_INDEX,
            ATA_STAT_ERR,
        ];
        for &b in &s {
            assert!(b.is_power_of_two());
        }
        // OR of all eight covers the full byte.
        let or: u8 = s.iter().fold(0u8, |a, &b| a | b);
        assert_eq!(or, 0xFF);
        // BUSY is the top bit (bit 7).
        assert_eq!(ATA_STAT_BUSY, 1 << 7);
        // ERR is bit 0 — check before reading other bits.
        assert_eq!(ATA_STAT_ERR, 1 << 0);
    }

    #[test]
    fn test_error_bits_full_byte() {
        let e = [
            ATA_ERR_BBK,
            ATA_ERR_UNC,
            ATA_ERR_MC,
            ATA_ERR_IDNF,
            ATA_ERR_MCR,
            ATA_ERR_ABRT,
            ATA_ERR_TK0NF,
            ATA_ERR_AMNF,
        ];
        for &b in &e {
            assert!(b.is_power_of_two());
        }
        assert_eq!(e.iter().fold(0u8, |a, &b| a | b), 0xFF);
    }

    #[test]
    fn test_command_opcodes_distinct() {
        let c = [
            WIN_NOP,
            WIN_READ,
            WIN_READ_EXT,
            WIN_WRITE,
            WIN_WRITE_EXT,
            WIN_VERIFY,
            WIN_IDENTIFY,
            WIN_SETFEATURES,
            WIN_FLUSH_CACHE,
            WIN_FLUSH_CACHE_EXT,
            WIN_SMART,
        ];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        // EXT variants are READ/WRITE + 4.
        assert_eq!(WIN_READ_EXT, WIN_READ + 4);
        assert_eq!(WIN_WRITE_EXT, WIN_WRITE + 4);
    }
}
