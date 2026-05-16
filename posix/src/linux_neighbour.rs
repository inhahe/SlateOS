//! `<linux/neighbour.h>` — ARP/NDP neighbor table constants.
//!
//! Neighbor discovery manages the mapping between L3 addresses (IP)
//! and L2 addresses (MAC). Includes ARP for IPv4 and NDP for IPv6.
//! Managed via `ip neigh` (iproute2).

// ---------------------------------------------------------------------------
// Neighbor states (NUD_*)
// ---------------------------------------------------------------------------

/// Incomplete (resolution in progress).
pub const NUD_INCOMPLETE: u16 = 0x01;
/// Reachable (confirmed recently).
pub const NUD_REACHABLE: u16 = 0x02;
/// Stale (needs reconfirmation).
pub const NUD_STALE: u16 = 0x04;
/// Delay (waiting for upper-layer hint).
pub const NUD_DELAY: u16 = 0x08;
/// Probe (actively probing).
pub const NUD_PROBE: u16 = 0x10;
/// Failed (resolution failed).
pub const NUD_FAILED: u16 = 0x20;
/// No ARP (static, never changes).
pub const NUD_NOARP: u16 = 0x40;
/// Permanent (user-configured).
pub const NUD_PERMANENT: u16 = 0x80;
/// None (not in table).
pub const NUD_NONE: u16 = 0x00;

// ---------------------------------------------------------------------------
// Neighbor attributes (NDA_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const NDA_UNSPEC: u16 = 0;
/// L2 (MAC) address.
pub const NDA_DST: u16 = 1;
/// L2 address.
pub const NDA_LLADDR: u16 = 2;
/// Cache info.
pub const NDA_CACHEINFO: u16 = 3;
/// Proxy.
pub const NDA_PROBES: u16 = 4;
/// VLAN.
pub const NDA_VLAN: u16 = 5;
/// Port.
pub const NDA_PORT: u16 = 6;
/// VNI.
pub const NDA_VNI: u16 = 7;
/// Interface index.
pub const NDA_IFINDEX: u16 = 8;
/// Master device.
pub const NDA_MASTER: u16 = 9;
/// Link netnsid.
pub const NDA_LINK_NETNSID: u16 = 10;
/// Source VNI.
pub const NDA_SRC_VNI: u16 = 11;
/// Protocol.
pub const NDA_PROTOCOL: u16 = 12;
/// Flags extension.
pub const NDA_FLAGS_EXT: u16 = 14;

// ---------------------------------------------------------------------------
// Neighbor flags (NTF_*)
// ---------------------------------------------------------------------------

/// Use (not just learned).
pub const NTF_USE: u8 = 0x01;
/// Self (local address).
pub const NTF_SELF: u8 = 0x02;
/// Master (bridge FDB entry).
pub const NTF_MASTER: u8 = 0x04;
/// Proxy.
pub const NTF_PROXY: u8 = 0x08;
/// External learned.
pub const NTF_EXT_LEARNED: u8 = 0x10;
/// Offloaded.
pub const NTF_OFFLOADED: u8 = 0x20;
/// Sticky (don't age out).
pub const NTF_STICKY: u8 = 0x40;
/// Router.
pub const NTF_ROUTER: u8 = 0x80;

// ---------------------------------------------------------------------------
// Ndmsg struct
// ---------------------------------------------------------------------------

/// Neighbor discovery message (12 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ndmsg {
    /// Address family.
    pub ndm_family: u8,
    /// Padding.
    _pad1: u8,
    /// Padding.
    _pad2: u16,
    /// Interface index.
    pub ndm_ifindex: i32,
    /// Neighbor state (NUD_*).
    pub ndm_state: u16,
    /// Flags (NTF_*).
    pub ndm_flags: u8,
    /// Type.
    pub ndm_type: u8,
}

impl Ndmsg {
    /// Create a zeroed neighbor message.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nud_states_powers_of_two() {
        let states = [
            NUD_INCOMPLETE, NUD_REACHABLE, NUD_STALE, NUD_DELAY,
            NUD_PROBE, NUD_FAILED, NUD_NOARP, NUD_PERMANENT,
        ];
        for s in &states {
            assert!(s.is_power_of_two(), "state {s:#x} not power of 2");
        }
    }

    #[test]
    fn test_nda_attrs_sequential() {
        assert_eq!(NDA_UNSPEC, 0);
        assert_eq!(NDA_DST, 1);
        assert_eq!(NDA_LLADDR, 2);
        assert_eq!(NDA_CACHEINFO, 3);
    }

    #[test]
    fn test_ntf_flags_powers_of_two() {
        let flags = [
            NTF_USE, NTF_SELF, NTF_MASTER, NTF_PROXY,
            NTF_EXT_LEARNED, NTF_OFFLOADED, NTF_STICKY,
            NTF_ROUTER,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} not power of 2");
        }
    }

    #[test]
    fn test_ndmsg_size() {
        assert_eq!(core::mem::size_of::<Ndmsg>(), 12);
    }

    #[test]
    fn test_nud_none() {
        assert_eq!(NUD_NONE, 0);
    }
}
