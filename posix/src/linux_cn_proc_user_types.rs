//! `<linux/cn_proc.h>` — process-event connector protocol.
//!
//! Userspace consumers (audit-relay, runtime-protection agents,
//! orchestration health monitors) bind a NETLINK_CONNECTOR socket
//! to receive notifications about fork/exec/exit/uid-change events
//! without polling /proc.

// ---------------------------------------------------------------------------
// Connector IDs (struct cb_id.idx / .val)
// ---------------------------------------------------------------------------

/// Connector subsystem index for process events.
pub const CN_IDX_PROC: u32 = 0x0001;
/// Connector subsystem value for process events.
pub const CN_VAL_PROC: u32 = 0x0001;

// ---------------------------------------------------------------------------
// Listener controls (struct proc_event.what for incoming "mcast" cmd)
// ---------------------------------------------------------------------------

/// PROC_CN_MCAST_LISTEN — start receiving events.
pub const PROC_CN_MCAST_LISTEN: u32 = 1;
/// PROC_CN_MCAST_IGNORE — stop receiving events.
pub const PROC_CN_MCAST_IGNORE: u32 = 2;

// ---------------------------------------------------------------------------
// proc_event.what (enum)
// ---------------------------------------------------------------------------

/// No event.
pub const PROC_EVENT_NONE: u32 = 0x0000_0000;
/// fork() — both pids and tids reported.
pub const PROC_EVENT_FORK: u32 = 0x0000_0001;
/// exec() — process replaced binary image.
pub const PROC_EVENT_EXEC: u32 = 0x0000_0002;
/// UID change.
pub const PROC_EVENT_UID: u32 = 0x0000_0004;
/// GID change.
pub const PROC_EVENT_GID: u32 = 0x0000_0040;
/// SID change.
pub const PROC_EVENT_SID: u32 = 0x0000_0080;
/// ptrace attach/detach.
pub const PROC_EVENT_PTRACE: u32 = 0x0000_0100;
/// process renamed (e.g. via prctl PR_SET_NAME).
pub const PROC_EVENT_COMM: u32 = 0x0000_0200;
/// coredump produced.
pub const PROC_EVENT_COREDUMP: u32 = 0x4000_0000;
/// exit() — last bit (sign bit on i32 — userspace uses u32 for it).
pub const PROC_EVENT_EXIT: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// Netlink groups
// ---------------------------------------------------------------------------

/// `NETLINK_CONNECTOR` family number.
pub const NETLINK_CONNECTOR: u32 = 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_id() {
        // (1, 1) is the only valid ID for the process connector.
        assert_eq!(CN_IDX_PROC, 1);
        assert_eq!(CN_VAL_PROC, 1);
    }

    #[test]
    fn test_listener_controls() {
        assert_eq!(PROC_CN_MCAST_LISTEN, 1);
        assert_eq!(PROC_CN_MCAST_IGNORE, 2);
    }

    #[test]
    fn test_event_what_distinct() {
        let e = [
            PROC_EVENT_NONE,
            PROC_EVENT_FORK,
            PROC_EVENT_EXEC,
            PROC_EVENT_UID,
            PROC_EVENT_GID,
            PROC_EVENT_SID,
            PROC_EVENT_PTRACE,
            PROC_EVENT_COMM,
            PROC_EVENT_COREDUMP,
            PROC_EVENT_EXIT,
        ];
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
        }
    }

    #[test]
    fn test_event_what_pow2_for_active_bits() {
        // Every defined event except NONE is a single bit (the kernel
        // ORs them together when building filter masks).
        for &b in &[
            PROC_EVENT_FORK,
            PROC_EVENT_EXEC,
            PROC_EVENT_UID,
            PROC_EVENT_GID,
            PROC_EVENT_SID,
            PROC_EVENT_PTRACE,
            PROC_EVENT_COMM,
            PROC_EVENT_COREDUMP,
            PROC_EVENT_EXIT,
        ] {
            assert!(b.is_power_of_two());
        }
    }

    #[test]
    fn test_netlink_connector_family() {
        assert_eq!(NETLINK_CONNECTOR, 11);
    }
}
