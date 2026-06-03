//! `<linux/fanotify.h>` — fanotify advanced features constants.
//!
//! This module covers fanotify features beyond basic file event
//! monitoring: permission events (allow/deny file access), directory
//! modification events (FAN_CREATE, FAN_DELETE, etc. added in 5.1+),
//! filesystem-level monitoring, event info records (file handles,
//! PIDs), and FID-based identification (using file handles instead
//! of file descriptors for identifying affected files).

// ---------------------------------------------------------------------------
// fanotify permission event responses
// ---------------------------------------------------------------------------

/// Allow the file access.
pub const FAN_ALLOW: u32 = 0x01;
/// Deny the file access.
pub const FAN_DENY: u32 = 0x02;
/// Audit the access (log it).
pub const FAN_AUDIT: u32 = 0x10;

// ---------------------------------------------------------------------------
// fanotify init flags (fanotify_init)
// ---------------------------------------------------------------------------

/// Report events with FID (file handle, not fd).
pub const FAN_REPORT_FID: u32 = 0x0000_0200;
/// Report directory FID for directory events.
pub const FAN_REPORT_DIR_FID: u32 = 0x0000_0400;
/// Report name (filename) for directory events.
pub const FAN_REPORT_NAME: u32 = 0x0000_0800;
/// Report target FID (for renames, the new location).
pub const FAN_REPORT_TARGET_FID: u32 = 0x0000_1000;
/// Report pidfd (instead of pid_t).
pub const FAN_REPORT_PIDFD: u32 = 0x0000_0080;

// ---------------------------------------------------------------------------
// fanotify event info record types
// ---------------------------------------------------------------------------

/// Info record contains a file handle (FID).
pub const FAN_EVENT_INFO_TYPE_FID: u32 = 1;
/// Info record contains a directory file handle + filename.
pub const FAN_EVENT_INFO_TYPE_DFID_NAME: u32 = 2;
/// Info record contains a directory file handle.
pub const FAN_EVENT_INFO_TYPE_DFID: u32 = 3;
/// Info record contains a pidfd.
pub const FAN_EVENT_INFO_TYPE_PIDFD: u32 = 4;
/// Info record contains error information.
pub const FAN_EVENT_INFO_TYPE_ERROR: u32 = 5;
/// Info record contains old+new parent for rename.
pub const FAN_EVENT_INFO_TYPE_OLD_DFID_NAME: u32 = 6;
/// Info record contains new parent for rename.
pub const FAN_EVENT_INFO_TYPE_NEW_DFID_NAME: u32 = 7;

// ---------------------------------------------------------------------------
// fanotify directory event masks (5.1+)
// ---------------------------------------------------------------------------

/// File was created in watched dir.
pub const FAN_DIR_CREATE: u32 = 0x0000_0100;
/// File was deleted from watched dir.
pub const FAN_DIR_DELETE: u32 = 0x0000_0200;
/// File was moved from watched dir.
pub const FAN_DIR_MOVED_FROM: u32 = 0x0000_0040;
/// File was moved to watched dir.
pub const FAN_DIR_MOVED_TO: u32 = 0x0000_0080;
/// File was renamed within/between watched dirs.
pub const FAN_DIR_RENAME: u32 = 0x1000_0000;

// ---------------------------------------------------------------------------
// fanotify filesystem event flags
// ---------------------------------------------------------------------------

/// Event occurred on filesystem (not specific file/dir).
pub const FAN_FS_ERROR: u32 = 0x0000_8000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_responses_distinct() {
        let resps = [FAN_ALLOW, FAN_DENY, FAN_AUDIT];
        for i in 0..resps.len() {
            for j in (i + 1)..resps.len() {
                assert_ne!(resps[i], resps[j]);
            }
        }
    }

    #[test]
    fn test_init_flags_no_overlap() {
        let flags = [
            FAN_REPORT_FID,
            FAN_REPORT_DIR_FID,
            FAN_REPORT_NAME,
            FAN_REPORT_TARGET_FID,
            FAN_REPORT_PIDFD,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_info_types_distinct() {
        let types = [
            FAN_EVENT_INFO_TYPE_FID,
            FAN_EVENT_INFO_TYPE_DFID_NAME,
            FAN_EVENT_INFO_TYPE_DFID,
            FAN_EVENT_INFO_TYPE_PIDFD,
            FAN_EVENT_INFO_TYPE_ERROR,
            FAN_EVENT_INFO_TYPE_OLD_DFID_NAME,
            FAN_EVENT_INFO_TYPE_NEW_DFID_NAME,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
