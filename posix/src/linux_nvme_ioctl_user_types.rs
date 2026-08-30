//! `<linux/nvme_ioctl.h>` — NVMe character-device ABI.
//!
//! `/dev/nvme0` (controller) and `/dev/nvme0n1` (namespace) accept
//! ioctls for sending raw admin / I/O commands and querying ids.
//! `nvme-cli`, fwupd, and Smartmontools-NVMe all live on top of
//! these calls.

// ---------------------------------------------------------------------------
// Ioctl type ('N') and command numbers (`_IO`/`_IOWR`)
// ---------------------------------------------------------------------------
//
// All NVMe ioctls share the magic byte 'N' (0x4E). The kernel encodes
// the direction and size into the ioctl number via `_IOR`/`_IOW`/
// `_IOWR`; here we keep just the (type, nr) pair so callers can build
// the full encoded value with the platform's `ioctl` macro.

pub const NVME_IOCTL_TYPE: u8 = b'N';

pub const NVME_IOCTL_NR_ID: u8 = 0x40;
pub const NVME_IOCTL_NR_ADMIN_CMD: u8 = 0x41;
pub const NVME_IOCTL_NR_SUBMIT_IO: u8 = 0x42;
pub const NVME_IOCTL_NR_IO_CMD: u8 = 0x43;
pub const NVME_IOCTL_NR_RESET: u8 = 0x44;
pub const NVME_IOCTL_NR_SUBSYS_RESET: u8 = 0x45;
pub const NVME_IOCTL_NR_RESCAN: u8 = 0x46;
pub const NVME_IOCTL_NR_ADMIN64_CMD: u8 = 0x47;
pub const NVME_IOCTL_NR_IO64_CMD: u8 = 0x48;
pub const NVME_IOCTL_NR_IO64_CMD_VEC: u8 = 0x49;
pub const NVME_IOCTL_NR_URING_CMD: u8 = 0x4A;
pub const NVME_IOCTL_NR_URING_CMD_VEC: u8 = 0x4B;

// ---------------------------------------------------------------------------
// `nvme_passthru_cmd` flags
// ---------------------------------------------------------------------------

/// Vendor-specific opcode — accept it even if not in the spec table.
pub const NVME_PASSTHRU_VENDOR_SPECIFIC: u32 = 1 << 0;
/// Wait for command completion (synchronous).
pub const NVME_PASSTHRU_BLK: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Device-node paths
// ---------------------------------------------------------------------------

pub const NVME_CHAR_DEV_GLOB: &str = "/dev/nvme*";
pub const NVME_NS_DEV_GLOB: &str = "/dev/nvme*n*";
pub const NVME_GENERIC_NODE_GLOB: &str = "/dev/ng*";

// ---------------------------------------------------------------------------
// Submission-queue depth limits exposed to userspace
// ---------------------------------------------------------------------------

/// NVMe spec section 4.1.3: max queue size is 65536 entries (16-bit).
pub const NVME_MAX_QUEUE_SIZE: u32 = 65536;
pub const NVME_MIN_QUEUE_SIZE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_type_is_N() {
        // ASCII 'N' = 0x4E. The NVMe maintainers chose this letter to be
        // visually distinct from the older SCSI 'S' magic.
        assert_eq!(NVME_IOCTL_TYPE, 0x4E);
        assert_eq!(NVME_IOCTL_TYPE as char, 'N');
    }

    #[test]
    fn test_ioctl_nr_dense_0x40_to_0x4B() {
        let n = [
            NVME_IOCTL_NR_ID,
            NVME_IOCTL_NR_ADMIN_CMD,
            NVME_IOCTL_NR_SUBMIT_IO,
            NVME_IOCTL_NR_IO_CMD,
            NVME_IOCTL_NR_RESET,
            NVME_IOCTL_NR_SUBSYS_RESET,
            NVME_IOCTL_NR_RESCAN,
            NVME_IOCTL_NR_ADMIN64_CMD,
            NVME_IOCTL_NR_IO64_CMD,
            NVME_IOCTL_NR_IO64_CMD_VEC,
            NVME_IOCTL_NR_URING_CMD,
            NVME_IOCTL_NR_URING_CMD_VEC,
        ];
        for (i, &v) in n.iter().enumerate() {
            assert_eq!(v as u32, 0x40 + i as u32);
        }
    }

    #[test]
    fn test_passthru_flags_single_bit() {
        assert!(NVME_PASSTHRU_VENDOR_SPECIFIC.is_power_of_two());
        assert!(NVME_PASSTHRU_BLK.is_power_of_two());
        assert_ne!(NVME_PASSTHRU_VENDOR_SPECIFIC, NVME_PASSTHRU_BLK);
    }

    #[test]
    fn test_dev_globs() {
        assert!(NVME_CHAR_DEV_GLOB.starts_with("/dev/nvme"));
        assert!(NVME_NS_DEV_GLOB.contains("n*"));
        assert!(NVME_GENERIC_NODE_GLOB.starts_with("/dev/ng"));
    }

    #[test]
    fn test_queue_bounds_match_spec() {
        // 16-bit field in the spec: 0..=65535 plus an implicit +1 for the
        // doorbell wrap = 65536 maximum entries.
        assert_eq!(NVME_MAX_QUEUE_SIZE, 65536);
        assert_eq!(NVME_MIN_QUEUE_SIZE, 2);
        assert!(NVME_MAX_QUEUE_SIZE > NVME_MIN_QUEUE_SIZE);
    }
}
