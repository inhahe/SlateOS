//! `<linux/ptrace.h>` — ptrace constants (kernel view).
//!
//! Re-exports from `sys_ptrace` and adds Linux-specific ptrace
//! options and event codes.

// ---------------------------------------------------------------------------
// Re-exports from sys_ptrace
// ---------------------------------------------------------------------------

pub use crate::sys_ptrace::PTRACE_ATTACH;
pub use crate::sys_ptrace::PTRACE_CONT;
pub use crate::sys_ptrace::PTRACE_DETACH;
pub use crate::sys_ptrace::PTRACE_GETEVENTMSG;
pub use crate::sys_ptrace::PTRACE_GETFPREGS;
pub use crate::sys_ptrace::PTRACE_GETREGS;
pub use crate::sys_ptrace::PTRACE_GETREGSET;
pub use crate::sys_ptrace::PTRACE_GETSIGINFO;
pub use crate::sys_ptrace::PTRACE_INTERRUPT;
pub use crate::sys_ptrace::PTRACE_KILL;
pub use crate::sys_ptrace::PTRACE_LISTEN;
pub use crate::sys_ptrace::PTRACE_PEEKDATA;
pub use crate::sys_ptrace::PTRACE_PEEKTEXT;
pub use crate::sys_ptrace::PTRACE_PEEKUSER;
pub use crate::sys_ptrace::PTRACE_POKEDATA;
pub use crate::sys_ptrace::PTRACE_POKETEXT;
pub use crate::sys_ptrace::PTRACE_POKEUSER;
pub use crate::sys_ptrace::PTRACE_SEIZE;
pub use crate::sys_ptrace::PTRACE_SETFPREGS;
pub use crate::sys_ptrace::PTRACE_SETOPTIONS;
pub use crate::sys_ptrace::PTRACE_SETREGS;
pub use crate::sys_ptrace::PTRACE_SETREGSET;
pub use crate::sys_ptrace::PTRACE_SETSIGINFO;
pub use crate::sys_ptrace::PTRACE_SINGLESTEP;
pub use crate::sys_ptrace::PTRACE_SYSCALL;
pub use crate::sys_ptrace::PTRACE_TRACEME;
pub use crate::sys_ptrace::ptrace;

// ---------------------------------------------------------------------------
// Linux-specific constants (not in sys_ptrace)
// ---------------------------------------------------------------------------

/// Trace-sysgood option (deliver SIGTRAP|0x80 on syscall stops).
pub const PTRACE_O_TRACESYSGOOD: i32 = 0x01;

/// Get seccomp filter (Linux 4.4+).
pub const PTRACE_SECCOMP_GET_FILTER: i32 = 0x420C;

/// Get syscall info (Linux 5.3+).
pub const PTRACE_GET_SYSCALL_INFO: i32 = 0x420E;

// Re-export options
pub use crate::sys_ptrace::PTRACE_O_EXITKILL;
pub use crate::sys_ptrace::PTRACE_O_SUSPEND_SECCOMP;
pub use crate::sys_ptrace::PTRACE_O_TRACECLONE;
pub use crate::sys_ptrace::PTRACE_O_TRACEEXEC;
pub use crate::sys_ptrace::PTRACE_O_TRACEEXIT;
pub use crate::sys_ptrace::PTRACE_O_TRACEFORK;
pub use crate::sys_ptrace::PTRACE_O_TRACESECCOMP;
pub use crate::sys_ptrace::PTRACE_O_TRACEVFORK;
pub use crate::sys_ptrace::PTRACE_O_TRACEVFORKDONE;

// Re-export events
pub use crate::sys_ptrace::PTRACE_EVENT_CLONE;
pub use crate::sys_ptrace::PTRACE_EVENT_EXEC;
pub use crate::sys_ptrace::PTRACE_EVENT_EXIT;
pub use crate::sys_ptrace::PTRACE_EVENT_FORK;
pub use crate::sys_ptrace::PTRACE_EVENT_SECCOMP;
pub use crate::sys_ptrace::PTRACE_EVENT_STOP;
pub use crate::sys_ptrace::PTRACE_EVENT_VFORK;
pub use crate::sys_ptrace::PTRACE_EVENT_VFORK_DONE;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ptrace_values() {
        assert_eq!(PTRACE_TRACEME, 0);
        assert_eq!(PTRACE_PEEKTEXT, 1);
        assert_eq!(PTRACE_ATTACH, 16);
        assert_eq!(PTRACE_DETACH, 17);
    }

    #[test]
    fn test_options_distinct() {
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
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(PTRACE_TRACEME, crate::sys_ptrace::PTRACE_TRACEME);
        assert_eq!(PTRACE_ATTACH, crate::sys_ptrace::PTRACE_ATTACH);
        assert_eq!(PTRACE_O_TRACEFORK, crate::sys_ptrace::PTRACE_O_TRACEFORK);
    }
}
