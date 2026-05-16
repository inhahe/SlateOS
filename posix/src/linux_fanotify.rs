//! `<linux/fanotify.h>` — filesystem-wide notification API.
//!
//! fanotify provides filesystem notification events (similar to inotify
//! but more powerful): it can monitor entire mount points, filter by
//! file type, and support permission events (approve/deny access).

use crate::errno;

// ---------------------------------------------------------------------------
// fanotify_init() flags (class + additional)
// ---------------------------------------------------------------------------

/// Pre-content class (permission before write).
pub const FAN_CLASS_PRE_CONTENT: u32 = 0x00000008;
/// Content class (permission event, blocking).
pub const FAN_CLASS_CONTENT: u32 = 0x00000004;
/// Notification class (no permission events).
pub const FAN_CLASS_NOTIF: u32 = 0x00000000;

/// Close-on-exec flag for fanotify fd.
pub const FAN_CLOEXEC: u32 = 0x00000001;
/// Non-blocking flag for fanotify fd.
pub const FAN_NONBLOCK: u32 = 0x00000002;
/// Unlimited queue.
pub const FAN_UNLIMITED_QUEUE: u32 = 0x00000010;
/// Unlimited marks.
pub const FAN_UNLIMITED_MARKS: u32 = 0x00000020;
/// Enable fanotify event info (FID).
pub const FAN_ENABLE_AUDIT: u32 = 0x00000040;
/// Report FID instead of fd.
pub const FAN_REPORT_FID: u32 = 0x00000200;
/// Report directory FID.
pub const FAN_REPORT_DIR_FID: u32 = 0x00000400;
/// Report event name.
pub const FAN_REPORT_NAME: u32 = 0x00000800;
/// Report target FID.
pub const FAN_REPORT_TARGET_FID: u32 = 0x00001000;
/// Convenience: FID + DIR_FID + NAME.
pub const FAN_REPORT_DFID_NAME: u32 = FAN_REPORT_DIR_FID | FAN_REPORT_NAME;
/// Convenience: FID + DIR_FID + NAME + TARGET_FID.
pub const FAN_REPORT_DFID_NAME_TARGET: u32 =
    FAN_REPORT_DFID_NAME | FAN_REPORT_FID | FAN_REPORT_TARGET_FID;

// ---------------------------------------------------------------------------
// fanotify event mask bits
// ---------------------------------------------------------------------------

/// File was accessed.
pub const FAN_ACCESS: u64 = 0x00000001;
/// File was modified.
pub const FAN_MODIFY: u64 = 0x00000002;
/// Metadata changed.
pub const FAN_ATTRIB: u64 = 0x00000004;
/// Writable file was closed.
pub const FAN_CLOSE_WRITE: u64 = 0x00000008;
/// Non-writable file was closed.
pub const FAN_CLOSE_NOWRITE: u64 = 0x00000010;
/// File was opened.
pub const FAN_OPEN: u64 = 0x00000020;
/// File was moved from this directory.
pub const FAN_MOVED_FROM: u64 = 0x00000040;
/// File was moved to this directory.
pub const FAN_MOVED_TO: u64 = 0x00000080;
/// Subfile was created.
pub const FAN_CREATE: u64 = 0x00000100;
/// Subfile was deleted.
pub const FAN_DELETE: u64 = 0x00000200;
/// Self was deleted.
pub const FAN_DELETE_SELF: u64 = 0x00000400;
/// Self was moved.
pub const FAN_MOVE_SELF: u64 = 0x00000800;
/// File was opened for exec.
pub const FAN_OPEN_EXEC: u64 = 0x00001000;

/// Convenience: close = CLOSE_WRITE | CLOSE_NOWRITE.
pub const FAN_CLOSE: u64 = FAN_CLOSE_WRITE | FAN_CLOSE_NOWRITE;
/// Convenience: move = MOVED_FROM | MOVED_TO.
pub const FAN_MOVE: u64 = FAN_MOVED_FROM | FAN_MOVED_TO;

// Permission events
/// Permission: file opened.
pub const FAN_OPEN_PERM: u64 = 0x00010000;
/// Permission: file accessed.
pub const FAN_ACCESS_PERM: u64 = 0x00020000;
/// Permission: file opened for exec.
pub const FAN_OPEN_EXEC_PERM: u64 = 0x00040000;

/// Overflow: event queue overflowed.
pub const FAN_Q_OVERFLOW: u64 = 0x00004000;
/// Event on a child.
pub const FAN_ONDIR: u64 = 0x40000000;
/// Event occurred against dir.
pub const FAN_EVENT_ON_CHILD: u64 = 0x08000000;

// ---------------------------------------------------------------------------
// fanotify_mark() flags
// ---------------------------------------------------------------------------

/// Add to mark mask.
pub const FAN_MARK_ADD: u32 = 0x00000001;
/// Remove from mark mask.
pub const FAN_MARK_REMOVE: u32 = 0x00000002;
/// Don't follow symlinks.
pub const FAN_MARK_DONT_FOLLOW: u32 = 0x00000004;
/// Only create on directories.
pub const FAN_MARK_ONLYDIR: u32 = 0x00000008;
/// Mark the inode.
pub const FAN_MARK_INODE: u32 = 0x00000000;
/// Mark the mount.
pub const FAN_MARK_MOUNT: u32 = 0x00000010;
/// Mark the filesystem.
pub const FAN_MARK_FILESYSTEM: u32 = 0x00000100;
/// Remove all marks.
pub const FAN_MARK_FLUSH: u32 = 0x00000080;
/// Ignore mask (events to ignore).
pub const FAN_MARK_IGNORED_MASK: u32 = 0x00000020;
/// Survive modify events.
pub const FAN_MARK_IGNORED_SURV_MODIFY: u32 = 0x00000040;
/// Ignore events on children.
pub const FAN_MARK_EVICTABLE: u32 = 0x00000200;

// ---------------------------------------------------------------------------
// fanotify response (for permission events)
// ---------------------------------------------------------------------------

/// Allow the file operation.
pub const FAN_ALLOW: u32 = 0x01;
/// Deny the file operation.
pub const FAN_DENY: u32 = 0x02;
/// Audit the access decision.
pub const FAN_AUDIT: u32 = 0x10;

// ---------------------------------------------------------------------------
// fanotify event metadata
// ---------------------------------------------------------------------------

/// Fanotify event metadata (24 bytes on 64-bit).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FanotifyEventMetadata {
    /// Total event length.
    pub event_len: u32,
    /// Metadata version.
    pub vers: u8,
    /// Reserved.
    pub reserved: u8,
    /// Metadata length.
    pub metadata_len: u16,
    /// Event mask.
    pub mask: u64,
    /// File descriptor.
    pub fd: i32,
    /// Process ID.
    pub pid: i32,
}

/// Current metadata version.
pub const FANOTIFY_METADATA_VERSION: u8 = 3;

// ---------------------------------------------------------------------------
// Stubs
// ---------------------------------------------------------------------------

/// Initialize fanotify.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fanotify_init(_flags: u32, _event_f_flags: u32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Modify fanotify marks.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fanotify_mark(
    _fanotify_fd: i32,
    _flags: u32,
    _mask: u64,
    _dirfd: i32,
    _pathname: *const u8,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_metadata_size() {
        assert_eq!(core::mem::size_of::<FanotifyEventMetadata>(), 24);
    }

    #[test]
    fn test_class_flags() {
        assert_eq!(FAN_CLASS_NOTIF, 0);
        assert_ne!(FAN_CLASS_CONTENT, FAN_CLASS_PRE_CONTENT);
        assert_ne!(FAN_CLASS_CONTENT, FAN_CLASS_NOTIF);
    }

    #[test]
    fn test_event_masks_distinct() {
        let masks = [
            FAN_ACCESS, FAN_MODIFY, FAN_ATTRIB,
            FAN_CLOSE_WRITE, FAN_CLOSE_NOWRITE, FAN_OPEN,
            FAN_MOVED_FROM, FAN_MOVED_TO, FAN_CREATE,
            FAN_DELETE, FAN_DELETE_SELF, FAN_MOVE_SELF,
            FAN_OPEN_EXEC,
        ];
        for i in 0..masks.len() {
            for j in (i + 1)..masks.len() {
                assert_ne!(masks[i], masks[j]);
            }
        }
    }

    #[test]
    fn test_convenience_masks() {
        assert_eq!(FAN_CLOSE, FAN_CLOSE_WRITE | FAN_CLOSE_NOWRITE);
        assert_eq!(FAN_MOVE, FAN_MOVED_FROM | FAN_MOVED_TO);
    }

    #[test]
    fn test_mark_flags_distinct() {
        let flags = [
            FAN_MARK_ADD, FAN_MARK_REMOVE, FAN_MARK_DONT_FOLLOW,
            FAN_MARK_ONLYDIR, FAN_MARK_MOUNT, FAN_MARK_FILESYSTEM,
            FAN_MARK_FLUSH, FAN_MARK_IGNORED_MASK,
            FAN_MARK_IGNORED_SURV_MODIFY, FAN_MARK_EVICTABLE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_response_values() {
        assert_eq!(FAN_ALLOW, 0x01);
        assert_eq!(FAN_DENY, 0x02);
        assert_eq!(FAN_AUDIT, 0x10);
    }

    #[test]
    fn test_fanotify_init_stub() {
        let ret = fanotify_init(FAN_CLASS_NOTIF | FAN_CLOEXEC, 0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_fanotify_mark_stub() {
        let ret = fanotify_mark(-1, FAN_MARK_ADD, FAN_OPEN, -1, core::ptr::null());
        assert_eq!(ret, -1);
    }
}
