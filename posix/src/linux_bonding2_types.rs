//! `<linux/if_bonding.h>` — Network bonding constants (extended).
//!
//! Extended bonding constants covering bonding modes,
//! LACP parameters, ARP monitoring options, and
//! bond slave attributes.

// ---------------------------------------------------------------------------
// Bonding modes (BOND_MODE_*)
// ---------------------------------------------------------------------------

/// Round-robin (balance-rr).
pub const BOND_MODE_ROUNDROBIN: u32 = 0;
/// Active-backup.
pub const BOND_MODE_ACTIVEBACKUP: u32 = 1;
/// XOR (balance-xor).
pub const BOND_MODE_XOR: u32 = 2;
/// Broadcast.
pub const BOND_MODE_BROADCAST: u32 = 3;
/// IEEE 802.3ad (LACP).
pub const BOND_MODE_8023AD: u32 = 4;
/// Adaptive transmit load balancing.
pub const BOND_MODE_TLB: u32 = 5;
/// Adaptive load balancing.
pub const BOND_MODE_ALB: u32 = 6;

// ---------------------------------------------------------------------------
// Bond xmit hash policy
// ---------------------------------------------------------------------------

/// Layer 2 (MAC) hash.
pub const BOND_XMIT_POLICY_LAYER2: u32 = 0;
/// Layer 3+4 (IP+port) hash.
pub const BOND_XMIT_POLICY_LAYER34: u32 = 1;
/// Layer 2+3 hash.
pub const BOND_XMIT_POLICY_LAYER23: u32 = 2;
/// Encapsulated layer 2+3 hash.
pub const BOND_XMIT_POLICY_ENCAP23: u32 = 3;
/// Encapsulated layer 3+4 hash.
pub const BOND_XMIT_POLICY_ENCAP34: u32 = 4;
/// VLAN + SRC-MAC hash.
pub const BOND_XMIT_POLICY_VLAN_SRCMAC: u32 = 5;

// ---------------------------------------------------------------------------
// LACP rate
// ---------------------------------------------------------------------------

/// Slow LACP rate (30s).
pub const BOND_LACP_RATE_SLOW: u32 = 0;
/// Fast LACP rate (1s).
pub const BOND_LACP_RATE_FAST: u32 = 1;

// ---------------------------------------------------------------------------
// Ad select
// ---------------------------------------------------------------------------

/// Stable (default).
pub const BOND_AD_SELECT_STABLE: u32 = 0;
/// Bandwidth.
pub const BOND_AD_SELECT_BANDWIDTH: u32 = 1;
/// Count.
pub const BOND_AD_SELECT_COUNT: u32 = 2;

// ---------------------------------------------------------------------------
// ARP validate
// ---------------------------------------------------------------------------

/// No ARP validation.
pub const BOND_ARP_VALIDATE_NONE: u32 = 0;
/// Validate active only.
pub const BOND_ARP_VALIDATE_ACTIVE: u32 = 1;
/// Validate backup only.
pub const BOND_ARP_VALIDATE_BACKUP: u32 = 2;
/// Validate all.
pub const BOND_ARP_VALIDATE_ALL: u32 = 3;
/// Filter active.
pub const BOND_ARP_FILTER_ACTIVE: u32 = 4;
/// Filter backup.
pub const BOND_ARP_FILTER_BACKUP: u32 = 5;

// ---------------------------------------------------------------------------
// Fail-over MAC policy
// ---------------------------------------------------------------------------

/// No fail-over MAC.
pub const BOND_FOM_NONE: u32 = 0;
/// Active MAC.
pub const BOND_FOM_ACTIVE: u32 = 1;
/// Follow MAC.
pub const BOND_FOM_FOLLOW: u32 = 2;

// ---------------------------------------------------------------------------
// Primary reselect
// ---------------------------------------------------------------------------

/// Always reselect.
pub const BOND_PRI_RESELECT_ALWAYS: u32 = 0;
/// Better reselect.
pub const BOND_PRI_RESELECT_BETTER: u32 = 1;
/// Failure reselect.
pub const BOND_PRI_RESELECT_FAILURE: u32 = 2;

// ---------------------------------------------------------------------------
// Bond slave state
// ---------------------------------------------------------------------------

/// Slave is active.
pub const BOND_STATE_ACTIVE: u32 = 0;
/// Slave is backup.
pub const BOND_STATE_BACKUP: u32 = 1;

// ---------------------------------------------------------------------------
// Bond link state
// ---------------------------------------------------------------------------

/// Link is up.
pub const BOND_LINK_UP: u32 = 0;
/// Link is down.
pub const BOND_LINK_DOWN: u32 = 1;
/// Link failure.
pub const BOND_LINK_FAIL: u32 = 2;
/// Link coming back.
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
    fn test_xmit_policies_distinct() {
        let pols = [
            BOND_XMIT_POLICY_LAYER2, BOND_XMIT_POLICY_LAYER34,
            BOND_XMIT_POLICY_LAYER23, BOND_XMIT_POLICY_ENCAP23,
            BOND_XMIT_POLICY_ENCAP34, BOND_XMIT_POLICY_VLAN_SRCMAC,
        ];
        for i in 0..pols.len() {
            for j in (i + 1)..pols.len() {
                assert_ne!(pols[i], pols[j]);
            }
        }
    }

    #[test]
    fn test_lacp_rates_distinct() {
        assert_ne!(BOND_LACP_RATE_SLOW, BOND_LACP_RATE_FAST);
    }

    #[test]
    fn test_arp_validate_distinct() {
        let vals = [
            BOND_ARP_VALIDATE_NONE, BOND_ARP_VALIDATE_ACTIVE,
            BOND_ARP_VALIDATE_BACKUP, BOND_ARP_VALIDATE_ALL,
            BOND_ARP_FILTER_ACTIVE, BOND_ARP_FILTER_BACKUP,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_fom_distinct() {
        let foms = [BOND_FOM_NONE, BOND_FOM_ACTIVE, BOND_FOM_FOLLOW];
        for i in 0..foms.len() {
            for j in (i + 1)..foms.len() {
                assert_ne!(foms[i], foms[j]);
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

    #[test]
    fn test_slave_states_distinct() {
        assert_ne!(BOND_STATE_ACTIVE, BOND_STATE_BACKUP);
    }

    #[test]
    fn test_roundrobin_is_zero() {
        assert_eq!(BOND_MODE_ROUNDROBIN, 0);
    }

    #[test]
    fn test_ad_select_distinct() {
        let sels = [
            BOND_AD_SELECT_STABLE, BOND_AD_SELECT_BANDWIDTH,
            BOND_AD_SELECT_COUNT,
        ];
        for i in 0..sels.len() {
            for j in (i + 1)..sels.len() {
                assert_ne!(sels[i], sels[j]);
            }
        }
    }
}
