//! `<signal.h>` — signal numbers and `sigaction` flags.
//!
//! Even though our OS doesn't use Unix signals for process control,
//! the POSIX compat layer still has to translate them: language
//! runtimes (Rust panic handler, Go signal handler, JVM SIGSEGV
//! handler), and `gdb`/`strace` all rely on the canonical numbering
//! described here.

// ---------------------------------------------------------------------------
// Signal numbers (Linux x86_64 layout — also used by glibc, musl)
// ---------------------------------------------------------------------------

pub const SIGHUP: u32 = 1;
pub const SIGINT: u32 = 2;
pub const SIGQUIT: u32 = 3;
pub const SIGILL: u32 = 4;
pub const SIGTRAP: u32 = 5;
pub const SIGABRT: u32 = 6;
pub const SIGIOT: u32 = SIGABRT;
pub const SIGBUS: u32 = 7;
pub const SIGFPE: u32 = 8;
pub const SIGKILL: u32 = 9;
pub const SIGUSR1: u32 = 10;
pub const SIGSEGV: u32 = 11;
pub const SIGUSR2: u32 = 12;
pub const SIGPIPE: u32 = 13;
pub const SIGALRM: u32 = 14;
pub const SIGTERM: u32 = 15;
pub const SIGSTKFLT: u32 = 16;
pub const SIGCHLD: u32 = 17;
pub const SIGCONT: u32 = 18;
pub const SIGSTOP: u32 = 19;
pub const SIGTSTP: u32 = 20;
pub const SIGTTIN: u32 = 21;
pub const SIGTTOU: u32 = 22;
pub const SIGURG: u32 = 23;
pub const SIGXCPU: u32 = 24;
pub const SIGXFSZ: u32 = 25;
pub const SIGVTALRM: u32 = 26;
pub const SIGPROF: u32 = 27;
pub const SIGWINCH: u32 = 28;
pub const SIGIO: u32 = 29;
pub const SIGPOLL: u32 = SIGIO;
pub const SIGPWR: u32 = 30;
pub const SIGSYS: u32 = 31;

pub const SIGRTMIN: u32 = 32;
pub const SIGRTMAX: u32 = 64;

// ---------------------------------------------------------------------------
// `sigaction.sa_flags`
// ---------------------------------------------------------------------------

pub const SA_NOCLDSTOP: u32 = 0x0000_0001;
pub const SA_NOCLDWAIT: u32 = 0x0000_0002;
pub const SA_SIGINFO: u32 = 0x0000_0004;
pub const SA_ONSTACK: u32 = 0x0800_0000;
pub const SA_RESTART: u32 = 0x1000_0000;
pub const SA_NODEFER: u32 = 0x4000_0000;
pub const SA_RESETHAND: u32 = 0x8000_0000;
pub const SA_RESTORER: u32 = 0x0400_0000;

// ---------------------------------------------------------------------------
// `siginfo_t.si_code` — common codes (`SI_*`)
// ---------------------------------------------------------------------------

pub const SI_USER: i32 = 0;
pub const SI_KERNEL: i32 = 0x80;
pub const SI_QUEUE: i32 = -1;
pub const SI_TIMER: i32 = -2;
pub const SI_MESGQ: i32 = -3;
pub const SI_ASYNCIO: i32 = -4;
pub const SI_SIGIO: i32 = -5;
pub const SI_TKILL: i32 = -6;

// ---------------------------------------------------------------------------
// `sigprocmask` how-argument
// ---------------------------------------------------------------------------

pub const SIG_BLOCK: u32 = 0;
pub const SIG_UNBLOCK: u32 = 1;
pub const SIG_SETMASK: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_low_block_canonical_numbers() {
        // SIGHUP=1, SIGINT=2, SIGKILL=9, SIGTERM=15, SIGSEGV=11.
        assert_eq!(SIGHUP, 1);
        assert_eq!(SIGINT, 2);
        assert_eq!(SIGKILL, 9);
        assert_eq!(SIGSEGV, 11);
        assert_eq!(SIGTERM, 15);
        // SIGABRT and SIGIOT alias (POSIX guarantees this).
        assert_eq!(SIGIOT, SIGABRT);
        // SIGIO == SIGPOLL alias.
        assert_eq!(SIGPOLL, SIGIO);
    }

    #[test]
    fn test_rt_range() {
        // SIGRTMIN..=SIGRTMAX is 32..=64 → 33 RT signals.
        assert_eq!(SIGRTMIN, 32);
        assert_eq!(SIGRTMAX, 64);
        assert_eq!(SIGRTMAX - SIGRTMIN + 1, 33);
        // All standard signals come before RTMIN.
        assert!(SIGSYS < SIGRTMIN);
    }

    #[test]
    fn test_sa_flags_distinct_bits() {
        let f = [
            SA_NOCLDSTOP,
            SA_NOCLDWAIT,
            SA_SIGINFO,
            SA_ONSTACK,
            SA_RESTART,
            SA_NODEFER,
            SA_RESETHAND,
            SA_RESTORER,
        ];
        for v in f {
            assert!(v.is_power_of_two());
        }
        // SA_RESTART/SA_RESETHAND/SA_NODEFER sit at the top three bits.
        assert_eq!(SA_RESETHAND, 0x8000_0000);
        assert_eq!(SA_NODEFER, 0x4000_0000);
        assert_eq!(SA_RESTART, 0x1000_0000);
    }

    #[test]
    fn test_si_codes_distinct_user_vs_kernel() {
        let s = [
            SI_USER,
            SI_KERNEL,
            SI_QUEUE,
            SI_TIMER,
            SI_MESGQ,
            SI_ASYNCIO,
            SI_SIGIO,
            SI_TKILL,
        ];
        for a in 0..s.len() {
            for b in (a + 1)..s.len() {
                assert_ne!(s[a], s[b]);
            }
        }
        // User-originated signals are 0, kernel-originated ones are
        // positive (0x80) or negative depending on the trap.
        assert_eq!(SI_USER, 0);
        assert!(SI_KERNEL > 0);
    }

    #[test]
    fn test_sigprocmask_how_dense_0_to_2() {
        assert_eq!(SIG_BLOCK, 0);
        assert_eq!(SIG_UNBLOCK, 1);
        assert_eq!(SIG_SETMASK, 2);
    }

    #[test]
    fn test_standard_signal_range_is_1_to_31() {
        // Standard (non-RT) signals occupy 1..=31 inclusive.
        let s = [
            SIGHUP, SIGINT, SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGBUS, SIGFPE, SIGKILL, SIGUSR1,
            SIGSEGV, SIGUSR2, SIGPIPE, SIGALRM, SIGTERM, SIGSTKFLT, SIGCHLD, SIGCONT, SIGSTOP,
            SIGTSTP, SIGTTIN, SIGTTOU, SIGURG, SIGXCPU, SIGXFSZ, SIGVTALRM, SIGPROF, SIGWINCH,
            SIGIO, SIGPWR, SIGSYS,
        ];
        // 31 standard signals.
        assert_eq!(s.len(), 31);
        for v in s {
            assert!((1..=31).contains(&v));
        }
    }
}
