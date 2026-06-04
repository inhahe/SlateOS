//! `<linux/if_bonding.h>` — bonding-driver numeric enums.
//!
//! Where `linux_bonding2_user_types` covers the human-readable sysfs
//! strings, this module pins the underlying numeric codes that the
//! kernel uses internally and that the netlink IFLA_BOND_* payload
//! carries.

// ---------------------------------------------------------------------------
// Bonding modes (`BOND_MODE_*`)
// ---------------------------------------------------------------------------

pub const BOND_MODE_ROUNDROBIN: u32 = 0;
pub const BOND_MODE_ACTIVEBACKUP: u32 = 1;
pub const BOND_MODE_XOR: u32 = 2;
pub const BOND_MODE_BROADCAST: u32 = 3;
pub const BOND_MODE_8023AD: u32 = 4;
pub const BOND_MODE_TLB: u32 = 5;
pub const BOND_MODE_ALB: u32 = 6;

// ---------------------------------------------------------------------------
// LACP rate (`AD_LACP_*`)
// ---------------------------------------------------------------------------

pub const AD_LACP_SLOW: u32 = 0;
pub const AD_LACP_FAST: u32 = 1;

// ---------------------------------------------------------------------------
// xmit-hash policies (`BOND_XMIT_POLICY_*`)
// ---------------------------------------------------------------------------

pub const BOND_XMIT_POLICY_LAYER2: u32 = 0;
pub const BOND_XMIT_POLICY_LAYER34: u32 = 1;
pub const BOND_XMIT_POLICY_LAYER23: u32 = 2;
pub const BOND_XMIT_POLICY_ENCAP23: u32 = 3;
pub const BOND_XMIT_POLICY_ENCAP34: u32 = 4;
pub const BOND_XMIT_POLICY_VLAN_SRCMAC: u32 = 5;

// ---------------------------------------------------------------------------
// arp_validate (`BOND_ARP_VALIDATE_*`)
// ---------------------------------------------------------------------------

pub const BOND_ARP_VALIDATE_NONE: u32 = 0;
pub const BOND_ARP_VALIDATE_ACTIVE: u32 = 1;
pub const BOND_ARP_VALIDATE_BACKUP: u32 = 2;
pub const BOND_ARP_VALIDATE_ALL: u32 = 3;
pub const BOND_ARP_VALIDATE_FILTER: u32 = 4;
pub const BOND_ARP_VALIDATE_FILTER_BACKUP: u32 = 5;
pub const BOND_ARP_VALIDATE_FILTER_ACTIVE: u32 = 6;

// ---------------------------------------------------------------------------
// primary_reselect (`BOND_PRI_RESELECT_*`)
// ---------------------------------------------------------------------------

pub const BOND_PRI_RESELECT_ALWAYS: u32 = 0;
pub const BOND_PRI_RESELECT_BETTER: u32 = 1;
pub const BOND_PRI_RESELECT_FAILURE: u32 = 2;

// ---------------------------------------------------------------------------
// fail_over_mac (`BOND_FOM_*`)
// ---------------------------------------------------------------------------

pub const BOND_FOM_NONE: u32 = 0;
pub const BOND_FOM_ACTIVE: u32 = 1;
pub const BOND_FOM_FOLLOW: u32 = 2;

// ---------------------------------------------------------------------------
// Default link-monitoring intervals (ms)
// ---------------------------------------------------------------------------

/// Default miimon (link-monitor) period — 100 ms.
pub const BOND_DEFAULT_MIIMON_MS: u32 = 100;

/// Maximum miimon value the kernel accepts.
pub const BOND_MAX_MIIMON_MS: u32 = i32::MAX as u32;

/// Maximum number of arp_ip_targets per bond.
pub const BOND_MAX_ARP_TARGETS: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_dense_0_to_6() {
        let m = [
            BOND_MODE_ROUNDROBIN,
            BOND_MODE_ACTIVEBACKUP,
            BOND_MODE_XOR,
            BOND_MODE_BROADCAST,
            BOND_MODE_8023AD,
            BOND_MODE_TLB,
            BOND_MODE_ALB,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // Mode 0 is the historical default.
        assert_eq!(BOND_MODE_ROUNDROBIN, 0);
    }

    #[test]
    fn test_lacp_rate_pair() {
        assert_eq!(AD_LACP_SLOW, 0);
        assert_eq!(AD_LACP_FAST, 1);
    }

    #[test]
    fn test_xmit_policies_dense_0_to_5() {
        let p = [
            BOND_XMIT_POLICY_LAYER2,
            BOND_XMIT_POLICY_LAYER34,
            BOND_XMIT_POLICY_LAYER23,
            BOND_XMIT_POLICY_ENCAP23,
            BOND_XMIT_POLICY_ENCAP34,
            BOND_XMIT_POLICY_VLAN_SRCMAC,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // (Layer34 sits at 1 — the historical second mode added to
        // the original Layer2 default.)
        assert_eq!(BOND_XMIT_POLICY_LAYER34, 1);
    }

    #[test]
    fn test_arp_validate_dense_0_to_6() {
        let v = [
            BOND_ARP_VALIDATE_NONE,
            BOND_ARP_VALIDATE_ACTIVE,
            BOND_ARP_VALIDATE_BACKUP,
            BOND_ARP_VALIDATE_ALL,
            BOND_ARP_VALIDATE_FILTER,
            BOND_ARP_VALIDATE_FILTER_BACKUP,
            BOND_ARP_VALIDATE_FILTER_ACTIVE,
        ];
        for (i, &x) in v.iter().enumerate() {
            assert_eq!(x as usize, i);
        }
    }

    #[test]
    fn test_pri_reselect_and_fom_dense() {
        for (i, &v) in [
            BOND_PRI_RESELECT_ALWAYS,
            BOND_PRI_RESELECT_BETTER,
            BOND_PRI_RESELECT_FAILURE,
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(v as usize, i);
        }
        for (i, &v) in [BOND_FOM_NONE, BOND_FOM_ACTIVE, BOND_FOM_FOLLOW]
            .iter()
            .enumerate()
        {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_link_monitor_bounds() {
        assert_eq!(BOND_DEFAULT_MIIMON_MS, 100);
        assert_eq!(BOND_MAX_MIIMON_MS, i32::MAX as u32);
        assert!(BOND_DEFAULT_MIIMON_MS < BOND_MAX_MIIMON_MS);
        // 16 ARP targets — historical kernel max.
        assert_eq!(BOND_MAX_ARP_TARGETS, 16);
        assert!(BOND_MAX_ARP_TARGETS.is_power_of_two());
    }
}
