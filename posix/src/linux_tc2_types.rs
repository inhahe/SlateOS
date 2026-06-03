//! `<linux/pkt_cls.h>` — Additional traffic control constants.
//!
//! Supplementary traffic control constants covering TC action types,
//! TC flower flags, and TC matchall flags.

// ---------------------------------------------------------------------------
// TC action types (TCA_ACT_*)
// ---------------------------------------------------------------------------

/// Unspec action.
pub const TCA_ACT_UNSPEC: i32 = 0;
/// OK (pass).
pub const TC_ACT_OK: i32 = 0;
/// Reclassify.
pub const TC_ACT_RECLASSIFY: i32 = 1;
/// Shot (drop).
pub const TC_ACT_SHOT: i32 = 2;
/// Pipe (continue to next action).
pub const TC_ACT_PIPE: i32 = 3;
/// Stolen.
pub const TC_ACT_STOLEN: i32 = 4;
/// Queued.
pub const TC_ACT_QUEUED: i32 = 5;
/// Repeat.
pub const TC_ACT_REPEAT: i32 = 6;
/// Redirect.
pub const TC_ACT_REDIRECT: i32 = 7;
/// Trap.
pub const TC_ACT_TRAP: i32 = 8;

// ---------------------------------------------------------------------------
// TC flower key flags
// ---------------------------------------------------------------------------

/// Match on Ethernet type.
pub const TCA_FLOWER_KEY_FLAGS_IS_FRAGMENT: u32 = 1 << 0;
/// First fragment.
pub const TCA_FLOWER_KEY_FLAGS_FRAG_IS_FIRST: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// TC filter protocol IDs
// ---------------------------------------------------------------------------

/// All protocols.
pub const ETH_P_ALL_TC: u16 = 0x0003;
/// IPv4 protocol.
pub const ETH_P_IP_TC: u16 = 0x0800;
/// IPv6 protocol.
pub const ETH_P_IPV6_TC: u16 = 0x86DD;
/// ARP protocol.
pub const ETH_P_ARP_TC: u16 = 0x0806;
/// 802.1Q VLAN.
pub const ETH_P_8021Q_TC: u16 = 0x8100;

// ---------------------------------------------------------------------------
// TC handle encoding
// ---------------------------------------------------------------------------

/// Major number shift.
pub const TC_H_MAJ_SHIFT: u32 = 16;
/// Major number mask.
pub const TC_H_MAJ_MASK: u32 = 0xFFFF0000;
/// Minor number mask.
pub const TC_H_MIN_MASK: u32 = 0x0000FFFF;
/// Root qdisc handle.
pub const TC_H_ROOT: u32 = 0xFFFFFFFF;
/// Ingress qdisc handle.
pub const TC_H_INGRESS: u32 = 0xFFFFFFF1;
/// Clsact qdisc handle.
pub const TC_H_CLSACT: u32 = 0xFFFFFFF2;
/// Unspec handle.
pub const TC_H_UNSPEC: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_types_distinct() {
        // TC_ACT_OK == TCA_ACT_UNSPEC == 0, which is by design.
        // All non-zero actions must be distinct.
        let actions = [
            TC_ACT_RECLASSIFY,
            TC_ACT_SHOT,
            TC_ACT_PIPE,
            TC_ACT_STOLEN,
            TC_ACT_QUEUED,
            TC_ACT_REPEAT,
            TC_ACT_REDIRECT,
            TC_ACT_TRAP,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_act_ok_is_zero() {
        assert_eq!(TC_ACT_OK, 0);
        assert_eq!(TCA_ACT_UNSPEC, 0);
    }

    #[test]
    fn test_flower_key_flags_power_of_two() {
        assert!(TCA_FLOWER_KEY_FLAGS_IS_FRAGMENT.is_power_of_two());
        assert!(TCA_FLOWER_KEY_FLAGS_FRAG_IS_FIRST.is_power_of_two());
    }

    #[test]
    fn test_flower_key_flags_no_overlap() {
        assert_eq!(
            TCA_FLOWER_KEY_FLAGS_IS_FRAGMENT & TCA_FLOWER_KEY_FLAGS_FRAG_IS_FIRST,
            0
        );
    }

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            ETH_P_ALL_TC,
            ETH_P_IP_TC,
            ETH_P_IPV6_TC,
            ETH_P_ARP_TC,
            ETH_P_8021Q_TC,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_handle_masks() {
        assert_eq!(TC_H_MAJ_MASK | TC_H_MIN_MASK, 0xFFFFFFFF);
        assert_eq!(TC_H_MAJ_MASK & TC_H_MIN_MASK, 0);
    }

    #[test]
    fn test_special_handles_distinct() {
        let handles = [TC_H_ROOT, TC_H_INGRESS, TC_H_CLSACT, TC_H_UNSPEC];
        for i in 0..handles.len() {
            for j in (i + 1)..handles.len() {
                assert_ne!(handles[i], handles[j]);
            }
        }
    }
}
