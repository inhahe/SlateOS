//! `<linux/ptrace.h>` — ptrace() debugging interface constants.
//!
//! ptrace allows one process (the tracer) to observe and control
//! another (the tracee). It's used by debuggers (GDB, lldb),
//! strace, sandbox enforcers, and checkpoint/restore tools. The
//! tracer can read/write memory, registers, intercept syscalls,
//! and inject signals.

// ---------------------------------------------------------------------------
// ptrace request codes
// ---------------------------------------------------------------------------

/// Attach to process (sends SIGSTOP).
pub const PTRACE_ATTACH: u32 = 16;
/// Detach from process.
pub const PTRACE_DETACH: u32 = 17;
/// Trace me (child requests tracing).
pub const PTRACE_TRACEME: u32 = 0;
/// Read word from tracee memory.
pub const PTRACE_PEEKDATA: u32 = 2;
/// Write word to tracee memory.
pub const PTRACE_POKEDATA: u32 = 5;
/// Read word from tracee user area.
pub const PTRACE_PEEKUSER: u32 = 3;
/// Write word to tracee user area.
pub const PTRACE_POKEUSER: u32 = 6;
/// Get register set.
pub const PTRACE_GETREGSET: u32 = 0x4204;
/// Set register set.
pub const PTRACE_SETREGSET: u32 = 0x4205;
/// Continue execution.
pub const PTRACE_CONT: u32 = 7;
/// Single-step.
pub const PTRACE_SINGLESTEP: u32 = 9;
/// Syscall-stop (stop at next syscall entry/exit).
pub const PTRACE_SYSCALL: u32 = 24;
/// Kill tracee.
pub const PTRACE_KILL: u32 = 8;
/// Set ptrace options.
pub const PTRACE_SETOPTIONS: u32 = 0x4200;
/// Get event message.
pub const PTRACE_GETEVENTMSG: u32 = 0x4201;
/// Get signal info.
pub const PTRACE_GETSIGINFO: u32 = 0x4202;
/// Set signal info.
pub const PTRACE_SETSIGINFO: u32 = 0x4203;
/// Seize (modern attach, no SIGSTOP).
pub const PTRACE_SEIZE: u32 = 0x4206;
/// Interrupt seized process.
pub const PTRACE_INTERRUPT: u32 = 0x4207;
/// Listen (for group-stop).
pub const PTRACE_LISTEN: u32 = 0x4208;
/// Get syscall info.
pub const PTRACE_GET_SYSCALL_INFO: u32 = 0x420E;

// ---------------------------------------------------------------------------
// ptrace options (PTRACE_SETOPTIONS)
// ---------------------------------------------------------------------------

/// Report fork events.
pub const PTRACE_O_TRACEFORK: u32 = 1 << 1;
/// Report vfork events.
pub const PTRACE_O_TRACEVFORK: u32 = 1 << 2;
/// Report clone events.
pub const PTRACE_O_TRACECLONE: u32 = 1 << 3;
/// Report exec events.
pub const PTRACE_O_TRACEEXEC: u32 = 1 << 4;
/// Report vfork-done events.
pub const PTRACE_O_TRACEVFORKDONE: u32 = 1 << 5;
/// Report exit events.
pub const PTRACE_O_TRACEEXIT: u32 = 1 << 6;
/// Report seccomp events.
pub const PTRACE_O_TRACESECCOMP: u32 = 1 << 7;
/// Suspend tracer's seccomp filter on tracee.
pub const PTRACE_O_SUSPEND_SECCOMP: u32 = 1 << 21;

// ---------------------------------------------------------------------------
// ptrace event codes (in wait status >> 16)
// ---------------------------------------------------------------------------

/// Fork event.
pub const PTRACE_EVENT_FORK: u32 = 1;
/// Vfork event.
pub const PTRACE_EVENT_VFORK: u32 = 2;
/// Clone event.
pub const PTRACE_EVENT_CLONE: u32 = 3;
/// Exec event.
pub const PTRACE_EVENT_EXEC: u32 = 4;
/// Vfork done event.
pub const PTRACE_EVENT_VFORK_DONE: u32 = 5;
/// Exit event.
pub const PTRACE_EVENT_EXIT: u32 = 6;
/// Seccomp event.
pub const PTRACE_EVENT_SECCOMP: u32 = 7;
/// Stop event.
pub const PTRACE_EVENT_STOP: u32 = 128;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_requests_distinct() {
        let reqs = [
            PTRACE_TRACEME,
            PTRACE_PEEKDATA,
            PTRACE_PEEKUSER,
            PTRACE_POKEDATA,
            PTRACE_POKEUSER,
            PTRACE_CONT,
            PTRACE_KILL,
            PTRACE_SINGLESTEP,
            PTRACE_ATTACH,
            PTRACE_DETACH,
            PTRACE_SYSCALL,
            PTRACE_SETOPTIONS,
            PTRACE_GETEVENTMSG,
            PTRACE_GETSIGINFO,
            PTRACE_SETSIGINFO,
            PTRACE_GETREGSET,
            PTRACE_SETREGSET,
            PTRACE_SEIZE,
            PTRACE_INTERRUPT,
            PTRACE_LISTEN,
            PTRACE_GET_SYSCALL_INFO,
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }

    #[test]
    fn test_options_no_overlap() {
        let opts = [
            PTRACE_O_TRACEFORK,
            PTRACE_O_TRACEVFORK,
            PTRACE_O_TRACECLONE,
            PTRACE_O_TRACEEXEC,
            PTRACE_O_TRACEVFORKDONE,
            PTRACE_O_TRACEEXIT,
            PTRACE_O_TRACESECCOMP,
            PTRACE_O_SUSPEND_SECCOMP,
        ];
        for i in 0..opts.len() {
            assert!(opts[i].is_power_of_two());
            for j in (i + 1)..opts.len() {
                assert_eq!(opts[i] & opts[j], 0);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            PTRACE_EVENT_FORK,
            PTRACE_EVENT_VFORK,
            PTRACE_EVENT_CLONE,
            PTRACE_EVENT_EXEC,
            PTRACE_EVENT_VFORK_DONE,
            PTRACE_EVENT_EXIT,
            PTRACE_EVENT_SECCOMP,
            PTRACE_EVENT_STOP,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}
