//! `<linux/tc_act/tc_csum.h>` — TC checksum action constants.
//!
//! The csum action recalculates packet checksums after header fields
//! have been modified by other actions (pedit, NAT, etc.). It can
//! update IPv4 header checksum, TCP/UDP/SCTP checksums, and ICMPv6.

// ---------------------------------------------------------------------------
// Checksum update flags
// ---------------------------------------------------------------------------

/// Recalculate IPv4 header checksum.
pub const TCA_CSUM_UPDATE_FLAG_IPV4HDR: u32 = 1 << 0;
/// Recalculate ICMP checksum.
pub const TCA_CSUM_UPDATE_FLAG_ICMP: u32 = 1 << 1;
/// Recalculate IGMP checksum.
pub const TCA_CSUM_UPDATE_FLAG_IGMP: u32 = 1 << 2;
/// Recalculate TCP checksum.
pub const TCA_CSUM_UPDATE_FLAG_TCP: u32 = 1 << 3;
/// Recalculate UDP checksum.
pub const TCA_CSUM_UPDATE_FLAG_UDP: u32 = 1 << 4;
/// Recalculate UDP-Lite checksum.
pub const TCA_CSUM_UPDATE_FLAG_UDPLITE: u32 = 1 << 5;
/// Recalculate SCTP checksum.
pub const TCA_CSUM_UPDATE_FLAG_SCTP: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Csum netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_CSUM_UNSPEC: u16 = 0;
/// Parameters.
pub const TCA_CSUM_PARMS: u16 = 1;
/// Timer info.
pub const TCA_CSUM_TM: u16 = 2;
/// Padding.
pub const TCA_CSUM_PAD: u16 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_flags_no_overlap() {
        let flags = [
            TCA_CSUM_UPDATE_FLAG_IPV4HDR,
            TCA_CSUM_UPDATE_FLAG_ICMP,
            TCA_CSUM_UPDATE_FLAG_IGMP,
            TCA_CSUM_UPDATE_FLAG_TCP,
            TCA_CSUM_UPDATE_FLAG_UDP,
            TCA_CSUM_UPDATE_FLAG_UDPLITE,
            TCA_CSUM_UPDATE_FLAG_SCTP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_update_flags_power_of_two() {
        let flags = [
            TCA_CSUM_UPDATE_FLAG_IPV4HDR,
            TCA_CSUM_UPDATE_FLAG_ICMP,
            TCA_CSUM_UPDATE_FLAG_IGMP,
            TCA_CSUM_UPDATE_FLAG_TCP,
            TCA_CSUM_UPDATE_FLAG_UDP,
            TCA_CSUM_UPDATE_FLAG_UDPLITE,
            TCA_CSUM_UPDATE_FLAG_SCTP,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [TCA_CSUM_UNSPEC, TCA_CSUM_PARMS, TCA_CSUM_TM, TCA_CSUM_PAD];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
