//! `<linux/nf_tables.h>` — nftables constants.
//!
//! nftables is the modern Linux packet filtering framework,
//! replacing iptables. It uses a virtual machine approach where
//! rules are compiled to bytecode expressions evaluated per-packet.
//! Configuration is via the nf_tables netlink interface.

// ---------------------------------------------------------------------------
// Netlink message types
// ---------------------------------------------------------------------------

/// New table.
pub const NFT_MSG_NEWTABLE: u16 = 0;
/// Get table.
pub const NFT_MSG_GETTABLE: u16 = 1;
/// Delete table.
pub const NFT_MSG_DELTABLE: u16 = 2;
/// New chain.
pub const NFT_MSG_NEWCHAIN: u16 = 3;
/// Get chain.
pub const NFT_MSG_GETCHAIN: u16 = 4;
/// Delete chain.
pub const NFT_MSG_DELCHAIN: u16 = 5;
/// New rule.
pub const NFT_MSG_NEWRULE: u16 = 6;
/// Get rule.
pub const NFT_MSG_GETRULE: u16 = 7;
/// Delete rule.
pub const NFT_MSG_DELRULE: u16 = 8;
/// New set.
pub const NFT_MSG_NEWSET: u16 = 9;
/// Get set.
pub const NFT_MSG_GETSET: u16 = 10;
/// Delete set.
pub const NFT_MSG_DELSET: u16 = 11;

// ---------------------------------------------------------------------------
// Verdicts
// ---------------------------------------------------------------------------

/// Continue to next rule.
pub const NFT_CONTINUE: i32 = -1;
/// Stop processing, packet breaks out of chain.
pub const NFT_BREAK: i32 = -2;
/// Jump to another chain.
pub const NFT_JUMP: i32 = -3;
/// Go to another chain (no return).
pub const NFT_GOTO: i32 = -4;
/// Return from current chain.
pub const NFT_RETURN: i32 = -5;

// ---------------------------------------------------------------------------
// Standard chain hooks
// ---------------------------------------------------------------------------

/// Pre-routing hook.
pub const NF_INET_PRE_ROUTING: u32 = 0;
/// Local input hook.
pub const NF_INET_LOCAL_IN: u32 = 1;
/// Forward hook.
pub const NF_INET_FORWARD: u32 = 2;
/// Local output hook.
pub const NF_INET_LOCAL_OUT: u32 = 3;
/// Post-routing hook.
pub const NF_INET_POST_ROUTING: u32 = 4;

// ---------------------------------------------------------------------------
// Address families
// ---------------------------------------------------------------------------

/// IPv4.
pub const NFPROTO_IPV4: u8 = 2;
/// IPv6.
pub const NFPROTO_IPV6: u8 = 10;
/// Bridge (layer 2).
pub const NFPROTO_BRIDGE: u8 = 7;
/// ARP.
pub const NFPROTO_ARP: u8 = 3;
/// Netdev (ingress/egress).
pub const NFPROTO_NETDEV: u8 = 5;
/// Inet (dual-stack IPv4+IPv6).
pub const NFPROTO_INET: u8 = 1;

// ---------------------------------------------------------------------------
// Chain types
// ---------------------------------------------------------------------------

/// Filter chain.
pub const NFT_CHAIN_TYPE_FILTER: &str = "filter";
/// NAT chain.
pub const NFT_CHAIN_TYPE_NAT: &str = "nat";
/// Route chain.
pub const NFT_CHAIN_TYPE_ROUTE: &str = "route";

// ---------------------------------------------------------------------------
// Chain policies
// ---------------------------------------------------------------------------

/// Accept (default allow).
pub const NF_ACCEPT: u32 = 1;
/// Drop (default deny).
pub const NF_DROP: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_distinct() {
        let msgs = [
            NFT_MSG_NEWTABLE,
            NFT_MSG_GETTABLE,
            NFT_MSG_DELTABLE,
            NFT_MSG_NEWCHAIN,
            NFT_MSG_GETCHAIN,
            NFT_MSG_DELCHAIN,
            NFT_MSG_NEWRULE,
            NFT_MSG_GETRULE,
            NFT_MSG_DELRULE,
            NFT_MSG_NEWSET,
            NFT_MSG_GETSET,
            NFT_MSG_DELSET,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_verdicts_distinct() {
        let verdicts = [NFT_CONTINUE, NFT_BREAK, NFT_JUMP, NFT_GOTO, NFT_RETURN];
        for i in 0..verdicts.len() {
            for j in (i + 1)..verdicts.len() {
                assert_ne!(verdicts[i], verdicts[j]);
            }
        }
        // All negative
        for v in &verdicts {
            assert!(*v < 0);
        }
    }

    #[test]
    fn test_hooks_distinct() {
        let hooks = [
            NF_INET_PRE_ROUTING,
            NF_INET_LOCAL_IN,
            NF_INET_FORWARD,
            NF_INET_LOCAL_OUT,
            NF_INET_POST_ROUTING,
        ];
        for i in 0..hooks.len() {
            for j in (i + 1)..hooks.len() {
                assert_ne!(hooks[i], hooks[j]);
            }
        }
    }

    #[test]
    fn test_protos_distinct() {
        let protos = [
            NFPROTO_IPV4,
            NFPROTO_IPV6,
            NFPROTO_BRIDGE,
            NFPROTO_ARP,
            NFPROTO_NETDEV,
            NFPROTO_INET,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_chain_types_distinct() {
        let types = [
            NFT_CHAIN_TYPE_FILTER,
            NFT_CHAIN_TYPE_NAT,
            NFT_CHAIN_TYPE_ROUTE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_policies_distinct() {
        assert_ne!(NF_ACCEPT, NF_DROP);
    }
}
