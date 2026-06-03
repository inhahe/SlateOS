//! `<sys/ptrace.h>` — `ptrace(2)` request codes.
//!
//! `ptrace` is the foundation of `gdb`, `strace`, `rr`, and CRIU.
//! The request enum below covers the portable core; arch-specific
//! requests (`PTRACE_GETREGSET`/`SETREGSET` with NT_* notes) sit on
//! top of these but are out of scope here.

// ---------------------------------------------------------------------------
// Core ptrace requests (`enum __ptrace_request`)
// ---------------------------------------------------------------------------

pub const PTRACE_TRACEME: u32 = 0;
pub const PTRACE_PEEKTEXT: u32 = 1;
pub const PTRACE_PEEKDATA: u32 = 2;
pub const PTRACE_PEEKUSER: u32 = 3;
pub const PTRACE_POKETEXT: u32 = 4;
pub const PTRACE_POKEDATA: u32 = 5;
pub const PTRACE_POKEUSER: u32 = 6;
pub const PTRACE_CONT: u32 = 7;
pub const PTRACE_KILL: u32 = 8;
pub const PTRACE_SINGLESTEP: u32 = 9;
pub const PTRACE_GETREGS: u32 = 12;
pub const PTRACE_SETREGS: u32 = 13;
pub const PTRACE_GETFPREGS: u32 = 14;
pub const PTRACE_SETFPREGS: u32 = 15;
pub const PTRACE_ATTACH: u32 = 16;
pub const PTRACE_DETACH: u32 = 17;
pub const PTRACE_GETFPXREGS: u32 = 18;
pub const PTRACE_SETFPXREGS: u32 = 19;
pub const PTRACE_SYSCALL: u32 = 24;
pub const PTRACE_GET_THREAD_AREA: u32 = 25;
pub const PTRACE_SET_THREAD_AREA: u32 = 26;
pub const PTRACE_ARCH_PRCTL: u32 = 30;

// ---------------------------------------------------------------------------
// Extended requests
// ---------------------------------------------------------------------------

pub const PTRACE_SETOPTIONS: u32 = 0x4200;
pub const PTRACE_GETEVENTMSG: u32 = 0x4201;
pub const PTRACE_GETSIGINFO: u32 = 0x4202;
pub const PTRACE_SETSIGINFO: u32 = 0x4203;
pub const PTRACE_GETREGSET: u32 = 0x4204;
pub const PTRACE_SETREGSET: u32 = 0x4205;
pub const PTRACE_SEIZE: u32 = 0x4206;
pub const PTRACE_INTERRUPT: u32 = 0x4207;
pub const PTRACE_LISTEN: u32 = 0x4208;
pub const PTRACE_PEEKSIGINFO: u32 = 0x4209;
pub const PTRACE_GETSIGMASK: u32 = 0x420A;
pub const PTRACE_SETSIGMASK: u32 = 0x420B;
pub const PTRACE_SECCOMP_GET_FILTER: u32 = 0x420C;
pub const PTRACE_SECCOMP_GET_METADATA: u32 = 0x420D;
pub const PTRACE_GET_SYSCALL_INFO: u32 = 0x420E;
pub const PTRACE_GET_RSEQ_CONFIGURATION: u32 = 0x420F;
pub const PTRACE_SET_SYSCALL_USER_DISPATCH_CONFIG: u32 = 0x4210;
pub const PTRACE_GET_SYSCALL_USER_DISPATCH_CONFIG: u32 = 0x4211;

// ---------------------------------------------------------------------------
// `PTRACE_SETOPTIONS` flags
// ---------------------------------------------------------------------------

pub const PTRACE_O_TRACESYSGOOD: u32 = 1 << 0;
pub const PTRACE_O_TRACEFORK: u32 = 1 << 1;
pub const PTRACE_O_TRACEVFORK: u32 = 1 << 2;
pub const PTRACE_O_TRACECLONE: u32 = 1 << 3;
pub const PTRACE_O_TRACEEXEC: u32 = 1 << 4;
pub const PTRACE_O_TRACEVFORKDONE: u32 = 1 << 5;
pub const PTRACE_O_TRACEEXIT: u32 = 1 << 6;
pub const PTRACE_O_TRACESECCOMP: u32 = 1 << 7;
pub const PTRACE_O_EXITKILL: u32 = 1 << 20;
pub const PTRACE_O_SUSPEND_SECCOMP: u32 = 1 << 21;

// ---------------------------------------------------------------------------
// Wait-status event codes (`PTRACE_EVENT_*`)
// ---------------------------------------------------------------------------

pub const PTRACE_EVENT_FORK: u32 = 1;
pub const PTRACE_EVENT_VFORK: u32 = 2;
pub const PTRACE_EVENT_CLONE: u32 = 3;
pub const PTRACE_EVENT_EXEC: u32 = 4;
pub const PTRACE_EVENT_VFORK_DONE: u32 = 5;
pub const PTRACE_EVENT_EXIT: u32 = 6;
pub const PTRACE_EVENT_SECCOMP: u32 = 7;
pub const PTRACE_EVENT_STOP: u32 = 128;

// ---------------------------------------------------------------------------
// Syscall
// ---------------------------------------------------------------------------

pub const NR_PTRACE: u32 = 101;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_traceme_zero() {
        // PTRACE_TRACEME is universally 0 across every libc.
        assert_eq!(PTRACE_TRACEME, 0);
    }

    #[test]
    fn test_peek_poke_pairs() {
        // PEEK/POKE pairs occupy adjacent op numbers in TEXT/DATA/USER order.
        assert_eq!(PTRACE_PEEKTEXT, 1);
        assert_eq!(PTRACE_PEEKDATA, 2);
        assert_eq!(PTRACE_PEEKUSER, 3);
        assert_eq!(PTRACE_POKETEXT, 4);
        assert_eq!(PTRACE_POKEDATA, 5);
        assert_eq!(PTRACE_POKEUSER, 6);
    }

    #[test]
    fn test_attach_detach() {
        // ATTACH/DETACH live at 16/17.
        assert_eq!(PTRACE_ATTACH, 16);
        assert_eq!(PTRACE_DETACH, 17);
        // Modern attach via SEIZE.
        assert_eq!(PTRACE_SEIZE, 0x4206);
    }

    #[test]
    fn test_extended_ops_in_0x420x_range() {
        let e = [
            PTRACE_SETOPTIONS,
            PTRACE_GETEVENTMSG,
            PTRACE_GETSIGINFO,
            PTRACE_SETSIGINFO,
            PTRACE_GETREGSET,
            PTRACE_SETREGSET,
            PTRACE_SEIZE,
            PTRACE_INTERRUPT,
            PTRACE_LISTEN,
            PTRACE_PEEKSIGINFO,
            PTRACE_GETSIGMASK,
            PTRACE_SETSIGMASK,
            PTRACE_SECCOMP_GET_FILTER,
            PTRACE_SECCOMP_GET_METADATA,
            PTRACE_GET_SYSCALL_INFO,
            PTRACE_GET_RSEQ_CONFIGURATION,
            PTRACE_SET_SYSCALL_USER_DISPATCH_CONFIG,
            PTRACE_GET_SYSCALL_USER_DISPATCH_CONFIG,
        ];
        for &v in e.iter() {
            // All extended op numbers live in 0x4200..=0x42FF.
            assert_eq!(v & 0xFF00, 0x4200);
        }
        // And run consecutively from 0x4200.
        for (i, &v) in e.iter().enumerate() {
            assert_eq!(v, 0x4200 + i as u32);
        }
    }

    #[test]
    fn test_options_low_8_bits_dense() {
        // The original eight TRACE_* options occupy bits 0..7.
        let o = [
            PTRACE_O_TRACESYSGOOD,
            PTRACE_O_TRACEFORK,
            PTRACE_O_TRACEVFORK,
            PTRACE_O_TRACECLONE,
            PTRACE_O_TRACEEXEC,
            PTRACE_O_TRACEVFORKDONE,
            PTRACE_O_TRACEEXIT,
            PTRACE_O_TRACESECCOMP,
        ];
        let mut or = 0u32;
        for (i, &v) in o.iter().enumerate() {
            assert!(v.is_power_of_two());
            assert_eq!(v, 1 << i);
            or |= v;
        }
        assert_eq!(or, 0xFF);
        // EXITKILL/SUSPEND_SECCOMP live up at bit 20/21.
        assert_eq!(PTRACE_O_EXITKILL, 1 << 20);
        assert_eq!(PTRACE_O_SUSPEND_SECCOMP, 1 << 21);
    }

    #[test]
    fn test_event_codes_distinct() {
        let e = [
            PTRACE_EVENT_FORK,
            PTRACE_EVENT_VFORK,
            PTRACE_EVENT_CLONE,
            PTRACE_EVENT_EXEC,
            PTRACE_EVENT_VFORK_DONE,
            PTRACE_EVENT_EXIT,
            PTRACE_EVENT_SECCOMP,
            PTRACE_EVENT_STOP,
        ];
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
        }
        // EVENT_STOP uses an out-of-range value (128) so it doesn't
        // collide with the dense 1..=7 set.
        assert_eq!(PTRACE_EVENT_STOP, 128);
    }

    #[test]
    fn test_syscall_number() {
        assert_eq!(NR_PTRACE, 101);
    }
}
