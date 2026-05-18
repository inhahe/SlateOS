//! `<sys/inotify.h>` — inotify initialization flag constants.
//!
//! These flags are passed to `inotify_init1()` to set properties
//! on the inotify file descriptor. They control close-on-exec and
//! non-blocking behavior of the returned fd.

// ---------------------------------------------------------------------------
// inotify_init1() flags
// ---------------------------------------------------------------------------

/// Set close-on-exec on the inotify fd.
pub const IN_CLOEXEC: u32 = 0o2000000;
/// Set non-blocking on the inotify fd.
pub const IN_NONBLOCK: u32 = 0o4000;

// ---------------------------------------------------------------------------
// inotify watch event mask flags (inotify_add_watch)
// ---------------------------------------------------------------------------

/// File was accessed (read).
pub const IN_ACCESS: u32 = 0x0000_0001;
/// File was modified (write).
pub const IN_MODIFY: u32 = 0x0000_0002;
/// File metadata changed (chmod, chown, etc.).
pub const IN_ATTRIB: u32 = 0x0000_0004;
/// File opened for writing was closed.
pub const IN_CLOSE_WRITE: u32 = 0x0000_0008;
/// File not opened for writing was closed.
pub const IN_CLOSE_NOWRITE: u32 = 0x0000_0010;
/// File was opened.
pub const IN_OPEN: u32 = 0x0000_0020;
/// File moved from watched directory.
pub const IN_MOVED_FROM: u32 = 0x0000_0040;
/// File moved to watched directory.
pub const IN_MOVED_TO: u32 = 0x0000_0080;
/// File/dir created in watched directory.
pub const IN_CREATE: u32 = 0x0000_0100;
/// File/dir deleted from watched directory.
pub const IN_DELETE: u32 = 0x0000_0200;
/// Watched file/dir was deleted.
pub const IN_DELETE_SELF: u32 = 0x0000_0400;
/// Watched file/dir was moved.
pub const IN_MOVE_SELF: u32 = 0x0000_0800;

// ---------------------------------------------------------------------------
// Watch modifier flags
// ---------------------------------------------------------------------------

/// Only watch path if it's a directory.
pub const IN_ONLYDIR: u32 = 0x0100_0000;
/// Don't follow symbolic links.
pub const IN_DONT_FOLLOW: u32 = 0x0200_0000;
/// Don't watch children's events.
pub const IN_EXCL_UNLINK: u32 = 0x0400_0000;
/// Add events to existing watch mask.
pub const IN_MASK_ADD: u32 = 0x2000_0000;
/// One-shot watch (auto-remove after first event).
pub const IN_ONESHOT: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// Convenience combinations
// ---------------------------------------------------------------------------

/// All close events (CLOSE_WRITE | CLOSE_NOWRITE).
pub const IN_CLOSE: u32 = IN_CLOSE_WRITE | IN_CLOSE_NOWRITE;
/// All move events (MOVED_FROM | MOVED_TO).
pub const IN_MOVE: u32 = IN_MOVED_FROM | IN_MOVED_TO;
/// All events.
pub const IN_ALL_EVENTS: u32 = IN_ACCESS | IN_MODIFY | IN_ATTRIB
    | IN_CLOSE_WRITE | IN_CLOSE_NOWRITE | IN_OPEN
    | IN_MOVED_FROM | IN_MOVED_TO | IN_CREATE
    | IN_DELETE | IN_DELETE_SELF | IN_MOVE_SELF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_flags_power_of_two() {
        let flags = [
            IN_ACCESS, IN_MODIFY, IN_ATTRIB, IN_CLOSE_WRITE,
            IN_CLOSE_NOWRITE, IN_OPEN, IN_MOVED_FROM, IN_MOVED_TO,
            IN_CREATE, IN_DELETE, IN_DELETE_SELF, IN_MOVE_SELF,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_event_flags_no_overlap() {
        let flags = [
            IN_ACCESS, IN_MODIFY, IN_ATTRIB, IN_CLOSE_WRITE,
            IN_CLOSE_NOWRITE, IN_OPEN, IN_MOVED_FROM, IN_MOVED_TO,
            IN_CREATE, IN_DELETE, IN_DELETE_SELF, IN_MOVE_SELF,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_close_combination() {
        assert_eq!(IN_CLOSE, IN_CLOSE_WRITE | IN_CLOSE_NOWRITE);
    }

    #[test]
    fn test_move_combination() {
        assert_eq!(IN_MOVE, IN_MOVED_FROM | IN_MOVED_TO);
    }

    #[test]
    fn test_init_flags_distinct() {
        assert_ne!(IN_CLOEXEC, IN_NONBLOCK);
    }

    #[test]
    fn test_oneshot_is_high_bit() {
        assert_eq!(IN_ONESHOT, 0x8000_0000);
    }
}
