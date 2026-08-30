//! `<sys/ptrace.h>` — Ptrace register access and watchpoint constants.
//!
//! These constants are used with `ptrace()` to read/write registers,
//! set hardware breakpoints/watchpoints, and access thread-specific
//! state on x86_64.

// ---------------------------------------------------------------------------
// Register offsets in user_regs_struct (x86_64)
// ---------------------------------------------------------------------------

/// Offset of R15 in user_regs_struct.
pub const PTRACE_REG_R15: u32 = 0;
/// Offset of R14.
pub const PTRACE_REG_R14: u32 = 8;
/// Offset of R13.
pub const PTRACE_REG_R13: u32 = 16;
/// Offset of R12.
pub const PTRACE_REG_R12: u32 = 24;
/// Offset of RBP.
pub const PTRACE_REG_RBP: u32 = 32;
/// Offset of RBX.
pub const PTRACE_REG_RBX: u32 = 40;
/// Offset of R11.
pub const PTRACE_REG_R11: u32 = 48;
/// Offset of R10.
pub const PTRACE_REG_R10: u32 = 56;
/// Offset of R9.
pub const PTRACE_REG_R9: u32 = 64;
/// Offset of R8.
pub const PTRACE_REG_R8: u32 = 72;
/// Offset of RAX.
pub const PTRACE_REG_RAX: u32 = 80;
/// Offset of RCX.
pub const PTRACE_REG_RCX: u32 = 88;
/// Offset of RDX.
pub const PTRACE_REG_RDX: u32 = 96;
/// Offset of RSI.
pub const PTRACE_REG_RSI: u32 = 104;
/// Offset of RDI.
pub const PTRACE_REG_RDI: u32 = 112;
/// Offset of orig_rax (syscall number).
pub const PTRACE_REG_ORIG_RAX: u32 = 120;
/// Offset of RIP (instruction pointer).
pub const PTRACE_REG_RIP: u32 = 128;
/// Offset of CS (code segment).
pub const PTRACE_REG_CS: u32 = 136;
/// Offset of EFLAGS.
pub const PTRACE_REG_EFLAGS: u32 = 144;
/// Offset of RSP (stack pointer).
pub const PTRACE_REG_RSP: u32 = 152;
/// Offset of SS (stack segment).
pub const PTRACE_REG_SS: u32 = 160;

// ---------------------------------------------------------------------------
// Hardware breakpoint/watchpoint types (DR7 condition field)
// ---------------------------------------------------------------------------

/// Break on execution.
pub const HW_BREAKPOINT_EXECUTE: u32 = 0;
/// Break on write.
pub const HW_BREAKPOINT_WRITE: u32 = 1;
/// Break on I/O (not commonly used).
pub const HW_BREAKPOINT_IO: u32 = 2;
/// Break on read or write.
pub const HW_BREAKPOINT_RW: u32 = 3;

// ---------------------------------------------------------------------------
// Hardware breakpoint length values (DR7 length field)
// ---------------------------------------------------------------------------

/// 1-byte watchpoint.
pub const HW_BREAKPOINT_LEN_1: u32 = 1;
/// 2-byte watchpoint.
pub const HW_BREAKPOINT_LEN_2: u32 = 2;
/// 4-byte watchpoint.
pub const HW_BREAKPOINT_LEN_4: u32 = 4;
/// 8-byte watchpoint (x86_64 only).
pub const HW_BREAKPOINT_LEN_8: u32 = 8;

/// Maximum number of hardware breakpoints on x86_64.
pub const HW_BREAKPOINT_NUM: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reg_offsets_sequential() {
        assert_eq!(PTRACE_REG_R15, 0);
        assert_eq!(PTRACE_REG_R14, 8);
        assert_eq!(PTRACE_REG_R13, 16);
        assert_eq!(PTRACE_REG_SS, 160);
    }

    #[test]
    fn test_reg_offsets_stride() {
        let offsets = [
            PTRACE_REG_R15,
            PTRACE_REG_R14,
            PTRACE_REG_R13,
            PTRACE_REG_R12,
            PTRACE_REG_RBP,
            PTRACE_REG_RBX,
            PTRACE_REG_R11,
            PTRACE_REG_R10,
            PTRACE_REG_R9,
            PTRACE_REG_R8,
            PTRACE_REG_RAX,
            PTRACE_REG_RCX,
            PTRACE_REG_RDX,
            PTRACE_REG_RSI,
            PTRACE_REG_RDI,
            PTRACE_REG_ORIG_RAX,
            PTRACE_REG_RIP,
            PTRACE_REG_CS,
            PTRACE_REG_EFLAGS,
            PTRACE_REG_RSP,
            PTRACE_REG_SS,
        ];
        for i in 1..offsets.len() {
            assert_eq!(offsets[i] - offsets[i - 1], 8);
        }
    }

    #[test]
    fn test_hw_bp_types_distinct() {
        let types = [
            HW_BREAKPOINT_EXECUTE,
            HW_BREAKPOINT_WRITE,
            HW_BREAKPOINT_IO,
            HW_BREAKPOINT_RW,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_hw_bp_lengths_power_of_two() {
        assert!(HW_BREAKPOINT_LEN_1.is_power_of_two());
        assert!(HW_BREAKPOINT_LEN_2.is_power_of_two());
        assert!(HW_BREAKPOINT_LEN_4.is_power_of_two());
        assert!(HW_BREAKPOINT_LEN_8.is_power_of_two());
    }

    #[test]
    fn test_hw_breakpoint_num() {
        assert_eq!(HW_BREAKPOINT_NUM, 4);
    }
}
