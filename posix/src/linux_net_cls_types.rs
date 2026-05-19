//! `<linux/pkt_cls.h>` — Additional packet classifier constants.
//!
//! Supplementary traffic control classifier constants covering
//! action types, classifier attribute types, and match flags.

// ---------------------------------------------------------------------------
// TC action types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TC_ACT_UNSPEC: i32 = -1;
/// Ok.
pub const TC_ACT_OK: i32 = 0;
/// Reclassify.
pub const TC_ACT_RECLASSIFY: i32 = 1;
/// Shot (drop).
pub const TC_ACT_SHOT: i32 = 2;
/// Pipe.
pub const TC_ACT_PIPE: i32 = 3;
/// Stolen.
pub const TC_ACT_STOLEN: i32 = 4;
/// Queued.
pub const TC_ACT_QUEUED: i32 = 5;
/// Repeat.
pub const TC_ACT_REPEAT: i32 = 6;
/// Redirect.
pub const TC_ACT_REDIRECT: i32 = 7;
/// Trap.
pub const TC_ACT_TRAP: i32 = 8;

// ---------------------------------------------------------------------------
// TC u32 classifier attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_U32_UNSPEC: u32 = 0;
/// Class ID.
pub const TCA_U32_CLASSID: u32 = 1;
/// Hash.
pub const TCA_U32_HASH: u32 = 2;
/// Link.
pub const TCA_U32_LINK: u32 = 3;
/// Divisor.
pub const TCA_U32_DIVISOR: u32 = 4;
/// Selector.
pub const TCA_U32_SEL: u32 = 5;
/// Police.
pub const TCA_U32_POLICE: u32 = 6;
/// Action.
pub const TCA_U32_ACT: u32 = 7;
/// Indev.
pub const TCA_U32_INDEV: u32 = 8;
/// Performance counts.
pub const TCA_U32_PCNT: u32 = 9;
/// Mark.
pub const TCA_U32_MARK: u32 = 10;
/// Flags.
pub const TCA_U32_FLAGS: u32 = 11;

// ---------------------------------------------------------------------------
// TC flow classifier attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_FLOW_UNSPEC: u32 = 0;
/// Keys.
pub const TCA_FLOW_KEYS: u32 = 1;
/// Mode.
pub const TCA_FLOW_MODE: u32 = 2;
/// Base class.
pub const TCA_FLOW_BASECLASS: u32 = 3;
/// RShift.
pub const TCA_FLOW_RSHIFT: u32 = 4;
/// Addend.
pub const TCA_FLOW_ADDEND: u32 = 5;
/// Mask.
pub const TCA_FLOW_MASK: u32 = 6;
/// XOR.
pub const TCA_FLOW_XOR: u32 = 7;
/// Divisor.
pub const TCA_FLOW_DIVISOR: u32 = 8;
/// Action.
pub const TCA_FLOW_ACT: u32 = 9;
/// Police.
pub const TCA_FLOW_POLICE: u32 = 10;
/// Ematches.
pub const TCA_FLOW_EMATCHES: u32 = 11;
/// Performance counts.
pub const TCA_FLOW_PERTURB: u32 = 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let actions = [
            TC_ACT_UNSPEC, TC_ACT_OK, TC_ACT_RECLASSIFY,
            TC_ACT_SHOT, TC_ACT_PIPE, TC_ACT_STOLEN,
            TC_ACT_QUEUED, TC_ACT_REPEAT, TC_ACT_REDIRECT,
            TC_ACT_TRAP,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_u32_attrs_distinct() {
        let attrs = [
            TCA_U32_UNSPEC, TCA_U32_CLASSID, TCA_U32_HASH,
            TCA_U32_LINK, TCA_U32_DIVISOR, TCA_U32_SEL,
            TCA_U32_POLICE, TCA_U32_ACT, TCA_U32_INDEV,
            TCA_U32_PCNT, TCA_U32_MARK, TCA_U32_FLAGS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_flow_attrs_distinct() {
        let attrs = [
            TCA_FLOW_UNSPEC, TCA_FLOW_KEYS, TCA_FLOW_MODE,
            TCA_FLOW_BASECLASS, TCA_FLOW_RSHIFT, TCA_FLOW_ADDEND,
            TCA_FLOW_MASK, TCA_FLOW_XOR, TCA_FLOW_DIVISOR,
            TCA_FLOW_ACT, TCA_FLOW_POLICE, TCA_FLOW_EMATCHES,
            TCA_FLOW_PERTURB,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
