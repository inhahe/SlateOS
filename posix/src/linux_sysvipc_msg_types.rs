//! `<linux/msg.h>` — System V message queue constants.
//!
//! System V message queues allow processes to exchange typed messages.
//! Each message has a type (positive long) and a data payload. Messages
//! are stored in kernel memory and persist until explicitly received
//! or the queue is removed. Receivers can select messages by type
//! (receive first message of a specific type, or first message of
//! any type ≤ a threshold). Older than POSIX mqueues but still
//! widely used in legacy applications.

// ---------------------------------------------------------------------------
// msgrcv() flags
// ---------------------------------------------------------------------------

/// Don't wait if no message available (non-blocking).
pub const MSG_NOERROR: u32 = 0o10000;
/// Truncate message if it exceeds the receive buffer.
pub const MSG_EXCEPT: u32 = 0o20000;
/// Receive any message except the specified type.
pub const MSG_COPY: u32 = 0o40000;

// ---------------------------------------------------------------------------
// msgctl() commands (in addition to IPC_RMID, IPC_SET, IPC_STAT)
// ---------------------------------------------------------------------------

/// Get system-wide message queue info.
pub const MSG_INFO: u32 = 12;
/// Get message queue status by index.
pub const MSG_STAT: u32 = 11;
/// Like MSG_STAT but respects permissions.
pub const MSG_STAT_ANY: u32 = 13;

// ---------------------------------------------------------------------------
// Message queue limits
// ---------------------------------------------------------------------------

/// Default maximum number of message queues system-wide.
pub const MSGMNI: u32 = 32000;
/// Default maximum message size in bytes (8 KiB).
pub const MSGMAX: u32 = 8192;
/// Default maximum total bytes in a single queue (16 KiB).
pub const MSGMNB: u32 = 16384;
/// Maximum number of message headers system-wide.
pub const MSGTQL: u32 = 2048;
/// Size of message segment (kernel internal).
pub const MSGSSZ: u32 = 16;
/// Maximum number of segments per message.
pub const MSGSEG: u32 = 32767;

// ---------------------------------------------------------------------------
// Message type special values
// ---------------------------------------------------------------------------

/// Receive any message type (first in queue).
pub const MSG_TYPE_ANY: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recv_flags_no_overlap() {
        let flags = [MSG_NOERROR, MSG_EXCEPT, MSG_COPY];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ctl_commands_distinct() {
        let cmds = [MSG_INFO, MSG_STAT, MSG_STAT_ANY];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_limits_positive() {
        assert!(MSGMNI > 0);
        assert!(MSGMAX > 0);
        assert!(MSGMNB > 0);
        assert!(MSGTQL > 0);
        assert!(MSGSSZ > 0);
        assert!(MSGSEG > 0);
    }

    #[test]
    fn test_msg_type_any_is_zero() {
        assert_eq!(MSG_TYPE_ANY, 0);
    }
}
