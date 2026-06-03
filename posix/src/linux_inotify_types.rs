//! `<linux/inotify.h>` — inotify event mask constants.
//!
//! inotify monitors filesystem events (file creation, modification,
//! deletion, etc.) on individual files or directories. It is the
//! primary mechanism for filesystem change notification on Linux.

// ---------------------------------------------------------------------------
// inotify event masks (IN_*)
// ---------------------------------------------------------------------------

/// File was accessed (read).
pub const IN_ACCESS: u32 = 0x00000001;
/// File was modified (write).
pub const IN_MODIFY: u32 = 0x00000002;
/// File metadata changed (chmod, chown, etc.).
pub const IN_ATTRIB: u32 = 0x00000004;
/// Writable file was closed.
pub const IN_CLOSE_WRITE: u32 = 0x00000008;
/// Non-writable file was closed.
pub const IN_CLOSE_NOWRITE: u32 = 0x00000010;
/// File was opened.
pub const IN_OPEN: u32 = 0x00000020;
/// File moved from watched directory.
pub const IN_MOVED_FROM: u32 = 0x00000040;
/// File moved into watched directory.
pub const IN_MOVED_TO: u32 = 0x00000080;
/// File/directory created in watched directory.
pub const IN_CREATE: u32 = 0x00000100;
/// File/directory deleted from watched directory.
pub const IN_DELETE: u32 = 0x00000200;
/// Watched file/directory was itself deleted.
pub const IN_DELETE_SELF: u32 = 0x00000400;
/// Watched file/directory was itself moved.
pub const IN_MOVE_SELF: u32 = 0x00000800;

// ---------------------------------------------------------------------------
// Combined convenience masks
// ---------------------------------------------------------------------------

/// File was moved (from or to).
pub const IN_MOVE: u32 = IN_MOVED_FROM | IN_MOVED_TO;
/// File was closed (write or nowrite).
pub const IN_CLOSE: u32 = IN_CLOSE_WRITE | IN_CLOSE_NOWRITE;
/// All events.
pub const IN_ALL_EVENTS: u32 = IN_ACCESS
    | IN_MODIFY
    | IN_ATTRIB
    | IN_CLOSE_WRITE
    | IN_CLOSE_NOWRITE
    | IN_OPEN
    | IN_MOVED_FROM
    | IN_MOVED_TO
    | IN_CREATE
    | IN_DELETE
    | IN_DELETE_SELF
    | IN_MOVE_SELF;

// ---------------------------------------------------------------------------
// Watch flags (ORed with event mask in inotify_add_watch)
// ---------------------------------------------------------------------------

/// Only report event once, then remove watch.
pub const IN_ONESHOT: u32 = 0x80000000;
/// Only watch path if it is a directory.
pub const IN_ONLYDIR: u32 = 0x01000000;
/// Don't follow symlinks.
pub const IN_DONT_FOLLOW: u32 = 0x02000000;
/// Exclude events for unlinked children.
pub const IN_EXCL_UNLINK: u32 = 0x04000000;
/// Add to existing watch mask (don't replace).
pub const IN_MASK_ADD: u32 = 0x20000000;
/// Create watch mask (additive to filesystem ops).
pub const IN_MASK_CREATE: u32 = 0x10000000;

// ---------------------------------------------------------------------------
// Kernel-generated event flags (in event->mask)
// ---------------------------------------------------------------------------

/// Subject is a directory.
pub const IN_ISDIR: u32 = 0x40000000;
/// Event queue overflowed.
pub const IN_Q_OVERFLOW: u32 = 0x00004000;
/// Watch was removed (explicit or file deleted).
pub const IN_IGNORED: u32 = 0x00008000;
/// Filesystem containing watched object was unmounted.
pub const IN_UNMOUNT: u32 = 0x00002000;

// ---------------------------------------------------------------------------
// inotify_init1 flags
// ---------------------------------------------------------------------------

/// Close-on-exec for inotify fd.
pub const IN_CLOEXEC: u32 = 0x80000;
/// Non-blocking for inotify fd.
pub const IN_NONBLOCK: u32 = 0x800;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_masks_powers_of_two() {
        let masks = [
            IN_ACCESS,
            IN_MODIFY,
            IN_ATTRIB,
            IN_CLOSE_WRITE,
            IN_CLOSE_NOWRITE,
            IN_OPEN,
            IN_MOVED_FROM,
            IN_MOVED_TO,
            IN_CREATE,
            IN_DELETE,
            IN_DELETE_SELF,
            IN_MOVE_SELF,
        ];
        for mask in &masks {
            assert!(mask.is_power_of_two(), "0x{:x}", mask);
        }
    }

    #[test]
    fn test_event_masks_no_overlap() {
        let masks = [
            IN_ACCESS,
            IN_MODIFY,
            IN_ATTRIB,
            IN_CLOSE_WRITE,
            IN_CLOSE_NOWRITE,
            IN_OPEN,
            IN_MOVED_FROM,
            IN_MOVED_TO,
            IN_CREATE,
            IN_DELETE,
            IN_DELETE_SELF,
            IN_MOVE_SELF,
        ];
        for i in 0..masks.len() {
            for j in (i + 1)..masks.len() {
                assert_eq!(masks[i] & masks[j], 0);
            }
        }
    }

    #[test]
    fn test_combined_move() {
        assert_eq!(IN_MOVE, IN_MOVED_FROM | IN_MOVED_TO);
    }

    #[test]
    fn test_combined_close() {
        assert_eq!(IN_CLOSE, IN_CLOSE_WRITE | IN_CLOSE_NOWRITE);
    }

    #[test]
    fn test_watch_flags_distinct() {
        let flags = [
            IN_ONESHOT,
            IN_ONLYDIR,
            IN_DONT_FOLLOW,
            IN_EXCL_UNLINK,
            IN_MASK_ADD,
            IN_MASK_CREATE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_kernel_flags_distinct() {
        let flags = [IN_ISDIR, IN_Q_OVERFLOW, IN_IGNORED, IN_UNMOUNT];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_init_flags_distinct() {
        assert_ne!(IN_CLOEXEC, IN_NONBLOCK);
    }
}
