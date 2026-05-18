//! `<signal.h>` — Signal set manipulation constants.
//!
//! Signal sets (sigset_t) are bitmasks that represent collections
//! of signals. They're used with sigprocmask(), sigpending(),
//! sigsuspend(), and pselect(). These constants define the set
//! manipulation "how" arguments and related limits.

// ---------------------------------------------------------------------------
// sigprocmask() "how" argument
// ---------------------------------------------------------------------------

/// Block signals in set (add to current mask).
pub const SIG_BLOCK: u32 = 0;
/// Unblock signals in set (remove from mask).
pub const SIG_UNBLOCK: u32 = 1;
/// Set mask to exactly this set.
pub const SIG_SETMASK: u32 = 2;

// ---------------------------------------------------------------------------
// Signal set size constants
// ---------------------------------------------------------------------------

/// Number of signals in a signal set.
pub const SIGSET_NWORDS: u32 = 1;
/// Size of sigset_t in bytes (on x86_64 Linux: 8 bytes = 64 bits).
pub const SIGSET_SIZE: u32 = 8;

// ---------------------------------------------------------------------------
// signalfd flags
// ---------------------------------------------------------------------------

/// Close-on-exec for signalfd.
pub const SFD_CLOEXEC: u32 = 0o2000000;
/// Non-blocking for signalfd.
pub const SFD_NONBLOCK: u32 = 0o4000;

// ---------------------------------------------------------------------------
// signalfd_siginfo field sizes
// ---------------------------------------------------------------------------

/// Size of signalfd_siginfo structure.
pub const SIGNALFD_SIGINFO_SIZE: u32 = 128;

// ---------------------------------------------------------------------------
// sigwaitinfo/sigtimedwait related
// ---------------------------------------------------------------------------

/// Maximum number of queued signals per process (default).
pub const SIGQUEUE_MAX_DEFAULT: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sigprocmask_how_distinct() {
        let hows = [SIG_BLOCK, SIG_UNBLOCK, SIG_SETMASK];
        for i in 0..hows.len() {
            for j in (i + 1)..hows.len() {
                assert_ne!(hows[i], hows[j]);
            }
        }
    }

    #[test]
    fn test_sig_block_is_zero() {
        assert_eq!(SIG_BLOCK, 0);
    }

    #[test]
    fn test_sigset_size() {
        assert_eq!(SIGSET_SIZE, 8);
    }

    #[test]
    fn test_sfd_flags_distinct() {
        assert_ne!(SFD_CLOEXEC, SFD_NONBLOCK);
    }

    #[test]
    fn test_siginfo_size() {
        assert_eq!(SIGNALFD_SIGINFO_SIZE, 128);
    }

    #[test]
    fn test_sigqueue_max() {
        assert_eq!(SIGQUEUE_MAX_DEFAULT, 32);
    }
}
