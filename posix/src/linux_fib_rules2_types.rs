//! `<linux/fib_rules.h>` — Additional FIB rules constants.
//!
//! Supplementary FIB rules constants covering rule actions,
//! attribute types, and match flags.

// ---------------------------------------------------------------------------
// FIB rule actions
// ---------------------------------------------------------------------------

/// Unspec.
pub const FR_ACT_UNSPEC: u32 = 0;
/// Use table for lookup.
pub const FR_ACT_TO_TBL: u32 = 1;
/// Return as if no matching rule.
pub const FR_ACT_GOTO: u32 = 2;
/// Drop packet.
pub const FR_ACT_NOP: u32 = 3;
/// Return ICMP unreachable.
pub const FR_ACT_RES3: u32 = 4;
/// Return ICMP prohibited.
pub const FR_ACT_RES4: u32 = 5;
/// Blackhole (silently drop).
pub const FR_ACT_BLACKHOLE: u32 = 6;
/// Unreachable.
pub const FR_ACT_UNREACHABLE: u32 = 7;
/// Prohibit.
pub const FR_ACT_PROHIBIT: u32 = 8;

// ---------------------------------------------------------------------------
// FIB rule attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const FRA_UNSPEC: u32 = 0;
/// Destination address.
pub const FRA_DST: u32 = 1;
/// Source address.
pub const FRA_SRC: u32 = 2;
/// Input interface.
pub const FRA_IIFNAME: u32 = 3;
/// Goto rule.
pub const FRA_GOTO: u32 = 4;
/// Priority.
pub const FRA_PRIORITY: u32 = 6;
/// Fwmark.
pub const FRA_FWMARK: u32 = 10;
/// Fwmask.
pub const FRA_FWMASK: u32 = 11;
/// Table.
pub const FRA_TABLE: u32 = 15;
/// L3 multicast device.
pub const FRA_L3MDEV: u32 = 19;
/// UID range.
pub const FRA_UID_RANGE: u32 = 20;
/// Protocol.
pub const FRA_PROTOCOL: u32 = 21;
/// IP proto.
pub const FRA_IP_PROTO: u32 = 22;
/// Sport range.
pub const FRA_SPORT_RANGE: u32 = 23;
/// Dport range.
pub const FRA_DPORT_RANGE: u32 = 24;

// ---------------------------------------------------------------------------
// FIB rule flags
// ---------------------------------------------------------------------------

/// Invert match.
pub const FIB_RULE_INVERT: u32 = 0x00000002;
/// Unresolved.
pub const FIB_RULE_UNRESOLVED: u32 = 0x00000004;
/// IIF detached.
pub const FIB_RULE_IIF_DETACHED: u32 = 0x00000008;
/// DEV detached.
pub const FIB_RULE_DEV_DETACHED: u32 = FIB_RULE_IIF_DETACHED;
/// OIF detached.
pub const FIB_RULE_OIF_DETACHED: u32 = 0x00000010;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let actions = [
            FR_ACT_UNSPEC, FR_ACT_TO_TBL, FR_ACT_GOTO,
            FR_ACT_NOP, FR_ACT_RES3, FR_ACT_RES4,
            FR_ACT_BLACKHOLE, FR_ACT_UNREACHABLE, FR_ACT_PROHIBIT,
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
            FRA_UNSPEC, FRA_DST, FRA_SRC, FRA_IIFNAME,
            FRA_GOTO, FRA_PRIORITY, FRA_FWMARK, FRA_FWMASK,
            FRA_TABLE, FRA_L3MDEV, FRA_UID_RANGE, FRA_PROTOCOL,
            FRA_IP_PROTO, FRA_SPORT_RANGE, FRA_DPORT_RANGE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            FIB_RULE_INVERT, FIB_RULE_UNRESOLVED,
            FIB_RULE_IIF_DETACHED, FIB_RULE_OIF_DETACHED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
