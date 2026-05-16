//! `<linux/vdpa.h>` — vDPA (virtio data path acceleration) constants.
//!
//! vDPA devices implement the virtio data path in hardware, allowing
//! virtio drivers to operate at near-native speed. Used by SmartNICs
//! and DPU offload for VM networking.

// ---------------------------------------------------------------------------
// vDPA netlink commands
// ---------------------------------------------------------------------------

/// Unspecified.
pub const VDPA_CMD_UNSPEC: u8 = 0;
/// Get management device info.
pub const VDPA_CMD_MGMTDEV_GET: u8 = 1;
/// Create vDPA device.
pub const VDPA_CMD_DEV_NEW: u8 = 2;
/// Delete vDPA device.
pub const VDPA_CMD_DEV_DEL: u8 = 3;
/// Get vDPA device info.
pub const VDPA_CMD_DEV_GET: u8 = 4;
/// Get device config.
pub const VDPA_CMD_DEV_CONFIG_GET: u8 = 5;
/// Get device vstats.
pub const VDPA_CMD_DEV_VSTATS_GET: u8 = 6;

// ---------------------------------------------------------------------------
// vDPA netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const VDPA_ATTR_UNSPEC: u16 = 0;
/// Management device bus name.
pub const VDPA_ATTR_MGMTDEV_BUS_NAME: u16 = 1;
/// Management device name.
pub const VDPA_ATTR_MGMTDEV_DEV_NAME: u16 = 2;
/// Supported classes.
pub const VDPA_ATTR_MGMTDEV_SUPPORTED_CLASSES: u16 = 3;
/// Device name.
pub const VDPA_ATTR_DEV_NAME: u16 = 4;
/// Device ID (virtio device ID).
pub const VDPA_ATTR_DEV_ID: u16 = 5;
/// Vendor ID.
pub const VDPA_ATTR_DEV_VENDOR_ID: u16 = 6;
/// Maximum VQ count.
pub const VDPA_ATTR_DEV_MAX_VQS: u16 = 7;
/// Maximum VQ size.
pub const VDPA_ATTR_DEV_MAX_VQ_SIZE: u16 = 8;
/// Negotiated features.
pub const VDPA_ATTR_DEV_NEGOTIATED_FEATURES: u16 = 9;
/// Net config MAC address.
pub const VDPA_ATTR_DEV_NET_CFG_MACADDR: u16 = 10;
/// Net config status.
pub const VDPA_ATTR_DEV_NET_STATUS: u16 = 11;
/// Net config MTU.
pub const VDPA_ATTR_DEV_NET_CFG_MTU: u16 = 12;
/// Net config max queue pairs.
pub const VDPA_ATTR_DEV_NET_CFG_MAX_VQP: u16 = 13;

// ---------------------------------------------------------------------------
// vDPA device features (management)
// ---------------------------------------------------------------------------

/// Maximum number of vDPA management devices.
pub const VDPA_MGMTDEV_ATTR_MAX: u16 = 20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            VDPA_CMD_UNSPEC, VDPA_CMD_MGMTDEV_GET,
            VDPA_CMD_DEV_NEW, VDPA_CMD_DEV_DEL,
            VDPA_CMD_DEV_GET, VDPA_CMD_DEV_CONFIG_GET,
            VDPA_CMD_DEV_VSTATS_GET,
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
            VDPA_ATTR_UNSPEC, VDPA_ATTR_MGMTDEV_BUS_NAME,
            VDPA_ATTR_MGMTDEV_DEV_NAME, VDPA_ATTR_MGMTDEV_SUPPORTED_CLASSES,
            VDPA_ATTR_DEV_NAME, VDPA_ATTR_DEV_ID,
            VDPA_ATTR_DEV_VENDOR_ID, VDPA_ATTR_DEV_MAX_VQS,
            VDPA_ATTR_DEV_MAX_VQ_SIZE, VDPA_ATTR_DEV_NEGOTIATED_FEATURES,
            VDPA_ATTR_DEV_NET_CFG_MACADDR, VDPA_ATTR_DEV_NET_STATUS,
            VDPA_ATTR_DEV_NET_CFG_MTU, VDPA_ATTR_DEV_NET_CFG_MAX_VQP,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_cmd_values() {
        assert_eq!(VDPA_CMD_UNSPEC, 0);
        assert_eq!(VDPA_CMD_DEV_NEW, 2);
    }
}
