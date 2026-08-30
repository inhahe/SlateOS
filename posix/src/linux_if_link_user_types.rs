//! `<linux/if_link.h>` — rtnetlink IFLA_* link attributes.
//!
//! Every `iproute2` command (`ip link …`), every NetworkManager poll,
//! and every container-runtime virtual-interface setup walks the
//! attribute IDs below over `NETLINK_ROUTE`. The constants line up
//! with `enum` definitions in `<linux/if_link.h>` and remain stable
//! across kernels (new attributes append).

// ---------------------------------------------------------------------------
// IFLA_* attribute IDs (enum)
// ---------------------------------------------------------------------------

pub const IFLA_UNSPEC: u32 = 0;
pub const IFLA_ADDRESS: u32 = 1;
pub const IFLA_BROADCAST: u32 = 2;
pub const IFLA_IFNAME: u32 = 3;
pub const IFLA_MTU: u32 = 4;
pub const IFLA_LINK: u32 = 5;
pub const IFLA_QDISC: u32 = 6;
pub const IFLA_STATS: u32 = 7;
pub const IFLA_COST: u32 = 8;
pub const IFLA_PRIORITY: u32 = 9;
pub const IFLA_MASTER: u32 = 10;
pub const IFLA_WIRELESS: u32 = 11;
pub const IFLA_PROTINFO: u32 = 12;
pub const IFLA_TXQLEN: u32 = 13;
pub const IFLA_MAP: u32 = 14;
pub const IFLA_WEIGHT: u32 = 15;
pub const IFLA_OPERSTATE: u32 = 16;
pub const IFLA_LINKMODE: u32 = 17;
pub const IFLA_LINKINFO: u32 = 18;
pub const IFLA_NET_NS_PID: u32 = 19;
pub const IFLA_IFALIAS: u32 = 20;
pub const IFLA_NUM_VF: u32 = 21;
pub const IFLA_VFINFO_LIST: u32 = 22;
pub const IFLA_STATS64: u32 = 23;
pub const IFLA_VF_PORTS: u32 = 24;
pub const IFLA_PORT_SELF: u32 = 25;
pub const IFLA_AF_SPEC: u32 = 26;
pub const IFLA_GROUP: u32 = 27;
pub const IFLA_NET_NS_FD: u32 = 28;
pub const IFLA_EXT_MASK: u32 = 29;
pub const IFLA_PROMISCUITY: u32 = 30;
pub const IFLA_NUM_TX_QUEUES: u32 = 31;
pub const IFLA_NUM_RX_QUEUES: u32 = 32;
pub const IFLA_CARRIER: u32 = 33;
pub const IFLA_PHYS_PORT_ID: u32 = 34;
pub const IFLA_CARRIER_CHANGES: u32 = 35;
pub const IFLA_PHYS_SWITCH_ID: u32 = 36;
pub const IFLA_LINK_NETNSID: u32 = 37;

// ---------------------------------------------------------------------------
// IFLA_OPERSTATE values (RFC 2863)
// ---------------------------------------------------------------------------

pub const IF_OPER_UNKNOWN: u8 = 0;
pub const IF_OPER_NOTPRESENT: u8 = 1;
pub const IF_OPER_DOWN: u8 = 2;
pub const IF_OPER_LOWERLAYERDOWN: u8 = 3;
pub const IF_OPER_TESTING: u8 = 4;
pub const IF_OPER_DORMANT: u8 = 5;
pub const IF_OPER_UP: u8 = 6;

// ---------------------------------------------------------------------------
// IFLA_LINKMODE
// ---------------------------------------------------------------------------

pub const IF_LINK_MODE_DEFAULT: u8 = 0;
pub const IF_LINK_MODE_DORMANT: u8 = 1;
pub const IF_LINK_MODE_TESTING: u8 = 2;

// ---------------------------------------------------------------------------
// IFLA_LINKINFO sub-attribute IDs
// ---------------------------------------------------------------------------

pub const IFLA_INFO_UNSPEC: u32 = 0;
pub const IFLA_INFO_KIND: u32 = 1;
pub const IFLA_INFO_DATA: u32 = 2;
pub const IFLA_INFO_XSTATS: u32 = 3;
pub const IFLA_INFO_SLAVE_KIND: u32 = 4;
pub const IFLA_INFO_SLAVE_DATA: u32 = 5;

// ---------------------------------------------------------------------------
// Extended-info request bits (IFLA_EXT_MASK)
// ---------------------------------------------------------------------------

pub const RTEXT_FILTER_VF: u32 = 1 << 0;
pub const RTEXT_FILTER_BRVLAN: u32 = 1 << 1;
pub const RTEXT_FILTER_BRVLAN_COMPRESSED: u32 = 1 << 2;
pub const RTEXT_FILTER_SKIP_STATS: u32 = 1 << 3;
pub const RTEXT_FILTER_MRP: u32 = 1 << 4;
pub const RTEXT_FILTER_CFM_CONFIG: u32 = 1 << 5;
pub const RTEXT_FILTER_CFM_STATUS: u32 = 1 << 6;
pub const RTEXT_FILTER_MST: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ifla_main_attrs_dense_0_to_37() {
        let a = [
            IFLA_UNSPEC,
            IFLA_ADDRESS,
            IFLA_BROADCAST,
            IFLA_IFNAME,
            IFLA_MTU,
            IFLA_LINK,
            IFLA_QDISC,
            IFLA_STATS,
            IFLA_COST,
            IFLA_PRIORITY,
            IFLA_MASTER,
            IFLA_WIRELESS,
            IFLA_PROTINFO,
            IFLA_TXQLEN,
            IFLA_MAP,
            IFLA_WEIGHT,
            IFLA_OPERSTATE,
            IFLA_LINKMODE,
            IFLA_LINKINFO,
            IFLA_NET_NS_PID,
            IFLA_IFALIAS,
            IFLA_NUM_VF,
            IFLA_VFINFO_LIST,
            IFLA_STATS64,
            IFLA_VF_PORTS,
            IFLA_PORT_SELF,
            IFLA_AF_SPEC,
            IFLA_GROUP,
            IFLA_NET_NS_FD,
            IFLA_EXT_MASK,
            IFLA_PROMISCUITY,
            IFLA_NUM_TX_QUEUES,
            IFLA_NUM_RX_QUEUES,
            IFLA_CARRIER,
            IFLA_PHYS_PORT_ID,
            IFLA_CARRIER_CHANGES,
            IFLA_PHYS_SWITCH_ID,
            IFLA_LINK_NETNSID,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_oper_state_dense_0_to_6() {
        let s = [
            IF_OPER_UNKNOWN,
            IF_OPER_NOTPRESENT,
            IF_OPER_DOWN,
            IF_OPER_LOWERLAYERDOWN,
            IF_OPER_TESTING,
            IF_OPER_DORMANT,
            IF_OPER_UP,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_link_mode_distinct() {
        assert_ne!(IF_LINK_MODE_DEFAULT, IF_LINK_MODE_DORMANT);
        assert_ne!(IF_LINK_MODE_DORMANT, IF_LINK_MODE_TESTING);
        assert_eq!(IF_LINK_MODE_DEFAULT, 0);
    }

    #[test]
    fn test_linkinfo_sub_attrs_dense() {
        let s = [
            IFLA_INFO_UNSPEC,
            IFLA_INFO_KIND,
            IFLA_INFO_DATA,
            IFLA_INFO_XSTATS,
            IFLA_INFO_SLAVE_KIND,
            IFLA_INFO_SLAVE_DATA,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_rtext_filter_bits_pow2_and_distinct() {
        let r = [
            RTEXT_FILTER_VF,
            RTEXT_FILTER_BRVLAN,
            RTEXT_FILTER_BRVLAN_COMPRESSED,
            RTEXT_FILTER_SKIP_STATS,
            RTEXT_FILTER_MRP,
            RTEXT_FILTER_CFM_CONFIG,
            RTEXT_FILTER_CFM_STATUS,
            RTEXT_FILTER_MST,
        ];
        for &b in &r {
            assert!(b.is_power_of_two());
        }
        for i in 0..r.len() {
            for j in (i + 1)..r.len() {
                assert_ne!(r[i], r[j]);
            }
        }
    }
}
