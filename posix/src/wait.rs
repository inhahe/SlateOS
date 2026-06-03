//! POSIX wait status macros as C-callable functions.
//!
//! Provides `WIFEXITED`, `WEXITSTATUS`, `WIFSIGNALED`, `WTERMSIG`,
//! `WIFSTOPPED`, `WSTOPSIG`, `WCOREDUMP` as exported functions.
//!
//! The underlying logic is in `process.rs`; this module re-exports
//! them with the standard POSIX uppercase names for C compatibility.
//!
//! ## Wait status encoding (Linux-compatible)
//!
//! - Normal exit: bits 15:8 = exit code, bits 7:0 = 0
//! - Signal death: bits 7:0 = signal number (non-zero, not 0x7f)
//! - Stopped: bits 15:8 = stop signal, bits 7:0 = 0x7f

// ---------------------------------------------------------------------------
// Wait option flags
// ---------------------------------------------------------------------------

/// Don't block if no child has exited.
pub const WNOHANG: i32 = 1;
/// Also report stopped children.
pub const WUNTRACED: i32 = 2;
/// Also report continued children.
pub const WCONTINUED: i32 = 8;

// waitid-specific option flags (not used by waitpid).
/// Wait for processes that have exited (waitid).
pub const WEXITED: i32 = 4;
/// Wait for stopped processes (waitid, equivalent to WUNTRACED for waitpid).
pub const WSTOPPED: i32 = 2;
/// Leave the child in a waitable state (don't consume the wait status).
pub const WNOWAIT: i32 = 0x0100_0000;

// ---------------------------------------------------------------------------
// Wait status inspection functions (C-callable)
// ---------------------------------------------------------------------------

/// True if child terminated normally (via `exit()`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_snake_case)]
pub extern "C" fn WIFEXITED(status: i32) -> i32 {
    i32::from(crate::process::wifexited(status))
}

/// Return the exit status of the child.
///
/// Only meaningful if `WIFEXITED(status)` is true.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_snake_case)]
pub extern "C" fn WEXITSTATUS(status: i32) -> i32 {
    crate::process::wexitstatus(status)
}

/// True if child was terminated by a signal.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_snake_case)]
pub extern "C" fn WIFSIGNALED(status: i32) -> i32 {
    i32::from(crate::process::wifsignaled(status))
}

/// Return the signal number that terminated the child.
///
/// Only meaningful if `WIFSIGNALED(status)` is true.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_snake_case)]
pub extern "C" fn WTERMSIG(status: i32) -> i32 {
    crate::process::wtermsig(status)
}

/// True if child is currently stopped.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_snake_case)]
pub extern "C" fn WIFSTOPPED(status: i32) -> i32 {
    // Stopped: low 8 bits = 0x7f.
    i32::from((status & 0xFF) == 0x7F)
}

/// Return the signal that stopped the child.
///
/// Only meaningful if `WIFSTOPPED(status)` is true.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_snake_case)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn WSTOPSIG(status: i32) -> i32 {
    (status >> 8) & 0xFF
}

/// True if child was resumed by `SIGCONT`.
///
/// Linux encoding: continued status is `0xFFFF`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_snake_case)]
pub extern "C" fn WIFCONTINUED(status: i32) -> i32 {
    i32::from(crate::process::wifcontinued(status))
}

/// True if child produced a core dump (non-POSIX but widely available).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(non_snake_case)]
pub extern "C" fn WCOREDUMP(status: i32) -> i32 {
    i32::from(status & 0x80 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn constants_match_posix() {
        assert_eq!(WNOHANG, 1);
        assert_eq!(WUNTRACED, 2);
        assert_eq!(WCONTINUED, 8);
    }

    #[test]
    fn waitid_constants_match_linux() {
        assert_eq!(WEXITED, 4);
        assert_eq!(WSTOPPED, 2); // Same value as WUNTRACED
        assert_eq!(WNOWAIT, 0x0100_0000);
    }

    #[test]
    fn wstopped_equals_wuntraced() {
        // POSIX: WSTOPPED is the waitid equivalent of waitpid's WUNTRACED.
        assert_eq!(WSTOPPED, WUNTRACED);
    }

    // -----------------------------------------------------------------------
    // WIFEXITED — normal exit: low byte == 0
    // -----------------------------------------------------------------------

    #[test]
    fn wifexited_normal_exit() {
        // Normal exit with code 42: (42 << 8) | 0 → low byte is 0.
        let status = 42 << 8;
        assert_eq!(WIFEXITED(status), 1);
    }

    #[test]
    fn wifexited_normal_exit_code_zero() {
        // Normal exit with code 0.
        let status = 0 << 8;
        assert_eq!(WIFEXITED(status), 1);
    }

    #[test]
    fn wifexited_normal_exit_code_255() {
        let status = 255 << 8;
        assert_eq!(WIFEXITED(status), 1);
    }

    #[test]
    fn wifexited_signal_death() {
        // Killed by signal 9 (SIGKILL): low byte = 9.
        let status = 0x09;
        assert_eq!(WIFEXITED(status), 0);
    }

    #[test]
    fn wifexited_stopped() {
        // Stopped: low byte = 0x7F.
        let status = (20 << 8) | 0x7F;
        assert_eq!(WIFEXITED(status), 0);
    }

    // -----------------------------------------------------------------------
    // WEXITSTATUS — bits 15:8 of a normal-exit status
    // -----------------------------------------------------------------------

    #[test]
    fn wexitstatus_extracts_42() {
        let status = 42 << 8;
        assert_eq!(WEXITSTATUS(status), 42);
    }

    #[test]
    fn wexitstatus_extracts_zero() {
        let status = 0 << 8;
        assert_eq!(WEXITSTATUS(status), 0);
    }

    #[test]
    fn wexitstatus_extracts_255() {
        let status = 255 << 8;
        assert_eq!(WEXITSTATUS(status), 255);
    }

    #[test]
    fn wexitstatus_extracts_1() {
        let status = 1 << 8;
        assert_eq!(WEXITSTATUS(status), 1);
    }

    #[test]
    fn wexitstatus_extracts_127() {
        let status = 127 << 8;
        assert_eq!(WEXITSTATUS(status), 127);
    }

    // -----------------------------------------------------------------------
    // WIFSIGNALED — signal death: low 7 bits != 0 and != 0x7f
    // -----------------------------------------------------------------------

    #[test]
    fn wifsignaled_signal_death() {
        // Killed by signal 9.
        let status = 0x09;
        assert_eq!(WIFSIGNALED(status), 1);
    }

    #[test]
    fn wifsignaled_signal_death_sig11() {
        // Killed by SIGSEGV (11).
        let status = 0x0B;
        assert_eq!(WIFSIGNALED(status), 1);
    }

    #[test]
    fn wifsignaled_normal_exit() {
        // Normal exit: low byte = 0 → not signaled.
        let status = 42 << 8;
        assert_eq!(WIFSIGNALED(status), 0);
    }

    #[test]
    fn wifsignaled_stopped() {
        // Stopped: low byte = 0x7F → not signaled.
        let status = (20 << 8) | 0x7F;
        assert_eq!(WIFSIGNALED(status), 0);
    }

    #[test]
    fn wifsignaled_signal_with_core_dump() {
        // Signal 11 with core dump bit (bit 7) set: 0x0B | 0x80 = 0x8B.
        // Low 7 bits = 0x0B (11), which is non-zero and != 0x7F → signaled.
        let status = 0x0B | 0x80;
        assert_eq!(WIFSIGNALED(status), 1);
    }

    // -----------------------------------------------------------------------
    // WTERMSIG — low 7 bits of a signal-death status
    // -----------------------------------------------------------------------

    #[test]
    fn wtermsig_extracts_signal_9() {
        let status = 0x09;
        assert_eq!(WTERMSIG(status), 9);
    }

    #[test]
    fn wtermsig_extracts_signal_11() {
        let status = 0x0B;
        assert_eq!(WTERMSIG(status), 11);
    }

    #[test]
    fn wtermsig_extracts_signal_with_core_dump() {
        // Core dump bit (0x80) should not affect the signal number
        // extraction since WTERMSIG masks with 0x7F.
        // But note: WTERMSIG in wait.rs delegates to process::wtermsig
        // which masks with 0x7f. Let's verify.
        let status = 0x0B | 0x80;
        assert_eq!(WTERMSIG(status), 11);
    }

    #[test]
    fn wtermsig_signal_1() {
        let status = 0x01;
        assert_eq!(WTERMSIG(status), 1);
    }

    // -----------------------------------------------------------------------
    // WIFSTOPPED — low byte == 0x7F
    // -----------------------------------------------------------------------

    #[test]
    fn wifstopped_stopped_process() {
        // Stopped by SIGTSTP (20): (20 << 8) | 0x7F.
        let status = (20 << 8) | 0x7F;
        assert_eq!(WIFSTOPPED(status), 1);
    }

    #[test]
    fn wifstopped_stopped_by_sigstop() {
        // SIGSTOP = 19.
        let status = (19 << 8) | 0x7F;
        assert_eq!(WIFSTOPPED(status), 1);
    }

    #[test]
    fn wifstopped_normal_exit() {
        let status = 42 << 8;
        assert_eq!(WIFSTOPPED(status), 0);
    }

    #[test]
    fn wifstopped_signal_death() {
        let status = 0x09;
        assert_eq!(WIFSTOPPED(status), 0);
    }

    // -----------------------------------------------------------------------
    // WSTOPSIG — bits 15:8 of a stopped status
    // -----------------------------------------------------------------------

    #[test]
    fn wstopsig_extracts_sigtstp() {
        let status = (20 << 8) | 0x7F;
        assert_eq!(WSTOPSIG(status), 20);
    }

    #[test]
    fn wstopsig_extracts_sigstop() {
        let status = (19 << 8) | 0x7F;
        assert_eq!(WSTOPSIG(status), 19);
    }

    #[test]
    fn wstopsig_extracts_sigttin() {
        // SIGTTIN = 21.
        let status = (21 << 8) | 0x7F;
        assert_eq!(WSTOPSIG(status), 21);
    }

    // -----------------------------------------------------------------------
    // WIFCONTINUED — status == 0xFFFF
    // -----------------------------------------------------------------------

    #[test]
    fn wifcontinued_continued_status() {
        let status = 0xFFFF;
        assert_eq!(WIFCONTINUED(status), 1);
    }

    #[test]
    fn wifcontinued_normal_exit() {
        let status = 42 << 8;
        assert_eq!(WIFCONTINUED(status), 0);
    }

    #[test]
    fn wifcontinued_signal_death() {
        let status = 0x09;
        assert_eq!(WIFCONTINUED(status), 0);
    }

    #[test]
    fn wifcontinued_stopped() {
        let status = (20 << 8) | 0x7F;
        assert_eq!(WIFCONTINUED(status), 0);
    }

    // -----------------------------------------------------------------------
    // WCOREDUMP — bit 7
    // -----------------------------------------------------------------------

    #[test]
    fn wcoredump_set() {
        // Signal 11 (SIGSEGV) with core dump: 0x0B | 0x80 = 0x8B.
        let status = 0x0B | 0x80;
        assert_eq!(WCOREDUMP(status), 1);
    }

    #[test]
    fn wcoredump_not_set() {
        // Signal 9 without core dump.
        let status = 0x09;
        assert_eq!(WCOREDUMP(status), 0);
    }

    #[test]
    fn wcoredump_normal_exit_no_core() {
        // Normal exit — bit 7 is 0.
        let status = 42 << 8;
        assert_eq!(WCOREDUMP(status), 0);
    }

    // -----------------------------------------------------------------------
    // Wait status encoding rules — combined scenarios
    // -----------------------------------------------------------------------

    #[test]
    fn encoding_normal_exit_all_macros() {
        // Normal exit with code 7.
        let status = 7 << 8;
        assert_eq!(WIFEXITED(status), 1);
        assert_eq!(WEXITSTATUS(status), 7);
        assert_eq!(WIFSIGNALED(status), 0);
        assert_eq!(WIFSTOPPED(status), 0);
        assert_eq!(WIFCONTINUED(status), 0);
        assert_eq!(WCOREDUMP(status), 0);
    }

    #[test]
    fn encoding_signal_death_all_macros() {
        // Killed by signal 6 (SIGABRT), no core dump.
        let status = 0x06;
        assert_eq!(WIFEXITED(status), 0);
        assert_eq!(WIFSIGNALED(status), 1);
        assert_eq!(WTERMSIG(status), 6);
        assert_eq!(WIFSTOPPED(status), 0);
        assert_eq!(WCOREDUMP(status), 0);
    }

    #[test]
    fn encoding_signal_death_with_core_all_macros() {
        // Killed by signal 11 (SIGSEGV) with core dump.
        let status = 0x0B | 0x80;
        assert_eq!(WIFEXITED(status), 0);
        assert_eq!(WIFSIGNALED(status), 1);
        assert_eq!(WTERMSIG(status), 11);
        assert_eq!(WIFSTOPPED(status), 0);
        assert_eq!(WCOREDUMP(status), 1);
    }

    #[test]
    fn encoding_stopped_all_macros() {
        // Stopped by SIGTSTP (20).
        let status = (20 << 8) | 0x7F;
        assert_eq!(WIFEXITED(status), 0);
        assert_eq!(WIFSIGNALED(status), 0);
        assert_eq!(WIFSTOPPED(status), 1);
        assert_eq!(WSTOPSIG(status), 20);
        assert_eq!(WIFCONTINUED(status), 0);
    }

    #[test]
    fn encoding_continued_all_macros() {
        // Resumed by SIGCONT.
        let status = 0xFFFF;
        assert_eq!(WIFEXITED(status), 0);
        assert_eq!(WIFSIGNALED(status), 0);
        assert_eq!(WIFSTOPPED(status), 0);
        assert_eq!(WIFCONTINUED(status), 1);
    }

    // -----------------------------------------------------------------------
    // Underlying process.rs pure functions
    // -----------------------------------------------------------------------

    #[test]
    fn process_wifexited_normal() {
        assert!(crate::process::wifexited(42 << 8));
        assert!(crate::process::wifexited(0));
    }

    #[test]
    fn process_wifexited_signal() {
        assert!(!crate::process::wifexited(0x09));
        assert!(!crate::process::wifexited(0x0B));
    }

    #[test]
    fn process_wifexited_stopped() {
        assert!(!crate::process::wifexited((20 << 8) | 0x7F));
    }

    #[test]
    fn process_wexitstatus_values() {
        assert_eq!(crate::process::wexitstatus(42 << 8), 42);
        assert_eq!(crate::process::wexitstatus(0), 0);
        assert_eq!(crate::process::wexitstatus(255 << 8), 255);
        assert_eq!(crate::process::wexitstatus(1 << 8), 1);
    }

    #[test]
    fn process_wifsignaled_values() {
        assert!(crate::process::wifsignaled(0x09));
        assert!(crate::process::wifsignaled(0x0B));
        assert!(!crate::process::wifsignaled(42 << 8)); // normal exit
        assert!(!crate::process::wifsignaled((20 << 8) | 0x7F)); // stopped
    }

    #[test]
    fn process_wtermsig_values() {
        assert_eq!(crate::process::wtermsig(0x09), 9);
        assert_eq!(crate::process::wtermsig(0x0B), 11);
        assert_eq!(crate::process::wtermsig(0x01), 1);
        // With core dump bit set — wtermsig masks with 0x7f.
        assert_eq!(crate::process::wtermsig(0x0B | 0x80), 11);
    }
}
