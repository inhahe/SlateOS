//! `<linux/ife.h>` — IFE (Inter-FE) constants.
//!
//! Inter-Frame Engine constants covering metadata type codes
//! and action attribute types.

// ---------------------------------------------------------------------------
// IFE metadata types
// ---------------------------------------------------------------------------

/// Unspec.
pub const IFE_META_UNSPEC: u32 = 0;
/// Skbmark.
pub const IFE_META_SKBMARK: u32 = 1;
/// Skbhash.
pub const IFE_META_HASHID: u32 = 2;
/// Priority.
pub const IFE_META_PRIO: u32 = 3;
/// Queue mapping.
pub const IFE_META_QMAP: u32 = 4;
/// Traffic class index.
pub const IFE_META_TCINDEX: u32 = 5;

// ---------------------------------------------------------------------------
// IFE action attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_IFE_UNSPEC: u32 = 0;
/// Parameters.
pub const TCA_IFE_PARMS: u32 = 1;
/// Timestamp.
pub const TCA_IFE_TM: u32 = 2;
/// Destination MAC.
pub const TCA_IFE_DMAC: u32 = 3;
/// Source MAC.
pub const TCA_IFE_SMAC: u32 = 4;
/// Ethertype.
pub const TCA_IFE_TYPE: u32 = 5;
/// Metadata.
pub const TCA_IFE_METALST: u32 = 6;

// ---------------------------------------------------------------------------
// IFE action types
// ---------------------------------------------------------------------------

/// Decode.
pub const IFE_DECODE: u32 = 0;
/// Encode.
pub const IFE_ENCODE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_meta_types_distinct() {
        let types = [
            IFE_META_UNSPEC,
            IFE_META_SKBMARK,
            IFE_META_HASHID,
            IFE_META_PRIO,
            IFE_META_QMAP,
            IFE_META_TCINDEX,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_action_attrs_distinct() {
        let attrs = [
            TCA_IFE_UNSPEC,
            TCA_IFE_PARMS,
            TCA_IFE_TM,
            TCA_IFE_DMAC,
            TCA_IFE_SMAC,
            TCA_IFE_TYPE,
            TCA_IFE_METALST,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_action_types_distinct() {
        assert_ne!(IFE_DECODE, IFE_ENCODE);
    }
}
