//! `<linux/device.h>` — devtmpfs device node constants.
//!
//! devtmpfs is the kernel-managed tmpfs instance mounted at `/dev`
//! that automatically creates and removes device nodes as hardware
//! is detected. These constants define device types, permissions,
//! and naming conventions.

// ---------------------------------------------------------------------------
// Device types (from stat.h, used in mknod)
// ---------------------------------------------------------------------------

/// Block device type.
pub const S_IFBLK: u32 = 0o060000;
/// Character device type.
pub const S_IFCHR: u32 = 0o020000;

// ---------------------------------------------------------------------------
// Default device permissions
// ---------------------------------------------------------------------------

/// Default block device permissions (root rw, group r).
pub const DEV_BLK_DEFAULT_MODE: u32 = 0o660;
/// Default char device permissions (root rw, group rw).
pub const DEV_CHR_DEFAULT_MODE: u32 = 0o666;
/// Restricted device permissions (root only).
pub const DEV_RESTRICTED_MODE: u32 = 0o600;

// ---------------------------------------------------------------------------
// Well-known device major numbers
// ---------------------------------------------------------------------------

/// Memory devices (null, zero, random).
pub const MEM_MAJOR: u32 = 1;
/// TTY devices.
pub const TTY_MAJOR: u32 = 4;
/// TTY alternate devices.
pub const TTYAUX_MAJOR: u32 = 5;
/// Loopback devices.
pub const LOOP_MAJOR: u32 = 7;
/// SCSI disk major.
pub const SCSI_DISK0_MAJOR: u32 = 8;
/// Misc devices.
pub const MISC_MAJOR: u32 = 10;
/// IDE disk major (legacy).
pub const IDE0_MAJOR: u32 = 3;
/// Floppy disk major (legacy).
pub const FLOPPY_MAJOR: u32 = 2;
/// Input event devices.
pub const INPUT_MAJOR: u32 = 13;
/// Sound devices (ALSA).
pub const SOUND_MAJOR: u32 = 14;
/// USB character devices.
pub const USB_CHAR_MAJOR: u32 = 180;
/// Block device: virtio.
pub const VIRTBLK_MAJOR: u32 = 252;
/// NVMe character devices.
pub const NVME_MAJOR: u32 = 259;

// ---------------------------------------------------------------------------
// Well-known device minor numbers
// ---------------------------------------------------------------------------

/// /dev/null minor.
pub const DEV_NULL_MINOR: u32 = 3;
/// /dev/zero minor.
pub const DEV_ZERO_MINOR: u32 = 5;
/// /dev/full minor.
pub const DEV_FULL_MINOR: u32 = 7;
/// /dev/random minor.
pub const DEV_RANDOM_MINOR: u32 = 8;
/// /dev/urandom minor.
pub const DEV_URANDOM_MINOR: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_types_distinct() {
        assert_ne!(S_IFBLK, S_IFCHR);
    }

    #[test]
    fn test_major_numbers_distinct() {
        let majors = [
            MEM_MAJOR,
            TTY_MAJOR,
            TTYAUX_MAJOR,
            LOOP_MAJOR,
            SCSI_DISK0_MAJOR,
            MISC_MAJOR,
            IDE0_MAJOR,
            FLOPPY_MAJOR,
            INPUT_MAJOR,
            SOUND_MAJOR,
            USB_CHAR_MAJOR,
            VIRTBLK_MAJOR,
            NVME_MAJOR,
        ];
        for i in 0..majors.len() {
            for j in (i + 1)..majors.len() {
                assert_ne!(majors[i], majors[j]);
            }
        }
    }

    #[test]
    fn test_mem_device_minors_distinct() {
        let minors = [
            DEV_NULL_MINOR,
            DEV_ZERO_MINOR,
            DEV_FULL_MINOR,
            DEV_RANDOM_MINOR,
            DEV_URANDOM_MINOR,
        ];
        for i in 0..minors.len() {
            for j in (i + 1)..minors.len() {
                assert_ne!(minors[i], minors[j]);
            }
        }
    }

    #[test]
    fn test_mem_major() {
        assert_eq!(MEM_MAJOR, 1);
    }

    #[test]
    fn test_null_minor() {
        assert_eq!(DEV_NULL_MINOR, 3);
    }
}
