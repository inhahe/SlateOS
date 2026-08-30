//! `<linux/netfilter/nf_tables.h>` — nftables constants.
//!
//! nftables is the modern Linux packet classification framework
//! replacing iptables.  These constants define table/chain/rule
//! message types, chain hooks, and expression types.

// ---------------------------------------------------------------------------
// nftables message types (nfnetlink subsystem)
// ---------------------------------------------------------------------------

/// Get table.
pub const NFT_MSG_GETTABLE: u32 = 0;
/// New table.
pub const NFT_MSG_NEWTABLE: u32 = 1;
/// Delete table.
pub const NFT_MSG_DELTABLE: u32 = 2;
/// Get chain.
pub const NFT_MSG_GETCHAIN: u32 = 3;
/// New chain.
pub const NFT_MSG_NEWCHAIN: u32 = 4;
/// Delete chain.
pub const NFT_MSG_DELCHAIN: u32 = 5;
/// Get rule.
pub const NFT_MSG_GETRULE: u32 = 6;
/// New rule.
pub const NFT_MSG_NEWRULE: u32 = 7;
/// Delete rule.
pub const NFT_MSG_DELRULE: u32 = 8;
/// Get set.
pub const NFT_MSG_GETSET: u32 = 9;
/// New set.
pub const NFT_MSG_NEWSET: u32 = 10;
/// Delete set.
pub const NFT_MSG_DELSET: u32 = 11;
/// Get set element.
pub const NFT_MSG_GETSETELEM: u32 = 12;
/// New set element.
pub const NFT_MSG_NEWSETELEM: u32 = 13;
/// Delete set element.
pub const NFT_MSG_DELSETELEM: u32 = 14;

// ---------------------------------------------------------------------------
// nftables chain types
// ---------------------------------------------------------------------------

/// Filter chain type.
pub const NFT_CHAIN_T_FILTER: u32 = 0;
/// Route chain type.
pub const NFT_CHAIN_T_ROUTE: u32 = 1;
/// NAT chain type.
pub const NFT_CHAIN_T_NAT: u32 = 2;

// ---------------------------------------------------------------------------
// nftables chain policies
// ---------------------------------------------------------------------------

/// Default policy: accept.
pub const NFT_CHAIN_POLICY_ACCEPT: u32 = 0;
/// Default policy: drop.
pub const NFT_CHAIN_POLICY_DROP: u32 = 1;

// ---------------------------------------------------------------------------
// nftables register numbers
// ---------------------------------------------------------------------------

/// Verdict register.
pub const NFT_REG_VERDICT: u32 = 0;
/// First data register (4-byte).
pub const NFT_REG32_00: u32 = 8;
/// Number of 4-byte data registers.
pub const NFT_REG32_COUNT: u32 = 16;

// ---------------------------------------------------------------------------
// nftables set flags
// ---------------------------------------------------------------------------

/// Anonymous set (auto-generated name).
pub const NFT_SET_ANONYMOUS: u32 = 1 << 0;
/// Set is constant (immutable).
pub const NFT_SET_CONSTANT: u32 = 1 << 1;
/// Set is an interval set.
pub const NFT_SET_INTERVAL: u32 = 1 << 2;
/// Set has a mapping (key → value).
pub const NFT_SET_MAP: u32 = 1 << 3;
/// Set has a timeout per element.
pub const NFT_SET_TIMEOUT: u32 = 1 << 4;
/// Set has element evaluation.
pub const NFT_SET_EVAL: u32 = 1 << 5;
/// Set is concatenation-based.
pub const NFT_SET_CONCAT: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_distinct() {
        let types = [
            NFT_MSG_GETTABLE,
            NFT_MSG_NEWTABLE,
            NFT_MSG_DELTABLE,
            NFT_MSG_GETCHAIN,
            NFT_MSG_NEWCHAIN,
            NFT_MSG_DELCHAIN,
            NFT_MSG_GETRULE,
            NFT_MSG_NEWRULE,
            NFT_MSG_DELRULE,
            NFT_MSG_GETSET,
            NFT_MSG_NEWSET,
            NFT_MSG_DELSET,
            NFT_MSG_GETSETELEM,
            NFT_MSG_NEWSETELEM,
            NFT_MSG_DELSETELEM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_gettable_is_zero() {
        assert_eq!(NFT_MSG_GETTABLE, 0);
    }

    #[test]
    fn test_chain_types_distinct() {
        let types = [NFT_CHAIN_T_FILTER, NFT_CHAIN_T_ROUTE, NFT_CHAIN_T_NAT];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_policies_distinct() {
        assert_ne!(NFT_CHAIN_POLICY_ACCEPT, NFT_CHAIN_POLICY_DROP);
    }

    #[test]
    fn test_set_flags_powers_of_two() {
        let flags = [
            NFT_SET_ANONYMOUS,
            NFT_SET_CONSTANT,
            NFT_SET_INTERVAL,
            NFT_SET_MAP,
            NFT_SET_TIMEOUT,
            NFT_SET_EVAL,
            NFT_SET_CONCAT,
        ];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_set_flags_no_overlap() {
        let flags = [
            NFT_SET_ANONYMOUS,
            NFT_SET_CONSTANT,
            NFT_SET_INTERVAL,
            NFT_SET_MAP,
            NFT_SET_TIMEOUT,
            NFT_SET_EVAL,
            NFT_SET_CONCAT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
