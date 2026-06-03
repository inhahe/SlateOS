//! `<linux/fanotify.h>` — fanotify filesystem notification constants.
//!
//! fanotify is a filesystem notification mechanism (successor to
//! inotify for certain use cases) that provides system-wide file
//! access notifications. It supports permission events (allowing
//! the listener to approve or deny access), making it suitable for
//! anti-malware, hierarchical storage managers, and audit systems.

// ---------------------------------------------------------------------------
// Event mask flags
// ---------------------------------------------------------------------------

/// File was accessed (read).
pub const FAN_ACCESS: u64 = 0x0000_0001;
/// File was modified.
pub const FAN_MODIFY: u64 = 0x0000_0002;
/// Metadata changed (attrib).
pub const FAN_ATTRIB: u64 = 0x0000_0004;
/// Writable file was closed.
pub const FAN_CLOSE_WRITE: u64 = 0x0000_0008;
/// Read-only file was closed.
pub const FAN_CLOSE_NOWRITE: u64 = 0x0000_0010;
/// File was opened.
pub const FAN_OPEN: u64 = 0x0000_0020;
/// File was moved from.
pub const FAN_MOVED_FROM: u64 = 0x0000_0040;
/// File was moved to.
pub const FAN_MOVED_TO: u64 = 0x0000_0080;
/// File was created.
pub const FAN_CREATE: u64 = 0x0000_0100;
/// File was deleted.
pub const FAN_DELETE: u64 = 0x0000_0200;
/// Self was deleted.
pub const FAN_DELETE_SELF: u64 = 0x0000_0400;
/// Self was moved.
pub const FAN_MOVE_SELF: u64 = 0x0000_0800;
/// File was opened for execution.
pub const FAN_OPEN_EXEC: u64 = 0x0000_1000;

// ---------------------------------------------------------------------------
// Permission events (require response)
// ---------------------------------------------------------------------------

/// Permission to open.
pub const FAN_OPEN_PERM: u64 = 0x0001_0000;
/// Permission to access (read).
pub const FAN_ACCESS_PERM: u64 = 0x0002_0000;
/// Permission to open for exec.
pub const FAN_OPEN_EXEC_PERM: u64 = 0x0004_0000;

// ---------------------------------------------------------------------------
// Response values
// ---------------------------------------------------------------------------

/// Allow the file access.
pub const FAN_ALLOW: u32 = 0x01;
/// Deny the file access.
pub const FAN_DENY: u32 = 0x02;
/// Audit the access decision.
pub const FAN_AUDIT: u32 = 0x10;

// ---------------------------------------------------------------------------
// Init flags (fanotify_init)
// ---------------------------------------------------------------------------

/// Class: content (higher priority).
pub const FAN_CLASS_CONTENT: u32 = 0x04;
/// Class: pre-content (highest priority).
pub const FAN_CLASS_PRE_CONTENT: u32 = 0x08;
/// Class: notification only (default).
pub const FAN_CLASS_NOTIF: u32 = 0x00;
/// Report thread ID.
pub const FAN_REPORT_TID: u32 = 0x100;
/// Report FID (file identifier).
pub const FAN_REPORT_FID: u32 = 0x200;
/// Report directory FID.
pub const FAN_REPORT_DIR_FID: u32 = 0x400;
/// Report name.
pub const FAN_REPORT_NAME: u32 = 0x800;
/// Report target FID.
pub const FAN_REPORT_TARGET_FID: u32 = 0x1000;

// ---------------------------------------------------------------------------
// Mark flags (fanotify_mark)
// ---------------------------------------------------------------------------

/// Add to mark mask.
pub const FAN_MARK_ADD: u32 = 0x01;
/// Remove from mark mask.
pub const FAN_MARK_REMOVE: u32 = 0x02;
/// Flush all marks.
pub const FAN_MARK_FLUSH: u32 = 0x80;
/// Mark on inode.
pub const FAN_MARK_INODE: u32 = 0x00;
/// Mark on mount.
pub const FAN_MARK_MOUNT: u32 = 0x10;
/// Mark on filesystem.
pub const FAN_MARK_FILESYSTEM: u32 = 0x100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_flags_no_overlap() {
        let events = [
            FAN_ACCESS,
            FAN_MODIFY,
            FAN_ATTRIB,
            FAN_CLOSE_WRITE,
            FAN_CLOSE_NOWRITE,
            FAN_OPEN,
            FAN_MOVED_FROM,
            FAN_MOVED_TO,
            FAN_CREATE,
            FAN_DELETE,
            FAN_DELETE_SELF,
            FAN_MOVE_SELF,
            FAN_OPEN_EXEC,
        ];
        for i in 0..events.len() {
            assert!(events[i].is_power_of_two());
            for j in (i + 1)..events.len() {
                assert_eq!(events[i] & events[j], 0);
            }
        }
    }

    #[test]
    fn test_perm_events_no_overlap() {
        let perms = [FAN_OPEN_PERM, FAN_ACCESS_PERM, FAN_OPEN_EXEC_PERM];
        for i in 0..perms.len() {
            assert!(perms[i].is_power_of_two());
            for j in (i + 1)..perms.len() {
                assert_eq!(perms[i] & perms[j], 0);
            }
        }
    }

    #[test]
    fn test_response_values_distinct() {
        let vals = [FAN_ALLOW, FAN_DENY, FAN_AUDIT];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_report_flags_no_overlap() {
        let flags = [
            FAN_REPORT_TID,
            FAN_REPORT_FID,
            FAN_REPORT_DIR_FID,
            FAN_REPORT_NAME,
            FAN_REPORT_TARGET_FID,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_mark_add_remove_distinct() {
        assert_ne!(FAN_MARK_ADD, FAN_MARK_REMOVE);
        assert_ne!(FAN_MARK_ADD, FAN_MARK_FLUSH);
    }
}
