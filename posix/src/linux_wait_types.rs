//! `<linux/wait.h>` — Wait/waitpid option constants.
//!
//! The wait() family of system calls allows a parent process to wait
//! for state changes in its children: exit, stop (SIGSTOP/SIGTSTP),
//! or continue (SIGCONT). Options control which children to wait for
//! and whether to block. waitid() extends this with more specific
//! ID types (PID, PGID, all children) and returns structured siginfo.

// ---------------------------------------------------------------------------
// wait options (waitpid / waitid flags)
// ---------------------------------------------------------------------------

/// Don't block if no child has exited.
pub const WNOHANG: u32 = 0x0000_0001;
/// Also report stopped children.
pub const WUNTRACED: u32 = 0x0000_0002;
/// Also report continued children.
pub const WCONTINUED: u32 = 0x0000_0008;
/// Don't reap (leave child in waitable state, peek only).
pub const WNOWAIT: u32 = 0x0100_0000;
/// Wait for exited children.
pub const WEXITED: u32 = 0x0000_0004;
/// Wait for stopped children.
pub const WSTOPPED: u32 = 0x0000_0002;
/// Wait for any clone child (not just own children).
pub const __WCLONE: u32 = 0x8000_0000;
/// Wait for all children (including clones).
pub const __WALL: u32 = 0x4000_0000;

// ---------------------------------------------------------------------------
// waitid ID types (P_*)
// ---------------------------------------------------------------------------

/// Wait for any child.
pub const P_ALL: u32 = 0;
/// Wait for child with specific PID.
pub const P_PID: u32 = 1;
/// Wait for child in specific process group.
pub const P_PGID: u32 = 2;
/// Wait for child by pidfd.
pub const P_PIDFD: u32 = 3;

// ---------------------------------------------------------------------------
// wait status check macros (as constants for the bit positions)
// ---------------------------------------------------------------------------

/// Shift to check if exited normally.
pub const WEXITSTATUS_SHIFT: u32 = 8;
/// Mask for exit status.
pub const WEXITSTATUS_MASK: u32 = 0xFF;
/// Value indicating process was stopped.
pub const WSTOPPED_VALUE: u32 = 0x7F;
/// Stop signal shift (bits 8-15 of status when stopped).
pub const WSTOPSIG_SHIFT: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wait_options() {
        assert_ne!(WNOHANG, 0);
        assert_ne!(WUNTRACED, 0);
        assert_ne!(WCONTINUED, 0);
        assert_ne!(WNOWAIT, 0);
    }

    #[test]
    fn test_id_types_distinct() {
        let types = [P_ALL, P_PID, P_PGID, P_PIDFD];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_wait_flag_bits() {
        // WNOHANG and WCONTINUED should not overlap
        assert_eq!(WNOHANG & WCONTINUED, 0);
        // __WCLONE and __WALL should not overlap
        assert_eq!(__WCLONE & __WALL, 0);
    }

    #[test]
    fn test_status_encoding() {
        // An exit status of 42 should be at bits 8-15
        let status = 42u32 << WEXITSTATUS_SHIFT;
        assert_eq!((status >> WEXITSTATUS_SHIFT) & WEXITSTATUS_MASK, 42);
    }
}
