//! `<linux/taskstats.h>` — Task statistics (taskstats) constants.
//!
//! taskstats provides per-task and per-tgid resource accounting via
//! genetlink. It reports CPU time, memory usage, I/O statistics,
//! delay accounting (scheduler delays, I/O wait, swap-in wait), and
//! context switch counts. Used by accounting tools (atop, collectl),
//! cgroup monitoring, and the delay accounting subsystem for
//! diagnosing performance problems and resource attribution.

// ---------------------------------------------------------------------------
// Taskstats genetlink commands (TASKSTATS_CMD_*)
// ---------------------------------------------------------------------------

/// Get taskstats for a specific PID/TGID.
pub const TASKSTATS_CMD_GET: u32 = 1;
/// New (push) notification on task exit.
pub const TASKSTATS_CMD_NEW: u32 = 2;

// ---------------------------------------------------------------------------
// Taskstats attributes (TASKSTATS_TYPE_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TASKSTATS_TYPE_UNSPEC: u32 = 0;
/// PID to query.
pub const TASKSTATS_TYPE_PID: u32 = 1;
/// TGID to query.
pub const TASKSTATS_TYPE_TGID: u32 = 2;
/// Taskstats structure.
pub const TASKSTATS_TYPE_STATS: u32 = 3;
/// Aggregate stats over PID.
pub const TASKSTATS_TYPE_AGGR_PID: u32 = 4;
/// Aggregate stats over TGID.
pub const TASKSTATS_TYPE_AGGR_TGID: u32 = 5;
/// Null terminator.
pub const TASKSTATS_TYPE_NULL: u32 = 6;

// ---------------------------------------------------------------------------
// Taskstats command attributes (TASKSTATS_CMD_ATTR_*)
// ---------------------------------------------------------------------------

/// PID attribute in request.
pub const TASKSTATS_CMD_ATTR_PID: u32 = 1;
/// TGID attribute in request.
pub const TASKSTATS_CMD_ATTR_TGID: u32 = 2;
/// Register for per-CPU exit notifications.
pub const TASKSTATS_CMD_ATTR_REGISTER_CPUMASK: u32 = 3;
/// Deregister from per-CPU exit notifications.
pub const TASKSTATS_CMD_ATTR_DEREGISTER_CPUMASK: u32 = 4;

// ---------------------------------------------------------------------------
// Taskstats version
// ---------------------------------------------------------------------------

/// Current taskstats structure version.
pub const TASKSTATS_VERSION: u32 = 13;

// ---------------------------------------------------------------------------
// Delay accounting flags
// ---------------------------------------------------------------------------

/// Delay accounting enabled for this task.
pub const DELAYACCT_ON: u32 = 1;

// ---------------------------------------------------------------------------
// Accounting flags (ac_flag in struct acct)
// ---------------------------------------------------------------------------

/// Process used superuser privileges.
pub const TASKSTATS_AC_FORK: u32 = 0x01;
/// Process used exec.
pub const TASKSTATS_AC_EXEC: u32 = 0x02;
/// Process ran as root.
pub const TASKSTATS_AC_ROOT: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        assert_ne!(TASKSTATS_CMD_GET, TASKSTATS_CMD_NEW);
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
    fn test_cmd_attrs_distinct() {
        let attrs = [
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
    fn test_version() {
        assert_eq!(TASKSTATS_VERSION, 13);
    }

    #[test]
    fn test_ac_flags_distinct() {
        let flags = [TASKSTATS_AC_FORK, TASKSTATS_AC_EXEC, TASKSTATS_AC_ROOT];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
