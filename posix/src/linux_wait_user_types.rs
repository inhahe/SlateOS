//! `<sys/wait.h>` — `wait*`/`waitid` and the status-word ABI.
//!
//! After `fork()`, the parent reaps children with `wait/waitpid/waitid`.
//! The status word is bitfield-packed: low byte = exit code, next byte
//! = signal, top bit = core dump. `waitid(2)` is the modern replacement
//! that returns `siginfo_t` directly and supports `P_PIDFD`.

// ---------------------------------------------------------------------------
// `waitpid`/`wait4` `options`
// ---------------------------------------------------------------------------

pub const WNOHANG: u32 = 0x0000_0001;
pub const WUNTRACED: u32 = 0x0000_0002;
pub const WSTOPPED: u32 = WUNTRACED;
pub const WEXITED: u32 = 0x0000_0004;
pub const WCONTINUED: u32 = 0x0000_0008;
pub const WNOWAIT: u32 = 0x0100_0000;

// ---------------------------------------------------------------------------
// Non-POSIX Linux extensions
// ---------------------------------------------------------------------------

pub const WNOTHREAD: u32 = 0x2000_0000;
pub const WALL: u32 = 0x4000_0000;
pub const WCLONE: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// `waitid(2)` `idtype_t` values
// ---------------------------------------------------------------------------

pub const P_ALL: u32 = 0;
pub const P_PID: u32 = 1;
pub const P_PGID: u32 = 2;
pub const P_PIDFD: u32 = 3;

// ---------------------------------------------------------------------------
// `siginfo_t.si_code` values for `SIGCHLD`
// ---------------------------------------------------------------------------

pub const CLD_EXITED: u32 = 1;
pub const CLD_KILLED: u32 = 2;
pub const CLD_DUMPED: u32 = 3;
pub const CLD_TRAPPED: u32 = 4;
pub const CLD_STOPPED: u32 = 5;
pub const CLD_CONTINUED: u32 = 6;

// ---------------------------------------------------------------------------
// Status-word encoding (used by `WIFEXITED`/`WEXITSTATUS` macros)
// ---------------------------------------------------------------------------

/// Mask covering the exit-code byte (low 8 bits, shifted up).
pub const WAIT_EXIT_CODE_MASK: u32 = 0xFF00;
/// Bit shift to extract `WEXITSTATUS`.
pub const WAIT_EXIT_CODE_SHIFT: u32 = 8;
/// Mask for the terminating-signal byte.
pub const WAIT_TERM_SIG_MASK: u32 = 0x007F;
/// Set when the child dumped core.
pub const WAIT_CORE_DUMP_BIT: u32 = 0x0080;
/// Marker value indicating the child was stopped (low byte == 0x7F).
pub const WAIT_STOPPED_MARKER: u32 = 0x007F;
/// Marker value for continued (whole-word check, glibc-internal).
pub const WAIT_CONTINUED_MARKER: u32 = 0xFFFF;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_WAIT4: u32 = 61;
pub const NR_WAITID: u32 = 247;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_options_dense_low_4_bits() {
        // WNOHANG/WUNTRACED/WEXITED/WCONTINUED occupy bits 0..3.
        assert_eq!(WNOHANG, 1);
        assert_eq!(WUNTRACED, 2);
        assert_eq!(WSTOPPED, WUNTRACED);
        assert_eq!(WEXITED, 4);
        assert_eq!(WCONTINUED, 8);
    }

    #[test]
    fn test_high_bit_linux_options() {
        // Three Linux extensions live in the top byte.
        assert_eq!(WNOTHREAD, 1 << 29);
        assert_eq!(WALL, 1 << 30);
        assert_eq!(WCLONE, 1 << 31);
        // WNOWAIT is bit 24.
        assert_eq!(WNOWAIT, 1 << 24);
    }

    #[test]
    fn test_idtype_dense_0_to_3() {
        let i = [P_ALL, P_PID, P_PGID, P_PIDFD];
        for (idx, &v) in i.iter().enumerate() {
            assert_eq!(v as usize, idx);
        }
    }

    #[test]
    fn test_cld_codes_dense_1_to_6() {
        let c = [CLD_EXITED, CLD_KILLED, CLD_DUMPED, CLD_TRAPPED, CLD_STOPPED, CLD_CONTINUED];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_status_layout_consistent() {
        // The signal byte is bits 0..6; the core-dump flag is bit 7.
        assert_eq!(WAIT_TERM_SIG_MASK | WAIT_CORE_DUMP_BIT, 0xFF);
        assert_eq!(WAIT_TERM_SIG_MASK & WAIT_CORE_DUMP_BIT, 0);
        // The exit-code byte is bits 8..15.
        assert_eq!(WAIT_EXIT_CODE_MASK, 0xFF00);
        assert_eq!(WAIT_EXIT_CODE_SHIFT, 8);
        // Stopped marker is exactly 0x7F in the low byte (so the
        // WIFSTOPPED macro looks at status & 0xFF == 0x7F).
        assert_eq!(WAIT_STOPPED_MARKER, 0x7F);
        assert_eq!(WAIT_CONTINUED_MARKER, 0xFFFF);
    }

    #[test]
    fn test_syscall_numbers() {
        assert_eq!(NR_WAIT4, 61);
        assert_eq!(NR_WAITID, 247);
    }
}
