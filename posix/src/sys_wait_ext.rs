//! Extended wait definitions (Linux-specific).
//!
//! Provides `idtype_t` values for `waitid()` and the `siginfo_t`-based
//! wait APIs.

// Re-export base wait API.
pub use crate::sys_wait::*;

// ---------------------------------------------------------------------------
// idtype_t values for waitid()
// ---------------------------------------------------------------------------

/// Wait for any child process.
pub const P_ALL: i32 = 0;

/// Wait for a specific process ID.
pub const P_PID: i32 = 1;

/// Wait for a specific process group ID.
pub const P_PGID: i32 = 2;

/// Wait for a specific process group (alias).
pub const P_PIDFD: i32 = 3;

// ---------------------------------------------------------------------------
// Additional wait flags
// ---------------------------------------------------------------------------

/// Leave the child waitable (don't consume the wait status).
pub const WEXITED_FLAG: i32 = WEXITED;

/// Clone-specific: wait for clone children.
pub const __WCLONE: i32 = 0x80000000_u32 as i32;

/// Wait for all children, regardless of type.
pub const __WALL: i32 = 0x40000000;

/// Wait for children without ptrace.
pub const __WNOTHREAD: i32 = 0x20000000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_p_types_distinct() {
        let types = [P_ALL, P_PID, P_PGID, P_PIDFD];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_p_all_zero() {
        assert_eq!(P_ALL, 0);
    }

    #[test]
    fn test_w_clone_flags() {
        assert_ne!(__WCLONE, 0);
        assert_ne!(__WALL, 0);
        assert_ne!(__WNOTHREAD, 0);
    }

    #[test]
    fn test_w_clone_no_overlap_with_standard() {
        // These should not overlap with WNOHANG/WUNTRACED/WCONTINUED.
        assert_eq!(__WCLONE & WNOHANG, 0);
        assert_eq!(__WALL & WNOHANG, 0);
        assert_eq!(__WNOTHREAD & WNOHANG, 0);
    }

    #[test]
    fn test_wifexited_via_reexport() {
        // Verify re-exports work.
        let status = 42 << 8; // normal exit code 42
        assert!(wifexited(status));
        assert_eq!(wexitstatus(status), 42);
    }
}
