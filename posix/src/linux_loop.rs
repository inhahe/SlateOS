//! `<linux/loop.h>` — loop device interface.
//!
//! Provides ioctl constants and structures for managing loop devices
//! (`/dev/loopN`), which mount regular files as block devices.

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum file path length in loop info.
pub const LO_NAME_SIZE: usize = 64;
/// Maximum encryption key length.
pub const LO_KEY_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// Loop flags
// ---------------------------------------------------------------------------

/// Read-only loop device.
pub const LO_FLAGS_READ_ONLY: u32 = 1;
/// Autoclear on last close.
pub const LO_FLAGS_AUTOCLEAR: u32 = 4;
/// Partition scan on setup.
pub const LO_FLAGS_PARTSCAN: u32 = 8;
/// Direct I/O mode.
pub const LO_FLAGS_DIRECT_IO: u32 = 16;

// ---------------------------------------------------------------------------
// Ioctl commands
// ---------------------------------------------------------------------------

/// Set backing file.
pub const LOOP_SET_FD: u64 = 0x4C00;
/// Clear backing file.
pub const LOOP_CLR_FD: u64 = 0x4C01;
/// Set loop info (64-bit).
pub const LOOP_SET_STATUS64: u64 = 0x4C04;
/// Get loop info (64-bit).
pub const LOOP_GET_STATUS64: u64 = 0x4C05;
/// Change backing file.
pub const LOOP_CHANGE_FD: u64 = 0x4C06;
/// Set capacity.
pub const LOOP_SET_CAPACITY: u64 = 0x4C07;
/// Set direct I/O mode.
pub const LOOP_SET_DIRECT_IO: u64 = 0x4C08;
/// Set block size.
pub const LOOP_SET_BLOCK_SIZE: u64 = 0x4C09;
/// Configure loop device (combined set fd + info).
pub const LOOP_CONFIGURE: u64 = 0x4C0A;
/// Get free loop device number.
pub const LOOP_CTL_GET_FREE: u64 = 0x4C82;
/// Add a loop device.
pub const LOOP_CTL_ADD: u64 = 0x4C80;
/// Remove a loop device.
pub const LOOP_CTL_REMOVE: u64 = 0x4C81;

// ---------------------------------------------------------------------------
// Loop info struct (64-bit version)
// ---------------------------------------------------------------------------

/// Loop device info (matches `struct loop_info64`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LoopInfo64 {
    /// Backing device.
    pub lo_device: u64,
    /// Backing inode.
    pub lo_inode: u64,
    /// Real device (internal).
    pub lo_rdevice: u64,
    /// Offset into the backing file.
    pub lo_offset: u64,
    /// Size limit (0 = no limit).
    pub lo_sizelimit: u64,
    /// Loop device number.
    pub lo_number: u32,
    /// Encryption type.
    pub lo_encrypt_type: u32,
    /// Encryption key size.
    pub lo_encrypt_key_size: u32,
    /// Flags (LO_FLAGS_*).
    pub lo_flags: u32,
    /// Backing file name.
    pub lo_file_name: [u8; LO_NAME_SIZE],
    /// Encryption name.
    pub lo_crypt_name: [u8; LO_NAME_SIZE],
    /// Encryption key.
    pub lo_encrypt_key: [u8; LO_KEY_SIZE],
    /// Init vector.
    pub lo_init: [u64; 2],
}

impl LoopInfo64 {
    /// Create a zeroed `LoopInfo64`.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_info64_size() {
        // The struct is sizeable — at least 200 bytes.
        assert!(core::mem::size_of::<LoopInfo64>() >= 200);
    }

    #[test]
    fn test_loop_info64_zeroed() {
        let info = LoopInfo64::zeroed();
        assert_eq!(info.lo_offset, 0);
        assert_eq!(info.lo_sizelimit, 0);
        assert_eq!(info.lo_flags, 0);
        assert_eq!(info.lo_number, 0);
    }

    #[test]
    fn test_loop_flags_are_bits() {
        let combined =
            LO_FLAGS_READ_ONLY | LO_FLAGS_AUTOCLEAR | LO_FLAGS_PARTSCAN | LO_FLAGS_DIRECT_IO;
        // No overlap.
        assert_eq!(combined, 1 | 4 | 8 | 16);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            LOOP_SET_FD,
            LOOP_CLR_FD,
            LOOP_SET_STATUS64,
            LOOP_GET_STATUS64,
            LOOP_CHANGE_FD,
            LOOP_SET_CAPACITY,
            LOOP_SET_DIRECT_IO,
            LOOP_SET_BLOCK_SIZE,
            LOOP_CONFIGURE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_ctl_commands_distinct() {
        assert_ne!(LOOP_CTL_GET_FREE, LOOP_CTL_ADD);
        assert_ne!(LOOP_CTL_ADD, LOOP_CTL_REMOVE);
        assert_ne!(LOOP_CTL_GET_FREE, LOOP_CTL_REMOVE);
    }

    #[test]
    fn test_name_sizes() {
        assert_eq!(LO_NAME_SIZE, 64);
        assert_eq!(LO_KEY_SIZE, 32);
    }
}
