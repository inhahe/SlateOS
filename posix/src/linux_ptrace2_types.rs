//! `<linux/ptrace.h>` — Additional ptrace constants.
//!
//! Supplementary ptrace constants covering request types,
//! options, event codes, and register categories.

// ---------------------------------------------------------------------------
// Ptrace request types
// ---------------------------------------------------------------------------

/// Trace me.
pub const PTRACE_TRACEME: u32 = 0;
/// Peek text.
pub const PTRACE_PEEKTEXT: u32 = 1;
/// Peek data.
pub const PTRACE_PEEKDATA: u32 = 2;
/// Peek user.
pub const PTRACE_PEEKUSER: u32 = 3;
/// Poke text.
pub const PTRACE_POKETEXT: u32 = 4;
/// Poke data.
pub const PTRACE_POKEDATA: u32 = 5;
/// Poke user.
pub const PTRACE_POKEUSER: u32 = 6;
/// Continue.
pub const PTRACE_CONT: u32 = 7;
/// Kill.
pub const PTRACE_KILL: u32 = 8;
/// Single step.
pub const PTRACE_SINGLESTEP: u32 = 9;
/// Get registers.
pub const PTRACE_GETREGS: u32 = 12;
/// Set registers.
pub const PTRACE_SETREGS: u32 = 13;
/// Get FP registers.
pub const PTRACE_GETFPREGS: u32 = 14;
/// Set FP registers.
pub const PTRACE_SETFPREGS: u32 = 15;
/// Attach.
pub const PTRACE_ATTACH: u32 = 16;
/// Detach.
pub const PTRACE_DETACH: u32 = 17;
/// Syscall tracing.
pub const PTRACE_SYSCALL: u32 = 24;
/// Set options.
pub const PTRACE_SETOPTIONS: u32 = 0x4200;
/// Get event message.
pub const PTRACE_GETEVENTMSG: u32 = 0x4201;
/// Get signal info.
pub const PTRACE_GETSIGINFO: u32 = 0x4202;
/// Set signal info.
pub const PTRACE_SETSIGINFO: u32 = 0x4203;
/// Get register set.
pub const PTRACE_GETREGSET: u32 = 0x4204;
/// Set register set.
pub const PTRACE_SETREGSET: u32 = 0x4205;
/// Seize (modern attach).
pub const PTRACE_SEIZE: u32 = 0x4206;
/// Interrupt.
pub const PTRACE_INTERRUPT: u32 = 0x4207;
/// Listen.
pub const PTRACE_LISTEN: u32 = 0x4208;
/// Peek signal info.
pub const PTRACE_PEEKSIGINFO: u32 = 0x4209;
/// Get signal mask.
pub const PTRACE_GETSIGMASK: u32 = 0x420A;
/// Set signal mask.
pub const PTRACE_SETSIGMASK: u32 = 0x420B;
/// Seccomp get filter.
pub const PTRACE_SECCOMP_GET_FILTER: u32 = 0x420C;
/// Get syscall info.
pub const PTRACE_GET_SYSCALL_INFO: u32 = 0x420E;

// ---------------------------------------------------------------------------
// Ptrace options (PTRACE_O_*)
// ---------------------------------------------------------------------------

/// Trace sys-good.
pub const PTRACE_O_TRACESYSGOOD: u32 = 1;
/// Trace fork.
pub const PTRACE_O_TRACEFORK: u32 = 2;
/// Trace vfork.
pub const PTRACE_O_TRACEVFORK: u32 = 4;
/// Trace clone.
pub const PTRACE_O_TRACECLONE: u32 = 8;
/// Trace exec.
pub const PTRACE_O_TRACEEXEC: u32 = 16;
/// Trace vfork done.
pub const PTRACE_O_TRACEVFORKDONE: u32 = 32;
/// Trace exit.
pub const PTRACE_O_TRACEEXIT: u32 = 64;
/// Trace seccomp.
pub const PTRACE_O_TRACESECCOMP: u32 = 128;
/// Exit kill.
pub const PTRACE_O_EXITKILL: u32 = 1 << 20;
/// Suspend seccomp.
pub const PTRACE_O_SUSPEND_SECCOMP: u32 = 1 << 21;

// ---------------------------------------------------------------------------
// Ptrace event codes
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
            PTRACE_PEEKTEXT,
            PTRACE_PEEKDATA,
            PTRACE_PEEKUSER,
            PTRACE_POKETEXT,
            PTRACE_POKEDATA,
            PTRACE_POKEUSER,
            PTRACE_CONT,
            PTRACE_KILL,
            PTRACE_SINGLESTEP,
            PTRACE_GETREGS,
            PTRACE_SETREGS,
            PTRACE_GETFPREGS,
            PTRACE_SETFPREGS,
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
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }

    #[test]
    fn test_options_power_of_two() {
        let opts = [
            PTRACE_O_TRACESYSGOOD,
            PTRACE_O_TRACEFORK,
            PTRACE_O_TRACEVFORK,
            PTRACE_O_TRACECLONE,
            PTRACE_O_TRACEEXEC,
            PTRACE_O_TRACEVFORKDONE,
            PTRACE_O_TRACEEXIT,
            PTRACE_O_TRACESECCOMP,
            PTRACE_O_EXITKILL,
            PTRACE_O_SUSPEND_SECCOMP,
        ];
        for o in &opts {
            assert!(o.is_power_of_two(), "0x{:08x} not power of two", o);
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

    #[test]
    fn test_traceme_zero() {
        assert_eq!(PTRACE_TRACEME, 0);
    }
}
