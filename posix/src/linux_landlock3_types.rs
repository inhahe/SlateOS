//! `<linux/landlock.h>` — Additional Landlock constants.
//!
//! Supplementary Landlock constants covering ruleset attributes,
//! rule types, and access rights for filesystem and network.

// ---------------------------------------------------------------------------
// Landlock ABI version
// ---------------------------------------------------------------------------

/// Landlock ABI version 1.
pub const LANDLOCK_ABI_V1: u32 = 1;
/// Landlock ABI version 2.
pub const LANDLOCK_ABI_V2: u32 = 2;
/// Landlock ABI version 3.
pub const LANDLOCK_ABI_V3: u32 = 3;
/// Landlock ABI version 4.
pub const LANDLOCK_ABI_V4: u32 = 4;
/// Landlock ABI version 5.
pub const LANDLOCK_ABI_V5: u32 = 5;

// ---------------------------------------------------------------------------
// Landlock rule types
// ---------------------------------------------------------------------------

/// Path beneath rule.
pub const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;
/// Network port rule.
pub const LANDLOCK_RULE_NET_PORT: u32 = 2;

// ---------------------------------------------------------------------------
// Filesystem access rights (LANDLOCK_ACCESS_FS_*)
// ---------------------------------------------------------------------------

/// Execute.
pub const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
/// Write file.
pub const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
/// Read file.
pub const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
/// Read directory.
pub const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
/// Remove directory.
pub const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
/// Remove file.
pub const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
/// Make character device.
pub const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
/// Make directory.
pub const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
/// Make regular file.
pub const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
/// Make socket.
pub const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
/// Make FIFO.
pub const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
/// Make block device.
pub const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
/// Make symbolic link.
pub const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
/// Refer (move between dirs).
pub const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
/// Truncate file.
pub const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;
/// IOCTL on device.
pub const LANDLOCK_ACCESS_FS_IOCTL_DEV: u64 = 1 << 15;

// ---------------------------------------------------------------------------
// Network access rights (LANDLOCK_ACCESS_NET_*)
// ---------------------------------------------------------------------------

/// Bind to port.
pub const LANDLOCK_ACCESS_NET_BIND_TCP: u64 = 1 << 0;
/// Connect to port.
pub const LANDLOCK_ACCESS_NET_CONNECT_TCP: u64 = 1 << 1;

// ---------------------------------------------------------------------------
// Ruleset create flags
// ---------------------------------------------------------------------------

/// No flags.
pub const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abi_versions() {
        assert_eq!(LANDLOCK_ABI_V1, 1);
        assert_eq!(LANDLOCK_ABI_V5, 5);
    }

    #[test]
    fn test_rule_types() {
        assert_eq!(LANDLOCK_RULE_PATH_BENEATH, 1);
        assert_eq!(LANDLOCK_RULE_NET_PORT, 2);
    }

    #[test]
    fn test_fs_access_power_of_two() {
        let rights: [u64; 16] = [
            LANDLOCK_ACCESS_FS_EXECUTE, LANDLOCK_ACCESS_FS_WRITE_FILE,
            LANDLOCK_ACCESS_FS_READ_FILE, LANDLOCK_ACCESS_FS_READ_DIR,
            LANDLOCK_ACCESS_FS_REMOVE_DIR, LANDLOCK_ACCESS_FS_REMOVE_FILE,
            LANDLOCK_ACCESS_FS_MAKE_CHAR, LANDLOCK_ACCESS_FS_MAKE_DIR,
            LANDLOCK_ACCESS_FS_MAKE_REG, LANDLOCK_ACCESS_FS_MAKE_SOCK,
            LANDLOCK_ACCESS_FS_MAKE_FIFO, LANDLOCK_ACCESS_FS_MAKE_BLOCK,
            LANDLOCK_ACCESS_FS_MAKE_SYM, LANDLOCK_ACCESS_FS_REFER,
            LANDLOCK_ACCESS_FS_TRUNCATE, LANDLOCK_ACCESS_FS_IOCTL_DEV,
        ];
        for r in &rights {
            assert!(r.is_power_of_two(), "0x{:016x} not power of two", r);
        }
    }

    #[test]
    fn test_net_access_power_of_two() {
        assert!(LANDLOCK_ACCESS_NET_BIND_TCP.is_power_of_two());
        assert!(LANDLOCK_ACCESS_NET_CONNECT_TCP.is_power_of_two());
    }

    #[test]
    fn test_net_access_distinct() {
        assert_ne!(LANDLOCK_ACCESS_NET_BIND_TCP, LANDLOCK_ACCESS_NET_CONNECT_TCP);
    }
}
