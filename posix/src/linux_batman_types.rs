//! `<linux/batman_adv.h>` — B.A.T.M.A.N. Advanced mesh networking constants.
//!
//! batman-adv is a kernel-level mesh networking protocol that operates
//! on Layer 2. Nodes automatically discover each other, select optimal
//! routes based on link quality metrics, and transparently forward
//! Ethernet frames across multi-hop wireless mesh networks. The
//! netlink interface configures mesh parameters, gateway selection,
//! and monitoring. Used in community wireless networks (Freifunk),
//! IoT mesh deployments, and ad-hoc networking.

// ---------------------------------------------------------------------------
// Netlink commands (BATADV_CMD_*)
// ---------------------------------------------------------------------------

/// Get mesh info.
pub const BATADV_CMD_GET_MESH_INFO: u32 = 1;
/// Set mesh parameters.
pub const BATADV_CMD_SET_MESH: u32 = 2;
/// Get hard interface info.
pub const BATADV_CMD_GET_HARDIF: u32 = 3;
/// Set hard interface parameters.
pub const BATADV_CMD_SET_HARDIF: u32 = 4;
/// Get translation table (local).
pub const BATADV_CMD_GET_TRANSTABLE_LOCAL: u32 = 5;
/// Get translation table (global).
pub const BATADV_CMD_GET_TRANSTABLE_GLOBAL: u32 = 6;
/// Get originator table.
pub const BATADV_CMD_GET_ORIGINATORS: u32 = 7;
/// Get next-hop neighbors.
pub const BATADV_CMD_GET_NEIGHBORS: u32 = 8;
/// Get gateways.
pub const BATADV_CMD_GET_GATEWAYS: u32 = 9;
/// Get BLA (Bridge Loop Avoidance) claims.
pub const BATADV_CMD_GET_BLA_CLAIM: u32 = 10;
/// Get BLA backbone gateways.
pub const BATADV_CMD_GET_BLA_BACKBONE: u32 = 11;
/// Get DAT (Distributed ARP Table) cache.
pub const BATADV_CMD_GET_DAT_CACHE: u32 = 12;
/// Get multicast flags.
pub const BATADV_CMD_GET_MCAST_FLAGS: u32 = 13;
/// Get VLAN info.
pub const BATADV_CMD_GET_VLAN: u32 = 14;
/// Set VLAN parameters.
pub const BATADV_CMD_SET_VLAN: u32 = 15;

// ---------------------------------------------------------------------------
// Routing algorithm IDs
// ---------------------------------------------------------------------------

/// BATMAN IV routing algorithm (TQ-based).
pub const BATADV_ROUTING_ALGO_BATMAN_IV: u32 = 0;
/// BATMAN V routing algorithm (throughput-based).
pub const BATADV_ROUTING_ALGO_BATMAN_V: u32 = 1;

// ---------------------------------------------------------------------------
// Gateway modes
// ---------------------------------------------------------------------------

/// Node is not a gateway.
pub const BATADV_GW_MODE_OFF: u32 = 0;
/// Node is a mesh gateway (has Internet uplink).
pub const BATADV_GW_MODE_SERVER: u32 = 1;
/// Node uses a mesh gateway for Internet access.
pub const BATADV_GW_MODE_CLIENT: u32 = 2;

// ---------------------------------------------------------------------------
// Gateway selection classes
// ---------------------------------------------------------------------------

/// Select gateway with best TQ (link quality).
pub const BATADV_GW_SEL_TQ: u32 = 0;
/// Select gateway with best bandwidth.
pub const BATADV_GW_SEL_BANDWIDTH: u32 = 1;

// ---------------------------------------------------------------------------
// Hard interface states
// ---------------------------------------------------------------------------

/// Interface is not in use by batman-adv.
pub const BATADV_IF_NOT_IN_USE: u32 = 0;
/// Interface is active.
pub const BATADV_IF_ACTIVE: u32 = 1;
/// Interface is inactive.
pub const BATADV_IF_INACTIVE: u32 = 2;

// ---------------------------------------------------------------------------
// Translation table flags
// ---------------------------------------------------------------------------

/// Entry is locally originated.
pub const BATADV_TT_CLIENT_DEL: u32 = 1 << 0;
/// Entry has been roamed (client moved to another node).
pub const BATADV_TT_CLIENT_ROAM: u32 = 1 << 1;
/// Entry is a non-purge entry.
pub const BATADV_TT_CLIENT_NOPURGE: u32 = 1 << 2;
/// Entry is a new client.
pub const BATADV_TT_CLIENT_NEW: u32 = 1 << 3;
/// Entry has pending update.
pub const BATADV_TT_CLIENT_PENDING: u32 = 1 << 4;
/// Entry is temporary.
pub const BATADV_TT_CLIENT_TEMP: u32 = 1 << 5;
/// Entry is for the mesh VLAN.
pub const BATADV_TT_CLIENT_WIFI: u32 = 1 << 6;
/// Entry is isolated (no inter-client communication).
pub const BATADV_TT_CLIENT_ISOLA: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            BATADV_CMD_GET_MESH_INFO, BATADV_CMD_SET_MESH,
            BATADV_CMD_GET_HARDIF, BATADV_CMD_SET_HARDIF,
            BATADV_CMD_GET_TRANSTABLE_LOCAL, BATADV_CMD_GET_TRANSTABLE_GLOBAL,
            BATADV_CMD_GET_ORIGINATORS, BATADV_CMD_GET_NEIGHBORS,
            BATADV_CMD_GET_GATEWAYS, BATADV_CMD_GET_BLA_CLAIM,
            BATADV_CMD_GET_BLA_BACKBONE, BATADV_CMD_GET_DAT_CACHE,
            BATADV_CMD_GET_MCAST_FLAGS, BATADV_CMD_GET_VLAN,
            BATADV_CMD_SET_VLAN,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_routing_algos_distinct() {
        assert_ne!(BATADV_ROUTING_ALGO_BATMAN_IV, BATADV_ROUTING_ALGO_BATMAN_V);
    }

    #[test]
    fn test_gw_modes_distinct() {
        let modes = [BATADV_GW_MODE_OFF, BATADV_GW_MODE_SERVER, BATADV_GW_MODE_CLIENT];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_if_states_distinct() {
        let states = [BATADV_IF_NOT_IN_USE, BATADV_IF_ACTIVE, BATADV_IF_INACTIVE];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_tt_flags_no_overlap() {
        let flags = [
            BATADV_TT_CLIENT_DEL, BATADV_TT_CLIENT_ROAM,
            BATADV_TT_CLIENT_NOPURGE, BATADV_TT_CLIENT_NEW,
            BATADV_TT_CLIENT_PENDING, BATADV_TT_CLIENT_TEMP,
            BATADV_TT_CLIENT_WIFI, BATADV_TT_CLIENT_ISOLA,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_gw_sel_distinct() {
        assert_ne!(BATADV_GW_SEL_TQ, BATADV_GW_SEL_BANDWIDTH);
    }
}
