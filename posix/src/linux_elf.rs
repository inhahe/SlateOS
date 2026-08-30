//! `<linux/elf.h>` — ELF binary format constants.
//!
//! The Executable and Linkable Format (ELF) is the standard binary
//! format for executables, shared libraries, and core dumps on Linux.

// ---------------------------------------------------------------------------
// ELF magic bytes
// ---------------------------------------------------------------------------

/// ELF magic byte 0.
pub const ELFMAG0: u8 = 0x7F;
/// ELF magic byte 1.
pub const ELFMAG1: u8 = b'E';
/// ELF magic byte 2.
pub const ELFMAG2: u8 = b'L';
/// ELF magic byte 3.
pub const ELFMAG3: u8 = b'F';

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

/// Invalid data encoding.
pub const ELFDATANONE: u8 = 0;
/// Little-endian.
pub const ELFDATA2LSB: u8 = 1;
/// Big-endian.
pub const ELFDATA2MSB: u8 = 2;

// ---------------------------------------------------------------------------
// ELF file types
// ---------------------------------------------------------------------------

/// No file type.
pub const ET_NONE: u16 = 0;
/// Relocatable file.
pub const ET_REL: u16 = 1;
/// Executable file.
pub const ET_EXEC: u16 = 2;
/// Shared object file.
pub const ET_DYN: u16 = 3;
/// Core file.
pub const ET_CORE: u16 = 4;

// ---------------------------------------------------------------------------
// ELF machine types
// ---------------------------------------------------------------------------

/// No machine.
pub const EM_NONE: u16 = 0;
/// Intel 80386.
pub const EM_386: u16 = 3;
/// ARM.
pub const EM_ARM: u16 = 40;
/// AMD x86-64.
pub const EM_X86_64: u16 = 62;
/// ARM AARCH64.
pub const EM_AARCH64: u16 = 183;
/// RISC-V.
pub const EM_RISCV: u16 = 243;

// ---------------------------------------------------------------------------
// ELF program header types
// ---------------------------------------------------------------------------

/// Null entry.
pub const PT_NULL: u32 = 0;
/// Loadable segment.
pub const PT_LOAD: u32 = 1;
/// Dynamic linking info.
pub const PT_DYNAMIC: u32 = 2;
/// Program interpreter path.
pub const PT_INTERP: u32 = 3;
/// Auxiliary information.
pub const PT_NOTE: u32 = 4;
/// Reserved.
pub const PT_SHLIB: u32 = 5;
/// Program header table.
pub const PT_PHDR: u32 = 6;
/// Thread-local storage.
pub const PT_TLS: u32 = 7;
/// GNU EH frame.
pub const PT_GNU_EH_FRAME: u32 = 0x6474E550;
/// GNU stack permissions.
pub const PT_GNU_STACK: u32 = 0x6474E551;
/// GNU read-only after relocation.
pub const PT_GNU_RELRO: u32 = 0x6474E552;

// ---------------------------------------------------------------------------
// Program header flags
// ---------------------------------------------------------------------------

/// Execute permission.
pub const PF_X: u32 = 1;
/// Write permission.
pub const PF_W: u32 = 2;
/// Read permission.
pub const PF_R: u32 = 4;

// ---------------------------------------------------------------------------
// Section header types
// ---------------------------------------------------------------------------

/// Null section.
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
/// Dynamic linking information.
pub const SHT_DYNAMIC: u32 = 6;
/// Notes.
pub const SHT_NOTE: u32 = 7;
/// BSS (uninitialized data).
pub const SHT_NOBITS: u32 = 8;
/// Relocation entries.
pub const SHT_REL: u32 = 9;
/// Dynamic symbol table.
pub const SHT_DYNSYM: u32 = 11;

// ---------------------------------------------------------------------------
// Dynamic section tags
// ---------------------------------------------------------------------------

/// End of dynamic section.
pub const DT_NULL: i64 = 0;
/// Name of needed library.
pub const DT_NEEDED: i64 = 1;
/// PLT relocation table size.
pub const DT_PLTRELSZ: i64 = 2;
/// Address of PLT/GOT.
pub const DT_PLTGOT: i64 = 3;
/// Address of symbol hash table.
pub const DT_HASH: i64 = 4;
/// Address of string table.
pub const DT_STRTAB: i64 = 5;
/// Address of symbol table.
pub const DT_SYMTAB: i64 = 6;
/// Address of rela relocations.
pub const DT_RELA: i64 = 7;
/// Size of rela relocations.
pub const DT_RELASZ: i64 = 8;
/// Runtime library path.
pub const DT_RUNPATH: i64 = 29;

// ---------------------------------------------------------------------------
// ELF header size indices (offsets into e_ident)
// ---------------------------------------------------------------------------

/// e_ident index: magic byte 0.
pub const EI_MAG0: usize = 0;
/// e_ident index: class (32/64-bit).
pub const EI_CLASS: usize = 4;
/// e_ident index: data encoding.
pub const EI_DATA: usize = 5;
/// e_ident index: ELF version.
pub const EI_VERSION: usize = 6;
/// e_ident index: OS/ABI.
pub const EI_OSABI: usize = 7;
/// e_ident size.
pub const EI_NIDENT: usize = 16;

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
    fn test_file_types_sequential() {
        assert_eq!(ET_NONE, 0);
        assert_eq!(ET_REL, 1);
        assert_eq!(ET_EXEC, 2);
        assert_eq!(ET_DYN, 3);
        assert_eq!(ET_CORE, 4);
    }

    #[test]
    fn test_machine_types_distinct() {
        let types = [EM_NONE, EM_386, EM_ARM, EM_X86_64, EM_AARCH64, EM_RISCV];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_phdr_types_distinct() {
        let types = [
            PT_NULL, PT_LOAD, PT_DYNAMIC, PT_INTERP, PT_NOTE, PT_SHLIB, PT_PHDR, PT_TLS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_pf_flags() {
        assert_eq!(PF_X, 1);
        assert_eq!(PF_W, 2);
        assert_eq!(PF_R, 4);
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

    #[test]
    fn test_elf_class() {
        assert_eq!(ELFCLASS32, 1);
        assert_eq!(ELFCLASS64, 2);
    }
}
