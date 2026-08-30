//! `<linux/fib_rules.h>` — FIB (Forwarding Information Base) rules constants.
//!
//! FIB rules implement policy-based routing in Linux. Each rule has a
//! priority, match criteria (source/destination prefix, interface,
//! fwmark, UID range, IP protocol, port range), and an action
//! (lookup a specific routing table, unreachable, blackhole, etc.).
//! Rules are evaluated in priority order; the first match determines
//! which routing table handles the packet. Used for multi-homing,
//! VRF routing, and traffic engineering.

// ---------------------------------------------------------------------------
// FIB rule actions (FR_ACT_*)
// ---------------------------------------------------------------------------

/// Unspecified action.
pub const FR_ACT_UNSPEC: u32 = 0;
/// Lookup routing table.
pub const FR_ACT_TO_TBL: u32 = 1;
/// Forward to gateway.
pub const FR_ACT_GOTO: u32 = 2;
/// No operation (skip rule).
pub const FR_ACT_NOP: u32 = 3;
/// Return ENETUNREACH.
pub const FR_ACT_UNREACHABLE: u32 = 6;
/// Blackhole (silently drop).
pub const FR_ACT_BLACKHOLE: u32 = 7;
/// Return EACCES (prohibit).
pub const FR_ACT_PROHIBIT: u32 = 8;

// ---------------------------------------------------------------------------
// FIB rule attributes (FRA_*)
// ---------------------------------------------------------------------------

/// Destination address/prefix.
pub const FRA_DST: u32 = 1;
/// Source address/prefix.
pub const FRA_SRC: u32 = 2;
/// Input interface name.
pub const FRA_IIFNAME: u32 = 3;
/// Goto rule target (priority).
pub const FRA_GOTO: u32 = 4;
/// Priority (lower = checked first).
pub const FRA_PRIORITY: u32 = 6;
/// Fwmark value to match.
pub const FRA_FWMARK: u32 = 10;
/// Fwmark mask.
pub const FRA_FWMASK: u32 = 11;
/// Routing table ID.
pub const FRA_TABLE: u32 = 15;
/// Suppress prefix length (don't match routes shorter than this).
pub const FRA_SUPPRESS_PREFIXLEN: u32 = 13;
/// Suppress interface group.
pub const FRA_SUPPRESS_IFGROUP: u32 = 14;
/// Output interface name.
pub const FRA_OIFNAME: u32 = 17;
/// UID range start.
pub const FRA_UID_RANGE: u32 = 20;
/// L3 master device (VRF).
pub const FRA_L3MDEV: u32 = 19;
/// IP protocol to match.
pub const FRA_IP_PROTO: u32 = 21;
/// Source port range.
pub const FRA_SPORT_RANGE: u32 = 22;
/// Destination port range.
pub const FRA_DPORT_RANGE: u32 = 23;
/// Tunnel ID.
pub const FRA_TUN_ID: u32 = 24;

// ---------------------------------------------------------------------------
// FIB rule flags
// ---------------------------------------------------------------------------

/// Rule is permanent (not removed by flush).
pub const FIB_RULE_PERMANENT: u32 = 0x0001;
/// Invert the match result.
pub const FIB_RULE_INVERT: u32 = 0x0002;
/// Rule was auto-generated (not user-defined).
pub const FIB_RULE_UNRESOLVED: u32 = 0x0004;
/// Input interface detached.
pub const FIB_RULE_IIF_DETACHED: u32 = 0x0008;
/// Output interface detached.
pub const FIB_RULE_OIF_DETACHED: u32 = 0x0010;
/// Find matching source address (for multihomed).
pub const FIB_RULE_FIND_SADDR: u32 = 0x0020;

// ---------------------------------------------------------------------------
// Well-known routing table IDs
// ---------------------------------------------------------------------------

/// Unspecified table.
pub const RT_TABLE_UNSPEC: u32 = 0;
/// Default table.
pub const RT_TABLE_DEFAULT: u32 = 253;
/// Main routing table.
pub const RT_TABLE_MAIN: u32 = 254;
/// Local (loopback/broadcast) table.
pub const RT_TABLE_LOCAL: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let acts = [
            FR_ACT_UNSPEC,
            FR_ACT_TO_TBL,
            FR_ACT_GOTO,
            FR_ACT_NOP,
            FR_ACT_UNREACHABLE,
            FR_ACT_BLACKHOLE,
            FR_ACT_PROHIBIT,
        ];
        for i in 0..acts.len() {
            for j in (i + 1)..acts.len() {
                assert_ne!(acts[i], acts[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            FRA_DST,
            FRA_SRC,
            FRA_IIFNAME,
            FRA_GOTO,
            FRA_PRIORITY,
            FRA_FWMARK,
            FRA_FWMASK,
            FRA_SUPPRESS_PREFIXLEN,
            FRA_SUPPRESS_IFGROUP,
            FRA_TABLE,
            FRA_OIFNAME,
            FRA_L3MDEV,
            FRA_UID_RANGE,
            FRA_IP_PROTO,
            FRA_SPORT_RANGE,
            FRA_DPORT_RANGE,
            FRA_TUN_ID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_flags_distinct() {
        let flags = [
            FIB_RULE_PERMANENT,
            FIB_RULE_INVERT,
            FIB_RULE_UNRESOLVED,
            FIB_RULE_IIF_DETACHED,
            FIB_RULE_OIF_DETACHED,
            FIB_RULE_FIND_SADDR,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_tables_distinct() {
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
    fn test_table_ordering() {
        assert!(RT_TABLE_UNSPEC < RT_TABLE_DEFAULT);
        assert!(RT_TABLE_DEFAULT < RT_TABLE_MAIN);
        assert!(RT_TABLE_MAIN < RT_TABLE_LOCAL);
    }
}
