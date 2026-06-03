//! `<sys/wait.h>` — process wait definitions.
//!
//! Re-exports wait functions and option constants from the `process`
//! and `wait` modules.

// ---------------------------------------------------------------------------
// Wait functions
// ---------------------------------------------------------------------------

pub use crate::process::wait;
pub use crate::process::wait3;
pub use crate::process::wait4;
pub use crate::process::waitid;
pub use crate::process::waitpid;

// ---------------------------------------------------------------------------
// Option flags
// ---------------------------------------------------------------------------

pub use crate::wait::WCONTINUED;
pub use crate::wait::WEXITED;
pub use crate::wait::WNOHANG;
pub use crate::wait::WNOWAIT;
pub use crate::wait::WSTOPPED;
pub use crate::wait::WUNTRACED;

// ---------------------------------------------------------------------------
// Status macros (as functions)
// ---------------------------------------------------------------------------

/// True if the child terminated normally.
#[inline]
#[allow(clippy::verbose_bit_mask)] // matches the canonical glibc `WIFEXITED` macro form
pub const fn wifexited(status: i32) -> bool {
    (status & 0x7F) == 0
}

/// Exit status of the child (only valid if `wifexited` is true).
#[inline]
pub const fn wexitstatus(status: i32) -> i32 {
    (status >> 8) & 0xFF
}

/// True if the child was terminated by a signal.
#[inline]
pub const fn wifsignaled(status: i32) -> bool {
    // `status & 0x7F` is in 0..=127, so `+ 1` cannot overflow i32.
    // This matches the canonical glibc `WIFSIGNALED` macro.
    #[allow(clippy::arithmetic_side_effects)]
    {
        ((status & 0x7F) + 1) as i8 >= 2
    }
}

/// Signal number that caused the child to terminate.
#[inline]
pub const fn wtermsig(status: i32) -> i32 {
    status & 0x7F
}

/// True if the child is currently stopped.
#[inline]
pub const fn wifstopped(status: i32) -> bool {
    (status & 0xFF) == 0x7F
}

/// Signal number that caused the child to stop.
#[inline]
pub const fn wstopsig(status: i32) -> i32 {
    (status >> 8) & 0xFF
}

/// True if the child has continued from a stop.
#[inline]
pub const fn wifcontinued(status: i32) -> bool {
    status == 0xFFFF
}

/// True if the child produced a core dump.
#[inline]
pub const fn wcoredump(status: i32) -> bool {
    (status & 0x80) != 0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_option_flags() {
        assert_eq!(WNOHANG, 1);
        assert_eq!(WUNTRACED, 2);
        assert_eq!(WCONTINUED, 8);
    }

    #[test]
    fn test_option_flags_distinct() {
        let flags = [WNOHANG, WUNTRACED, WCONTINUED, WEXITED, WNOWAIT];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_wifexited_normal_exit() {
        // Normal exit with code 42: status = (42 << 8) | 0
        let status = 42 << 8;
        assert!(wifexited(status));
        assert_eq!(wexitstatus(status), 42);
    }

    #[test]
    fn test_wifexited_zero() {
        // Normal exit with code 0.
        let status = 0;
        assert!(wifexited(status));
        assert_eq!(wexitstatus(status), 0);
    }

    #[test]
    fn test_wifsignaled() {
        // Killed by signal 9 (SIGKILL): status = 9
        let status = 9;
        assert!(wifsignaled(status));
        assert_eq!(wtermsig(status), 9);
        assert!(!wifexited(status));
    }

    #[test]
    fn test_wifstopped() {
        // Stopped by signal 19 (SIGSTOP): status = (19 << 8) | 0x7F
        let status = (19 << 8) | 0x7F;
        assert!(wifstopped(status));
        assert_eq!(wstopsig(status), 19);
    }

    #[test]
    fn test_wifcontinued() {
        let status = 0xFFFF;
        assert!(wifcontinued(status));
    }

    #[test]
    fn test_wcoredump() {
        // Signal 11 with core dump: status = 11 | 0x80
        let status = 11 | 0x80;
        assert!(wcoredump(status));
        assert!(wifsignaled(status));
    }

    #[test]
    fn test_wcoredump_no_core() {
        // Signal 9 without core dump.
        let status = 9;
        assert!(!wcoredump(status));
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(WNOHANG, crate::wait::WNOHANG);
        assert_eq!(WUNTRACED, crate::wait::WUNTRACED);
        assert_eq!(WCONTINUED, crate::wait::WCONTINUED);
    }
}
