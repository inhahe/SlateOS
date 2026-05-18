//! `<linux/netlink.h>` — Netlink message header and group constants.
//!
//! Netlink sockets provide communication between kernel and
//! userspace.  These constants define message header flags,
//! standard message types, and multicast group assignments.

// ---------------------------------------------------------------------------
// Netlink message header flags (nlmsghdr.nlmsg_flags)
// ---------------------------------------------------------------------------

/// Request message (must get a reply).
pub const NLM_F_REQUEST: u16 = 0x0001;
/// Multipart message (more parts follow).
pub const NLM_F_MULTI: u16 = 0x0002;
/// Reply with ACK on success.
pub const NLM_F_ACK: u16 = 0x0004;
/// Echo this request back.
pub const NLM_F_ECHO: u16 = 0x0008;
/// Dump was inconsistent (data changed during dump).
pub const NLM_F_DUMP_INTR: u16 = 0x0010;
/// Dump was filtered.
pub const NLM_F_DUMP_FILTERED: u16 = 0x0020;

// ---------------------------------------------------------------------------
// GET request flags (additional)
// ---------------------------------------------------------------------------

/// Return the complete table (match root).
pub const NLM_F_ROOT: u16 = 0x0100;
/// Return all matching entries.
pub const NLM_F_MATCH: u16 = 0x0200;
/// Atomic snapshot (deprecated).
pub const NLM_F_ATOMIC: u16 = 0x0400;
/// Dump request (ROOT | MATCH).
pub const NLM_F_DUMP: u16 = NLM_F_ROOT | NLM_F_MATCH;

// ---------------------------------------------------------------------------
// NEW request flags (additional)
// ---------------------------------------------------------------------------

/// Replace existing entry.
pub const NLM_F_REPLACE: u16 = 0x0100;
/// Do not replace if exists.
pub const NLM_F_EXCL: u16 = 0x0200;
/// Create entry if it doesn't exist.
pub const NLM_F_CREATE: u16 = 0x0400;
/// Append to end of list.
pub const NLM_F_APPEND: u16 = 0x0800;

// ---------------------------------------------------------------------------
// Standard message types
// ---------------------------------------------------------------------------

/// No operation.
pub const NLMSG_NOOP: u16 = 0x1;
/// Error or ACK.
pub const NLMSG_ERROR: u16 = 0x2;
/// End of multipart dump.
pub const NLMSG_DONE: u16 = 0x3;
/// Overrun notification.
pub const NLMSG_OVERRUN: u16 = 0x4;
/// Minimum reserved message type.
pub const NLMSG_MIN_TYPE: u16 = 0x10;

// ---------------------------------------------------------------------------
// Netlink header layout
// ---------------------------------------------------------------------------

/// Size of struct nlmsghdr (bytes).
pub const NLMSG_HDRLEN: u32 = 16;
/// Alignment for netlink messages.
pub const NLMSG_ALIGNTO: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_flags_no_overlap() {
        let flags = [NLM_F_REQUEST, NLM_F_MULTI, NLM_F_ACK, NLM_F_ECHO];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_request_is_one() {
        assert_eq!(NLM_F_REQUEST, 1);
    }

    #[test]
    fn test_dump_is_root_or_match() {
        assert_eq!(NLM_F_DUMP, NLM_F_ROOT | NLM_F_MATCH);
    }

    #[test]
    fn test_msg_types_distinct() {
        let types = [NLMSG_NOOP, NLMSG_ERROR, NLMSG_DONE, NLMSG_OVERRUN];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_min_type() {
        assert_eq!(NLMSG_MIN_TYPE, 16);
    }

    #[test]
    fn test_hdrlen() {
        assert_eq!(NLMSG_HDRLEN, 16);
    }

    #[test]
    fn test_alignto() {
        assert_eq!(NLMSG_ALIGNTO, 4);
        assert!(NLMSG_ALIGNTO.is_power_of_two());
    }
}
