//! `<linux/landlock.h>` — Landlock access control constants.
//!
//! Landlock is a stackable Linux Security Module (LSM) that allows
//! unprivileged processes to restrict their own access rights. Unlike
//! seccomp (which filters syscalls), Landlock restricts filesystem and
//! network access at the object level. A process creates a ruleset,
//! adds rules, then enforces it — after which it cannot gain back the
//! restricted permissions.

// ---------------------------------------------------------------------------
// Landlock ABI versions
// ---------------------------------------------------------------------------

/// ABI version 1 (Linux 5.13): filesystem access control.
pub const LANDLOCK_ABI_V1: u32 = 1;
/// ABI version 2 (Linux 5.19): adds REFER right.
pub const LANDLOCK_ABI_V2: u32 = 2;
/// ABI version 3 (Linux 6.2): adds TRUNCATE right.
pub const LANDLOCK_ABI_V3: u32 = 3;
/// ABI version 4 (Linux 6.7): adds network access control.
pub const LANDLOCK_ABI_V4: u32 = 4;

// ---------------------------------------------------------------------------
// Ruleset creation flags
// ---------------------------------------------------------------------------

/// Create ruleset attr flag: none (no flags defined yet).
pub const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Filesystem access rights
// ---------------------------------------------------------------------------

/// Execute a file.
pub const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
/// Open a file for writing.
pub const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
/// Open a file for reading.
pub const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
/// Open a directory for reading.
pub const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
/// Remove a directory.
pub const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
/// Remove a file.
pub const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
/// Create a character device.
pub const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
/// Create a directory.
pub const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
/// Create a regular file.
pub const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
/// Create a socket.
pub const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
/// Create a FIFO (named pipe).
pub const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
/// Create a block device.
pub const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
/// Create a symbolic link.
pub const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
/// Link or rename across directories (ABI v2).
pub const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
/// Truncate a file (ABI v3).
pub const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;

// ---------------------------------------------------------------------------
// Network access rights (ABI v4)
// ---------------------------------------------------------------------------

/// Bind to a TCP port.
pub const LANDLOCK_ACCESS_NET_BIND_TCP: u64 = 1 << 0;
/// Connect to a TCP port.
pub const LANDLOCK_ACCESS_NET_CONNECT_TCP: u64 = 1 << 1;

// ---------------------------------------------------------------------------
// Rule types
// ---------------------------------------------------------------------------

/// Rule applies to a filesystem path.
pub const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;
/// Rule applies to a network port (ABI v4).
pub const LANDLOCK_RULE_NET_PORT: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abi_versions_ascending() {
        assert!(LANDLOCK_ABI_V1 < LANDLOCK_ABI_V2);
        assert!(LANDLOCK_ABI_V2 < LANDLOCK_ABI_V3);
        assert!(LANDLOCK_ABI_V3 < LANDLOCK_ABI_V4);
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
            LANDLOCK_ACCESS_FS_TRUNCATE,
        ];
        for i in 0..rights.len() {
            assert!(rights[i].is_power_of_two());
            for j in (i + 1)..rights.len() {
                assert_eq!(rights[i] & rights[j], 0);
            }
        }
    }

    #[test]
    fn test_net_access_no_overlap() {
        assert_eq!(
            LANDLOCK_ACCESS_NET_BIND_TCP & LANDLOCK_ACCESS_NET_CONNECT_TCP,
            0
        );
        assert!(LANDLOCK_ACCESS_NET_BIND_TCP.is_power_of_two());
        assert!(LANDLOCK_ACCESS_NET_CONNECT_TCP.is_power_of_two());
    }

    #[test]
    fn test_rule_types_distinct() {
        assert_ne!(LANDLOCK_RULE_PATH_BENEATH, LANDLOCK_RULE_NET_PORT);
    }
}
