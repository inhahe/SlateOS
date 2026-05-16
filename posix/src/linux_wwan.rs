//! `<linux/wwan.h>` — Wireless Wide Area Network constants.
//!
//! The WWAN subsystem manages mobile broadband modems (LTE, 5G).
//! It provides a Generic Netlink interface for modem control and
//! data port management.

// ---------------------------------------------------------------------------
// WWAN port types
// ---------------------------------------------------------------------------

/// AT command port.
pub const WWAN_PORT_AT: u32 = 0;
/// MBIM (Mobile Broadband Interface Model) port.
pub const WWAN_PORT_MBIM: u32 = 1;
/// QMI (Qualcomm MSM Interface) port.
pub const WWAN_PORT_QMI: u32 = 2;
/// QCDM (Qualcomm diagnostic) port.
pub const WWAN_PORT_QCDM: u32 = 3;
/// FIREHOSE (Qualcomm EDL) port.
pub const WWAN_PORT_FIREHOSE: u32 = 4;
/// XMMRPC port.
pub const WWAN_PORT_XMMRPC: u32 = 5;

/// Maximum port type.
pub const WWAN_PORT_MAX: u32 = 6;

// ---------------------------------------------------------------------------
// WWAN netlink commands
// ---------------------------------------------------------------------------

/// Unspecified.
pub const WWAN_CMD_UNSPEC: u8 = 0;
/// Get device.
pub const WWAN_CMD_GET_DEVICE: u8 = 1;
/// New device.
pub const WWAN_CMD_NEW_DEVICE: u8 = 2;
/// Delete device.
pub const WWAN_CMD_DEL_DEVICE: u8 = 3;
/// Get debug data.
pub const WWAN_CMD_GET_DEBUG: u8 = 4;

// ---------------------------------------------------------------------------
// WWAN netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const WWAN_ATTR_UNSPEC: u16 = 0;
/// Device index.
pub const WWAN_ATTR_DEV_INDEX: u16 = 1;
/// Device name.
pub const WWAN_ATTR_DEV_NAME: u16 = 2;
/// Link index.
pub const WWAN_ATTR_LINK_INDEX: u16 = 3;

// ---------------------------------------------------------------------------
// Modem states
// ---------------------------------------------------------------------------

/// Modem is offline.
pub const WWAN_MODEM_STATE_OFFLINE: u32 = 0;
/// Modem is online.
pub const WWAN_MODEM_STATE_ONLINE: u32 = 1;
/// Modem is in low power mode.
pub const WWAN_MODEM_STATE_LOW_POWER: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_types_distinct() {
        let ports = [
            WWAN_PORT_AT, WWAN_PORT_MBIM, WWAN_PORT_QMI,
            WWAN_PORT_QCDM, WWAN_PORT_FIREHOSE, WWAN_PORT_XMMRPC,
        ];
        for i in 0..ports.len() {
            for j in (i + 1)..ports.len() {
                assert_ne!(ports[i], ports[j]);
            }
        }
    }

    #[test]
    fn test_port_max() {
        assert_eq!(WWAN_PORT_MAX, WWAN_PORT_XMMRPC + 1);
    }

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            WWAN_CMD_UNSPEC, WWAN_CMD_GET_DEVICE,
            WWAN_CMD_NEW_DEVICE, WWAN_CMD_DEL_DEVICE,
            WWAN_CMD_GET_DEBUG,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            WWAN_ATTR_UNSPEC, WWAN_ATTR_DEV_INDEX,
            WWAN_ATTR_DEV_NAME, WWAN_ATTR_LINK_INDEX,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_modem_states_distinct() {
        let states = [
            WWAN_MODEM_STATE_OFFLINE, WWAN_MODEM_STATE_ONLINE,
            WWAN_MODEM_STATE_LOW_POWER,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
