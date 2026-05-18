//! `<sys/msg.h>` — System V message queue constants.
//!
//! System V message queues (`msgget`, `msgsnd`, `msgrcv`, `msgctl`)
//! provide inter-process message passing.  These constants define
//! flags, ctl commands, and default limits.

// ---------------------------------------------------------------------------
// msgget() flags
// ---------------------------------------------------------------------------

/// Create a new message queue.
pub const IPC_CREAT_MSG: u32 = 0o1000;
/// Fail if queue exists (with IPC_CREAT).
pub const IPC_EXCL_MSG: u32 = 0o2000;

// ---------------------------------------------------------------------------
// msgsnd() / msgrcv() flags
// ---------------------------------------------------------------------------

/// Do not block if the queue is full (msgsnd) or empty (msgrcv).
pub const IPC_NOWAIT_MSG: u32 = 0o4000;
/// Receive any message with type <= abs(msgtyp).
pub const MSG_NOERROR: u32 = 0o10000;
/// Receive the first message of any type except msgtyp.
pub const MSG_EXCEPT: u32 = 0o20000;
/// Copy message without removing it (Linux extension).
pub const MSG_COPY: u32 = 0o40000;

// ---------------------------------------------------------------------------
// msgctl() commands
// ---------------------------------------------------------------------------

/// Get message queue info.
pub const IPC_STAT_MSG: u32 = 2;
/// Set message queue info.
pub const IPC_SET_MSG: u32 = 1;
/// Remove message queue.
pub const IPC_RMID_MSG: u32 = 0;
/// Get system-wide message queue info (Linux extension).
pub const IPC_INFO_MSG: u32 = 3;
/// Get queue info by index (Linux extension).
pub const MSG_INFO: u32 = 12;
/// Get statistics (Linux extension).
pub const MSG_STAT: u32 = 11;
/// Get statistics, newer version (Linux extension).
pub const MSG_STAT_ANY: u32 = 13;

// ---------------------------------------------------------------------------
// Message queue limits (defaults)
// ---------------------------------------------------------------------------

/// Maximum message size (bytes, default).
pub const MSGMAX_DEFAULT: u32 = 8192;
/// Maximum bytes on a queue (default).
pub const MSGMNB_DEFAULT: u32 = 16384;
/// Maximum number of message queues system-wide (default).
pub const MSGMNI_DEFAULT: u32 = 32000;
/// Maximum number of messages system-wide (default).
pub const MSGTQL_DEFAULT: u32 = 16384;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_flags_no_overlap() {
        assert_eq!(IPC_CREAT_MSG & IPC_EXCL_MSG, 0);
    }

    #[test]
    fn test_sndrcv_flags_distinct() {
        let flags = [IPC_NOWAIT_MSG, MSG_NOERROR, MSG_EXCEPT, MSG_COPY];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_ctl_commands_distinct() {
        let cmds = [
            IPC_STAT_MSG, IPC_SET_MSG, IPC_RMID_MSG,
            IPC_INFO_MSG, MSG_INFO, MSG_STAT, MSG_STAT_ANY,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_rmid_is_zero() {
        assert_eq!(IPC_RMID_MSG, 0);
    }

    #[test]
    fn test_msgmax_default() {
        assert_eq!(MSGMAX_DEFAULT, 8192);
    }

    #[test]
    fn test_msgmnb_default() {
        assert_eq!(MSGMNB_DEFAULT, 16384);
    }

    #[test]
    fn test_msgmni_default() {
        assert_eq!(MSGMNI_DEFAULT, 32000);
    }

    #[test]
    fn test_limits_positive() {
        assert!(MSGMAX_DEFAULT > 0);
        assert!(MSGMNB_DEFAULT > 0);
        assert!(MSGMNI_DEFAULT > 0);
        assert!(MSGTQL_DEFAULT > 0);
    }
}
