//! `<linux/seg6.h>` + `<linux/seg6_iptunnel.h>` — Segment Routing v6 (SRv6) constants.
//!
//! SRv6 uses IPv6 extension headers to encode a list of network
//! segments (waypoints) that a packet must traverse. Used by
//! iproute2 `ip -6 route add encap seg6` and carrier/datacenter
//! network orchestration.

// ---------------------------------------------------------------------------
// SRv6 action types (for seg6_iptunnel)
// ---------------------------------------------------------------------------

/// Inline insertion (insert SRH into existing packet).
pub const SEG6_IPTUN_MODE_INLINE: u32 = 0;
/// Encapsulation (outer IPv6 header + SRH).
pub const SEG6_IPTUN_MODE_ENCAP: u32 = 1;
/// L2 encapsulation.
pub const SEG6_IPTUN_MODE_L2ENCAP: u32 = 2;

// ---------------------------------------------------------------------------
// Segment Routing Header (SRH) attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const SEG6_IPTUNNEL_UNSPEC: u16 = 0;
/// SRH.
pub const SEG6_IPTUNNEL_SRH: u16 = 1;

// ---------------------------------------------------------------------------
// SRv6 local action types
// ---------------------------------------------------------------------------

/// Unspecified.
pub const SEG6_LOCAL_UNSPEC: u16 = 0;
/// Action type.
pub const SEG6_LOCAL_ACTION: u16 = 1;
/// SRH.
pub const SEG6_LOCAL_SRH: u16 = 2;
/// Table.
pub const SEG6_LOCAL_TABLE: u16 = 3;
/// NH4 (next-hop IPv4).
pub const SEG6_LOCAL_NH4: u16 = 4;
/// NH6 (next-hop IPv6).
pub const SEG6_LOCAL_NH6: u16 = 5;
/// Interface index.
pub const SEG6_LOCAL_IIF: u16 = 6;
/// OIF.
pub const SEG6_LOCAL_OIF: u16 = 7;
/// BPF.
pub const SEG6_LOCAL_BPF: u16 = 8;

// ---------------------------------------------------------------------------
// SRv6 End behaviors
// ---------------------------------------------------------------------------

/// End (basic SRv6 endpoint).
pub const SEG6_LOCAL_ACTION_END: u32 = 1;
/// End.X (endpoint with L3 cross-connect).
pub const SEG6_LOCAL_ACTION_END_X: u32 = 2;
/// End.T (endpoint with table lookup).
pub const SEG6_LOCAL_ACTION_END_T: u32 = 3;
/// End.DX2 (endpoint with L2 cross-connect).
pub const SEG6_LOCAL_ACTION_END_DX2: u32 = 4;
/// End.DX6 (endpoint with IPv6 decap + cross-connect).
pub const SEG6_LOCAL_ACTION_END_DX6: u32 = 5;
/// End.DX4 (endpoint with IPv4 decap + cross-connect).
pub const SEG6_LOCAL_ACTION_END_DX4: u32 = 6;
/// End.DT6 (endpoint with IPv6 decap + table lookup).
pub const SEG6_LOCAL_ACTION_END_DT6: u32 = 7;
/// End.DT4 (endpoint with IPv4 decap + table lookup).
pub const SEG6_LOCAL_ACTION_END_DT4: u32 = 8;
/// End.B6 (endpoint bound to SRv6 policy).
pub const SEG6_LOCAL_ACTION_END_B6: u32 = 9;
/// End.B6.Encaps.
pub const SEG6_LOCAL_ACTION_END_B6_ENCAPS: u32 = 10;
/// End.BPF (endpoint with BPF program).
pub const SEG6_LOCAL_ACTION_END_BPF: u32 = 15;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iptun_modes() {
        assert_eq!(SEG6_IPTUN_MODE_INLINE, 0);
        assert_eq!(SEG6_IPTUN_MODE_ENCAP, 1);
        assert_eq!(SEG6_IPTUN_MODE_L2ENCAP, 2);
    }

    #[test]
    fn test_local_attrs_distinct() {
        let attrs = [
            SEG6_LOCAL_UNSPEC,
            SEG6_LOCAL_ACTION,
            SEG6_LOCAL_SRH,
            SEG6_LOCAL_TABLE,
            SEG6_LOCAL_NH4,
            SEG6_LOCAL_NH6,
            SEG6_LOCAL_IIF,
            SEG6_LOCAL_OIF,
            SEG6_LOCAL_BPF,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_actions_distinct() {
        let actions = [
            SEG6_LOCAL_ACTION_END,
            SEG6_LOCAL_ACTION_END_X,
            SEG6_LOCAL_ACTION_END_T,
            SEG6_LOCAL_ACTION_END_DX2,
            SEG6_LOCAL_ACTION_END_DX6,
            SEG6_LOCAL_ACTION_END_DX4,
            SEG6_LOCAL_ACTION_END_DT6,
            SEG6_LOCAL_ACTION_END_DT4,
            SEG6_LOCAL_ACTION_END_B6,
            SEG6_LOCAL_ACTION_END_B6_ENCAPS,
            SEG6_LOCAL_ACTION_END_BPF,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_action_sequential() {
        assert_eq!(SEG6_LOCAL_ACTION_END, 1);
        assert_eq!(SEG6_LOCAL_ACTION_END_X, 2);
        assert_eq!(SEG6_LOCAL_ACTION_END_T, 3);
    }
}
