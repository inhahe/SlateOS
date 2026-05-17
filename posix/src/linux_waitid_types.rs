//! `<linux/wait.h>` — waitid()/waitpid() constants and macros.
//!
//! waitid() is the modern wait interface that returns structured
//! information about child process state changes via siginfo_t.
//! It supersedes waitpid() by supporting more ID types (PID, PGID,
//! pidfd) and returning richer status information without bitfield
//! packing.

// ---------------------------------------------------------------------------
// waitid id_type values (first argument)
// ---------------------------------------------------------------------------

/// Wait for any child (id is ignored).
pub const P_ALL: u32 = 0;
/// Wait for child with specific PID.
pub const P_PID: u32 = 1;
/// Wait for child in specific process group.
pub const P_PGID: u32 = 2;
/// Wait for child identified by pidfd (Linux 5.4+).
pub const P_PIDFD: u32 = 3;

// ---------------------------------------------------------------------------
// waitid options (bitfield, OR together)
// ---------------------------------------------------------------------------

/// Report terminated children.
pub const WEXITED: u32 = 0x0000_0004;
/// Report stopped (signaled) children.
pub const WSTOPPED: u32 = 0x0000_0002;
/// Report continued children.
pub const WCONTINUED: u32 = 0x0000_0008;
/// Don't reap (leave waitable).
pub const WNOWAIT: u32 = 0x0100_0000;
/// Don't block (return immediately).
pub const WNOHANG: u32 = 0x0000_0001;
/// Also report cloned children.
pub const __WCLONE: u32 = 0x8000_0000;
/// Also report non-SIGCHLD children.
pub const __WALL: u32 = 0x4000_0000;

// ---------------------------------------------------------------------------
// waitpid() status macros — CLD_* codes in siginfo_t.si_code
// ---------------------------------------------------------------------------

/// Child exited normally.
pub const CLD_EXITED: u32 = 1;
/// Child killed by signal.
pub const CLD_KILLED: u32 = 2;
/// Child killed by signal and dumped core.
pub const CLD_DUMPED: u32 = 3;
/// Child was trapped (ptrace).
pub const CLD_TRAPPED: u32 = 4;
/// Child was stopped.
pub const CLD_STOPPED: u32 = 5;
/// Child was continued.
pub const CLD_CONTINUED: u32 = 6;

// ---------------------------------------------------------------------------
// Traditional wait status extraction (for waitpid compatibility)
// ---------------------------------------------------------------------------

/// Mask for exit status byte.
pub const WAIT_STATUS_MASK: u32 = 0x7F;
/// Bit indicating core dump.
pub const WAIT_COREDUMP_BIT: u32 = 0x80;
/// Exit status shift (bits 8-15).
pub const WAIT_EXIT_SHIFT: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_options_no_overlap() {
        // The core options that are bitwise
        let opts = [WNOHANG, WSTOPPED, WEXITED, WCONTINUED, WNOWAIT, __WCLONE, __WALL];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_eq!(opts[i] & opts[j], 0);
            }
        }
    }

    #[test]
    fn test_cld_codes_distinct() {
        let codes = [
            CLD_EXITED, CLD_KILLED, CLD_DUMPED,
            CLD_TRAPPED, CLD_STOPPED, CLD_CONTINUED,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_cld_codes_sequential() {
        assert_eq!(CLD_EXITED, 1);
        assert_eq!(CLD_KILLED, 2);
        assert_eq!(CLD_DUMPED, 3);
        assert_eq!(CLD_TRAPPED, 4);
        assert_eq!(CLD_STOPPED, 5);
        assert_eq!(CLD_CONTINUED, 6);
    }

    #[test]
    fn test_status_mask_range() {
        assert_eq!(WAIT_STATUS_MASK, 0x7F);
        assert_eq!(WAIT_COREDUMP_BIT, 0x80);
        assert_eq!(WAIT_EXIT_SHIFT, 8);
    }
}
