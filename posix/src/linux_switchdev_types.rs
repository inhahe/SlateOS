//! `<linux/switchdev.h>` — Switch device (switchdev) offload constants.
//!
//! switchdev is the Linux framework for offloading L2/L3 forwarding
//! to hardware switches (ASICs like Memory, Memory, memory).
//! It allows the kernel's software bridge, routing, and TC to push
//! FDB entries, VLAN filters, and routes directly into switch
//! hardware tables. Used by Memory DSA drivers, Mellanox mlxsw,
//! Marvell prestera, and other Ethernet switch chip drivers.

// ---------------------------------------------------------------------------
// switchdev attribute IDs (SWITCHDEV_ATTR_ID_*)
// ---------------------------------------------------------------------------

/// Port parent device.
pub const SWITCHDEV_ATTR_PORT_PARENT_ID: u32 = 0;
/// Port STP state.
pub const SWITCHDEV_ATTR_PORT_STP_STATE: u32 = 1;
/// Port bridge flags (learning, flooding, etc.).
pub const SWITCHDEV_ATTR_PORT_BRIDGE_FLAGS: u32 = 2;
/// Port MAC learning enable.
pub const SWITCHDEV_ATTR_PORT_LEARNING: u32 = 3;
/// Port multicast router.
pub const SWITCHDEV_ATTR_PORT_MROUTER: u32 = 4;
/// Bridge ageing time.
pub const SWITCHDEV_ATTR_BRIDGE_AGEING_TIME: u32 = 5;
/// Bridge VLAN filtering enable.
pub const SWITCHDEV_ATTR_BRIDGE_VLAN_FILTERING: u32 = 6;
/// Bridge VLAN protocol (802.1Q or 802.1ad).
pub const SWITCHDEV_ATTR_BRIDGE_VLAN_PROTOCOL: u32 = 7;
/// Bridge multicast disabled.
pub const SWITCHDEV_ATTR_BRIDGE_MC_DISABLED: u32 = 8;
/// Bridge MDB (multicast database) entry.
pub const SWITCHDEV_ATTR_BRIDGE_MROUTER: u32 = 9;
/// Bridge MST state.
pub const SWITCHDEV_ATTR_BRIDGE_MST: u32 = 10;

// ---------------------------------------------------------------------------
// switchdev object IDs (SWITCHDEV_OBJ_ID_*)
// ---------------------------------------------------------------------------

/// Port VLAN object (add/del VLANs to port).
pub const SWITCHDEV_OBJ_PORT_VLAN: u32 = 0;
/// Port MDB (multicast group) object.
pub const SWITCHDEV_OBJ_PORT_MDB: u32 = 1;
/// Host MDB object.
pub const SWITCHDEV_OBJ_HOST_MDB: u32 = 2;
/// Mirror (SPAN) object.
pub const SWITCHDEV_OBJ_MIRROR: u32 = 3;
/// Ring (MRP, HSR) object.
pub const SWITCHDEV_OBJ_RING_ROLE: u32 = 4;

// ---------------------------------------------------------------------------
// FDB notification types
// ---------------------------------------------------------------------------

/// FDB entry was learned.
pub const SWITCHDEV_FDB_ADD_TO_BRIDGE: u32 = 0;
/// FDB entry should be removed.
pub const SWITCHDEV_FDB_DEL_TO_BRIDGE: u32 = 1;
/// FDB entry learned by device, notify bridge.
pub const SWITCHDEV_FDB_ADD_TO_DEVICE: u32 = 2;
/// FDB entry deleted by device, notify bridge.
pub const SWITCHDEV_FDB_DEL_TO_DEVICE: u32 = 3;
/// FDB offload indication changed.
pub const SWITCHDEV_FDB_OFFLOADED: u32 = 4;
/// FDB flush request.
pub const SWITCHDEV_FDB_FLUSH_TO_BRIDGE: u32 = 5;

// ---------------------------------------------------------------------------
// Port bridge flags
// ---------------------------------------------------------------------------

/// Enable unicast flooding on port.
pub const BR_FLOOD: u32 = 1 << 0;
/// Enable MAC learning on port.
pub const BR_LEARNING: u32 = 1 << 1;
/// Sync learned FDB entries to bridge.
pub const BR_LEARNING_SYNC: u32 = 1 << 2;
/// Enable multicast flooding on port.
pub const BR_MCAST_FLOOD: u32 = 1 << 3;
/// Enable broadcast flooding on port.
pub const BR_BCAST_FLOOD: u32 = 1 << 4;
/// Port is in isolated mode (no inter-port forwarding).
pub const BR_ISOLATED: u32 = 1 << 5;
/// Port is locked (only pre-authorized MACs).
pub const BR_PORT_LOCKED: u32 = 1 << 6;
/// Port sends locked MACs as notifications.
pub const BR_PORT_MAB: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// STP states
// ---------------------------------------------------------------------------

/// Port is disabled.
pub const BR_STATE_DISABLED: u32 = 0;
/// Port is listening (STP state).
pub const BR_STATE_LISTENING: u32 = 1;
/// Port is learning (MAC table population).
pub const BR_STATE_LEARNING: u32 = 2;
/// Port is forwarding (normal operation).
pub const BR_STATE_FORWARDING: u32 = 3;
/// Port is blocking (STP loop prevention).
pub const BR_STATE_BLOCKING: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attr_ids_distinct() {
        let attrs = [
            SWITCHDEV_ATTR_PORT_PARENT_ID, SWITCHDEV_ATTR_PORT_STP_STATE,
            SWITCHDEV_ATTR_PORT_BRIDGE_FLAGS, SWITCHDEV_ATTR_PORT_LEARNING,
            SWITCHDEV_ATTR_PORT_MROUTER, SWITCHDEV_ATTR_BRIDGE_AGEING_TIME,
            SWITCHDEV_ATTR_BRIDGE_VLAN_FILTERING, SWITCHDEV_ATTR_BRIDGE_VLAN_PROTOCOL,
            SWITCHDEV_ATTR_BRIDGE_MC_DISABLED, SWITCHDEV_ATTR_BRIDGE_MROUTER,
            SWITCHDEV_ATTR_BRIDGE_MST,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_obj_ids_distinct() {
        let objs = [
            SWITCHDEV_OBJ_PORT_VLAN, SWITCHDEV_OBJ_PORT_MDB,
            SWITCHDEV_OBJ_HOST_MDB, SWITCHDEV_OBJ_MIRROR,
            SWITCHDEV_OBJ_RING_ROLE,
        ];
        for i in 0..objs.len() {
            for j in (i + 1)..objs.len() {
                assert_ne!(objs[i], objs[j]);
            }
        }
    }

    #[test]
    fn test_fdb_types_distinct() {
        let fdb = [
            SWITCHDEV_FDB_ADD_TO_BRIDGE, SWITCHDEV_FDB_DEL_TO_BRIDGE,
            SWITCHDEV_FDB_ADD_TO_DEVICE, SWITCHDEV_FDB_DEL_TO_DEVICE,
            SWITCHDEV_FDB_OFFLOADED, SWITCHDEV_FDB_FLUSH_TO_BRIDGE,
        ];
        for i in 0..fdb.len() {
            for j in (i + 1)..fdb.len() {
                assert_ne!(fdb[i], fdb[j]);
            }
        }
    }

    #[test]
    fn test_bridge_flags_no_overlap() {
        let flags = [
            BR_FLOOD, BR_LEARNING, BR_LEARNING_SYNC,
            BR_MCAST_FLOOD, BR_BCAST_FLOOD, BR_ISOLATED,
            BR_PORT_LOCKED, BR_PORT_MAB,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_stp_states_distinct() {
        let states = [
            BR_STATE_DISABLED, BR_STATE_LISTENING,
            BR_STATE_LEARNING, BR_STATE_FORWARDING,
            BR_STATE_BLOCKING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
