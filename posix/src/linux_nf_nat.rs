//! `<linux/netfilter/nf_nat.h>` — Network Address Translation constants.
//!
//! NAT rewrites source or destination addresses/ports in IP packets,
//! enabling many-to-one address sharing (masquerade), port forwarding
//! (DNAT), and transparent proxying. It relies on connection tracking
//! to maintain consistent mappings for bidirectional flows.

// ---------------------------------------------------------------------------
// NAT range flags (NF_NAT_RANGE_*)
// ---------------------------------------------------------------------------

/// Map to a specific IP address range.
pub const NF_NAT_RANGE_MAP_IPS: u32 = 1 << 0;
/// Map to a specific port range.
pub const NF_NAT_RANGE_PROTO_SPECIFIED: u32 = 1 << 1;
/// Randomize port allocation.
pub const NF_NAT_RANGE_PROTO_RANDOM: u32 = 1 << 2;
/// Persistent mapping (same source gets same NAT mapping).
pub const NF_NAT_RANGE_PERSISTENT: u32 = 1 << 3;
/// Fully randomize port (stronger than RANDOM).
pub const NF_NAT_RANGE_PROTO_RANDOM_FULLY: u32 = 1 << 4;
/// Offset-based port allocation.
pub const NF_NAT_RANGE_PROTO_OFFSET: u32 = 1 << 5;
/// Use netmap (1:1 address mapping).
pub const NF_NAT_RANGE_NETMAP: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// NAT types
// ---------------------------------------------------------------------------

/// Source NAT (change source address/port).
pub const NF_NAT_SNAT: u8 = 0;
/// Destination NAT (change destination address/port).
pub const NF_NAT_DNAT: u8 = 1;
/// Masquerade (SNAT with dynamic source from interface).
pub const NF_NAT_MASQUERADE: u8 = 2;
/// Redirect (DNAT to local machine).
pub const NF_NAT_REDIRECT: u8 = 3;

// ---------------------------------------------------------------------------
// NAT manip type (internal)
// ---------------------------------------------------------------------------

/// Manipulate source.
pub const NF_NAT_MANIP_SRC: u8 = 0;
/// Manipulate destination.
pub const NF_NAT_MANIP_DST: u8 = 1;

// ---------------------------------------------------------------------------
// NAT helper protocols
// ---------------------------------------------------------------------------

/// FTP helper (tracks PORT/PASV).
pub const NF_NAT_HELPER_FTP: u8 = 0;
/// IRC helper (tracks DCC).
pub const NF_NAT_HELPER_IRC: u8 = 1;
/// SIP helper (tracks SDP).
pub const NF_NAT_HELPER_SIP: u8 = 2;
/// TFTP helper.
pub const NF_NAT_HELPER_TFTP: u8 = 3;
/// Amanda helper.
pub const NF_NAT_HELPER_AMANDA: u8 = 4;
/// PPTP helper (GRE call IDs).
pub const NF_NAT_HELPER_PPTP: u8 = 5;
/// H.323 helper.
pub const NF_NAT_HELPER_H323: u8 = 6;

// ---------------------------------------------------------------------------
// Port range constants
// ---------------------------------------------------------------------------

/// Minimum ephemeral port (default).
pub const NF_NAT_PORT_MIN: u16 = 1024;
/// Maximum port number.
pub const NF_NAT_PORT_MAX: u16 = 65535;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_flags_no_overlap() {
        let flags = [
            NF_NAT_RANGE_MAP_IPS, NF_NAT_RANGE_PROTO_SPECIFIED,
            NF_NAT_RANGE_PROTO_RANDOM, NF_NAT_RANGE_PERSISTENT,
            NF_NAT_RANGE_PROTO_RANDOM_FULLY, NF_NAT_RANGE_PROTO_OFFSET,
            NF_NAT_RANGE_NETMAP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_range_flags_power_of_two() {
        let flags = [
            NF_NAT_RANGE_MAP_IPS, NF_NAT_RANGE_PROTO_SPECIFIED,
            NF_NAT_RANGE_PROTO_RANDOM, NF_NAT_RANGE_PERSISTENT,
            NF_NAT_RANGE_PROTO_RANDOM_FULLY, NF_NAT_RANGE_PROTO_OFFSET,
            NF_NAT_RANGE_NETMAP,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_nat_types_distinct() {
        let types = [
            NF_NAT_SNAT, NF_NAT_DNAT,
            NF_NAT_MASQUERADE, NF_NAT_REDIRECT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_manip_types_distinct() {
        assert_ne!(NF_NAT_MANIP_SRC, NF_NAT_MANIP_DST);
    }

    #[test]
    fn test_helpers_distinct() {
        let helpers = [
            NF_NAT_HELPER_FTP, NF_NAT_HELPER_IRC, NF_NAT_HELPER_SIP,
            NF_NAT_HELPER_TFTP, NF_NAT_HELPER_AMANDA,
            NF_NAT_HELPER_PPTP, NF_NAT_HELPER_H323,
        ];
        for i in 0..helpers.len() {
            for j in (i + 1)..helpers.len() {
                assert_ne!(helpers[i], helpers[j]);
            }
        }
    }

    #[test]
    fn test_port_range() {
        assert!(NF_NAT_PORT_MIN < NF_NAT_PORT_MAX);
    }
}
