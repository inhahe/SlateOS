//! `<linux/tc_act/tc_mirred.h>` — TC mirror/redirect action constants.
//!
//! The mirred action mirrors (copies) or redirects packets to
//! another network interface. It is commonly used for port mirroring
//! (SPAN), traffic redirection to tunnel endpoints, and hardware
//! offload of forwarding rules.

// ---------------------------------------------------------------------------
// Mirred action types (TCA_EGRESS_*/TCA_INGRESS_*)
// ---------------------------------------------------------------------------

/// Redirect packet to egress of target device.
pub const TCA_EGRESS_REDIR: u8 = 1;
/// Mirror packet to egress of target device.
pub const TCA_EGRESS_MIRROR: u8 = 2;
/// Redirect packet to ingress of target device.
pub const TCA_INGRESS_REDIR: u8 = 3;
/// Mirror packet to ingress of target device.
pub const TCA_INGRESS_MIRROR: u8 = 4;

// ---------------------------------------------------------------------------
// Mirred netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified attribute.
pub const TCA_MIRRED_UNSPEC: u16 = 0;
/// Timer info.
pub const TCA_MIRRED_TM: u16 = 1;
/// Mirred parameters.
pub const TCA_MIRRED_PARMS: u16 = 2;
/// Padding.
pub const TCA_MIRRED_PAD: u16 = 3;
/// Block index.
pub const TCA_MIRRED_BLOCKID: u16 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mirred_actions_distinct() {
        let actions = [
            TCA_EGRESS_REDIR,
            TCA_EGRESS_MIRROR,
            TCA_INGRESS_REDIR,
            TCA_INGRESS_MIRROR,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_MIRRED_UNSPEC,
            TCA_MIRRED_TM,
            TCA_MIRRED_PARMS,
            TCA_MIRRED_PAD,
            TCA_MIRRED_BLOCKID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
