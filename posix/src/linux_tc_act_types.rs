//! `<linux/tc_act/tc_*.h>` — Traffic Control (TC) action constants.
//!
//! TC actions are the "do something" part of Linux's traffic control
//! framework. When a TC filter matches a packet, it can trigger one
//! or more actions: mirror/redirect to another interface, modify
//! packet headers (pedit), apply NAT, police bandwidth, gate packets
//! by time schedule, or drop/accept. Actions are composable — a
//! single filter can chain multiple actions. Used by OpenVSwitch,
//! container networking (CNI plugins), and hardware offload.

// ---------------------------------------------------------------------------
// Generic TC action verdicts (from tcf_result)
// ---------------------------------------------------------------------------

/// Accept the packet (continue processing).
pub const TC_ACT_OK: i32 = 0;
/// Reclassify the packet.
pub const TC_ACT_RECLASSIFY: i32 = 1;
/// Drop the packet.
pub const TC_ACT_SHOT: i32 = 2;
/// Forward the packet through the current pipe.
pub const TC_ACT_PIPE: i32 = 3;
/// Steal the packet (consume without counting).
pub const TC_ACT_STOLEN: i32 = 4;
/// Queue the packet.
pub const TC_ACT_QUEUED: i32 = 5;
/// Repeat the action.
pub const TC_ACT_REPEAT: i32 = 6;
/// Redirect the packet.
pub const TC_ACT_REDIRECT: i32 = 7;
/// Trap the packet (send to CPU).
pub const TC_ACT_TRAP: i32 = 8;
/// Unspec action.
pub const TC_ACT_UNSPEC: i32 = -1;

// ---------------------------------------------------------------------------
// Action types (TCA_ACT_* kind strings → numeric IDs)
// ---------------------------------------------------------------------------

/// Mirror/redirect action (act_mirred).
pub const TCA_ACT_MIRRED: u32 = 8;
/// Packet edit action (act_pedit) — modify header fields.
pub const TCA_ACT_PEDIT: u32 = 7;
/// Police action (act_police) — rate limiting.
pub const TCA_ACT_POLICE: u32 = 1;
/// NAT action (act_nat).
pub const TCA_ACT_NAT: u32 = 9;
/// Checksum action (act_csum) — recalculate checksums.
pub const TCA_ACT_CSUM: u32 = 16;
/// VLAN action (act_vlan) — push/pop/modify VLAN tags.
pub const TCA_ACT_VLAN: u32 = 18;
/// Gate action (act_gate) — time-based packet gating (802.1Qbv).
pub const TCA_ACT_GATE: u32 = 30;
/// CT action (act_ct) — connection tracking.
pub const TCA_ACT_CT: u32 = 29;
/// Skbedit action (act_skbedit) — edit skb metadata.
pub const TCA_ACT_SKBEDIT: u32 = 11;
/// Tunnel key action (act_tunnel_key) — set/unset tunnel metadata.
pub const TCA_ACT_TUNNEL_KEY: u32 = 17;
/// Sample action (act_sample) — packet sampling (sFlow/IPFIX).
pub const TCA_ACT_SAMPLE: u32 = 26;

// ---------------------------------------------------------------------------
// Mirred action subtypes (TCA_MIRRED_*)
// ---------------------------------------------------------------------------

/// Redirect egress to another device.
pub const TCA_EGRESS_REDIR: u32 = 1;
/// Mirror egress to another device.
pub const TCA_EGRESS_MIRROR: u32 = 2;
/// Redirect ingress to another device.
pub const TCA_INGRESS_REDIR: u32 = 3;
/// Mirror ingress to another device.
pub const TCA_INGRESS_MIRROR: u32 = 4;

// ---------------------------------------------------------------------------
// VLAN action modes
// ---------------------------------------------------------------------------

/// Pop (remove) outer VLAN tag.
pub const TCA_VLAN_ACT_POP: u32 = 1;
/// Push (add) a VLAN tag.
pub const TCA_VLAN_ACT_PUSH: u32 = 2;
/// Modify existing VLAN tag.
pub const TCA_VLAN_ACT_MODIFY: u32 = 3;
/// Pop Ethernet header (for Q-in-Q).
pub const TCA_VLAN_ACT_POP_ETH: u32 = 4;
/// Push Ethernet header.
pub const TCA_VLAN_ACT_PUSH_ETH: u32 = 5;

// ---------------------------------------------------------------------------
// CT action flags
// ---------------------------------------------------------------------------

/// Commit connection to conntrack table.
pub const TCA_CT_ACT_COMMIT: u32 = 1 << 0;
/// Force commit even if already tracked.
pub const TCA_CT_ACT_FORCE: u32 = 1 << 1;
/// Clear conntrack state.
pub const TCA_CT_ACT_CLEAR: u32 = 1 << 2;
/// Apply NAT.
pub const TCA_CT_ACT_NAT: u32 = 1 << 3;
/// Apply source NAT.
pub const TCA_CT_ACT_NAT_SRC: u32 = 1 << 4;
/// Apply destination NAT.
pub const TCA_CT_ACT_NAT_DST: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verdicts_distinct() {
        let verdicts = [
            TC_ACT_OK,
            TC_ACT_RECLASSIFY,
            TC_ACT_SHOT,
            TC_ACT_PIPE,
            TC_ACT_STOLEN,
            TC_ACT_QUEUED,
            TC_ACT_REPEAT,
            TC_ACT_REDIRECT,
            TC_ACT_TRAP,
            TC_ACT_UNSPEC,
        ];
        for i in 0..verdicts.len() {
            for j in (i + 1)..verdicts.len() {
                assert_ne!(verdicts[i], verdicts[j]);
            }
        }
    }

    #[test]
    fn test_action_types_distinct() {
        let acts = [
            TCA_ACT_POLICE,
            TCA_ACT_PEDIT,
            TCA_ACT_MIRRED,
            TCA_ACT_NAT,
            TCA_ACT_SKBEDIT,
            TCA_ACT_CSUM,
            TCA_ACT_TUNNEL_KEY,
            TCA_ACT_VLAN,
            TCA_ACT_SAMPLE,
            TCA_ACT_CT,
            TCA_ACT_GATE,
        ];
        for i in 0..acts.len() {
            for j in (i + 1)..acts.len() {
                assert_ne!(acts[i], acts[j]);
            }
        }
    }

    #[test]
    fn test_mirred_subtypes_distinct() {
        let mirred = [
            TCA_EGRESS_REDIR,
            TCA_EGRESS_MIRROR,
            TCA_INGRESS_REDIR,
            TCA_INGRESS_MIRROR,
        ];
        for i in 0..mirred.len() {
            for j in (i + 1)..mirred.len() {
                assert_ne!(mirred[i], mirred[j]);
            }
        }
    }

    #[test]
    fn test_vlan_modes_distinct() {
        let modes = [
            TCA_VLAN_ACT_POP,
            TCA_VLAN_ACT_PUSH,
            TCA_VLAN_ACT_MODIFY,
            TCA_VLAN_ACT_POP_ETH,
            TCA_VLAN_ACT_PUSH_ETH,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_ct_flags_no_overlap() {
        let flags = [
            TCA_CT_ACT_COMMIT,
            TCA_CT_ACT_FORCE,
            TCA_CT_ACT_CLEAR,
            TCA_CT_ACT_NAT,
            TCA_CT_ACT_NAT_SRC,
            TCA_CT_ACT_NAT_DST,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_tc_act_ok_is_zero() {
        assert_eq!(TC_ACT_OK, 0);
    }
}
