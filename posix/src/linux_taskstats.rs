//! `<linux/taskstats.h>` — per-task statistics interface.
//!
//! Taskstats provides detailed per-task and per-tgid statistics
//! including CPU time, I/O accounting, memory usage, and delay
//! accounting. Delivered via Generic Netlink (TASKSTATS family).

// ---------------------------------------------------------------------------
// Taskstats commands
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TASKSTATS_CMD_UNSPEC: u8 = 0;
/// Get stats.
pub const TASKSTATS_CMD_GET: u8 = 1;
/// New stats notification.
pub const TASKSTATS_CMD_NEW: u8 = 2;

// ---------------------------------------------------------------------------
// Taskstats command attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TASKSTATS_CMD_ATTR_UNSPEC: u16 = 0;
/// PID to query.
pub const TASKSTATS_CMD_ATTR_PID: u16 = 1;
/// TGID to query.
pub const TASKSTATS_CMD_ATTR_TGID: u16 = 2;
/// Register CPU mask.
pub const TASKSTATS_CMD_ATTR_REGISTER_CPUMASK: u16 = 3;
/// Deregister CPU mask.
pub const TASKSTATS_CMD_ATTR_DEREGISTER_CPUMASK: u16 = 4;

// ---------------------------------------------------------------------------
// Taskstats type attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TASKSTATS_TYPE_UNSPEC: u16 = 0;
/// PID.
pub const TASKSTATS_TYPE_PID: u16 = 1;
/// TGID.
pub const TASKSTATS_TYPE_TGID: u16 = 2;
/// Stats data.
pub const TASKSTATS_TYPE_STATS: u16 = 3;
/// Aggregated PID stats.
pub const TASKSTATS_TYPE_AGGR_PID: u16 = 4;
/// Aggregated TGID stats.
pub const TASKSTATS_TYPE_AGGR_TGID: u16 = 5;
/// Null.
pub const TASKSTATS_TYPE_NULL: u16 = 6;

// ---------------------------------------------------------------------------
// Taskstats version
// ---------------------------------------------------------------------------

/// Current taskstats version.
pub const TASKSTATS_VERSION: u16 = 13;

/// Generic Netlink family name.
pub const TASKSTATS_GENL_NAME: &str = "TASKSTATS";

// ---------------------------------------------------------------------------
// Delay accounting flags
// ---------------------------------------------------------------------------

/// CPU delay accounting.
pub const TASKSTATS_FL_CPU: u8 = 1;
/// Block I/O delay.
pub const TASKSTATS_FL_IO: u8 = 2;
/// Swap-in delay.
pub const TASKSTATS_FL_SWAPIN: u8 = 4;
/// Memory reclaim delay.
pub const TASKSTATS_FL_RECLAIM: u8 = 8;
/// Thrashing delay.
pub const TASKSTATS_FL_THRASHING: u8 = 16;
/// Compact delay.
pub const TASKSTATS_FL_COMPACT: u8 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands() {
        assert_eq!(TASKSTATS_CMD_UNSPEC, 0);
        assert_eq!(TASKSTATS_CMD_GET, 1);
        assert_eq!(TASKSTATS_CMD_NEW, 2);
    }

    #[test]
    fn test_cmd_attrs_distinct() {
        let attrs = [
            TASKSTATS_CMD_ATTR_UNSPEC,
            TASKSTATS_CMD_ATTR_PID,
            TASKSTATS_CMD_ATTR_TGID,
            TASKSTATS_CMD_ATTR_REGISTER_CPUMASK,
            TASKSTATS_CMD_ATTR_DEREGISTER_CPUMASK,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_type_attrs_distinct() {
        let types = [
            TASKSTATS_TYPE_UNSPEC,
            TASKSTATS_TYPE_PID,
            TASKSTATS_TYPE_TGID,
            TASKSTATS_TYPE_STATS,
            TASKSTATS_TYPE_AGGR_PID,
            TASKSTATS_TYPE_AGGR_TGID,
            TASKSTATS_TYPE_NULL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_delay_flags_powers_of_two() {
        let flags = [
            TASKSTATS_FL_CPU,
            TASKSTATS_FL_IO,
            TASKSTATS_FL_SWAPIN,
            TASKSTATS_FL_RECLAIM,
            TASKSTATS_FL_THRASHING,
            TASKSTATS_FL_COMPACT,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f} not power of 2");
        }
    }

    #[test]
    fn test_version() {
        assert!(TASKSTATS_VERSION >= 8);
    }

    #[test]
    fn test_genl_name() {
        assert_eq!(TASKSTATS_GENL_NAME, "TASKSTATS");
    }
}
