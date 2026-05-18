//! `<linux/landlock.h>` — Landlock security constants (extended).
//!
//! Extended Landlock constants covering access rights for
//! filesystem, network, and rule/ruleset attributes.

// ---------------------------------------------------------------------------
// Landlock ruleset create flags
// ---------------------------------------------------------------------------

/// No flags (default).
pub const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Landlock filesystem access rights
// ---------------------------------------------------------------------------

/// Execute a file.
pub const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
/// Open file for writing.
pub const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
/// Open file for reading.
pub const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
/// Read directory.
pub const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
/// Remove directory.
pub const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
/// Remove file.
pub const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
/// Create character device.
pub const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
/// Create directory.
pub const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
/// Create regular file.
pub const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
/// Create socket.
pub const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
/// Create FIFO.
pub const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
/// Create block device.
pub const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
/// Create symlink.
pub const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
/// Refer (cross-rename or link).
pub const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
/// Truncate file.
pub const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;
/// IOctl on device.
pub const LANDLOCK_ACCESS_FS_IOCTL_DEV: u64 = 1 << 15;

// ---------------------------------------------------------------------------
// Landlock network access rights
// ---------------------------------------------------------------------------

/// Bind to a TCP port.
pub const LANDLOCK_ACCESS_NET_BIND_TCP: u64 = 1 << 0;
/// Connect to a TCP port.
pub const LANDLOCK_ACCESS_NET_CONNECT_TCP: u64 = 1 << 1;

// ---------------------------------------------------------------------------
// Landlock rule types
// ---------------------------------------------------------------------------

/// Path beneath rule.
pub const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;
/// Network port rule.
pub const LANDLOCK_RULE_NET_PORT: u32 = 2;

// ---------------------------------------------------------------------------
// Landlock ABI versions
// ---------------------------------------------------------------------------

/// ABI version 1 (filesystem only).
pub const LANDLOCK_ABI_V1: u32 = 1;
/// ABI version 2 (+ refer).
pub const LANDLOCK_ABI_V2: u32 = 2;
/// ABI version 3 (+ truncate).
pub const LANDLOCK_ABI_V3: u32 = 3;
/// ABI version 4 (+ network).
pub const LANDLOCK_ABI_V4: u32 = 4;
/// ABI version 5 (+ ioctl_dev).
pub const LANDLOCK_ABI_V5: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_access_powers_of_two() {
        let rights = [
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
            assert!(r.is_power_of_two(), "right {r:#x} not power of two");
        }
    }

    #[test]
    fn test_fs_access_no_overlap() {
        let rights = [
            LANDLOCK_ACCESS_FS_EXECUTE, LANDLOCK_ACCESS_FS_WRITE_FILE,
            LANDLOCK_ACCESS_FS_READ_FILE, LANDLOCK_ACCESS_FS_READ_DIR,
            LANDLOCK_ACCESS_FS_REMOVE_DIR, LANDLOCK_ACCESS_FS_REMOVE_FILE,
            LANDLOCK_ACCESS_FS_MAKE_CHAR, LANDLOCK_ACCESS_FS_MAKE_DIR,
            LANDLOCK_ACCESS_FS_MAKE_REG, LANDLOCK_ACCESS_FS_MAKE_SOCK,
            LANDLOCK_ACCESS_FS_MAKE_FIFO, LANDLOCK_ACCESS_FS_MAKE_BLOCK,
            LANDLOCK_ACCESS_FS_MAKE_SYM, LANDLOCK_ACCESS_FS_REFER,
            LANDLOCK_ACCESS_FS_TRUNCATE, LANDLOCK_ACCESS_FS_IOCTL_DEV,
        ];
        for i in 0..rights.len() {
            for j in (i + 1)..rights.len() {
                assert_eq!(rights[i] & rights[j], 0);
            }
        }
    }

    #[test]
    fn test_net_access_distinct() {
        assert_ne!(LANDLOCK_ACCESS_NET_BIND_TCP, LANDLOCK_ACCESS_NET_CONNECT_TCP);
    }

    #[test]
    fn test_rule_types_distinct() {
        assert_ne!(LANDLOCK_RULE_PATH_BENEATH, LANDLOCK_RULE_NET_PORT);
    }

    #[test]
    fn test_abi_versions_ascending() {
        assert!(LANDLOCK_ABI_V1 < LANDLOCK_ABI_V2);
        assert!(LANDLOCK_ABI_V2 < LANDLOCK_ABI_V3);
        assert!(LANDLOCK_ABI_V3 < LANDLOCK_ABI_V4);
        assert!(LANDLOCK_ABI_V4 < LANDLOCK_ABI_V5);
    }
}
