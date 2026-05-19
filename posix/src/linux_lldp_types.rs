//! `<linux/lldp.h>` — LLDP (Link Layer Discovery Protocol) constants.
//!
//! Constants for LLDP covering TLV types, chassis/port subtypes,
//! and system capabilities.

// ---------------------------------------------------------------------------
// LLDP TLV types
// ---------------------------------------------------------------------------

/// End of LLDPDU.
pub const LLDP_TLV_END: u32 = 0;
/// Chassis ID.
pub const LLDP_TLV_CHASSIS_ID: u32 = 1;
/// Port ID.
pub const LLDP_TLV_PORT_ID: u32 = 2;
/// TTL (Time to Live).
pub const LLDP_TLV_TTL: u32 = 3;
/// Port description.
pub const LLDP_TLV_PORT_DESC: u32 = 4;
/// System name.
pub const LLDP_TLV_SYSTEM_NAME: u32 = 5;
/// System description.
pub const LLDP_TLV_SYSTEM_DESC: u32 = 6;
/// System capabilities.
pub const LLDP_TLV_SYSTEM_CAP: u32 = 7;
/// Management address.
pub const LLDP_TLV_MGMT_ADDR: u32 = 8;
/// Organizationally specific.
pub const LLDP_TLV_ORG_SPECIFIC: u32 = 127;

// ---------------------------------------------------------------------------
// LLDP chassis ID subtypes
// ---------------------------------------------------------------------------

/// Chassis component.
pub const LLDP_CHASSIS_COMPONENT: u32 = 1;
/// Interface alias.
pub const LLDP_CHASSIS_IFACE_ALIAS: u32 = 2;
/// Port component.
pub const LLDP_CHASSIS_PORT_COMPONENT: u32 = 3;
/// MAC address.
pub const LLDP_CHASSIS_MAC_ADDR: u32 = 4;
/// Network address.
pub const LLDP_CHASSIS_NET_ADDR: u32 = 5;
/// Interface name.
pub const LLDP_CHASSIS_IFACE_NAME: u32 = 6;
/// Locally assigned.
pub const LLDP_CHASSIS_LOCAL: u32 = 7;

// ---------------------------------------------------------------------------
// LLDP system capabilities
// ---------------------------------------------------------------------------

/// Other capability.
pub const LLDP_CAP_OTHER: u16 = 1 << 0;
/// Repeater.
pub const LLDP_CAP_REPEATER: u16 = 1 << 1;
/// Bridge.
pub const LLDP_CAP_BRIDGE: u16 = 1 << 2;
/// WLAN AP.
pub const LLDP_CAP_WLAN_AP: u16 = 1 << 3;
/// Router.
pub const LLDP_CAP_ROUTER: u16 = 1 << 4;
/// Telephone.
pub const LLDP_CAP_TELEPHONE: u16 = 1 << 5;
/// DOCSIS cable device.
pub const LLDP_CAP_DOCSIS: u16 = 1 << 6;
/// Station only.
pub const LLDP_CAP_STATION: u16 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tlv_types_distinct() {
        let types = [
            LLDP_TLV_END, LLDP_TLV_CHASSIS_ID, LLDP_TLV_PORT_ID,
            LLDP_TLV_TTL, LLDP_TLV_PORT_DESC, LLDP_TLV_SYSTEM_NAME,
            LLDP_TLV_SYSTEM_DESC, LLDP_TLV_SYSTEM_CAP,
            LLDP_TLV_MGMT_ADDR, LLDP_TLV_ORG_SPECIFIC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_chassis_subtypes_distinct() {
        let subs = [
            LLDP_CHASSIS_COMPONENT, LLDP_CHASSIS_IFACE_ALIAS,
            LLDP_CHASSIS_PORT_COMPONENT, LLDP_CHASSIS_MAC_ADDR,
            LLDP_CHASSIS_NET_ADDR, LLDP_CHASSIS_IFACE_NAME,
            LLDP_CHASSIS_LOCAL,
        ];
        for i in 0..subs.len() {
            for j in (i + 1)..subs.len() {
                assert_ne!(subs[i], subs[j]);
            }
        }
    }

    #[test]
    fn test_caps_power_of_two() {
        let caps = [
            LLDP_CAP_OTHER, LLDP_CAP_REPEATER, LLDP_CAP_BRIDGE,
            LLDP_CAP_WLAN_AP, LLDP_CAP_ROUTER, LLDP_CAP_TELEPHONE,
            LLDP_CAP_DOCSIS, LLDP_CAP_STATION,
        ];
        for c in &caps {
            assert!(c.is_power_of_two(), "0x{:04x} not power of two", c);
        }
    }

    #[test]
    fn test_caps_no_overlap() {
        let caps = [
            LLDP_CAP_OTHER, LLDP_CAP_REPEATER, LLDP_CAP_BRIDGE,
            LLDP_CAP_WLAN_AP, LLDP_CAP_ROUTER, LLDP_CAP_TELEPHONE,
            LLDP_CAP_DOCSIS, LLDP_CAP_STATION,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_end_is_zero() {
        assert_eq!(LLDP_TLV_END, 0);
    }
}
