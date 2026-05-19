//! `<linux/tc_act/tc_pedit.h>` — TC pedit action constants.
//!
//! Traffic control packet edit action constants covering attribute types,
//! header types, and edit command codes.

// ---------------------------------------------------------------------------
// TC pedit attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_PEDIT_UNSPEC: u32 = 0;
/// Timestamp.
pub const TCA_PEDIT_TM: u32 = 1;
/// Parameters.
pub const TCA_PEDIT_PARMS: u32 = 2;
/// Extended parameters.
pub const TCA_PEDIT_PARMS_EX: u32 = 3;
/// Keys extended.
pub const TCA_PEDIT_KEYS_EX: u32 = 4;
/// Key extended.
pub const TCA_PEDIT_KEY_EX: u32 = 5;

// ---------------------------------------------------------------------------
// TC pedit header types
// ---------------------------------------------------------------------------

/// Network header.
pub const TCA_PEDIT_KEY_EX_HDR_TYPE_NETWORK: u32 = 0;
/// Ethernet header.
pub const TCA_PEDIT_KEY_EX_HDR_TYPE_ETH: u32 = 1;
/// IPv4 header.
pub const TCA_PEDIT_KEY_EX_HDR_TYPE_IP4: u32 = 2;
/// IPv6 header.
pub const TCA_PEDIT_KEY_EX_HDR_TYPE_IP6: u32 = 3;
/// TCP header.
pub const TCA_PEDIT_KEY_EX_HDR_TYPE_TCP: u32 = 4;
/// UDP header.
pub const TCA_PEDIT_KEY_EX_HDR_TYPE_UDP: u32 = 5;

// ---------------------------------------------------------------------------
// TC pedit edit commands
// ---------------------------------------------------------------------------

/// Set.
pub const TCA_PEDIT_KEY_EX_CMD_SET: u32 = 0;
/// Add.
pub const TCA_PEDIT_KEY_EX_CMD_ADD: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_PEDIT_UNSPEC, TCA_PEDIT_TM, TCA_PEDIT_PARMS,
            TCA_PEDIT_PARMS_EX, TCA_PEDIT_KEYS_EX, TCA_PEDIT_KEY_EX,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_header_types_distinct() {
        let types = [
            TCA_PEDIT_KEY_EX_HDR_TYPE_NETWORK,
            TCA_PEDIT_KEY_EX_HDR_TYPE_ETH,
            TCA_PEDIT_KEY_EX_HDR_TYPE_IP4,
            TCA_PEDIT_KEY_EX_HDR_TYPE_IP6,
            TCA_PEDIT_KEY_EX_HDR_TYPE_TCP,
            TCA_PEDIT_KEY_EX_HDR_TYPE_UDP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_edit_cmds_distinct() {
        assert_ne!(TCA_PEDIT_KEY_EX_CMD_SET, TCA_PEDIT_KEY_EX_CMD_ADD);
    }
}
