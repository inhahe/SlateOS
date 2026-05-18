//! `<linux/netfilter/nfnetlink_log.h>` — NFLOG constants.
//!
//! NFLOG provides packet logging from netfilter to userspace
//! via netlink.  These constants define message types, copy
//! modes, and attribute types.

// ---------------------------------------------------------------------------
// NFLOG commands
// ---------------------------------------------------------------------------

/// Bind to a log group.
pub const NFULNL_MSG_PACKET: u32 = 0;
/// Configuration message.
pub const NFULNL_MSG_CONFIG: u32 = 1;

// ---------------------------------------------------------------------------
// NFLOG config commands
// ---------------------------------------------------------------------------

/// Bind to subsystem.
pub const NFULNL_CFG_CMD_BIND: u32 = 1;
/// Unbind from subsystem.
pub const NFULNL_CFG_CMD_UNBIND: u32 = 2;
/// Bind to protocol family.
pub const NFULNL_CFG_CMD_PF_BIND: u32 = 3;
/// Unbind from protocol family.
pub const NFULNL_CFG_CMD_PF_UNBIND: u32 = 4;

// ---------------------------------------------------------------------------
// NFLOG copy modes
// ---------------------------------------------------------------------------

/// Don't copy any payload.
pub const NFULNL_COPY_NONE: u32 = 0;
/// Copy packet metadata only.
pub const NFULNL_COPY_META: u32 = 1;
/// Copy packet payload (up to copy_range bytes).
pub const NFULNL_COPY_PACKET: u32 = 2;

// ---------------------------------------------------------------------------
// NFLOG packet attributes (nla types)
// ---------------------------------------------------------------------------

/// Packet header (mark, timestamp, etc.).
pub const NFULA_PACKET_HDR: u32 = 1;
/// Packet mark.
pub const NFULA_MARK: u32 = 2;
/// Timestamp.
pub const NFULA_TIMESTAMP: u32 = 3;
/// Input interface index.
pub const NFULA_IFINDEX_INDEV: u32 = 4;
/// Output interface index.
pub const NFULA_IFINDEX_OUTDEV: u32 = 5;
/// Physical input interface.
pub const NFULA_IFINDEX_PHYSINDEV: u32 = 6;
/// Physical output interface.
pub const NFULA_IFINDEX_PHYSOUTDEV: u32 = 7;
/// Hardware address.
pub const NFULA_HWADDR: u32 = 8;
/// Packet payload.
pub const NFULA_PAYLOAD: u32 = 9;
/// Prefix string.
pub const NFULA_PREFIX: u32 = 10;
/// UID of socket owner.
pub const NFULA_UID: u32 = 11;
/// Sequence number (local).
pub const NFULA_SEQ: u32 = 12;
/// Sequence number (global).
pub const NFULA_SEQ_GLOBAL: u32 = 13;
/// GID of socket owner.
pub const NFULA_GID: u32 = 14;
/// Hardware type.
pub const NFULA_HWTYPE: u32 = 15;
/// Hardware header.
pub const NFULA_HWHEADER: u32 = 16;
/// Hardware header length.
pub const NFULA_HWLEN: u32 = 17;
/// Connection tracking info.
pub const NFULA_CT: u32 = 18;
/// Connection tracking info (extra).
pub const NFULA_CT_INFO: u32 = 19;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_distinct() {
        assert_ne!(NFULNL_MSG_PACKET, NFULNL_MSG_CONFIG);
    }

    #[test]
    fn test_config_cmds_distinct() {
        let cmds = [
            NFULNL_CFG_CMD_BIND, NFULNL_CFG_CMD_UNBIND,
            NFULNL_CFG_CMD_PF_BIND, NFULNL_CFG_CMD_PF_UNBIND,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_copy_modes_distinct() {
        let modes = [NFULNL_COPY_NONE, NFULNL_COPY_META, NFULNL_COPY_PACKET];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_none_is_zero() {
        assert_eq!(NFULNL_COPY_NONE, 0);
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            NFULA_PACKET_HDR, NFULA_MARK, NFULA_TIMESTAMP,
            NFULA_IFINDEX_INDEV, NFULA_IFINDEX_OUTDEV,
            NFULA_PAYLOAD, NFULA_PREFIX, NFULA_UID,
            NFULA_SEQ, NFULA_SEQ_GLOBAL, NFULA_GID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
