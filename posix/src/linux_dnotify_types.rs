//! `<linux/dnotify.h>` — dnotify (directory notification) constants.
//!
//! dnotify is the oldest Linux file notification mechanism (added
//! in 2.4.0). It uses fcntl(fd, F_NOTIFY, mask) on a directory fd
//! to receive SIGIO when specified events occur in that directory.
//! dnotify only works on directories, requires holding an fd open,
//! and doesn't provide the filename of the affected file. It's been
//! superseded by inotify and fanotify but remains for compatibility.

// ---------------------------------------------------------------------------
// dnotify event flags (for F_NOTIFY fcntl)
// ---------------------------------------------------------------------------

/// File was accessed in the directory.
pub const DN_ACCESS: u32 = 0x0000_0001;
/// File was modified in the directory.
pub const DN_MODIFY: u32 = 0x0000_0002;
/// File was created in the directory.
pub const DN_CREATE: u32 = 0x0000_0004;
/// File was deleted from the directory.
pub const DN_DELETE: u32 = 0x0000_0008;
/// File was renamed in the directory.
pub const DN_RENAME: u32 = 0x0000_0010;
/// File attributes changed in the directory.
pub const DN_ATTRIB: u32 = 0x0000_0020;
/// Request multiple notifications (don't remove after first).
pub const DN_MULTISHOT: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// F_NOTIFY fcntl command
// ---------------------------------------------------------------------------

/// The fcntl command number for dnotify.
pub const F_NOTIFY_CMD: u32 = 1026;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_flags_no_overlap() {
        let flags = [
            DN_ACCESS, DN_MODIFY, DN_CREATE,
            DN_DELETE, DN_RENAME, DN_ATTRIB,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_multishot_separate() {
        // DN_MULTISHOT is in bit 31, doesn't overlap event bits
        assert!(DN_MULTISHOT.is_power_of_two());
        let events = DN_ACCESS | DN_MODIFY | DN_CREATE | DN_DELETE | DN_RENAME | DN_ATTRIB;
        assert_eq!(events & DN_MULTISHOT, 0);
    }
}
