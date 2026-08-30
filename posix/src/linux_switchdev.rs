//! `<linux/switchdev.h>` — Switch device offload constants.
//!
//! Switchdev is the Linux kernel framework for offloading L2/L3
//! forwarding, bridging, and routing to hardware switches. It
//! allows the kernel's networking stack to program hardware FDB
//! entries, VLAN filtering, and forwarding rules directly into
//! switch ASICs.

// ---------------------------------------------------------------------------
// Switchdev object types
// ---------------------------------------------------------------------------

/// FDB (Forwarding Database) entry.
pub const SWITCHDEV_OBJ_ID_FDB: u16 = 1;
/// Port VLAN.
pub const SWITCHDEV_OBJ_ID_PORT_VLAN: u16 = 2;
/// MDB (Multicast Database) entry.
pub const SWITCHDEV_OBJ_ID_MDB: u16 = 3;
/// Host MDB entry.
pub const SWITCHDEV_OBJ_ID_HOST_MDB: u16 = 4;
/// Mirror entry.
pub const SWITCHDEV_OBJ_ID_MIRROR: u16 = 5;

// ---------------------------------------------------------------------------
// Switchdev attribute types
// ---------------------------------------------------------------------------

/// Bridge port STP state.
pub const SWITCHDEV_ATTR_PORT_STP_STATE: u16 = 1;
/// Bridge port bridge flags.
pub const SWITCHDEV_ATTR_PORT_BRIDGE_FLAGS: u16 = 2;
/// Port pre-bridge flags.
pub const SWITCHDEV_ATTR_PORT_PRE_BRIDGE_FLAGS: u16 = 3;
/// Bridge VLAN filtering enable.
pub const SWITCHDEV_ATTR_BRIDGE_VLAN_FILTERING: u16 = 4;
/// Bridge ageing time.
pub const SWITCHDEV_ATTR_BRIDGE_AGEING_TIME: u16 = 5;
/// Bridge multicast disabled.
pub const SWITCHDEV_ATTR_BRIDGE_MC_DISABLED: u16 = 6;
/// Bridge VLAN protocol (0x8100/0x88A8).
pub const SWITCHDEV_ATTR_BRIDGE_VLAN_PROTOCOL: u16 = 7;

// ---------------------------------------------------------------------------
// FDB notification types
// ---------------------------------------------------------------------------

/// FDB entry added (learned).
pub const SWITCHDEV_FDB_ADD: u8 = 0;
/// FDB entry deleted (aged out or flushed).
pub const SWITCHDEV_FDB_DEL: u8 = 1;
/// FDB offloaded to hardware.
pub const SWITCHDEV_FDB_OFFLOADED: u8 = 2;

// ---------------------------------------------------------------------------
// Switchdev notification types
// ---------------------------------------------------------------------------

/// Port object added.
pub const SWITCHDEV_NOTIFIER_FDB_ADD: u32 = 1;
/// Port object deleted.
pub const SWITCHDEV_NOTIFIER_FDB_DEL: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_obj_ids_distinct() {
        let ids = [
            SWITCHDEV_OBJ_ID_FDB,
            SWITCHDEV_OBJ_ID_PORT_VLAN,
            SWITCHDEV_OBJ_ID_MDB,
            SWITCHDEV_OBJ_ID_HOST_MDB,
            SWITCHDEV_OBJ_ID_MIRROR,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_attr_types_distinct() {
        let attrs = [
            SWITCHDEV_ATTR_PORT_STP_STATE,
            SWITCHDEV_ATTR_PORT_BRIDGE_FLAGS,
            SWITCHDEV_ATTR_PORT_PRE_BRIDGE_FLAGS,
            SWITCHDEV_ATTR_BRIDGE_VLAN_FILTERING,
            SWITCHDEV_ATTR_BRIDGE_AGEING_TIME,
            SWITCHDEV_ATTR_BRIDGE_MC_DISABLED,
            SWITCHDEV_ATTR_BRIDGE_VLAN_PROTOCOL,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_fdb_notifications_distinct() {
        let notifs = [
            SWITCHDEV_FDB_ADD,
            SWITCHDEV_FDB_DEL,
            SWITCHDEV_FDB_OFFLOADED,
        ];
        for i in 0..notifs.len() {
            for j in (i + 1)..notifs.len() {
                assert_ne!(notifs[i], notifs[j]);
            }
        }
    }
}
