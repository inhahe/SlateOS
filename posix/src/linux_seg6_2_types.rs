//! `<linux/seg6.h>` — Additional SRv6 (Segment Routing v6) constants.
//!
//! Supplementary SRv6 constants covering action types,
//! SRH flags, and BPF SRv6 helpers.

// ---------------------------------------------------------------------------
// SEG6 action types (SEG6_LOCAL_ACTION_*)
// ---------------------------------------------------------------------------

/// Unspec action.
pub const SEG6_LOCAL_ACTION_UNSPEC: u32 = 0;
/// End (node segment).
pub const SEG6_LOCAL_ACTION_END: u32 = 1;
/// End.X (cross-connect).
pub const SEG6_LOCAL_ACTION_END_X: u32 = 2;
/// End.T (lookup in table).
pub const SEG6_LOCAL_ACTION_END_T: u32 = 3;
/// End.DX2 (decap + L2 cross-connect).
pub const SEG6_LOCAL_ACTION_END_DX2: u32 = 4;
/// End.DX6 (decap + IPv6 cross-connect).
pub const SEG6_LOCAL_ACTION_END_DX6: u32 = 5;
/// End.DX4 (decap + IPv4 cross-connect).
pub const SEG6_LOCAL_ACTION_END_DX4: u32 = 6;
/// End.DT6 (decap + IPv6 table).
pub const SEG6_LOCAL_ACTION_END_DT6: u32 = 7;
/// End.DT4 (decap + IPv4 table).
pub const SEG6_LOCAL_ACTION_END_DT4: u32 = 8;
/// End.B6 (binding SID).
pub const SEG6_LOCAL_ACTION_END_B6: u32 = 9;
/// End.B6.Encaps (binding SID with encap).
pub const SEG6_LOCAL_ACTION_END_B6_ENCAPS: u32 = 10;
/// BPF action.
pub const SEG6_LOCAL_ACTION_END_BPF: u32 = 11;
/// End.DT46 (dual-stack decap + table).
pub const SEG6_LOCAL_ACTION_END_DT46: u32 = 12;

// ---------------------------------------------------------------------------
// SRH (Segment Routing Header) flags
// ---------------------------------------------------------------------------

/// HMAC flag present.
pub const SR6_FLAG_HMAC: u32 = 1 << 3;
/// Alert flag.
pub const SR6_FLAG_ALERT: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// SEG6 encap modes
// ---------------------------------------------------------------------------

/// Inline encapsulation (modify existing SRH).
pub const SEG6_IPTUN_MODE_INLINE: u32 = 0;
/// Encap mode (add new outer IPv6 + SRH).
pub const SEG6_IPTUN_MODE_ENCAP: u32 = 1;
/// L2 encap mode.
pub const SEG6_IPTUN_MODE_L2ENCAP: u32 = 2;
/// Encap with reduced SRH.
pub const SEG6_IPTUN_MODE_ENCAP_RED: u32 = 3;
/// L2 encap with reduced SRH.
pub const SEG6_IPTUN_MODE_L2ENCAP_RED: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let actions = [
            SEG6_LOCAL_ACTION_UNSPEC,
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
            SEG6_LOCAL_ACTION_END_DT46,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_srh_flags_distinct() {
        assert_ne!(SR6_FLAG_HMAC, SR6_FLAG_ALERT);
        assert_eq!(SR6_FLAG_HMAC & SR6_FLAG_ALERT, 0);
    }

    #[test]
    fn test_encap_modes_distinct() {
        let modes = [
            SEG6_IPTUN_MODE_INLINE,
            SEG6_IPTUN_MODE_ENCAP,
            SEG6_IPTUN_MODE_L2ENCAP,
            SEG6_IPTUN_MODE_ENCAP_RED,
            SEG6_IPTUN_MODE_L2ENCAP_RED,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(SEG6_LOCAL_ACTION_UNSPEC, 0);
    }
}
