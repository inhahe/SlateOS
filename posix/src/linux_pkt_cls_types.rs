//! `<linux/pkt_cls.h>` — netlink traffic-classifier constants.
//!
//! Constants for the `tc(8)` userspace tool to install packet
//! classifiers and actions via netlink. Used by tc, nftables-compat,
//! and DPDK-style traffic-shaping daemons.

// ---------------------------------------------------------------------------
// TCA_ACT_* attribute types (struct tcamsg netlink attrs)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_ACT_UNSPEC: u32 = 0;
/// Action kind ("mirred", "police", "csum"…).
pub const TCA_ACT_KIND: u32 = 1;
/// Action-specific options blob.
pub const TCA_ACT_OPTIONS: u32 = 2;
/// Operation index (handle).
pub const TCA_ACT_INDEX: u32 = 3;
/// Per-action statistics.
pub const TCA_ACT_STATS: u32 = 4;
/// Pad for alignment.
pub const TCA_ACT_PAD: u32 = 5;
/// Cookie (opaque per-action token).
pub const TCA_ACT_COOKIE: u32 = 6;
/// HW-offload statistics.
pub const TCA_ACT_HW_STATS: u32 = 7;
/// Action flags.
pub const TCA_ACT_FLAGS: u32 = 8;

// ---------------------------------------------------------------------------
// Verdict codes (returned by classifier programs)
// ---------------------------------------------------------------------------

/// Drop the packet.
pub const TC_ACT_OK: i32 = 0;
/// Reclassify packet at root qdisc.
pub const TC_ACT_RECLASSIFY: i32 = 1;
/// Shot — drop and free.
pub const TC_ACT_SHOT: i32 = 2;
/// Stolen — the action takes ownership.
pub const TC_ACT_PIPE: i32 = 3;
/// Drop but free skb so caller doesn't.
pub const TC_ACT_STOLEN: i32 = 4;
/// Queue at userspace classifier.
pub const TC_ACT_QUEUED: i32 = 5;
/// Repeat — restart at first action.
pub const TC_ACT_REPEAT: i32 = 6;
/// Redirect — send out via another iface (mirred).
pub const TC_ACT_REDIRECT: i32 = 7;
/// Trap — let userspace inspect.
pub const TC_ACT_TRAP: i32 = 8;
/// Value to mark "not-OK" extended action.
pub const TC_ACT_VALUE_MAX: i32 = TC_ACT_TRAP;

// ---------------------------------------------------------------------------
// Extended-action code space (bit 28+ encodes extended actions)
// ---------------------------------------------------------------------------

/// Bit position where extended-action codes start.
pub const TC_ACT_EXT_SHIFT: u32 = 28;
/// Extended-action mask used to identify special codes.
pub const TC_ACT_EXT_VAL_MASK: u32 = (1 << TC_ACT_EXT_SHIFT) - 1;
/// Goto-chain extended action.
pub const TC_ACT_GOTO_CHAIN: i32 = 0x2000_0000_u32 as i32;
/// Jump extended action.
pub const TC_ACT_JUMP: i32 = 0x1000_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tca_attrs_distinct() {
        let attrs = [
            TCA_ACT_UNSPEC,
            TCA_ACT_KIND,
            TCA_ACT_OPTIONS,
            TCA_ACT_INDEX,
            TCA_ACT_STATS,
            TCA_ACT_PAD,
            TCA_ACT_COOKIE,
            TCA_ACT_HW_STATS,
            TCA_ACT_FLAGS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_verdicts_distinct() {
        let verdicts = [
            TC_ACT_OK,
            TC_ACT_RECLASSIFY,
            TC_ACT_SHOT,
            TC_ACT_PIPE,
            TC_ACT_STOLEN,
            TC_ACT_QUEUED,
            TC_ACT_REPEAT,
            TC_ACT_REDIRECT,
            TC_ACT_TRAP,
        ];
        for i in 0..verdicts.len() {
            for j in (i + 1)..verdicts.len() {
                assert_ne!(verdicts[i], verdicts[j]);
            }
        }
        assert_eq!(TC_ACT_VALUE_MAX, TC_ACT_TRAP);
    }

    #[test]
    fn test_extended_actions_fit_high_bits() {
        // Extended actions must use bits above the mask so they
        // cannot collide with regular verdicts.
        assert_eq!(TC_ACT_EXT_VAL_MASK, 0x0fff_ffff);
        let gc = TC_ACT_GOTO_CHAIN as u32;
        let jp = TC_ACT_JUMP as u32;
        assert!(gc & !TC_ACT_EXT_VAL_MASK != 0);
        assert!(jp & !TC_ACT_EXT_VAL_MASK != 0);
        assert_ne!(gc, jp);
    }
}
