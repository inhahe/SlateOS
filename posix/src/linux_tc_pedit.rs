//! `<linux/tc_act/tc_pedit.h>` — TC packet edit action constants.
//!
//! The pedit (packet edit) action allows in-place modification of
//! packet header fields at arbitrary offsets. It can rewrite MAC
//! addresses, IP addresses, ports, TTL, DSCP, and any other field
//! by specifying offset, mask, and value.

// ---------------------------------------------------------------------------
// Pedit header types (extended key)
// ---------------------------------------------------------------------------

/// Match/edit at layer 2 (Ethernet header).
pub const TCA_PEDIT_KEY_EX_HDR_TYPE_ETH: u16 = 0;
/// Match/edit at IPv4 header.
pub const TCA_PEDIT_KEY_EX_HDR_TYPE_IP4: u16 = 1;
/// Match/edit at IPv6 header.
pub const TCA_PEDIT_KEY_EX_HDR_TYPE_IP6: u16 = 2;
/// Match/edit at TCP header.
pub const TCA_PEDIT_KEY_EX_HDR_TYPE_TCP: u16 = 3;
/// Match/edit at UDP header.
pub const TCA_PEDIT_KEY_EX_HDR_TYPE_UDP: u16 = 4;

// ---------------------------------------------------------------------------
// Pedit commands
// ---------------------------------------------------------------------------

/// Set (overwrite) the field.
pub const TCA_PEDIT_KEY_EX_CMD_SET: u16 = 0;
/// Add value to the field.
pub const TCA_PEDIT_KEY_EX_CMD_ADD: u16 = 1;

// ---------------------------------------------------------------------------
// Pedit netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_PEDIT_UNSPEC: u16 = 0;
/// Timer info.
pub const TCA_PEDIT_TM: u16 = 1;
/// Parameters (legacy).
pub const TCA_PEDIT_PARMS: u16 = 2;
/// Padding.
pub const TCA_PEDIT_PAD: u16 = 3;
/// Extended parameters.
pub const TCA_PEDIT_PARMS_EX: u16 = 4;
/// Extended keys list.
pub const TCA_PEDIT_KEYS_EX: u16 = 5;
/// Individual extended key.
pub const TCA_PEDIT_KEY_EX: u16 = 6;

// ---------------------------------------------------------------------------
// Common field offsets (IPv4)
// ---------------------------------------------------------------------------

/// IPv4 source address offset.
pub const PEDIT_IP4_SRC_OFFSET: u32 = 12;
/// IPv4 destination address offset.
pub const PEDIT_IP4_DST_OFFSET: u32 = 16;
/// IPv4 TTL offset.
pub const PEDIT_IP4_TTL_OFFSET: u32 = 8;
/// IPv4 TOS/DSCP offset.
pub const PEDIT_IP4_TOS_OFFSET: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hdr_types_distinct() {
        let types = [
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
    fn test_commands_distinct() {
        assert_ne!(TCA_PEDIT_KEY_EX_CMD_SET, TCA_PEDIT_KEY_EX_CMD_ADD);
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_PEDIT_UNSPEC,
            TCA_PEDIT_TM,
            TCA_PEDIT_PARMS,
            TCA_PEDIT_PAD,
            TCA_PEDIT_PARMS_EX,
            TCA_PEDIT_KEYS_EX,
            TCA_PEDIT_KEY_EX,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_ipv4_offsets_distinct() {
        let offsets = [
            PEDIT_IP4_SRC_OFFSET,
            PEDIT_IP4_DST_OFFSET,
            PEDIT_IP4_TTL_OFFSET,
            PEDIT_IP4_TOS_OFFSET,
        ];
        for i in 0..offsets.len() {
            for j in (i + 1)..offsets.len() {
                assert_ne!(offsets[i], offsets[j]);
            }
        }
    }
}
