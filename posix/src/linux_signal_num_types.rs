//! `<signal.h>` — Signal number constants.
//!
//! Unix signals are asynchronous notifications delivered to processes.
//! These constants define the standard signal numbers on Linux x86_64.
//! The first 31 are standard (POSIX); 32-64 are real-time signals.

// ---------------------------------------------------------------------------
// Standard signals (1-31)
// ---------------------------------------------------------------------------

/// Hangup (terminal disconnected).
pub const SIGHUP: u32 = 1;
/// Interrupt (Ctrl-C).
pub const SIGINT: u32 = 2;
/// Quit (Ctrl-\, produces core dump).
pub const SIGQUIT: u32 = 3;
/// Illegal instruction.
pub const SIGILL: u32 = 4;
/// Trace/breakpoint trap.
pub const SIGTRAP: u32 = 5;
/// Abort signal (from abort()).
pub const SIGABRT: u32 = 6;
/// Bus error (bad memory access alignment).
pub const SIGBUS: u32 = 7;
/// Floating-point exception.
pub const SIGFPE: u32 = 8;
/// Kill signal (cannot be caught or ignored).
pub const SIGKILL: u32 = 9;
/// User-defined signal 1.
pub const SIGUSR1: u32 = 10;
/// Segmentation fault (invalid memory reference).
pub const SIGSEGV: u32 = 11;
/// User-defined signal 2.
pub const SIGUSR2: u32 = 12;
/// Broken pipe (write to pipe with no reader).
pub const SIGPIPE: u32 = 13;
/// Alarm clock (from alarm()).
pub const SIGALRM: u32 = 14;
/// Termination signal.
pub const SIGTERM: u32 = 15;
/// Stack fault (unused on modern Linux).
pub const SIGSTKFLT: u32 = 16;
/// Child process stopped or terminated.
pub const SIGCHLD: u32 = 17;
/// Continue (resume stopped process).
pub const SIGCONT: u32 = 18;
/// Stop signal (cannot be caught or ignored).
pub const SIGSTOP: u32 = 19;
/// Terminal stop (Ctrl-Z).
pub const SIGTSTP: u32 = 20;
/// Background process read from terminal.
pub const SIGTTIN: u32 = 21;
/// Background process wrote to terminal.
pub const SIGTTOU: u32 = 22;
/// Urgent condition on socket.
pub const SIGURG: u32 = 23;
/// CPU time limit exceeded.
pub const SIGXCPU: u32 = 24;
/// File size limit exceeded.
pub const SIGXFSZ: u32 = 25;
/// Virtual timer expired.
pub const SIGVTALRM: u32 = 26;
/// Profiling timer expired.
pub const SIGPROF: u32 = 27;
/// Window resize signal.
pub const SIGWINCH: u32 = 28;
/// I/O possible (poll-driven I/O).
pub const SIGIO: u32 = 29;
/// Power failure.
pub const SIGPWR: u32 = 30;
/// Bad system call (invalid syscall number).
pub const SIGSYS: u32 = 31;

// ---------------------------------------------------------------------------
// Real-time signal range
// ---------------------------------------------------------------------------

/// First real-time signal number.
pub const SIGRTMIN: u32 = 32;
/// Last real-time signal number.
pub const SIGRTMAX: u32 = 64;
/// Number of standard signals.
pub const NSIG: u32 = 65;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signals_distinct() {
        let sigs = [
            SIGHUP, SIGINT, SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGBUS, SIGFPE, SIGKILL, SIGUSR1,
            SIGSEGV, SIGUSR2, SIGPIPE, SIGALRM, SIGTERM, SIGSTKFLT, SIGCHLD, SIGCONT, SIGSTOP,
            SIGTSTP, SIGTTIN, SIGTTOU, SIGURG, SIGXCPU, SIGXFSZ, SIGVTALRM, SIGPROF, SIGWINCH,
            SIGIO, SIGPWR, SIGSYS,
        ];
        for i in 0..sigs.len() {
            for j in (i + 1)..sigs.len() {
                assert_ne!(sigs[i], sigs[j]);
            }
        }
    }

    #[test]
    fn test_well_known_values() {
        assert_eq!(SIGKILL, 9);
        assert_eq!(SIGTERM, 15);
        assert_eq!(SIGINT, 2);
        assert_eq!(SIGSEGV, 11);
    }

    #[test]
    fn test_rt_signal_range() {
        assert_eq!(SIGRTMIN, 32);
        assert_eq!(SIGRTMAX, 64);
        assert!(SIGRTMIN < SIGRTMAX);
    }

    #[test]
    fn test_nsig() {
        assert_eq!(NSIG, 65);
        assert!(NSIG > SIGRTMAX);
    }
}
