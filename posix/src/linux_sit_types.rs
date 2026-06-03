//! `<linux/if_tunnel.h>` — SIT (Simple Internet Transition) tunnel constants.
//!
//! SIT tunnels carry IPv6 traffic over IPv4 networks.
//! These constants define SIT modes, flags, 6rd parameters,
//! and ISATAP settings.

// ---------------------------------------------------------------------------
// SIT tunnel modes
// ---------------------------------------------------------------------------

/// Simple 6-in-4 encapsulation.
pub const SIT_MODE_SIMPLE: u32 = 0;
/// 6rd (IPv6 Rapid Deployment).
pub const SIT_MODE_6RD: u32 = 1;
/// ISATAP (Intra-Site Automatic Tunnel Addressing Protocol).
pub const SIT_MODE_ISATAP: u32 = 2;
/// Any (accept all).
pub const SIT_MODE_ANY: u32 = 3;

// ---------------------------------------------------------------------------
// SIT tunnel flags
// ---------------------------------------------------------------------------

/// Sequence numbering.
pub const SIT_F_SEQ: u32 = 1 << 0;
/// Checksum.
pub const SIT_F_CSUM: u32 = 1 << 1;
/// Key.
pub const SIT_F_KEY: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// SIT netlink attribute types (IFLA_SIT_*)
// (shares some with IFLA_IPTUN but has SIT-specific ones)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const IFLA_SIT_UNSPEC: u32 = 0;
/// Link (parent interface).
pub const IFLA_SIT_LINK: u32 = 1;
/// Local address.
pub const IFLA_SIT_LOCAL: u32 = 2;
/// Remote address.
pub const IFLA_SIT_REMOTE: u32 = 3;
/// TTL.
pub const IFLA_SIT_TTL: u32 = 4;
/// TOS.
pub const IFLA_SIT_TOS: u32 = 5;
/// Flags.
pub const IFLA_SIT_FLAGS: u32 = 6;
/// Protocol.
pub const IFLA_SIT_PROTO: u32 = 7;
/// PMTU discovery.
pub const IFLA_SIT_PMTUDISC: u32 = 8;
/// 6RD prefix.
pub const IFLA_SIT_6RD_PREFIX: u32 = 9;
/// 6RD relay prefix.
pub const IFLA_SIT_6RD_RELAY_PREFIX: u32 = 10;
/// 6RD prefix length.
pub const IFLA_SIT_6RD_PREFIXLEN: u32 = 11;
/// 6RD relay prefix length.
pub const IFLA_SIT_6RD_RELAY_PREFIXLEN: u32 = 12;
/// Encapsulation type.
pub const IFLA_SIT_ENCAP_TYPE: u32 = 13;
/// Encapsulation flags.
pub const IFLA_SIT_ENCAP_FLAGS: u32 = 14;
/// Encapsulation source port.
pub const IFLA_SIT_ENCAP_SPORT: u32 = 15;
/// Encapsulation dest port.
pub const IFLA_SIT_ENCAP_DPORT: u32 = 16;
/// Collect metadata.
pub const IFLA_SIT_COLLECT_METADATA: u32 = 17;
/// FW mark.
pub const IFLA_SIT_FWMARK: u32 = 18;

// ---------------------------------------------------------------------------
// ISATAP router entry flags
// ---------------------------------------------------------------------------

/// Router is reachable.
pub const ISATAP_F_ROUTER: u32 = 1 << 0;
/// Router is potential.
pub const ISATAP_F_POTENTIAL: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [SIT_MODE_SIMPLE, SIT_MODE_6RD, SIT_MODE_ISATAP, SIT_MODE_ANY];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [SIT_F_SEQ, SIT_F_CSUM, SIT_F_KEY];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [SIT_F_SEQ, SIT_F_CSUM, SIT_F_KEY];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            IFLA_SIT_UNSPEC,
            IFLA_SIT_LINK,
            IFLA_SIT_LOCAL,
            IFLA_SIT_REMOTE,
            IFLA_SIT_TTL,
            IFLA_SIT_TOS,
            IFLA_SIT_FLAGS,
            IFLA_SIT_PROTO,
            IFLA_SIT_PMTUDISC,
            IFLA_SIT_6RD_PREFIX,
            IFLA_SIT_6RD_RELAY_PREFIX,
            IFLA_SIT_6RD_PREFIXLEN,
            IFLA_SIT_6RD_RELAY_PREFIXLEN,
            IFLA_SIT_ENCAP_TYPE,
            IFLA_SIT_ENCAP_FLAGS,
            IFLA_SIT_ENCAP_SPORT,
            IFLA_SIT_ENCAP_DPORT,
            IFLA_SIT_COLLECT_METADATA,
            IFLA_SIT_FWMARK,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_isatap_flags_no_overlap() {
        assert_eq!(ISATAP_F_ROUTER & ISATAP_F_POTENTIAL, 0);
    }

    #[test]
    fn test_simple_is_zero() {
        assert_eq!(SIT_MODE_SIMPLE, 0);
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(IFLA_SIT_UNSPEC, 0);
    }
}
