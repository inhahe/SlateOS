//! `<linux/watch_queue.h>` — Watch queue notification constants.
//!
//! Watch queues (Linux 5.8+) allow userspace to receive kernel event
//! notifications through a pipe. The kernel posts structured
//! notifications (key/keyring changes, mount changes, superblock
//! events) into a special pipe buffer that userspace reads with
//! standard read(2). Replaces ad-hoc notification mechanisms with a
//! unified, filterable, typesafe event system.

// ---------------------------------------------------------------------------
// Watch notification types
// ---------------------------------------------------------------------------

/// Meta/control notification (watch queue management).
pub const WATCH_TYPE_META: u32 = 0;
/// Key/keyring change notification.
pub const WATCH_TYPE_KEY_NOTIFY: u32 = 1;
/// Mount topology change notification.
pub const WATCH_TYPE_MOUNT_NOTIFY: u32 = 2;
/// Superblock change notification.
pub const WATCH_TYPE_SB_NOTIFY: u32 = 3;

// ---------------------------------------------------------------------------
// Watch meta subtypes
// ---------------------------------------------------------------------------

/// Removal notification (watch was removed).
pub const WATCH_META_REMOVAL_NOTIFICATION: u32 = 0;
/// Loss notification (events were lost due to buffer overflow).
pub const WATCH_META_LOSS_NOTIFICATION: u32 = 1;

// ---------------------------------------------------------------------------
// Key/keyring notification subtypes
// ---------------------------------------------------------------------------

/// Key was updated.
pub const NOTIFY_KEY_UPDATED: u32 = 0;
/// Key was linked into a keyring.
pub const NOTIFY_KEY_LINKED: u32 = 1;
/// Key was unlinked from a keyring.
pub const NOTIFY_KEY_UNLINKED: u32 = 2;
/// Key was cleared.
pub const NOTIFY_KEY_CLEARED: u32 = 3;
/// Key was revoked.
pub const NOTIFY_KEY_REVOKED: u32 = 4;
/// Key was invalidated.
pub const NOTIFY_KEY_INVALIDATED: u32 = 5;
/// Key was set (attributes changed).
pub const NOTIFY_KEY_SETATTR: u32 = 6;

// ---------------------------------------------------------------------------
// Mount notification subtypes
// ---------------------------------------------------------------------------

/// New mount added.
pub const NOTIFY_MOUNT_NEW_MOUNT: u32 = 0;
/// Mount was unmounted.
pub const NOTIFY_MOUNT_UNMOUNT: u32 = 1;
/// Mount point expired.
pub const NOTIFY_MOUNT_EXPIRY: u32 = 2;
/// Mount was moved.
pub const NOTIFY_MOUNT_MOVE: u32 = 3;
/// Mount attributes changed (readonly etc.).
pub const NOTIFY_MOUNT_SETATTR: u32 = 4;

// ---------------------------------------------------------------------------
// Superblock notification subtypes
// ---------------------------------------------------------------------------

/// Filesystem error.
pub const NOTIFY_SB_ERROR: u32 = 0;
/// Filesystem is now read-only.
pub const NOTIFY_SB_READONLY: u32 = 1;
/// Filesystem quota exceeded.
pub const NOTIFY_SB_QUOTA: u32 = 2;
/// Network filesystem lost server connection.
pub const NOTIFY_SB_NETWORK: u32 = 3;

// ---------------------------------------------------------------------------
// Watch queue IOCTLs
// ---------------------------------------------------------------------------

/// Set filter on watch queue pipe.
pub const IOC_WATCH_QUEUE_SET_FILTER: u32 = 0x5760_0001;
/// Set size of watch queue pipe buffer.
pub const IOC_WATCH_QUEUE_SET_SIZE: u32 = 0x5760_0002;

// ---------------------------------------------------------------------------
// Watch notification header flags
// ---------------------------------------------------------------------------

/// Notification header info field: type shift.
pub const WATCH_INFO_TYPE_SHIFT: u32 = 0;
/// Notification header info field: type mask (8 bits).
pub const WATCH_INFO_TYPE_MASK: u32 = 0xFF;
/// Notification header info field: subtype shift.
pub const WATCH_INFO_SUBTYPE_SHIFT: u32 = 8;
/// Notification header info field: subtype mask (8 bits, shifted).
pub const WATCH_INFO_SUBTYPE_MASK: u32 = 0xFF00;
/// Notification header info field: length shift.
pub const WATCH_INFO_LENGTH_SHIFT: u32 = 16;
/// Notification header info field: length mask.
pub const WATCH_INFO_LENGTH_MASK: u32 = 0x3F_0000;
/// Notification header info field: overflow flag.
pub const WATCH_INFO_OVERRUN: u32 = 0x0040_0000;
/// Notification header info field: ID present flag.
pub const WATCH_INFO_ID: u32 = 0xFF00_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watch_types_distinct() {
        let types = [
            WATCH_TYPE_META,
            WATCH_TYPE_KEY_NOTIFY,
            WATCH_TYPE_MOUNT_NOTIFY,
            WATCH_TYPE_SB_NOTIFY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_key_subtypes_distinct() {
        let subs = [
            NOTIFY_KEY_UPDATED,
            NOTIFY_KEY_LINKED,
            NOTIFY_KEY_UNLINKED,
            NOTIFY_KEY_CLEARED,
            NOTIFY_KEY_REVOKED,
            NOTIFY_KEY_INVALIDATED,
            NOTIFY_KEY_SETATTR,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_mount_subtypes_distinct() {
        let subs = [
            NOTIFY_MOUNT_NEW_MOUNT,
            NOTIFY_MOUNT_UNMOUNT,
            NOTIFY_MOUNT_EXPIRY,
            NOTIFY_MOUNT_MOVE,
            NOTIFY_MOUNT_SETATTR,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_sb_subtypes_distinct() {
        let subs = [
            NOTIFY_SB_ERROR,
            NOTIFY_SB_READONLY,
            NOTIFY_SB_QUOTA,
            NOTIFY_SB_NETWORK,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_meta_subtypes_distinct() {
        assert_ne!(
            WATCH_META_REMOVAL_NOTIFICATION,
            WATCH_META_LOSS_NOTIFICATION
        );
    }

    #[test]
    fn test_masks_no_overlap() {
        // Type and subtype masks occupy different bit ranges
        assert_eq!(WATCH_INFO_TYPE_MASK & WATCH_INFO_SUBTYPE_MASK, 0);
        assert_eq!(WATCH_INFO_SUBTYPE_MASK & WATCH_INFO_LENGTH_MASK, 0);
    }

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(IOC_WATCH_QUEUE_SET_FILTER, IOC_WATCH_QUEUE_SET_SIZE);
    }
}
