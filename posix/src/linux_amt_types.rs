//! `<linux/amt.h>` — AMT (Automatic Multicast Tunneling) constants.
//!
//! AMT (RFC 7450) tunnels multicast traffic through unicast UDP
//! between a gateway (source-side) and a relay (receiver-side).
//! This allows multicast delivery across networks that don't
//! support native multicast routing (most of the Internet). The
//! gateway discovers relays via anycast, establishes a UDP tunnel,
//! and forwards IGMP/MLD joins and multicast data. Used for IPTV
//! delivery, multicast VPNs, and inter-domain multicast.

// ---------------------------------------------------------------------------
// AMT message types
// ---------------------------------------------------------------------------

/// Relay Discovery message (relay → gateway).
pub const AMT_MSG_RELAY_DISCOVERY: u32 = 1;
/// Relay Advertisement message (relay → gateway).
pub const AMT_MSG_RELAY_ADVERTISEMENT: u32 = 2;
/// Request message (gateway → relay).
pub const AMT_MSG_REQUEST: u32 = 3;
/// Membership Query message (relay → gateway, carries IGMP/MLD query).
pub const AMT_MSG_MEMBERSHIP_QUERY: u32 = 4;
/// Membership Update message (gateway → relay, carries IGMP/MLD report).
pub const AMT_MSG_MEMBERSHIP_UPDATE: u32 = 5;
/// Multicast Data message (relay → gateway, carries multicast packet).
pub const AMT_MSG_MULTICAST_DATA: u32 = 6;
/// Teardown message (either direction).
pub const AMT_MSG_TEARDOWN: u32 = 7;

// ---------------------------------------------------------------------------
// AMT netlink attributes (IFLA_AMT_*)
// ---------------------------------------------------------------------------

/// AMT mode (gateway or relay).
pub const IFLA_AMT_MODE: u32 = 1;
/// Relay MAC address.
pub const IFLA_AMT_RELAY_PORT: u32 = 2;
/// Gateway port.
pub const IFLA_AMT_GATEWAY_PORT: u32 = 3;
/// Maximum tunnels (relay mode).
pub const IFLA_AMT_MAX_TUNNELS: u32 = 4;
/// Local IP address.
pub const IFLA_AMT_LOCAL_IP: u32 = 5;
/// Remote IP address (relay address for gateway mode).
pub const IFLA_AMT_REMOTE_IP: u32 = 6;
/// Discovery IP (anycast address for relay discovery).
pub const IFLA_AMT_DISCOVERY_IP: u32 = 7;
/// Underlying interface index.
pub const IFLA_AMT_DEV: u32 = 8;

// ---------------------------------------------------------------------------
// AMT modes
// ---------------------------------------------------------------------------

/// AMT gateway mode (sends joins, receives multicast).
pub const AMT_MODE_GATEWAY: u32 = 0;
/// AMT relay mode (receives joins, forwards multicast).
pub const AMT_MODE_RELAY: u32 = 1;

// ---------------------------------------------------------------------------
// AMT default ports
// ---------------------------------------------------------------------------

/// Default AMT UDP port (IANA assigned).
pub const AMT_PORT: u32 = 2268;

// ---------------------------------------------------------------------------
// AMT limits
// ---------------------------------------------------------------------------

/// Default maximum tunnels for a relay.
pub const AMT_MAX_TUNNELS_DEFAULT: u32 = 128;
/// Maximum relay discovery retries.
pub const AMT_MAX_DISCOVERY_RETRIES: u32 = 3;
/// Relay discovery timeout (seconds).
pub const AMT_DISCOVERY_TIMEOUT: u32 = 30;
/// Request timeout (seconds).
pub const AMT_REQUEST_TIMEOUT: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_distinct() {
        let msgs = [
            AMT_MSG_RELAY_DISCOVERY,
            AMT_MSG_RELAY_ADVERTISEMENT,
            AMT_MSG_REQUEST,
            AMT_MSG_MEMBERSHIP_QUERY,
            AMT_MSG_MEMBERSHIP_UPDATE,
            AMT_MSG_MULTICAST_DATA,
            AMT_MSG_TEARDOWN,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_ifla_attrs_distinct() {
        let attrs = [
            IFLA_AMT_MODE,
            IFLA_AMT_RELAY_PORT,
            IFLA_AMT_GATEWAY_PORT,
            IFLA_AMT_MAX_TUNNELS,
            IFLA_AMT_LOCAL_IP,
            IFLA_AMT_REMOTE_IP,
            IFLA_AMT_DISCOVERY_IP,
            IFLA_AMT_DEV,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_modes_distinct() {
        assert_ne!(AMT_MODE_GATEWAY, AMT_MODE_RELAY);
    }

    #[test]
    fn test_default_port() {
        assert_eq!(AMT_PORT, 2268);
    }

    #[test]
    fn test_limits_positive() {
        assert!(AMT_MAX_TUNNELS_DEFAULT > 0);
        assert!(AMT_MAX_DISCOVERY_RETRIES > 0);
        assert!(AMT_DISCOVERY_TIMEOUT > 0);
        assert!(AMT_REQUEST_TIMEOUT > 0);
    }

    #[test]
    fn test_timeouts_ordered() {
        // Request timeout should be shorter than discovery timeout
        assert!(AMT_REQUEST_TIMEOUT < AMT_DISCOVERY_TIMEOUT);
    }
}
