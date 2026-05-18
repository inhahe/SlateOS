//! `<linux/batman_adv.h>` — B.A.T.M.A.N. Advanced mesh networking constants.
//!
//! batman-adv is a mesh networking protocol implementation.
//! These constants define netlink attribute types, packet
//! types, routing algorithms, and gateway modes.

// ---------------------------------------------------------------------------
// BATADV netlink commands (BATADV_CMD_*)
// ---------------------------------------------------------------------------

/// Get mesh info.
pub const BATADV_CMD_GET_MESH_INFO: u32 = 1;
/// Get mesh.
pub const BATADV_CMD_GET_MESH: u32 = 7;
/// Set mesh.
pub const BATADV_CMD_SET_MESH: u32 = 8;
/// Dump originator table.
pub const BATADV_CMD_GET_ORIGINATORS: u32 = 3;
/// Dump neighbor table.
pub const BATADV_CMD_GET_NEIGHBORS: u32 = 4;
/// Dump translation table (global).
pub const BATADV_CMD_GET_TRANSTABLE_GLOBAL: u32 = 5;
/// Dump translation table (local).
pub const BATADV_CMD_GET_TRANSTABLE_LOCAL: u32 = 6;
/// Get gateway list.
pub const BATADV_CMD_GET_GATEWAYS: u32 = 9;
/// Get BLA claims.
pub const BATADV_CMD_GET_BLA_CLAIM: u32 = 10;
/// Get BLA backbones.
pub const BATADV_CMD_GET_BLA_BACKBONE: u32 = 11;
/// Get DAT cache.
pub const BATADV_CMD_GET_DAT_CACHE: u32 = 12;
/// Get multicast flags.
pub const BATADV_CMD_GET_MCAST_FLAGS: u32 = 13;
/// Get hardif.
pub const BATADV_CMD_GET_HARDIF: u32 = 15;
/// Set hardif.
pub const BATADV_CMD_SET_HARDIF: u32 = 16;
/// Get VLAN.
pub const BATADV_CMD_GET_VLAN: u32 = 17;
/// Set VLAN.
pub const BATADV_CMD_SET_VLAN: u32 = 18;

// ---------------------------------------------------------------------------
// BATADV packet types
// ---------------------------------------------------------------------------

/// OGM (Originator Message).
pub const BATADV_IV_OGM: u32 = 0x00;
/// ICMP.
pub const BATADV_ICMP: u32 = 0x02;
/// Unicast.
pub const BATADV_UNICAST: u32 = 0x03;
/// Broadcast.
pub const BATADV_BCAST: u32 = 0x04;
/// Coded unicast (network coding).
pub const BATADV_CODED: u32 = 0x06;
/// ELP (Echo Location Protocol).
pub const BATADV_ELP: u32 = 0x07;
/// OGMv2.
pub const BATADV_OGM2: u32 = 0x08;
/// Unicast TVLV.
pub const BATADV_UNICAST_TVLV: u32 = 0x09;

// ---------------------------------------------------------------------------
// BATADV gateway modes
// ---------------------------------------------------------------------------

/// Gateway mode off.
pub const BATADV_GW_MODE_OFF: u32 = 0;
/// Client mode.
pub const BATADV_GW_MODE_CLIENT: u32 = 1;
/// Server mode.
pub const BATADV_GW_MODE_SERVER: u32 = 2;

// ---------------------------------------------------------------------------
// BATADV routing algorithms
// ---------------------------------------------------------------------------

/// BATMAN IV (default).
pub const BATADV_ALGO_BATMAN_IV: u32 = 0;
/// BATMAN V.
pub const BATADV_ALGO_BATMAN_V: u32 = 1;

// ---------------------------------------------------------------------------
// BATADV hard interface states
// ---------------------------------------------------------------------------

/// Not in use.
pub const BATADV_IF_NOT_IN_USE: u32 = 0;
/// Active.
pub const BATADV_IF_ACTIVE: u32 = 1;
/// Inactive.
pub const BATADV_IF_INACTIVE: u32 = 2;
/// To be removed.
pub const BATADV_IF_TO_BE_REMOVED: u32 = 3;
/// To be activated.
pub const BATADV_IF_TO_BE_ACTIVATED: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            BATADV_CMD_GET_MESH_INFO, BATADV_CMD_GET_MESH,
            BATADV_CMD_SET_MESH, BATADV_CMD_GET_ORIGINATORS,
            BATADV_CMD_GET_NEIGHBORS,
            BATADV_CMD_GET_TRANSTABLE_GLOBAL,
            BATADV_CMD_GET_TRANSTABLE_LOCAL,
            BATADV_CMD_GET_GATEWAYS, BATADV_CMD_GET_BLA_CLAIM,
            BATADV_CMD_GET_BLA_BACKBONE, BATADV_CMD_GET_DAT_CACHE,
            BATADV_CMD_GET_MCAST_FLAGS, BATADV_CMD_GET_HARDIF,
            BATADV_CMD_SET_HARDIF, BATADV_CMD_GET_VLAN,
            BATADV_CMD_SET_VLAN,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_packet_types_distinct() {
        let pkts = [
            BATADV_IV_OGM, BATADV_ICMP, BATADV_UNICAST,
            BATADV_BCAST, BATADV_CODED, BATADV_ELP,
            BATADV_OGM2, BATADV_UNICAST_TVLV,
        ];
        for i in 0..pkts.len() {
            for j in (i + 1)..pkts.len() {
                assert_ne!(pkts[i], pkts[j]);
            }
        }
    }

    #[test]
    fn test_gw_modes_distinct() {
        let modes = [BATADV_GW_MODE_OFF, BATADV_GW_MODE_CLIENT, BATADV_GW_MODE_SERVER];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_algos_distinct() {
        assert_ne!(BATADV_ALGO_BATMAN_IV, BATADV_ALGO_BATMAN_V);
    }

    #[test]
    fn test_if_states_distinct() {
        let states = [
            BATADV_IF_NOT_IN_USE, BATADV_IF_ACTIVE,
            BATADV_IF_INACTIVE, BATADV_IF_TO_BE_REMOVED,
            BATADV_IF_TO_BE_ACTIVATED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_gw_off_is_zero() {
        assert_eq!(BATADV_GW_MODE_OFF, 0);
    }
}
