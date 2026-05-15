//! `<sys/ptrace.h>` — process trace.
//!
//! Re-exports `ptrace()` and core `PTRACE_*` constants from the
//! `unistd` module, and adds additional request codes that programs
//! including `<sys/ptrace.h>` expect.

// ---------------------------------------------------------------------------
// Re-exports from unistd
// ---------------------------------------------------------------------------

pub use crate::unistd::ptrace;
pub use crate::unistd::PTRACE_TRACEME;
pub use crate::unistd::PTRACE_PEEKTEXT;
pub use crate::unistd::PTRACE_PEEKDATA;
pub use crate::unistd::PTRACE_POKETEXT;
pub use crate::unistd::PTRACE_POKEDATA;
pub use crate::unistd::PTRACE_CONT;
pub use crate::unistd::PTRACE_KILL;
pub use crate::unistd::PTRACE_SINGLESTEP;
pub use crate::unistd::PTRACE_ATTACH;
pub use crate::unistd::PTRACE_DETACH;

// ---------------------------------------------------------------------------
// Additional PTRACE_* request codes
// ---------------------------------------------------------------------------

/// Read user-area registers.
pub const PTRACE_PEEKUSER: i32 = 3;

/// Write user-area registers.
pub const PTRACE_POKEUSER: i32 = 6;

/// Get registers (general-purpose).
pub const PTRACE_GETREGS: i32 = 12;

/// Set registers (general-purpose).
pub const PTRACE_SETREGS: i32 = 13;

/// Get floating-point registers.
pub const PTRACE_GETFPREGS: i32 = 14;

/// Set floating-point registers.
pub const PTRACE_SETFPREGS: i32 = 15;

/// Set ptrace options.
pub const PTRACE_SETOPTIONS: i32 = 0x4200;

/// Get event message.
pub const PTRACE_GETEVENTMSG: i32 = 0x4201;

/// Get signal information.
pub const PTRACE_GETSIGINFO: i32 = 0x4202;

/// Set signal information.
pub const PTRACE_SETSIGINFO: i32 = 0x4203;

/// Get register set.
pub const PTRACE_GETREGSET: i32 = 0x4204;

/// Set register set.
pub const PTRACE_SETREGSET: i32 = 0x4205;

/// Seize a tracee without stopping it.
pub const PTRACE_SEIZE: i32 = 0x4206;

/// Interrupt a seized tracee.
pub const PTRACE_INTERRUPT: i32 = 0x4207;

/// Listen for events (seized tracee).
pub const PTRACE_LISTEN: i32 = 0x4208;

/// Continue and deliver a signal.
pub const PTRACE_SYSCALL: i32 = 24;

// ---------------------------------------------------------------------------
// Ptrace options (for PTRACE_SETOPTIONS)
// ---------------------------------------------------------------------------

/// Trace fork.
pub const PTRACE_O_TRACEFORK: i32 = 0x02;

/// Trace vfork.
pub const PTRACE_O_TRACEVFORK: i32 = 0x04;

/// Trace clone.
pub const PTRACE_O_TRACECLONE: i32 = 0x08;

/// Trace exec.
pub const PTRACE_O_TRACEEXEC: i32 = 0x10;

/// Trace vfork-done.
pub const PTRACE_O_TRACEVFORKDONE: i32 = 0x20;

/// Trace exit.
pub const PTRACE_O_TRACEEXIT: i32 = 0x40;

/// Trace seccomp events.
pub const PTRACE_O_TRACESECCOMP: i32 = 0x80;

/// Suspend the tracee on PTRACE_SEIZE.
pub const PTRACE_O_SUSPEND_SECCOMP: i32 = 0x200000;

/// Exit-kill: tracee is killed when tracer exits.
pub const PTRACE_O_EXITKILL: i32 = 0x100000;

// ---------------------------------------------------------------------------
// Ptrace event codes
// ---------------------------------------------------------------------------

/// Fork event.
pub const PTRACE_EVENT_FORK: i32 = 1;

/// Vfork event.
pub const PTRACE_EVENT_VFORK: i32 = 2;

/// Clone event.
pub const PTRACE_EVENT_CLONE: i32 = 3;

/// Exec event.
pub const PTRACE_EVENT_EXEC: i32 = 4;

/// Vfork-done event.
pub const PTRACE_EVENT_VFORK_DONE: i32 = 5;

/// Exit event.
pub const PTRACE_EVENT_EXIT: i32 = 6;

/// Seccomp event.
pub const PTRACE_EVENT_SECCOMP: i32 = 7;

/// Stop event.
pub const PTRACE_EVENT_STOP: i32 = 128;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Core request constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_core_constants() {
        assert_eq!(PTRACE_TRACEME, 0);
        assert_eq!(PTRACE_PEEKTEXT, 1);
        assert_eq!(PTRACE_PEEKDATA, 2);
        assert_eq!(PTRACE_CONT, 7);
        assert_eq!(PTRACE_KILL, 8);
        assert_eq!(PTRACE_SINGLESTEP, 9);
        assert_eq!(PTRACE_ATTACH, 16);
        assert_eq!(PTRACE_DETACH, 17);
    }

    #[test]
    fn test_additional_constants() {
        assert_eq!(PTRACE_PEEKUSER, 3);
        assert_eq!(PTRACE_POKEUSER, 6);
        assert_eq!(PTRACE_GETREGS, 12);
        assert_eq!(PTRACE_SETREGS, 13);
        assert_eq!(PTRACE_GETFPREGS, 14);
        assert_eq!(PTRACE_SETFPREGS, 15);
        assert_eq!(PTRACE_SYSCALL, 24);
    }

    #[test]
    fn test_core_requests_distinct() {
        let reqs = [
            PTRACE_TRACEME, PTRACE_PEEKTEXT, PTRACE_PEEKDATA,
            PTRACE_PEEKUSER, PTRACE_POKETEXT, PTRACE_POKEDATA,
            PTRACE_POKEUSER, PTRACE_CONT, PTRACE_KILL,
            PTRACE_SINGLESTEP, PTRACE_GETREGS, PTRACE_SETREGS,
            PTRACE_GETFPREGS, PTRACE_SETFPREGS, PTRACE_ATTACH,
            PTRACE_DETACH, PTRACE_SYSCALL,
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(
                    reqs[i], reqs[j],
                    "PTRACE request codes must be distinct"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Extended request constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_extended_requests() {
        assert_eq!(PTRACE_SETOPTIONS, 0x4200);
        assert_eq!(PTRACE_GETEVENTMSG, 0x4201);
        assert_eq!(PTRACE_GETSIGINFO, 0x4202);
        assert_eq!(PTRACE_SETSIGINFO, 0x4203);
    }

    #[test]
    fn test_extended_requests_distinct() {
        let ext = [
            PTRACE_SETOPTIONS, PTRACE_GETEVENTMSG,
            PTRACE_GETSIGINFO, PTRACE_SETSIGINFO,
            PTRACE_GETREGSET, PTRACE_SETREGSET,
            PTRACE_SEIZE, PTRACE_INTERRUPT, PTRACE_LISTEN,
        ];
        for i in 0..ext.len() {
            for j in (i + 1)..ext.len() {
                assert_ne!(ext[i], ext[j]);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Options
    // -----------------------------------------------------------------------

    #[test]
    fn test_options_are_bitmask() {
        let opts = [
            PTRACE_O_TRACEFORK, PTRACE_O_TRACEVFORK,
            PTRACE_O_TRACECLONE, PTRACE_O_TRACEEXEC,
            PTRACE_O_TRACEVFORKDONE, PTRACE_O_TRACEEXIT,
            PTRACE_O_TRACESECCOMP,
        ];
        for &o in &opts {
            assert_ne!(o, 0);
            assert_eq!(o & (o - 1), 0, "option 0x{o:X} is not a power of two");
        }
    }

    #[test]
    fn test_options_distinct() {
        let opts = [
            PTRACE_O_TRACEFORK, PTRACE_O_TRACEVFORK,
            PTRACE_O_TRACECLONE, PTRACE_O_TRACEEXEC,
            PTRACE_O_TRACEVFORKDONE, PTRACE_O_TRACEEXIT,
            PTRACE_O_TRACESECCOMP, PTRACE_O_EXITKILL,
            PTRACE_O_SUSPEND_SECCOMP,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------------

    #[test]
    fn test_events() {
        assert_eq!(PTRACE_EVENT_FORK, 1);
        assert_eq!(PTRACE_EVENT_VFORK, 2);
        assert_eq!(PTRACE_EVENT_CLONE, 3);
        assert_eq!(PTRACE_EVENT_EXEC, 4);
        assert_eq!(PTRACE_EVENT_EXIT, 6);
        assert_eq!(PTRACE_EVENT_SECCOMP, 7);
        assert_eq!(PTRACE_EVENT_STOP, 128);
    }

    #[test]
    fn test_events_distinct() {
        let evts = [
            PTRACE_EVENT_FORK, PTRACE_EVENT_VFORK,
            PTRACE_EVENT_CLONE, PTRACE_EVENT_EXEC,
            PTRACE_EVENT_VFORK_DONE, PTRACE_EVENT_EXIT,
            PTRACE_EVENT_SECCOMP, PTRACE_EVENT_STOP,
        ];
        for i in 0..evts.len() {
            for j in (i + 1)..evts.len() {
                assert_ne!(evts[i], evts[j]);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Function stub
    // -----------------------------------------------------------------------

    #[test]
    fn test_ptrace_returns_enosys() {
        let ret = ptrace(PTRACE_TRACEME, 0, 0, 0);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // Cross-module consistency
    // -----------------------------------------------------------------------

    #[test]
    fn test_cross_module() {
        assert_eq!(PTRACE_TRACEME, crate::unistd::PTRACE_TRACEME);
        assert_eq!(PTRACE_CONT, crate::unistd::PTRACE_CONT);
        assert_eq!(PTRACE_ATTACH, crate::unistd::PTRACE_ATTACH);
    }
}
