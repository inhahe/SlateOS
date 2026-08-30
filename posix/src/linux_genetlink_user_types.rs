//! `<linux/genetlink.h>` — generic-netlink dispatch ABI.
//!
//! Generic netlink is the framework on top of which nl80211, taskstats,
//! devlink, ethtool's new API, wireguard, and many other kernel
//! subsystems publish family-specific protocols. Userspace registers
//! families dynamically by name; the family ID is allocated by the
//! kernel and discovered via the CTRL family.

// ---------------------------------------------------------------------------
// Header & version
// ---------------------------------------------------------------------------

/// genlmsghdr size on the wire.
pub const GENL_HDRLEN: u32 = 4;
/// Initial admin-message wire version.
pub const GENL_ADMIN_PERM: u32 = 0x01;
/// CMD requires CAP_NET_ADMIN.
pub const GENL_CMD_CAP_DO: u32 = 0x02;
/// CMD supports DUMP.
pub const GENL_CMD_CAP_DUMP: u32 = 0x04;
/// CMD requires HASPOL policy validation.
pub const GENL_CMD_CAP_HASPOL: u32 = 0x08;

// ---------------------------------------------------------------------------
// Family-name limits
// ---------------------------------------------------------------------------

/// Max length of a family or op name.
pub const GENL_NAMSIZ: usize = 16;
/// Reserved family ID 0 — "no family".
pub const GENL_ID_GENERATE: u32 = 0;
/// Family ID range starts here.
pub const GENL_MIN_ID: u32 = 0x10;
/// Family ID range upper bound (16-bit family IDs).
pub const GENL_MAX_ID: u32 = 1023;

// ---------------------------------------------------------------------------
// Built-in CTRL family — used to discover others
// ---------------------------------------------------------------------------

/// Family ID of the always-present CTRL family.
pub const GENL_ID_CTRL: u32 = 0x10;
/// CTRL commands.
pub const CTRL_CMD_UNSPEC: u32 = 0;
/// `CTRL_CMD_NEWFAMILY`.
pub const CTRL_CMD_NEWFAMILY: u32 = 1;
/// `CTRL_CMD_DELFAMILY`.
pub const CTRL_CMD_DELFAMILY: u32 = 2;
/// `CTRL_CMD_GETFAMILY` — look family up by name.
pub const CTRL_CMD_GETFAMILY: u32 = 3;
/// `CTRL_CMD_NEWOPS`.
pub const CTRL_CMD_NEWOPS: u32 = 4;
/// `CTRL_CMD_DELOPS`.
pub const CTRL_CMD_DELOPS: u32 = 5;
/// `CTRL_CMD_GETOPS`.
pub const CTRL_CMD_GETOPS: u32 = 6;
/// `CTRL_CMD_NEWMCAST_GRP`.
pub const CTRL_CMD_NEWMCAST_GRP: u32 = 7;
/// `CTRL_CMD_DELMCAST_GRP`.
pub const CTRL_CMD_DELMCAST_GRP: u32 = 8;
/// `CTRL_CMD_GETMCAST_GRP`.
pub const CTRL_CMD_GETMCAST_GRP: u32 = 9;
/// `CTRL_CMD_GETPOLICY` — fetch family policy (nl80211, devlink).
pub const CTRL_CMD_GETPOLICY: u32 = 10;

// ---------------------------------------------------------------------------
// CTRL attributes (returned by GETFAMILY)
// ---------------------------------------------------------------------------

/// Sentinel.
pub const CTRL_ATTR_UNSPEC: u32 = 0;
/// Numeric family ID.
pub const CTRL_ATTR_FAMILY_ID: u32 = 1;
/// Family name string.
pub const CTRL_ATTR_FAMILY_NAME: u32 = 2;
/// Family wire version.
pub const CTRL_ATTR_VERSION: u32 = 3;
/// Header size used by family.
pub const CTRL_ATTR_HDRSIZE: u32 = 4;
/// Maximum supported attribute.
pub const CTRL_ATTR_MAXATTR: u32 = 5;
/// List of operations.
pub const CTRL_ATTR_OPS: u32 = 6;
/// List of multicast groups.
pub const CTRL_ATTR_MCAST_GROUPS: u32 = 7;
/// Policy (extended netlink).
pub const CTRL_ATTR_POLICY: u32 = 8;
/// Operation policy.
pub const CTRL_ATTR_OP_POLICY: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_and_namsiz() {
        // genlmsghdr is 4 bytes: cmd:u8 + version:u8 + reserved:u16.
        assert_eq!(GENL_HDRLEN, 4);
        // Family/op names are NUL-terminated 16-byte buffers.
        assert_eq!(GENL_NAMSIZ, 16);
    }

    #[test]
    fn test_id_ranges() {
        assert_eq!(GENL_ID_GENERATE, 0);
        // CTRL family lives at the start of the dynamic range.
        assert_eq!(GENL_ID_CTRL, GENL_MIN_ID);
        assert!(GENL_MIN_ID < GENL_MAX_ID);
    }

    #[test]
    fn test_command_capability_bits_pow2() {
        let c = [
            GENL_ADMIN_PERM,
            GENL_CMD_CAP_DO,
            GENL_CMD_CAP_DUMP,
            GENL_CMD_CAP_HASPOL,
        ];
        for &b in &c {
            assert!(b.is_power_of_two());
        }
    }

    #[test]
    fn test_ctrl_commands_dense() {
        let c = [
            CTRL_CMD_UNSPEC,
            CTRL_CMD_NEWFAMILY,
            CTRL_CMD_DELFAMILY,
            CTRL_CMD_GETFAMILY,
            CTRL_CMD_NEWOPS,
            CTRL_CMD_DELOPS,
            CTRL_CMD_GETOPS,
            CTRL_CMD_NEWMCAST_GRP,
            CTRL_CMD_DELMCAST_GRP,
            CTRL_CMD_GETMCAST_GRP,
            CTRL_CMD_GETPOLICY,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_ctrl_attributes_dense() {
        let a = [
            CTRL_ATTR_UNSPEC,
            CTRL_ATTR_FAMILY_ID,
            CTRL_ATTR_FAMILY_NAME,
            CTRL_ATTR_VERSION,
            CTRL_ATTR_HDRSIZE,
            CTRL_ATTR_MAXATTR,
            CTRL_ATTR_OPS,
            CTRL_ATTR_MCAST_GROUPS,
            CTRL_ATTR_POLICY,
            CTRL_ATTR_OP_POLICY,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }
}
