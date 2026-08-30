//! `<linux/pkt_cls.h>` — TC basic classifier constants.
//!
//! Traffic control basic classifier constants covering
//! attribute types and related filter attribute types.

// ---------------------------------------------------------------------------
// TC basic classifier attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_BASIC_UNSPEC: u32 = 0;
/// Class ID.
pub const TCA_BASIC_CLASSID: u32 = 1;
/// Ematches.
pub const TCA_BASIC_EMATCHES: u32 = 2;
/// Action.
pub const TCA_BASIC_ACT: u32 = 3;
/// Police.
pub const TCA_BASIC_POLICE: u32 = 4;
/// Performance counts.
pub const TCA_BASIC_PCNT: u32 = 5;

// ---------------------------------------------------------------------------
// TC route classifier attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_ROUTE4_UNSPEC: u32 = 0;
/// Class ID.
pub const TCA_ROUTE4_CLASSID: u32 = 1;
/// To.
pub const TCA_ROUTE4_TO: u32 = 2;
/// From.
pub const TCA_ROUTE4_FROM: u32 = 3;
/// IIF.
pub const TCA_ROUTE4_IIF: u32 = 4;
/// Police.
pub const TCA_ROUTE4_POLICE: u32 = 5;
/// Action.
pub const TCA_ROUTE4_ACT: u32 = 6;

// ---------------------------------------------------------------------------
// TC fw classifier attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_FW_UNSPEC: u32 = 0;
/// Class ID.
pub const TCA_FW_CLASSID: u32 = 1;
/// Police.
pub const TCA_FW_POLICE: u32 = 2;
/// Indev.
pub const TCA_FW_INDEV: u32 = 3;
/// Action.
pub const TCA_FW_ACT: u32 = 4;
/// Mask.
pub const TCA_FW_MASK: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_attrs_distinct() {
        let attrs = [
            TCA_BASIC_UNSPEC,
            TCA_BASIC_CLASSID,
            TCA_BASIC_EMATCHES,
            TCA_BASIC_ACT,
            TCA_BASIC_POLICE,
            TCA_BASIC_PCNT,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_route4_attrs_distinct() {
        let attrs = [
            TCA_ROUTE4_UNSPEC,
            TCA_ROUTE4_CLASSID,
            TCA_ROUTE4_TO,
            TCA_ROUTE4_FROM,
            TCA_ROUTE4_IIF,
            TCA_ROUTE4_POLICE,
            TCA_ROUTE4_ACT,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_fw_attrs_distinct() {
        let attrs = [
            TCA_FW_UNSPEC,
            TCA_FW_CLASSID,
            TCA_FW_POLICE,
            TCA_FW_INDEV,
            TCA_FW_ACT,
            TCA_FW_MASK,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
