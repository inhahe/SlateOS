//! `<linux/landlock.h>` — Landlock access right constants.
//!
//! Landlock is a Linux security module that allows unprivileged
//! processes to restrict their own access rights. These constants
//! define the filesystem and network access rights that can be
//! granted or denied in a Landlock ruleset.

// ---------------------------------------------------------------------------
// Filesystem access rights (LANDLOCK_ACCESS_FS_*)
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
/// Refer (link/rename) to/from a file.
pub const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
/// Truncate a file.
pub const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;
/// Send ioctl to a file.
pub const LANDLOCK_ACCESS_FS_IOCTL_DEV: u64 = 1 << 15;

// ---------------------------------------------------------------------------
// Network access rights (LANDLOCK_ACCESS_NET_*)
// ---------------------------------------------------------------------------

/// Bind a TCP socket to a port.
pub const LANDLOCK_ACCESS_NET_BIND_TCP: u64 = 1 << 0;
/// Connect a TCP socket to a port.
pub const LANDLOCK_ACCESS_NET_CONNECT_TCP: u64 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_access_power_of_two() {
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
            assert!(r.is_power_of_two());
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
    fn test_net_access_power_of_two() {
        assert!(LANDLOCK_ACCESS_NET_BIND_TCP.is_power_of_two());
        assert!(LANDLOCK_ACCESS_NET_CONNECT_TCP.is_power_of_two());
    }

    #[test]
    fn test_net_access_no_overlap() {
        assert_eq!(LANDLOCK_ACCESS_NET_BIND_TCP & LANDLOCK_ACCESS_NET_CONNECT_TCP, 0);
    }

    #[test]
    fn test_execute_is_bit0() {
        assert_eq!(LANDLOCK_ACCESS_FS_EXECUTE, 1);
    }
}
