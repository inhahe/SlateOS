//! `<linux/fsnotify.h>` — Filesystem notification core constants.
//!
//! fsnotify is the internal kernel framework that unifies filesystem
//! event notification backends (inotify, fanotify, dnotify). It
//! provides a common event delivery infrastructure with connector
//! objects on inodes and mounts. Each backend wraps fsnotify to
//! provide its own userspace API. The framework handles event
//! coalescing, overflow reporting, and permission checking.

// ---------------------------------------------------------------------------
// fsnotify event mask bits (shared between inotify/fanotify)
// ---------------------------------------------------------------------------

/// File was accessed (read).
pub const FS_ACCESS: u32 = 0x0000_0001;
/// File was modified (write).
pub const FS_MODIFY: u32 = 0x0000_0002;
/// File attributes changed (chmod, chown, touch).
pub const FS_ATTRIB: u32 = 0x0000_0004;
/// File opened for writing was closed.
pub const FS_CLOSE_WRITE: u32 = 0x0000_0008;
/// File not opened for writing was closed.
pub const FS_CLOSE_NOWRITE: u32 = 0x0000_0010;
/// File was opened.
pub const FS_OPEN: u32 = 0x0000_0020;
/// File was moved from this directory.
pub const FS_MOVED_FROM: u32 = 0x0000_0040;
/// File was moved to this directory.
pub const FS_MOVED_TO: u32 = 0x0000_0080;
/// Subfile was created in directory.
pub const FS_CREATE: u32 = 0x0000_0100;
/// Subfile was deleted from directory.
pub const FS_DELETE: u32 = 0x0000_0200;
/// The watched item itself was deleted.
pub const FS_DELETE_SELF: u32 = 0x0000_0400;
/// The watched item itself was moved.
pub const FS_MOVE_SELF: u32 = 0x0000_0800;

// ---------------------------------------------------------------------------
// fsnotify special flags
// ---------------------------------------------------------------------------

/// Event is on a directory (not a file).
pub const FS_ISDIR: u32 = 0x4000_0000;
/// Event queue overflowed.
pub const FS_Q_OVERFLOW: u32 = 0x0000_4000;
/// Watch was removed (cleanup).
pub const FS_IGNORED: u32 = 0x0000_8000;

// ---------------------------------------------------------------------------
// fsnotify group types (internal)
// ---------------------------------------------------------------------------

/// inotify backend.
pub const FSNOTIFY_GROUP_INOTIFY: u32 = 0;
/// fanotify backend.
pub const FSNOTIFY_GROUP_FANOTIFY: u32 = 1;
/// dnotify backend.
pub const FSNOTIFY_GROUP_DNOTIFY: u32 = 2;
/// Audit backend.
pub const FSNOTIFY_GROUP_AUDIT: u32 = 3;

// ---------------------------------------------------------------------------
// fsnotify connector types
// ---------------------------------------------------------------------------

/// Connector is attached to an inode.
pub const FSNOTIFY_OBJ_INODE: u32 = 0;
/// Connector is attached to a mount.
pub const FSNOTIFY_OBJ_MOUNT: u32 = 1;
/// Connector is attached to a super_block.
pub const FSNOTIFY_OBJ_SB: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_bits_no_overlap() {
        let events = [
            FS_ACCESS, FS_MODIFY, FS_ATTRIB, FS_CLOSE_WRITE,
            FS_CLOSE_NOWRITE, FS_OPEN, FS_MOVED_FROM, FS_MOVED_TO,
            FS_CREATE, FS_DELETE, FS_DELETE_SELF, FS_MOVE_SELF,
        ];
        for i in 0..events.len() {
            assert!(events[i].is_power_of_two());
            for j in (i + 1)..events.len() {
                assert_eq!(events[i] & events[j], 0);
            }
        }
    }

    #[test]
    fn test_group_types_distinct() {
        let groups = [
            FSNOTIFY_GROUP_INOTIFY, FSNOTIFY_GROUP_FANOTIFY,
            FSNOTIFY_GROUP_DNOTIFY, FSNOTIFY_GROUP_AUDIT,
        ];
        for i in 0..groups.len() {
            for j in (i + 1)..groups.len() {
                assert_ne!(groups[i], groups[j]);
            }
        }
    }

    #[test]
    fn test_connector_types_distinct() {
        let types = [
            FSNOTIFY_OBJ_INODE, FSNOTIFY_OBJ_MOUNT, FSNOTIFY_OBJ_SB,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
