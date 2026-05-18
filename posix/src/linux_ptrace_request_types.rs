//! `<linux/ptrace.h>` — ptrace request type constants.
//!
//! The ptrace system call allows a tracer process to observe and
//! control another (tracee). Request types specify the operation:
//! reading/writing registers and memory, single-stepping,
//! continuing, and managing syscall tracing.

// ---------------------------------------------------------------------------
// ptrace request codes (x86_64)
// ---------------------------------------------------------------------------

/// Attach to a process (become tracer).
pub const PTRACE_ATTACH: u32 = 16;
/// Detach from tracee (resume it).
pub const PTRACE_DETACH: u32 = 17;
/// Continue execution.
pub const PTRACE_CONT: u32 = 7;
/// Single-step one instruction.
pub const PTRACE_SINGLESTEP: u32 = 9;
/// Get general-purpose registers.
pub const PTRACE_GETREGS: u32 = 12;
/// Set general-purpose registers.
pub const PTRACE_SETREGS: u32 = 13;
/// Get floating-point registers.
pub const PTRACE_GETFPREGS: u32 = 14;
/// Set floating-point registers.
pub const PTRACE_SETFPREGS: u32 = 15;
/// Read word from tracee memory.
pub const PTRACE_PEEKDATA: u32 = 2;
/// Write word to tracee memory.
pub const PTRACE_POKEDATA: u32 = 5;
/// Read word from tracee user area.
pub const PTRACE_PEEKUSER: u32 = 3;
/// Write word to tracee user area.
pub const PTRACE_POKEUSER: u32 = 6;
/// Read word from tracee text segment.
pub const PTRACE_PEEKTEXT: u32 = 1;
/// Write word to tracee text segment.
pub const PTRACE_POKETEXT: u32 = 4;
/// Kill the tracee.
pub const PTRACE_KILL: u32 = 8;
/// Continue and stop at next syscall entry/exit.
pub const PTRACE_SYSCALL: u32 = 24;
/// Seize (modern attach, no SIGSTOP).
pub const PTRACE_SEIZE: u32 = 0x4206;
/// Interrupt a seized tracee.
pub const PTRACE_INTERRUPT: u32 = 0x4207;
/// Listen (for group-stop notifications).
pub const PTRACE_LISTEN: u32 = 0x4208;
/// Get register set (by NT_* type).
pub const PTRACE_GETREGSET: u32 = 0x4204;
/// Set register set (by NT_* type).
pub const PTRACE_SETREGSET: u32 = 0x4205;
/// Get signal info for pending signal.
pub const PTRACE_GETSIGINFO: u32 = 0x4202;
/// Set signal info for delivery.
pub const PTRACE_SETSIGINFO: u32 = 0x4203;

// ---------------------------------------------------------------------------
// ptrace options (set via PTRACE_SETOPTIONS)
// ---------------------------------------------------------------------------

/// PTRACE_SETOPTIONS request code.
pub const PTRACE_SETOPTIONS: u32 = 0x4200;

/// Trace clone events.
pub const PTRACE_O_TRACECLONE: u32 = 1 << 3;
/// Trace exec events.
pub const PTRACE_O_TRACEEXEC: u32 = 1 << 4;
/// Trace exit events.
pub const PTRACE_O_TRACEEXIT: u32 = 1 << 6;
/// Trace fork events.
pub const PTRACE_O_TRACEFORK: u32 = 1 << 1;
/// Trace vfork events.
pub const PTRACE_O_TRACEVFORK: u32 = 1 << 2;
/// Trace vfork-done events.
pub const PTRACE_O_TRACEVFORKDONE: u32 = 1 << 5;
/// Trace seccomp events.
pub const PTRACE_O_TRACESECCOMP: u32 = 1 << 7;
/// Suspend seccomp filtering.
pub const PTRACE_O_SUSPEND_SECCOMP: u32 = 1 << 21;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_requests_distinct() {
        let reqs = [
            PTRACE_ATTACH, PTRACE_DETACH, PTRACE_CONT,
            PTRACE_SINGLESTEP, PTRACE_GETREGS, PTRACE_SETREGS,
            PTRACE_GETFPREGS, PTRACE_SETFPREGS, PTRACE_PEEKDATA,
            PTRACE_POKEDATA, PTRACE_PEEKUSER, PTRACE_POKEUSER,
            PTRACE_PEEKTEXT, PTRACE_POKETEXT, PTRACE_KILL,
            PTRACE_SYSCALL,
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }

    #[test]
    fn test_modern_requests_distinct() {
        let reqs = [
            PTRACE_SEIZE, PTRACE_INTERRUPT, PTRACE_LISTEN,
            PTRACE_GETREGSET, PTRACE_SETREGSET,
            PTRACE_GETSIGINFO, PTRACE_SETSIGINFO,
            PTRACE_SETOPTIONS,
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
            PTRACE_O_TRACECLONE, PTRACE_O_TRACEEXEC,
            PTRACE_O_TRACEEXIT, PTRACE_O_TRACEFORK,
            PTRACE_O_TRACEVFORK, PTRACE_O_TRACEVFORKDONE,
            PTRACE_O_TRACESECCOMP, PTRACE_O_SUSPEND_SECCOMP,
        ];
        for i in 0..opts.len() {
            assert!(opts[i].is_power_of_two());
            for j in (i + 1)..opts.len() {
                assert_eq!(opts[i] & opts[j], 0);
            }
        }
    }
}
