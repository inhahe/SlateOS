//! `<linux/connector.h>` — Kernel connector (CN) netlink constants.
//!
//! The connector is a simple kernel→userspace notification bus built
//! on netlink. Kernel subsystems register by ID and send structured
//! messages to userspace listeners. Unlike genetlink, connector is
//! fire-and-forget with minimal overhead. Used primarily by the
//! process events connector (fork/exec/exit notifications for
//! systemd, Docker, audit) and a few legacy drivers.

// ---------------------------------------------------------------------------
// Connector IDs (CN_IDX_*)
// ---------------------------------------------------------------------------

/// Connector index: process events (fork/exec/exit/uid/gid/sid).
pub const CN_IDX_PROC: u32 = 1;
/// Connector index: CIFS (SMB) notifications.
pub const CN_IDX_CIFS: u32 = 2;
/// Connector index: W1 (1-Wire bus).
pub const CN_IDX_W1: u32 = 3;
/// Connector index: volume ID.
pub const CN_IDX_V86D: u32 = 4;
/// Connector index: DRBD (distributed block device).
pub const CN_IDX_DRBD: u32 = 5;

// ---------------------------------------------------------------------------
// Connector value IDs (CN_VAL_*)
// ---------------------------------------------------------------------------

/// Value: process events (generic).
pub const CN_VAL_PROC: u32 = 1;
/// Value: CIFS.
pub const CN_VAL_CIFS: u32 = 1;
/// Value: W1 master.
pub const CN_VAL_W1_MASTER: u32 = 1;
/// Value: W1 slave.
pub const CN_VAL_W1_SLAVE: u32 = 2;

// ---------------------------------------------------------------------------
// Process event types (proc connector)
// ---------------------------------------------------------------------------

/// No event (used in subscribe/unsubscribe).
pub const PROC_EVENT_NONE: u32 = 0x0000_0000;
/// Fork event.
pub const PROC_EVENT_FORK: u32 = 0x0000_0001;
/// Exec event.
pub const PROC_EVENT_EXEC: u32 = 0x0000_0002;
/// UID change event (setuid/setreuid).
pub const PROC_EVENT_UID: u32 = 0x0000_0004;
/// GID change event (setgid/setregid).
pub const PROC_EVENT_GID: u32 = 0x0000_0040;
/// SID change event (setsid).
pub const PROC_EVENT_SID: u32 = 0x0000_0080;
/// Ptrace attach/detach event.
pub const PROC_EVENT_PTRACE: u32 = 0x0000_0100;
/// Process name change (comm, via prctl).
pub const PROC_EVENT_COMM: u32 = 0x0000_0200;
/// Coredump event.
pub const PROC_EVENT_COREDUMP: u32 = 0x4000_0000;
/// Exit event.
pub const PROC_EVENT_EXIT: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// Proc connector operations
// ---------------------------------------------------------------------------

/// Subscribe to process events.
pub const PROC_CN_MCAST_LISTEN: u32 = 1;
/// Unsubscribe from process events.
pub const PROC_CN_MCAST_IGNORE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_indices_distinct() {
        let idxs = [CN_IDX_PROC, CN_IDX_CIFS, CN_IDX_W1, CN_IDX_V86D, CN_IDX_DRBD];
        for i in 0..idxs.len() {
            for j in (i + 1)..idxs.len() {
                assert_ne!(idxs[i], idxs[j]);
            }
        }
    }

    #[test]
    fn test_proc_events_no_overlap() {
        let events = [
            PROC_EVENT_FORK, PROC_EVENT_EXEC, PROC_EVENT_UID,
            PROC_EVENT_GID, PROC_EVENT_SID, PROC_EVENT_PTRACE,
            PROC_EVENT_COMM, PROC_EVENT_COREDUMP, PROC_EVENT_EXIT,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_eq!(events[i] & events[j], 0);
            }
        }
    }

    #[test]
    fn test_proc_event_none_is_zero() {
        assert_eq!(PROC_EVENT_NONE, 0);
    }

    #[test]
    fn test_mcast_ops_distinct() {
        assert_ne!(PROC_CN_MCAST_LISTEN, PROC_CN_MCAST_IGNORE);
    }
}
