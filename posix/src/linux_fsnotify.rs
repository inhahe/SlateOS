//! `<linux/fsnotify.h>` — Filesystem notification infrastructure constants.
//!
//! fsnotify is the kernel's internal notification infrastructure
//! that underlies both inotify and fanotify. It provides common
//! event types and group/mark management used by both frontends.

// ---------------------------------------------------------------------------
// fsnotify event types (internal kernel events)
// ---------------------------------------------------------------------------

/// Object was accessed.
pub const FS_ACCESS: u32 = 0x00000001;
/// Object was modified.
pub const FS_MODIFY: u32 = 0x00000002;
/// Metadata changed.
pub const FS_ATTRIB: u32 = 0x00000004;
/// Writable close.
pub const FS_CLOSE_WRITE: u32 = 0x00000008;
/// Non-writable close.
pub const FS_CLOSE_NOWRITE: u32 = 0x00000010;
/// Object opened.
pub const FS_OPEN: u32 = 0x00000020;
/// Moved from.
pub const FS_MOVED_FROM: u32 = 0x00000040;
/// Moved to.
pub const FS_MOVED_TO: u32 = 0x00000080;
/// Created.
pub const FS_CREATE: u32 = 0x00000100;
/// Deleted.
pub const FS_DELETE: u32 = 0x00000200;
/// Self was deleted.
pub const FS_DELETE_SELF: u32 = 0x00000400;
/// Self was moved.
pub const FS_MOVE_SELF: u32 = 0x00000800;
/// Object opened for exec.
pub const FS_OPEN_EXEC: u32 = 0x00001000;

// ---------------------------------------------------------------------------
// fsnotify event info types
// ---------------------------------------------------------------------------

/// Event carries path info.
pub const FSNOTIFY_EVENT_PATH: u32 = 0;
/// Event carries inode info.
pub const FSNOTIFY_EVENT_INODE: u32 = 1;
/// Event carries error info.
pub const FSNOTIFY_EVENT_ERROR: u32 = 2;

// ---------------------------------------------------------------------------
// fsnotify mark types
// ---------------------------------------------------------------------------

/// Mark on an inode.
pub const FSNOTIFY_OBJ_TYPE_INODE: u32 = 0;
/// Mark on a mount.
pub const FSNOTIFY_OBJ_TYPE_VFSMOUNT: u32 = 1;
/// Mark on a superblock (filesystem-wide).
pub const FSNOTIFY_OBJ_TYPE_SB: u32 = 2;

/// Number of mark types.
pub const FSNOTIFY_OBJ_TYPE_COUNT: u32 = 3;

// ---------------------------------------------------------------------------
// fsnotify group priorities
// ---------------------------------------------------------------------------

/// Normal priority (inotify).
pub const FSNOTIFY_PRIO_NORMAL: u32 = 0;
/// Content-based (fanotify permission events).
pub const FSNOTIFY_PRIO_CONTENT: u32 = 1;
/// Pre-content (fanotify pre-access).
pub const FSNOTIFY_PRIO_PRE_CONTENT: u32 = 2;

/// Number of priority levels.
pub const FSNOTIFY_PRIO_COUNT: u32 = 3;

// ---------------------------------------------------------------------------
// Event queue limits
// ---------------------------------------------------------------------------

/// Default maximum queued events (inotify).
pub const FSNOTIFY_DEFAULT_MAX_EVENTS: u32 = 16384;

/// Default maximum user instances.
pub const FSNOTIFY_DEFAULT_MAX_INSTANCES: u32 = 128;

/// Default maximum user watches.
pub const FSNOTIFY_DEFAULT_MAX_WATCHES: u32 = 65536;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_powers_of_two() {
        let events = [
            FS_ACCESS, FS_MODIFY, FS_ATTRIB, FS_CLOSE_WRITE,
            FS_CLOSE_NOWRITE, FS_OPEN, FS_MOVED_FROM, FS_MOVED_TO,
            FS_CREATE, FS_DELETE, FS_DELETE_SELF, FS_MOVE_SELF,
            FS_OPEN_EXEC,
        ];
        for event in &events {
            assert!(event.is_power_of_two(), "0x{:x}", event);
        }
    }

    #[test]
    fn test_event_types_no_overlap() {
        let events = [
            FS_ACCESS, FS_MODIFY, FS_ATTRIB, FS_CLOSE_WRITE,
            FS_CLOSE_NOWRITE, FS_OPEN, FS_MOVED_FROM, FS_MOVED_TO,
            FS_CREATE, FS_DELETE, FS_DELETE_SELF, FS_MOVE_SELF,
            FS_OPEN_EXEC,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_eq!(events[i] & events[j], 0);
            }
        }
    }

    #[test]
    fn test_event_info_types_distinct() {
        let types = [FSNOTIFY_EVENT_PATH, FSNOTIFY_EVENT_INODE, FSNOTIFY_EVENT_ERROR];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_obj_types_distinct() {
        let types = [
            FSNOTIFY_OBJ_TYPE_INODE,
            FSNOTIFY_OBJ_TYPE_VFSMOUNT,
            FSNOTIFY_OBJ_TYPE_SB,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
        // All less than count
        for t in &types {
            assert!(*t < FSNOTIFY_OBJ_TYPE_COUNT);
        }
    }

    #[test]
    fn test_priorities_ordered() {
        assert!(FSNOTIFY_PRIO_NORMAL < FSNOTIFY_PRIO_CONTENT);
        assert!(FSNOTIFY_PRIO_CONTENT < FSNOTIFY_PRIO_PRE_CONTENT);
    }

    #[test]
    fn test_queue_limits() {
        assert!(FSNOTIFY_DEFAULT_MAX_EVENTS > 0);
        assert!(FSNOTIFY_DEFAULT_MAX_INSTANCES > 0);
        assert!(FSNOTIFY_DEFAULT_MAX_WATCHES > 0);
    }
}
