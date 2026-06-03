//! `<linux/fanotify.h>` — filesystem notification ABI (fanotify).
//!
//! fanotify gives userspace antivirus, container runtimes, and
//! HSM agents (Veeam, ClamAV, Lustre) a way to receive open/access/
//! modify events on whole mounts and to optionally permit or deny
//! the operation. It complements inotify with permission events and
//! mount/superblock-wide scope.

// ---------------------------------------------------------------------------
// fanotify_init() flags
// ---------------------------------------------------------------------------

/// Receive event notifications only.
pub const FAN_CLASS_NOTIF: u32 = 0x0000_0000;
/// Receive permission events (must be uid 0 / CAP_SYS_ADMIN).
pub const FAN_CLASS_CONTENT: u32 = 0x0000_0004;
/// Receive permission events before content has been cached.
pub const FAN_CLASS_PRE_CONTENT: u32 = 0x0000_0008;
/// Mask covering all classes.
pub const FAN_ALL_CLASS_BITS: u32 =
    FAN_CLASS_NOTIF | FAN_CLASS_CONTENT | FAN_CLASS_PRE_CONTENT;

/// fanotify fd is close-on-exec.
pub const FAN_CLOEXEC: u32 = 0x0000_0001;
/// fanotify fd is non-blocking.
pub const FAN_NONBLOCK: u32 = 0x0000_0002;
/// Unlimited queue.
pub const FAN_UNLIMITED_QUEUE: u32 = 0x0000_0010;
/// Unlimited marks (no per-process cap).
pub const FAN_UNLIMITED_MARKS: u32 = 0x0000_0020;
/// Report TID, not PID, in events.
pub const FAN_REPORT_TID: u32 = 0x0000_0100;
/// Report file-id (FID) rather than fd in events.
pub const FAN_REPORT_FID: u32 = 0x0000_0200;
/// Report directory FID + name.
pub const FAN_REPORT_DIR_FID: u32 = 0x0000_0400;
/// Report filename of affected entry.
pub const FAN_REPORT_NAME: u32 = 0x0000_0800;
/// Report containing-dir FID + name for create/move events.
pub const FAN_REPORT_DFID_NAME: u32 = FAN_REPORT_DIR_FID | FAN_REPORT_NAME;

// ---------------------------------------------------------------------------
// Event mask bits (fanotify_mark + event_metadata.mask)
// ---------------------------------------------------------------------------

/// File or dir was accessed (read).
pub const FAN_ACCESS: u64 = 0x0000_0001;
/// File or dir was modified.
pub const FAN_MODIFY: u64 = 0x0000_0002;
/// Writable fd closed.
pub const FAN_CLOSE_WRITE: u64 = 0x0000_0008;
/// Non-writable fd closed.
pub const FAN_CLOSE_NOWRITE: u64 = 0x0000_0010;
/// File or dir was opened.
pub const FAN_OPEN: u64 = 0x0000_0020;
/// File or dir was opened with O_EXEC intent.
pub const FAN_OPEN_EXEC: u64 = 0x0000_0040;
/// Event queue overflowed.
pub const FAN_Q_OVERFLOW: u64 = 0x0000_4000;
/// Permission to open requested.
pub const FAN_OPEN_PERM: u64 = 0x0001_0000;
/// Permission to access requested.
pub const FAN_ACCESS_PERM: u64 = 0x0002_0000;
/// Permission to exec requested.
pub const FAN_OPEN_EXEC_PERM: u64 = 0x0004_0000;
/// Apply to directories too.
pub const FAN_ONDIR: u64 = 0x4000_0000;
/// Apply to children only.
pub const FAN_EVENT_ON_CHILD: u64 = 0x0800_0000;

// ---------------------------------------------------------------------------
// Permission-response values written back to fd
// ---------------------------------------------------------------------------

/// Allow the operation.
pub const FAN_ALLOW: u32 = 0x01;
/// Deny the operation.
pub const FAN_DENY: u32 = 0x02;
/// Audit the response.
pub const FAN_AUDIT: u32 = 0x10;

// ---------------------------------------------------------------------------
// fanotify_mark() actions
// ---------------------------------------------------------------------------

/// Add to mark mask.
pub const FAN_MARK_ADD: u32 = 0x01;
/// Remove from mark mask.
pub const FAN_MARK_REMOVE: u32 = 0x02;
/// Mark inode (default).
pub const FAN_MARK_INODE: u32 = 0x00;
/// Mark mountpoint.
pub const FAN_MARK_MOUNT: u32 = 0x10;
/// Mark filesystem (superblock).
pub const FAN_MARK_FILESYSTEM: u32 = 0x100;
/// Mark all (ignore mask).
pub const FAN_MARK_IGNORED_MASK: u32 = 0x20;
/// Mark applied on filesystem-wide.
pub const FAN_MARK_FLUSH: u32 = 0x80;

// ---------------------------------------------------------------------------
// Protocol metadata
// ---------------------------------------------------------------------------

/// Wire-protocol version reported in event_metadata.vers.
pub const FANOTIFY_METADATA_VERSION: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classes_dense_within_mask() {
        // The 3 classes use the 0x4|0x8 = 0xC slot pair (NOTIF=0).
        assert_eq!(FAN_CLASS_NOTIF, 0);
        assert_eq!(FAN_CLASS_CONTENT, 4);
        assert_eq!(FAN_CLASS_PRE_CONTENT, 8);
        assert_eq!(FAN_ALL_CLASS_BITS, 0xC);
    }

    #[test]
    fn test_init_flags_distinct() {
        let f = [
            FAN_CLOEXEC,
            FAN_NONBLOCK,
            FAN_UNLIMITED_QUEUE,
            FAN_UNLIMITED_MARKS,
            FAN_REPORT_TID,
            FAN_REPORT_FID,
            FAN_REPORT_DIR_FID,
            FAN_REPORT_NAME,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
        // DFID_NAME is the combo userspace passes for create/move.
        assert_eq!(FAN_REPORT_DFID_NAME, FAN_REPORT_DIR_FID | FAN_REPORT_NAME);
    }

    #[test]
    fn test_event_bits_pow2_distinct() {
        let e = [
            FAN_ACCESS,
            FAN_MODIFY,
            FAN_CLOSE_WRITE,
            FAN_CLOSE_NOWRITE,
            FAN_OPEN,
            FAN_OPEN_EXEC,
            FAN_Q_OVERFLOW,
            FAN_OPEN_PERM,
            FAN_ACCESS_PERM,
            FAN_OPEN_EXEC_PERM,
            FAN_ONDIR,
            FAN_EVENT_ON_CHILD,
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
    fn test_permission_responses() {
        assert_eq!(FAN_ALLOW, 1);
        assert_eq!(FAN_DENY, 2);
        // FAN_AUDIT can be OR'd with ALLOW or DENY.
        assert!(FAN_AUDIT.is_power_of_two());
        assert_ne!(FAN_AUDIT, FAN_ALLOW);
        assert_ne!(FAN_AUDIT, FAN_DENY);
    }

    #[test]
    fn test_mark_actions_distinct() {
        // ADD/REMOVE are mutually exclusive (one bit each).
        assert_ne!(FAN_MARK_ADD, FAN_MARK_REMOVE);
        // Scope flags are also distinct single bits.
        let m = [
            FAN_MARK_MOUNT,
            FAN_MARK_FILESYSTEM,
            FAN_MARK_IGNORED_MASK,
            FAN_MARK_FLUSH,
        ];
        for &b in &m {
            assert!(b.is_power_of_two());
        }
    }

    #[test]
    fn test_protocol_version() {
        // The current stable wire protocol is version 3.
        assert_eq!(FANOTIFY_METADATA_VERSION, 3);
    }
}
