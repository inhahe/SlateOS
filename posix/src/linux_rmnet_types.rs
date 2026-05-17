//! `<linux/if_rmnet.h>` — RmNet (Qualcomm mobile data) constants.
//!
//! RmNet is the virtual network device driver used on Qualcomm
//! cellular modems (Android phones, mobile hotspots). It multiplexes
//! multiple PDN (Packet Data Network) connections over a single USB
//! or shared-memory link to the modem. Each RmNet device represents
//! a separate APN/data session (internet, IMS/VoLTE, tethering).
//! Configured via netlink; the device handles MAP (Multiplexing and
//! Aggregation Protocol) framing and checksum offload.

// ---------------------------------------------------------------------------
// RmNet netlink attributes (IFLA_RMNET_*)
// ---------------------------------------------------------------------------

/// MUX ID (multiplexer channel ID, 1-254).
pub const IFLA_RMNET_MUX_ID: u32 = 1;
/// RmNet flags (ingress/egress features).
pub const IFLA_RMNET_FLAGS: u32 = 2;

// ---------------------------------------------------------------------------
// RmNet flags
// ---------------------------------------------------------------------------

/// Enable ingress data format deaggregation.
pub const RMNET_FLAGS_INGRESS_DEAGGREGATION: u32 = 1 << 0;
/// Enable ingress MAP commands.
pub const RMNET_FLAGS_INGRESS_MAP_COMMANDS: u32 = 1 << 1;
/// Enable ingress MAP checksum validation.
pub const RMNET_FLAGS_INGRESS_MAP_CKSUMV4: u32 = 1 << 2;
/// Enable egress MAP checksum offload.
pub const RMNET_FLAGS_EGRESS_MAP_CKSUMV4: u32 = 1 << 3;
/// Enable ingress MAP v5 checksum.
pub const RMNET_FLAGS_INGRESS_MAP_CKSUMV5: u32 = 1 << 4;
/// Enable egress MAP v5 checksum.
pub const RMNET_FLAGS_EGRESS_MAP_CKSUMV5: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// MAP header constants
// ---------------------------------------------------------------------------

/// MAP header version.
pub const RMNET_MAP_VERSION: u32 = 1;
/// MAP header command flag.
pub const RMNET_MAP_CMD_FLAG: u32 = 1 << 7;
/// MAP pad bytes mask.
pub const RMNET_MAP_PAD_MASK: u32 = 0x3F;
/// MAP MUX ID mask.
pub const RMNET_MAP_MUX_ID_MASK: u32 = 0xFF;

// ---------------------------------------------------------------------------
// MAP command types
// ---------------------------------------------------------------------------

/// Flow control command.
pub const RMNET_MAP_CMD_FLOW_ENABLE: u32 = 1;
/// Flow disable command.
pub const RMNET_MAP_CMD_FLOW_DISABLE: u32 = 2;

// ---------------------------------------------------------------------------
// MUX ID range
// ---------------------------------------------------------------------------

/// Minimum valid MUX ID.
pub const RMNET_MUX_ID_MIN: u32 = 1;
/// Maximum valid MUX ID.
pub const RMNET_MUX_ID_MAX: u32 = 254;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ifla_attrs_distinct() {
        assert_ne!(IFLA_RMNET_MUX_ID, IFLA_RMNET_FLAGS);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            RMNET_FLAGS_INGRESS_DEAGGREGATION,
            RMNET_FLAGS_INGRESS_MAP_COMMANDS,
            RMNET_FLAGS_INGRESS_MAP_CKSUMV4,
            RMNET_FLAGS_EGRESS_MAP_CKSUMV4,
            RMNET_FLAGS_INGRESS_MAP_CKSUMV5,
            RMNET_FLAGS_EGRESS_MAP_CKSUMV5,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_cmd_types_distinct() {
        assert_ne!(RMNET_MAP_CMD_FLOW_ENABLE, RMNET_MAP_CMD_FLOW_DISABLE);
    }

    #[test]
    fn test_mux_id_range() {
        assert!(RMNET_MUX_ID_MIN < RMNET_MUX_ID_MAX);
        assert!(RMNET_MUX_ID_MIN >= 1);
        assert!(RMNET_MUX_ID_MAX <= 254);
    }

    #[test]
    fn test_map_version() {
        assert_eq!(RMNET_MAP_VERSION, 1);
    }
}
