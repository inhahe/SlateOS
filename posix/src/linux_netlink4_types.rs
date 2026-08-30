//! `<linux/netlink.h>` — Additional netlink constants.
//!
//! Supplementary netlink constants covering extended ACK flags,
//! socket diagnosis attributes, and netlink control messages.

// ---------------------------------------------------------------------------
// Netlink extended ACK attribute types
// ---------------------------------------------------------------------------

/// No attribute.
pub const NLMSGERR_ATTR_UNUSED: u32 = 0;
/// Error message string.
pub const NLMSGERR_ATTR_MSG: u32 = 1;
/// Offset of the invalid attribute.
pub const NLMSGERR_ATTR_OFFS: u32 = 2;
/// Cookie.
pub const NLMSGERR_ATTR_COOKIE: u32 = 3;
/// Policy attributes.
pub const NLMSGERR_ATTR_POLICY: u32 = 4;
/// Missed type.
pub const NLMSGERR_ATTR_MISS_TYPE: u32 = 5;
/// Missed nest.
pub const NLMSGERR_ATTR_MISS_NEST: u32 = 6;

// ---------------------------------------------------------------------------
// Netlink message flags (additional)
// ---------------------------------------------------------------------------

/// Extended ACK available.
pub const NLM_F_ACK_TLVS: u32 = 0x200;
/// Capped result.
pub const NLM_F_CAPPED: u32 = 0x100;

// ---------------------------------------------------------------------------
// Netlink control message types
// ---------------------------------------------------------------------------

/// No-op.
pub const NLMSG_NOOP: u32 = 0x1;
/// Error/ACK.
pub const NLMSG_ERROR: u32 = 0x2;
/// End of multi-part dump.
pub const NLMSG_DONE: u32 = 0x3;
/// Overrun notification.
pub const NLMSG_OVERRUN: u32 = 0x4;
/// Minimum message type (user-defined starts here).
pub const NLMSG_MIN_TYPE: u32 = 0x10;

// ---------------------------------------------------------------------------
// Netlink socket diagnosis protocol info
// ---------------------------------------------------------------------------

/// NETLINK_SOCK_DIAG protocol.
pub const SOCK_DIAG_BY_FAMILY: u32 = 20;
/// Socket destroy request.
pub const SOCK_DESTROY: u32 = 21;

// ---------------------------------------------------------------------------
// Netlink alignment
// ---------------------------------------------------------------------------

/// Netlink message alignment.
pub const NLMSG_ALIGNTO: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extack_attrs_distinct() {
        let attrs = [
            NLMSGERR_ATTR_UNUSED,
            NLMSGERR_ATTR_MSG,
            NLMSGERR_ATTR_OFFS,
            NLMSGERR_ATTR_COOKIE,
            NLMSGERR_ATTR_POLICY,
            NLMSGERR_ATTR_MISS_TYPE,
            NLMSGERR_ATTR_MISS_NEST,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_ctrl_msg_types_distinct() {
        let types = [
            NLMSG_NOOP,
            NLMSG_ERROR,
            NLMSG_DONE,
            NLMSG_OVERRUN,
            NLMSG_MIN_TYPE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_msg_flags_no_overlap() {
        assert_eq!(NLM_F_ACK_TLVS & NLM_F_CAPPED, 0);
    }

    #[test]
    fn test_alignment() {
        assert!(NLMSG_ALIGNTO.is_power_of_two());
        assert_eq!(NLMSG_ALIGNTO, 4);
    }

    #[test]
    fn test_diag_distinct() {
        assert_ne!(SOCK_DIAG_BY_FAMILY, SOCK_DESTROY);
    }

    #[test]
    fn test_min_type() {
        assert!(NLMSG_MIN_TYPE > NLMSG_OVERRUN);
    }
}
