//! `<linux/genetlink.h>` — Generic Netlink protocol.
//!
//! Generic Netlink extends the standard Netlink protocol with a
//! multiplexing layer, allowing many subsystems to share a single
//! Netlink socket family. Used by nl80211 (WiFi), taskstats,
//! devlink, and many other kernel subsystems.

pub use crate::linux_netlink::Nlmsghdr;
pub use crate::linux_netlink::NLM_F_REQUEST;
pub use crate::linux_netlink::NLM_F_DUMP;
pub use crate::linux_netlink::NETLINK_GENERIC;

// ---------------------------------------------------------------------------
// Generic Netlink header
// ---------------------------------------------------------------------------

/// Generic Netlink message header (4 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Genlmsghdr {
    /// Command identifier.
    pub cmd: u8,
    /// Version.
    pub version: u8,
    /// Reserved.
    pub reserved: u16,
}

impl Genlmsghdr {
    /// Create a zeroed generic netlink header.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Generic Netlink family constants
// ---------------------------------------------------------------------------

/// Generic Netlink family ID: controller.
pub const GENL_ID_CTRL: u16 = 0x10;

/// Minimum family ID for dynamic allocation.
pub const GENL_MIN_ID: u16 = 0x10;
/// Maximum family ID.
pub const GENL_MAX_ID: u16 = 1023;

/// Generic Netlink header length.
pub const GENL_HDRLEN: usize = 4;

/// Generic Netlink version.
pub const GENL_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Controller commands (CTRL_CMD_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const CTRL_CMD_UNSPEC: u8 = 0;
/// New family notification.
pub const CTRL_CMD_NEWFAMILY: u8 = 1;
/// Delete family notification.
pub const CTRL_CMD_DELFAMILY: u8 = 2;
/// Get family info.
pub const CTRL_CMD_GETFAMILY: u8 = 3;
/// New multicast group.
pub const CTRL_CMD_NEWMCAST_GRP: u8 = 7;
/// Delete multicast group.
pub const CTRL_CMD_DELMCAST_GRP: u8 = 8;
/// Get multicast group.
pub const CTRL_CMD_GETMCAST_GRP: u8 = 9;
/// Get policy.
pub const CTRL_CMD_GETPOLICY: u8 = 10;

// ---------------------------------------------------------------------------
// Controller attributes (CTRL_ATTR_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const CTRL_ATTR_UNSPEC: u16 = 0;
/// Family ID.
pub const CTRL_ATTR_FAMILY_ID: u16 = 1;
/// Family name.
pub const CTRL_ATTR_FAMILY_NAME: u16 = 2;
/// Version.
pub const CTRL_ATTR_VERSION: u16 = 3;
/// Header size.
pub const CTRL_ATTR_HDRSIZE: u16 = 4;
/// Maximum attributes.
pub const CTRL_ATTR_MAXATTR: u16 = 5;
/// Operations (nested).
pub const CTRL_ATTR_OPS: u16 = 6;
/// Multicast groups (nested).
pub const CTRL_ATTR_MCAST_GROUPS: u16 = 7;
/// Policy.
pub const CTRL_ATTR_POLICY: u16 = 8;
/// Operations policy.
pub const CTRL_ATTR_OP_POLICY: u16 = 9;
/// Operations (dump).
pub const CTRL_ATTR_OP: u16 = 10;

// ---------------------------------------------------------------------------
// Operation attributes (CTRL_ATTR_OP_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const CTRL_ATTR_OP_UNSPEC: u16 = 0;
/// Operation ID.
pub const CTRL_ATTR_OP_ID: u16 = 1;
/// Operation flags.
pub const CTRL_ATTR_OP_FLAGS: u16 = 2;

// ---------------------------------------------------------------------------
// Multicast group attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const CTRL_ATTR_MCAST_GRP_UNSPEC: u16 = 0;
/// Group name.
pub const CTRL_ATTR_MCAST_GRP_NAME: u16 = 1;
/// Group ID.
pub const CTRL_ATTR_MCAST_GRP_ID: u16 = 2;

// ---------------------------------------------------------------------------
// Generic Netlink controller family name
// ---------------------------------------------------------------------------

/// Controller family name string.
pub const GENL_CTRL_NAME: &str = "nlctrl";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genlmsghdr_size() {
        assert_eq!(core::mem::size_of::<Genlmsghdr>(), 4);
        assert_eq!(GENL_HDRLEN, 4);
    }

    #[test]
    fn test_genl_id_ctrl() {
        assert_eq!(GENL_ID_CTRL, 0x10);
        assert_eq!(GENL_MIN_ID, GENL_ID_CTRL);
    }

    #[test]
    fn test_ctrl_cmds_sequential() {
        assert_eq!(CTRL_CMD_UNSPEC, 0);
        assert_eq!(CTRL_CMD_NEWFAMILY, 1);
        assert_eq!(CTRL_CMD_DELFAMILY, 2);
        assert_eq!(CTRL_CMD_GETFAMILY, 3);
    }

    #[test]
    fn test_ctrl_attrs_sequential() {
        assert_eq!(CTRL_ATTR_UNSPEC, 0);
        assert_eq!(CTRL_ATTR_FAMILY_ID, 1);
        assert_eq!(CTRL_ATTR_FAMILY_NAME, 2);
        assert_eq!(CTRL_ATTR_VERSION, 3);
        assert_eq!(CTRL_ATTR_MCAST_GROUPS, 7);
    }

    #[test]
    fn test_ctrl_attrs_distinct() {
        let attrs = [
            CTRL_ATTR_UNSPEC, CTRL_ATTR_FAMILY_ID, CTRL_ATTR_FAMILY_NAME,
            CTRL_ATTR_VERSION, CTRL_ATTR_HDRSIZE, CTRL_ATTR_MAXATTR,
            CTRL_ATTR_OPS, CTRL_ATTR_MCAST_GROUPS, CTRL_ATTR_POLICY,
            CTRL_ATTR_OP_POLICY, CTRL_ATTR_OP,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_mcast_grp_attrs() {
        assert_eq!(CTRL_ATTR_MCAST_GRP_UNSPEC, 0);
        assert_eq!(CTRL_ATTR_MCAST_GRP_NAME, 1);
        assert_eq!(CTRL_ATTR_MCAST_GRP_ID, 2);
    }

    #[test]
    fn test_genl_ctrl_name() {
        assert_eq!(GENL_CTRL_NAME, "nlctrl");
    }
}
