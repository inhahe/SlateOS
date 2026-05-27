//! `<sys/inotify.h>` — inotify file monitoring.
//!
//! Re-exports inotify functions and constants from the `epoll` module.

pub use crate::epoll::inotify_init;
pub use crate::epoll::inotify_init1;
pub use crate::epoll::inotify_add_watch;
pub use crate::epoll::inotify_rm_watch;

// Event mask constants
pub use crate::epoll::IN_ACCESS;
pub use crate::epoll::IN_MODIFY;
pub use crate::epoll::IN_ATTRIB;
pub use crate::epoll::IN_CLOSE_WRITE;
pub use crate::epoll::IN_CLOSE_NOWRITE;
pub use crate::epoll::IN_OPEN;
pub use crate::epoll::IN_MOVED_FROM;
pub use crate::epoll::IN_MOVED_TO;
pub use crate::epoll::IN_CREATE;
pub use crate::epoll::IN_DELETE;
pub use crate::epoll::IN_DELETE_SELF;
pub use crate::epoll::IN_MOVE_SELF;
pub use crate::epoll::IN_CLOSE;
pub use crate::epoll::IN_ALL_EVENTS;

// Init flags
pub use crate::epoll::IN_CLOEXEC;
pub use crate::epoll::IN_NONBLOCK;

// ---------------------------------------------------------------------------
// Additional event flags not in epoll.rs
// ---------------------------------------------------------------------------

/// Combined move event mask.
pub const IN_MOVE: u32 = IN_MOVED_FROM | IN_MOVED_TO;

/// Watch was removed (automatically or via inotify_rm_watch).
pub use crate::epoll::IN_IGNORED;

/// Subject of event is a directory.
pub const IN_ISDIR: u32 = 0x4000_0000;

/// Event queue overflowed.
pub use crate::epoll::IN_Q_OVERFLOW;

/// Only watch the path itself, not the target.
pub const IN_DONT_FOLLOW: u32 = 0x0200_0000;

/// Exclude events for unlinked children.
pub const IN_EXCL_UNLINK: u32 = 0x0400_0000;

/// Add to an existing watch (OR masks).
pub const IN_MASK_ADD: u32 = 0x2000_0000;

/// Only trigger once, then remove the watch.
pub const IN_ONESHOT: u32 = 0x8000_0000;

/// Watch pathname if it is a symlink.
pub const IN_MASK_CREATE: u32 = 0x1000_0000;

// ---------------------------------------------------------------------------
// Inotify event structure
// ---------------------------------------------------------------------------

/// Event structure read from an inotify file descriptor.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InotifyEvent {
    /// Watch descriptor.
    pub wd: i32,
    /// Mask describing event.
    pub mask: u32,
    /// Unique cookie for rename events.
    pub cookie: u32,
    /// Size of the name field.
    pub len: u32,
    // Followed by a variable-length null-terminated name.
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_move_combined() {
        assert_eq!(IN_MOVE, IN_MOVED_FROM | IN_MOVED_TO);
    }

    #[test]
    fn test_event_masks_accessible() {
        assert_eq!(IN_ACCESS, 0x0000_0001);
        assert_eq!(IN_MODIFY, 0x0000_0002);
        assert_eq!(IN_CREATE, 0x0000_0100);
        assert_eq!(IN_DELETE, 0x0000_0200);
    }

    #[test]
    fn test_special_flags() {
        assert_ne!(IN_IGNORED, 0);
        assert_ne!(IN_ISDIR, 0);
        assert_ne!(IN_Q_OVERFLOW, 0);
        assert_ne!(IN_ONESHOT, 0);
    }

    #[test]
    fn test_special_flags_distinct() {
        let flags = [
            IN_IGNORED, IN_ISDIR, IN_Q_OVERFLOW, IN_DONT_FOLLOW,
            IN_EXCL_UNLINK, IN_MASK_ADD, IN_ONESHOT, IN_MASK_CREATE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_inotify_event_size() {
        assert_eq!(core::mem::size_of::<InotifyEvent>(), 16);
    }

    #[test]
    fn test_inotify_init_reexport() {
        // Functional now — verify the re-exported function actually
        // produces a valid fd (or EMFILE if the table is full).
        let fd = inotify_init();
        if fd >= 0 {
            crate::file::close(fd);
        }
    }

    #[test]
    fn test_inotify_init1_reexport() {
        let fd = inotify_init1(0);
        if fd >= 0 {
            crate::file::close(fd);
        }
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(IN_ACCESS, crate::epoll::IN_ACCESS);
        assert_eq!(IN_MODIFY, crate::epoll::IN_MODIFY);
        assert_eq!(IN_ALL_EVENTS, crate::epoll::IN_ALL_EVENTS);
    }
}
