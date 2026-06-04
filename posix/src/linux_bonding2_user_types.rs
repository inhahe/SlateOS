//! Linux bonding-driver sysfs interface (`/sys/class/net/<bondN>/bonding/`).
//!
//! Before iproute2 grew first-class IFLA_BOND_* netlink support, the
//! bonding driver was configured entirely through sysfs string
//! files: `echo balance-rr > /sys/.../mode` etc. Distributions
//! (`/etc/network/interfaces`, NetworkManager bond options) still
//! emit these legacy strings, and the kernel still accepts them.

// ---------------------------------------------------------------------------
// Mode strings — what userspace writes to `bonding/mode`
// ---------------------------------------------------------------------------

pub const BOND_MODE_BALANCE_RR: &str = "balance-rr";
pub const BOND_MODE_ACTIVE_BACKUP: &str = "active-backup";
pub const BOND_MODE_BALANCE_XOR: &str = "balance-xor";
pub const BOND_MODE_BROADCAST: &str = "broadcast";
pub const BOND_MODE_802_3AD: &str = "802.3ad";
pub const BOND_MODE_BALANCE_TLB: &str = "balance-tlb";
pub const BOND_MODE_BALANCE_ALB: &str = "balance-alb";

// ---------------------------------------------------------------------------
// LACP-rate strings (`bonding/lacp_rate`)
// ---------------------------------------------------------------------------

pub const BOND_LACP_RATE_SLOW: &str = "slow";
pub const BOND_LACP_RATE_FAST: &str = "fast";

// ---------------------------------------------------------------------------
// xmit-hash-policy strings (`bonding/xmit_hash_policy`)
// ---------------------------------------------------------------------------

pub const BOND_XMIT_LAYER2: &str = "layer2";
pub const BOND_XMIT_LAYER2_3: &str = "layer2+3";
pub const BOND_XMIT_LAYER3_4: &str = "layer3+4";
pub const BOND_XMIT_ENCAP2_3: &str = "encap2+3";
pub const BOND_XMIT_ENCAP3_4: &str = "encap3+4";
pub const BOND_XMIT_VLAN_SRCMAC: &str = "vlan+srcmac";

// ---------------------------------------------------------------------------
// arp_validate strings (`bonding/arp_validate`)
// ---------------------------------------------------------------------------

pub const BOND_ARP_VALIDATE_NONE: &str = "none";
pub const BOND_ARP_VALIDATE_ACTIVE: &str = "active";
pub const BOND_ARP_VALIDATE_BACKUP: &str = "backup";
pub const BOND_ARP_VALIDATE_ALL: &str = "all";
pub const BOND_ARP_VALIDATE_FILTER: &str = "filter";
pub const BOND_ARP_VALIDATE_FILTER_ACTIVE: &str = "filter_active";
pub const BOND_ARP_VALIDATE_FILTER_BACKUP: &str = "filter_backup";

// ---------------------------------------------------------------------------
// sysfs attribute names under `/sys/class/net/<bondN>/bonding/`
// ---------------------------------------------------------------------------

pub const BOND_ATTR_MODE: &str = "mode";
pub const BOND_ATTR_SLAVES: &str = "slaves";
pub const BOND_ATTR_ACTIVE_SLAVE: &str = "active_slave";
pub const BOND_ATTR_MIIMON: &str = "miimon";
pub const BOND_ATTR_LACP_RATE: &str = "lacp_rate";
pub const BOND_ATTR_XMIT_HASH_POLICY: &str = "xmit_hash_policy";
pub const BOND_ATTR_ARP_INTERVAL: &str = "arp_interval";
pub const BOND_ATTR_ARP_IP_TARGET: &str = "arp_ip_target";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_strings_distinct() {
        let m = [
            BOND_MODE_BALANCE_RR,
            BOND_MODE_ACTIVE_BACKUP,
            BOND_MODE_BALANCE_XOR,
            BOND_MODE_BROADCAST,
            BOND_MODE_802_3AD,
            BOND_MODE_BALANCE_TLB,
            BOND_MODE_BALANCE_ALB,
        ];
        for (i, &x) in m.iter().enumerate() {
            for &y in &m[i + 1..] {
                assert_ne!(x, y);
            }
            assert!(!x.is_empty());
        }
        // The three balance-* modes share a prefix.
        for &v in &[
            BOND_MODE_BALANCE_RR,
            BOND_MODE_BALANCE_XOR,
            BOND_MODE_BALANCE_TLB,
            BOND_MODE_BALANCE_ALB,
        ] {
            assert!(v.starts_with("balance-"));
        }
        // 802.3ad is special — the only mode with a numeric prefix.
        assert_eq!(BOND_MODE_802_3AD, "802.3ad");
    }

    #[test]
    fn test_lacp_rate_pair() {
        assert_eq!(BOND_LACP_RATE_SLOW, "slow");
        assert_eq!(BOND_LACP_RATE_FAST, "fast");
        assert_ne!(BOND_LACP_RATE_SLOW, BOND_LACP_RATE_FAST);
    }

    #[test]
    fn test_xmit_hash_policy_strings_distinct() {
        let x = [
            BOND_XMIT_LAYER2,
            BOND_XMIT_LAYER2_3,
            BOND_XMIT_LAYER3_4,
            BOND_XMIT_ENCAP2_3,
            BOND_XMIT_ENCAP3_4,
            BOND_XMIT_VLAN_SRCMAC,
        ];
        for (i, &a) in x.iter().enumerate() {
            for &b in &x[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // The layer/encap pairs use the same "+N" suffix for the
        // upper protocol layer.
        assert!(BOND_XMIT_LAYER2_3.ends_with("+3"));
        assert!(BOND_XMIT_ENCAP2_3.ends_with("+3"));
        assert!(BOND_XMIT_LAYER3_4.ends_with("+4"));
        assert!(BOND_XMIT_ENCAP3_4.ends_with("+4"));
    }

    #[test]
    fn test_arp_validate_strings_distinct() {
        let v = [
            BOND_ARP_VALIDATE_NONE,
            BOND_ARP_VALIDATE_ACTIVE,
            BOND_ARP_VALIDATE_BACKUP,
            BOND_ARP_VALIDATE_ALL,
            BOND_ARP_VALIDATE_FILTER,
            BOND_ARP_VALIDATE_FILTER_ACTIVE,
            BOND_ARP_VALIDATE_FILTER_BACKUP,
        ];
        for (i, &a) in v.iter().enumerate() {
            for &b in &v[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // The filter_* family extends "filter".
        for &s in &[
            BOND_ARP_VALIDATE_FILTER,
            BOND_ARP_VALIDATE_FILTER_ACTIVE,
            BOND_ARP_VALIDATE_FILTER_BACKUP,
        ] {
            assert!(s.starts_with("filter"));
        }
    }

    #[test]
    fn test_sysfs_attr_names_distinct_and_lowercase() {
        let a = [
            BOND_ATTR_MODE,
            BOND_ATTR_SLAVES,
            BOND_ATTR_ACTIVE_SLAVE,
            BOND_ATTR_MIIMON,
            BOND_ATTR_LACP_RATE,
            BOND_ATTR_XMIT_HASH_POLICY,
            BOND_ATTR_ARP_INTERVAL,
            BOND_ATTR_ARP_IP_TARGET,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
            for ch in x.chars() {
                assert!(ch.is_ascii_lowercase() || ch == '_');
            }
        }
    }
}
