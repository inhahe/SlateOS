//! `<linux/pkt_cls.h>` — TC matchall classifier constants.
//!
//! Traffic control matchall (match-everything) classifier constants
//! covering attribute types.

// ---------------------------------------------------------------------------
// TC matchall classifier attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_MATCHALL_UNSPEC: u32 = 0;
/// Class ID.
pub const TCA_MATCHALL_CLASSID: u32 = 1;
/// Action.
pub const TCA_MATCHALL_ACT: u32 = 2;
/// Flags.
pub const TCA_MATCHALL_FLAGS: u32 = 3;
/// Performance counts.
pub const TCA_MATCHALL_PCNT: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_MATCHALL_UNSPEC,
            TCA_MATCHALL_CLASSID,
            TCA_MATCHALL_ACT,
            TCA_MATCHALL_FLAGS,
            TCA_MATCHALL_PCNT,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
