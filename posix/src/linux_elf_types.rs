//! `<linux/elf.h>` — ELF binary format constants.
//!
//! The Executable and Linkable Format (ELF) is the standard binary
//! format for executables, shared libraries, and object files on
//! Linux. The kernel's ELF loader parses the program headers to map
//! segments into memory, set up the stack, and transfer control to
//! the entry point (or dynamic linker).

// ---------------------------------------------------------------------------
// ELF identification (e_ident[])
// ---------------------------------------------------------------------------

/// ELF magic byte 0.
pub const ELFMAG0: u8 = 0x7F;
/// ELF magic byte 1.
pub const ELFMAG1: u8 = b'E';
/// ELF magic byte 2.
pub const ELFMAG2: u8 = b'L';
/// ELF magic byte 3.
pub const ELFMAG3: u8 = b'F';
/// Size of e_ident array.
pub const EI_NIDENT: usize = 16;
/// Index of class byte.
pub const EI_CLASS: usize = 4;
/// Index of data encoding byte.
pub const EI_DATA: usize = 5;
/// Index of ELF version byte.
pub const EI_VERSION: usize = 6;
/// Index of OS/ABI byte.
pub const EI_OSABI: usize = 7;

// ---------------------------------------------------------------------------
// ELF class (32-bit vs 64-bit)
// ---------------------------------------------------------------------------

/// Invalid class.
pub const ELFCLASSNONE: u8 = 0;
/// 32-bit objects.
pub const ELFCLASS32: u8 = 1;
/// 64-bit objects.
pub const ELFCLASS64: u8 = 2;

// ---------------------------------------------------------------------------
// ELF data encoding (endianness)
// ---------------------------------------------------------------------------

/// Invalid encoding.
pub const ELFDATANONE: u8 = 0;
/// Little-endian.
pub const ELFDATA2LSB: u8 = 1;
/// Big-endian.
pub const ELFDATA2MSB: u8 = 2;

// ---------------------------------------------------------------------------
// ELF object file types (e_type)
// ---------------------------------------------------------------------------

/// No file type.
pub const ET_NONE: u16 = 0;
/// Relocatable file.
pub const ET_REL: u16 = 1;
/// Executable file.
pub const ET_EXEC: u16 = 2;
/// Shared object file.
pub const ET_DYN: u16 = 3;
/// Core dump file.
pub const ET_CORE: u16 = 4;

// ---------------------------------------------------------------------------
// Machine types (e_machine) — common architectures
// ---------------------------------------------------------------------------

/// x86 (32-bit).
pub const EM_386: u16 = 3;
/// ARM (32-bit).
pub const EM_ARM: u16 = 40;
/// x86-64.
pub const EM_X86_64: u16 = 62;
/// ARM (64-bit, AArch64).
pub const EM_AARCH64: u16 = 183;
/// RISC-V.
pub const EM_RISCV: u16 = 243;

// ---------------------------------------------------------------------------
// Program header types (p_type)
// ---------------------------------------------------------------------------

/// Null (ignore this entry).
pub const PT_NULL: u32 = 0;
/// Loadable segment.
pub const PT_LOAD: u32 = 1;
/// Dynamic linking info.
pub const PT_DYNAMIC: u32 = 2;
/// Path to interpreter.
pub const PT_INTERP: u32 = 3;
/// Auxiliary information.
pub const PT_NOTE: u32 = 4;
/// Reserved (unused).
pub const PT_SHLIB: u32 = 5;
/// Program header table itself.
pub const PT_PHDR: u32 = 6;
/// Thread-local storage.
pub const PT_TLS: u32 = 7;
/// GNU stack permissions.
pub const PT_GNU_STACK: u32 = 0x6474_E551;
/// GNU relro (read-only after relocation).
pub const PT_GNU_RELRO: u32 = 0x6474_E552;

// ---------------------------------------------------------------------------
// Program header flags (p_flags)
// ---------------------------------------------------------------------------

/// Segment is executable.
pub const PF_X: u32 = 1 << 0;
/// Segment is writable.
pub const PF_W: u32 = 1 << 1;
/// Segment is readable.
pub const PF_R: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Section header types (sh_type)
// ---------------------------------------------------------------------------

/// Inactive section.
pub const SHT_NULL: u32 = 0;
/// Program data.
pub const SHT_PROGBITS: u32 = 1;
/// Symbol table.
pub const SHT_SYMTAB: u32 = 2;
/// String table.
pub const SHT_STRTAB: u32 = 3;
/// Relocation entries with addends.
pub const SHT_RELA: u32 = 4;
/// Symbol hash table.
pub const SHT_HASH: u32 = 5;
/// Dynamic linking info.
pub const SHT_DYNAMIC: u32 = 6;
/// Notes.
pub const SHT_NOTE: u32 = 7;
/// BSS (no file space).
pub const SHT_NOBITS: u32 = 8;
/// Relocation entries (no addends).
pub const SHT_REL: u32 = 9;
/// Dynamic symbol table.
pub const SHT_DYNSYM: u32 = 11;

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
    fn test_classes_distinct() {
        let classes = [ELFCLASSNONE, ELFCLASS32, ELFCLASS64];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
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
        let machines = [EM_386, EM_ARM, EM_X86_64, EM_AARCH64, EM_RISCV];
        for i in 0..machines.len() {
            for j in (i + 1)..machines.len() {
                assert_ne!(machines[i], machines[j]);
            }
        }
    }

    #[test]
    fn test_phdr_types_distinct() {
        let types = [
            PT_NULL,
            PT_LOAD,
            PT_DYNAMIC,
            PT_INTERP,
            PT_NOTE,
            PT_SHLIB,
            PT_PHDR,
            PT_TLS,
            PT_GNU_STACK,
            PT_GNU_RELRO,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_phdr_flags_no_overlap() {
        assert_eq!(PF_X & PF_W, 0);
        assert_eq!(PF_X & PF_R, 0);
        assert_eq!(PF_W & PF_R, 0);
    }

    #[test]
    fn test_section_types_distinct() {
        let types = [
            SHT_NULL,
            SHT_PROGBITS,
            SHT_SYMTAB,
            SHT_STRTAB,
            SHT_RELA,
            SHT_HASH,
            SHT_DYNAMIC,
            SHT_NOTE,
            SHT_NOBITS,
            SHT_REL,
            SHT_DYNSYM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
