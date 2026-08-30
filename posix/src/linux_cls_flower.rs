//! `<linux/pkt_cls.h>` (flower) — TC flower classifier constants.
//!
//! The flower classifier is the most widely used TC filter for
//! hardware-offloaded flow matching. It matches on L2-L4 header
//! fields (MAC, VLAN, IP, TCP/UDP, etc.) and supports masked
//! matching for ACL-style rules.

// ---------------------------------------------------------------------------
// Flower match key types (TCA_FLOWER_KEY_*)
// ---------------------------------------------------------------------------

/// Ethernet source MAC.
pub const TCA_FLOWER_KEY_ETH_SRC: u16 = 4;
/// Ethernet destination MAC.
pub const TCA_FLOWER_KEY_ETH_DST: u16 = 3;
/// EtherType.
pub const TCA_FLOWER_KEY_ETH_TYPE: u16 = 8;
/// IP protocol.
pub const TCA_FLOWER_KEY_IP_PROTO: u16 = 9;
/// IPv4 source address.
pub const TCA_FLOWER_KEY_IPV4_SRC: u16 = 10;
/// IPv4 destination address.
pub const TCA_FLOWER_KEY_IPV4_DST: u16 = 11;
/// IPv6 source address.
pub const TCA_FLOWER_KEY_IPV6_SRC: u16 = 14;
/// IPv6 destination address.
pub const TCA_FLOWER_KEY_IPV6_DST: u16 = 15;
/// TCP source port.
pub const TCA_FLOWER_KEY_TCP_SRC: u16 = 16;
/// TCP destination port.
pub const TCA_FLOWER_KEY_TCP_DST: u16 = 17;
/// UDP source port.
pub const TCA_FLOWER_KEY_UDP_SRC: u16 = 18;
/// UDP destination port.
pub const TCA_FLOWER_KEY_UDP_DST: u16 = 19;
/// VLAN ID.
pub const TCA_FLOWER_KEY_VLAN_ID: u16 = 5;
/// VLAN priority.
pub const TCA_FLOWER_KEY_VLAN_PRIO: u16 = 6;
/// VLAN EtherType.
pub const TCA_FLOWER_KEY_VLAN_ETH_TYPE: u16 = 7;
/// IP TOS/DSCP.
pub const TCA_FLOWER_KEY_IP_TOS: u16 = 33;
/// IP TTL.
pub const TCA_FLOWER_KEY_IP_TTL: u16 = 34;
/// TCP flags.
pub const TCA_FLOWER_KEY_TCP_FLAGS: u16 = 35;
/// ICMP type.
pub const TCA_FLOWER_KEY_ICMPV4_TYPE: u16 = 30;
/// ICMP code.
pub const TCA_FLOWER_KEY_ICMPV4_CODE: u16 = 31;

// ---------------------------------------------------------------------------
// Flower flags (TCA_FLOWER_FLAGS)
// ---------------------------------------------------------------------------

/// Skip software processing (hw-only).
pub const TCA_CLS_FLAGS_SKIP_SW: u32 = 1 << 0;
/// Skip hardware offload (sw-only).
pub const TCA_CLS_FLAGS_SKIP_HW: u32 = 1 << 1;
/// In hardware (status flag).
pub const TCA_CLS_FLAGS_IN_HW: u32 = 1 << 2;
/// Not in hardware (status flag).
pub const TCA_CLS_FLAGS_NOT_IN_HW: u32 = 1 << 3;
/// Verbose output.
pub const TCA_CLS_FLAGS_VERBOSE: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Flower tunnel key attributes
// ---------------------------------------------------------------------------

/// Tunnel ID (VNI/VSID).
pub const TCA_FLOWER_KEY_ENC_KEY_ID: u16 = 20;
/// Tunnel IPv4 source.
pub const TCA_FLOWER_KEY_ENC_IPV4_SRC: u16 = 21;
/// Tunnel IPv4 destination.
pub const TCA_FLOWER_KEY_ENC_IPV4_DST: u16 = 22;
/// Tunnel IPv6 source.
pub const TCA_FLOWER_KEY_ENC_IPV6_SRC: u16 = 23;
/// Tunnel IPv6 destination.
pub const TCA_FLOWER_KEY_ENC_IPV6_DST: u16 = 24;
/// Tunnel destination port (e.g., VXLAN 4789).
pub const TCA_FLOWER_KEY_ENC_UDP_DST_PORT: u16 = 43;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keys_distinct() {
        let keys = [
            TCA_FLOWER_KEY_ETH_SRC,
            TCA_FLOWER_KEY_ETH_DST,
            TCA_FLOWER_KEY_ETH_TYPE,
            TCA_FLOWER_KEY_IP_PROTO,
            TCA_FLOWER_KEY_IPV4_SRC,
            TCA_FLOWER_KEY_IPV4_DST,
            TCA_FLOWER_KEY_IPV6_SRC,
            TCA_FLOWER_KEY_IPV6_DST,
            TCA_FLOWER_KEY_TCP_SRC,
            TCA_FLOWER_KEY_TCP_DST,
            TCA_FLOWER_KEY_UDP_SRC,
            TCA_FLOWER_KEY_UDP_DST,
            TCA_FLOWER_KEY_VLAN_ID,
            TCA_FLOWER_KEY_VLAN_PRIO,
            TCA_FLOWER_KEY_VLAN_ETH_TYPE,
            TCA_FLOWER_KEY_IP_TOS,
            TCA_FLOWER_KEY_IP_TTL,
            TCA_FLOWER_KEY_TCP_FLAGS,
            TCA_FLOWER_KEY_ICMPV4_TYPE,
            TCA_FLOWER_KEY_ICMPV4_CODE,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            TCA_CLS_FLAGS_SKIP_SW,
            TCA_CLS_FLAGS_SKIP_HW,
            TCA_CLS_FLAGS_IN_HW,
            TCA_CLS_FLAGS_NOT_IN_HW,
            TCA_CLS_FLAGS_VERBOSE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_tunnel_keys_distinct() {
        let keys = [
            TCA_FLOWER_KEY_ENC_KEY_ID,
            TCA_FLOWER_KEY_ENC_IPV4_SRC,
            TCA_FLOWER_KEY_ENC_IPV4_DST,
            TCA_FLOWER_KEY_ENC_IPV6_SRC,
            TCA_FLOWER_KEY_ENC_IPV6_DST,
            TCA_FLOWER_KEY_ENC_UDP_DST_PORT,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }
}
