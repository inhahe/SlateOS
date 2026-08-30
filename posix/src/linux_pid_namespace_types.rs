//! `<linux/pid_namespace.h>` — PID namespace constants.
//!
//! PID namespaces isolate process ID number spaces. Processes in
//! different PID namespaces can have the same PID. The first process
//! in a new PID namespace becomes PID 1 (init) for that namespace
//! and adopts orphaned children. PID namespaces are hierarchical:
//! a process is visible in its own namespace and all ancestor
//! namespaces (with different PIDs at each level). Used by containers
//! to provide isolated PID trees.

// ---------------------------------------------------------------------------
// PID namespace limits
// ---------------------------------------------------------------------------

/// Maximum nesting depth of PID namespaces (32 levels).
pub const MAX_PID_NS_LEVEL: u32 = 32;
/// Maximum PID value (2^22 - 1, about 4 million).
pub const PID_MAX_DEFAULT: u32 = 32768;
/// Maximum PID limit (can be raised via /proc/sys/kernel/pid_max).
pub const PID_MAX_LIMIT: u32 = 4194304;

// ---------------------------------------------------------------------------
// PID allocation flags
// ---------------------------------------------------------------------------

/// Allocate PIDs sequentially (not randomly).
pub const PID_ALLOC_SEQUENTIAL: u32 = 0;
/// Allocate PIDs randomly within range (for security).
pub const PID_ALLOC_RANDOM: u32 = 1;

// ---------------------------------------------------------------------------
// PID namespace states
// ---------------------------------------------------------------------------

/// Namespace is active (processes running).
pub const PIDNS_STATE_ACTIVE: u32 = 0;
/// Namespace init (PID 1) has exited; namespace is dying.
pub const PIDNS_STATE_DYING: u32 = 1;
/// Namespace has been fully cleaned up.
pub const PIDNS_STATE_DEAD: u32 = 2;

// ---------------------------------------------------------------------------
// Special PIDs
// ---------------------------------------------------------------------------

/// PID of the init process in any PID namespace.
pub const PIDNS_INIT_PID: u32 = 1;
/// Idle task PID (scheduler, not a real process).
pub const PIDNS_IDLE_PID: u32 = 0;

// ---------------------------------------------------------------------------
// PID types (what kind of ID)
// ---------------------------------------------------------------------------

/// Process ID (unique per-process).
pub const PIDTYPE_PID: u32 = 0;
/// Thread group ID (same as leader's PID).
pub const PIDTYPE_TGID: u32 = 1;
/// Process group ID (job control).
pub const PIDTYPE_PGID: u32 = 2;
/// Session ID (login session).
pub const PIDTYPE_SID: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pid_limits() {
        assert!(PID_MAX_DEFAULT > 0);
        assert!(PID_MAX_LIMIT > PID_MAX_DEFAULT);
        assert!(MAX_PID_NS_LEVEL > 0);
    }

    #[test]
    fn test_alloc_modes_distinct() {
        assert_ne!(PID_ALLOC_SEQUENTIAL, PID_ALLOC_RANDOM);
    }

    #[test]
    fn test_states_distinct() {
        let states = [PIDNS_STATE_ACTIVE, PIDNS_STATE_DYING, PIDNS_STATE_DEAD];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_special_pids() {
        assert_eq!(PIDNS_INIT_PID, 1);
        assert_eq!(PIDNS_IDLE_PID, 0);
        assert_ne!(PIDNS_INIT_PID, PIDNS_IDLE_PID);
    }

    #[test]
    fn test_pid_types_distinct() {
        let types = [PIDTYPE_PID, PIDTYPE_TGID, PIDTYPE_PGID, PIDTYPE_SID];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
