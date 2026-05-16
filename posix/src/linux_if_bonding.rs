//! `<linux/if_bonding.h>` — Network bonding (link aggregation) constants.
//!
//! Linux bonding combines multiple network interfaces into a single
//! logical interface for redundancy (active-backup) or throughput
//! (balance-rr, 802.3ad LACP). Used by `ip link add type bond`
//! and NetworkManager.

// ---------------------------------------------------------------------------
// Bonding modes
// ---------------------------------------------------------------------------

/// Round-robin.
pub const BOND_MODE_ROUNDROBIN: i32 = 0;
/// Active-backup.
pub const BOND_MODE_ACTIVEBACKUP: i32 = 1;
/// XOR (balance-xor).
pub const BOND_MODE_XOR: i32 = 2;
/// Broadcast.
pub const BOND_MODE_BROADCAST: i32 = 3;
/// IEEE 802.3ad LACP.
pub const BOND_MODE_8023AD: i32 = 4;
/// Adaptive transmit load balancing.
pub const BOND_MODE_TLB: i32 = 5;
/// Adaptive load balancing.
pub const BOND_MODE_ALB: i32 = 6;

// ---------------------------------------------------------------------------
// Link monitoring
// ---------------------------------------------------------------------------

/// No link monitoring.
pub const BOND_LINK_MON_NONE: i32 = 0;
/// MII monitoring (carrier detect).
pub const BOND_LINK_MON_MII: i32 = 1;
/// ARP monitoring.
pub const BOND_LINK_MON_ARP: i32 = 2;

// ---------------------------------------------------------------------------
// Slave states
// ---------------------------------------------------------------------------

/// Slave is up.
pub const BOND_STATE_ACTIVE: i32 = 0;
/// Slave is backup.
pub const BOND_STATE_BACKUP: i32 = 1;

// ---------------------------------------------------------------------------
// LACP rate
// ---------------------------------------------------------------------------

/// Slow (30s intervals).
pub const AD_LACP_SLOW: i32 = 0;
/// Fast (1s intervals).
pub const AD_LACP_FAST: i32 = 1;

// ---------------------------------------------------------------------------
// Primary reselect policy
// ---------------------------------------------------------------------------

/// Always reselect primary.
pub const BOND_PRI_RESELECT_ALWAYS: i32 = 0;
/// Reselect only on better primary.
pub const BOND_PRI_RESELECT_BETTER: i32 = 1;
/// Never reselect (failure only).
pub const BOND_PRI_RESELECT_FAILURE: i32 = 2;

// ---------------------------------------------------------------------------
// XOR hash policy
// ---------------------------------------------------------------------------

/// Layer 2 (MAC addresses).
pub const BOND_XMIT_POLICY_LAYER2: i32 = 0;
/// Layer 3+4 (IP + port).
pub const BOND_XMIT_POLICY_LAYER34: i32 = 1;
/// Layer 2+3 (MAC + IP).
pub const BOND_XMIT_POLICY_LAYER23: i32 = 2;
/// Encapsulated layer 2+3.
pub const BOND_XMIT_POLICY_ENCAP23: i32 = 3;
/// Encapsulated layer 3+4.
pub const BOND_XMIT_POLICY_ENCAP34: i32 = 4;
/// VLAN + source MAC.
pub const BOND_XMIT_POLICY_VLAN_SRCMAC: i32 = 5;

// ---------------------------------------------------------------------------
// Fail-over MAC policy
// ---------------------------------------------------------------------------

/// No fail-over MAC.
pub const BOND_FOM_NONE: i32 = 0;
/// Active slave MAC.
pub const BOND_FOM_ACTIVE: i32 = 1;
/// Follow primary.
pub const BOND_FOM_FOLLOW: i32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            BOND_MODE_ROUNDROBIN, BOND_MODE_ACTIVEBACKUP,
            BOND_MODE_XOR, BOND_MODE_BROADCAST,
            BOND_MODE_8023AD, BOND_MODE_TLB, BOND_MODE_ALB,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_mode_values() {
        assert_eq!(BOND_MODE_ROUNDROBIN, 0);
        assert_eq!(BOND_MODE_8023AD, 4);
        assert_eq!(BOND_MODE_ALB, 6);
    }

    #[test]
    fn test_slave_states() {
        assert_eq!(BOND_STATE_ACTIVE, 0);
        assert_eq!(BOND_STATE_BACKUP, 1);
    }

    #[test]
    fn test_lacp_rate() {
        assert_eq!(AD_LACP_SLOW, 0);
        assert_eq!(AD_LACP_FAST, 1);
    }

    #[test]
    fn test_xmit_policies_distinct() {
        let policies = [
            BOND_XMIT_POLICY_LAYER2, BOND_XMIT_POLICY_LAYER34,
            BOND_XMIT_POLICY_LAYER23, BOND_XMIT_POLICY_ENCAP23,
            BOND_XMIT_POLICY_ENCAP34, BOND_XMIT_POLICY_VLAN_SRCMAC,
        ];
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
            }
        }
    }

    #[test]
    fn test_pri_reselect() {
        assert_eq!(BOND_PRI_RESELECT_ALWAYS, 0);
        assert_eq!(BOND_PRI_RESELECT_BETTER, 1);
        assert_eq!(BOND_PRI_RESELECT_FAILURE, 2);
    }
}
