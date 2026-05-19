//! `<linux/batadv_packet.h>` — Additional batman-adv constants.
//!
//! Supplementary batman-adv constants covering routing algorithms,
//! gateway modes, and packet types.

// ---------------------------------------------------------------------------
// Routing algorithms
// ---------------------------------------------------------------------------

/// BATMAN IV routing algorithm.
pub const BATADV_ROUTING_ALGO_IV: u32 = 0;
/// BATMAN V routing algorithm.
pub const BATADV_ROUTING_ALGO_V: u32 = 1;

// ---------------------------------------------------------------------------
// Gateway modes (BATADV_GW_MODE_*)
// ---------------------------------------------------------------------------

/// Off (no gateway).
pub const BATADV_GW_MODE_OFF: u32 = 0;
/// Client (use best gateway).
pub const BATADV_GW_MODE_CLIENT: u32 = 1;
/// Server (announce as gateway).
pub const BATADV_GW_MODE_SERVER: u32 = 2;

// ---------------------------------------------------------------------------
// Packet types (BATADV_*)
// ---------------------------------------------------------------------------

/// OGM (Originator Message).
pub const BATADV_OGM: u32 = 0x01;
/// ICMP packet.
pub const BATADV_ICMP: u32 = 0x02;
/// Unicast packet.
pub const BATADV_UNICAST: u32 = 0x03;
/// Broadcast packet.
pub const BATADV_BCAST: u32 = 0x04;
/// Vis (visualization) packet.
pub const BATADV_VIS: u32 = 0x05;
/// Unicast fragmented.
pub const BATADV_UNICAST_FRAG: u32 = 0x06;
/// Translation table query.
pub const BATADV_TT_QUERY: u32 = 0x07;
/// Roaming advertisement.
pub const BATADV_ROAM_ADV: u32 = 0x08;
/// Unicast 4addr.
pub const BATADV_UNICAST_4ADDR: u32 = 0x09;
/// Coded packet.
pub const BATADV_CODED: u32 = 0x0A;
/// ELP (Echo Location Protocol).
pub const BATADV_ELP: u32 = 0x0B;
/// OGM v2.
pub const BATADV_OGM2: u32 = 0x0C;

// ---------------------------------------------------------------------------
// TT (Translation Table) flags
// ---------------------------------------------------------------------------

/// TT change: add entry.
pub const BATADV_TT_CHANGE_ADD: u32 = 1 << 0;
/// TT change: delete entry.
pub const BATADV_TT_CHANGE_DEL: u32 = 1 << 1;
/// TT change: full table.
pub const BATADV_TT_CHANGE_FULL: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routing_algos_distinct() {
        assert_ne!(BATADV_ROUTING_ALGO_IV, BATADV_ROUTING_ALGO_V);
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
    fn test_packet_types_distinct() {
        let types = [
            BATADV_OGM, BATADV_ICMP, BATADV_UNICAST,
            BATADV_BCAST, BATADV_VIS, BATADV_UNICAST_FRAG,
            BATADV_TT_QUERY, BATADV_ROAM_ADV,
            BATADV_UNICAST_4ADDR, BATADV_CODED,
            BATADV_ELP, BATADV_OGM2,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_tt_flags_power_of_two() {
        assert!(BATADV_TT_CHANGE_ADD.is_power_of_two());
        assert!(BATADV_TT_CHANGE_DEL.is_power_of_two());
        assert!(BATADV_TT_CHANGE_FULL.is_power_of_two());
    }

    #[test]
    fn test_tt_flags_no_overlap() {
        let flags = [BATADV_TT_CHANGE_ADD, BATADV_TT_CHANGE_DEL, BATADV_TT_CHANGE_FULL];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
