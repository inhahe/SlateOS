//! `<linux/fib_rules.h>` — FIB (Forwarding Information Base) rule constants.
//!
//! FIB rules control policy routing — selecting which routing table
//! to use based on source address, destination, TOS, incoming interface,
//! etc. Managed by `ip rule add/del` (iproute2).

// ---------------------------------------------------------------------------
// Rule actions
// ---------------------------------------------------------------------------

/// Unspecified action.
pub const FR_ACT_UNSPEC: u8 = 0;
/// Use specific table.
pub const FR_ACT_TO_TBL: u8 = 1;
/// Go to another rule.
pub const FR_ACT_GOTO: u8 = 2;
/// Drop / NOP.
pub const FR_ACT_NOP: u8 = 3;
/// Blackhole (silently drop).
pub const FR_ACT_BLACKHOLE: u8 = 6;
/// Unreachable (ICMP dest unreachable).
pub const FR_ACT_UNREACHABLE: u8 = 7;
/// Prohibit (ICMP admin prohibited).
pub const FR_ACT_PROHIBIT: u8 = 8;

// ---------------------------------------------------------------------------
// FIB rule attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const FRA_UNSPEC: u16 = 0;
/// Destination.
pub const FRA_DST: u16 = 1;
/// Source.
pub const FRA_SRC: u16 = 2;
/// Input interface name.
pub const FRA_IIFNAME: u16 = 3;
/// Go to rule number.
pub const FRA_GOTO: u16 = 4;
/// Priority.
pub const FRA_PRIORITY: u16 = 6;
/// Firewall mark.
pub const FRA_FWMARK: u16 = 10;
/// Firewall mark mask.
pub const FRA_FWMASK: u16 = 11;
/// Routing table.
pub const FRA_TABLE: u16 = 15;
/// Suppress prefix length.
pub const FRA_SUPPRESS_PREFIXLEN: u16 = 13;
/// Suppress interface group.
pub const FRA_SUPPRESS_IFGROUP: u16 = 14;
/// Output interface name.
pub const FRA_OIFNAME: u16 = 17;
/// L3 master device.
pub const FRA_L3MDEV: u16 = 19;
/// UID range.
pub const FRA_UID_RANGE: u16 = 20;
/// Protocol.
pub const FRA_PROTOCOL: u16 = 21;
/// IP protocol.
pub const FRA_IP_PROTO: u16 = 22;
/// Source port range.
pub const FRA_SPORT_RANGE: u16 = 23;
/// Destination port range.
pub const FRA_DPORT_RANGE: u16 = 24;

// ---------------------------------------------------------------------------
// Default rule priorities
// ---------------------------------------------------------------------------

/// Local table priority.
pub const FIB_RULE_PREF_LOCAL: u32 = 0;
/// Main table priority.
pub const FIB_RULE_PREF_MAIN: u32 = 32766;
/// Default table priority.
pub const FIB_RULE_PREF_DEFAULT: u32 = 32767;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let actions = [
            FR_ACT_UNSPEC,
            FR_ACT_TO_TBL,
            FR_ACT_GOTO,
            FR_ACT_NOP,
            FR_ACT_BLACKHOLE,
            FR_ACT_UNREACHABLE,
            FR_ACT_PROHIBIT,
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
            FRA_L3MDEV,
            FRA_UID_RANGE,
            FRA_PROTOCOL,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_default_priorities() {
        assert_eq!(FIB_RULE_PREF_LOCAL, 0);
        assert!(FIB_RULE_PREF_MAIN < FIB_RULE_PREF_DEFAULT);
        assert_eq!(FIB_RULE_PREF_DEFAULT, 32767);
    }
}
