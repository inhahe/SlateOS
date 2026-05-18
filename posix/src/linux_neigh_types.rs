//! `<linux/neighbour.h>` — Neighbor (ARP/NDP) table entry constants.
//!
//! The neighbor subsystem manages the mapping between L3 addresses
//! (IP) and L2 addresses (MAC). These constants define neighbor
//! entry states, flags, and netlink attributes for ARP (IPv4) and
//! NDP (IPv6) cache entries.

// ---------------------------------------------------------------------------
// Neighbor entry states (NUD_*)
// ---------------------------------------------------------------------------

/// Incomplete: resolution in progress.
pub const NUD_INCOMPLETE: u16 = 0x01;
/// Reachable: recently confirmed reachability.
pub const NUD_REACHABLE: u16 = 0x02;
/// Stale: needs reconfirmation on next use.
pub const NUD_STALE: u16 = 0x04;
/// Delay: waiting before reconfirmation probe.
pub const NUD_DELAY: u16 = 0x08;
/// Probe: actively sending unicast probes.
pub const NUD_PROBE: u16 = 0x10;
/// Failed: resolution failed.
pub const NUD_FAILED: u16 = 0x20;
/// No ARP (static, not subject to timeouts).
pub const NUD_NOARP: u16 = 0x40;
/// Permanent: admin-set, never times out.
pub const NUD_PERMANENT: u16 = 0x80;
/// None: empty/uninitialized entry.
pub const NUD_NONE: u16 = 0x00;

// ---------------------------------------------------------------------------
// Neighbor entry flags (ndm_flags)
// ---------------------------------------------------------------------------

/// Neighbor is a proxy.
pub const NTF_PROXY: u8 = 0x08;
/// Used externally managed entry.
pub const NTF_EXT_LEARNED: u8 = 0x10;
/// Entry is offloaded to hardware.
pub const NTF_OFFLOADED: u8 = 0x20;
/// Sticky (don't change once resolved).
pub const NTF_STICKY: u8 = 0x40;
/// Router flag (NDP: is a router).
pub const NTF_ROUTER: u8 = 0x80;

// ---------------------------------------------------------------------------
// Neighbor netlink attributes (NDA_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const NDA_UNSPEC: u16 = 0;
/// Destination address (L3).
pub const NDA_DST: u16 = 1;
/// Link layer address (MAC).
pub const NDA_LLADDR: u16 = 2;
/// Cache info (timers).
pub const NDA_CACHEINFO: u16 = 3;
/// Proxy table entry.
pub const NDA_PROBES: u16 = 4;
/// VLAN ID.
pub const NDA_VLAN: u16 = 5;
/// Port (VXLAN, etc.).
pub const NDA_PORT: u16 = 6;
/// VNI (VXLAN network ID).
pub const NDA_VNI: u16 = 7;
/// Interface index.
pub const NDA_IFINDEX: u16 = 8;
/// Master interface index.
pub const NDA_MASTER: u16 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nud_states_no_overlap() {
        let states = [
            NUD_INCOMPLETE, NUD_REACHABLE, NUD_STALE,
            NUD_DELAY, NUD_PROBE, NUD_FAILED,
            NUD_NOARP, NUD_PERMANENT,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_eq!(states[i] & states[j], 0);
            }
        }
    }

    #[test]
    fn test_nud_states_power_of_two() {
        let states = [
            NUD_INCOMPLETE, NUD_REACHABLE, NUD_STALE,
            NUD_DELAY, NUD_PROBE, NUD_FAILED,
            NUD_NOARP, NUD_PERMANENT,
        ];
        for s in &states {
            assert!(s.is_power_of_two());
        }
    }

    #[test]
    fn test_nud_none_is_zero() {
        assert_eq!(NUD_NONE, 0);
    }

    #[test]
    fn test_ntf_flags_distinct() {
        let flags = [
            NTF_PROXY, NTF_EXT_LEARNED, NTF_OFFLOADED,
            NTF_STICKY, NTF_ROUTER,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_nda_attrs_distinct() {
        let attrs = [
            NDA_UNSPEC, NDA_DST, NDA_LLADDR, NDA_CACHEINFO,
            NDA_PROBES, NDA_VLAN, NDA_PORT, NDA_VNI,
            NDA_IFINDEX, NDA_MASTER,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
