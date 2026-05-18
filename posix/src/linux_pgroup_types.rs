//! `<unistd.h>` — Process group constants.
//!
//! Process groups are used for job control and signal delivery.
//! `setpgid()` and `getpgid()` manage group membership.  These
//! constants define related values, limits, and error codes.

// ---------------------------------------------------------------------------
// Process group ID special values
// ---------------------------------------------------------------------------

/// Use the calling process's PID as its PGID (setpgid(0,0)).
pub const PGID_SELF: u32 = 0;

// ---------------------------------------------------------------------------
// waitpid() process group selection
// ---------------------------------------------------------------------------

/// Wait for any child in the same process group as the caller.
pub const WAIT_PGRP_SELF: i32 = 0;
/// Wait for any child (regardless of process group).
pub const WAIT_ANY_CHILD: i32 = -1;

// ---------------------------------------------------------------------------
// Process group limits
// ---------------------------------------------------------------------------

/// Maximum number of process groups per session (practical limit).
pub const PGRP_MAX_DEFAULT: u32 = 65536;

// ---------------------------------------------------------------------------
// Kill process group (kill(-pgid, sig))
// ---------------------------------------------------------------------------

/// Kill all processes with the caller's process group.
pub const KILL_PGRP_SELF: i32 = 0;
/// Kill all processes the caller has permission to signal.
pub const KILL_ALL: i32 = -1;

// ---------------------------------------------------------------------------
// setpgid/getpgid error values
// ---------------------------------------------------------------------------

/// Error return from getpgid().
pub const GETPGID_ERROR: i32 = -1;
/// Error return from setpgid().
pub const SETPGID_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// Process group related signals
// ---------------------------------------------------------------------------

/// Signal number for interrupt (Ctrl+C, sent to foreground pgrp).
pub const PGRP_SIGINT: u32 = 2;
/// Signal number for quit (Ctrl+\, sent to foreground pgrp).
pub const PGRP_SIGQUIT: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pgid_self_is_zero() {
        assert_eq!(PGID_SELF, 0);
    }

    #[test]
    fn test_wait_values_distinct() {
        assert_ne!(WAIT_PGRP_SELF, WAIT_ANY_CHILD);
    }

    #[test]
    fn test_wait_any_child() {
        assert_eq!(WAIT_ANY_CHILD, -1);
    }

    #[test]
    fn test_pgrp_max() {
        assert!(PGRP_MAX_DEFAULT > 0);
    }

    #[test]
    fn test_kill_values_distinct() {
        assert_ne!(KILL_PGRP_SELF, KILL_ALL);
    }

    #[test]
    fn test_error_returns() {
        assert_eq!(GETPGID_ERROR, -1);
        assert_eq!(SETPGID_ERROR, -1);
    }

    #[test]
    fn test_signals_distinct() {
        assert_ne!(PGRP_SIGINT, PGRP_SIGQUIT);
    }

    #[test]
    fn test_sigint_is_two() {
        assert_eq!(PGRP_SIGINT, 2);
    }

    #[test]
    fn test_sigquit_is_three() {
        assert_eq!(PGRP_SIGQUIT, 3);
    }
}
