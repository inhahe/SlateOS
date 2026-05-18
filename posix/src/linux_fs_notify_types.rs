//! `<linux/fanotify.h>` / `<linux/inotify.h>` — Filesystem notification constants.
//!
//! inotify and fanotify provide filesystem event monitoring.
//! These constants define event masks, flags, and control values.

// ---------------------------------------------------------------------------
// inotify events (IN_*)
// ---------------------------------------------------------------------------

/// File was accessed.
pub const IN_ACCESS: u32 = 0x00000001;
/// File was modified.
pub const IN_MODIFY: u32 = 0x00000002;
/// Metadata changed.
pub const IN_ATTRIB: u32 = 0x00000004;
/// Writable file was closed.
pub const IN_CLOSE_WRITE: u32 = 0x00000008;
/// Non-writable file was closed.
pub const IN_CLOSE_NOWRITE: u32 = 0x00000010;
/// File was opened.
pub const IN_OPEN: u32 = 0x00000020;
/// File was moved from.
pub const IN_MOVED_FROM: u32 = 0x00000040;
/// File was moved to.
pub const IN_MOVED_TO: u32 = 0x00000080;
/// Subfile was created.
pub const IN_CREATE: u32 = 0x00000100;
/// Subfile was deleted.
pub const IN_DELETE: u32 = 0x00000200;
/// Self was deleted.
pub const IN_DELETE_SELF: u32 = 0x00000400;
/// Self was moved.
pub const IN_MOVE_SELF: u32 = 0x00000800;

// ---------------------------------------------------------------------------
// inotify combined masks
// ---------------------------------------------------------------------------

/// Close events (write + nowrite).
pub const IN_CLOSE: u32 = IN_CLOSE_WRITE | IN_CLOSE_NOWRITE;
/// Move events (from + to).
pub const IN_MOVE: u32 = IN_MOVED_FROM | IN_MOVED_TO;
/// All events.
pub const IN_ALL_EVENTS: u32 = IN_ACCESS | IN_MODIFY | IN_ATTRIB
    | IN_CLOSE_WRITE | IN_CLOSE_NOWRITE | IN_OPEN
    | IN_MOVED_FROM | IN_MOVED_TO | IN_CREATE
    | IN_DELETE | IN_DELETE_SELF | IN_MOVE_SELF;

// ---------------------------------------------------------------------------
// inotify watch flags
// ---------------------------------------------------------------------------

/// Only report events once.
pub const IN_ONESHOT: u32 = 0x80000000;
/// Only watch path if directory.
pub const IN_ONLYDIR: u32 = 0x01000000;
/// Don't follow symlinks.
pub const IN_DONT_FOLLOW: u32 = 0x02000000;
/// Exclude events on unlinked objects.
pub const IN_EXCL_UNLINK: u32 = 0x04000000;
/// Add to existing watch mask.
pub const IN_MASK_ADD: u32 = 0x20000000;
/// Event occurred on directory.
pub const IN_ISDIR: u32 = 0x40000000;

// ---------------------------------------------------------------------------
// inotify init flags
// ---------------------------------------------------------------------------

/// Non-blocking.
pub const IN_NONBLOCK: u32 = 0x00000800;
/// Close-on-exec.
pub const IN_CLOEXEC: u32 = 0x00080000;

// ---------------------------------------------------------------------------
// fanotify event masks (FAN_*)
// ---------------------------------------------------------------------------

/// File was accessed.
pub const FAN_ACCESS: u64 = 0x00000001;
/// File was modified.
pub const FAN_MODIFY: u64 = 0x00000002;
/// Metadata changed.
pub const FAN_ATTRIB: u64 = 0x00000004;
/// Writable file closed.
pub const FAN_CLOSE_WRITE: u64 = 0x00000008;
/// Non-writable file closed.
pub const FAN_CLOSE_NOWRITE: u64 = 0x00000010;
/// File was opened.
pub const FAN_OPEN: u64 = 0x00000020;
/// File moved from.
pub const FAN_MOVED_FROM: u64 = 0x00000040;
/// File moved to.
pub const FAN_MOVED_TO: u64 = 0x00000080;
/// Subfile created.
pub const FAN_CREATE: u64 = 0x00000100;
/// Subfile deleted.
pub const FAN_DELETE: u64 = 0x00000200;
/// Self was deleted.
pub const FAN_DELETE_SELF: u64 = 0x00000400;
/// Self was moved.
pub const FAN_MOVE_SELF: u64 = 0x00000800;
/// File was opened for exec.
pub const FAN_OPEN_EXEC: u64 = 0x00001000;

// ---------------------------------------------------------------------------
// fanotify permission events
// ---------------------------------------------------------------------------

/// Permission to open.
pub const FAN_OPEN_PERM: u64 = 0x00010000;
/// Permission to access.
pub const FAN_ACCESS_PERM: u64 = 0x00020000;
/// Permission to open for exec.
pub const FAN_OPEN_EXEC_PERM: u64 = 0x00040000;

// ---------------------------------------------------------------------------
// fanotify special events
// ---------------------------------------------------------------------------

/// Overflow event.
pub const FAN_Q_OVERFLOW: u64 = 0x00004000;
/// Filesystem error.
pub const FAN_FS_ERROR: u64 = 0x00008000;
/// Rename event.
pub const FAN_RENAME: u64 = 0x10000000;

// ---------------------------------------------------------------------------
// fanotify init flags
// ---------------------------------------------------------------------------

/// Class: notification only.
pub const FAN_CLASS_NOTIF: u32 = 0x00000000;
/// Class: content permission.
pub const FAN_CLASS_CONTENT: u32 = 0x00000004;
/// Class: pre-content permission.
pub const FAN_CLASS_PRE_CONTENT: u32 = 0x00000008;
/// Close-on-exec.
pub const FAN_CLOEXEC: u32 = 0x00000001;
/// Non-blocking.
pub const FAN_NONBLOCK: u32 = 0x00000002;
/// Unlimited queue.
pub const FAN_UNLIMITED_QUEUE: u32 = 0x00000010;
/// Unlimited marks.
pub const FAN_UNLIMITED_MARKS: u32 = 0x00000020;
/// Enable audit.
pub const FAN_ENABLE_AUDIT: u32 = 0x00000040;
/// Report FID.
pub const FAN_REPORT_FID: u32 = 0x00000200;
/// Report directory FID.
pub const FAN_REPORT_DIR_FID: u32 = 0x00000400;
/// Report name.
pub const FAN_REPORT_NAME: u32 = 0x00000800;
/// Report target FID.
pub const FAN_REPORT_TARGET_FID: u32 = 0x00001000;
/// Report pidfd.
pub const FAN_REPORT_PIDFD: u32 = 0x00000080;

// ---------------------------------------------------------------------------
// fanotify mark flags
// ---------------------------------------------------------------------------

/// Add mark.
pub const FAN_MARK_ADD: u32 = 0x00000001;
/// Remove mark.
pub const FAN_MARK_REMOVE: u32 = 0x00000002;
/// Don't follow symlinks.
pub const FAN_MARK_DONT_FOLLOW: u32 = 0x00000004;
/// Only watch directories.
pub const FAN_MARK_ONLYDIR: u32 = 0x00000008;
/// Remove all marks on inode.
pub const FAN_MARK_IGNORED_MASK: u32 = 0x00000020;
/// Survive inode mask.
pub const FAN_MARK_IGNORED_SURV_MODIFY: u32 = 0x00000040;
/// Flush marks.
pub const FAN_MARK_FLUSH: u32 = 0x00000080;
/// Evictable mark.
pub const FAN_MARK_EVICTABLE: u32 = 0x00000200;
/// Ignore surv modify (new API).
pub const FAN_MARK_IGNORE: u32 = 0x00000400;

// ---------------------------------------------------------------------------
// fanotify mark target types
// ---------------------------------------------------------------------------

/// Mark inode.
pub const FAN_MARK_INODE: u32 = 0x00000000;
/// Mark mount.
pub const FAN_MARK_MOUNT: u32 = 0x00000010;
/// Mark filesystem.
pub const FAN_MARK_FILESYSTEM: u32 = 0x00000100;

// ---------------------------------------------------------------------------
// fanotify response values
// ---------------------------------------------------------------------------

/// Allow the access.
pub const FAN_ALLOW: u32 = 0x01;
/// Deny the access.
pub const FAN_DENY: u32 = 0x02;
/// Audit the access.
pub const FAN_AUDIT: u32 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inotify_events_distinct() {
        let events = [
            IN_ACCESS, IN_MODIFY, IN_ATTRIB,
            IN_CLOSE_WRITE, IN_CLOSE_NOWRITE, IN_OPEN,
            IN_MOVED_FROM, IN_MOVED_TO, IN_CREATE,
            IN_DELETE, IN_DELETE_SELF, IN_MOVE_SELF,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_inotify_events_power_of_two() {
        let events = [
            IN_ACCESS, IN_MODIFY, IN_ATTRIB,
            IN_CLOSE_WRITE, IN_CLOSE_NOWRITE, IN_OPEN,
            IN_MOVED_FROM, IN_MOVED_TO, IN_CREATE,
            IN_DELETE, IN_DELETE_SELF, IN_MOVE_SELF,
        ];
        for e in &events {
            assert!(e.is_power_of_two(), "0x{:08x} is not power of two", e);
        }
    }

    #[test]
    fn test_close_combined() {
        assert_eq!(IN_CLOSE, IN_CLOSE_WRITE | IN_CLOSE_NOWRITE);
    }

    #[test]
    fn test_move_combined() {
        assert_eq!(IN_MOVE, IN_MOVED_FROM | IN_MOVED_TO);
    }

    #[test]
    fn test_all_events_includes_all() {
        assert_ne!(IN_ALL_EVENTS & IN_ACCESS, 0);
        assert_ne!(IN_ALL_EVENTS & IN_DELETE_SELF, 0);
        assert_ne!(IN_ALL_EVENTS & IN_MOVE_SELF, 0);
    }

    #[test]
    fn test_fanotify_events_distinct() {
        let events: [u64; 13] = [
            FAN_ACCESS, FAN_MODIFY, FAN_ATTRIB,
            FAN_CLOSE_WRITE, FAN_CLOSE_NOWRITE, FAN_OPEN,
            FAN_MOVED_FROM, FAN_MOVED_TO, FAN_CREATE,
            FAN_DELETE, FAN_DELETE_SELF, FAN_MOVE_SELF,
            FAN_OPEN_EXEC,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_fan_perm_events_distinct() {
        let perms: [u64; 3] = [FAN_OPEN_PERM, FAN_ACCESS_PERM, FAN_OPEN_EXEC_PERM];
        for i in 0..perms.len() {
            for j in (i + 1)..perms.len() {
                assert_ne!(perms[i], perms[j]);
            }
        }
    }

    #[test]
    fn test_fan_class_values() {
        assert_eq!(FAN_CLASS_NOTIF, 0);
        assert_ne!(FAN_CLASS_CONTENT, FAN_CLASS_PRE_CONTENT);
    }

    #[test]
    fn test_fan_mark_flags_distinct() {
        let flags = [
            FAN_MARK_ADD, FAN_MARK_REMOVE, FAN_MARK_DONT_FOLLOW,
            FAN_MARK_ONLYDIR, FAN_MARK_IGNORED_MASK,
            FAN_MARK_IGNORED_SURV_MODIFY, FAN_MARK_FLUSH,
            FAN_MARK_EVICTABLE, FAN_MARK_IGNORE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_fan_mark_targets_distinct() {
        let targets = [FAN_MARK_INODE, FAN_MARK_MOUNT, FAN_MARK_FILESYSTEM];
        for i in 0..targets.len() {
            for j in (i + 1)..targets.len() {
                assert_ne!(targets[i], targets[j]);
            }
        }
    }

    #[test]
    fn test_fan_responses() {
        assert_eq!(FAN_ALLOW, 0x01);
        assert_eq!(FAN_DENY, 0x02);
        assert_eq!(FAN_AUDIT, 0x10);
    }

    #[test]
    fn test_inotify_init_flags() {
        assert_ne!(IN_NONBLOCK, IN_CLOEXEC);
    }
}
