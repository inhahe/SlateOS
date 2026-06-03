//! `<linux/signal.h>` — Signal handling constants.
//!
//! Signals are asynchronous notifications sent to processes. They can
//! indicate errors (SIGSEGV), user requests (SIGINT), child events
//! (SIGCHLD), or application-defined events (SIGUSR1/2). sigaction()
//! installs signal handlers with fine-grained control over signal
//! masks, flags, and alternate signal stacks.

// ---------------------------------------------------------------------------
// sigaction flags (sa_flags)
// ---------------------------------------------------------------------------

/// Restart interrupted syscalls.
pub const SA_RESTART: u32 = 0x1000_0000;
/// Don't block signal during handler.
pub const SA_NODEFER: u32 = 0x4000_0000;
/// Reset to SIG_DFL after handler returns.
pub const SA_RESETHAND: u32 = 0x8000_0000;
/// Use siginfo_t handler (3-arg form).
pub const SA_SIGINFO: u32 = 0x0000_0004;
/// Use alternate signal stack.
pub const SA_ONSTACK: u32 = 0x0800_0000;
/// Don't receive SIGCHLD on child stop.
pub const SA_NOCLDSTOP: u32 = 0x0000_0001;
/// Don't create zombie children.
pub const SA_NOCLDWAIT: u32 = 0x0000_0002;

// ---------------------------------------------------------------------------
// Signal set operations (for sigprocmask)
// ---------------------------------------------------------------------------

/// Block signals in set.
pub const SIG_BLOCK: u32 = 0;
/// Unblock signals in set.
pub const SIG_UNBLOCK: u32 = 1;
/// Set signal mask to set.
pub const SIG_SETMASK: u32 = 2;

// ---------------------------------------------------------------------------
// Special signal handlers
// ---------------------------------------------------------------------------

/// Default signal action.
pub const SIG_DFL: usize = 0;
/// Ignore signal.
pub const SIG_IGN: usize = 1;

// ---------------------------------------------------------------------------
// Signal stack constants
// ---------------------------------------------------------------------------

/// Minimum alternate signal stack size.
pub const MINSIGSTKSZ: u32 = 2048;
/// Default alternate signal stack size.
pub const SIGSTKSZ: u32 = 8192;
/// On alternate stack (sigaltstack flags).
pub const SS_ONSTACK: u32 = 1;
/// Alternate stack disabled.
pub const SS_DISABLE: u32 = 2;
/// Alternate stack autodisarm on entry.
pub const SS_AUTODISARM: u32 = 1 << 31;

// ---------------------------------------------------------------------------
// Real-time signal range
// ---------------------------------------------------------------------------

/// First real-time signal number.
pub const SIGRTMIN: u32 = 32;
/// Last real-time signal number.
pub const SIGRTMAX: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sa_flags_distinct() {
        let flags = [
            SA_NOCLDSTOP,
            SA_NOCLDWAIT,
            SA_SIGINFO,
            SA_ONSTACK,
            SA_RESTART,
            SA_NODEFER,
            SA_RESETHAND,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_sig_ops_distinct() {
        let ops = [SIG_BLOCK, SIG_UNBLOCK, SIG_SETMASK];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_handlers_distinct() {
        assert_ne!(SIG_DFL, SIG_IGN);
    }

    #[test]
    fn test_stack_sizes() {
        assert!(MINSIGSTKSZ < SIGSTKSZ);
        assert!(MINSIGSTKSZ > 0);
    }

    #[test]
    fn test_rt_signal_range() {
        assert!(SIGRTMIN < SIGRTMAX);
        assert!(SIGRTMIN >= 32);
    }
}
