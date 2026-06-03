//! `<linux/wwan.h>` — wireless WAN (LTE/5G modem) framework.
//!
//! The WWAN subsystem exposes a unified character-device interface
//! (`/dev/wwan*`) for cellular modems. Each modem device gets ports
//! for AT commands, MBIM, QMI, FIRMWARE, and XMMRPC traffic, all
//! demultiplexed by a single driver core.

// ---------------------------------------------------------------------------
// Device-node prefix
// ---------------------------------------------------------------------------

pub const DEV_WWAN_PREFIX: &str = "/dev/wwan";

// ---------------------------------------------------------------------------
// `enum wwan_port_type` — the kind of channel each `/dev/wwan*` is
// ---------------------------------------------------------------------------

pub const WWAN_PORT_UNKNOWN: u32 = 0;
pub const WWAN_PORT_AT: u32 = 1;
pub const WWAN_PORT_MBIM: u32 = 2;
pub const WWAN_PORT_QMI: u32 = 3;
pub const WWAN_PORT_QCDM: u32 = 4;
pub const WWAN_PORT_FIRMWARE: u32 = 5;
pub const WWAN_PORT_XMMRPC: u32 = 6;
pub const WWAN_PORT_MAX: u32 = WWAN_PORT_XMMRPC;

// ---------------------------------------------------------------------------
// netlink interface
// ---------------------------------------------------------------------------

pub const WWAN_GENL_NAME: &str = "wwan";

// ---------------------------------------------------------------------------
// Generic-netlink commands
// ---------------------------------------------------------------------------

pub const WWAN_CMD_UNSPEC: u8 = 0;
pub const WWAN_CMD_GET_LINK: u8 = 1;
pub const WWAN_CMD_NEW_LINK: u8 = 2;
pub const WWAN_CMD_DEL_LINK: u8 = 3;
pub const WWAN_CMD_MAX: u8 = WWAN_CMD_DEL_LINK;

// ---------------------------------------------------------------------------
// Netlink attributes for WWAN links
// ---------------------------------------------------------------------------

pub const WWAN_ATTR_UNSPEC: u16 = 0;
pub const WWAN_ATTR_LINK_ID: u16 = 1;
pub const WWAN_ATTR_LINK_NAME: u16 = 2;
pub const WWAN_ATTR_LINK_PARENT_IFINDEX: u16 = 3;
pub const WWAN_ATTR_LINK_FLAGS: u16 = 4;
pub const WWAN_ATTR_MAX: u16 = WWAN_ATTR_LINK_FLAGS;

// ---------------------------------------------------------------------------
// Common MBIM message size
// ---------------------------------------------------------------------------

/// MBIM transactions are framed in 1280-byte chunks by default (RFC 6353).
pub const MBIM_DEFAULT_MSG_SIZE: usize = 1280;
/// Maximum size negotiated at session open.
pub const MBIM_MAX_MSG_SIZE: usize = 65_536;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_prefix() {
        assert_eq!(DEV_WWAN_PREFIX, "/dev/wwan");
    }

    #[test]
    fn test_port_types_dense_0_to_6() {
        let p = [
            WWAN_PORT_UNKNOWN,
            WWAN_PORT_AT,
            WWAN_PORT_MBIM,
            WWAN_PORT_QMI,
            WWAN_PORT_QCDM,
            WWAN_PORT_FIRMWARE,
            WWAN_PORT_XMMRPC,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(WWAN_PORT_MAX, WWAN_PORT_XMMRPC);
    }

    #[test]
    fn test_genl_name() {
        assert_eq!(WWAN_GENL_NAME, "wwan");
    }

    #[test]
    fn test_cmd_dense_0_to_3() {
        assert_eq!(WWAN_CMD_UNSPEC, 0);
        assert_eq!(WWAN_CMD_GET_LINK, 1);
        assert_eq!(WWAN_CMD_NEW_LINK, 2);
        assert_eq!(WWAN_CMD_DEL_LINK, 3);
        assert_eq!(WWAN_CMD_MAX, WWAN_CMD_DEL_LINK);
    }

    #[test]
    fn test_attrs_dense_0_to_4() {
        let a = [
            WWAN_ATTR_UNSPEC,
            WWAN_ATTR_LINK_ID,
            WWAN_ATTR_LINK_NAME,
            WWAN_ATTR_LINK_PARENT_IFINDEX,
            WWAN_ATTR_LINK_FLAGS,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(WWAN_ATTR_MAX, WWAN_ATTR_LINK_FLAGS);
    }

    #[test]
    fn test_mbim_sizes() {
        assert_eq!(MBIM_DEFAULT_MSG_SIZE, 1280);
        // Default fits inside the absolute max.
        assert!(MBIM_DEFAULT_MSG_SIZE < MBIM_MAX_MSG_SIZE);
        // Max is a power of two.
        assert!(MBIM_MAX_MSG_SIZE.is_power_of_two());
    }
}
