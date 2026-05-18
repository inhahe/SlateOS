//! `<elf.h>` — ELF relocation type constants (x86_64).
//!
//! Relocations patch code/data addresses at link time or load time.
//! Each relocation type specifies how to compute the final value
//! from the symbol address, addend, and/or GOT/PLT entries. These
//! are the x86_64-specific relocation types.

// ---------------------------------------------------------------------------
// x86_64 relocation types (r_type field)
// ---------------------------------------------------------------------------

/// No relocation.
pub const R_X86_64_NONE: u32 = 0;
/// Direct 64-bit address (S + A).
pub const R_X86_64_64: u32 = 1;
/// PC-relative 32-bit (S + A - P).
pub const R_X86_64_PC32: u32 = 2;
/// GOT entry 32-bit (G + A).
pub const R_X86_64_GOT32: u32 = 3;
/// PLT entry 32-bit (L + A - P).
pub const R_X86_64_PLT32: u32 = 4;
/// Copy relocation (for data imports).
pub const R_X86_64_COPY: u32 = 5;
/// Create GOT entry (S).
pub const R_X86_64_GLOB_DAT: u32 = 6;
/// Create PLT entry (S).
pub const R_X86_64_JUMP_SLOT: u32 = 7;
/// Relative address (B + A).
pub const R_X86_64_RELATIVE: u32 = 8;
/// GOT-relative PC 32-bit (G + GOT + A - P).
pub const R_X86_64_GOTPCREL: u32 = 9;
/// Direct 32-bit zero-extended (S + A).
pub const R_X86_64_32: u32 = 10;
/// Direct 32-bit sign-extended (S + A).
pub const R_X86_64_32S: u32 = 11;
/// Direct 16-bit zero-extended.
pub const R_X86_64_16: u32 = 12;
/// PC-relative 16-bit.
pub const R_X86_64_PC16: u32 = 13;
/// Direct 8-bit.
pub const R_X86_64_8: u32 = 14;
/// PC-relative 8-bit.
pub const R_X86_64_PC8: u32 = 15;
/// TLS GD GOT-relative (General Dynamic).
pub const R_X86_64_TLSGD: u32 = 19;
/// TLS LD GOT-relative (Local Dynamic).
pub const R_X86_64_TLSLD: u32 = 20;
/// TLS offset within TLS block.
pub const R_X86_64_TPOFF32: u32 = 23;
/// Initial-exec TLS GOT entry.
pub const R_X86_64_GOTTPOFF: u32 = 22;
/// TP-relative offset (Local-exec).
pub const R_X86_64_TPOFF64: u32 = 18;
/// IRELATIVE (indirect function resolver).
pub const R_X86_64_IRELATIVE: u32 = 37;
/// GOTPCRELX (relaxable GOT-relative PC32).
pub const R_X86_64_GOTPCRELX: u32 = 41;
/// REX_GOTPCRELX (relaxable with REX prefix).
pub const R_X86_64_REX_GOTPCRELX: u32 = 42;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reloc_types_distinct() {
        let types = [
            R_X86_64_NONE, R_X86_64_64, R_X86_64_PC32,
            R_X86_64_GOT32, R_X86_64_PLT32, R_X86_64_COPY,
            R_X86_64_GLOB_DAT, R_X86_64_JUMP_SLOT, R_X86_64_RELATIVE,
            R_X86_64_GOTPCREL, R_X86_64_32, R_X86_64_32S,
            R_X86_64_16, R_X86_64_PC16, R_X86_64_8, R_X86_64_PC8,
            R_X86_64_TLSGD, R_X86_64_TLSLD, R_X86_64_TPOFF32,
            R_X86_64_GOTTPOFF, R_X86_64_TPOFF64,
            R_X86_64_IRELATIVE, R_X86_64_GOTPCRELX,
            R_X86_64_REX_GOTPCRELX,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_none_is_zero() {
        assert_eq!(R_X86_64_NONE, 0);
    }

    #[test]
    fn test_common_types() {
        assert_eq!(R_X86_64_64, 1);
        assert_eq!(R_X86_64_PC32, 2);
        assert_eq!(R_X86_64_PLT32, 4);
    }
}
