//! `<linux/seg6.h>` — Segment Routing v6 (SRv6) constants.
//!
//! SRv6 extends IPv6 with a Segment Routing Header (SRH) that encodes
//! a list of waypoints (segments) in the packet. Each segment is an
//! IPv6 address; routers process the list to steer traffic through a
//! defined path. Used for traffic engineering, service chaining, VPNs,
//! and fast-reroute in carrier networks and data centers.

// ---------------------------------------------------------------------------
// Segment Routing Header (SRH) fields
// ---------------------------------------------------------------------------

/// SRH routing type (IPv6 Routing Header type 4).
pub const IPV6_SRCRT_TYPE_4: u32 = 4;

/// SRH first segment field — last entry in the segment list.
pub const SEG6_IPTUNNEL_SRH: u32 = 1;

// ---------------------------------------------------------------------------
// SRv6 inline mode flags
// ---------------------------------------------------------------------------

/// Encapsulation mode: outer IPv6 header + SRH.
pub const SEG6_IPTUN_MODE_ENCAP: u32 = 0;
/// Inline mode: SRH inserted into existing IPv6 header.
pub const SEG6_IPTUN_MODE_INLINE: u32 = 1;
/// L2 encapsulation mode.
pub const SEG6_IPTUN_MODE_L2ENCAP: u32 = 2;
/// Encapsulation with reduced SRH.
pub const SEG6_IPTUN_MODE_ENCAP_RED: u32 = 3;
/// L2 encapsulation with reduced SRH.
pub const SEG6_IPTUN_MODE_L2ENCAP_RED: u32 = 4;

// ---------------------------------------------------------------------------
// SRv6 local action (SEG6_LOCAL_ACTION)
// ---------------------------------------------------------------------------

/// End — basic SRv6 segment endpoint.
pub const SEG6_LOCAL_ACTION_END: u32 = 1;
/// End.X — endpoint with L3 cross-connect.
pub const SEG6_LOCAL_ACTION_END_X: u32 = 2;
/// End.T — endpoint with specific table lookup.
pub const SEG6_LOCAL_ACTION_END_T: u32 = 3;
/// End.DX2 — endpoint with decap + L2 cross-connect.
pub const SEG6_LOCAL_ACTION_END_DX2: u32 = 4;
/// End.DX6 — endpoint with decap + IPv6 cross-connect.
pub const SEG6_LOCAL_ACTION_END_DX6: u32 = 5;
/// End.DX4 — endpoint with decap + IPv4 cross-connect.
pub const SEG6_LOCAL_ACTION_END_DX4: u32 = 6;
/// End.DT6 — endpoint with decap + IPv6 table lookup.
pub const SEG6_LOCAL_ACTION_END_DT6: u32 = 7;
/// End.DT4 — endpoint with decap + IPv4 table lookup.
pub const SEG6_LOCAL_ACTION_END_DT4: u32 = 8;
/// End.B6 — endpoint bound to SRv6 policy.
pub const SEG6_LOCAL_ACTION_END_B6: u32 = 9;
/// End.B6.Encaps — endpoint bound to SRv6 encaps policy.
pub const SEG6_LOCAL_ACTION_END_B6_ENCAPS: u32 = 10;
/// End.BM — endpoint bound to SR-MPLS policy.
pub const SEG6_LOCAL_ACTION_END_BM: u32 = 11;
/// End.DT46 — endpoint with decap + dual-stack table lookup.
pub const SEG6_LOCAL_ACTION_END_DT46: u32 = 17;

// ---------------------------------------------------------------------------
// SRv6 local attributes (netlink)
// ---------------------------------------------------------------------------

/// SRH attribute.
pub const SEG6_LOCAL_SRH: u32 = 1;
/// Table attribute.
pub const SEG6_LOCAL_TABLE: u32 = 2;
/// NH4 (IPv4 next hop) attribute.
pub const SEG6_LOCAL_NH4: u32 = 3;
/// NH6 (IPv6 next hop) attribute.
pub const SEG6_LOCAL_NH6: u32 = 4;
/// IIF (input interface) attribute.
pub const SEG6_LOCAL_IIF: u32 = 5;
/// OIF (output interface) attribute.
pub const SEG6_LOCAL_OIF: u32 = 6;
/// BPF program attribute.
pub const SEG6_LOCAL_BPF: u32 = 7;
/// VRF table attribute.
pub const SEG6_LOCAL_VRFTABLE: u32 = 8;
/// Counters attribute.
pub const SEG6_LOCAL_COUNTERS: u32 = 9;
/// Flavors attribute.
pub const SEG6_LOCAL_FLAVORS: u32 = 10;

// ---------------------------------------------------------------------------
// SRv6 End flavors
// ---------------------------------------------------------------------------

/// Penultimate Segment Pop (PSP) flavor.
pub const SEG6_LOCAL_FLV_PSP: u32 = 1 << 0;
/// Ultimate Segment Pop (USP) flavor.
pub const SEG6_LOCAL_FLV_USP: u32 = 1 << 1;
/// Ultimate Segment Decap (USD) flavor.
pub const SEG6_LOCAL_FLV_USD: u32 = 1 << 2;
/// Next C-SID flavor.
pub const SEG6_LOCAL_FLV_NEXT_CSID: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tunnel_modes_distinct() {
        let modes = [
            SEG6_IPTUN_MODE_ENCAP, SEG6_IPTUN_MODE_INLINE,
            SEG6_IPTUN_MODE_L2ENCAP, SEG6_IPTUN_MODE_ENCAP_RED,
            SEG6_IPTUN_MODE_L2ENCAP_RED,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_local_actions_distinct() {
        let actions = [
            SEG6_LOCAL_ACTION_END, SEG6_LOCAL_ACTION_END_X,
            SEG6_LOCAL_ACTION_END_T, SEG6_LOCAL_ACTION_END_DX2,
            SEG6_LOCAL_ACTION_END_DX6, SEG6_LOCAL_ACTION_END_DX4,
            SEG6_LOCAL_ACTION_END_DT6, SEG6_LOCAL_ACTION_END_DT4,
            SEG6_LOCAL_ACTION_END_B6, SEG6_LOCAL_ACTION_END_B6_ENCAPS,
            SEG6_LOCAL_ACTION_END_BM, SEG6_LOCAL_ACTION_END_DT46,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_local_attrs_distinct() {
        let attrs = [
            SEG6_LOCAL_SRH, SEG6_LOCAL_TABLE,
            SEG6_LOCAL_NH4, SEG6_LOCAL_NH6,
            SEG6_LOCAL_IIF, SEG6_LOCAL_OIF,
            SEG6_LOCAL_BPF, SEG6_LOCAL_VRFTABLE,
            SEG6_LOCAL_COUNTERS, SEG6_LOCAL_FLAVORS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_flavors_no_overlap() {
        let flavors = [
            SEG6_LOCAL_FLV_PSP, SEG6_LOCAL_FLV_USP,
            SEG6_LOCAL_FLV_USD, SEG6_LOCAL_FLV_NEXT_CSID,
        ];
        for i in 0..flavors.len() {
            assert!(flavors[i].is_power_of_two());
            for j in (i + 1)..flavors.len() {
                assert_eq!(flavors[i] & flavors[j], 0);
            }
        }
    }

    #[test]
    fn test_srh_routing_type() {
        assert_eq!(IPV6_SRCRT_TYPE_4, 4);
    }
}
