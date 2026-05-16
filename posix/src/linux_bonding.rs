//! `<linux/if_bonding.h>` — Network bonding (link aggregation) constants.
//!
//! Bonding combines multiple physical network interfaces into a
//! single logical bond for fault tolerance (active-backup) or
//! increased throughput (802.3ad LACP, balance-rr). Configured
//! via sysfs, netlink, or ifenslave.

// ---------------------------------------------------------------------------
// Bonding modes
// ---------------------------------------------------------------------------

/// Round-robin (packet-level load balancing).
pub const BOND_MODE_ROUNDROBIN: u32 = 0;
/// Active-backup (only one slave active).
pub const BOND_MODE_ACTIVEBACKUP: u32 = 1;
/// XOR (source/dest MAC hash).
pub const BOND_MODE_XOR: u32 = 2;
/// Broadcast (transmit on all slaves).
pub const BOND_MODE_BROADCAST: u32 = 3;
/// IEEE 802.3ad LACP.
pub const BOND_MODE_8023AD: u32 = 4;
/// Adaptive transmit load balancing.
pub const BOND_MODE_TLB: u32 = 5;
/// Adaptive load balancing (includes receive).
pub const BOND_MODE_ALB: u32 = 6;

// ---------------------------------------------------------------------------
// Mode name strings
// ---------------------------------------------------------------------------

/// "balance-rr"
pub const BOND_MODE_NAME_ROUNDROBIN: &str = "balance-rr";
/// "active-backup"
pub const BOND_MODE_NAME_ACTIVEBACKUP: &str = "active-backup";
/// "balance-xor"
pub const BOND_MODE_NAME_XOR: &str = "balance-xor";
/// "broadcast"
pub const BOND_MODE_NAME_BROADCAST: &str = "broadcast";
/// "802.3ad"
pub const BOND_MODE_NAME_8023AD: &str = "802.3ad";
/// "balance-tlb"
pub const BOND_MODE_NAME_TLB: &str = "balance-tlb";
/// "balance-alb"
pub const BOND_MODE_NAME_ALB: &str = "balance-alb";

// ---------------------------------------------------------------------------
// LACP rate
// ---------------------------------------------------------------------------

/// Slow LACP (every 30s).
pub const BOND_LACP_SLOW: u32 = 0;
/// Fast LACP (every 1s).
pub const BOND_LACP_FAST: u32 = 1;

// ---------------------------------------------------------------------------
// Link monitoring (MII or ARP)
// ---------------------------------------------------------------------------

/// Default MII monitoring interval (ms).
pub const BOND_DEFAULT_MIIMON: u32 = 100;
/// Default link up delay (ms).
pub const BOND_DEFAULT_UPDELAY: u32 = 0;
/// Default link down delay (ms).
pub const BOND_DEFAULT_DOWNDELAY: u32 = 0;

// ---------------------------------------------------------------------------
// Transmit hash policy
// ---------------------------------------------------------------------------

/// Hash by layer 2 (MAC addresses).
pub const BOND_XMIT_POLICY_LAYER2: u32 = 0;
/// Hash by layer 3+4 (IP + port).
pub const BOND_XMIT_POLICY_LAYER34: u32 = 1;
/// Hash by layer 2+3.
pub const BOND_XMIT_POLICY_LAYER23: u32 = 2;
/// Encapsulated layer 3+4.
pub const BOND_XMIT_POLICY_ENCAP23: u32 = 3;
/// Encapsulated layer 3+4 with VLAN.
pub const BOND_XMIT_POLICY_ENCAP34: u32 = 4;
/// VLAN + source/dest port.
pub const BOND_XMIT_POLICY_VLAN_SRCMAC: u32 = 5;

// ---------------------------------------------------------------------------
// Slave state
// ---------------------------------------------------------------------------

/// Slave is active.
pub const BOND_STATE_ACTIVE: u8 = 0;
/// Slave is backup.
pub const BOND_STATE_BACKUP: u8 = 1;

// ---------------------------------------------------------------------------
// Link type
// ---------------------------------------------------------------------------

/// IFLA_INFO_KIND value for bond.
pub const BOND_KIND: &str = "bond";

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
    fn test_mode_names_distinct() {
        let names = [
            BOND_MODE_NAME_ROUNDROBIN, BOND_MODE_NAME_ACTIVEBACKUP,
            BOND_MODE_NAME_XOR, BOND_MODE_NAME_BROADCAST,
            BOND_MODE_NAME_8023AD, BOND_MODE_NAME_TLB, BOND_MODE_NAME_ALB,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_lacp_rates_distinct() {
        assert_ne!(BOND_LACP_SLOW, BOND_LACP_FAST);
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
    fn test_slave_states_distinct() {
        assert_ne!(BOND_STATE_ACTIVE, BOND_STATE_BACKUP);
    }

    #[test]
    fn test_kind() {
        assert_eq!(BOND_KIND, "bond");
    }
}
