//! `<linux/fib_rules.h>` — FIB (Forwarding Information Base) rule constants.
//!
//! FIB rules allow policy-based routing by matching packets against
//! criteria (source address, mark, interface) and directing them to
//! specific routing tables. These constants define rule actions,
//! attributes, and matching flags.

// ---------------------------------------------------------------------------
// FIB rule actions (FRA_*)
// ---------------------------------------------------------------------------

/// Use the specified routing table.
pub const FR_ACT_TO_TBL: u8 = 1;
/// Jump to another rule.
pub const FR_ACT_GOTO: u8 = 2;
/// No operation (skip).
pub const FR_ACT_NOP: u8 = 3;
/// Return ENETUNREACH.
pub const FR_ACT_UNREACHABLE: u8 = 6;
/// Silently drop (blackhole).
pub const FR_ACT_BLACKHOLE: u8 = 7;
/// Return EACCES (prohibit).
pub const FR_ACT_PROHIBIT: u8 = 8;

// ---------------------------------------------------------------------------
// FIB rule attributes (netlink, FRA_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const FRA_UNSPEC: u16 = 0;
/// Destination prefix.
pub const FRA_DST: u16 = 1;
/// Source prefix.
pub const FRA_SRC: u16 = 2;
/// Interface name (iif).
pub const FRA_IIFNAME: u16 = 3;
/// Jump target rule.
pub const FRA_GOTO: u16 = 4;
/// Rule priority.
pub const FRA_PRIORITY: u16 = 6;
/// fwmark value.
pub const FRA_FWMARK: u16 = 10;
/// fwmark mask.
pub const FRA_FWMASK: u16 = 11;
/// Routing table ID.
pub const FRA_TABLE: u16 = 15;
/// Output interface name.
pub const FRA_OIFNAME: u16 = 17;
/// UID range start.
pub const FRA_UID_RANGE: u16 = 20;
/// IP protocol to match.
pub const FRA_PROTOCOL: u16 = 21;
/// Source port range.
pub const FRA_SPORT_RANGE: u16 = 22;
/// Destination port range.
pub const FRA_DPORT_RANGE: u16 = 23;

// ---------------------------------------------------------------------------
// Well-known routing table IDs
// ---------------------------------------------------------------------------

/// Unspecified table.
pub const RT_TABLE_UNSPEC: u8 = 0;
/// Default table.
pub const RT_TABLE_DEFAULT: u8 = 253;
/// Main routing table.
pub const RT_TABLE_MAIN: u8 = 254;
/// Local routes (kernel-managed).
pub const RT_TABLE_LOCAL: u8 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let actions = [
            FR_ACT_TO_TBL,
            FR_ACT_GOTO,
            FR_ACT_NOP,
            FR_ACT_UNREACHABLE,
            FR_ACT_BLACKHOLE,
            FR_ACT_PROHIBIT,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_attributes_distinct() {
        let attrs = [
            FRA_UNSPEC,
            FRA_DST,
            FRA_SRC,
            FRA_IIFNAME,
            FRA_GOTO,
            FRA_PRIORITY,
            FRA_FWMARK,
            FRA_FWMASK,
            FRA_TABLE,
            FRA_OIFNAME,
            FRA_UID_RANGE,
            FRA_PROTOCOL,
            FRA_SPORT_RANGE,
            FRA_DPORT_RANGE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_table_ids_distinct() {
        let tables = [
            RT_TABLE_UNSPEC,
            RT_TABLE_DEFAULT,
            RT_TABLE_MAIN,
            RT_TABLE_LOCAL,
        ];
        for i in 0..tables.len() {
            for j in (i + 1)..tables.len() {
                assert_ne!(tables[i], tables[j]);
            }
        }
    }

    #[test]
    fn test_main_table() {
        assert_eq!(RT_TABLE_MAIN, 254);
    }

    #[test]
    fn test_to_tbl_is_one() {
        assert_eq!(FR_ACT_TO_TBL, 1);
    }
}
