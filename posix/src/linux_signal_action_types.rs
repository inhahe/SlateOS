//! `<signal.h>` — Signal action (sigaction) flag constants.
//!
//! These flags modify signal delivery behavior when passed in the
//! `sa_flags` field of the `sigaction` structure. They control
//! restart semantics, stack usage, and child process notifications.

// ---------------------------------------------------------------------------
// sigaction sa_flags
// ---------------------------------------------------------------------------

/// Don't send SIGCHLD when child stops.
pub const SA_NOCLDSTOP: u32 = 0x0000_0001;
/// Don't create zombie on child death.
pub const SA_NOCLDWAIT: u32 = 0x0000_0002;
/// Use sa_sigaction handler (3-arg form).
pub const SA_SIGINFO: u32 = 0x0000_0004;
/// Use alternate signal stack.
pub const SA_ONSTACK: u32 = 0x0800_0000;
/// Restart interrupted syscalls.
pub const SA_RESTART: u32 = 0x1000_0000;
/// Don't block signal in handler.
pub const SA_NODEFER: u32 = 0x4000_0000;
/// Reset handler to SIG_DFL on entry.
pub const SA_RESETHAND: u32 = 0x8000_0000;
/// Historical (restorer function present).
pub const SA_RESTORER: u32 = 0x0400_0000;

// ---------------------------------------------------------------------------
// Signal handler special values
// ---------------------------------------------------------------------------

/// Default signal action.
pub const SIG_DFL: u64 = 0;
/// Ignore the signal.
pub const SIG_IGN: u64 = 1;
/// Error return from sigaction.
pub const SIG_ERR: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Alternate signal stack flags (ss_flags)
// ---------------------------------------------------------------------------

/// Currently on alternate stack.
pub const SS_ONSTACK: u32 = 1;
/// Alternate stack is disabled.
pub const SS_DISABLE: u32 = 2;
/// Automatic stack discard.
pub const SS_AUTODISARM: u32 = 1 << 31;

// ---------------------------------------------------------------------------
// Default signal stack size
// ---------------------------------------------------------------------------

/// Minimum signal stack size.
pub const MINSIGSTKSZ: u32 = 2048;
/// Default signal stack size.
pub const SIGSTKSZ: u32 = 8192;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sa_flags_distinct() {
        let flags = [
            SA_NOCLDSTOP, SA_NOCLDWAIT, SA_SIGINFO,
            SA_ONSTACK, SA_RESTART, SA_NODEFER,
            SA_RESETHAND, SA_RESTORER,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_sig_special_values() {
        assert_eq!(SIG_DFL, 0);
        assert_eq!(SIG_IGN, 1);
        assert_eq!(SIG_ERR, u64::MAX);
    }

    #[test]
    fn test_ss_flags() {
        assert_eq!(SS_ONSTACK, 1);
        assert_eq!(SS_DISABLE, 2);
        assert_ne!(SS_ONSTACK, SS_DISABLE);
    }

    #[test]
    fn test_stack_sizes() {
        assert!(MINSIGSTKSZ < SIGSTKSZ);
        assert_eq!(MINSIGSTKSZ, 2048);
        assert_eq!(SIGSTKSZ, 8192);
    }

    #[test]
    fn test_sa_restart() {
        assert_eq!(SA_RESTART, 0x1000_0000);
    }
}
