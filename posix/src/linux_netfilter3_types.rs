//! `<linux/netfilter.h>` — Additional netfilter constants (batch 3).
//!
//! Supplementary netfilter constants covering nftables object types,
//! chain types, and verdict codes.

// ---------------------------------------------------------------------------
// Nftables object types (NFT_OBJECT_*)
// ---------------------------------------------------------------------------

/// Unspec object.
pub const NFT_OBJECT_UNSPEC: u32 = 0;
/// Counter object.
pub const NFT_OBJECT_COUNTER: u32 = 1;
/// Quota object.
pub const NFT_OBJECT_QUOTA: u32 = 2;
/// Conntrack helper object.
pub const NFT_OBJECT_CT_HELPER: u32 = 3;
/// Limit object.
pub const NFT_OBJECT_LIMIT: u32 = 4;
/// Conntrack timeout object.
pub const NFT_OBJECT_CT_TIMEOUT: u32 = 5;
/// Security mark object.
pub const NFT_OBJECT_SECMARK: u32 = 6;
/// Conntrack expectation object.
pub const NFT_OBJECT_CT_EXPECT: u32 = 7;
/// Synproxy object.
pub const NFT_OBJECT_SYNPROXY: u32 = 8;

// ---------------------------------------------------------------------------
// Nftables chain types
// ---------------------------------------------------------------------------

/// Filter chain.
pub const NFT_CHAIN_FILTER: u32 = 0;
/// NAT chain.
pub const NFT_CHAIN_NAT: u32 = 1;
/// Route chain.
pub const NFT_CHAIN_ROUTE: u32 = 2;

// ---------------------------------------------------------------------------
// Nftables chain policy
// ---------------------------------------------------------------------------

/// Accept policy.
pub const NFT_CHAIN_POLICY_ACCEPT: u32 = 0;
/// Drop policy.
pub const NFT_CHAIN_POLICY_DROP: u32 = 1;

// ---------------------------------------------------------------------------
// Nftables set flags
// ---------------------------------------------------------------------------

/// Anonymous set.
pub const NFT_SET_ANONYMOUS: u32 = 1 << 0;
/// Constant set.
pub const NFT_SET_CONSTANT: u32 = 1 << 1;
/// Interval set.
pub const NFT_SET_INTERVAL: u32 = 1 << 2;
/// Map set.
pub const NFT_SET_MAP: u32 = 1 << 3;
/// Timeout set.
pub const NFT_SET_TIMEOUT: u32 = 1 << 4;
/// Eval set.
pub const NFT_SET_EVAL: u32 = 1 << 5;
/// Object set.
pub const NFT_SET_OBJECT: u32 = 1 << 6;
/// Concatenation set.
pub const NFT_SET_CONCAT: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_types_distinct() {
        let types = [
            NFT_OBJECT_UNSPEC,
            NFT_OBJECT_COUNTER,
            NFT_OBJECT_QUOTA,
            NFT_OBJECT_CT_HELPER,
            NFT_OBJECT_LIMIT,
            NFT_OBJECT_CT_TIMEOUT,
            NFT_OBJECT_SECMARK,
            NFT_OBJECT_CT_EXPECT,
            NFT_OBJECT_SYNPROXY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_object_values() {
        assert_eq!(NFT_OBJECT_UNSPEC, 0);
        assert_eq!(NFT_OBJECT_SYNPROXY, 8);
    }

    #[test]
    fn test_chain_types_distinct() {
        let types = [NFT_CHAIN_FILTER, NFT_CHAIN_NAT, NFT_CHAIN_ROUTE];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_chain_policies_distinct() {
        assert_ne!(NFT_CHAIN_POLICY_ACCEPT, NFT_CHAIN_POLICY_DROP);
    }

    #[test]
    fn test_set_flags_power_of_two() {
        let flags = [
            NFT_SET_ANONYMOUS,
            NFT_SET_CONSTANT,
            NFT_SET_INTERVAL,
            NFT_SET_MAP,
            NFT_SET_TIMEOUT,
            NFT_SET_EVAL,
            NFT_SET_OBJECT,
            NFT_SET_CONCAT,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:02x} not power of two", f);
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
            NFT_SET_OBJECT,
            NFT_SET_CONCAT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
