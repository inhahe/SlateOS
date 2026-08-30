//! `<signal.h>` — Signal info (siginfo_t) code constants.
//!
//! When a signal is delivered with SA_SIGINFO, the kernel fills a
//! `siginfo_t` structure with details about the signal source. The
//! `si_code` field identifies why the signal was sent: by the kernel,
//! a user process, a timer, or hardware.

// ---------------------------------------------------------------------------
// Generic si_code values (negative = from user, positive = from kernel)
// ---------------------------------------------------------------------------

/// Sent by kill(), sigsend(), or raise().
pub const SI_USER: i32 = 0;
/// Sent by the kernel.
pub const SI_KERNEL: i32 = 0x80;
/// Sent by sigqueue() with a value.
pub const SI_QUEUE: i32 = -1;
/// POSIX timer expired.
pub const SI_TIMER: i32 = -2;
/// POSIX message queue state changed.
pub const SI_MESGQ: i32 = -3;
/// Async I/O completed.
pub const SI_ASYNCIO: i32 = -4;
/// Sent by tkill() or tgkill().
pub const SI_TKILL: i32 = -6;

// ---------------------------------------------------------------------------
// SIGILL si_code values
// ---------------------------------------------------------------------------

/// Illegal opcode.
pub const ILL_ILLOPC: i32 = 1;
/// Illegal operand.
pub const ILL_ILLOPN: i32 = 2;
/// Illegal addressing mode.
pub const ILL_ILLADR: i32 = 3;
/// Illegal trap.
pub const ILL_ILLTRP: i32 = 4;
/// Privileged opcode.
pub const ILL_PRVOPC: i32 = 5;
/// Coprocessor error.
pub const ILL_COPROC: i32 = 7;

// ---------------------------------------------------------------------------
// SIGSEGV si_code values
// ---------------------------------------------------------------------------

/// Address not mapped to object.
pub const SEGV_MAPERR: i32 = 1;
/// Invalid permissions for mapped object.
pub const SEGV_ACCERR: i32 = 2;
/// Failed address bound checks.
pub const SEGV_BNDERR: i32 = 3;
/// Protection key violation.
pub const SEGV_PKUERR: i32 = 4;

// ---------------------------------------------------------------------------
// SIGBUS si_code values
// ---------------------------------------------------------------------------

/// Invalid address alignment.
pub const BUS_ADRALN: i32 = 1;
/// Non-existent physical address.
pub const BUS_ADRERR: i32 = 2;
/// Object-specific hardware error.
pub const BUS_OBJERR: i32 = 3;
/// Hardware memory error (machine check).
pub const BUS_MCEERR_AR: i32 = 4;
/// Hardware memory error (deferred).
pub const BUS_MCEERR_AO: i32 = 5;

// ---------------------------------------------------------------------------
// SIGFPE si_code values
// ---------------------------------------------------------------------------

/// Integer divide by zero.
pub const FPE_INTDIV: i32 = 1;
/// Integer overflow.
pub const FPE_INTOVF: i32 = 2;
/// Floating-point divide by zero.
pub const FPE_FLTDIV: i32 = 3;
/// Floating-point overflow.
pub const FPE_FLTOVF: i32 = 4;
/// Floating-point underflow.
pub const FPE_FLTUND: i32 = 5;
/// Floating-point inexact result.
pub const FPE_FLTRES: i32 = 6;
/// Invalid floating-point operation.
pub const FPE_FLTINV: i32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generic_codes_distinct() {
        let codes = [
            SI_USER, SI_KERNEL, SI_QUEUE, SI_TIMER, SI_MESGQ, SI_ASYNCIO, SI_TKILL,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_user_is_zero() {
        assert_eq!(SI_USER, 0);
    }

    #[test]
    fn test_segv_codes_distinct() {
        let codes = [SEGV_MAPERR, SEGV_ACCERR, SEGV_BNDERR, SEGV_PKUERR];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_fpe_codes_distinct() {
        let codes = [
            FPE_INTDIV, FPE_INTOVF, FPE_FLTDIV, FPE_FLTOVF, FPE_FLTUND, FPE_FLTRES, FPE_FLTINV,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_bus_codes_distinct() {
        let codes = [
            BUS_ADRALN,
            BUS_ADRERR,
            BUS_OBJERR,
            BUS_MCEERR_AR,
            BUS_MCEERR_AO,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_kernel_code() {
        assert_eq!(SI_KERNEL, 0x80);
    }
}
