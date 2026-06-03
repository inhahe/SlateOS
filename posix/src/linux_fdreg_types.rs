//! `<linux/fdreg.h>` — i82077AA floppy-disk-controller register constants.
//!
//! Register addresses, command bytes, and status flags for the PC-AT
//! floppy disk controller as exposed by the kernel's `<linux/fdreg.h>`
//! header. Kept for legacy floppy support and for the few QEMU/x86
//! boot-from-floppy paths.

// ---------------------------------------------------------------------------
// Controller register offsets (from the FDC base I/O port)
// ---------------------------------------------------------------------------

/// Digital Output Register.
pub const FD_DOR: u32 = 2;
/// Tape Drive Register.
pub const FD_TDR: u32 = 3;
/// Main Status Register (read-only).
pub const FD_STATUS: u32 = 4;
/// Data Register (read/write).
pub const FD_DATA: u32 = 5;
/// Digital Input Register (read-only).
pub const FD_DIR: u32 = 7;
/// Configuration Control Register (write-only).
pub const FD_DCR: u32 = 7;

// ---------------------------------------------------------------------------
// Main Status Register bits (read from FD_STATUS)
// ---------------------------------------------------------------------------

/// FDC busy (any command executing).
pub const STATUS_BUSYMASK: u32 = 0x0F;
/// FDC busy with a command for drive 0..3 (per-drive busy bit).
pub const STATUS_BUSY: u32 = 0x10;
/// Non-DMA mode.
pub const STATUS_NON_DMA: u32 = 0x20;
/// Direction of transfer (1 = FDC → CPU).
pub const STATUS_DIR: u32 = 0x40;
/// Data register ready.
pub const STATUS_READY: u32 = 0x80;

// ---------------------------------------------------------------------------
// Selected FDC commands
// ---------------------------------------------------------------------------

/// Read track.
pub const FD_READ: u32 = 0xE6;
/// Write track.
pub const FD_WRITE: u32 = 0xC5;
/// Sense interrupt status.
pub const FD_SENSEI: u32 = 0x08;
/// Specify timings.
pub const FD_SPECIFY: u32 = 0x03;
/// Recalibrate (seek to track 0).
pub const FD_RECALIBRATE: u32 = 0x07;
/// Seek to a specific cylinder.
pub const FD_SEEK: u32 = 0x0F;
/// Read sector ID.
pub const FD_READID: u32 = 0x4A;
/// Format a track.
pub const FD_FORMAT: u32 = 0x4D;
/// Version (dump regs, 82077 enhanced).
pub const FD_VERSION: u32 = 0x10;
/// Configure (enhanced controllers).
pub const FD_CONFIGURE: u32 = 0x13;

// ---------------------------------------------------------------------------
// Drive status (ST0) bits returned by sense-interrupt
// ---------------------------------------------------------------------------

/// Drive select bits.
pub const ST0_DS: u32 = 0x03;
/// Head address.
pub const ST0_HA: u32 = 0x04;
/// Not ready.
pub const ST0_NR: u32 = 0x08;
/// Equipment check.
pub const ST0_ECE: u32 = 0x10;
/// Seek end.
pub const ST0_SE: u32 = 0x20;
/// Interrupt code.
pub const ST0_INTR: u32 = 0xC0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_offsets_distinct_rw() {
        // Read and write registers may overlap (DIR is read, DCR is write
        // at the same offset), but everything else is distinct.
        let regs = [FD_DOR, FD_TDR, FD_STATUS, FD_DATA];
        for i in 0..regs.len() {
            for j in (i + 1)..regs.len() {
                assert_ne!(regs[i], regs[j]);
            }
        }
        assert_eq!(FD_DIR, FD_DCR); // intentional read/write overlap
    }

    #[test]
    fn test_status_bits_distinct() {
        let bits = [
            STATUS_BUSY,
            STATUS_NON_DMA,
            STATUS_DIR,
            STATUS_READY,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
    }

    #[test]
    fn test_status_single_bits() {
        for &b in &[STATUS_BUSY, STATUS_NON_DMA, STATUS_DIR, STATUS_READY] {
            assert!(b.is_power_of_two(), "status bit {b:#x} is not a single bit");
        }
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            FD_READ,
            FD_WRITE,
            FD_SENSEI,
            FD_SPECIFY,
            FD_RECALIBRATE,
            FD_SEEK,
            FD_READID,
            FD_FORMAT,
            FD_VERSION,
            FD_CONFIGURE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_st0_bits_nonoverlapping_groups() {
        // ST0_DS (drive-select) is a 2-bit field, distinct from the
        // single-bit head / not-ready / equipment-check / seek-end flags.
        assert_eq!(ST0_DS & ST0_NR, 0);
        assert_eq!(ST0_DS & ST0_SE, 0);
        assert!(ST0_NR.is_power_of_two());
        assert!(ST0_ECE.is_power_of_two());
        assert!(ST0_SE.is_power_of_two());
    }
}
