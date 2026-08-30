//! `<linux/bpf_common.h>` — Classic BPF instruction class and mode constants.
//!
//! Classic BPF (cBPF) is used by seccomp filters and older packet
//! filters. Each instruction has a class, size, and mode encoded
//! in the opcode byte. These constants define the encoding scheme.

// ---------------------------------------------------------------------------
// BPF instruction classes (bits 0-2 of opcode)
// ---------------------------------------------------------------------------

/// Load into accumulator.
pub const BPF_LD: u16 = 0x00;
/// Load into index register.
pub const BPF_LDX: u16 = 0x01;
/// Store accumulator.
pub const BPF_ST: u16 = 0x02;
/// Store index register.
pub const BPF_STX: u16 = 0x03;
/// ALU operation.
pub const BPF_ALU: u16 = 0x04;
/// Jump/branch.
pub const BPF_JMP: u16 = 0x05;
/// Return.
pub const BPF_RET: u16 = 0x06;
/// Miscellaneous.
pub const BPF_MISC: u16 = 0x07;

// ---------------------------------------------------------------------------
// BPF load sizes (bits 3-4 of opcode)
// ---------------------------------------------------------------------------

/// Load word (32-bit).
pub const BPF_W: u16 = 0x00;
/// Load half-word (16-bit).
pub const BPF_H: u16 = 0x08;
/// Load byte (8-bit).
pub const BPF_B: u16 = 0x10;

// ---------------------------------------------------------------------------
// BPF addressing modes (bits 5-7 of opcode)
// ---------------------------------------------------------------------------

/// Immediate value.
pub const BPF_IMM: u16 = 0x00;
/// Absolute offset (packet data).
pub const BPF_ABS: u16 = 0x20;
/// Indirect offset (packet data + X).
pub const BPF_IND: u16 = 0x40;
/// Memory (scratch memory store).
pub const BPF_MEM: u16 = 0x60;
/// Length of packet.
pub const BPF_LEN: u16 = 0x80;
/// Extension / misc.
pub const BPF_MSH: u16 = 0xA0;

// ---------------------------------------------------------------------------
// BPF ALU operations (bits 4-7 of opcode, class=ALU)
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
/// Modulo.
pub const BPF_MOD: u16 = 0x90;
/// Bitwise XOR.
pub const BPF_XOR: u16 = 0xA0;

// ---------------------------------------------------------------------------
// BPF source operand (bit 3 of opcode for ALU/JMP)
// ---------------------------------------------------------------------------

/// Source is constant (K).
pub const BPF_K: u16 = 0x00;
/// Source is index register (X).
pub const BPF_X: u16 = 0x08;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classes_distinct() {
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
    fn test_sizes_distinct() {
        let sizes = [BPF_W, BPF_H, BPF_B];
        for i in 0..sizes.len() {
            for j in (i + 1)..sizes.len() {
                assert_ne!(sizes[i], sizes[j]);
            }
        }
    }

    #[test]
    fn test_alu_ops_distinct() {
        let ops = [
            BPF_ADD, BPF_SUB, BPF_MUL, BPF_DIV, BPF_OR, BPF_AND, BPF_LSH, BPF_RSH, BPF_NEG,
            BPF_MOD, BPF_XOR,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_source_operands() {
        assert_eq!(BPF_K, 0x00);
        assert_eq!(BPF_X, 0x08);
        assert_ne!(BPF_K, BPF_X);
    }

    #[test]
    fn test_ld_class_is_zero() {
        assert_eq!(BPF_LD, 0);
    }
}
