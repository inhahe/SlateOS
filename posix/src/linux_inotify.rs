//! `<linux/inotify.h>` — inotify filesystem notification (kernel view).
//!
//! Re-exports from `sys_inotify` / `epoll`.

// ---------------------------------------------------------------------------
// Re-exports: functions
// ---------------------------------------------------------------------------

pub use crate::epoll::inotify_add_watch;
pub use crate::epoll::inotify_init;
pub use crate::epoll::inotify_init1;
pub use crate::epoll::inotify_rm_watch;

// ---------------------------------------------------------------------------
// Re-exports: event masks
// ---------------------------------------------------------------------------

pub use crate::epoll::IN_ACCESS;
pub use crate::epoll::IN_ALL_EVENTS;
pub use crate::epoll::IN_ATTRIB;
pub use crate::epoll::IN_CLOSE;
pub use crate::epoll::IN_CLOSE_NOWRITE;
pub use crate::epoll::IN_CLOSE_WRITE;
pub use crate::epoll::IN_CREATE;
pub use crate::epoll::IN_DELETE;
pub use crate::epoll::IN_DELETE_SELF;
pub use crate::epoll::IN_MODIFY;
pub use crate::epoll::IN_MOVE_SELF;
pub use crate::epoll::IN_MOVED_FROM;
pub use crate::epoll::IN_MOVED_TO;
pub use crate::epoll::IN_OPEN;

// ---------------------------------------------------------------------------
// Re-exports: init flags
// ---------------------------------------------------------------------------

pub use crate::epoll::IN_CLOEXEC;
pub use crate::epoll::IN_NONBLOCK;

// ---------------------------------------------------------------------------
// Re-exports: extended flags from sys_inotify
// ---------------------------------------------------------------------------

pub use crate::sys_inotify::IN_DONT_FOLLOW;
pub use crate::sys_inotify::IN_EXCL_UNLINK;
pub use crate::sys_inotify::IN_IGNORED;
pub use crate::sys_inotify::IN_ISDIR;
pub use crate::sys_inotify::IN_MASK_ADD;
pub use crate::sys_inotify::IN_MASK_CREATE;
pub use crate::sys_inotify::IN_MOVE;
pub use crate::sys_inotify::IN_ONESHOT;
pub use crate::sys_inotify::IN_Q_OVERFLOW;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_masks_distinct() {
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
                assert_ne!(masks[i], masks[j]);
            }
        }
    }

    #[test]
    fn test_convenience_masks() {
        assert_eq!(IN_CLOSE, IN_CLOSE_WRITE | IN_CLOSE_NOWRITE);
        assert_eq!(IN_MOVE, IN_MOVED_FROM | IN_MOVED_TO);
    }

    #[test]
    fn test_inotify_init_reexport() {
        // Functional now: returns an fd or EMFILE if the static
        // instance table is exhausted by concurrent tests.
        let fd = inotify_init();
        if fd >= 0 {
            crate::file::close(fd);
        }
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(IN_ACCESS, crate::epoll::IN_ACCESS);
        assert_eq!(IN_MODIFY, crate::epoll::IN_MODIFY);
        assert_eq!(IN_IGNORED, crate::sys_inotify::IN_IGNORED);
    }
}
