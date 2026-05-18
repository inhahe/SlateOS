//! `<linux/netfilter/nf_nat.h>` — NAT (Network Address Translation) constants.
//!
//! NAT modifies packet addresses as they pass through the firewall.
//! SNAT rewrites the source address (for outbound traffic behind a
//! gateway), DNAT rewrites the destination (for port forwarding and
//! load balancing), and MASQUERADE is auto-SNAT that tracks the
//! outgoing interface's current address.

// ---------------------------------------------------------------------------
// NAT types
// ---------------------------------------------------------------------------

/// No NAT.
pub const NF_NAT_NONE: u32 = 0;
/// Source NAT (rewrite source address).
pub const NF_NAT_SNAT: u32 = 1;
/// Destination NAT (rewrite destination address).
pub const NF_NAT_DNAT: u32 = 2;
/// Masquerade (auto-SNAT using outgoing interface address).
pub const NF_NAT_MASQUERADE: u32 = 3;
/// Redirect (DNAT to local host).
pub const NF_NAT_REDIRECT: u32 = 4;

// ---------------------------------------------------------------------------
// NAT range flags (nf_nat_range2.flags)
// ---------------------------------------------------------------------------

/// Map to a specific IP address.
pub const NF_NAT_RANGE_MAP_IPS: u32 = 1 << 0;
/// Map to a specific protocol port/range.
pub const NF_NAT_RANGE_PROTO_SPECIFIED: u32 = 1 << 1;
/// Randomise the source port.
pub const NF_NAT_RANGE_PROTO_RANDOM: u32 = 1 << 2;
/// Persist the mapping (consistent NAT).
pub const NF_NAT_RANGE_PERSISTENT: u32 = 1 << 3;
/// Fully random source port (stronger randomisation).
pub const NF_NAT_RANGE_PROTO_RANDOM_FULLY: u32 = 1 << 4;
/// Offset-based port mapping.
pub const NF_NAT_RANGE_PROTO_OFFSET: u32 = 1 << 5;
/// Netmap: map entire subnet.
pub const NF_NAT_RANGE_NETMAP: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nat_types_distinct() {
        let types = [
            NF_NAT_NONE, NF_NAT_SNAT, NF_NAT_DNAT,
            NF_NAT_MASQUERADE, NF_NAT_REDIRECT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_nat_types_sequential() {
        assert_eq!(NF_NAT_NONE, 0);
        assert_eq!(NF_NAT_SNAT, 1);
        assert_eq!(NF_NAT_DNAT, 2);
        assert_eq!(NF_NAT_MASQUERADE, 3);
        assert_eq!(NF_NAT_REDIRECT, 4);
    }

    #[test]
    fn test_range_flags_no_overlap() {
        let flags = [
            NF_NAT_RANGE_MAP_IPS, NF_NAT_RANGE_PROTO_SPECIFIED,
            NF_NAT_RANGE_PROTO_RANDOM, NF_NAT_RANGE_PERSISTENT,
            NF_NAT_RANGE_PROTO_RANDOM_FULLY, NF_NAT_RANGE_PROTO_OFFSET,
            NF_NAT_RANGE_NETMAP,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_range_flags_composable() {
        let combined = NF_NAT_RANGE_MAP_IPS | NF_NAT_RANGE_PROTO_SPECIFIED;
        assert_ne!(combined & NF_NAT_RANGE_MAP_IPS, 0);
        assert_ne!(combined & NF_NAT_RANGE_PROTO_SPECIFIED, 0);
        assert_eq!(combined & NF_NAT_RANGE_PERSISTENT, 0);
    }
}
