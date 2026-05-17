//! `<linux/fou.h>` — FOU/GUE (Foo-over-UDP / Generic UDP Encapsulation) constants.
//!
//! FOU and GUE are UDP-based tunnel encapsulation methods. FOU wraps
//! IP protocols (IPIP, GRE, etc.) directly in UDP for NAT traversal
//! and ECMP load balancing. GUE adds a shim header with version,
//! protocol, and flags. Both use the kernel's UDP tunnel framework
//! and are configured via netlink (genetlink family "fou"). Used in
//! data centers for overlay networking, especially when hardware
//! offload of outer UDP checksums is available.

// ---------------------------------------------------------------------------
// FOU netlink commands
// ---------------------------------------------------------------------------

/// Add a FOU/GUE receive port.
pub const FOU_CMD_ADD: u32 = 1;
/// Delete a FOU/GUE receive port.
pub const FOU_CMD_DEL: u32 = 2;
/// Get FOU/GUE port info.
pub const FOU_CMD_GET: u32 = 3;

// ---------------------------------------------------------------------------
// FOU netlink attributes (FOU_ATTR_*)
// ---------------------------------------------------------------------------

/// Local port (UDP port to listen on).
pub const FOU_ATTR_PORT: u32 = 1;
/// Address family (AF_INET or AF_INET6).
pub const FOU_ATTR_AF: u32 = 2;
/// IP protocol encapsulated (IPPROTO_IPIP, IPPROTO_GRE, etc.).
pub const FOU_ATTR_IPPROTO: u32 = 3;
/// Encapsulation type (FOU or GUE).
pub const FOU_ATTR_TYPE: u32 = 4;
/// Remote port (for outgoing encapsulation).
pub const FOU_ATTR_REMCSUM_NOPARTIAL: u32 = 5;
/// Local IPv4 address binding.
pub const FOU_ATTR_LOCAL_V4: u32 = 6;
/// Local IPv6 address binding.
pub const FOU_ATTR_LOCAL_V6: u32 = 7;
/// Peer IPv4 address.
pub const FOU_ATTR_PEER_V4: u32 = 8;
/// Peer IPv6 address.
pub const FOU_ATTR_PEER_V6: u32 = 9;
/// Peer port.
pub const FOU_ATTR_PEER_PORT: u32 = 10;
/// Interface index binding.
pub const FOU_ATTR_IFINDEX: u32 = 11;

// ---------------------------------------------------------------------------
// Encapsulation types
// ---------------------------------------------------------------------------

/// Direct FOU encapsulation (IP protocol in UDP).
pub const FOU_ENCAP_DIRECT: u32 = 0;
/// GUE encapsulation (with GUE header).
pub const FOU_ENCAP_GUE: u32 = 1;

// ---------------------------------------------------------------------------
// GUE header flags
// ---------------------------------------------------------------------------

/// GUE version 0.
pub const GUE_VERSION_0: u32 = 0;
/// GUE version 1.
pub const GUE_VERSION_1: u32 = 1;

// ---------------------------------------------------------------------------
// GUE flag bits (in the GUE header flags field)
// ---------------------------------------------------------------------------

/// GUE control message (not data).
pub const GUE_FLAG_CONTROL: u32 = 1 << 0;
/// GUE has remote checksum offload.
pub const GUE_FLAG_REMCSUM: u32 = 1 << 1;
/// GUE has private data.
pub const GUE_FLAG_PRIV: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Common encapsulated protocols
// ---------------------------------------------------------------------------

/// IPIP (IPv4-in-IPv4) protocol number.
pub const FOU_IPPROTO_IPIP: u32 = 4;
/// IPv6 encapsulation protocol number.
pub const FOU_IPPROTO_IPV6: u32 = 41;
/// GRE protocol number.
pub const FOU_IPPROTO_GRE: u32 = 47;
/// UDP protocol number.
pub const FOU_IPPROTO_UDP: u32 = 17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [FOU_CMD_ADD, FOU_CMD_DEL, FOU_CMD_GET];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            FOU_ATTR_PORT, FOU_ATTR_AF, FOU_ATTR_IPPROTO,
            FOU_ATTR_TYPE, FOU_ATTR_REMCSUM_NOPARTIAL,
            FOU_ATTR_LOCAL_V4, FOU_ATTR_LOCAL_V6,
            FOU_ATTR_PEER_V4, FOU_ATTR_PEER_V6,
            FOU_ATTR_PEER_PORT, FOU_ATTR_IFINDEX,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_encap_types_distinct() {
        assert_ne!(FOU_ENCAP_DIRECT, FOU_ENCAP_GUE);
    }

    #[test]
    fn test_gue_versions_distinct() {
        assert_ne!(GUE_VERSION_0, GUE_VERSION_1);
    }

    #[test]
    fn test_gue_flags_no_overlap() {
        let flags = [GUE_FLAG_CONTROL, GUE_FLAG_REMCSUM, GUE_FLAG_PRIV];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            FOU_IPPROTO_IPIP, FOU_IPPROTO_IPV6,
            FOU_IPPROTO_GRE, FOU_IPPROTO_UDP,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }
}
