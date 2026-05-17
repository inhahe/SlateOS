//! `<linux/pkt_sched.h>` — Traffic Control (TC) constants.
//!
//! Linux Traffic Control manages packet scheduling, shaping,
//! classification, and policing. Qdiscs (queueing disciplines)
//! determine the order packets leave an interface. Classes group
//! traffic for hierarchical scheduling. Filters classify packets
//! into classes. Used for bandwidth management, QoS, and network
//! simulation.

// ---------------------------------------------------------------------------
// Qdisc types (well-known handles)
// ---------------------------------------------------------------------------

/// Root qdisc handle.
pub const TC_H_ROOT: u32 = 0xFFFF_FFFF;
/// Unspecified handle.
pub const TC_H_UNSPEC: u32 = 0;
/// Ingress qdisc handle.
pub const TC_H_INGRESS: u32 = 0xFFFF_FFF1;
/// Clsact qdisc handle (BPF classifier/action).
pub const TC_H_CLSACT: u32 = 0xFFFF_FFF2;

// ---------------------------------------------------------------------------
// TC handle manipulation
// ---------------------------------------------------------------------------

/// Major number mask.
pub const TC_H_MAJ_MASK: u32 = 0xFFFF_0000;
/// Minor number mask.
pub const TC_H_MIN_MASK: u32 = 0x0000_FFFF;

// ---------------------------------------------------------------------------
// Qdisc kinds (numerical identifiers)
// ---------------------------------------------------------------------------

/// FIFO (first-in first-out).
pub const TCQ_F_FIFO: u32 = 0;
/// Priority-based (prio).
pub const TCQ_F_PRIO: u32 = 1;
/// Token bucket filter.
pub const TCQ_F_TBF: u32 = 2;
/// Stochastic Fairness Queueing.
pub const TCQ_F_SFQ: u32 = 3;
/// FQ-CoDel (Fair Queuing Controlled Delay).
pub const TCQ_F_FQ_CODEL: u32 = 4;

// ---------------------------------------------------------------------------
// TC action verdicts
// ---------------------------------------------------------------------------

/// Continue processing.
pub const TC_ACT_UNSPEC: i32 = -1;
/// Accept packet.
pub const TC_ACT_OK: i32 = 0;
/// Reclassify packet.
pub const TC_ACT_RECLASSIFY: i32 = 1;
/// Drop packet.
pub const TC_ACT_SHOT: i32 = 2;
/// Feed to another pipe.
pub const TC_ACT_PIPE: i32 = 3;
/// Steal packet (consume it).
pub const TC_ACT_STOLEN: i32 = 4;
/// Redirect packet.
pub const TC_ACT_REDIRECT: i32 = 7;

// ---------------------------------------------------------------------------
// TC filter protocol IDs
// ---------------------------------------------------------------------------

/// All protocols.
pub const ETH_P_ALL: u16 = 0x0003;
/// IPv4.
pub const ETH_P_IP: u16 = 0x0800;
/// IPv6.
pub const ETH_P_IPV6: u16 = 0x86DD;
/// ARP.
pub const ETH_P_ARP: u16 = 0x0806;
/// 802.1Q VLAN.
pub const ETH_P_8021Q: u16 = 0x8100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handles_distinct() {
        let handles = [TC_H_ROOT, TC_H_UNSPEC, TC_H_INGRESS, TC_H_CLSACT];
        for i in 0..handles.len() {
            for j in (i + 1)..handles.len() {
                assert_ne!(handles[i], handles[j]);
            }
        }
    }

    #[test]
    fn test_handle_masks_complement() {
        assert_eq!(TC_H_MAJ_MASK | TC_H_MIN_MASK, 0xFFFF_FFFF);
        assert_eq!(TC_H_MAJ_MASK & TC_H_MIN_MASK, 0);
    }

    #[test]
    fn test_action_verdicts_distinct() {
        let verdicts = [
            TC_ACT_UNSPEC, TC_ACT_OK, TC_ACT_RECLASSIFY,
            TC_ACT_SHOT, TC_ACT_PIPE, TC_ACT_STOLEN, TC_ACT_REDIRECT,
        ];
        for i in 0..verdicts.len() {
            for j in (i + 1)..verdicts.len() {
                assert_ne!(verdicts[i], verdicts[j]);
            }
        }
    }

    #[test]
    fn test_eth_protocols_distinct() {
        let protos = [ETH_P_ALL, ETH_P_IP, ETH_P_IPV6, ETH_P_ARP, ETH_P_8021Q];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }
}
