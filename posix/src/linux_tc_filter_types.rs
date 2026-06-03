//! `<linux/pkt_cls.h>` — Traffic control filter protocol constants.
//!
//! TC filters classify packets by matching protocol headers.
//! These constants define filter protocols, priorities, and
//! u32/basic/matchall classifier attributes.

// ---------------------------------------------------------------------------
// Filter protocols (for tc filter add ... protocol)
// ---------------------------------------------------------------------------

/// Match all protocols (ETH_P_ALL).
pub const TC_FILTER_PROTO_ALL: u16 = 0x0003;
/// Match IPv4 (ETH_P_IP).
pub const TC_FILTER_PROTO_IP: u16 = 0x0800;
/// Match IPv6 (ETH_P_IPV6).
pub const TC_FILTER_PROTO_IPV6: u16 = 0x86DD;
/// Match ARP (ETH_P_ARP).
pub const TC_FILTER_PROTO_ARP: u16 = 0x0806;
/// Match 802.1Q VLAN (ETH_P_8021Q).
pub const TC_FILTER_PROTO_VLAN: u16 = 0x8100;

// ---------------------------------------------------------------------------
// Filter priorities
// ---------------------------------------------------------------------------

/// Highest priority.
pub const TC_FILTER_PRIO_MAX: u16 = 1;
/// Lowest priority.
pub const TC_FILTER_PRIO_MIN: u16 = 0xFFFF;
/// Default filter priority.
pub const TC_FILTER_PRIO_DEFAULT: u16 = 49152;

// ---------------------------------------------------------------------------
// u32 classifier attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_U32_UNSPEC: u32 = 0;
/// Class ID to assign.
pub const TCA_U32_CLASSID: u32 = 1;
/// Hash table.
pub const TCA_U32_HASH: u32 = 2;
/// Link to another filter.
pub const TCA_U32_LINK: u32 = 3;
/// Divisor for hash table.
pub const TCA_U32_DIVISOR: u32 = 4;
/// Selector (match keys).
pub const TCA_U32_SEL: u32 = 5;
/// Police action.
pub const TCA_U32_POLICE: u32 = 6;
/// Action list.
pub const TCA_U32_ACT: u32 = 7;
/// Ingress interface.
pub const TCA_U32_INDEV: u32 = 8;
/// Performance counters.
pub const TCA_U32_PCNT: u32 = 9;
/// Mark.
pub const TCA_U32_MARK: u32 = 10;
/// Flags.
pub const TCA_U32_FLAGS: u32 = 11;

// ---------------------------------------------------------------------------
// Matchall classifier attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_MATCHALL_UNSPEC: u32 = 0;
/// Class ID.
pub const TCA_MATCHALL_CLASSID: u32 = 1;
/// Action list.
pub const TCA_MATCHALL_ACT: u32 = 2;
/// Flags.
pub const TCA_MATCHALL_FLAGS: u32 = 3;
/// Performance counters.
pub const TCA_MATCHALL_PCNT: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            TC_FILTER_PROTO_ALL,
            TC_FILTER_PROTO_IP,
            TC_FILTER_PROTO_IPV6,
            TC_FILTER_PROTO_ARP,
            TC_FILTER_PROTO_VLAN,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_ip_protocol() {
        assert_eq!(TC_FILTER_PROTO_IP, 0x0800);
    }

    #[test]
    fn test_ipv6_protocol() {
        assert_eq!(TC_FILTER_PROTO_IPV6, 0x86DD);
    }

    #[test]
    fn test_priority_range() {
        assert!(TC_FILTER_PRIO_MAX < TC_FILTER_PRIO_MIN);
    }

    #[test]
    fn test_u32_attrs_distinct() {
        let attrs = [
            TCA_U32_UNSPEC,
            TCA_U32_CLASSID,
            TCA_U32_HASH,
            TCA_U32_LINK,
            TCA_U32_DIVISOR,
            TCA_U32_SEL,
            TCA_U32_POLICE,
            TCA_U32_ACT,
            TCA_U32_INDEV,
            TCA_U32_PCNT,
            TCA_U32_MARK,
            TCA_U32_FLAGS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_matchall_attrs_distinct() {
        let attrs = [
            TCA_MATCHALL_UNSPEC,
            TCA_MATCHALL_CLASSID,
            TCA_MATCHALL_ACT,
            TCA_MATCHALL_FLAGS,
            TCA_MATCHALL_PCNT,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
