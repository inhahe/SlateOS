//! `<linux/genetlink.h>` — Generic netlink constants.
//!
//! Generic netlink is a multiplexing layer on top of netlink that
//! allows multiple kernel subsystems to share `NETLINK_GENERIC`
//! without allocating dedicated protocol numbers. Families register
//! and receive dynamically-assigned IDs.

// ---------------------------------------------------------------------------
// Generic netlink header constants
// ---------------------------------------------------------------------------

/// Generic netlink header version.
pub const GENL_HDRLEN: u32 = 4;
/// Family name maximum length.
pub const GENL_NAMSIZ: u32 = 16;

// ---------------------------------------------------------------------------
// Generic netlink message types (commands)
// ---------------------------------------------------------------------------

/// No operation.
pub const GENL_ID_CTRL: u16 = 0x10;
/// VFS dquot (not commonly used).
pub const GENL_ID_VFS_DQUOT: u16 = 0x11;
/// PMC (power management) events.
pub const GENL_ID_PMCRAID: u16 = 0x12;

// ---------------------------------------------------------------------------
// Generic netlink controller commands (CTRL_CMD_*)
// ---------------------------------------------------------------------------

/// Unspecified command.
pub const CTRL_CMD_UNSPEC: u8 = 0;
/// New family registered.
pub const CTRL_CMD_NEWFAMILY: u8 = 1;
/// Family deleted.
pub const CTRL_CMD_DELFAMILY: u8 = 2;
/// Get family info.
pub const CTRL_CMD_GETFAMILY: u8 = 3;
/// New multicast group.
pub const CTRL_CMD_NEWMCAST_GRP: u8 = 7;
/// Delete multicast group.
pub const CTRL_CMD_DELMCAST_GRP: u8 = 8;
/// Get multicast group.
pub const CTRL_CMD_GETMCAST_GRP: u8 = 9;
/// Get policy dump.
pub const CTRL_CMD_GETPOLICY: u8 = 10;

// ---------------------------------------------------------------------------
// Generic netlink controller attributes (CTRL_ATTR_*)
// ---------------------------------------------------------------------------

/// Unspecified attribute.
pub const CTRL_ATTR_UNSPEC: u16 = 0;
/// Family ID.
pub const CTRL_ATTR_FAMILY_ID: u16 = 1;
/// Family name.
pub const CTRL_ATTR_FAMILY_NAME: u16 = 2;
/// Version number.
pub const CTRL_ATTR_VERSION: u16 = 3;
/// Header size.
pub const CTRL_ATTR_HDRSIZE: u16 = 4;
/// Maximum attribute number.
pub const CTRL_ATTR_MAXATTR: u16 = 5;
/// Operations list (nested).
pub const CTRL_ATTR_OPS: u16 = 6;
/// Multicast groups (nested).
pub const CTRL_ATTR_MCAST_GROUPS: u16 = 7;
/// Policy (nested).
pub const CTRL_ATTR_POLICY: u16 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genl_constants() {
        assert_eq!(GENL_HDRLEN, 4);
        assert_eq!(GENL_NAMSIZ, 16);
    }

    #[test]
    fn test_ctrl_commands_distinct() {
        let cmds = [
            CTRL_CMD_UNSPEC,
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
            CTRL_ATTR_UNSPEC,
            CTRL_ATTR_FAMILY_ID,
            CTRL_ATTR_FAMILY_NAME,
            CTRL_ATTR_VERSION,
            CTRL_ATTR_HDRSIZE,
            CTRL_ATTR_MAXATTR,
            CTRL_ATTR_OPS,
            CTRL_ATTR_MCAST_GROUPS,
            CTRL_ATTR_POLICY,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_ctrl_id() {
        assert_eq!(GENL_ID_CTRL, 0x10);
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(CTRL_CMD_UNSPEC, 0);
        assert_eq!(CTRL_ATTR_UNSPEC, 0);
    }
}
