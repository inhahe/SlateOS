//! `<linux/taskstats.h>` — taskstats generic-netlink interface.
//!
//! taskstats is the kernel's per-task accounting channel
//! (`/proc/<pid>/io` aggregation, IO/RSS waits, CPU delay accounting).
//! Userspace clients (`pidstat`, `iotop`, `htop` with delayacct,
//! Kubernetes cAdvisor) subscribe to the generic-netlink family below
//! and consume struct taskstats events.

// ---------------------------------------------------------------------------
// Generic netlink family
// ---------------------------------------------------------------------------

/// Family name registered with genl.
pub const TASKSTATS_GENL_NAME: &str = "TASKSTATS";
/// Family version.
pub const TASKSTATS_GENL_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Commands (generic-netlink command IDs)
// ---------------------------------------------------------------------------

/// Unspecified — used in genl reply parser as a sentinel.
pub const TASKSTATS_CMD_UNSPEC: u32 = 0;
/// Get task stats by PID or TGID.
pub const TASKSTATS_CMD_GET: u32 = 1;
/// New-task notification (sent when a registered listener should
/// receive stats on task exit).
pub const TASKSTATS_CMD_NEW: u32 = 2;

// ---------------------------------------------------------------------------
// Attributes (TASKSTATS_TYPE_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TASKSTATS_TYPE_UNSPEC: u16 = 0;
/// PID attribute.
pub const TASKSTATS_TYPE_PID: u16 = 1;
/// TGID attribute.
pub const TASKSTATS_TYPE_TGID: u16 = 2;
/// Full taskstats payload.
pub const TASKSTATS_TYPE_STATS: u16 = 3;
/// Aggregated stats by PID.
pub const TASKSTATS_TYPE_AGGR_PID: u16 = 4;
/// Aggregated stats by TGID.
pub const TASKSTATS_TYPE_AGGR_TGID: u16 = 5;
/// Null-terminator (used in nested NLA arrays).
pub const TASKSTATS_TYPE_NULL: u16 = 6;

// ---------------------------------------------------------------------------
// Command attributes used in TASKSTATS_CMD_GET requests
// ---------------------------------------------------------------------------

/// PID to query.
pub const TASKSTATS_CMD_ATTR_PID: u16 = 1;
/// TGID to query.
pub const TASKSTATS_CMD_ATTR_TGID: u16 = 2;
/// Register a CPU-mask listener (string "x-y,z-w").
pub const TASKSTATS_CMD_ATTR_REGISTER_CPUMASK: u16 = 3;
/// Deregister a CPU-mask listener.
pub const TASKSTATS_CMD_ATTR_DEREGISTER_CPUMASK: u16 = 4;

// ---------------------------------------------------------------------------
// struct taskstats.version — incremented on layout change
// ---------------------------------------------------------------------------

/// Current ABI version embedded in struct taskstats.
pub const TASKSTATS_VERSION: u32 = 13;

// ---------------------------------------------------------------------------
// Maximum length of the CPU-mask listener string.
// ---------------------------------------------------------------------------

/// Maximum bytes accepted in a `REGISTER_CPUMASK` string (matches
/// `TASKSTATS_CMD_ATTR_MAX` indirectly via `CPUMASK_STR_LEN`).
pub const TASKSTATS_CPUMASK_MAXLEN: u32 = 100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_name_and_version() {
        assert_eq!(TASKSTATS_GENL_NAME, "TASKSTATS");
        assert_eq!(TASKSTATS_GENL_VERSION, 1);
        // The name is uppercase ASCII — verify so a future rename to
        // lowercase would be caught.
        for b in TASKSTATS_GENL_NAME.bytes() {
            assert!(b.is_ascii_uppercase());
        }
    }

    #[test]
    fn test_commands_distinct_and_unspec_zero() {
        let c = [TASKSTATS_CMD_UNSPEC, TASKSTATS_CMD_GET, TASKSTATS_CMD_NEW];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
        assert_eq!(TASKSTATS_CMD_UNSPEC, 0);
    }

    #[test]
    fn test_types_distinct() {
        let t = [
            TASKSTATS_TYPE_UNSPEC,
            TASKSTATS_TYPE_PID,
            TASKSTATS_TYPE_TGID,
            TASKSTATS_TYPE_STATS,
            TASKSTATS_TYPE_AGGR_PID,
            TASKSTATS_TYPE_AGGR_TGID,
            TASKSTATS_TYPE_NULL,
        ];
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                assert_ne!(t[i], t[j]);
            }
        }
    }

    #[test]
    fn test_cmd_attrs_distinct() {
        let a = [
            TASKSTATS_CMD_ATTR_PID,
            TASKSTATS_CMD_ATTR_TGID,
            TASKSTATS_CMD_ATTR_REGISTER_CPUMASK,
            TASKSTATS_CMD_ATTR_DEREGISTER_CPUMASK,
        ];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
    }

    #[test]
    fn test_version_and_cpumask_len_sane() {
        // Layout version must be > 0; 13 is the current mainline value.
        assert!(TASKSTATS_VERSION >= 1);
        // 100 chars is enough for "0-N,M-N" patterns on 4096-CPU systems.
        assert!(TASKSTATS_CPUMASK_MAXLEN >= 64);
    }
}
