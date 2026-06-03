//! `<sys/fanotify.h>` — `fanotify_init(2)` flags and class bits.
//!
//! fanotify (filesystem-wide notify) is used by Linux antivirus
//! daemons (clamav-onaccess), audit/observability tools, and
//! container runtimes that need permission events. The flag bits
//! below split into two arguments: the first selects a notification
//! class and event-report features; the second selects descriptor
//! semantics for the fanotify fd itself.

// ---------------------------------------------------------------------------
// Notification classes (first arg to fanotify_init, low 2 bits)
// ---------------------------------------------------------------------------

/// Pre-content / notify-only — the default. Events are observed.
pub const FAN_CLASS_NOTIF: u32 = 0x0000_0000;
/// Content-access permission class — allow/deny on open/read.
pub const FAN_CLASS_CONTENT: u32 = 0x0000_0004;
/// Pre-content permission class — allow/deny before content is filled.
pub const FAN_CLASS_PRE_CONTENT: u32 = 0x0000_0008;

/// Mask covering all notification-class bits.
pub const FAN_ALL_CLASS_BITS: u32 =
    FAN_CLASS_NOTIF | FAN_CLASS_CONTENT | FAN_CLASS_PRE_CONTENT;

// ---------------------------------------------------------------------------
// fanotify_init flags (first arg, OR'd with one class)
// ---------------------------------------------------------------------------

/// Close-on-exec the fanotify fd.
pub const FAN_CLOEXEC: u32 = 0x0000_0001;
/// Make the fanotify fd non-blocking.
pub const FAN_NONBLOCK: u32 = 0x0000_0002;
/// Use an unlimited mark count.
pub const FAN_UNLIMITED_QUEUE: u32 = 0x0000_0010;
/// Use an unlimited mark count.
pub const FAN_UNLIMITED_MARKS: u32 = 0x0000_0020;
/// Enable audit-mode reporting on permission events.
pub const FAN_ENABLE_AUDIT: u32 = 0x0000_0040;
/// Report file-id (FID) records instead of fd records.
pub const FAN_REPORT_FID: u32 = 0x0000_0200;
/// Report directory-fid (DFID) records.
pub const FAN_REPORT_DIR_FID: u32 = 0x0000_0400;
/// Report names along with DFIDs.
pub const FAN_REPORT_NAME: u32 = 0x0000_0800;
/// Composite: report directory fid + name (DFID+NAME).
pub const FAN_REPORT_DFID_NAME: u32 = FAN_REPORT_DIR_FID | FAN_REPORT_NAME;
/// Report the target's pidfd as well as TID.
pub const FAN_REPORT_PIDFD: u32 = 0x0000_0080;
/// Report target's TID instead of leader PID.
pub const FAN_REPORT_TID: u32 = 0x0000_0100;

// ---------------------------------------------------------------------------
// Event fd flags (second arg to fanotify_init)
// ---------------------------------------------------------------------------

/// Open the per-event fd read-only.
pub const FAN_EVENT_RDONLY: u32 = 0;
/// Open the per-event fd write-only.
pub const FAN_EVENT_WRONLY: u32 = 1;
/// Open the per-event fd read-write.
pub const FAN_EVENT_RDWR: u32 = 2;
/// Add `O_LARGEFILE` to the per-event fd.
pub const FAN_EVENT_LARGEFILE: u32 = 0x0000_8000;
/// Add `O_CLOEXEC` to the per-event fd.
pub const FAN_EVENT_CLOEXEC: u32 = 0x0008_0000;
/// Add `O_APPEND` to the per-event fd.
pub const FAN_EVENT_APPEND: u32 = 0x0010_0000;
/// Add `O_NONBLOCK` to the per-event fd.
pub const FAN_EVENT_NONBLOCK: u32 = 0x0000_0800;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_class_default_zero() {
        // FAN_CLASS_NOTIF == 0 lets `fanotify_init(0, ...)` use the
        // default notification class without explicit bits.
        assert_eq!(FAN_CLASS_NOTIF, 0);
        assert_ne!(FAN_CLASS_CONTENT, FAN_CLASS_PRE_CONTENT);
        assert_ne!(FAN_CLASS_NOTIF, FAN_CLASS_CONTENT);
        assert_eq!(
            FAN_ALL_CLASS_BITS,
            FAN_CLASS_CONTENT | FAN_CLASS_PRE_CONTENT
        );
    }

    #[test]
    fn test_init_flags_distinct_pow2() {
        let f = [
            FAN_CLOEXEC,
            FAN_NONBLOCK,
            FAN_UNLIMITED_QUEUE,
            FAN_UNLIMITED_MARKS,
            FAN_ENABLE_AUDIT,
            FAN_REPORT_FID,
            FAN_REPORT_DIR_FID,
            FAN_REPORT_NAME,
            FAN_REPORT_PIDFD,
            FAN_REPORT_TID,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_report_dfid_name_composite() {
        // The DFID+NAME shortcut must equal exactly the OR of its
        // components, otherwise userspace using the shortcut would
        // miss a feature bit.
        assert_eq!(
            FAN_REPORT_DFID_NAME,
            FAN_REPORT_DIR_FID | FAN_REPORT_NAME
        );
    }

    #[test]
    fn test_event_open_modes_distinct() {
        let m = [FAN_EVENT_RDONLY, FAN_EVENT_WRONLY, FAN_EVENT_RDWR];
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
        // RDONLY=0, matches O_RDONLY semantics so a zeroed second arg
        // opens for read.
        assert_eq!(FAN_EVENT_RDONLY, 0);
    }

    #[test]
    fn test_event_extra_flags_pow2() {
        let f = [
            FAN_EVENT_LARGEFILE,
            FAN_EVENT_CLOEXEC,
            FAN_EVENT_APPEND,
            FAN_EVENT_NONBLOCK,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_init_and_event_flag_bits_disjoint() {
        // FAN_NONBLOCK (0x2) vs FAN_EVENT_NONBLOCK (0x800) live in
        // separate argument words. They must not be confused.
        assert_ne!(FAN_NONBLOCK, FAN_EVENT_NONBLOCK);
    }
}
