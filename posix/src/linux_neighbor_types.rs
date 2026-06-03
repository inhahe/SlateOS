//! `<linux/neighbour.h>` — Neighbor (ARP/NDP) table constants.
//!
//! The neighbor subsystem manages ARP (IPv4) and NDP (IPv6)
//! caches.  These constants define neighbor entry states,
//! flags, attribute types, and table parameters.

// ---------------------------------------------------------------------------
// Neighbor entry states (NUD_*)
// ---------------------------------------------------------------------------

/// Incomplete (resolution in progress).
pub const NUD_INCOMPLETE: u16 = 0x01;
/// Reachable (confirmed recently).
pub const NUD_REACHABLE: u16 = 0x02;
/// Stale (needs re-confirmation).
pub const NUD_STALE: u16 = 0x04;
/// Delay (waiting for confirmation).
pub const NUD_DELAY: u16 = 0x08;
/// Probe (actively probing).
pub const NUD_PROBE: u16 = 0x10;
/// Failed (resolution failed).
pub const NUD_FAILED: u16 = 0x20;
/// No ARP (static, no resolution needed).
pub const NUD_NOARP: u16 = 0x40;
/// Permanent (never expires).
pub const NUD_PERMANENT: u16 = 0x80;
/// No state (not in neighbor table).
pub const NUD_NONE: u16 = 0x00;

// ---------------------------------------------------------------------------
// Neighbor entry flags (NTF_*)
// ---------------------------------------------------------------------------

/// Use (entry is in use).
pub const NTF_USE: u8 = 0x01;
/// Self (entry for local address).
pub const NTF_SELF: u8 = 0x02;
/// Master (on bridge master device).
pub const NTF_MASTER: u8 = 0x04;
/// Proxy (proxy ARP entry).
pub const NTF_PROXY: u8 = 0x08;
/// Externally learned.
pub const NTF_EXT_LEARNED: u8 = 0x10;
/// Offloaded to hardware.
pub const NTF_OFFLOADED: u8 = 0x20;
/// Sticky (don't age out).
pub const NTF_STICKY: u8 = 0x40;
/// Router (neighbor is a router).
pub const NTF_ROUTER: u8 = 0x80;

// ---------------------------------------------------------------------------
// Neighbor attribute types (NDA_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const NDA_UNSPEC: u16 = 0;
/// Destination address.
pub const NDA_DST: u16 = 1;
/// Link-layer address.
pub const NDA_LLADDR: u16 = 2;
/// Cache info.
pub const NDA_CACHEINFO: u16 = 3;
/// Probes sent.
pub const NDA_PROBES: u16 = 4;
/// VLAN ID.
pub const NDA_VLAN: u16 = 5;
/// Port (for bridge FDB).
pub const NDA_PORT: u16 = 6;
/// VNI (VXLAN).
pub const NDA_VNI: u16 = 7;
/// Interface index.
pub const NDA_IFINDEX: u16 = 8;
/// Master interface.
pub const NDA_MASTER: u16 = 9;
/// Link netns ID.
pub const NDA_LINK_NETNSID: u16 = 10;
/// Source VNI.
pub const NDA_SRC_VNI: u16 = 11;
/// Protocol.
pub const NDA_PROTOCOL: u16 = 12;
/// NH ID (nexthop).
pub const NDA_NH_ID: u16 = 13;
/// FDB ext attrs.
pub const NDA_FDB_EXT_ATTRS: u16 = 14;
/// Flags extension.
pub const NDA_FLAGS_EXT: u16 = 15;

// ---------------------------------------------------------------------------
// Neighbor table parameters (NDTPA_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const NDTPA_UNSPEC: u16 = 0;
/// Interface index.
pub const NDTPA_IFINDEX: u16 = 1;
/// Reference count.
pub const NDTPA_REFCNT: u16 = 2;
/// Reachable time (ms).
pub const NDTPA_REACHABLE_TIME: u16 = 3;
/// Base reachable time (ms).
pub const NDTPA_BASE_REACHABLE_TIME: u16 = 4;
/// Retransmit timer (ms).
pub const NDTPA_RETRANS_TIME: u16 = 5;
/// GC staleness (ms).
pub const NDTPA_GC_STALETIME: u16 = 6;
/// Delay before first probe (ms).
pub const NDTPA_DELAY_PROBE_TIME: u16 = 7;
/// Queue length.
pub const NDTPA_QUEUE_LEN: u16 = 8;
/// App probes.
pub const NDTPA_APP_PROBES: u16 = 9;
/// Unicast probes.
pub const NDTPA_UCAST_PROBES: u16 = 10;
/// Multicast probes.
pub const NDTPA_MCAST_PROBES: u16 = 11;
/// Anycast delay.
pub const NDTPA_ANYCAST_DELAY: u16 = 12;
/// Proxy delay.
pub const NDTPA_PROXY_DELAY: u16 = 13;
/// Proxy queue length.
pub const NDTPA_PROXY_QLEN: u16 = 14;
/// Locktime (ms).
pub const NDTPA_LOCKTIME: u16 = 15;
/// Queue length in bytes.
pub const NDTPA_QUEUE_LENBYTES: u16 = 16;
/// Multicast reprobes.
pub const NDTPA_MCAST_REPROBES: u16 = 17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_are_powers_of_two() {
        let states = [
            NUD_INCOMPLETE,
            NUD_REACHABLE,
            NUD_STALE,
            NUD_DELAY,
            NUD_PROBE,
            NUD_FAILED,
            NUD_NOARP,
            NUD_PERMANENT,
        ];
        for s in &states {
            assert!(
                s.is_power_of_two(),
                "NUD state {s:#06x} is not power of two"
            );
        }
    }

    #[test]
    fn test_states_no_overlap() {
        let states = [
            NUD_INCOMPLETE,
            NUD_REACHABLE,
            NUD_STALE,
            NUD_DELAY,
            NUD_PROBE,
            NUD_FAILED,
            NUD_NOARP,
            NUD_PERMANENT,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_eq!(states[i] & states[j], 0);
            }
        }
    }

    #[test]
    fn test_none_is_zero() {
        assert_eq!(NUD_NONE, 0);
    }

    #[test]
    fn test_flags_are_powers_of_two() {
        let flags = [
            NTF_USE,
            NTF_SELF,
            NTF_MASTER,
            NTF_PROXY,
            NTF_EXT_LEARNED,
            NTF_OFFLOADED,
            NTF_STICKY,
            NTF_ROUTER,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "NTF flag {f:#04x} is not power of two");
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            NTF_USE,
            NTF_SELF,
            NTF_MASTER,
            NTF_PROXY,
            NTF_EXT_LEARNED,
            NTF_OFFLOADED,
            NTF_STICKY,
            NTF_ROUTER,
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
            NDA_UNSPEC,
            NDA_DST,
            NDA_LLADDR,
            NDA_CACHEINFO,
            NDA_PROBES,
            NDA_VLAN,
            NDA_PORT,
            NDA_VNI,
            NDA_IFINDEX,
            NDA_MASTER,
            NDA_LINK_NETNSID,
            NDA_SRC_VNI,
            NDA_PROTOCOL,
            NDA_NH_ID,
            NDA_FDB_EXT_ATTRS,
            NDA_FLAGS_EXT,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_ndtpa_params_distinct() {
        let params = [
            NDTPA_UNSPEC,
            NDTPA_IFINDEX,
            NDTPA_REFCNT,
            NDTPA_REACHABLE_TIME,
            NDTPA_BASE_REACHABLE_TIME,
            NDTPA_RETRANS_TIME,
            NDTPA_GC_STALETIME,
            NDTPA_DELAY_PROBE_TIME,
            NDTPA_QUEUE_LEN,
            NDTPA_APP_PROBES,
            NDTPA_UCAST_PROBES,
            NDTPA_MCAST_PROBES,
            NDTPA_ANYCAST_DELAY,
            NDTPA_PROXY_DELAY,
            NDTPA_PROXY_QLEN,
            NDTPA_LOCKTIME,
            NDTPA_QUEUE_LENBYTES,
            NDTPA_MCAST_REPROBES,
        ];
        for i in 0..params.len() {
            for j in (i + 1)..params.len() {
                assert_ne!(params[i], params[j]);
            }
        }
    }

    #[test]
    fn test_nda_unspec_is_zero() {
        assert_eq!(NDA_UNSPEC, 0);
    }
}
