//! `<linux/netfilter/nf_tables.h>` — nftables table and chain constants.
//!
//! nftables is the successor to iptables, providing a unified
//! framework for packet filtering, NAT, and traffic classification.
//! Rules are organised into tables (identified by family) containing
//! chains (identified by hook point and priority). nftables uses a
//! virtual machine to evaluate rules efficiently.

// ---------------------------------------------------------------------------
// Table families (NFPROTO_*)
// ---------------------------------------------------------------------------

/// Unspecified protocol family.
pub const NFPROTO_UNSPEC: u32 = 0;
/// IPv4 (inet).
pub const NFPROTO_INET: u32 = 1;
/// IPv4.
pub const NFPROTO_IPV4: u32 = 2;
/// ARP.
pub const NFPROTO_ARP: u32 = 3;
/// Netdev (ingress/egress).
pub const NFPROTO_NETDEV: u32 = 5;
/// Bridge.
pub const NFPROTO_BRIDGE: u32 = 7;
/// IPv6.
pub const NFPROTO_IPV6: u32 = 10;
/// Number of protocol families.
pub const NFPROTO_NUMPROTO: u32 = 13;

// ---------------------------------------------------------------------------
// Chain types
// ---------------------------------------------------------------------------

/// Filter chain (packet filtering).
pub const NFT_CHAIN_T_DEFAULT: u32 = 0;
/// NAT chain.
pub const NFT_CHAIN_T_NAT: u32 = 1;
/// Route chain (re-routing packets).
pub const NFT_CHAIN_T_ROUTE: u32 = 2;

// ---------------------------------------------------------------------------
// Chain flags
// ---------------------------------------------------------------------------

/// Chain is a base chain (attached to a hook).
pub const NFT_CHAIN_BASE: u32 = 1 << 0;
/// Chain has hardware offload support.
pub const NFT_CHAIN_HW_OFFLOAD: u32 = 1 << 1;
/// Chain has a binding (used by flowtable).
pub const NFT_CHAIN_BINDING: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Set flags
// ---------------------------------------------------------------------------

/// Set is anonymous (created by the kernel).
pub const NFT_SET_ANONYMOUS: u32 = 1 << 0;
/// Set is constant (cannot be updated).
pub const NFT_SET_CONSTANT: u32 = 1 << 1;
/// Set is an interval set.
pub const NFT_SET_INTERVAL: u32 = 1 << 2;
/// Set is a map (key → value).
pub const NFT_SET_MAP: u32 = 1 << 3;
/// Set has a timeout.
pub const NFT_SET_TIMEOUT: u32 = 1 << 4;
/// Set has an evaluation path.
pub const NFT_SET_EVAL: u32 = 1 << 5;
/// Set has object mapping.
pub const NFT_SET_OBJECT: u32 = 1 << 6;
/// Set has concatenated keys.
pub const NFT_SET_CONCAT: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nfproto_distinct() {
        let protos = [
            NFPROTO_UNSPEC,
            NFPROTO_INET,
            NFPROTO_IPV4,
            NFPROTO_ARP,
            NFPROTO_NETDEV,
            NFPROTO_BRIDGE,
            NFPROTO_IPV6,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_chain_types_distinct() {
        assert_ne!(NFT_CHAIN_T_DEFAULT, NFT_CHAIN_T_NAT);
        assert_ne!(NFT_CHAIN_T_NAT, NFT_CHAIN_T_ROUTE);
        assert_ne!(NFT_CHAIN_T_DEFAULT, NFT_CHAIN_T_ROUTE);
    }

    #[test]
    fn test_chain_flags_no_overlap() {
        let flags = [NFT_CHAIN_BASE, NFT_CHAIN_HW_OFFLOAD, NFT_CHAIN_BINDING];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
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
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ipv4_ipv6_values() {
        assert_eq!(NFPROTO_IPV4, 2);
        assert_eq!(NFPROTO_IPV6, 10);
    }
}
