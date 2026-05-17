//! `<mtd/ubi-user.h>` — UBI (Unsorted Block Images) constants.
//!
//! UBI is a wear-leveling and volume management layer for raw NAND
//! flash. It sits on top of MTD (Memory Technology Devices) and below
//! filesystems like UBIFS. UBI handles bad block management, wear
//! leveling, and provides logical erase blocks (LEBs) as a reliable
//! abstraction over physical erase blocks (PEBs).

// ---------------------------------------------------------------------------
// UBI volume types
// ---------------------------------------------------------------------------

/// Dynamic volume (writable, no CRC on data).
pub const UBI_DYNAMIC_VOLUME: u8 = 3;
/// Static volume (read-only after write, CRC-protected).
pub const UBI_STATIC_VOLUME: u8 = 4;

// ---------------------------------------------------------------------------
// UBI ioctl commands (user-space interface)
// ---------------------------------------------------------------------------

/// Create volume.
pub const UBI_IOCMKVOL: u32 = 0x40986F00;
/// Remove volume.
pub const UBI_IOCRMVOL: u32 = 0x40046F01;
/// Resize volume.
pub const UBI_IOCRSVOL: u32 = 0x40086F02;
/// Rename volumes.
pub const UBI_IOCRNVOL: u32 = 0x40C86F03;
/// Update volume (set update marker).
pub const UBI_IOCVOLUP: u32 = 0x40086F04;
/// Atomically change LEB.
pub const UBI_IOCEBCH: u32 = 0x40086F05;
/// Map LEB.
pub const UBI_IOCEBMAP: u32 = 0x40086F07;
/// Unmap LEB.
pub const UBI_IOCEBUNMAP: u32 = 0x40046F08;

// ---------------------------------------------------------------------------
// UBI volume flags
// ---------------------------------------------------------------------------

/// Autoresize volume on attach.
pub const UBI_VOL_FLAG_AUTORESIZE: u8 = 1 << 0;
/// Skip CRC verification on read.
pub const UBI_VOL_FLAG_SKIP_CRC_CHECK: u8 = 1 << 1;

// ---------------------------------------------------------------------------
// VID header and EC header magic
// ---------------------------------------------------------------------------

/// Erase counter header magic ("UBI!").
pub const UBI_EC_HDR_MAGIC: u32 = 0x5542_4921;
/// Volume identifier header magic ("UBI#").
pub const UBI_VID_HDR_MAGIC: u32 = 0x5542_4923;

// ---------------------------------------------------------------------------
// LEB sizes and limits
// ---------------------------------------------------------------------------

/// Maximum volume name length.
pub const UBI_MAX_VOL_NAME_LEN: u32 = 127;
/// Maximum volumes per UBI device.
pub const UBI_MAX_VOLUMES: u32 = 128;
/// Volume ID for internal (layout) volume.
pub const UBI_LAYOUT_VOLUME_ID: u32 = 0x7FFFEFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_types_distinct() {
        assert_ne!(UBI_DYNAMIC_VOLUME, UBI_STATIC_VOLUME);
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            UBI_IOCMKVOL, UBI_IOCRMVOL, UBI_IOCRSVOL,
            UBI_IOCRNVOL, UBI_IOCVOLUP, UBI_IOCEBCH,
            UBI_IOCEBMAP, UBI_IOCEBUNMAP,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_magic_numbers() {
        assert_ne!(UBI_EC_HDR_MAGIC, UBI_VID_HDR_MAGIC);
        assert_eq!(UBI_EC_HDR_MAGIC, 0x5542_4921);
        assert_eq!(UBI_VID_HDR_MAGIC, 0x5542_4923);
    }

    #[test]
    fn test_limits() {
        assert_eq!(UBI_MAX_VOL_NAME_LEN, 127);
        assert_eq!(UBI_MAX_VOLUMES, 128);
    }

    #[test]
    fn test_vol_flags_no_overlap() {
        assert_eq!(UBI_VOL_FLAG_AUTORESIZE & UBI_VOL_FLAG_SKIP_CRC_CHECK, 0);
        assert!(UBI_VOL_FLAG_AUTORESIZE.is_power_of_two());
        assert!(UBI_VOL_FLAG_SKIP_CRC_CHECK.is_power_of_two());
    }
}
