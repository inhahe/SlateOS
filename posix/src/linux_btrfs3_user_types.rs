//! `<linux/btrfs.h>` (part 3) — Btrfs send/receive stream constants.
//!
//! `btrfs send` produces a structured stream of TLV-encoded commands
//! that describe how to recreate the difference between two snapshots
//! on the receiver. This module covers the stream magic, version,
//! command IDs, and attribute IDs.

// ---------------------------------------------------------------------------
// Stream magic & version
// ---------------------------------------------------------------------------

/// Stream magic — `btrfs-stream\0` (13 bytes).
pub const BTRFS_SEND_STREAM_MAGIC: &[u8; 13] = b"btrfs-stream\0";

/// Send-stream version field length.
pub const BTRFS_SEND_STREAM_MAGIC_LEN: usize = 13;

/// Send-stream version (current).
pub const BTRFS_SEND_STREAM_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Command IDs (`btrfs_send_cmd`)
// ---------------------------------------------------------------------------

pub const BTRFS_SEND_C_UNSPEC: u32 = 0;
pub const BTRFS_SEND_C_SUBVOL: u32 = 1;
pub const BTRFS_SEND_C_SNAPSHOT: u32 = 2;
pub const BTRFS_SEND_C_MKFILE: u32 = 3;
pub const BTRFS_SEND_C_MKDIR: u32 = 4;
pub const BTRFS_SEND_C_MKNOD: u32 = 5;
pub const BTRFS_SEND_C_MKFIFO: u32 = 6;
pub const BTRFS_SEND_C_MKSOCK: u32 = 7;
pub const BTRFS_SEND_C_SYMLINK: u32 = 8;
pub const BTRFS_SEND_C_RENAME: u32 = 9;
pub const BTRFS_SEND_C_LINK: u32 = 10;
pub const BTRFS_SEND_C_UNLINK: u32 = 11;
pub const BTRFS_SEND_C_RMDIR: u32 = 12;
pub const BTRFS_SEND_C_SET_XATTR: u32 = 13;
pub const BTRFS_SEND_C_REMOVE_XATTR: u32 = 14;
pub const BTRFS_SEND_C_WRITE: u32 = 15;
pub const BTRFS_SEND_C_CLONE: u32 = 16;
pub const BTRFS_SEND_C_TRUNCATE: u32 = 17;
pub const BTRFS_SEND_C_CHMOD: u32 = 18;
pub const BTRFS_SEND_C_CHOWN: u32 = 19;
pub const BTRFS_SEND_C_UTIMES: u32 = 20;
pub const BTRFS_SEND_C_END: u32 = 21;
pub const BTRFS_SEND_C_UPDATE_EXTENT: u32 = 22;
pub const BTRFS_SEND_C_MAX: u32 = 22;

// ---------------------------------------------------------------------------
// Attribute IDs (`btrfs_send_attr`) — a few key ones
// ---------------------------------------------------------------------------

pub const BTRFS_SEND_A_UNSPEC: u32 = 0;
pub const BTRFS_SEND_A_UUID: u32 = 1;
pub const BTRFS_SEND_A_CTRANSID: u32 = 2;
pub const BTRFS_SEND_A_INO: u32 = 3;
pub const BTRFS_SEND_A_SIZE: u32 = 4;
pub const BTRFS_SEND_A_MODE: u32 = 5;
pub const BTRFS_SEND_A_UID: u32 = 6;
pub const BTRFS_SEND_A_GID: u32 = 7;
pub const BTRFS_SEND_A_RDEV: u32 = 8;
pub const BTRFS_SEND_A_CTIME: u32 = 9;
pub const BTRFS_SEND_A_MTIME: u32 = 10;
pub const BTRFS_SEND_A_ATIME: u32 = 11;
pub const BTRFS_SEND_A_OTIME: u32 = 12;
pub const BTRFS_SEND_A_XATTR_NAME: u32 = 13;
pub const BTRFS_SEND_A_XATTR_DATA: u32 = 14;
pub const BTRFS_SEND_A_PATH: u32 = 15;

// ---------------------------------------------------------------------------
// Receive options
// ---------------------------------------------------------------------------

/// Skip orphan-clean-up step (continue partial receive).
pub const BTRFS_RECV_FLAG_NO_CLEAN: u64 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_magic_is_13_bytes_with_trailing_nul() {
        assert_eq!(BTRFS_SEND_STREAM_MAGIC.len(), BTRFS_SEND_STREAM_MAGIC_LEN);
        assert_eq!(BTRFS_SEND_STREAM_MAGIC_LEN, 13);
        assert_eq!(*BTRFS_SEND_STREAM_MAGIC.last().unwrap(), 0);
        // The text part is "btrfs-stream".
        assert_eq!(&BTRFS_SEND_STREAM_MAGIC[..12], b"btrfs-stream");
    }

    #[test]
    fn test_stream_version_is_v1() {
        assert_eq!(BTRFS_SEND_STREAM_VERSION, 1);
    }

    #[test]
    fn test_send_cmd_dense_0_to_22() {
        let c = [
            BTRFS_SEND_C_UNSPEC,
            BTRFS_SEND_C_SUBVOL,
            BTRFS_SEND_C_SNAPSHOT,
            BTRFS_SEND_C_MKFILE,
            BTRFS_SEND_C_MKDIR,
            BTRFS_SEND_C_MKNOD,
            BTRFS_SEND_C_MKFIFO,
            BTRFS_SEND_C_MKSOCK,
            BTRFS_SEND_C_SYMLINK,
            BTRFS_SEND_C_RENAME,
            BTRFS_SEND_C_LINK,
            BTRFS_SEND_C_UNLINK,
            BTRFS_SEND_C_RMDIR,
            BTRFS_SEND_C_SET_XATTR,
            BTRFS_SEND_C_REMOVE_XATTR,
            BTRFS_SEND_C_WRITE,
            BTRFS_SEND_C_CLONE,
            BTRFS_SEND_C_TRUNCATE,
            BTRFS_SEND_C_CHMOD,
            BTRFS_SEND_C_CHOWN,
            BTRFS_SEND_C_UTIMES,
            BTRFS_SEND_C_END,
            BTRFS_SEND_C_UPDATE_EXTENT,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(BTRFS_SEND_C_MAX, BTRFS_SEND_C_UPDATE_EXTENT);
    }

    #[test]
    fn test_send_attr_dense_0_to_15() {
        let a = [
            BTRFS_SEND_A_UNSPEC,
            BTRFS_SEND_A_UUID,
            BTRFS_SEND_A_CTRANSID,
            BTRFS_SEND_A_INO,
            BTRFS_SEND_A_SIZE,
            BTRFS_SEND_A_MODE,
            BTRFS_SEND_A_UID,
            BTRFS_SEND_A_GID,
            BTRFS_SEND_A_RDEV,
            BTRFS_SEND_A_CTIME,
            BTRFS_SEND_A_MTIME,
            BTRFS_SEND_A_ATIME,
            BTRFS_SEND_A_OTIME,
            BTRFS_SEND_A_XATTR_NAME,
            BTRFS_SEND_A_XATTR_DATA,
            BTRFS_SEND_A_PATH,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_time_attrs_clustered_9_to_12() {
        // The four timestamps are adjacent in the table.
        for v in [
            BTRFS_SEND_A_CTIME,
            BTRFS_SEND_A_MTIME,
            BTRFS_SEND_A_ATIME,
            BTRFS_SEND_A_OTIME,
        ] {
            assert!((9..=12).contains(&v));
        }
    }

    #[test]
    fn test_recv_flag_single_bit() {
        assert!(BTRFS_RECV_FLAG_NO_CLEAN.is_power_of_two());
    }
}
