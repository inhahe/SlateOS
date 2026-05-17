//! `<linux/vdpa.h>` — vDPA (virtio Data Path Acceleration) constants.
//!
//! vDPA allows hardware devices to directly implement the VirtIO
//! data path while using a software control path. This gives near-
//! native performance for VM I/O while maintaining the VirtIO
//! ecosystem's portability. A vDPA device exposes VirtIO virtqueues
//! backed by hardware (SmartNIC, FPGA) instead of software
//! emulation. Configured via netlink. Used by Mellanox ConnectX-6,
//! Intel infrastructure IPUs, and other SmartNICs.

// ---------------------------------------------------------------------------
// vDPA netlink commands
// ---------------------------------------------------------------------------

/// Get vDPA device info.
pub const VDPA_CMD_DEV_GET: u32 = 1;
/// Create a new vDPA device.
pub const VDPA_CMD_DEV_NEW: u32 = 2;
/// Delete a vDPA device.
pub const VDPA_CMD_DEV_DEL: u32 = 3;
/// Get management device info.
pub const VDPA_CMD_MGMTDEV_GET: u32 = 4;
/// Get device config.
pub const VDPA_CMD_DEV_CONFIG_GET: u32 = 5;
/// Get vendor statistics.
pub const VDPA_CMD_DEV_VSTATS_GET: u32 = 6;

// ---------------------------------------------------------------------------
// vDPA netlink attributes
// ---------------------------------------------------------------------------

/// Bus name.
pub const VDPA_ATTR_BUS_NAME: u32 = 1;
/// Device name.
pub const VDPA_ATTR_DEV_NAME: u32 = 2;
/// Management device bus name.
pub const VDPA_ATTR_MGMTDEV_BUS_NAME: u32 = 3;
/// Management device name.
pub const VDPA_ATTR_MGMTDEV_DEV_NAME: u32 = 4;
/// Supported VirtIO features.
pub const VDPA_ATTR_DEV_SUPPORTED_FEATURES: u32 = 5;
/// Negotiated features.
pub const VDPA_ATTR_DEV_NEGOTIATED_FEATURES: u32 = 6;
/// Maximum virtqueue count.
pub const VDPA_ATTR_DEV_MAX_VQS: u32 = 7;
/// Maximum VQ size.
pub const VDPA_ATTR_DEV_MAX_VQ_SIZE: u32 = 8;
/// VirtIO device ID.
pub const VDPA_ATTR_DEV_ID: u32 = 9;
/// Minimum VQ size.
pub const VDPA_ATTR_DEV_MIN_VQ_SIZE: u32 = 10;
/// Net config (MAC, MTU, etc.).
pub const VDPA_ATTR_DEV_NET_CFG_MACADDR: u32 = 11;
/// Net status.
pub const VDPA_ATTR_DEV_NET_STATUS: u32 = 12;
/// Net MTU.
pub const VDPA_ATTR_DEV_NET_CFG_MTU: u32 = 13;
/// Max VQ pairs (multi-queue).
pub const VDPA_ATTR_DEV_NET_CFG_MAX_VQP: u32 = 14;
/// Device queue index.
pub const VDPA_ATTR_DEV_QUEUE_INDEX: u32 = 15;

// ---------------------------------------------------------------------------
// vDPA management device supported classes
// ---------------------------------------------------------------------------

/// Network device class.
pub const VDPA_MGMTDEV_CLASS_NET: u32 = 1;
/// Block device class.
pub const VDPA_MGMTDEV_CLASS_BLOCK: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            VDPA_CMD_DEV_GET, VDPA_CMD_DEV_NEW, VDPA_CMD_DEV_DEL,
            VDPA_CMD_MGMTDEV_GET, VDPA_CMD_DEV_CONFIG_GET,
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
            VDPA_ATTR_BUS_NAME, VDPA_ATTR_DEV_NAME,
            VDPA_ATTR_MGMTDEV_BUS_NAME, VDPA_ATTR_MGMTDEV_DEV_NAME,
            VDPA_ATTR_DEV_SUPPORTED_FEATURES,
            VDPA_ATTR_DEV_NEGOTIATED_FEATURES,
            VDPA_ATTR_DEV_MAX_VQS, VDPA_ATTR_DEV_MAX_VQ_SIZE,
            VDPA_ATTR_DEV_ID, VDPA_ATTR_DEV_MIN_VQ_SIZE,
            VDPA_ATTR_DEV_NET_CFG_MACADDR, VDPA_ATTR_DEV_NET_STATUS,
            VDPA_ATTR_DEV_NET_CFG_MTU, VDPA_ATTR_DEV_NET_CFG_MAX_VQP,
            VDPA_ATTR_DEV_QUEUE_INDEX,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_classes_distinct() {
        assert_ne!(VDPA_MGMTDEV_CLASS_NET, VDPA_MGMTDEV_CLASS_BLOCK);
    }
}
