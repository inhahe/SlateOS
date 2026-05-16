//! `<linux/connector.h>` + `<linux/cn_proc.h>` — Kernel connector / process events.
//!
//! The kernel connector is a netlink-based facility for delivering
//! kernel events to userspace. The most common use is process events
//! (fork, exec, exit, uid/gid changes) used by process monitors
//! like systemd and process accounting tools.

// ---------------------------------------------------------------------------
// Connector IDs
// ---------------------------------------------------------------------------

/// Connector index: process events.
pub const CN_IDX_PROC: u32 = 1;
/// Connector value: process events.
pub const CN_VAL_PROC: u32 = 1;
/// Connector index: CIFS.
pub const CN_IDX_CIFS: u32 = 2;
/// Connector index: W1 (1-wire).
pub const CN_IDX_W1: u32 = 3;
/// Connector index: iSCSI.
pub const CN_IDX_ISCSI: u32 = 4;
/// Connector index: DRBD.
pub const CN_IDX_DRBD: u32 = 6;
/// Connector index: V86D (uvesafb).
pub const CN_IDX_V86D: u32 = 7;

/// Maximum netlink groups for connector.
pub const CN_NETLINK_USERS: u32 = 11;

// ---------------------------------------------------------------------------
// Connector message header
// ---------------------------------------------------------------------------

/// Connector callback ID (8 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CbId {
    /// Index (subsystem).
    pub idx: u32,
    /// Value.
    pub val: u32,
}

/// Connector message header (20 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CnMsg {
    /// Callback ID.
    pub id: CbId,
    /// Sequence number.
    pub seq: u32,
    /// Ack sequence.
    pub ack: u32,
    /// Payload length.
    pub len: u16,
    /// Flags.
    pub flags: u16,
}

impl CnMsg {
    /// Create a zeroed connector message.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Process event types (cn_proc)
// ---------------------------------------------------------------------------

/// No event.
pub const PROC_EVENT_NONE: u32 = 0x00000000;
/// Fork event.
pub const PROC_EVENT_FORK: u32 = 0x00000001;
/// Exec event.
pub const PROC_EVENT_EXEC: u32 = 0x00000002;
/// UID change event.
pub const PROC_EVENT_UID: u32 = 0x00000004;
/// GID change event.
pub const PROC_EVENT_GID: u32 = 0x00000040;
/// Session ID change.
pub const PROC_EVENT_SID: u32 = 0x00000080;
/// ptrace event.
pub const PROC_EVENT_PTRACE: u32 = 0x00000100;
/// comm (process name) change.
pub const PROC_EVENT_COMM: u32 = 0x00000200;
/// coredump event.
pub const PROC_EVENT_COREDUMP: u32 = 0x40000000;
/// Exit event.
pub const PROC_EVENT_EXIT: u32 = 0x80000000;

// ---------------------------------------------------------------------------
// Process connector multicast operations
// ---------------------------------------------------------------------------

/// Listen for process events.
pub const PROC_CN_MCAST_LISTEN: u32 = 1;
/// Ignore process events.
pub const PROC_CN_MCAST_IGNORE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cb_id_size() {
        assert_eq!(core::mem::size_of::<CbId>(), 8);
    }

    #[test]
    fn test_cn_msg_size() {
        assert_eq!(core::mem::size_of::<CnMsg>(), 20);
    }

    #[test]
    fn test_connector_ids() {
        assert_eq!(CN_IDX_PROC, 1);
        assert_eq!(CN_VAL_PROC, 1);
    }

    #[test]
    fn test_proc_events_distinct() {
        let events = [
            PROC_EVENT_NONE, PROC_EVENT_FORK, PROC_EVENT_EXEC,
            PROC_EVENT_UID, PROC_EVENT_GID, PROC_EVENT_SID,
            PROC_EVENT_PTRACE, PROC_EVENT_COMM,
            PROC_EVENT_COREDUMP, PROC_EVENT_EXIT,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_proc_mcast_ops() {
        assert_eq!(PROC_CN_MCAST_LISTEN, 1);
        assert_eq!(PROC_CN_MCAST_IGNORE, 2);
    }

    #[test]
    fn test_idx_values_distinct() {
        let idxs = [
            CN_IDX_PROC, CN_IDX_CIFS, CN_IDX_W1,
            CN_IDX_ISCSI, CN_IDX_DRBD, CN_IDX_V86D,
        ];
        for i in 0..idxs.len() {
            for j in (i + 1)..idxs.len() {
                assert_ne!(idxs[i], idxs[j]);
            }
        }
    }
}
