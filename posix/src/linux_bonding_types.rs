//! `<linux/if_bonding.h>` — Link aggregation (bonding) constants.
//!
//! Linux bonding combines multiple physical NICs into a single logical
//! interface for increased throughput and/or fault tolerance. Different
//! modes provide different load-balancing and failover behaviors.
//! Used in servers, data centers, and high-availability setups.

// ---------------------------------------------------------------------------
// Bonding modes
// ---------------------------------------------------------------------------

/// Round-robin (balance-rr): transmit on slaves in order.
pub const BOND_MODE_ROUNDROBIN: u32 = 0;
/// Active-backup: one active slave, others on standby.
pub const BOND_MODE_ACTIVEBACKUP: u32 = 1;
/// XOR (balance-xor): hash-based transmit slave selection.
pub const BOND_MODE_XOR: u32 = 2;
/// Broadcast: transmit on all slaves.
pub const BOND_MODE_BROADCAST: u32 = 3;
/// 802.3ad (LACP): dynamic link aggregation.
pub const BOND_MODE_8023AD: u32 = 4;
/// Adaptive transmit load balancing (balance-tlb).
pub const BOND_MODE_TLB: u32 = 5;
/// Adaptive load balancing (balance-alb).
pub const BOND_MODE_ALB: u32 = 6;

// ---------------------------------------------------------------------------
// LACP rate
// ---------------------------------------------------------------------------

/// LACP slow rate (30-second interval).
pub const BOND_LACP_SLOW: u32 = 0;
/// LACP fast rate (1-second interval).
pub const BOND_LACP_FAST: u32 = 1;

// ---------------------------------------------------------------------------
// Primary reselection policy
// ---------------------------------------------------------------------------

/// Always use primary when it comes back.
pub const BOND_PRI_RESELECT_ALWAYS: u32 = 0;
/// Use primary only if active slave fails.
pub const BOND_PRI_RESELECT_BETTER: u32 = 1;
/// Stick with current active.
pub const BOND_PRI_RESELECT_FAILURE: u32 = 2;

// ---------------------------------------------------------------------------
// XOR/802.3ad hash policy
// ---------------------------------------------------------------------------

/// Hash on L2 (MAC addresses).
pub const BOND_XMIT_POLICY_LAYER2: u32 = 0;
/// Hash on L3+L4 (IP + port).
pub const BOND_XMIT_POLICY_LAYER34: u32 = 1;
/// Hash on L2+L3 (MAC + IP).
pub const BOND_XMIT_POLICY_LAYER23: u32 = 2;
/// Hash on encapsulated L3+L4.
pub const BOND_XMIT_POLICY_ENCAP23: u32 = 3;
/// Hash on encapsulated L3+L4 (v2).
pub const BOND_XMIT_POLICY_ENCAP34: u32 = 4;
/// VLAN + source MAC hash.
pub const BOND_XMIT_POLICY_VLAN_SRCMAC: u32 = 5;

// ---------------------------------------------------------------------------
// Slave states
// ---------------------------------------------------------------------------

/// Slave is active.
pub const BOND_STATE_ACTIVE: u32 = 0;
/// Slave is backup.
pub const BOND_STATE_BACKUP: u32 = 1;

// ---------------------------------------------------------------------------
// MII monitor link states
// ---------------------------------------------------------------------------

/// Link is up.
pub const BOND_LINK_UP: u32 = 0;
/// Link is down.
pub const BOND_LINK_DOWN: u32 = 1;
/// Link is going down.
pub const BOND_LINK_FAIL: u32 = 2;
/// Link is coming back up.
pub const BOND_LINK_BACK: u32 = 3;

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
    fn test_lacp_rates_distinct() {
        assert_ne!(BOND_LACP_SLOW, BOND_LACP_FAST);
    }

    #[test]
    fn test_hash_policies_distinct() {
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
    fn test_link_states_distinct() {
        let states = [BOND_LINK_UP, BOND_LINK_DOWN, BOND_LINK_FAIL, BOND_LINK_BACK];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
