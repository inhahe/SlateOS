//! `<linux/fanotify.h>` — Additional fanotify constants (batch 3).
//!
//! Supplementary fanotify constants covering response flags,
//! file handle types, and info record types.

// ---------------------------------------------------------------------------
// Fanotify response decisions (FAN_*)
// ---------------------------------------------------------------------------

/// Allow access.
pub const FAN_ALLOW: u32 = 0x01;
/// Deny access.
pub const FAN_DENY: u32 = 0x02;
/// Audit response.
pub const FAN_AUDIT: u32 = 0x10;
/// Info response.
pub const FAN_INFO: u32 = 0x20;

// ---------------------------------------------------------------------------
// Fanotify info record types (FAN_EVENT_INFO_TYPE_*)
// ---------------------------------------------------------------------------

/// File handle info.
pub const FAN_EVENT_INFO_TYPE_FID: u32 = 1;
/// Directory file handle info.
pub const FAN_EVENT_INFO_TYPE_DFID: u32 = 2;
/// Directory file handle + name info.
pub const FAN_EVENT_INFO_TYPE_DFID_NAME: u32 = 3;
/// Pidfd info.
pub const FAN_EVENT_INFO_TYPE_PIDFD: u32 = 4;
/// Error info.
pub const FAN_EVENT_INFO_TYPE_ERROR: u32 = 5;
/// Old and new parent+name (rename).
pub const FAN_EVENT_INFO_TYPE_OLD_DFID_NAME: u32 = 10;
/// New parent+name (rename).
pub const FAN_EVENT_INFO_TYPE_NEW_DFID_NAME: u32 = 12;

// ---------------------------------------------------------------------------
// Fanotify class flags
// ---------------------------------------------------------------------------

/// Notification class: pre-content.
pub const FAN_CLASS_PRE_CONTENT: u32 = 0x00000008;
/// Notification class: content.
pub const FAN_CLASS_CONTENT: u32 = 0x00000004;
/// Notification class: notif (default).
pub const FAN_CLASS_NOTIF: u32 = 0x00000000;

// ---------------------------------------------------------------------------
// Fanotify mark type flags
// ---------------------------------------------------------------------------

/// Mark: add mark.
pub const FAN_MARK_ADD: u32 = 0x00000001;
/// Mark: remove mark.
pub const FAN_MARK_REMOVE: u32 = 0x00000002;
/// Mark: don't follow symlinks.
pub const FAN_MARK_DONT_FOLLOW: u32 = 0x00000004;
/// Mark: only dir.
pub const FAN_MARK_ONLYDIR: u32 = 0x00000008;
/// Mark: ignored mask.
pub const FAN_MARK_IGNORED_MASK: u32 = 0x00000020;
/// Mark: survived overflow.
pub const FAN_MARK_IGNORED_SURV_MODIFY: u32 = 0x00000040;
/// Mark: flush.
pub const FAN_MARK_FLUSH: u32 = 0x00000080;
/// Mark: evictable.
pub const FAN_MARK_EVICTABLE: u32 = 0x00000200;
/// Mark: ignore surv modify.
pub const FAN_MARK_IGNORE: u32 = 0x00000400;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_values() {
        assert_eq!(FAN_ALLOW, 0x01);
        assert_eq!(FAN_DENY, 0x02);
        assert_eq!(FAN_AUDIT, 0x10);
    }

    #[test]
    fn test_info_record_types_distinct() {
        let types = [
            FAN_EVENT_INFO_TYPE_FID, FAN_EVENT_INFO_TYPE_DFID,
            FAN_EVENT_INFO_TYPE_DFID_NAME, FAN_EVENT_INFO_TYPE_PIDFD,
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

    #[test]
    fn test_class_flags_distinct() {
        let classes = [
            FAN_CLASS_PRE_CONTENT, FAN_CLASS_CONTENT,
            FAN_CLASS_NOTIF,
        ];
        // NOTIF is 0, so compare all pairwise
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_mark_flags_distinct() {
        let flags = [
            FAN_MARK_ADD, FAN_MARK_REMOVE, FAN_MARK_DONT_FOLLOW,
            FAN_MARK_ONLYDIR, FAN_MARK_IGNORED_MASK,
            FAN_MARK_IGNORED_SURV_MODIFY, FAN_MARK_FLUSH,
            FAN_MARK_EVICTABLE, FAN_MARK_IGNORE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_mark_add_remove_no_overlap() {
        assert_eq!(FAN_MARK_ADD & FAN_MARK_REMOVE, 0);
    }
}
