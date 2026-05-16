//! `<linux/watch_queue.h>` — General notification (watch queue) constants.
//!
//! The watch queue mechanism provides a general-purpose notification
//! system via pipes. Kernel subsystems post notifications to a pipe
//! buffer, and userspace reads structured events. Used by the
//! keyring subsystem and mount notifications.

// ---------------------------------------------------------------------------
// Notification types
// ---------------------------------------------------------------------------

/// Meta/administrative notification.
pub const WATCH_TYPE_META: u32 = 0;
/// Key/keyring change notification.
pub const WATCH_TYPE_KEY_NOTIFY: u32 = 1;

// ---------------------------------------------------------------------------
// Meta notification subtypes
// ---------------------------------------------------------------------------

/// Loss of events (queue overflowed).
pub const WATCH_META_REMOVAL_NOTIFICATION: u32 = 0;
/// Removal of watch.
pub const WATCH_META_LOSS_NOTIFICATION: u32 = 1;

// ---------------------------------------------------------------------------
// Key notification subtypes
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
/// Key description was set.
pub const NOTIFY_KEY_SETATTR: u32 = 6;

// ---------------------------------------------------------------------------
// Watch queue flags
// ---------------------------------------------------------------------------

/// Notification includes extended info.
pub const WATCH_INFO_LENGTH_SHIFT: u32 = 0;
/// Length mask in watch info header.
pub const WATCH_INFO_LENGTH_MASK: u32 = 0x7F;
/// Type shift in watch info header.
pub const WATCH_INFO_TYPE_SHIFT: u32 = 8;
/// Type mask.
pub const WATCH_INFO_TYPE_MASK: u32 = 0xFF;
/// Subtype shift.
pub const WATCH_INFO_SUBTYPE_SHIFT: u32 = 16;
/// Subtype mask.
pub const WATCH_INFO_SUBTYPE_MASK: u32 = 0xFF;
/// ID shift (for key ID, etc.).
pub const WATCH_INFO_ID_SHIFT: u32 = 24;

// ---------------------------------------------------------------------------
// ioctl for pipe watch queue setup
// ---------------------------------------------------------------------------

/// Set pipe as watch queue (ioctl number).
pub const IOC_WATCH_QUEUE_SET_SIZE: u32 = 0x5760;
/// Set filter on watch queue.
pub const IOC_WATCH_QUEUE_SET_FILTER: u32 = 0x5761;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watch_types_distinct() {
        assert_ne!(WATCH_TYPE_META, WATCH_TYPE_KEY_NOTIFY);
    }

    #[test]
    fn test_meta_subtypes_distinct() {
        assert_ne!(WATCH_META_REMOVAL_NOTIFICATION, WATCH_META_LOSS_NOTIFICATION);
    }

    #[test]
    fn test_key_subtypes_distinct() {
        let subtypes = [
            NOTIFY_KEY_UPDATED, NOTIFY_KEY_LINKED, NOTIFY_KEY_UNLINKED,
            NOTIFY_KEY_CLEARED, NOTIFY_KEY_REVOKED, NOTIFY_KEY_INVALIDATED,
            NOTIFY_KEY_SETATTR,
        ];
        for i in 0..subtypes.len() {
            for j in (i + 1)..subtypes.len() {
                assert_ne!(subtypes[i], subtypes[j]);
            }
        }
    }

    #[test]
    fn test_info_shifts_ordered() {
        assert!(WATCH_INFO_LENGTH_SHIFT < WATCH_INFO_TYPE_SHIFT);
        assert!(WATCH_INFO_TYPE_SHIFT < WATCH_INFO_SUBTYPE_SHIFT);
        assert!(WATCH_INFO_SUBTYPE_SHIFT < WATCH_INFO_ID_SHIFT);
    }

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(IOC_WATCH_QUEUE_SET_SIZE, IOC_WATCH_QUEUE_SET_FILTER);
    }
}
