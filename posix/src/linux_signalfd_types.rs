//! `<linux/signalfd.h>` — signalfd() file descriptor constants.
//!
//! signalfd creates a file descriptor that can be used to accept
//! signals synchronously via read(). This avoids the reentrancy
//! issues of traditional signal handlers by delivering signal info
//! as structured data on a pollable fd. Combined with epoll/io_uring,
//! it enables unified event-driven signal handling.

// ---------------------------------------------------------------------------
// signalfd flags
// ---------------------------------------------------------------------------

/// Set close-on-exec on the new fd.
pub const SFD_CLOEXEC: u32 = 0o200_0000;
/// Set non-blocking on the new fd.
pub const SFD_NONBLOCK: u32 = 0o000_4000;

// ---------------------------------------------------------------------------
// signalfd_siginfo structure field sizes
// ---------------------------------------------------------------------------

/// Size of struct signalfd_siginfo (128 bytes).
pub const SIGNALFD_SIGINFO_SIZE: u32 = 128;

// ---------------------------------------------------------------------------
// Signal numbers (subset commonly used with signalfd)
// ---------------------------------------------------------------------------

/// Hangup.
pub const SIGHUP: u32 = 1;
/// Interrupt (Ctrl+C).
pub const SIGINT: u32 = 2;
/// Quit (Ctrl+\).
pub const SIGQUIT: u32 = 3;
/// Illegal instruction.
pub const SIGILL: u32 = 4;
/// Abort.
pub const SIGABRT: u32 = 6;
/// Floating-point exception.
pub const SIGFPE: u32 = 8;
/// Kill (cannot be caught).
pub const SIGKILL: u32 = 9;
/// Segmentation fault.
pub const SIGSEGV: u32 = 11;
/// Broken pipe.
pub const SIGPIPE: u32 = 13;
/// Alarm clock.
pub const SIGALRM: u32 = 14;
/// Termination.
pub const SIGTERM: u32 = 15;
/// Child status changed.
pub const SIGCHLD: u32 = 17;
/// Continue if stopped.
pub const SIGCONT: u32 = 18;
/// Stop (cannot be caught).
pub const SIGSTOP: u32 = 19;
/// Terminal stop (Ctrl+Z).
pub const SIGTSTP: u32 = 20;
/// Urgent socket condition.
pub const SIGURG: u32 = 23;
/// User-defined signal 1.
pub const SIGUSR1: u32 = 10;
/// User-defined signal 2.
pub const SIGUSR2: u32 = 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_distinct() {
        assert_ne!(SFD_CLOEXEC, SFD_NONBLOCK);
    }

    #[test]
    fn test_signal_numbers_distinct() {
        let sigs = [
            SIGHUP, SIGINT, SIGQUIT, SIGILL, SIGABRT, SIGFPE, SIGKILL, SIGUSR1, SIGSEGV, SIGUSR2,
            SIGPIPE, SIGALRM, SIGTERM, SIGCHLD, SIGCONT, SIGSTOP, SIGTSTP, SIGURG,
        ];
        for i in 0..sigs.len() {
            for j in (i + 1)..sigs.len() {
                assert_ne!(sigs[i], sigs[j]);
            }
        }
    }

    #[test]
    fn test_siginfo_size() {
        assert_eq!(SIGNALFD_SIGINFO_SIZE, 128);
    }

    #[test]
    fn test_signal_range() {
        let sigs = [
            SIGHUP, SIGINT, SIGQUIT, SIGILL, SIGABRT, SIGFPE, SIGKILL, SIGUSR1, SIGSEGV, SIGUSR2,
            SIGPIPE, SIGALRM, SIGTERM, SIGCHLD, SIGCONT, SIGSTOP, SIGTSTP, SIGURG,
        ];
        for &s in &sigs {
            assert!(s >= 1 && s <= 64);
        }
    }
}
