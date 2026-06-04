//! `<linux/batadv.h>` / `<linux/batman_adv.h>` — generic-netlink family
//! for the B.A.T.M.A.N. Advanced mesh-routing driver.
//!
//! All control-plane interaction with `batman-adv` (querying neighbours,
//! translation tables, originators) flows through the
//! `batadv` generic-netlink family. This module covers the family
//! name, command codes, and top-level attribute identifiers.

// ---------------------------------------------------------------------------
// Family identity
// ---------------------------------------------------------------------------

pub const BATADV_NL_NAME: &str = "batadv";
pub const BATADV_NL_MCAST_GROUP_CONFIG: &str = "config";
pub const BATADV_NL_MCAST_GROUP_TPMETER: &str = "tpmeter";

// ---------------------------------------------------------------------------
// Generic-netlink command identifiers (`enum batadv_nl_commands`)
// ---------------------------------------------------------------------------

pub const BATADV_CMD_UNSPEC: u32 = 0;
pub const BATADV_CMD_GET_MESH: u32 = 1;
pub const BATADV_CMD_TP_METER: u32 = 2;
pub const BATADV_CMD_TP_METER_CANCEL: u32 = 3;
pub const BATADV_CMD_GET_ROUTING_ALGOS: u32 = 4;
pub const BATADV_CMD_GET_HARDIF: u32 = 5;
pub const BATADV_CMD_GET_TRANSTABLE_LOCAL: u32 = 6;
pub const BATADV_CMD_GET_TRANSTABLE_GLOBAL: u32 = 7;
pub const BATADV_CMD_GET_ORIGINATORS: u32 = 8;
pub const BATADV_CMD_GET_NEIGHBORS: u32 = 9;
pub const BATADV_CMD_GET_GATEWAYS: u32 = 10;
pub const BATADV_CMD_GET_BLA_CLAIM: u32 = 11;
pub const BATADV_CMD_GET_BLA_BACKBONE: u32 = 12;
pub const BATADV_CMD_GET_DAT_CACHE: u32 = 13;
pub const BATADV_CMD_GET_MCAST_FLAGS: u32 = 14;
pub const BATADV_CMD_SET_MESH: u32 = 15;
pub const BATADV_CMD_SET_HARDIF: u32 = 16;
pub const BATADV_CMD_GET_VLAN: u32 = 17;
pub const BATADV_CMD_SET_VLAN: u32 = 18;
pub const __BATADV_CMD_AFTER_LAST: u32 = 19;
pub const BATADV_CMD_MAX: u32 = __BATADV_CMD_AFTER_LAST - 1;

// ---------------------------------------------------------------------------
// Routing-algorithm identifiers
// ---------------------------------------------------------------------------

pub const BATADV_ALGO_BATMAN_IV: &str = "BATMAN_IV";
pub const BATADV_ALGO_BATMAN_V: &str = "BATMAN_V";

// ---------------------------------------------------------------------------
// Default tunables (driver defaults from net/batman-adv/main.h)
// ---------------------------------------------------------------------------

pub const BATADV_TT_VERSION: u8 = 1;
pub const BATADV_TT_LOCAL_TIMEOUT: u32 = 600_000;
pub const BATADV_TT_CLIENT_TEMP_TIMEOUT: u32 = 600_000;
pub const BATADV_TT_WORK_PERIOD: u32 = 5_000;
pub const BATADV_ORIG_WORK_PERIOD: u32 = 1_000;
pub const BATADV_BLA_PERIOD_LENGTH: u32 = 10_000;
pub const BATADV_BLA_BACKBONE_TIMEOUT: u32 = 60_000;
pub const BATADV_BLA_CLAIM_TIMEOUT: u32 = 600_000;
pub const BATADV_BLA_WAIT_PERIODS: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_and_groups_distinct() {
        assert_eq!(BATADV_NL_NAME, "batadv");
        assert_eq!(BATADV_NL_MCAST_GROUP_CONFIG, "config");
        assert_eq!(BATADV_NL_MCAST_GROUP_TPMETER, "tpmeter");
        assert_ne!(BATADV_NL_MCAST_GROUP_CONFIG, BATADV_NL_MCAST_GROUP_TPMETER);
    }

    #[test]
    fn test_cmd_codes_dense_0_to_18() {
        let c = [
            BATADV_CMD_UNSPEC,
            BATADV_CMD_GET_MESH,
            BATADV_CMD_TP_METER,
            BATADV_CMD_TP_METER_CANCEL,
            BATADV_CMD_GET_ROUTING_ALGOS,
            BATADV_CMD_GET_HARDIF,
            BATADV_CMD_GET_TRANSTABLE_LOCAL,
            BATADV_CMD_GET_TRANSTABLE_GLOBAL,
            BATADV_CMD_GET_ORIGINATORS,
            BATADV_CMD_GET_NEIGHBORS,
            BATADV_CMD_GET_GATEWAYS,
            BATADV_CMD_GET_BLA_CLAIM,
            BATADV_CMD_GET_BLA_BACKBONE,
            BATADV_CMD_GET_DAT_CACHE,
            BATADV_CMD_GET_MCAST_FLAGS,
            BATADV_CMD_SET_MESH,
            BATADV_CMD_SET_HARDIF,
            BATADV_CMD_GET_VLAN,
            BATADV_CMD_SET_VLAN,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(__BATADV_CMD_AFTER_LAST, c.len() as u32);
        assert_eq!(BATADV_CMD_MAX, __BATADV_CMD_AFTER_LAST - 1);
        assert_eq!(BATADV_CMD_MAX, BATADV_CMD_SET_VLAN);
    }

    #[test]
    fn test_routing_algos_named() {
        assert_eq!(BATADV_ALGO_BATMAN_IV, "BATMAN_IV");
        assert_eq!(BATADV_ALGO_BATMAN_V, "BATMAN_V");
        assert_ne!(BATADV_ALGO_BATMAN_IV, BATADV_ALGO_BATMAN_V);
    }

    #[test]
    fn test_default_periods_ordered_reasonably() {
        // Work periods are short; client/backbone timeouts are long.
        assert!(BATADV_ORIG_WORK_PERIOD < BATADV_TT_WORK_PERIOD);
        assert!(BATADV_TT_WORK_PERIOD < BATADV_BLA_PERIOD_LENGTH);
        assert!(BATADV_BLA_BACKBONE_TIMEOUT < BATADV_TT_LOCAL_TIMEOUT);
        // TT_LOCAL and TT_CLIENT_TEMP share the same timeout by design.
        assert_eq!(BATADV_TT_LOCAL_TIMEOUT, BATADV_TT_CLIENT_TEMP_TIMEOUT);
        // BLA needs at least three periods of silence to consider the
        // bridge loop avoidance state stable.
        assert_eq!(BATADV_BLA_WAIT_PERIODS, 3);
        assert_eq!(BATADV_TT_VERSION, 1);
    }
}
