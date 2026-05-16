//! `<linux/netfilter_arp.h>` — Netfilter ARP hook constants.
//!
//! ARP-level netfilter hooks for filtering and mangling ARP packets.
//! Used by arptables (nftables successor: nft with arp family).

// ---------------------------------------------------------------------------
// ARP hook points
// ---------------------------------------------------------------------------

/// ARP input hook (incoming ARP).
pub const NF_ARP_IN: u32 = 0;
/// ARP output hook (outgoing ARP).
pub const NF_ARP_OUT: u32 = 1;
/// ARP forward hook.
pub const NF_ARP_FORWARD: u32 = 2;
/// Number of ARP hooks.
pub const NF_ARP_NUMHOOKS: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_values() {
        assert_eq!(NF_ARP_IN, 0);
        assert_eq!(NF_ARP_OUT, 1);
        assert_eq!(NF_ARP_FORWARD, 2);
        assert_eq!(NF_ARP_NUMHOOKS, 3);
    }

    #[test]
    fn test_hooks_distinct() {
        let hooks = [NF_ARP_IN, NF_ARP_OUT, NF_ARP_FORWARD];
        for i in 0..hooks.len() {
            for j in (i + 1)..hooks.len() {
                assert_ne!(hooks[i], hooks[j]);
            }
        }
    }
}
