//! `<linux/if_bonding.h>` — Additional bonding constants.
//!
//! Supplementary bonding constants covering bonding modes,
//! LACP rates, and primary reselect policies.

// ---------------------------------------------------------------------------
// Bonding modes (BOND_MODE_*)
// ---------------------------------------------------------------------------

/// Round-robin.
pub const BOND_MODE_ROUNDROBIN: u32 = 0;
/// Active-backup.
pub const BOND_MODE_ACTIVEBACKUP: u32 = 1;
/// XOR.
pub const BOND_MODE_XOR: u32 = 2;
/// Broadcast.
pub const BOND_MODE_BROADCAST: u32 = 3;
/// 802.3ad (LACP).
pub const BOND_MODE_8023AD: u32 = 4;
/// Transmit load balancing (TLB).
pub const BOND_MODE_TLB: u32 = 5;
/// Adaptive load balancing (ALB).
pub const BOND_MODE_ALB: u32 = 6;

// ---------------------------------------------------------------------------
// LACP rates
// ---------------------------------------------------------------------------

/// Slow LACP (every 30 seconds).
pub const BOND_LACP_SLOW: u32 = 0;
/// Fast LACP (every 1 second).
pub const BOND_LACP_FAST: u32 = 1;

// ---------------------------------------------------------------------------
// Primary reselect policies
// ---------------------------------------------------------------------------

/// Always reselect.
pub const BOND_PRI_RESELECT_ALWAYS: u32 = 0;
/// Better reselect.
pub const BOND_PRI_RESELECT_BETTER: u32 = 1;
/// Failure reselect.
pub const BOND_PRI_RESELECT_FAILURE: u32 = 2;

// ---------------------------------------------------------------------------
// ARP validate modes
// ---------------------------------------------------------------------------

/// No ARP validation.
pub const BOND_ARP_VALIDATE_NONE: u32 = 0;
/// Validate active only.
pub const BOND_ARP_VALIDATE_ACTIVE: u32 = 1;
/// Validate backup only.
pub const BOND_ARP_VALIDATE_BACKUP: u32 = 2;
/// Validate all.
pub const BOND_ARP_VALIDATE_ALL: u32 = 3;

// ---------------------------------------------------------------------------
// Transmit hash policies
// ---------------------------------------------------------------------------

/// Layer 2 (MAC address) hash.
pub const BOND_XMIT_HASH_LAYER2: u32 = 0;
/// Layer 3+4 (IP + port) hash.
pub const BOND_XMIT_HASH_LAYER34: u32 = 1;
/// Layer 2+3 hash.
pub const BOND_XMIT_HASH_LAYER23: u32 = 2;
/// Encap layer 2+3 hash.
pub const BOND_XMIT_HASH_ENCAP23: u32 = 3;
/// Encap layer 3+4 hash.
pub const BOND_XMIT_HASH_ENCAP34: u32 = 4;
/// VLAN + source MAC hash.
pub const BOND_XMIT_HASH_VLAN_SRCMAC: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            BOND_MODE_ROUNDROBIN,
            BOND_MODE_ACTIVEBACKUP,
            BOND_MODE_XOR,
            BOND_MODE_BROADCAST,
            BOND_MODE_8023AD,
            BOND_MODE_TLB,
            BOND_MODE_ALB,
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
    fn test_pri_reselect_distinct() {
        let policies = [
            BOND_PRI_RESELECT_ALWAYS,
            BOND_PRI_RESELECT_BETTER,
            BOND_PRI_RESELECT_FAILURE,
        ];
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
            }
        }
    }

    #[test]
    fn test_arp_validate_distinct() {
        let modes = [
            BOND_ARP_VALIDATE_NONE,
            BOND_ARP_VALIDATE_ACTIVE,
            BOND_ARP_VALIDATE_BACKUP,
            BOND_ARP_VALIDATE_ALL,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_xmit_hash_distinct() {
        let hashes = [
            BOND_XMIT_HASH_LAYER2,
            BOND_XMIT_HASH_LAYER34,
            BOND_XMIT_HASH_LAYER23,
            BOND_XMIT_HASH_ENCAP23,
            BOND_XMIT_HASH_ENCAP34,
            BOND_XMIT_HASH_VLAN_SRCMAC,
        ];
        for i in 0..hashes.len() {
            for j in (i + 1)..hashes.len() {
                assert_ne!(hashes[i], hashes[j]);
            }
        }
    }
}
