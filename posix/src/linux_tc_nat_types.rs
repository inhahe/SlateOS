//! `<linux/tc_act/tc_nat.h>` — TC NAT action constants.
//!
//! Traffic control NAT action constants covering attribute types
//! and NAT flags.

// ---------------------------------------------------------------------------
// TC NAT attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_NAT_UNSPEC: u32 = 0;
/// Parameters.
pub const TCA_NAT_PARMS: u32 = 1;
/// Timestamp.
pub const TCA_NAT_TM: u32 = 2;

// ---------------------------------------------------------------------------
// TC NAT flags
// ---------------------------------------------------------------------------

/// Source NAT.
pub const TCA_NAT_FLAG_EGRESS: u32 = 1;

// ---------------------------------------------------------------------------
// TC connmark attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_CONNMARK_UNSPEC: u32 = 0;
/// Parameters.
pub const TCA_CONNMARK_PARMS: u32 = 1;
/// Timestamp.
pub const TCA_CONNMARK_TM: u32 = 2;

// ---------------------------------------------------------------------------
// TC csum attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_CSUM_UNSPEC: u32 = 0;
/// Parameters.
pub const TCA_CSUM_PARMS: u32 = 1;
/// Timestamp.
pub const TCA_CSUM_TM: u32 = 2;

// ---------------------------------------------------------------------------
// TC csum update flags
// ---------------------------------------------------------------------------

/// Update IPv4 header checksum.
pub const TCA_CSUM_UPDATE_FLAG_IPV4HDR: u32 = 1 << 0;
/// Update ICMP checksum.
pub const TCA_CSUM_UPDATE_FLAG_ICMP: u32 = 1 << 1;
/// Update IGMP checksum.
pub const TCA_CSUM_UPDATE_FLAG_IGMP: u32 = 1 << 2;
/// Update TCP checksum.
pub const TCA_CSUM_UPDATE_FLAG_TCP: u32 = 1 << 3;
/// Update UDP checksum.
pub const TCA_CSUM_UPDATE_FLAG_UDP: u32 = 1 << 4;
/// Update UDPLITE checksum.
pub const TCA_CSUM_UPDATE_FLAG_UDPLITE: u32 = 1 << 5;
/// Update SCTP checksum.
pub const TCA_CSUM_UPDATE_FLAG_SCTP: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nat_attrs_distinct() {
        let attrs = [TCA_NAT_UNSPEC, TCA_NAT_PARMS, TCA_NAT_TM];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_connmark_attrs_distinct() {
        let attrs = [TCA_CONNMARK_UNSPEC, TCA_CONNMARK_PARMS, TCA_CONNMARK_TM];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_csum_attrs_distinct() {
        let attrs = [TCA_CSUM_UNSPEC, TCA_CSUM_PARMS, TCA_CSUM_TM];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_csum_flags_no_overlap() {
        let flags = [
            TCA_CSUM_UPDATE_FLAG_IPV4HDR, TCA_CSUM_UPDATE_FLAG_ICMP,
            TCA_CSUM_UPDATE_FLAG_IGMP, TCA_CSUM_UPDATE_FLAG_TCP,
            TCA_CSUM_UPDATE_FLAG_UDP, TCA_CSUM_UPDATE_FLAG_UDPLITE,
            TCA_CSUM_UPDATE_FLAG_SCTP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_csum_flags_power_of_two() {
        let flags = [
            TCA_CSUM_UPDATE_FLAG_IPV4HDR, TCA_CSUM_UPDATE_FLAG_ICMP,
            TCA_CSUM_UPDATE_FLAG_IGMP, TCA_CSUM_UPDATE_FLAG_TCP,
            TCA_CSUM_UPDATE_FLAG_UDP, TCA_CSUM_UPDATE_FLAG_UDPLITE,
            TCA_CSUM_UPDATE_FLAG_SCTP,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x} is not power of two", flag);
        }
    }
}
