//! `<rdma/rdma_netlink.h>` — RDMA (Remote Direct Memory Access) constants.
//!
//! These constants define RDMA netlink client types, attribute
//! types, link states, port capabilities, and transport types
//! for InfiniBand and RoCE devices.

// ---------------------------------------------------------------------------
// RDMA netlink client types
// ---------------------------------------------------------------------------

/// IB-specific client.
pub const RDMA_NL_IWCM: u32 = 2;
/// RDMA core manager client.
pub const RDMA_NL_RSVD: u32 = 3;
/// Netlink/LS client.
pub const RDMA_NL_LS: u32 = 4;
/// NLDEV (device management) client.
pub const RDMA_NL_NLDEV: u32 = 5;

// ---------------------------------------------------------------------------
// RDMA NLDEV attribute types (RDMA_NLDEV_ATTR_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const RDMA_NLDEV_ATTR_UNSPEC: u32 = 0;
/// Device index.
pub const RDMA_NLDEV_ATTR_DEV_INDEX: u32 = 1;
/// Device name.
pub const RDMA_NLDEV_ATTR_DEV_NAME: u32 = 2;
/// Port index.
pub const RDMA_NLDEV_ATTR_PORT_INDEX: u32 = 3;
/// Capability flags.
pub const RDMA_NLDEV_ATTR_CAP_FLAGS: u32 = 4;
/// Firmware version.
pub const RDMA_NLDEV_ATTR_FW_VERSION: u32 = 5;
/// Node GUID.
pub const RDMA_NLDEV_ATTR_NODE_GUID: u32 = 6;
/// System image GUID.
pub const RDMA_NLDEV_ATTR_SYS_IMAGE_GUID: u32 = 7;
/// Subnet prefix.
pub const RDMA_NLDEV_ATTR_SUBNET_PREFIX: u32 = 8;
/// LID (Local Identifier).
pub const RDMA_NLDEV_ATTR_LID: u32 = 9;
/// SM LID.
pub const RDMA_NLDEV_ATTR_SM_LID: u32 = 10;
/// LMC (LID Mask Count).
pub const RDMA_NLDEV_ATTR_LMC: u32 = 11;
/// Port state.
pub const RDMA_NLDEV_ATTR_PORT_STATE: u32 = 12;
/// Port physical state.
pub const RDMA_NLDEV_ATTR_PORT_PHYS_STATE: u32 = 13;
/// Device capabilities.
pub const RDMA_NLDEV_ATTR_DEV_NODE_TYPE: u32 = 14;

// ---------------------------------------------------------------------------
// RDMA NLDEV commands
// ---------------------------------------------------------------------------

/// Get device.
pub const RDMA_NLDEV_CMD_GET: u32 = 1;
/// Set device.
pub const RDMA_NLDEV_CMD_SET: u32 = 2;
/// New link.
pub const RDMA_NLDEV_CMD_NEWLINK: u32 = 3;
/// Delete link.
pub const RDMA_NLDEV_CMD_DELLINK: u32 = 4;
/// Get port.
pub const RDMA_NLDEV_CMD_PORT_GET: u32 = 5;
/// Get system parameters.
pub const RDMA_NLDEV_CMD_SYS_GET: u32 = 6;
/// Set system parameters.
pub const RDMA_NLDEV_CMD_SYS_SET: u32 = 7;
/// Resource get.
pub const RDMA_NLDEV_CMD_RES_GET: u32 = 9;
/// Resource QP get.
pub const RDMA_NLDEV_CMD_RES_QP_GET: u32 = 10;
/// Resource CM_ID get.
pub const RDMA_NLDEV_CMD_RES_CM_ID_GET: u32 = 11;
/// Resource CQ get.
pub const RDMA_NLDEV_CMD_RES_CQ_GET: u32 = 12;
/// Resource MR get.
pub const RDMA_NLDEV_CMD_RES_MR_GET: u32 = 13;
/// Resource PD get.
pub const RDMA_NLDEV_CMD_RES_PD_GET: u32 = 14;
/// Statistics get.
pub const RDMA_NLDEV_CMD_STAT_GET: u32 = 17;

// ---------------------------------------------------------------------------
// RDMA transport types
// ---------------------------------------------------------------------------

/// InfiniBand.
pub const RDMA_TRANSPORT_IB: u32 = 0;
/// iWARP (Internet Wide Area RDMA Protocol).
pub const RDMA_TRANSPORT_IWARP: u32 = 1;
/// Userspace RDMA.
pub const RDMA_TRANSPORT_USNIC: u32 = 2;
/// Userspace RDMA (usnic_udp).
pub const RDMA_TRANSPORT_USNIC_UDP: u32 = 3;
/// Unspecified transport.
pub const RDMA_TRANSPORT_UNSPEC: u32 = 4;

// ---------------------------------------------------------------------------
// RDMA link layer types
// ---------------------------------------------------------------------------

/// Unspecified link layer.
pub const IB_LINK_LAYER_UNSPECIFIED: u32 = 0;
/// InfiniBand link layer.
pub const IB_LINK_LAYER_INFINIBAND: u32 = 1;
/// Ethernet link layer (RoCE).
pub const IB_LINK_LAYER_ETHERNET: u32 = 2;

// ---------------------------------------------------------------------------
// RDMA port states
// ---------------------------------------------------------------------------

/// No change.
pub const IB_PORT_NOP: u32 = 0;
/// Port down.
pub const IB_PORT_DOWN: u32 = 1;
/// Port init.
pub const IB_PORT_INIT: u32 = 2;
/// Port armed.
pub const IB_PORT_ARMED: u32 = 3;
/// Port active.
pub const IB_PORT_ACTIVE: u32 = 4;
/// Port active deferred.
pub const IB_PORT_ACTIVE_DEFER: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nl_clients_distinct() {
        let clients = [RDMA_NL_IWCM, RDMA_NL_RSVD, RDMA_NL_LS, RDMA_NL_NLDEV];
        for i in 0..clients.len() {
            for j in (i + 1)..clients.len() {
                assert_ne!(clients[i], clients[j]);
            }
        }
    }

    #[test]
    fn test_nldev_attrs_distinct() {
        let attrs = [
            RDMA_NLDEV_ATTR_UNSPEC, RDMA_NLDEV_ATTR_DEV_INDEX,
            RDMA_NLDEV_ATTR_DEV_NAME, RDMA_NLDEV_ATTR_PORT_INDEX,
            RDMA_NLDEV_ATTR_CAP_FLAGS, RDMA_NLDEV_ATTR_FW_VERSION,
            RDMA_NLDEV_ATTR_NODE_GUID, RDMA_NLDEV_ATTR_SYS_IMAGE_GUID,
            RDMA_NLDEV_ATTR_SUBNET_PREFIX, RDMA_NLDEV_ATTR_LID,
            RDMA_NLDEV_ATTR_SM_LID, RDMA_NLDEV_ATTR_LMC,
            RDMA_NLDEV_ATTR_PORT_STATE, RDMA_NLDEV_ATTR_PORT_PHYS_STATE,
            RDMA_NLDEV_ATTR_DEV_NODE_TYPE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_nldev_cmds_distinct() {
        let cmds = [
            RDMA_NLDEV_CMD_GET, RDMA_NLDEV_CMD_SET,
            RDMA_NLDEV_CMD_NEWLINK, RDMA_NLDEV_CMD_DELLINK,
            RDMA_NLDEV_CMD_PORT_GET, RDMA_NLDEV_CMD_SYS_GET,
            RDMA_NLDEV_CMD_SYS_SET, RDMA_NLDEV_CMD_RES_GET,
            RDMA_NLDEV_CMD_RES_QP_GET, RDMA_NLDEV_CMD_RES_CM_ID_GET,
            RDMA_NLDEV_CMD_RES_CQ_GET, RDMA_NLDEV_CMD_RES_MR_GET,
            RDMA_NLDEV_CMD_RES_PD_GET, RDMA_NLDEV_CMD_STAT_GET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_transport_types_distinct() {
        let transports = [
            RDMA_TRANSPORT_IB, RDMA_TRANSPORT_IWARP,
            RDMA_TRANSPORT_USNIC, RDMA_TRANSPORT_USNIC_UDP,
            RDMA_TRANSPORT_UNSPEC,
        ];
        for i in 0..transports.len() {
            for j in (i + 1)..transports.len() {
                assert_ne!(transports[i], transports[j]);
            }
        }
    }

    #[test]
    fn test_link_layers_distinct() {
        let layers = [
            IB_LINK_LAYER_UNSPECIFIED, IB_LINK_LAYER_INFINIBAND,
            IB_LINK_LAYER_ETHERNET,
        ];
        for i in 0..layers.len() {
            for j in (i + 1)..layers.len() {
                assert_ne!(layers[i], layers[j]);
            }
        }
    }

    #[test]
    fn test_port_states_distinct() {
        let states = [
            IB_PORT_NOP, IB_PORT_DOWN, IB_PORT_INIT,
            IB_PORT_ARMED, IB_PORT_ACTIVE, IB_PORT_ACTIVE_DEFER,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_ib_transport_is_zero() {
        assert_eq!(RDMA_TRANSPORT_IB, 0);
    }

    #[test]
    fn test_port_active() {
        assert_eq!(IB_PORT_ACTIVE, 4);
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(RDMA_NLDEV_ATTR_UNSPEC, 0);
    }
}
