//! `<linux/cn_proc.h>` — Process event connector constants.
//!
//! The process event connector provides real-time notifications of
//! process lifecycle events over a netlink socket (CN_IDX_PROC).
//! Monitoring daemons subscribe to receive events for fork, exec,
//! exit, UID/GID changes, and other state transitions.

// ---------------------------------------------------------------------------
// Process event types (what field in proc_event)
// ---------------------------------------------------------------------------

/// No event (used as acknowledgment).
pub const PROC_EVENT_NONE: u32 = 0x0000_0000;
/// Fork event (new child process created).
pub const PROC_EVENT_FORK: u32 = 0x0000_0001;
/// Exec event (process called execve).
pub const PROC_EVENT_EXEC: u32 = 0x0000_0002;
/// UID change event (setuid/setreuid/etc.).
pub const PROC_EVENT_UID: u32 = 0x0000_0004;
/// GID change event (setgid/setregid/etc.).
pub const PROC_EVENT_GID: u32 = 0x0000_0040;
/// Session ID change event (setsid).
pub const PROC_EVENT_SID: u32 = 0x0000_0080;
/// Ptrace event (ptrace attach/detach).
pub const PROC_EVENT_PTRACE: u32 = 0x0000_0100;
/// Comm change event (process name changed via prctl).
pub const PROC_EVENT_COMM: u32 = 0x0000_0200;
/// Coredump event (process dumped core).
pub const PROC_EVENT_COREDUMP: u32 = 0x4000_0000;
/// Exit event (process exited).
pub const PROC_EVENT_EXIT: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// Connector control operations
// ---------------------------------------------------------------------------

/// Listen for process events (subscribe).
pub const PROC_CN_MCAST_LISTEN: u32 = 1;
/// Ignore process events (unsubscribe).
pub const PROC_CN_MCAST_IGNORE: u32 = 2;

// ---------------------------------------------------------------------------
// Connector netlink IDs
// ---------------------------------------------------------------------------

/// Connector index for process events.
pub const CN_IDX_PROC: u32 = 1;
/// Connector value for process events.
pub const CN_VAL_PROC: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_no_overlap() {
        let events = [
            PROC_EVENT_FORK, PROC_EVENT_EXEC, PROC_EVENT_UID,
            PROC_EVENT_GID, PROC_EVENT_SID, PROC_EVENT_PTRACE,
            PROC_EVENT_COMM, PROC_EVENT_COREDUMP, PROC_EVENT_EXIT,
        ];
        for i in 0..events.len() {
            assert!(events[i].is_power_of_two());
            for j in (i + 1)..events.len() {
                assert_eq!(events[i] & events[j], 0);
            }
        }
    }

    #[test]
    fn test_none_event_is_zero() {
        assert_eq!(PROC_EVENT_NONE, 0);
    }

    #[test]
    fn test_mcast_ops_distinct() {
        assert_ne!(PROC_CN_MCAST_LISTEN, PROC_CN_MCAST_IGNORE);
    }

    #[test]
    fn test_connector_ids() {
        assert_eq!(CN_IDX_PROC, 1);
        assert_eq!(CN_VAL_PROC, 1);
    }
}
