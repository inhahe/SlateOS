//! `<linux/exit.h>` — Process exit and termination constants.
//!
//! When a process exits (via exit(), _exit(), or being killed by a
//! signal), the kernel records an exit code encoding why it terminated.
//! The parent process retrieves this via wait()/waitpid(). The exit
//! status encodes either a normal exit code (0-255) or signal information
//! (which signal killed it, whether it dumped core). The kernel also
//! handles exit groups (all threads in a process exit together).

// ---------------------------------------------------------------------------
// Exit status encoding (waitpid status word)
// ---------------------------------------------------------------------------

/// Mask for exit code in status word (bits 8-15).
pub const EXIT_CODE_MASK: u32 = 0xFF00;
/// Shift to extract exit code.
pub const EXIT_CODE_SHIFT: u32 = 8;
/// Mask for signal number (bits 0-6).
pub const EXIT_SIGNAL_MASK: u32 = 0x7F;
/// Core dump flag (bit 7).
pub const EXIT_CORE_DUMP: u32 = 0x80;
/// Process was stopped (not exited).
pub const EXIT_STOPPED: u32 = 0x7F;
/// Process continued (SIGCONT).
pub const EXIT_CONTINUED: u32 = 0xFFFF;

// ---------------------------------------------------------------------------
// Standard exit codes
// ---------------------------------------------------------------------------

/// Successful termination.
pub const EXIT_SUCCESS: u32 = 0;
/// Generic failure.
pub const EXIT_FAILURE: u32 = 1;
/// Command not found (shell convention).
pub const EXIT_NOT_FOUND: u32 = 127;
/// Command not executable (shell convention).
pub const EXIT_NOT_EXEC: u32 = 126;
/// Killed by signal N = 128 + N (shell convention).
pub const EXIT_SIGNAL_BASE: u32 = 128;

// ---------------------------------------------------------------------------
// Exit flags (do_exit / exit_group behavior)
// ---------------------------------------------------------------------------

/// Exit all threads in the thread group.
pub const EXIT_GROUP: u32 = 0x01;
/// Process was killed by OOM killer.
pub const EXIT_OOM_KILLED: u32 = 0x02;
/// Process exited due to seccomp kill.
pub const EXIT_SECCOMP: u32 = 0x04;
/// Exit due to fatal signal.
pub const EXIT_SIGNAL_FATAL: u32 = 0x08;

// ---------------------------------------------------------------------------
// Process states relevant to exit
// ---------------------------------------------------------------------------

/// Zombie (exited, waiting for parent to wait()).
pub const TASK_DEAD: u32 = 0x0080;
/// Exit zombie (final state, being reaped).
pub const EXIT_ZOMBIE: u32 = 0x0020;
/// Exit dead (fully reaped, task_struct being freed).
pub const EXIT_DEAD_STATE: u32 = 0x0010;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_codes_distinct() {
        let codes = [EXIT_SUCCESS, EXIT_FAILURE, EXIT_NOT_FOUND, EXIT_NOT_EXEC];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_status_encoding() {
        // Simulate normal exit with code 42
        let status = 42 << EXIT_CODE_SHIFT;
        assert_eq!((status & EXIT_CODE_MASK) >> EXIT_CODE_SHIFT, 42);
        assert_eq!(status & EXIT_SIGNAL_MASK, 0); // no signal
    }

    #[test]
    fn test_signal_base() {
        // Signal 9 (SIGKILL) → exit code 137
        assert_eq!(EXIT_SIGNAL_BASE + 9, 137);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            EXIT_GROUP, EXIT_OOM_KILLED,
            EXIT_SECCOMP, EXIT_SIGNAL_FATAL,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
