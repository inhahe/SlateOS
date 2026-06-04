//! `<linux/cciss_ioctl.h>` — Compaq/HP Smart Array (cciss) controller.
//!
//! The `/dev/cciss/cXdYpZ` block devices expose Smart Array (P400,
//! P800, …) RAID controllers. The cciss IOCTLs let userspace
//! passthrough SCSI/SAS commands and query controller info.

// ---------------------------------------------------------------------------
// Device path family
// ---------------------------------------------------------------------------

pub const CCISS_DEVICE_PREFIX: &str = "/dev/cciss/c";

// ---------------------------------------------------------------------------
// CCISS ioctls (type 0x42 = 'B')
// ---------------------------------------------------------------------------

/// `_IOR('B', 0, struct LogvolInfo)` — get logical volume info.
pub const CCISS_GETLOGVOL: u32 = 0x4000_4200;

/// `_IOR('B', 1, struct HostIDInfo)` — get host adapter ID.
pub const CCISS_GETHEARTBEAT: u32 = 0x4000_4201;

/// `_IOR('B', 2, struct BusTypesInfo)` — get bus type bits.
pub const CCISS_GETBUSTYPES: u32 = 0x4000_4202;

/// `_IOR('B', 3, FirmwareVer_type)` — get firmware version.
pub const CCISS_GETFIRMVER: u32 = 0x4000_4203;

/// `_IOR('B', 4, DriverVer_type)` — get driver version.
pub const CCISS_GETDRIVVER: u32 = 0x4000_4204;

/// `_IO('B', 5)` — rescan the disk list (deprecated).
pub const CCISS_REVALIDVOLS: u32 = 0x0000_4205;

/// `_IOWR('B', 6, IOCTL_Command_struct)` — passthrough a Smart Array cmd.
pub const CCISS_PASSTHRU: u32 = 0xC158_4206;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum logical volumes per controller (legacy CCISS limit).
pub const CCISS_MAX_LOGVOL: u32 = 256;

/// Maximum drives per controller.
pub const CCISS_MAX_DRIVES: u32 = 16;

/// Maximum SCSI CDB length accepted by CCISS_PASSTHRU.
pub const CCISS_MAX_CDB_LEN: usize = 16;

/// Maximum sense-data buffer length.
pub const CCISS_MAX_SENSE_LEN: usize = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_prefix() {
        assert!(CCISS_DEVICE_PREFIX.starts_with("/dev/"));
        assert!(CCISS_DEVICE_PREFIX.ends_with("c"));
    }

    #[test]
    fn test_ioctls_in_b_family_dense_0_to_6() {
        let o = [
            CCISS_GETLOGVOL,
            CCISS_GETHEARTBEAT,
            CCISS_GETBUSTYPES,
            CCISS_GETFIRMVER,
            CCISS_GETDRIVVER,
            CCISS_REVALIDVOLS,
            CCISS_PASSTHRU,
        ];
        for (i, &v) in o.iter().enumerate() {
            // Type byte is 'B' = 0x42.
            assert_eq!((v >> 8) & 0xFF, 0x42);
            // Number sub-field 0..6 dense.
            assert_eq!(v & 0xFF, i as u32);
        }
    }

    #[test]
    fn test_passthru_direction_is_iowr() {
        // _IOWR sets both read and write direction bits (0xC).
        assert_eq!(CCISS_PASSTHRU >> 30, 0x3);
    }

    #[test]
    fn test_revalidvols_is_io_with_no_direction() {
        assert_eq!(CCISS_REVALIDVOLS >> 30, 0);
    }

    #[test]
    fn test_query_ioctls_are_ior() {
        for v in [
            CCISS_GETLOGVOL,
            CCISS_GETHEARTBEAT,
            CCISS_GETBUSTYPES,
            CCISS_GETFIRMVER,
            CCISS_GETDRIVVER,
        ] {
            assert_eq!(v >> 30, 0x1);
        }
    }

    #[test]
    fn test_size_limits() {
        assert_eq!(CCISS_MAX_LOGVOL, 256);
        assert_eq!(CCISS_MAX_DRIVES, 16);
        // Both are powers of two.
        assert!(CCISS_MAX_LOGVOL.is_power_of_two());
        assert!(CCISS_MAX_DRIVES.is_power_of_two());
        // Logical volumes >> physical drives (RAID overcommit).
        assert!(CCISS_MAX_LOGVOL > CCISS_MAX_DRIVES);
    }

    #[test]
    fn test_cdb_and_sense_lengths() {
        assert_eq!(CCISS_MAX_CDB_LEN, 16);
        assert_eq!(CCISS_MAX_SENSE_LEN, 32);
        // Sense > CDB by 2x.
        assert_eq!(CCISS_MAX_SENSE_LEN / CCISS_MAX_CDB_LEN, 2);
    }
}
