//! `<linux/if_link.h>` IFLA_BOND_* — netlink attribute IDs for the
//! Linux bonding driver.
//!
//! `ip link add bond0 type bond` walks this attribute table when
//! programming the bonding driver from `iproute2`. Each attribute
//! corresponds to a knob also exposed through legacy sysfs
//! (`/sys/class/net/bondN/bonding/<attr>`).

// ---------------------------------------------------------------------------
// IFLA_BOND_* attribute IDs (dense from 0..30)
// ---------------------------------------------------------------------------

pub const IFLA_BOND_UNSPEC: u32 = 0;
pub const IFLA_BOND_MODE: u32 = 1;
pub const IFLA_BOND_ACTIVE_SLAVE: u32 = 2;
pub const IFLA_BOND_MIIMON: u32 = 3;
pub const IFLA_BOND_UPDELAY: u32 = 4;
pub const IFLA_BOND_DOWNDELAY: u32 = 5;
pub const IFLA_BOND_USE_CARRIER: u32 = 6;
pub const IFLA_BOND_ARP_INTERVAL: u32 = 7;
pub const IFLA_BOND_ARP_IP_TARGET: u32 = 8;
pub const IFLA_BOND_ARP_VALIDATE: u32 = 9;
pub const IFLA_BOND_ARP_ALL_TARGETS: u32 = 10;
pub const IFLA_BOND_PRIMARY: u32 = 11;
pub const IFLA_BOND_PRIMARY_RESELECT: u32 = 12;
pub const IFLA_BOND_FAIL_OVER_MAC: u32 = 13;
pub const IFLA_BOND_XMIT_HASH_POLICY: u32 = 14;
pub const IFLA_BOND_RESEND_IGMP: u32 = 15;
pub const IFLA_BOND_NUM_PEER_NOTIF: u32 = 16;
pub const IFLA_BOND_ALL_SLAVES_ACTIVE: u32 = 17;
pub const IFLA_BOND_MIN_LINKS: u32 = 18;
pub const IFLA_BOND_LP_INTERVAL: u32 = 19;
pub const IFLA_BOND_PACKETS_PER_SLAVE: u32 = 20;
pub const IFLA_BOND_AD_LACP_RATE: u32 = 21;
pub const IFLA_BOND_AD_SELECT: u32 = 22;
pub const IFLA_BOND_AD_INFO: u32 = 23;
pub const IFLA_BOND_AD_ACTOR_SYS_PRIO: u32 = 24;
pub const IFLA_BOND_AD_USER_PORT_KEY: u32 = 25;
pub const IFLA_BOND_AD_ACTOR_SYSTEM: u32 = 26;
pub const IFLA_BOND_TLB_DYNAMIC_LB: u32 = 27;
pub const IFLA_BOND_PEER_NOTIF_DELAY: u32 = 28;
pub const IFLA_BOND_AD_LACP_ACTIVE: u32 = 29;
pub const IFLA_BOND_MISSED_MAX: u32 = 30;
pub const IFLA_BOND_NS_IP6_TARGET: u32 = 31;

/// One past the last valid attribute ID.
pub const __IFLA_BOND_MAX: u32 = 32;

// ---------------------------------------------------------------------------
// IFLA_BOND_AD_INFO_* sub-attributes
// ---------------------------------------------------------------------------

pub const IFLA_BOND_AD_INFO_UNSPEC: u32 = 0;
pub const IFLA_BOND_AD_INFO_AGGREGATOR: u32 = 1;
pub const IFLA_BOND_AD_INFO_NUM_PORTS: u32 = 2;
pub const IFLA_BOND_AD_INFO_ACTOR_KEY: u32 = 3;
pub const IFLA_BOND_AD_INFO_PARTNER_KEY: u32 = 4;
pub const IFLA_BOND_AD_INFO_PARTNER_MAC: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attr_ids_dense_0_to_31() {
        let a = [
            IFLA_BOND_UNSPEC,
            IFLA_BOND_MODE,
            IFLA_BOND_ACTIVE_SLAVE,
            IFLA_BOND_MIIMON,
            IFLA_BOND_UPDELAY,
            IFLA_BOND_DOWNDELAY,
            IFLA_BOND_USE_CARRIER,
            IFLA_BOND_ARP_INTERVAL,
            IFLA_BOND_ARP_IP_TARGET,
            IFLA_BOND_ARP_VALIDATE,
            IFLA_BOND_ARP_ALL_TARGETS,
            IFLA_BOND_PRIMARY,
            IFLA_BOND_PRIMARY_RESELECT,
            IFLA_BOND_FAIL_OVER_MAC,
            IFLA_BOND_XMIT_HASH_POLICY,
            IFLA_BOND_RESEND_IGMP,
            IFLA_BOND_NUM_PEER_NOTIF,
            IFLA_BOND_ALL_SLAVES_ACTIVE,
            IFLA_BOND_MIN_LINKS,
            IFLA_BOND_LP_INTERVAL,
            IFLA_BOND_PACKETS_PER_SLAVE,
            IFLA_BOND_AD_LACP_RATE,
            IFLA_BOND_AD_SELECT,
            IFLA_BOND_AD_INFO,
            IFLA_BOND_AD_ACTOR_SYS_PRIO,
            IFLA_BOND_AD_USER_PORT_KEY,
            IFLA_BOND_AD_ACTOR_SYSTEM,
            IFLA_BOND_TLB_DYNAMIC_LB,
            IFLA_BOND_PEER_NOTIF_DELAY,
            IFLA_BOND_AD_LACP_ACTIVE,
            IFLA_BOND_MISSED_MAX,
            IFLA_BOND_NS_IP6_TARGET,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // _MAX is one past the last (32 valid attrs total).
        assert_eq!(__IFLA_BOND_MAX as usize, a.len());
    }

    #[test]
    fn test_ad_info_subattrs_dense_0_to_5() {
        let a = [
            IFLA_BOND_AD_INFO_UNSPEC,
            IFLA_BOND_AD_INFO_AGGREGATOR,
            IFLA_BOND_AD_INFO_NUM_PORTS,
            IFLA_BOND_AD_INFO_ACTOR_KEY,
            IFLA_BOND_AD_INFO_PARTNER_KEY,
            IFLA_BOND_AD_INFO_PARTNER_MAC,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_unspec_is_zero() {
        // Every netlink attribute table starts with UNSPEC = 0.
        assert_eq!(IFLA_BOND_UNSPEC, 0);
        assert_eq!(IFLA_BOND_AD_INFO_UNSPEC, 0);
    }

    #[test]
    fn test_arp_attrs_clustered() {
        // ARP-related attrs (interval, ip_target, validate, all_targets) sit at 7..10.
        for v in [
            IFLA_BOND_ARP_INTERVAL,
            IFLA_BOND_ARP_IP_TARGET,
            IFLA_BOND_ARP_VALIDATE,
            IFLA_BOND_ARP_ALL_TARGETS,
        ] {
            assert!((7..=10).contains(&v));
        }
    }

    #[test]
    fn test_ad_attrs_clustered() {
        // The 802.3ad LACP cluster: lacp_rate(21), ad_select(22), ad_info(23),
        // ad_actor_sys_prio(24), ad_user_port_key(25), ad_actor_system(26),
        // ad_lacp_active(29).
        for v in [
            IFLA_BOND_AD_LACP_RATE,
            IFLA_BOND_AD_SELECT,
            IFLA_BOND_AD_INFO,
            IFLA_BOND_AD_ACTOR_SYS_PRIO,
            IFLA_BOND_AD_USER_PORT_KEY,
            IFLA_BOND_AD_ACTOR_SYSTEM,
            IFLA_BOND_AD_LACP_ACTIVE,
        ] {
            assert!((21..=29).contains(&v));
        }
    }
}
