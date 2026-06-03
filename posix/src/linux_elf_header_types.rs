//! `<elf.h>` — ELF file header constants.
//!
//! The ELF (Executable and Linkable Format) header is at the start
//! of every ELF file. It identifies the file class (32/64-bit),
//! data encoding (endianness), OS/ABI, and object file type
//! (executable, shared library, relocatable, core dump).

// ---------------------------------------------------------------------------
// ELF identification (e_ident[])
// ---------------------------------------------------------------------------

/// ELF magic byte 0.
pub const ELFMAG0: u8 = 0x7F;
/// ELF magic byte 1 ('E').
pub const ELFMAG1: u8 = b'E';
/// ELF magic byte 2 ('L').
pub const ELFMAG2: u8 = b'L';
/// ELF magic byte 3 ('F').
pub const ELFMAG3: u8 = b'F';

/// Class: 32-bit objects.
pub const ELFCLASS32: u8 = 1;
/// Class: 64-bit objects.
pub const ELFCLASS64: u8 = 2;

/// Data encoding: little-endian.
pub const ELFDATA2LSB: u8 = 1;
/// Data encoding: big-endian.
pub const ELFDATA2MSB: u8 = 2;

/// OS/ABI: System V (generic).
pub const ELFOSABI_NONE: u8 = 0;
/// OS/ABI: Linux-specific.
pub const ELFOSABI_LINUX: u8 = 3;
/// OS/ABI: FreeBSD.
pub const ELFOSABI_FREEBSD: u8 = 9;

// ---------------------------------------------------------------------------
// ELF file types (e_type)
// ---------------------------------------------------------------------------

/// No file type.
pub const ET_NONE: u16 = 0;
/// Relocatable file (.o).
pub const ET_REL: u16 = 1;
/// Executable file.
pub const ET_EXEC: u16 = 2;
/// Shared object file (.so).
pub const ET_DYN: u16 = 3;
/// Core dump file.
pub const ET_CORE: u16 = 4;

// ---------------------------------------------------------------------------
// ELF machine types (e_machine)
// ---------------------------------------------------------------------------

/// x86 (i386).
pub const EM_386: u16 = 3;
/// ARM (32-bit).
pub const EM_ARM: u16 = 40;
/// x86-64 (AMD64).
pub const EM_X86_64: u16 = 62;
/// AArch64 (ARM 64-bit).
pub const EM_AARCH64: u16 = 183;
/// RISC-V.
pub const EM_RISCV: u16 = 243;
/// MIPS.
pub const EM_MIPS: u16 = 8;
/// PowerPC 64-bit.
pub const EM_PPC64: u16 = 21;

// ---------------------------------------------------------------------------
// ELF version
// ---------------------------------------------------------------------------

/// Current ELF version.
pub const EV_CURRENT: u8 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_bytes() {
        assert_eq!(ELFMAG0, 0x7F);
        assert_eq!(ELFMAG1, b'E');
        assert_eq!(ELFMAG2, b'L');
        assert_eq!(ELFMAG3, b'F');
    }

    #[test]
    fn test_file_types_distinct() {
        let types = [ET_NONE, ET_REL, ET_EXEC, ET_DYN, ET_CORE];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_machine_types_distinct() {
        let machines = [
            EM_386, EM_ARM, EM_X86_64, EM_AARCH64, EM_RISCV, EM_MIPS, EM_PPC64,
        ];
        for i in 0..machines.len() {
            for j in (i + 1)..machines.len() {
                assert_ne!(machines[i], machines[j]);
            }
        }
    }

    #[test]
    fn test_class_distinct() {
        assert_ne!(ELFCLASS32, ELFCLASS64);
    }

    #[test]
    fn test_data_encoding_distinct() {
        assert_ne!(ELFDATA2LSB, ELFDATA2MSB);
    }
}
