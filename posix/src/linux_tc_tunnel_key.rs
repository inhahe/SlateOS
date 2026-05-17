//! `<linux/tc_act/tc_tunnel_key.h>` — TC tunnel key action constants.
//!
//! The tunnel_key action sets or releases tunnel metadata on packets.
//! It is used to encapsulate packets into tunnels (VXLAN, Geneve,
//! GRE, etc.) by attaching tunnel key information that the tunnel
//! device then uses for encapsulation.

// ---------------------------------------------------------------------------
// Tunnel key action types
// ---------------------------------------------------------------------------

/// Set tunnel key (encapsulate).
pub const TCA_TUNNEL_KEY_ACT_SET: u8 = 1;
/// Release tunnel key (decapsulate).
pub const TCA_TUNNEL_KEY_ACT_RELEASE: u8 = 2;

// ---------------------------------------------------------------------------
// Tunnel key netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified attribute.
pub const TCA_TUNNEL_KEY_UNSPEC: u16 = 0;
/// Timer info.
pub const TCA_TUNNEL_KEY_TM: u16 = 1;
/// Parameters.
pub const TCA_TUNNEL_KEY_PARMS: u16 = 2;
/// Tunnel ID (VNI/VSID).
pub const TCA_TUNNEL_KEY_ENC_KEY_ID: u16 = 3;
/// Tunnel IPv4 source.
pub const TCA_TUNNEL_KEY_ENC_IPV4_SRC: u16 = 4;
/// Tunnel IPv4 destination.
pub const TCA_TUNNEL_KEY_ENC_IPV4_DST: u16 = 5;
/// Tunnel IPv6 source.
pub const TCA_TUNNEL_KEY_ENC_IPV6_SRC: u16 = 6;
/// Tunnel IPv6 destination.
pub const TCA_TUNNEL_KEY_ENC_IPV6_DST: u16 = 7;
/// Tunnel destination port.
pub const TCA_TUNNEL_KEY_ENC_DST_PORT: u16 = 8;
/// Don't fragment flag.
pub const TCA_TUNNEL_KEY_NO_CSUM: u16 = 9;
/// Padding.
pub const TCA_TUNNEL_KEY_PAD: u16 = 10;
/// Tunnel TOS.
pub const TCA_TUNNEL_KEY_ENC_TOS: u16 = 11;
/// Tunnel TTL.
pub const TCA_TUNNEL_KEY_ENC_TTL: u16 = 12;
/// Geneve options.
pub const TCA_TUNNEL_KEY_ENC_OPTS: u16 = 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        assert_ne!(TCA_TUNNEL_KEY_ACT_SET, TCA_TUNNEL_KEY_ACT_RELEASE);
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_TUNNEL_KEY_UNSPEC, TCA_TUNNEL_KEY_TM,
            TCA_TUNNEL_KEY_PARMS, TCA_TUNNEL_KEY_ENC_KEY_ID,
            TCA_TUNNEL_KEY_ENC_IPV4_SRC, TCA_TUNNEL_KEY_ENC_IPV4_DST,
            TCA_TUNNEL_KEY_ENC_IPV6_SRC, TCA_TUNNEL_KEY_ENC_IPV6_DST,
            TCA_TUNNEL_KEY_ENC_DST_PORT, TCA_TUNNEL_KEY_NO_CSUM,
            TCA_TUNNEL_KEY_PAD, TCA_TUNNEL_KEY_ENC_TOS,
            TCA_TUNNEL_KEY_ENC_TTL, TCA_TUNNEL_KEY_ENC_OPTS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
