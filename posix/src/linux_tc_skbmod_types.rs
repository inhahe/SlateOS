//! `<linux/tc_act/tc_skbmod.h>` — TC skbmod action constants.
//!
//! Traffic control skbmod action constants covering action flags
//! and attribute types for modifying skb fields.

// ---------------------------------------------------------------------------
// TC skbmod flags
// ---------------------------------------------------------------------------

/// Set destination MAC.
pub const SKBMOD_F_DMAC: u32 = 1 << 0;
/// Set source MAC.
pub const SKBMOD_F_SMAC: u32 = 1 << 1;
/// Set ethertype.
pub const SKBMOD_F_ETYPE: u32 = 1 << 2;
/// Swap source and destination MAC.
pub const SKBMOD_F_SWAPMAC: u32 = 1 << 3;
/// ECN.
pub const SKBMOD_F_ECN: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// TC skbmod attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_SKBMOD_UNSPEC: u32 = 0;
/// Timestamp.
pub const TCA_SKBMOD_TM: u32 = 1;
/// Parameters.
pub const TCA_SKBMOD_PARMS: u32 = 2;
/// Destination MAC.
pub const TCA_SKBMOD_DMAC: u32 = 3;
/// Source MAC.
pub const TCA_SKBMOD_SMAC: u32 = 4;
/// Ethertype.
pub const TCA_SKBMOD_ETYPE: u32 = 5;

// ---------------------------------------------------------------------------
// TC skbedit attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_SKBEDIT_UNSPEC: u32 = 0;
/// Timestamp.
pub const TCA_SKBEDIT_TM: u32 = 1;
/// Parameters.
pub const TCA_SKBEDIT_PARMS: u32 = 2;
/// Priority.
pub const TCA_SKBEDIT_PRIORITY: u32 = 3;
/// Queue mapping.
pub const TCA_SKBEDIT_QUEUE_MAPPING: u32 = 4;
/// Mark.
pub const TCA_SKBEDIT_MARK: u32 = 5;
/// Packet type.
pub const TCA_SKBEDIT_PTYPE: u32 = 6;
/// Mask.
pub const TCA_SKBEDIT_MASK: u32 = 7;
/// Flags.
pub const TCA_SKBEDIT_FLAGS: u32 = 8;
/// Queue mapping max.
pub const TCA_SKBEDIT_QUEUE_MAPPING_MAX: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skbmod_flags_no_overlap() {
        let flags = [
            SKBMOD_F_DMAC, SKBMOD_F_SMAC, SKBMOD_F_ETYPE,
            SKBMOD_F_SWAPMAC, SKBMOD_F_ECN,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_skbmod_flags_power_of_two() {
        let flags = [
            SKBMOD_F_DMAC, SKBMOD_F_SMAC, SKBMOD_F_ETYPE,
            SKBMOD_F_SWAPMAC, SKBMOD_F_ECN,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x} is not power of two", flag);
        }
    }

    #[test]
    fn test_skbmod_attrs_distinct() {
        let attrs = [
            TCA_SKBMOD_UNSPEC, TCA_SKBMOD_TM, TCA_SKBMOD_PARMS,
            TCA_SKBMOD_DMAC, TCA_SKBMOD_SMAC, TCA_SKBMOD_ETYPE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_skbedit_attrs_distinct() {
        let attrs = [
            TCA_SKBEDIT_UNSPEC, TCA_SKBEDIT_TM, TCA_SKBEDIT_PARMS,
            TCA_SKBEDIT_PRIORITY, TCA_SKBEDIT_QUEUE_MAPPING,
            TCA_SKBEDIT_MARK, TCA_SKBEDIT_PTYPE,
            TCA_SKBEDIT_MASK, TCA_SKBEDIT_FLAGS,
            TCA_SKBEDIT_QUEUE_MAPPING_MAX,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
