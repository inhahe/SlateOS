//! `<linux/cciss_ioctl.h>` — CCISS (Compaq Smart Array) constants.
//!
//! CCISS RAID controller constants covering ioctl commands,
//! command types, transfer directions, and bus types.

// ---------------------------------------------------------------------------
// CCISS ioctl commands
// ---------------------------------------------------------------------------

/// Pass-through ioctl.
pub const CCISS_PASSTHRU: u32 = 0x4004_C310;
/// Big pass-through ioctl.
pub const CCISS_BIG_PASSTHRU: u32 = 0x4050_C318;
/// Get driver version.
pub const CCISS_GETDRIVVER: u32 = 0x8004_C30A;
/// Get PCI info.
pub const CCISS_GETPCIINFO: u32 = 0x8014_C301;
/// Get interrupt coalescing.
pub const CCISS_GETINTINFO: u32 = 0x8008_C302;
/// Set interrupt coalescing.
pub const CCISS_SETINTINFO: u32 = 0x4008_C303;
/// Get node ID.
pub const CCISS_GETNODENAME: u32 = 0x8010_C304;
/// Set node ID.
pub const CCISS_SETNODENAME: u32 = 0x4010_C305;
/// Get heartbeat.
pub const CCISS_GETHEARTBEAT: u32 = 0x8004_C306;
/// Get bus types.
pub const CCISS_GETBUSTYPES: u32 = 0x8004_C307;
/// Get firmware version.
pub const CCISS_GETFIRMVER: u32 = 0x8004_C308;
/// Get logical volume info.
pub const CCISS_GETLUNINFO: u32 = 0x800C_C309;
/// Revalidate logical volumes.
pub const CCISS_REVALIDVOLS: u32 = 0x0000_C30E;
/// Deregister disk.
pub const CCISS_DEREGDISK: u32 = 0x0000_C30C;
/// Register disk.
pub const CCISS_REGNEWD: u32 = 0x0000_C30D;
/// Re-scan.
pub const CCISS_REGNEWDISK: u32 = 0x4004_C30F;

// ---------------------------------------------------------------------------
// CCISS command types
// ---------------------------------------------------------------------------

/// Vendor-specific command.
pub const CCISS_CMD_VENDOR: u8 = 0xC0;
/// Read command.
pub const CCISS_CMD_READ: u8 = 0x08;
/// Write command.
pub const CCISS_CMD_WRITE: u8 = 0x0A;
/// Read capacity.
pub const CCISS_CMD_READ_CAPACITY: u8 = 0x25;
/// Report LUNs.
pub const CCISS_CMD_REPORT_LUNS: u8 = 0xA0;
/// Inquiry.
pub const CCISS_CMD_INQUIRY: u8 = 0x12;

// ---------------------------------------------------------------------------
// CCISS transfer directions
// ---------------------------------------------------------------------------

/// No data transfer.
pub const XFER_NONE: u8 = 0x00;
/// Data in (read).
pub const XFER_READ: u8 = 0x01;
/// Data out (write).
pub const XFER_WRITE: u8 = 0x02;
/// Bidirectional.
pub const XFER_RSVD: u8 = 0x03;

// ---------------------------------------------------------------------------
// CCISS bus types
// ---------------------------------------------------------------------------

/// Ultra2 SCSI.
pub const BUS_TYPE_ULTRA2: u32 = 0x01;
/// Ultra3 SCSI.
pub const BUS_TYPE_ULTRA3: u32 = 0x02;
/// Fibre Channel.
pub const BUS_TYPE_FIBRE4: u32 = 0x04;
/// Fibre 1G.
pub const BUS_TYPE_FIBRE1G: u32 = 0x08;

// ---------------------------------------------------------------------------
// CCISS error info return codes
// ---------------------------------------------------------------------------

/// Command success.
pub const CMD_SUCCESS: u32 = 0x00;
/// Target status.
pub const CMD_TARGET_STATUS: u32 = 0x01;
/// Data underrun.
pub const CMD_DATA_UNDERRUN: u32 = 0x02;
/// Data overrun.
pub const CMD_DATA_OVERRUN: u32 = 0x03;
/// Invalid command.
pub const CMD_INVALID: u32 = 0x04;
/// Protocol error.
pub const CMD_PROTOCOL_ERR: u32 = 0x05;
/// Hardware error.
pub const CMD_HARDWARE_ERR: u32 = 0x06;
/// Connection lost.
pub const CMD_CONNECTION_LOST: u32 = 0x07;
/// Aborted.
pub const CMD_ABORTED: u32 = 0x08;
/// Abort failed.
pub const CMD_ABORT_FAILED: u32 = 0x09;
/// Unsolicited abort.
pub const CMD_UNSOLICITED_ABORT: u32 = 0x0A;
/// Timeout.
pub const CMD_TIMEOUT: u32 = 0x0B;
/// Unabortable.
pub const CMD_UNABORTABLE: u32 = 0x0C;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_types_distinct() {
        let cmds: [u8; 6] = [
            CCISS_CMD_VENDOR, CCISS_CMD_READ, CCISS_CMD_WRITE,
            CCISS_CMD_READ_CAPACITY, CCISS_CMD_REPORT_LUNS,
            CCISS_CMD_INQUIRY,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_xfer_dirs_distinct() {
        let dirs: [u8; 4] = [XFER_NONE, XFER_READ, XFER_WRITE, XFER_RSVD];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }

    #[test]
    fn test_bus_types_power_of_two() {
        let types = [
            BUS_TYPE_ULTRA2, BUS_TYPE_ULTRA3,
            BUS_TYPE_FIBRE4, BUS_TYPE_FIBRE1G,
        ];
        for t in &types {
            assert!(t.is_power_of_two(), "0x{:02x} not power of two", t);
        }
    }

    #[test]
    fn test_error_codes_distinct() {
        let codes = [
            CMD_SUCCESS, CMD_TARGET_STATUS, CMD_DATA_UNDERRUN,
            CMD_DATA_OVERRUN, CMD_INVALID, CMD_PROTOCOL_ERR,
            CMD_HARDWARE_ERR, CMD_CONNECTION_LOST, CMD_ABORTED,
            CMD_ABORT_FAILED, CMD_UNSOLICITED_ABORT,
            CMD_TIMEOUT, CMD_UNABORTABLE,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_error_codes_sequential() {
        assert_eq!(CMD_SUCCESS, 0);
        assert_eq!(CMD_UNABORTABLE, 0x0C);
    }
}
