//! `<linux/pkt_sched.h>` — TC RED qdisc constants.
//!
//! Traffic control RED (Random Early Detection) qdisc constants
//! covering attribute types and flags.

// ---------------------------------------------------------------------------
// TC RED attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_RED_UNSPEC: u32 = 0;
/// Parameters.
pub const TCA_RED_PARMS: u32 = 1;
/// Stab.
pub const TCA_RED_STAB: u32 = 2;
/// Max P.
pub const TCA_RED_MAX_P: u32 = 3;
/// Flags.
pub const TCA_RED_FLAGS: u32 = 4;
/// Early drop block.
pub const TCA_RED_EARLY_DROP_BLOCK: u32 = 5;
/// Mark block.
pub const TCA_RED_MARK_BLOCK: u32 = 6;

// ---------------------------------------------------------------------------
// TC RED flags
// ---------------------------------------------------------------------------

/// ECN capable.
pub const TC_RED_ECN: u32 = 1;
/// Hardware stats.
pub const TC_RED_HARDDROP: u32 = 2;
/// Adaptive mode.
pub const TC_RED_ADAPTATIVE: u32 = 4;
/// Nodrop mode.
pub const TC_RED_NODROP: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_RED_UNSPEC, TCA_RED_PARMS, TCA_RED_STAB,
            TCA_RED_MAX_P, TCA_RED_FLAGS, TCA_RED_EARLY_DROP_BLOCK,
            TCA_RED_MARK_BLOCK,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_flags_distinct() {
        let flags = [TC_RED_ECN, TC_RED_HARDDROP, TC_RED_ADAPTATIVE, TC_RED_NODROP];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
