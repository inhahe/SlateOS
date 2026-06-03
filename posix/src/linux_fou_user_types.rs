//! `<linux/fou.h>` — Foo-Over-UDP generic-netlink constants.
//!
//! FOU lets the kernel encapsulate any IP protocol inside UDP for
//! NAT traversal and for ECMP load-balancing UDP flows. iproute2
//! (`ip fou add port 1234 ipproto gre`) and openvswitch's tunnel
//! code talk to the FOU genl family using the constants below.

// ---------------------------------------------------------------------------
// Genetlink family name
// ---------------------------------------------------------------------------

/// Genl family name.
pub const FOU_GENL_NAME: &str = "fou";
/// Genl family version.
pub const FOU_GENL_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Commands (struct nlmsghdr.cmd)
// ---------------------------------------------------------------------------

/// Reserved / never used over the wire.
pub const FOU_CMD_UNSPEC: u32 = 0;
/// Add a FOU receive port.
pub const FOU_CMD_ADD: u32 = 1;
/// Delete a FOU receive port.
pub const FOU_CMD_DEL: u32 = 2;
/// Get the configured ports (dump).
pub const FOU_CMD_GET: u32 = 3;

// ---------------------------------------------------------------------------
// Attributes (FOU_ATTR_*)
// ---------------------------------------------------------------------------

/// Reserved attribute type.
pub const FOU_ATTR_UNSPEC: u16 = 0;
/// Listen port (u16, network byte order).
pub const FOU_ATTR_PORT: u16 = 1;
/// Inner IP protocol number (u8).
pub const FOU_ATTR_AF: u16 = 2;
/// Encapsulation type (FOU vs GUE).
pub const FOU_ATTR_IPPROTO: u16 = 3;
/// FOU type: bare FOU.
pub const FOU_ATTR_TYPE: u16 = 4;
/// Padding for alignment.
pub const FOU_ATTR_REMCSUM_NOPARTIAL: u16 = 5;
/// Local IPv4 address.
pub const FOU_ATTR_LOCAL_V4: u16 = 6;
/// Local IPv6 address.
pub const FOU_ATTR_LOCAL_V6: u16 = 7;
/// Peer IPv4 address.
pub const FOU_ATTR_PEER_V4: u16 = 8;
/// Peer IPv6 address.
pub const FOU_ATTR_PEER_V6: u16 = 9;
/// Peer port.
pub const FOU_ATTR_PEER_PORT: u16 = 10;
/// Interface index.
pub const FOU_ATTR_IFINDEX: u16 = 11;

// ---------------------------------------------------------------------------
// Encapsulation types (struct net_ip_tunnel encap.type)
// ---------------------------------------------------------------------------

/// Bare FOU (no inner header magic).
pub const FOU_ENCAP_DIRECT: u32 = 0;
/// GUE (Generic UDP Encapsulation, RFC 8086 draft).
pub const FOU_ENCAP_GUE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genl_name() {
        assert_eq!(FOU_GENL_NAME, "fou");
        assert_eq!(FOU_GENL_VERSION, 1);
    }

    #[test]
    fn test_cmds_dense_and_unspec_zero() {
        let c = [FOU_CMD_UNSPEC, FOU_CMD_ADD, FOU_CMD_DEL, FOU_CMD_GET];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // UNSPEC must be 0 — generic-netlink reserves command 0.
        assert_eq!(FOU_CMD_UNSPEC, 0);
    }

    #[test]
    fn test_attrs_distinct_and_dense() {
        let a = [
            FOU_ATTR_UNSPEC,
            FOU_ATTR_PORT,
            FOU_ATTR_AF,
            FOU_ATTR_IPPROTO,
            FOU_ATTR_TYPE,
            FOU_ATTR_REMCSUM_NOPARTIAL,
            FOU_ATTR_LOCAL_V4,
            FOU_ATTR_LOCAL_V6,
            FOU_ATTR_PEER_V4,
            FOU_ATTR_PEER_V6,
            FOU_ATTR_PEER_PORT,
            FOU_ATTR_IFINDEX,
        ];
        // Attribute enums must be dense — netlink uses them as policy
        // table indices.
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_encap_types_distinct() {
        assert_ne!(FOU_ENCAP_DIRECT, FOU_ENCAP_GUE);
        assert_eq!(FOU_ENCAP_DIRECT, 0);
    }
}
