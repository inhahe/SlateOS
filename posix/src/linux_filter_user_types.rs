//! `<linux/filter.h>` — classic BPF (cBPF) for socket and seccomp filters.
//!
//! Pre-eBPF BPF is still used in `SO_ATTACH_FILTER` (tcpdump),
//! `seccomp_set_filter()` (Chromium sandbox, systemd-resolved),
//! and `SECCOMP_RET_*` policies. The opcode encoding here is the
//! original 4-tuple {code, jt, jf, k} from the BPF paper.

// ---------------------------------------------------------------------------
// Opcode classes (BPF_CLASS)
// ---------------------------------------------------------------------------

/// Load from packet/aux to A.
pub const BPF_LD: u8 = 0x00;
/// Load to X.
pub const BPF_LDX: u8 = 0x01;
/// Store A.
pub const BPF_ST: u8 = 0x02;
/// Store X.
pub const BPF_STX: u8 = 0x03;
/// ALU on A.
pub const BPF_ALU: u8 = 0x04;
/// Jump.
pub const BPF_JMP: u8 = 0x05;
/// Reserved.
pub const BPF_RET: u8 = 0x06;
/// 64-bit miscellaneous.
pub const BPF_MISC: u8 = 0x07;

// ---------------------------------------------------------------------------
// Sizes (BPF_SIZE)
// ---------------------------------------------------------------------------

/// 32-bit word.
pub const BPF_W: u8 = 0x00;
/// 16-bit halfword.
pub const BPF_H: u8 = 0x08;
/// 8-bit byte.
pub const BPF_B: u8 = 0x10;

// ---------------------------------------------------------------------------
// Modes (BPF_MODE)
// ---------------------------------------------------------------------------

/// Immediate.
pub const BPF_IMM: u8 = 0x00;
/// Absolute offset into packet.
pub const BPF_ABS: u8 = 0x20;
/// Relative offset (X + k).
pub const BPF_IND: u8 = 0x40;
/// Memory.
pub const BPF_MEM: u8 = 0x60;
/// Packet length.
pub const BPF_LEN: u8 = 0x80;
/// Multiplicand source.
pub const BPF_MSH: u8 = 0xa0;

// ---------------------------------------------------------------------------
// ALU operations
// ---------------------------------------------------------------------------

/// add.
pub const BPF_ADD: u8 = 0x00;
/// sub.
pub const BPF_SUB: u8 = 0x10;
/// mul.
pub const BPF_MUL: u8 = 0x20;
/// div.
pub const BPF_DIV: u8 = 0x30;
/// or.
pub const BPF_OR: u8 = 0x40;
/// and.
pub const BPF_AND: u8 = 0x50;
/// lsh.
pub const BPF_LSH: u8 = 0x60;
/// rsh.
pub const BPF_RSH: u8 = 0x70;
/// negate.
pub const BPF_NEG: u8 = 0x80;
/// mod.
pub const BPF_MOD: u8 = 0x90;
/// xor.
pub const BPF_XOR: u8 = 0xa0;

// ---------------------------------------------------------------------------
// Jump operations
// ---------------------------------------------------------------------------

/// jump absolute.
pub const BPF_JA: u8 = 0x00;
/// jump if equal.
pub const BPF_JEQ: u8 = 0x10;
/// jump if greater than.
pub const BPF_JGT: u8 = 0x20;
/// jump if greater or equal.
pub const BPF_JGE: u8 = 0x30;
/// jump if set (bitwise AND).
pub const BPF_JSET: u8 = 0x40;

// ---------------------------------------------------------------------------
// Sources (K=immediate vs X register)
// ---------------------------------------------------------------------------

/// Source is the K immediate.
pub const BPF_K: u8 = 0x00;
/// Source is the X register.
pub const BPF_X: u8 = 0x08;

// ---------------------------------------------------------------------------
// Length and structure
// ---------------------------------------------------------------------------

/// Maximum classic-BPF program length.
pub const BPF_MAXINSNS: u32 = 4096;
/// Each instruction is 8 bytes (code:u16, jt:u8, jf:u8, k:u32).
pub const BPF_INSN_BYTES: usize = 8;

// ---------------------------------------------------------------------------
// Helper for assembling `code` field
// ---------------------------------------------------------------------------

/// `BPF_STMT(code, k)` analog: returns just the code byte combined.
#[must_use]
pub const fn bpf_stmt_code(class: u8, size: u8, mode: u8) -> u8 {
    class | size | mode
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_class_dense_low_3_bits() {
        let c = [
            BPF_LD,
            BPF_LDX,
            BPF_ST,
            BPF_STX,
            BPF_ALU,
            BPF_JMP,
            BPF_RET,
            BPF_MISC,
        ];
        for (i, &x) in c.iter().enumerate() {
            assert_eq!(x as usize, i);
            // The class field occupies bits 0..2 of the code byte.
            assert_eq!(x & 0x07, x);
        }
    }

    #[test]
    fn test_size_bits_distinct() {
        let s = [BPF_W, BPF_H, BPF_B];
        for &b in &s {
            // Size field occupies bits 3..4: values 0,8,16.
            assert!(b & 0x18 == b);
        }
        for i in 0..s.len() {
            for j in (i + 1)..s.len() {
                assert_ne!(s[i], s[j]);
            }
        }
    }

    #[test]
    fn test_mode_bits_distinct() {
        let m = [BPF_IMM, BPF_ABS, BPF_IND, BPF_MEM, BPF_LEN, BPF_MSH];
        for &b in &m {
            // Mode field occupies bits 5..7: multiples of 0x20.
            assert_eq!(b & 0xE0, b);
        }
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
    }

    #[test]
    fn test_alu_ops_distinct() {
        let a = [
            BPF_ADD, BPF_SUB, BPF_MUL, BPF_DIV, BPF_OR, BPF_AND, BPF_LSH,
            BPF_RSH, BPF_NEG, BPF_MOD, BPF_XOR,
        ];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
    }

    #[test]
    fn test_jump_ops_distinct() {
        let j = [BPF_JA, BPF_JEQ, BPF_JGT, BPF_JGE, BPF_JSET];
        for i in 0..j.len() {
            for k in (i + 1)..j.len() {
                assert_ne!(j[i], j[k]);
            }
        }
    }

    #[test]
    fn test_source_bits() {
        // K and X must be the only valid source-field values, distinct.
        assert_eq!(BPF_K, 0x00);
        assert_eq!(BPF_X, 0x08);
        assert_ne!(BPF_K, BPF_X);
    }

    #[test]
    fn test_program_limits() {
        // 4096 is the kernel's hard cap on classic BPF instructions.
        assert_eq!(BPF_MAXINSNS, 4096);
        // sock_filter struct is exactly 8 bytes.
        assert_eq!(BPF_INSN_BYTES, 8);
    }

    #[test]
    fn test_code_helper() {
        // BPF_LD | BPF_W | BPF_ABS = 0x00 | 0x00 | 0x20 = 0x20.
        assert_eq!(bpf_stmt_code(BPF_LD, BPF_W, BPF_ABS), 0x20);
        // BPF_RET | BPF_K = 0x06.
        assert_eq!(bpf_stmt_code(BPF_RET, 0, BPF_K), 0x06);
        // BPF_JMP | BPF_JEQ | BPF_K = 0x05 | 0x10 | 0x00 = 0x15.
        assert_eq!(bpf_stmt_code(BPF_JMP, BPF_JEQ, BPF_K), 0x15);
    }
}
