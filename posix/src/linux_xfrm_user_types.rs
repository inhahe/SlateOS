//! `<linux/xfrm.h>` — IPsec xfrm user-netlink constants.
//!
//! Constants used by `ip xfrm` (iproute2) and IKE daemons
//! (strongswan, libreswan) to install IPsec policies and SAs via
//! `AF_NETLINK` / `NETLINK_XFRM`.

// ---------------------------------------------------------------------------
// Netlink message types (struct nlmsghdr.nlmsg_type)
// ---------------------------------------------------------------------------

/// Add new SA.
pub const XFRM_MSG_NEWSA: u16 = 0x10;
/// Delete SA.
pub const XFRM_MSG_DELSA: u16 = 0x11;
/// Get SA.
pub const XFRM_MSG_GETSA: u16 = 0x12;
/// Add new policy.
pub const XFRM_MSG_NEWPOLICY: u16 = 0x13;
/// Delete policy.
pub const XFRM_MSG_DELPOLICY: u16 = 0x14;
/// Get policy.
pub const XFRM_MSG_GETPOLICY: u16 = 0x15;
/// Allocate an SPI.
pub const XFRM_MSG_ALLOCSPI: u16 = 0x16;
/// Acquire — userspace must establish an SA.
pub const XFRM_MSG_ACQUIRE: u16 = 0x17;
/// SA expired.
pub const XFRM_MSG_EXPIRE: u16 = 0x18;
/// Update policy.
pub const XFRM_MSG_UPDPOLICY: u16 = 0x19;
/// Update SA.
pub const XFRM_MSG_UPDSA: u16 = 0x1a;
/// Policy expired.
pub const XFRM_MSG_POLEXPIRE: u16 = 0x1b;
/// Flush SAs.
pub const XFRM_MSG_FLUSHSA: u16 = 0x1c;
/// Flush policies.
pub const XFRM_MSG_FLUSHPOLICY: u16 = 0x1d;

// ---------------------------------------------------------------------------
// Policy direction (struct xfrm_userpolicy_id.dir)
// ---------------------------------------------------------------------------

/// Inbound policy.
pub const XFRM_POLICY_IN: u8 = 0;
/// Outbound policy.
pub const XFRM_POLICY_OUT: u8 = 1;
/// Forwarded policy.
pub const XFRM_POLICY_FWD: u8 = 2;
/// Number of distinct directions.
pub const XFRM_POLICY_MAX: u8 = 3;

// ---------------------------------------------------------------------------
// Policy action (struct xfrm_userpolicy_info.action)
// ---------------------------------------------------------------------------

/// Allow / use IPsec.
pub const XFRM_POLICY_ALLOW: u8 = 0;
/// Block — drop matching traffic.
pub const XFRM_POLICY_BLOCK: u8 = 1;

// ---------------------------------------------------------------------------
// SA modes (struct xfrm_usersa_info.mode)
// ---------------------------------------------------------------------------

/// Transport mode (host-to-host).
pub const XFRM_MODE_TRANSPORT: u8 = 0;
/// Tunnel mode.
pub const XFRM_MODE_TUNNEL: u8 = 1;
/// Route-optimisation (mobile IPv6).
pub const XFRM_MODE_ROUTEOPTIMIZATION: u8 = 2;
/// IN-trigger.
pub const XFRM_MODE_IN_TRIGGER: u8 = 3;
/// Beet — Bound-End-to-End-Tunnel.
pub const XFRM_MODE_BEET: u8 = 4;

// ---------------------------------------------------------------------------
// Multicast group bits (NETLINK_XFRM groups)
// ---------------------------------------------------------------------------

/// Acquire-event group.
pub const XFRMNLGRP_ACQUIRE: u32 = 1 << 0;
/// Expire-event group.
pub const XFRMNLGRP_EXPIRE: u32 = 1 << 1;
/// SA-change group.
pub const XFRMNLGRP_SA: u32 = 1 << 2;
/// Policy-change group.
pub const XFRMNLGRP_POLICY: u32 = 1 << 3;
/// Aevent group.
pub const XFRMNLGRP_AEVENTS: u32 = 1 << 4;
/// Report group.
pub const XFRMNLGRP_REPORT: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_codes_distinct_and_in_range() {
        let msgs = [
            XFRM_MSG_NEWSA,
            XFRM_MSG_DELSA,
            XFRM_MSG_GETSA,
            XFRM_MSG_NEWPOLICY,
            XFRM_MSG_DELPOLICY,
            XFRM_MSG_GETPOLICY,
            XFRM_MSG_ALLOCSPI,
            XFRM_MSG_ACQUIRE,
            XFRM_MSG_EXPIRE,
            XFRM_MSG_UPDPOLICY,
            XFRM_MSG_UPDSA,
            XFRM_MSG_POLEXPIRE,
            XFRM_MSG_FLUSHSA,
            XFRM_MSG_FLUSHPOLICY,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
            // All xfrm messages use the 0x10..0x1f range to avoid
            // collision with the generic netlink message numbers.
            assert!(msgs[i] >= 0x10);
            assert!(msgs[i] < 0x20);
        }
    }

    #[test]
    fn test_policy_directions_distinct() {
        let dirs = [XFRM_POLICY_IN, XFRM_POLICY_OUT, XFRM_POLICY_FWD];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
            assert!(dirs[i] < XFRM_POLICY_MAX);
        }
    }

    #[test]
    fn test_policy_actions_distinct() {
        assert_ne!(XFRM_POLICY_ALLOW, XFRM_POLICY_BLOCK);
    }

    #[test]
    fn test_sa_modes_distinct() {
        let modes = [
            XFRM_MODE_TRANSPORT,
            XFRM_MODE_TUNNEL,
            XFRM_MODE_ROUTEOPTIMIZATION,
            XFRM_MODE_IN_TRIGGER,
            XFRM_MODE_BEET,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_nlgrp_bits_distinct_powers_of_two() {
        let grps = [
            XFRMNLGRP_ACQUIRE,
            XFRMNLGRP_EXPIRE,
            XFRMNLGRP_SA,
            XFRMNLGRP_POLICY,
            XFRMNLGRP_AEVENTS,
            XFRMNLGRP_REPORT,
        ];
        for &g in &grps {
            assert!(g.is_power_of_two());
        }
        for i in 0..grps.len() {
            for j in (i + 1)..grps.len() {
                assert_ne!(grps[i], grps[j]);
            }
        }
    }
}
