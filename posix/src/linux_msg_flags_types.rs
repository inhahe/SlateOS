//! `<sys/socket.h>` — Message send/receive flag constants.
//!
//! These flags are passed to `sendmsg()`, `recvmsg()`, `send()`, and
//! `recv()` to control message delivery semantics: out-of-band data,
//! peeking, non-blocking behavior, and ancillary data handling.

// ---------------------------------------------------------------------------
// send()/recv() flags (msg_flags)
// ---------------------------------------------------------------------------

/// Process out-of-band data.
pub const MSG_OOB: u32 = 0x01;
/// Peek at incoming data without consuming.
pub const MSG_PEEK: u32 = 0x02;
/// Don't use routing table (send on raw).
pub const MSG_DONTROUTE: u32 = 0x04;
/// Data completes record (SEQPACKET).
pub const MSG_EOR: u32 = 0x80;
/// Wait for full request or error.
pub const MSG_WAITALL: u32 = 0x100;
/// Send without blocking.
pub const MSG_DONTWAIT: u32 = 0x40;
/// Truncated message was received.
pub const MSG_TRUNC: u32 = 0x20;
/// Control data was truncated.
pub const MSG_CTRUNC: u32 = 0x08;
/// Don't generate SIGPIPE on broken pipe.
pub const MSG_NOSIGNAL: u32 = 0x4000;
/// Confirm path validity (for UDP).
pub const MSG_CONFIRM: u32 = 0x0800;
/// More data to send (cork-like).
pub const MSG_MORE: u32 = 0x8000;
/// Set close-on-exec for received fds.
pub const MSG_CMSG_CLOEXEC: u32 = 0x40000000;
/// Error queue messages.
pub const MSG_ERRQUEUE: u32 = 0x2000;
/// Fast open (send data in SYN).
pub const MSG_FASTOPEN: u32 = 0x20000000;
/// Use zero-copy sendmsg.
pub const MSG_ZEROCOPY: u32 = 0x4000000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_flags_distinct() {
        let flags = [
            MSG_OOB, MSG_PEEK, MSG_DONTROUTE, MSG_EOR,
            MSG_WAITALL, MSG_DONTWAIT, MSG_TRUNC, MSG_CTRUNC,
            MSG_NOSIGNAL, MSG_CONFIRM, MSG_MORE,
            MSG_CMSG_CLOEXEC, MSG_ERRQUEUE, MSG_FASTOPEN,
            MSG_ZEROCOPY,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_common_values() {
        assert_eq!(MSG_OOB, 0x01);
        assert_eq!(MSG_PEEK, 0x02);
        assert_eq!(MSG_DONTWAIT, 0x40);
        assert_eq!(MSG_NOSIGNAL, 0x4000);
    }

    #[test]
    fn test_cmsg_cloexec() {
        assert_eq!(MSG_CMSG_CLOEXEC, 0x40000000);
    }

    #[test]
    fn test_zerocopy() {
        assert_eq!(MSG_ZEROCOPY, 0x4000000);
    }
}
