//! `<linux/sched.h>` — PID namespace constants.
//!
//! PID namespaces isolate the process ID number space so that
//! processes in different namespaces can have the same PID. The
//! first process in a new PID namespace is PID 1 (init) within
//! that namespace and has special orphan reaping duties.

// ---------------------------------------------------------------------------
// PID namespace limits and special values
// ---------------------------------------------------------------------------

/// PID of init in a PID namespace.
pub const PIDNS_INIT_PID: u32 = 1;
/// Maximum PID value (default, can be raised).
pub const PID_MAX_DEFAULT: u32 = 32768;
/// Maximum PID value (upper limit on 64-bit).
pub const PID_MAX_LIMIT: u32 = 4_194_304;
/// Maximum nesting depth for PID namespaces.
pub const PIDNS_MAX_NESTING: u32 = 32;

// ---------------------------------------------------------------------------
// pidfd_open() flags
// ---------------------------------------------------------------------------

/// pidfd non-blocking flag.
pub const PIDFD_NONBLOCK: u32 = 0x0000_0800;
/// pidfd_open() thread flag (get fd for specific thread).
pub const PIDFD_THREAD: u32 = 0x1000_0000;

// ---------------------------------------------------------------------------
// waitid() P_PIDFD flag
// ---------------------------------------------------------------------------

/// Wait for a pidfd (not a PID number).
pub const P_PIDFD: u32 = 3;

// ---------------------------------------------------------------------------
// pidfd_send_signal() flags
// ---------------------------------------------------------------------------

/// Send signal to specific thread (not thread group).
pub const PIDFD_SIGNAL_THREAD: u32 = 1 << 0;
/// Send signal to the thread group leader.
pub const PIDFD_SIGNAL_THREAD_GROUP: u32 = 1 << 1;
/// Send signal to the process group.
pub const PIDFD_SIGNAL_PROCESS_GROUP: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Process ID types (for waitid/kill)
// ---------------------------------------------------------------------------

/// Specify by process ID.
pub const P_PID: u32 = 1;
/// Specify by process group ID.
pub const P_PGID: u32 = 2;
/// Specify all children.
pub const P_ALL: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pid_limits() {
        assert_eq!(PIDNS_INIT_PID, 1);
        assert!(PID_MAX_DEFAULT < PID_MAX_LIMIT);
    }

    #[test]
    fn test_pidfd_flags_distinct() {
        assert_ne!(PIDFD_NONBLOCK, PIDFD_THREAD);
    }

    #[test]
    fn test_signal_flags_no_overlap() {
        let flags = [
            PIDFD_SIGNAL_THREAD, PIDFD_SIGNAL_THREAD_GROUP,
            PIDFD_SIGNAL_PROCESS_GROUP,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_p_types_distinct() {
        let types = [P_PID, P_PGID, P_ALL, P_PIDFD];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_max_nesting() {
        assert!(PIDNS_MAX_NESTING > 0);
        assert!(PIDNS_MAX_NESTING <= 32);
    }
}
