//! `<linux/bpf.h>` — BPF instruction encoding (class, source, op bits).
//!
//! Each BPF instruction is a packed 64-bit word:
//!   `[opcode:8 | dst:4 | src:4 | offset:16 | imm:32]`
//! Within the 8-bit opcode, the low 3 bits select an *instruction class*
//! (ALU, JMP, LD, …), the next bits select an arithmetic op, and the
//! source bit picks register-vs-immediate operands. `bpftool` and
//! libbpf encode/decode using exactly these constants.

// ---------------------------------------------------------------------------
// Instruction classes (low 3 opcode bits, `BPF_CLASS()`)
// ---------------------------------------------------------------------------

pub const BPF_LD: u8 = 0x00;
pub const BPF_LDX: u8 = 0x01;
pub const BPF_ST: u8 = 0x02;
pub const BPF_STX: u8 = 0x03;
pub const BPF_ALU: u8 = 0x04;
pub const BPF_JMP: u8 = 0x05;
pub const BPF_JMP32: u8 = 0x06;
pub const BPF_ALU64: u8 = 0x07;

/// Mask isolating the class bits.
pub const BPF_CLASS_MASK: u8 = 0x07;

// ---------------------------------------------------------------------------
// Source-operand bit (`BPF_SRC()`)
// ---------------------------------------------------------------------------

/// Operand-2 is an immediate constant in `imm`.
pub const BPF_K: u8 = 0x00;
/// Operand-2 is the register named in `src`.
pub const BPF_X: u8 = 0x08;

// ---------------------------------------------------------------------------
// ALU / JMP op codes (high 4 bits, `BPF_OP()`)
// ---------------------------------------------------------------------------

pub const BPF_ADD: u8 = 0x00;
pub const BPF_SUB: u8 = 0x10;
pub const BPF_MUL: u8 = 0x20;
pub const BPF_DIV: u8 = 0x30;
pub const BPF_OR: u8 = 0x40;
pub const BPF_AND: u8 = 0x50;
pub const BPF_LSH: u8 = 0x60;
pub const BPF_RSH: u8 = 0x70;
pub const BPF_NEG: u8 = 0x80;
pub const BPF_MOD: u8 = 0x90;
pub const BPF_XOR: u8 = 0xa0;
pub const BPF_MOV: u8 = 0xb0;
pub const BPF_ARSH: u8 = 0xc0;
pub const BPF_END: u8 = 0xd0;

pub const BPF_JA: u8 = 0x00;
pub const BPF_JEQ: u8 = 0x10;
pub const BPF_JGT: u8 = 0x20;
pub const BPF_JGE: u8 = 0x30;
pub const BPF_JSET: u8 = 0x40;
pub const BPF_JNE: u8 = 0x50;
pub const BPF_JSGT: u8 = 0x60;
pub const BPF_JSGE: u8 = 0x70;
pub const BPF_CALL: u8 = 0x80;
pub const BPF_EXIT: u8 = 0x90;
pub const BPF_JLT: u8 = 0xa0;
pub const BPF_JLE: u8 = 0xb0;
pub const BPF_JSLT: u8 = 0xc0;
pub const BPF_JSLE: u8 = 0xd0;

// ---------------------------------------------------------------------------
// Memory-size encoding (bits 3..4, used by LD/LDX/ST/STX)
// ---------------------------------------------------------------------------

pub const BPF_W: u8 = 0x00;
pub const BPF_H: u8 = 0x08;
pub const BPF_B: u8 = 0x10;
pub const BPF_DW: u8 = 0x18;

/// Mask isolating the memory-size bits.
pub const BPF_SIZE_MASK: u8 = 0x18;

// ---------------------------------------------------------------------------
// Instruction word size (bytes)
// ---------------------------------------------------------------------------

pub const BPF_INSN_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_class_codes_dense_0_to_7() {
        let c = [
            BPF_LD, BPF_LDX, BPF_ST, BPF_STX, BPF_ALU, BPF_JMP, BPF_JMP32,
            BPF_ALU64,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // Class mask isolates low 3 bits — 8 classes fit exactly.
        assert_eq!(BPF_CLASS_MASK, 0x07);
    }

    #[test]
    fn test_src_bit_is_bit_3() {
        assert_eq!(BPF_K, 0x00);
        assert_eq!(BPF_X, 0x08);
        assert_eq!(BPF_X, 1 << 3);
        // K and X are mutually exclusive.
        assert_eq!(BPF_K & BPF_X, 0);
    }

    #[test]
    fn test_alu_ops_high_nibble_dense_0_to_d() {
        let a = [
            BPF_ADD, BPF_SUB, BPF_MUL, BPF_DIV, BPF_OR, BPF_AND, BPF_LSH,
            BPF_RSH, BPF_NEG, BPF_MOD, BPF_XOR, BPF_MOV, BPF_ARSH, BPF_END,
        ];
        for (i, &v) in a.iter().enumerate() {
            // Op codes occupy the high nibble, stepping by 0x10.
            assert_eq!(v as usize, i * 0x10);
        }
    }

    #[test]
    fn test_jmp_ops_have_paired_signed_unsigned() {
        // Unsigned-vs-signed jump pairs share a base op.
        assert_eq!(BPF_JGT, 0x20);
        assert_eq!(BPF_JSGT, 0x60);
        assert_eq!(BPF_JLT, 0xa0);
        assert_eq!(BPF_JSLT, 0xc0);
    }

    #[test]
    fn test_memory_sizes_dense_powers_of_two() {
        // W=32, H=16, B=8, DW=64 — encoded in bits 3..4 of opcode.
        let m = [BPF_W, BPF_H, BPF_B, BPF_DW];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i * 0x08);
        }
        assert_eq!(BPF_SIZE_MASK, 0x18);
        // SIZE_MASK covers bits 3..4 (0b11000).
        assert_eq!(BPF_SIZE_MASK, 0b0001_1000);
    }

    #[test]
    fn test_insn_size() {
        // Each BPF instruction is a packed 8-byte word.
        assert_eq!(BPF_INSN_SIZE, 8);
        assert!(BPF_INSN_SIZE.is_power_of_two());
    }

    #[test]
    fn test_call_and_exit_at_known_codes() {
        // BPF_CALL=0x80, BPF_EXIT=0x90 — used by every loadable program.
        assert_eq!(BPF_CALL, 0x80);
        assert_eq!(BPF_EXIT, 0x90);
    }
}
