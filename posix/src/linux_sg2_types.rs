//! `<scsi/sg.h>` — Additional SCSI Generic constants.
//!
//! Supplementary SG (SCSI Generic) constants covering
//! ioctl commands, flags, driver status, and info bits.

// ---------------------------------------------------------------------------
// SG ioctl commands
// ---------------------------------------------------------------------------

/// Get version number.
pub const SG_GET_VERSION_NUM: u32 = 0x2282;
/// Set timeout.
pub const SG_SET_TIMEOUT: u32 = 0x2201;
/// Get timeout.
pub const SG_GET_TIMEOUT: u32 = 0x2202;
/// Set command queue.
pub const SG_SET_COMMAND_Q: u32 = 0x2271;
/// Get command queue.
pub const SG_GET_COMMAND_Q: u32 = 0x2270;
/// Set reserved size.
pub const SG_SET_RESERVED_SIZE: u32 = 0x2275;
/// Get reserved size.
pub const SG_GET_RESERVED_SIZE: u32 = 0x2272;
/// Get SCSI ID.
pub const SG_GET_SCSI_ID: u32 = 0x2276;
/// Set force low DMA.
pub const SG_SET_FORCE_LOW_DMA: u32 = 0x2279;
/// Get low DMA.
pub const SG_GET_LOW_DMA: u32 = 0x227A;
/// Set force pack ID.
pub const SG_SET_FORCE_PACK_ID: u32 = 0x227B;
/// Get pack ID.
pub const SG_GET_PACK_ID: u32 = 0x227C;
/// Get num waiting.
pub const SG_GET_NUM_WAITING: u32 = 0x227D;
/// Set debug.
pub const SG_SET_DEBUG: u32 = 0x227E;
/// Get SG table size.
pub const SG_GET_SG_TABLESIZE: u32 = 0x227F;
/// Emulated host.
pub const SG_EMULATED_HOST: u32 = 0x2203;
/// SCSI ioctl: send command.
pub const SG_IO: u32 = 0x2285;
/// Get request table.
pub const SG_GET_REQUEST_TABLE: u32 = 0x2286;
/// Set keep orphan.
pub const SG_SET_KEEP_ORPHAN: u32 = 0x2287;
/// Get keep orphan.
pub const SG_GET_KEEP_ORPHAN: u32 = 0x2288;
/// Get access count.
pub const SG_GET_ACCESS_COUNT: u32 = 0x2289;

// ---------------------------------------------------------------------------
// SG data direction flags
// ---------------------------------------------------------------------------

/// No data transfer.
pub const SG_DXFER_NONE: i32 = -1;
/// Data to device.
pub const SG_DXFER_TO_DEV: i32 = -2;
/// Data from device.
pub const SG_DXFER_FROM_DEV: i32 = -3;
/// Bidirectional data.
pub const SG_DXFER_TO_FROM_DEV: i32 = -4;
/// Unknown direction.
pub const SG_DXFER_UNKNOWN: i32 = -5;

// ---------------------------------------------------------------------------
// SG info bits (from sg_io_hdr)
// ---------------------------------------------------------------------------

/// Indirect IO.
pub const SG_INFO_INDIRECT_IO: u32 = 0;
/// Direct IO.
pub const SG_INFO_DIRECT_IO: u32 = 1;
/// Mixed IO.
pub const SG_INFO_MIXED_IO: u32 = 2;
/// IO mask.
pub const SG_INFO_IO_MASK: u32 = 3;
/// Check condition.
pub const SG_INFO_CHECK: u32 = 1 << 0;
/// Direct IO requested.
pub const SG_INFO_DIRECT_IO_MASK: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// SG driver status
// ---------------------------------------------------------------------------

/// OK.
pub const SG_DRIVER_OK: u32 = 0x00;
/// Driver busy.
pub const SG_DRIVER_BUSY: u32 = 0x01;
/// Driver soft error.
pub const SG_DRIVER_SOFT: u32 = 0x02;
/// Driver media error.
pub const SG_DRIVER_MEDIA: u32 = 0x03;
/// Driver error.
pub const SG_DRIVER_ERROR: u32 = 0x04;
/// Invalid driver.
pub const SG_DRIVER_INVALID: u32 = 0x05;
/// Driver timeout.
pub const SG_DRIVER_TIMEOUT: u32 = 0x06;
/// Driver hard error.
pub const SG_DRIVER_HARD: u32 = 0x07;
/// Driver sense available.
pub const SG_DRIVER_SENSE: u32 = 0x08;

// ---------------------------------------------------------------------------
// SG default values
// ---------------------------------------------------------------------------

/// Default timeout (60 seconds in jiffies).
pub const SG_DEFAULT_TIMEOUT: u32 = 60000;
/// Default number of retries.
pub const SG_DEFAULT_RETRIES: u32 = 0;
/// Max sense length.
pub const SG_MAX_SENSE: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_distinct() {
        let cmds = [
            SG_GET_VERSION_NUM, SG_SET_TIMEOUT, SG_GET_TIMEOUT,
            SG_SET_COMMAND_Q, SG_GET_COMMAND_Q,
            SG_SET_RESERVED_SIZE, SG_GET_RESERVED_SIZE,
            SG_GET_SCSI_ID, SG_SET_FORCE_LOW_DMA, SG_GET_LOW_DMA,
            SG_SET_FORCE_PACK_ID, SG_GET_PACK_ID,
            SG_GET_NUM_WAITING, SG_SET_DEBUG,
            SG_GET_SG_TABLESIZE, SG_EMULATED_HOST,
            SG_IO, SG_GET_REQUEST_TABLE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_dxfer_distinct() {
        let dirs = [
            SG_DXFER_NONE, SG_DXFER_TO_DEV,
            SG_DXFER_FROM_DEV, SG_DXFER_TO_FROM_DEV,
            SG_DXFER_UNKNOWN,
        ];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }

    #[test]
    fn test_dxfer_all_negative() {
        assert!(SG_DXFER_NONE < 0);
        assert!(SG_DXFER_TO_DEV < 0);
        assert!(SG_DXFER_FROM_DEV < 0);
        assert!(SG_DXFER_TO_FROM_DEV < 0);
        assert!(SG_DXFER_UNKNOWN < 0);
    }

    #[test]
    fn test_driver_status_distinct() {
        let statuses = [
            SG_DRIVER_OK, SG_DRIVER_BUSY, SG_DRIVER_SOFT,
            SG_DRIVER_MEDIA, SG_DRIVER_ERROR, SG_DRIVER_INVALID,
            SG_DRIVER_TIMEOUT, SG_DRIVER_HARD, SG_DRIVER_SENSE,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_defaults() {
        assert_eq!(SG_DEFAULT_TIMEOUT, 60000);
        assert_eq!(SG_DEFAULT_RETRIES, 0);
        assert_eq!(SG_MAX_SENSE, 16);
    }
}
