//! `<linux/devlink.h>` — Devlink device management constants.
//!
//! Devlink is a kernel API for managing network device resources at
//! the device level (before network interfaces are created). Used by
//! iproute2 `devlink` command for firmware management, port splitting,
//! health reporting, resource management, and eswitch configuration.

// ---------------------------------------------------------------------------
// Devlink commands
// ---------------------------------------------------------------------------

/// Unspecified.
pub const DEVLINK_CMD_UNSPEC: u8 = 0;
/// Get devlink instance.
pub const DEVLINK_CMD_GET: u8 = 1;
/// Set devlink instance.
pub const DEVLINK_CMD_SET: u8 = 2;
/// New devlink instance.
pub const DEVLINK_CMD_NEW: u8 = 3;
/// Delete devlink instance.
pub const DEVLINK_CMD_DEL: u8 = 4;
/// Get port.
pub const DEVLINK_CMD_PORT_GET: u8 = 5;
/// Set port.
pub const DEVLINK_CMD_PORT_SET: u8 = 6;
/// New port.
pub const DEVLINK_CMD_PORT_NEW: u8 = 7;
/// Delete port.
pub const DEVLINK_CMD_PORT_DEL: u8 = 8;
/// Port split.
pub const DEVLINK_CMD_PORT_SPLIT: u8 = 9;
/// Port unsplit.
pub const DEVLINK_CMD_PORT_UNSPLIT: u8 = 10;
/// Get eswitch mode.
pub const DEVLINK_CMD_ESWITCH_GET: u8 = 29;
/// Set eswitch mode.
pub const DEVLINK_CMD_ESWITCH_SET: u8 = 30;
/// Get device parameters.
pub const DEVLINK_CMD_PARAM_GET: u8 = 38;
/// Set device parameters.
pub const DEVLINK_CMD_PARAM_SET: u8 = 39;
/// Get device info (fw version, etc.).
pub const DEVLINK_CMD_INFO_GET: u8 = 51;
/// Get health reporter.
pub const DEVLINK_CMD_HEALTH_REPORTER_GET: u8 = 52;
/// Set health reporter.
pub const DEVLINK_CMD_HEALTH_REPORTER_SET: u8 = 53;
/// Recover health.
pub const DEVLINK_CMD_HEALTH_REPORTER_RECOVER: u8 = 54;
/// Diagnose health.
pub const DEVLINK_CMD_HEALTH_REPORTER_DIAGNOSE: u8 = 55;
/// Flash update.
pub const DEVLINK_CMD_FLASH_UPDATE: u8 = 58;
/// Trap get.
pub const DEVLINK_CMD_TRAP_GET: u8 = 61;
/// Trap set.
pub const DEVLINK_CMD_TRAP_SET: u8 = 62;
/// Reload.
pub const DEVLINK_CMD_RELOAD: u8 = 37;

// ---------------------------------------------------------------------------
// Devlink attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const DEVLINK_ATTR_UNSPEC: u16 = 0;
/// Bus name.
pub const DEVLINK_ATTR_BUS_NAME: u16 = 1;
/// Device name.
pub const DEVLINK_ATTR_DEV_NAME: u16 = 2;
/// Port index.
pub const DEVLINK_ATTR_PORT_INDEX: u16 = 3;
/// Port type.
pub const DEVLINK_ATTR_PORT_TYPE: u16 = 4;
/// Desired port type.
pub const DEVLINK_ATTR_PORT_DESIRED_TYPE: u16 = 5;
/// Port netdev ifindex.
pub const DEVLINK_ATTR_PORT_NETDEV_IFINDEX: u16 = 6;
/// Port netdev name.
pub const DEVLINK_ATTR_PORT_NETDEV_NAME: u16 = 7;
/// Port split count.
pub const DEVLINK_ATTR_PORT_SPLIT_COUNT: u16 = 9;
/// Port split group.
pub const DEVLINK_ATTR_PORT_SPLIT_GROUP: u16 = 10;
/// eswitch mode.
pub const DEVLINK_ATTR_ESWITCH_MODE: u16 = 25;

// ---------------------------------------------------------------------------
// Port types
// ---------------------------------------------------------------------------

/// Not set.
pub const DEVLINK_PORT_TYPE_NOTSET: u16 = 0;
/// Auto.
pub const DEVLINK_PORT_TYPE_AUTO: u16 = 1;
/// Ethernet.
pub const DEVLINK_PORT_TYPE_ETH: u16 = 2;
/// InfiniBand.
pub const DEVLINK_PORT_TYPE_IB: u16 = 3;

// ---------------------------------------------------------------------------
// eswitch modes
// ---------------------------------------------------------------------------

/// Legacy mode.
pub const DEVLINK_ESWITCH_MODE_LEGACY: u16 = 0;
/// Switchdev mode.
pub const DEVLINK_ESWITCH_MODE_SWITCHDEV: u16 = 1;

// ---------------------------------------------------------------------------
// Reload actions
// ---------------------------------------------------------------------------

/// Unspecified reload.
pub const DEVLINK_RELOAD_ACTION_UNSPEC: u8 = 0;
/// Driver reinit.
pub const DEVLINK_RELOAD_ACTION_DRIVER_REINIT: u8 = 1;
/// Firmware activate.
pub const DEVLINK_RELOAD_ACTION_FW_ACTIVATE: u8 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            DEVLINK_CMD_UNSPEC,
            DEVLINK_CMD_GET,
            DEVLINK_CMD_SET,
            DEVLINK_CMD_NEW,
            DEVLINK_CMD_DEL,
            DEVLINK_CMD_PORT_GET,
            DEVLINK_CMD_PORT_SET,
            DEVLINK_CMD_PORT_NEW,
            DEVLINK_CMD_PORT_DEL,
            DEVLINK_CMD_PORT_SPLIT,
            DEVLINK_CMD_PORT_UNSPLIT,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_attrs_sequential() {
        assert_eq!(DEVLINK_ATTR_UNSPEC, 0);
        assert_eq!(DEVLINK_ATTR_BUS_NAME, 1);
        assert_eq!(DEVLINK_ATTR_DEV_NAME, 2);
        assert_eq!(DEVLINK_ATTR_PORT_INDEX, 3);
    }

    #[test]
    fn test_port_types() {
        assert_eq!(DEVLINK_PORT_TYPE_NOTSET, 0);
        assert_eq!(DEVLINK_PORT_TYPE_ETH, 2);
        assert_eq!(DEVLINK_PORT_TYPE_IB, 3);
    }

    #[test]
    fn test_eswitch_modes() {
        assert_eq!(DEVLINK_ESWITCH_MODE_LEGACY, 0);
        assert_eq!(DEVLINK_ESWITCH_MODE_SWITCHDEV, 1);
    }

    #[test]
    fn test_reload_actions() {
        assert_eq!(DEVLINK_RELOAD_ACTION_UNSPEC, 0);
        assert_eq!(DEVLINK_RELOAD_ACTION_DRIVER_REINIT, 1);
        assert_eq!(DEVLINK_RELOAD_ACTION_FW_ACTIVATE, 2);
    }

    #[test]
    fn test_health_cmds() {
        assert_eq!(DEVLINK_CMD_HEALTH_REPORTER_GET, 52);
        assert_eq!(DEVLINK_CMD_HEALTH_REPORTER_RECOVER, 54);
        assert_eq!(DEVLINK_CMD_HEALTH_REPORTER_DIAGNOSE, 55);
    }
}
