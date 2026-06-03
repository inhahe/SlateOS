//! `<linux/filter.h>` — BPF (Berkeley Packet Filter) socket filter.
//!
//! Provides `SockFilter` and `SockFprog` structs for classic BPF,
//! plus BPF instruction constants.

// ---------------------------------------------------------------------------
// BPF instruction classes
// ---------------------------------------------------------------------------

/// Load word.
pub const BPF_LD: u16 = 0x00;
/// Load doubleword.
pub const BPF_LDX: u16 = 0x01;
/// Store word.
pub const BPF_ST: u16 = 0x02;
/// Store doubleword.
pub const BPF_STX: u16 = 0x03;
/// ALU operation.
pub const BPF_ALU: u16 = 0x04;
/// Jump.
pub const BPF_JMP: u16 = 0x05;
/// Return.
pub const BPF_RET: u16 = 0x06;
/// Miscellaneous.
pub const BPF_MISC: u16 = 0x07;

// ---------------------------------------------------------------------------
// BPF operand sizes
// ---------------------------------------------------------------------------

/// Word (4 bytes).
pub const BPF_W: u16 = 0x00;
/// Half-word (2 bytes).
pub const BPF_H: u16 = 0x08;
/// Byte.
pub const BPF_B: u16 = 0x10;

// ---------------------------------------------------------------------------
// BPF addressing modes
// ---------------------------------------------------------------------------

/// Immediate value.
pub const BPF_IMM: u16 = 0x00;
/// Absolute offset.
pub const BPF_ABS: u16 = 0x20;
/// Indirect offset.
pub const BPF_IND: u16 = 0x40;
/// Scratch memory.
pub const BPF_MEM: u16 = 0x60;
/// Packet length.
pub const BPF_LEN: u16 = 0x80;
/// MSH (IP header length).
pub const BPF_MSH: u16 = 0xa0;

// ---------------------------------------------------------------------------
// BPF source operand
// ---------------------------------------------------------------------------

/// Constant (immediate) source.
pub const BPF_K: u16 = 0x00;
/// X register source.
pub const BPF_X: u16 = 0x08;
/// Accumulator source.
pub const BPF_A: u16 = 0x10;

// ---------------------------------------------------------------------------
// BPF ALU operations
// ---------------------------------------------------------------------------

/// Add.
pub const BPF_ADD: u16 = 0x00;
/// Subtract.
pub const BPF_SUB: u16 = 0x10;
/// Multiply.
pub const BPF_MUL: u16 = 0x20;
/// Divide.
pub const BPF_DIV: u16 = 0x30;
/// Bitwise OR.
pub const BPF_OR: u16 = 0x40;
/// Bitwise AND.
pub const BPF_AND: u16 = 0x50;
/// Left shift.
pub const BPF_LSH: u16 = 0x60;
/// Right shift.
pub const BPF_RSH: u16 = 0x70;
/// Negate.
pub const BPF_NEG: u16 = 0x80;

// ---------------------------------------------------------------------------
// BPF jump operations
// ---------------------------------------------------------------------------

/// Always jump.
pub const BPF_JA: u16 = 0x00;
/// Jump if equal.
pub const BPF_JEQ: u16 = 0x10;
/// Jump if greater than.
pub const BPF_JGT: u16 = 0x20;
/// Jump if greater or equal.
pub const BPF_JGE: u16 = 0x30;
/// Jump if bits set.
pub const BPF_JSET: u16 = 0x40;

// ---------------------------------------------------------------------------
// BPF misc operations
// ---------------------------------------------------------------------------

/// Copy A to X.
pub const BPF_TAX: u16 = 0x00;
/// Copy X to A.
pub const BPF_TXA: u16 = 0x80;

// ---------------------------------------------------------------------------
// Maximum BPF program length
// ---------------------------------------------------------------------------

/// Maximum number of instructions in a classic BPF program.
pub const BPF_MAXINSNS: usize = 4096;

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

/// A single BPF instruction.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SockFilter {
    /// Instruction code.
    pub code: u16,
    /// Jump true offset.
    pub jt: u8,
    /// Jump false offset.
    pub jf: u8,
    /// Generic multiuse field (constant, offset, etc.).
    pub k: u32,
}

/// A BPF program (array of instructions).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockFprog {
    /// Number of filter blocks.
    pub len: u16,
    /// Pointer to array of `SockFilter`.
    pub filter: *mut SockFilter,
}

/// Construct a `SockFilter` BPF statement (no jump offsets).
#[inline]
pub const fn bpf_stmt(code: u16, k: u32) -> SockFilter {
    SockFilter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

/// Construct a `SockFilter` BPF jump instruction.
#[inline]
pub const fn bpf_jump(code: u16, k: u32, jt: u8, jf: u8) -> SockFilter {
    SockFilter { code, jt, jf, k }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sock_filter_size() {
        assert_eq!(core::mem::size_of::<SockFilter>(), 8);
    }

    #[test]
    fn test_sock_fprog_size() {
        // 2 bytes len + padding + pointer (8 bytes on 64-bit).
        assert!(core::mem::size_of::<SockFprog>() >= 10);
    }

    #[test]
    fn test_bpf_stmt() {
        let insn = bpf_stmt(BPF_RET | BPF_K, 0);
        assert_eq!(insn.code, BPF_RET | BPF_K);
        assert_eq!(insn.jt, 0);
        assert_eq!(insn.jf, 0);
        assert_eq!(insn.k, 0);
    }

    #[test]
    fn test_bpf_jump() {
        let insn = bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, 42, 1, 0);
        assert_eq!(insn.code, BPF_JMP | BPF_JEQ | BPF_K);
        assert_eq!(insn.jt, 1);
        assert_eq!(insn.jf, 0);
        assert_eq!(insn.k, 42);
    }

    #[test]
    fn test_instruction_classes_distinct() {
        let classes = [
            BPF_LD, BPF_LDX, BPF_ST, BPF_STX, BPF_ALU, BPF_JMP, BPF_RET, BPF_MISC,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_bpf_classes_are_3_bit() {
        // Classes occupy bits 0-2 (values 0-7).
        assert!(BPF_LD <= 7);
        assert!(BPF_MISC <= 7);
    }

    #[test]
    fn test_bpf_maxinsns() {
        assert_eq!(BPF_MAXINSNS, 4096);
    }

    #[test]
    fn test_seccomp_bpf_filter() {
        // Build a trivial "allow all" seccomp filter.
        let allow_all = bpf_stmt(BPF_RET | BPF_K, 0x7FFF_0000); // SECCOMP_RET_ALLOW
        assert_eq!(allow_all.k, 0x7FFF_0000);
    }
}
