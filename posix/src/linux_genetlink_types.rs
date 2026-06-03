//! `<linux/genetlink.h>` — Generic Netlink (genetlink) constants.
//!
//! Generic Netlink is a multiplexing layer on top of Netlink that
//! allows kernel subsystems to register named families (e.g., "nl80211",
//! "devlink", "ovs_datapath") without needing a dedicated netlink
//! protocol number. Userspace discovers family IDs via the controller
//! family. All modern netlink-based kernel APIs use genetlink. The
//! controller provides family registration, multicast group management,
//! and operation dispatch.

// ---------------------------------------------------------------------------
// Genetlink header constants
// ---------------------------------------------------------------------------

/// Generic Netlink header version.
pub const GENL_HDRLEN: u32 = 4;
/// Maximum family name length.
pub const GENL_NAMSIZ: u32 = 16;

// ---------------------------------------------------------------------------
// Fixed family IDs
// ---------------------------------------------------------------------------

/// Minimum dynamic family ID.
pub const GENL_MIN_ID: u32 = 0x10;
/// Maximum family ID.
pub const GENL_MAX_ID: u32 = 1023;
/// Controller family ID (always available).
pub const GENL_ID_CTRL: u32 = 0x10;
/// VFS dquot family ID.
pub const GENL_ID_VFS_DQUOT: u32 = 0x11;
/// PMCRAID family ID.
pub const GENL_ID_PMCRAID: u32 = 0x12;

// ---------------------------------------------------------------------------
// Controller commands (CTRL_CMD_*)
// ---------------------------------------------------------------------------

/// New family registered.
pub const CTRL_CMD_NEWFAMILY: u32 = 1;
/// Family unregistered.
pub const CTRL_CMD_DELFAMILY: u32 = 2;
/// Get family info (resolve name → ID).
pub const CTRL_CMD_GETFAMILY: u32 = 3;
/// New multicast group.
pub const CTRL_CMD_NEWMCAST_GRP: u32 = 7;
/// Multicast group removed.
pub const CTRL_CMD_DELMCAST_GRP: u32 = 8;
/// Get multicast group.
pub const CTRL_CMD_GETMCAST_GRP: u32 = 9;
/// Get policy (operation/attribute specs).
pub const CTRL_CMD_GETPOLICY: u32 = 10;

// ---------------------------------------------------------------------------
// Controller attributes (CTRL_ATTR_*)
// ---------------------------------------------------------------------------

/// Family ID.
pub const CTRL_ATTR_FAMILY_ID: u32 = 1;
/// Family name string.
pub const CTRL_ATTR_FAMILY_NAME: u32 = 2;
/// Family version.
pub const CTRL_ATTR_VERSION: u32 = 3;
/// Family header size.
pub const CTRL_ATTR_HDRSIZE: u32 = 4;
/// Maximum attribute number.
pub const CTRL_ATTR_MAXATTR: u32 = 5;
/// Operations list.
pub const CTRL_ATTR_OPS: u32 = 6;
/// Multicast groups list.
pub const CTRL_ATTR_MCAST_GROUPS: u32 = 7;
/// Policy (attribute specifications).
pub const CTRL_ATTR_POLICY: u32 = 8;
/// Operation policy.
pub const CTRL_ATTR_OP_POLICY: u32 = 9;
/// Operation attribute.
pub const CTRL_ATTR_OP: u32 = 10;

// ---------------------------------------------------------------------------
// Operation attributes (CTRL_ATTR_OP_*)
// ---------------------------------------------------------------------------

/// Operation ID.
pub const CTRL_ATTR_OP_ID: u32 = 1;
/// Operation flags.
pub const CTRL_ATTR_OP_FLAGS: u32 = 2;

// ---------------------------------------------------------------------------
// Operation flags
// ---------------------------------------------------------------------------

/// Admin privileges required.
pub const GENL_ADMIN_PERM: u32 = 1 << 0;
/// Command has a doit handler.
pub const GENL_CMD_CAP_DO: u32 = 1 << 1;
/// Command has a dumpit handler.
pub const GENL_CMD_CAP_DUMP: u32 = 1 << 2;
/// Command has policy.
pub const GENL_CMD_CAP_HASPOL: u32 = 1 << 3;
/// Unsigned strict attribute checking.
pub const GENL_UNS_ADMIN_PERM: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctrl_commands_distinct() {
        let cmds = [
            CTRL_CMD_NEWFAMILY,
            CTRL_CMD_DELFAMILY,
            CTRL_CMD_GETFAMILY,
            CTRL_CMD_NEWMCAST_GRP,
            CTRL_CMD_DELMCAST_GRP,
            CTRL_CMD_GETMCAST_GRP,
            CTRL_CMD_GETPOLICY,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_ctrl_attrs_distinct() {
        let attrs = [
            CTRL_ATTR_FAMILY_ID,
            CTRL_ATTR_FAMILY_NAME,
            CTRL_ATTR_VERSION,
            CTRL_ATTR_HDRSIZE,
            CTRL_ATTR_MAXATTR,
            CTRL_ATTR_OPS,
            CTRL_ATTR_MCAST_GROUPS,
            CTRL_ATTR_POLICY,
            CTRL_ATTR_OP_POLICY,
            CTRL_ATTR_OP,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_genl_id_range() {
        assert!(GENL_MIN_ID <= GENL_MAX_ID);
        assert_eq!(GENL_ID_CTRL, GENL_MIN_ID);
    }

    #[test]
    fn test_fixed_ids_distinct() {
        let ids = [GENL_ID_CTRL, GENL_ID_VFS_DQUOT, GENL_ID_PMCRAID];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_namsiz() {
        assert_eq!(GENL_NAMSIZ, 16);
    }
}
