//! `<linux/netfilter_bridge.h>` — Netfilter bridge hook constants.
//!
//! Bridge-level netfilter hooks for filtering frames that traverse
//! a Linux bridge. Used by ebtables / nft bridge family.

// ---------------------------------------------------------------------------
// Bridge hook points
// ---------------------------------------------------------------------------

/// Pre-routing (before bridging decision).
pub const NF_BR_PRE_ROUTING: u32 = 0;
/// Local in (frame destined to bridge itself).
pub const NF_BR_LOCAL_IN: u32 = 1;
/// Forward (frame being forwarded to another port).
pub const NF_BR_FORWARD: u32 = 2;
/// Local out (frame originating from bridge).
pub const NF_BR_LOCAL_OUT: u32 = 3;
/// Post-routing (after bridging decision).
pub const NF_BR_POST_ROUTING: u32 = 4;
/// Number of bridge hooks.
pub const NF_BR_NUMHOOKS: u32 = 5;

// ---------------------------------------------------------------------------
// Bridge table priorities
// ---------------------------------------------------------------------------

/// Filter table priority.
pub const NF_BR_PRI_FILTER_BRIDGED: i32 = -200;
/// Other filter priority.
pub const NF_BR_PRI_FILTER_OTHER: i32 = 200;
/// NAT destination priority.
pub const NF_BR_PRI_NAT_DST_BRIDGED: i32 = -300;
/// NAT source priority.
pub const NF_BR_PRI_NAT_SRC: i32 = 300;
/// Brnf priority.
pub const NF_BR_PRI_BRNF: i32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_values() {
        assert_eq!(NF_BR_PRE_ROUTING, 0);
        assert_eq!(NF_BR_LOCAL_IN, 1);
        assert_eq!(NF_BR_FORWARD, 2);
        assert_eq!(NF_BR_LOCAL_OUT, 3);
        assert_eq!(NF_BR_POST_ROUTING, 4);
        assert_eq!(NF_BR_NUMHOOKS, 5);
    }

    #[test]
    fn test_hooks_distinct() {
        let hooks = [
            NF_BR_PRE_ROUTING,
            NF_BR_LOCAL_IN,
            NF_BR_FORWARD,
            NF_BR_LOCAL_OUT,
            NF_BR_POST_ROUTING,
        ];
        for i in 0..hooks.len() {
            for j in (i + 1)..hooks.len() {
                assert_ne!(hooks[i], hooks[j]);
            }
        }
    }

    #[test]
    fn test_priorities_distinct() {
        let pris = [
            NF_BR_PRI_FILTER_BRIDGED,
            NF_BR_PRI_FILTER_OTHER,
            NF_BR_PRI_NAT_DST_BRIDGED,
            NF_BR_PRI_NAT_SRC,
            NF_BR_PRI_BRNF,
        ];
        for i in 0..pris.len() {
            for j in (i + 1)..pris.len() {
                assert_ne!(pris[i], pris[j]);
            }
        }
    }
}
