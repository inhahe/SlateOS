//! `<linux/inotify.h>` — inotify(7) userspace constants.
//!
//! inotify is the foundational change-notification API. systemd
//! path units, Code/IntelliJ file watchers, ufw, syncthing, and
//! every desktop-environment file manager rely on it. We expose
//! it under the `fsnotify` family name because inotify is the
//! kernel's main `fsnotify` consumer.

// ---------------------------------------------------------------------------
// inotify_init1() flags
// ---------------------------------------------------------------------------

/// `IN_CLOEXEC`.
pub const IN_CLOEXEC: u32 = 0o2_000_000;
/// `IN_NONBLOCK`.
pub const IN_NONBLOCK: u32 = 0o0_004_000;

// ---------------------------------------------------------------------------
// Event mask bits (struct inotify_event.mask)
// ---------------------------------------------------------------------------

/// File was accessed.
pub const IN_ACCESS: u32 = 0x0000_0001;
/// File was modified.
pub const IN_MODIFY: u32 = 0x0000_0002;
/// Metadata changed.
pub const IN_ATTRIB: u32 = 0x0000_0004;
/// Writable fd closed.
pub const IN_CLOSE_WRITE: u32 = 0x0000_0008;
/// Non-writable fd closed.
pub const IN_CLOSE_NOWRITE: u32 = 0x0000_0010;
/// File was opened.
pub const IN_OPEN: u32 = 0x0000_0020;
/// File moved from X.
pub const IN_MOVED_FROM: u32 = 0x0000_0040;
/// File moved to Y.
pub const IN_MOVED_TO: u32 = 0x0000_0080;
/// File/dir was created in watched directory.
pub const IN_CREATE: u32 = 0x0000_0100;
/// File/dir was deleted from watched directory.
pub const IN_DELETE: u32 = 0x0000_0200;
/// Watched file/dir was itself deleted.
pub const IN_DELETE_SELF: u32 = 0x0000_0400;
/// Watched file/dir was itself moved.
pub const IN_MOVE_SELF: u32 = 0x0000_0800;

// ---------------------------------------------------------------------------
// Aggregated bits
// ---------------------------------------------------------------------------

/// Convenience: any close.
pub const IN_CLOSE: u32 = IN_CLOSE_WRITE | IN_CLOSE_NOWRITE;
/// Convenience: any move.
pub const IN_MOVE: u32 = IN_MOVED_FROM | IN_MOVED_TO;
/// Convenience: every event.
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
// Watch-modifier flags (passed to inotify_add_watch)
// ---------------------------------------------------------------------------

/// Only watch path if it is a dir.
pub const IN_ONLYDIR: u32 = 0x0100_0000;
/// Don't follow symlinks.
pub const IN_DONT_FOLLOW: u32 = 0x0200_0000;
/// Don't auto-remove watch on unlink.
pub const IN_EXCL_UNLINK: u32 = 0x0400_0000;
/// Combine with existing mask rather than replace.
pub const IN_MASK_ADD: u32 = 0x2000_0000;
/// Only deliver this event once.
pub const IN_ONESHOT: u32 = 0x8000_0000;
/// Watch failed because of a mask filter.
pub const IN_MASK_CREATE: u32 = 0x1000_0000;

// ---------------------------------------------------------------------------
// Status bits reported in events
// ---------------------------------------------------------------------------

/// Event is for a directory.
pub const IN_ISDIR: u32 = 0x4000_0000;
/// Watched filesystem was unmounted.
pub const IN_UNMOUNT: u32 = 0x0000_2000;
/// Event queue overflowed.
pub const IN_Q_OVERFLOW: u32 = 0x0000_4000;
/// Watch was removed (by kernel or via rm_watch).
pub const IN_IGNORED: u32 = 0x0000_8000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_flags() {
        assert_eq!(IN_CLOEXEC, 0o2_000_000);
        assert_eq!(IN_NONBLOCK, 0o4_000);
    }

    #[test]
    fn test_event_bits_pow2_distinct() {
        let e = [
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
        for &b in &e {
            assert!(b.is_power_of_two());
        }
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
        }
    }

    #[test]
    fn test_aggregates() {
        assert_eq!(IN_CLOSE, 0x18);
        assert_eq!(IN_MOVE, 0xC0);
        // IN_ALL_EVENTS must equal the OR of all 12 individual bits.
        let or = IN_ACCESS
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
        assert_eq!(IN_ALL_EVENTS, or);
        assert_eq!(IN_ALL_EVENTS.count_ones(), 12);
    }

    #[test]
    fn test_modifier_flags_distinct() {
        let m = [
            IN_ONLYDIR,
            IN_DONT_FOLLOW,
            IN_EXCL_UNLINK,
            IN_MASK_ADD,
            IN_ONESHOT,
            IN_MASK_CREATE,
        ];
        for &b in &m {
            assert!(b.is_power_of_two());
        }
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
    }

    #[test]
    fn test_status_bits_distinct() {
        let s = [IN_ISDIR, IN_UNMOUNT, IN_Q_OVERFLOW, IN_IGNORED];
        for &b in &s {
            assert!(b.is_power_of_two());
        }
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
    }
}
