//! `<linux/fanotify.h>` — fanotify mark flag and type constants.
//!
//! `fanotify_mark()` adds, modifies, or removes fanotify marks on
//! filesystem objects. Marks determine which events are monitored.
//! These constants define the mark types (inode, mount, filesystem)
//! and the mark manipulation flags.

// ---------------------------------------------------------------------------
// fanotify_mark() flag types (what to mark)
// ---------------------------------------------------------------------------

/// Add events to the mark.
pub const FAN_MARK_ADD: u32 = 0x0000_0001;
/// Remove events from the mark.
pub const FAN_MARK_REMOVE: u32 = 0x0000_0002;
/// Don't follow symbolic links.
pub const FAN_MARK_DONT_FOLLOW: u32 = 0x0000_0004;
/// Only mark if path is a directory.
pub const FAN_MARK_ONLYDIR: u32 = 0x0000_0008;
/// Remove all marks for the filesystem.
pub const FAN_MARK_FLUSH: u32 = 0x0000_0080;
/// Ignore mask: don't generate events.
pub const FAN_MARK_IGNORED_MASK: u32 = 0x0000_0020;
/// Ignore mask survives modify events.
pub const FAN_MARK_IGNORED_SURV_MODIFY: u32 = 0x0000_0040;
/// Evictable mark (drop on memory pressure).
pub const FAN_MARK_EVICTABLE: u32 = 0x0000_0200;
/// Ignore surrogates (use with IGNORED_MASK).
pub const FAN_MARK_IGNORE: u32 = 0x0000_0400;

// ---------------------------------------------------------------------------
// fanotify_mark() mark types (which object)
// ---------------------------------------------------------------------------

/// Mark applies to an inode.
pub const FAN_MARK_INODE: u32 = 0x0000_0000;
/// Mark applies to a mount.
pub const FAN_MARK_MOUNT: u32 = 0x0000_0010;
/// Mark applies to entire filesystem.
pub const FAN_MARK_FILESYSTEM: u32 = 0x0000_0100;

// ---------------------------------------------------------------------------
// fanotify event flags
// ---------------------------------------------------------------------------

/// File was accessed.
pub const FAN_ACCESS: u64 = 0x0000_0001;
/// File was modified.
pub const FAN_MODIFY: u64 = 0x0000_0002;
/// File was closed (write mode).
pub const FAN_CLOSE_WRITE: u64 = 0x0000_0008;
/// File was closed (read-only mode).
pub const FAN_CLOSE_NOWRITE: u64 = 0x0000_0010;
/// File was opened.
pub const FAN_OPEN: u64 = 0x0000_0020;
/// Permission: allow/deny open.
pub const FAN_OPEN_PERM: u64 = 0x0001_0000;
/// Permission: allow/deny access.
pub const FAN_ACCESS_PERM: u64 = 0x0002_0000;
/// Permission: allow/deny open for exec.
pub const FAN_OPEN_EXEC_PERM: u64 = 0x0004_0000;
/// File was opened for execution.
pub const FAN_OPEN_EXEC: u64 = 0x0000_1000;
/// File was created.
pub const FAN_CREATE: u64 = 0x0000_0100;
/// File was deleted.
pub const FAN_DELETE: u64 = 0x0000_0200;
/// File was renamed.
pub const FAN_RENAME: u64 = 0x1000_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mark_flags_distinct() {
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
    fn test_mark_types_distinct() {
        let types = [FAN_MARK_INODE, FAN_MARK_MOUNT, FAN_MARK_FILESYSTEM];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_event_flags_distinct() {
        let events = [
            FAN_ACCESS, FAN_MODIFY, FAN_CLOSE_WRITE,
            FAN_CLOSE_NOWRITE, FAN_OPEN, FAN_OPEN_PERM,
            FAN_ACCESS_PERM, FAN_OPEN_EXEC_PERM, FAN_OPEN_EXEC,
            FAN_CREATE, FAN_DELETE, FAN_RENAME,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_mark_inode_is_zero() {
        assert_eq!(FAN_MARK_INODE, 0);
    }

    #[test]
    fn test_mark_add_is_one() {
        assert_eq!(FAN_MARK_ADD, 1);
    }
}
