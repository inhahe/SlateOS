//! `<linux/dsa.h>` — Distributed Switch Architecture constants.
//!
//! DSA is the Linux kernel framework for managing switch chips
//! (typically Ethernet switches) that are attached to a CPU port.
//! Each switch port appears as a separate netdev, and the framework
//! handles VLAN, bridging, and forwarding offload to hardware.

// ---------------------------------------------------------------------------
// Tag protocols (how the switch tags frames)
// ---------------------------------------------------------------------------

/// No tag (dumb switch or software bridging).
pub const DSA_TAG_PROTO_NONE: u8 = 0;
/// Marvell DSA tag.
pub const DSA_TAG_PROTO_DSA: u8 = 1;
/// Marvell EDSA tag.
pub const DSA_TAG_PROTO_EDSA: u8 = 2;
/// Broadcom tag.
pub const DSA_TAG_PROTO_BRCM: u8 = 3;
/// Broadcom prepend tag.
pub const DSA_TAG_PROTO_BRCM_PREPEND: u8 = 4;
/// QCA (Qualcomm Atheros) tag.
pub const DSA_TAG_PROTO_QCA: u8 = 5;
/// Trailer tag (Realtek).
pub const DSA_TAG_PROTO_TRAILER: u8 = 6;
/// KSZ (Microchip KSZ) tag.
pub const DSA_TAG_PROTO_KSZ: u8 = 7;
/// Ocelot (Microsemi/Microchip) tag.
pub const DSA_TAG_PROTO_OCELOT: u8 = 8;
/// SJA1105 (NXP) tag.
pub const DSA_TAG_PROTO_SJA1105: u8 = 9;
/// LAN9303 tag.
pub const DSA_TAG_PROTO_LAN9303: u8 = 10;
/// MTK (MediaTek) tag.
pub const DSA_TAG_PROTO_MTK: u8 = 11;
/// RTL (Realtek) 4-byte tag.
pub const DSA_TAG_PROTO_RTL4: u8 = 12;
/// RTL (Realtek) 8-byte tag.
pub const DSA_TAG_PROTO_RTL8: u8 = 13;

// ---------------------------------------------------------------------------
// Port types
// ---------------------------------------------------------------------------

/// User-facing port (external).
pub const DSA_PORT_TYPE_USER: u8 = 0;
/// CPU port (connects switch to host).
pub const DSA_PORT_TYPE_CPU: u8 = 1;
/// DSA port (connects to another switch in cascade).
pub const DSA_PORT_TYPE_DSA: u8 = 2;
/// Unused port.
pub const DSA_PORT_TYPE_UNUSED: u8 = 3;

// ---------------------------------------------------------------------------
// Switch operations flags
// ---------------------------------------------------------------------------

/// Switch supports VLAN filtering.
pub const DSA_SWITCH_VLAN_FILTERING: u32 = 1 << 0;
/// Switch supports port bridging.
pub const DSA_SWITCH_BRIDGE: u32 = 1 << 1;
/// Switch supports STP.
pub const DSA_SWITCH_STP: u32 = 1 << 2;
/// Switch supports port mirroring.
pub const DSA_SWITCH_MIRROR: u32 = 1 << 3;
/// Switch supports ACLs.
pub const DSA_SWITCH_ACL: u32 = 1 << 4;
/// Switch supports port isolation.
pub const DSA_SWITCH_ISOLATION: u32 = 1 << 5;
/// Switch supports LAG (Link Aggregation).
pub const DSA_SWITCH_LAG: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum ports per switch.
pub const DSA_MAX_PORTS: u8 = 12;
/// Maximum switches in a tree.
pub const DSA_MAX_SWITCHES: u8 = 4;
/// Maximum cascading depth.
pub const DSA_MAX_CASCADE: u8 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_protocols_distinct() {
        let protos = [
            DSA_TAG_PROTO_NONE, DSA_TAG_PROTO_DSA, DSA_TAG_PROTO_EDSA,
            DSA_TAG_PROTO_BRCM, DSA_TAG_PROTO_BRCM_PREPEND,
            DSA_TAG_PROTO_QCA, DSA_TAG_PROTO_TRAILER,
            DSA_TAG_PROTO_KSZ, DSA_TAG_PROTO_OCELOT,
            DSA_TAG_PROTO_SJA1105, DSA_TAG_PROTO_LAN9303,
            DSA_TAG_PROTO_MTK, DSA_TAG_PROTO_RTL4, DSA_TAG_PROTO_RTL8,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_port_types_distinct() {
        let types = [
            DSA_PORT_TYPE_USER, DSA_PORT_TYPE_CPU,
            DSA_PORT_TYPE_DSA, DSA_PORT_TYPE_UNUSED,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_switch_flags_no_overlap() {
        let flags = [
            DSA_SWITCH_VLAN_FILTERING, DSA_SWITCH_BRIDGE,
            DSA_SWITCH_STP, DSA_SWITCH_MIRROR,
            DSA_SWITCH_ACL, DSA_SWITCH_ISOLATION, DSA_SWITCH_LAG,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_switch_flags_power_of_two() {
        let flags = [
            DSA_SWITCH_VLAN_FILTERING, DSA_SWITCH_BRIDGE,
            DSA_SWITCH_STP, DSA_SWITCH_MIRROR,
            DSA_SWITCH_ACL, DSA_SWITCH_ISOLATION, DSA_SWITCH_LAG,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_limits() {
        assert!(DSA_MAX_PORTS > 0);
        assert!(DSA_MAX_SWITCHES > 0);
        assert!(DSA_MAX_CASCADE > 0);
    }
}
