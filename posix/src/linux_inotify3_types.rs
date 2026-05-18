//! `<linux/inotify.h>` — Additional inotify constants (batch 3).
//!
//! Supplementary inotify constants covering watch descriptor flags,
//! event size limits, and queue configuration.

// ---------------------------------------------------------------------------
// Inotify init flags (IN_*)
// ---------------------------------------------------------------------------

/// Set close-on-exec.
pub const IN_CLOEXEC: u32 = 0o2000000;
/// Set non-blocking.
pub const IN_NONBLOCK: u32 = 0o4000;

// ---------------------------------------------------------------------------
// Inotify watch flags (additional)
// ---------------------------------------------------------------------------

/// Watch for metadata changes.
pub const IN_ATTRIB: u32 = 0x00000004;
/// Watch for close after write.
pub const IN_CLOSE_WRITE: u32 = 0x00000008;
/// Watch for close without write.
pub const IN_CLOSE_NOWRITE: u32 = 0x00000010;
/// Close: write or no-write.
pub const IN_CLOSE: u32 = 0x00000008 | 0x00000010;
/// Watch for file open.
pub const IN_OPEN: u32 = 0x00000020;
/// Watch for moves from.
pub const IN_MOVED_FROM: u32 = 0x00000040;
/// Watch for moves to.
pub const IN_MOVED_TO: u32 = 0x00000080;
/// Move: from or to.
pub const IN_MOVE: u32 = 0x00000040 | 0x00000080;
/// Watch for file creation.
pub const IN_CREATE: u32 = 0x00000100;
/// Watch for file deletion.
pub const IN_DELETE: u32 = 0x00000200;
/// Watch for self deletion.
pub const IN_DELETE_SELF: u32 = 0x00000400;
/// Watch for self move.
pub const IN_MOVE_SELF: u32 = 0x00000800;

// ---------------------------------------------------------------------------
// Inotify special flags
// ---------------------------------------------------------------------------

/// Only watch path if it's a directory.
pub const IN_ONLYDIR: u32 = 0x01000000;
/// Don't follow symlinks.
pub const IN_DONT_FOLLOW: u32 = 0x02000000;
/// Exclude unlinked events.
pub const IN_EXCL_UNLINK: u32 = 0x04000000;
/// Add to existing watch mask.
pub const IN_MASK_ADD: u32 = 0x20000000;
/// One-shot watch (auto-remove after event).
pub const IN_ONESHOT: u32 = 0x80000000;
/// Create mask for user events.
pub const IN_MASK_CREATE: u32 = 0x10000000;

// ---------------------------------------------------------------------------
// Inotify system limits
// ---------------------------------------------------------------------------

/// Default max watches per user.
pub const INOTIFY_MAX_USER_WATCHES: u32 = 8192;
/// Default max instances per user.
pub const INOTIFY_MAX_USER_INSTANCES: u32 = 128;
/// Default max queued events.
pub const INOTIFY_MAX_QUEUED_EVENTS: u32 = 16384;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_flags_distinct() {
        assert_ne!(IN_CLOEXEC, IN_NONBLOCK);
    }

    #[test]
    fn test_watch_flags_distinct() {
        let flags = [
            IN_ATTRIB, IN_CLOSE_WRITE, IN_CLOSE_NOWRITE,
            IN_OPEN, IN_MOVED_FROM, IN_MOVED_TO,
            IN_CREATE, IN_DELETE, IN_DELETE_SELF, IN_MOVE_SELF,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
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
    fn test_special_flags_distinct() {
        let flags = [
            IN_ONLYDIR, IN_DONT_FOLLOW, IN_EXCL_UNLINK,
            IN_MASK_ADD, IN_ONESHOT, IN_MASK_CREATE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_system_limits() {
        assert!(INOTIFY_MAX_USER_WATCHES > 0);
        assert!(INOTIFY_MAX_USER_INSTANCES > 0);
        assert!(INOTIFY_MAX_QUEUED_EVENTS > 0);
    }

    #[test]
    fn test_watch_flags_are_powers_of_two() {
        let flags = [
            IN_ATTRIB, IN_CLOSE_WRITE, IN_CLOSE_NOWRITE,
            IN_OPEN, IN_MOVED_FROM, IN_MOVED_TO,
            IN_CREATE, IN_DELETE, IN_DELETE_SELF, IN_MOVE_SELF,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }
}
