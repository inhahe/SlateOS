//! `<linux/batman_adv.h>` — Additional B.A.T.M.A.N. constants.
//!
//! Supplementary B.A.T.M.A.N. Advanced mesh networking constants
//! covering attribute types, command types, and gateway modes.

// ---------------------------------------------------------------------------
// BATMAN command types
// ---------------------------------------------------------------------------

/// Unspec.
pub const BATADV_CMD_UNSPEC: u32 = 0;
/// Get mesh info.
pub const BATADV_CMD_GET_MESH_INFO: u32 = 1;
/// Get routing algos.
pub const BATADV_CMD_GET_ROUTING_ALGOS: u32 = 2;
/// Get hardif.
pub const BATADV_CMD_GET_HARDIF: u32 = 3;
/// Get transtable local.
pub const BATADV_CMD_GET_TRANSTABLE_LOCAL: u32 = 4;
/// Get transtable global.
pub const BATADV_CMD_GET_TRANSTABLE_GLOBAL: u32 = 5;
/// Get originators.
pub const BATADV_CMD_GET_ORIGINATORS: u32 = 6;
/// Get neighbors.
pub const BATADV_CMD_GET_NEIGHBORS: u32 = 7;
/// Get gateways.
pub const BATADV_CMD_GET_GATEWAYS: u32 = 8;
/// Get BLA claim.
pub const BATADV_CMD_GET_BLA_CLAIM: u32 = 9;
/// Get BLA backbone.
pub const BATADV_CMD_GET_BLA_BACKBONE: u32 = 10;
/// Get DAT cache.
pub const BATADV_CMD_GET_DAT_CACHE: u32 = 11;
/// Get multicast flags.
pub const BATADV_CMD_GET_MCAST_FLAGS: u32 = 12;

// ---------------------------------------------------------------------------
// BATMAN gateway modes
// ---------------------------------------------------------------------------

/// Off.
pub const BATADV_GW_MODE_OFF: u32 = 0;
/// Client.
pub const BATADV_GW_MODE_CLIENT: u32 = 1;
/// Server.
pub const BATADV_GW_MODE_SERVER: u32 = 2;

// ---------------------------------------------------------------------------
// BATMAN hard interface states
// ---------------------------------------------------------------------------

/// Not in use.
pub const BATADV_IF_NOT_IN_USE: u32 = 0;
/// Active.
pub const BATADV_IF_ACTIVE: u32 = 1;
/// Inactive.
pub const BATADV_IF_INACTIVE: u32 = 2;
/// To be removed.
pub const BATADV_IF_TO_BE_REMOVED: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            BATADV_CMD_UNSPEC, BATADV_CMD_GET_MESH_INFO,
            BATADV_CMD_GET_ROUTING_ALGOS, BATADV_CMD_GET_HARDIF,
            BATADV_CMD_GET_TRANSTABLE_LOCAL, BATADV_CMD_GET_TRANSTABLE_GLOBAL,
            BATADV_CMD_GET_ORIGINATORS, BATADV_CMD_GET_NEIGHBORS,
            BATADV_CMD_GET_GATEWAYS, BATADV_CMD_GET_BLA_CLAIM,
            BATADV_CMD_GET_BLA_BACKBONE, BATADV_CMD_GET_DAT_CACHE,
            BATADV_CMD_GET_MCAST_FLAGS,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_gw_modes_distinct() {
        let modes = [
            BATADV_GW_MODE_OFF, BATADV_GW_MODE_CLIENT,
            BATADV_GW_MODE_SERVER,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_if_states_distinct() {
        let states = [
            BATADV_IF_NOT_IN_USE, BATADV_IF_ACTIVE,
            BATADV_IF_INACTIVE, BATADV_IF_TO_BE_REMOVED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
