//! `<linux/pkt_cls.h>` — Traffic control action constants.
//!
//! TC actions define what happens to packets matching a
//! classifier rule.  These constants define action types,
//! return codes, and control flags.

// ---------------------------------------------------------------------------
// TC action return codes
// ---------------------------------------------------------------------------

/// Continue to next action.
pub const TC_ACT_UNSPEC: i32 = -1;
/// Accept (allow packet through).
pub const TC_ACT_OK: i32 = 0;
/// Reclassify the packet.
pub const TC_ACT_RECLASSIFY: i32 = 1;
/// Drop (shot) the packet.
pub const TC_ACT_SHOT: i32 = 2;
/// Forward to pipe (next filter).
pub const TC_ACT_PIPE: i32 = 3;
/// Stolen (consumed by action).
pub const TC_ACT_STOLEN: i32 = 4;
/// Queue to a different class.
pub const TC_ACT_QUEUED: i32 = 5;
/// Repeat the action.
pub const TC_ACT_REPEAT: i32 = 6;
/// Redirect to another device.
pub const TC_ACT_REDIRECT: i32 = 7;
/// Trap (send to CPU for slow path).
pub const TC_ACT_TRAP: i32 = 8;

// ---------------------------------------------------------------------------
// TC action types (TCA_ACT_*)
// ---------------------------------------------------------------------------

/// Generic action.
pub const TCA_ACT_TAB: u32 = 1;
/// Action flags.
pub const TCA_ACT_FLAGS: u32 = 2;
/// HW statistics.
pub const TCA_ACT_HW_STATS: u32 = 3;
/// Used HW stats.
pub const TCA_ACT_USED_HW_STATS: u32 = 4;
/// Action in HW count.
pub const TCA_ACT_IN_HW_COUNT: u32 = 5;

// ---------------------------------------------------------------------------
// TC action opcodes
// ---------------------------------------------------------------------------

/// No bind.
pub const TC_ACT_BIND: u32 = 0;
/// No bind with no BPF.
pub const TC_ACT_NOBIND: u32 = 0;
/// Replace existing action.
pub const TCA_ACT_REPLACE: u32 = 1;
/// No override (keep existing).
pub const TCA_ACT_NOREPLACE: u32 = 0;

// ---------------------------------------------------------------------------
// TC HW stats flags
// ---------------------------------------------------------------------------

/// Request immediate HW stats.
pub const TCA_ACT_HW_STATS_IMMEDIATE: u32 = 1 << 0;
/// Request delayed HW stats.
pub const TCA_ACT_HW_STATS_DELAYED: u32 = 1 << 1;
/// Disable HW stats.
pub const TCA_ACT_HW_STATS_DISABLED: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_codes_distinct() {
        let codes = [
            TC_ACT_UNSPEC, TC_ACT_OK, TC_ACT_RECLASSIFY,
            TC_ACT_SHOT, TC_ACT_PIPE, TC_ACT_STOLEN,
            TC_ACT_QUEUED, TC_ACT_REPEAT, TC_ACT_REDIRECT,
            TC_ACT_TRAP,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_unspec_is_negative() {
        assert_eq!(TC_ACT_UNSPEC, -1);
    }

    #[test]
    fn test_ok_is_zero() {
        assert_eq!(TC_ACT_OK, 0);
    }

    #[test]
    fn test_shot_is_two() {
        assert_eq!(TC_ACT_SHOT, 2);
    }

    #[test]
    fn test_act_types_distinct() {
        let types = [TCA_ACT_TAB, TCA_ACT_FLAGS, TCA_ACT_HW_STATS, TCA_ACT_USED_HW_STATS, TCA_ACT_IN_HW_COUNT];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_hw_stats_flags() {
        assert_eq!(TCA_ACT_HW_STATS_IMMEDIATE & TCA_ACT_HW_STATS_DELAYED, 0);
    }
}
