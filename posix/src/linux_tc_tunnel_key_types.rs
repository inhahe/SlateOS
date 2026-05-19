//! `<linux/tc_act/tc_tunnel_key.h>` — TC tunnel key action constants.
//!
//! Traffic control tunnel key action constants covering attribute types
//! and action commands for tunnel encapsulation/decapsulation.

// ---------------------------------------------------------------------------
// TC tunnel key action commands
// ---------------------------------------------------------------------------

/// Set tunnel key.
pub const TCA_TUNNEL_KEY_ACT_SET: u32 = 1;
/// Release tunnel key.
pub const TCA_TUNNEL_KEY_ACT_RELEASE: u32 = 2;

// ---------------------------------------------------------------------------
// TC tunnel key attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_TUNNEL_KEY_UNSPEC: u32 = 0;
/// Timestamp.
pub const TCA_TUNNEL_KEY_TM: u32 = 1;
/// Parameters.
pub const TCA_TUNNEL_KEY_PARMS: u32 = 2;
/// Encap IPv4 source.
pub const TCA_TUNNEL_KEY_ENC_IPV4_SRC: u32 = 3;
/// Encap IPv4 destination.
pub const TCA_TUNNEL_KEY_ENC_IPV4_DST: u32 = 4;
/// Encap IPv6 source.
pub const TCA_TUNNEL_KEY_ENC_IPV6_SRC: u32 = 5;
/// Encap IPv6 destination.
pub const TCA_TUNNEL_KEY_ENC_IPV6_DST: u32 = 6;
/// Encap key ID.
pub const TCA_TUNNEL_KEY_ENC_KEY_ID: u32 = 7;
/// Encap destination port.
pub const TCA_TUNNEL_KEY_ENC_DST_PORT: u32 = 9;
/// No csum.
pub const TCA_TUNNEL_KEY_NO_CSUM: u32 = 10;
/// Encap options.
pub const TCA_TUNNEL_KEY_ENC_OPTS: u32 = 11;
/// Encap TOS.
pub const TCA_TUNNEL_KEY_ENC_TOS: u32 = 12;
/// Encap TTL.
pub const TCA_TUNNEL_KEY_ENC_TTL: u32 = 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_cmds_distinct() {
        assert_ne!(TCA_TUNNEL_KEY_ACT_SET, TCA_TUNNEL_KEY_ACT_RELEASE);
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_TUNNEL_KEY_UNSPEC, TCA_TUNNEL_KEY_TM,
            TCA_TUNNEL_KEY_PARMS, TCA_TUNNEL_KEY_ENC_IPV4_SRC,
            TCA_TUNNEL_KEY_ENC_IPV4_DST, TCA_TUNNEL_KEY_ENC_IPV6_SRC,
            TCA_TUNNEL_KEY_ENC_IPV6_DST, TCA_TUNNEL_KEY_ENC_KEY_ID,
            TCA_TUNNEL_KEY_ENC_DST_PORT, TCA_TUNNEL_KEY_NO_CSUM,
            TCA_TUNNEL_KEY_ENC_OPTS, TCA_TUNNEL_KEY_ENC_TOS,
            TCA_TUNNEL_KEY_ENC_TTL,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
